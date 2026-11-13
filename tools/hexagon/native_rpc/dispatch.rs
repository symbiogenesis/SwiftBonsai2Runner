//! Immutable placement data and single-flight buffer reuse policy.
//!
//! Identity records bind a caller's already verified observations. Constructing
//! these records authenticates no package, catalog, driver or model by itself.
#![allow(
    clippy::redundant_pub_crate,
    reason = "Queue ownership internals are shared only with the audited native sibling"
)]
#![cfg_attr(
    not(all(windows, target_arch = "aarch64")),
    allow(
        dead_code,
        reason = "Submission tracking is consumed by native ARM64 dispatch and model-free controls"
    )
)]

use crate::{Error, Q8Matmul};

/// Exact observed files and source used when constructing a hybrid plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DispatchIdentity {
    /// SHA-256 of the complete verified model, including tensor metadata.
    pub model: [u8; 32],
    /// Exact immutable model file length, not an estimated memory requirement.
    pub model_bytes: u64,
    /// SHA-256 of the selected signed DSP module.
    pub module: [u8; 32],
    /// SHA-256 of its independently verified catalog.
    pub catalog: [u8; 32],
    /// SHA-256 of the service-derived, independently verified driver DLL.
    pub driver: [u8; 32],
    /// Exact Runner source revision controlling the dispatcher.
    pub source: [u8; 20],
}

/// The one fixed owner of complete mutable KV/session state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheOwner {
    /// Rust CPU owns all mutable model state; projections return only values.
    Cpu,
    /// Reserved for an independently qualified complete HTP0 state provider.
    /// Current plan construction rejects this variant.
    Htp0,
}

/// One complete, immutable source tensor placement; IDs are model catalog IDs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TensorPlacement {
    /// Tensor execution stays in Rust on CPU for the entire session.
    Cpu {
        /// Stable model tensor catalog ID.
        tensor: u16,
        /// Absolute offset in the verified model file.
        offset: u64,
        /// Complete checked tensor extent.
        bytes: u64,
    },
    /// The supported `Q8_0` projection subset is assigned to HTP0 explicitly.
    /// This is planning data, not permission to execute it.
    Htp0Q8 {
        /// Stable model tensor catalog ID.
        tensor: u16,
        /// Absolute, 32-byte-aligned offset in the verified model file.
        offset: u64,
        /// Complete shape and solver mapping; no later solver fallback.
        projection: Q8Matmul,
    },
}

impl TensorPlacement {
    /// Stable tensor catalog ID, shared across CPU and HTP0 routes.
    #[must_use]
    pub const fn tensor(self) -> u16 {
        match self {
            Self::Cpu { tensor, .. } | Self::Htp0Q8 { tensor, .. } => tensor,
        }
    }

    fn extent(self) -> Result<(u64, u64), Error> {
        match self {
            Self::Cpu { offset, bytes, .. } => Ok((offset, bytes)),
            Self::Htp0Q8 {
                offset, projection, ..
            } => {
                if !offset.is_multiple_of(32) {
                    return Err(Error::Contract);
                }
                Ok((
                    offset,
                    u64::try_from(projection.weight_bytes()).map_err(|_| Error::Contract)?,
                ))
            }
        }
    }
}

/// Immutable placements and one cache owner for an exact source/model/session.
///
/// This record has no mutation/fallback API. Replacing a model, module, driver,
/// shape or placement requires a new plan and a newly admitted contained owner.
/// It opens no native objects and does not establish signed execution support.
#[derive(Debug)]
pub struct HybridPlan {
    identity: DispatchIdentity,
    placements: Box<[TensorPlacement]>,
    cache_owner: CacheOwner,
}

impl HybridPlan {
    /// Bind nonoverlapping tensors to exact file identities and one state owner.
    ///
    /// Input order is the immutable model catalog order, with strictly increasing
    /// IDs. Source extents may be in any order, but may not overlap or escape the
    /// model file. Aliased/view tensors need a separately supported contract.
    ///
    /// # Errors
    /// Rejects unknown/zero identities, empty/excessive catalogs, duplicate IDs,
    /// invalid file ranges, and HTP0 cache ownership until complete state support
    /// has its own independent implementation and qualification.
    pub fn new(
        identity: DispatchIdentity,
        placements: &[TensorPlacement],
        cache_owner: CacheOwner,
    ) -> Result<Self, Error> {
        if identity.model_bytes == 0
            || identity.source == [0; 20]
            || [
                identity.model,
                identity.module,
                identity.catalog,
                identity.driver,
            ]
            .contains(&[0; 32])
            || placements.is_empty()
            || placements.len() > 65_534
            || cache_owner != CacheOwner::Cpu
            || placements
                .windows(2)
                .any(|pair| pair[0].tensor() >= pair[1].tensor())
        {
            return Err(Error::Contract);
        }
        let mut ranges = Vec::new();
        ranges
            .try_reserve_exact(placements.len())
            .map_err(|_| Error::Contract)?;
        for &placement in placements {
            let (start, bytes) = placement.extent()?;
            let end = start.checked_add(bytes).ok_or(Error::Contract)?;
            if bytes == 0 || end > identity.model_bytes {
                return Err(Error::Contract);
            }
            ranges.push((start, end));
        }
        ranges.sort_unstable();
        if ranges.windows(2).any(|pair| pair[0].1 > pair[1].0) {
            return Err(Error::Contract);
        }
        let mut owned = Vec::new();
        owned
            .try_reserve_exact(placements.len())
            .map_err(|_| Error::Contract)?;
        owned.extend_from_slice(placements);
        Ok(Self {
            identity,
            placements: owned.into_boxed_slice(),
            cache_owner,
        })
    }

    /// Exact immutable source, model and independently observed native identities.
    #[must_use]
    pub const fn identity(&self) -> &DispatchIdentity {
        &self.identity
    }

    /// Ordered placements cannot be changed after plan construction.
    #[must_use]
    pub const fn placements(&self) -> &[TensorPlacement] {
        &self.placements
    }

    /// The single owner of every mutable model cache in this plan.
    #[must_use]
    pub const fn cache_owner(&self) -> CacheOwner {
        self.cache_owner
    }
}

/// Queue buffer identities are independent of tensor contents and wire counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BufferIdentity {
    pub(crate) fd: u32,
    pub(crate) bytes: u32,
    pub(crate) offset: u32,
    pub(crate) flags: u32,
    pub(crate) address: u64,
}

/// One mapped buffer can have at most one attempted, uncompleted consumer.
/// All uncertainty is terminal; counters cannot replenish the original budget.
pub(crate) struct SubmissionTracker {
    buffer: BufferIdentity,
    maximum: u32,
    attempted: u32,
    pending: Option<u64>,
    poisoned: bool,
}

impl SubmissionTracker {
    pub(crate) const fn new(buffer: BufferIdentity, maximum: u32) -> Self {
        Self {
            buffer,
            maximum,
            attempted: 0,
            pending: None,
            poisoned: false,
        }
    }

    pub(crate) const fn reusable(&self) -> bool {
        !self.poisoned && self.pending.is_none() && self.attempted < self.maximum
    }

    /// Call before entering the native write, including any progress callback.
    pub(crate) fn begin(&mut self) -> Result<u64, Error> {
        if !self.reusable() {
            self.poison();
            return Err(Error::Contract);
        }
        self.attempted += 1;
        let sequence = u64::from(self.attempted);
        self.pending = Some(sequence);
        Ok(sequence)
    }

    /// Both native correlation/cache checks and numeric checks precede this.
    pub(crate) fn complete(&mut self, sequence: u64, buffer: BufferIdentity) -> Result<(), Error> {
        if self.poisoned || self.pending != Some(sequence) || buffer != self.buffer {
            self.poison();
            return Err(Error::Contract);
        }
        self.pending = None;
        Ok(())
    }

    pub(crate) const fn poison(&mut self) {
        self.poisoned = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> DispatchIdentity {
        DispatchIdentity {
            model: [1; 32],
            model_bytes: 10_000,
            module: [2; 32],
            catalog: [3; 32],
            driver: [4; 32],
            source: [5; 20],
        }
    }

    #[test]
    fn hybrid_plan_freezes_placement_and_rejects_aliases_and_unqualified_state() -> Result<(), Error>
    {
        let q8 = Q8Matmul::new(128, 32, 2, 100_000)?;
        let placements = [
            TensorPlacement::Cpu {
                tensor: 1,
                offset: 0,
                bytes: 32,
            },
            TensorPlacement::Htp0Q8 {
                tensor: 2,
                offset: 32,
                projection: q8,
            },
        ];
        let plan = HybridPlan::new(identity(), &placements, CacheOwner::Cpu)?;
        assert_eq!(plan.placements(), placements);
        assert_eq!(plan.identity(), &identity());
        assert_eq!(plan.cache_owner(), CacheOwner::Cpu);
        assert!(HybridPlan::new(identity(), &placements, CacheOwner::Htp0).is_err());
        for bad in [
            TensorPlacement::Cpu {
                tensor: 2,
                offset: 31,
                bytes: 4,
            },
            TensorPlacement::Cpu {
                tensor: 1,
                offset: 64,
                bytes: 4,
            },
            TensorPlacement::Cpu {
                tensor: 2,
                offset: 9999,
                bytes: 2,
            },
            TensorPlacement::Cpu {
                tensor: 2,
                offset: u64::MAX,
                bytes: 2,
            },
            TensorPlacement::Cpu {
                tensor: 2,
                offset: 64,
                bytes: 0,
            },
            TensorPlacement::Htp0Q8 {
                tensor: 2,
                offset: 33,
                projection: q8,
            },
        ] {
            assert!(HybridPlan::new(identity(), &[placements[0], bad], CacheOwner::Cpu).is_err());
        }
        Ok(())
    }

    fn buffer() -> BufferIdentity {
        BufferIdentity {
            fd: 7,
            bytes: 4096,
            offset: 0,
            flags: 0x90,
            address: 4096,
        }
    }

    #[test]
    fn single_flight_requires_correlated_complete_before_reuse() -> Result<(), Error> {
        let mut tracker = SubmissionTracker::new(buffer(), 2);
        assert_eq!(tracker.begin()?, 1);
        assert!(!tracker.reusable());
        tracker.complete(1, buffer())?;
        assert_eq!(tracker.begin()?, 2);
        tracker.complete(2, buffer())?;
        assert!(!tracker.reusable());
        assert!(tracker.begin().is_err());
        Ok(())
    }

    #[test]
    fn failed_write_cancel_stale_reply_and_changed_mapping_remain_terminal() -> Result<(), Error> {
        for case in 0..8 {
            let mut tracker = SubmissionTracker::new(buffer(), 2);
            tracker.begin()?;
            let mut returned = buffer();
            match case {
                0 => tracker.poison(), // refused or uncertain write/cancellation
                1 => {
                    assert!(tracker.complete(0, returned).is_err());
                }
                2 => {
                    assert!(tracker.begin().is_err());
                }
                3 => returned.fd += 1,
                4 => returned.bytes += 1,
                5 => returned.offset += 1,
                6 => returned.flags ^= 0x10,
                _ => returned.address += 4096,
            }
            if case >= 3 {
                assert!(tracker.complete(1, returned).is_err());
            }
            assert!(!tracker.reusable());
            assert!(tracker.complete(1, buffer()).is_err());
            assert!(tracker.begin().is_err());
        }
        Ok(())
    }
}
