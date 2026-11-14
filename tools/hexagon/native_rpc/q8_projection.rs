//! Explicit source-selection and receipt contract for terminal native Q8 work.
#![allow(
    clippy::redundant_pub_crate,
    reason = "Selection internals are shared only with the audited native sibling"
)]
#![cfg_attr(
    not(all(windows, target_arch = "aarch64")),
    allow(
        dead_code,
        reason = "Native selection is consumed only on Windows ARM64"
    )
)]

use crate::{
    DispatchIdentity, Error, HybridPlan, Q8Dispatcher, Q8Matmul, QueueComputeReport,
    SessionPreparationReport, compute,
};
use std::sync::{Arc, atomic::AtomicBool};
use std::time::Instant;

/// One selected immutable Q8 tensor and the original absolute session bounds.
///
/// Construction opens no driver and authenticates no identity. The caller must
/// independently admit guarded source/model/package/catalog/driver files, loaded
/// DSP identity, Windows ABI and a contained child before native execution.
pub struct Q8Projection<'a> {
    pub(crate) plan: &'a HybridPlan,
    pub(crate) tensor: u16,
    pub(crate) projection: Q8Matmul,
    pub(crate) operations: u32,
    pub(crate) deadline: Instant,
    pub(crate) cancellation: Arc<AtomicBool>,
}

impl<'a> Q8Projection<'a> {
    /// Select one HTP0 tensor before native setup or memory allocation.
    ///
    /// At most 24,000 operations and 120 remaining seconds are admitted, with
    /// three seconds reserved within this same inherited deadline for cleanup.
    /// Reusing this value cannot replenish either bound; the native API consumes
    /// it and rechecks cancellation/expiry before module admission, subsequent
    /// queue setup stages and each submit. Synchronous admission/calls still
    /// require the supervisor's external deadline.
    ///
    /// # Errors
    /// Rejects CPU/unselected tensors, empty/excessive work, cancellation and
    /// expired/excessive remaining time before any native authority is exercised.
    pub fn new(
        plan: &'a HybridPlan,
        tensor: u16,
        operations: u32,
        deadline: Instant,
        cancellation: Arc<AtomicBool>,
    ) -> Result<Self, Error> {
        let projection =
            Q8Dispatcher::validate_selection(plan, tensor, operations, deadline, &cancellation)?;
        Ok(Self {
            plan,
            tensor,
            projection,
            operations,
            deadline,
            cancellation,
        })
    }

    /// Exact selected matrix and immutable solver mapping.
    #[must_use]
    pub const fn projection(&self) -> Q8Matmul {
        self.projection
    }

    /// The original absolute deadline, including the cleanup reserve.
    #[must_use]
    pub const fn deadline(&self) -> Instant {
        self.deadline
    }
}

/// Synchronous Q8 submission with mandatory independent numerical review.
pub trait Q8Submitter {
    /// Submit exact selected row-major GGUF Q8 weights and contiguous F32 inputs.
    ///
    /// The oracle must independently accept every returned value from unchanged
    /// source inputs; shape/finiteness alone are insufficient. It executes before
    /// any caller output is published. The caller binds weights to the selected
    /// guarded model tensor; the queue transport does not authenticate the bytes.
    /// No output is changed on error or oracle unwind. No failed owner can retry.
    ///
    /// # Errors
    /// Rejects malformed input, bound exhaustion, native refusal, uncorrelated
    /// replies, oracle rejection, cancellation and expiry. Native calls still
    /// require external process supervision and verified teardown.
    fn submit(
        &mut self,
        weights: &[u8],
        activation: &[f32],
        output: &mut [f32],
        oracle: &mut dyn FnMut(&[f32]) -> Result<(), Error>,
    ) -> Result<(), Error>;
}

/// Native queue observations, complete caller work and cleanup remain separate.
#[derive(Clone, Copy, Debug)]
pub struct Q8ProjectionReport {
    /// Caller-bound model/source/package/catalog/driver identities, not authenticated here.
    pub identity: DispatchIdentity,
    /// Immutable selected model catalog ID.
    pub tensor: u16,
    /// Exact selected shape and source-derived solver mapping.
    pub projection: Q8Matmul,
    /// Original upper bound on writes, never replenished.
    pub operation_limit: u32,
    /// Native setup, latest operation and explicit cleanup results.
    pub queue: QueueComputeReport,
    /// Original write attempts, including a refused final write.
    pub operations_submitted: u32,
    /// Correlated and independently accepted outputs published synchronously.
    pub operations_completed: u32,
    /// The complete caller callback succeeded before cleanup.
    pub work_completed: bool,
    /// The original deadline or its work portion expired.
    pub deadline_expired: bool,
    /// Cancellation was observed before further native work or publication.
    pub cancelled: bool,
}

impl Q8ProjectionReport {
    pub(crate) const fn new(limits: &Q8Projection<'_>) -> Self {
        Self {
            identity: *limits.plan.identity(),
            tensor: limits.tensor,
            projection: limits.projection,
            operation_limit: limits.operations,
            queue: QueueComputeReport::new_fixture(
                SessionPreparationReport::new(0),
                compute::ComputeFixture::Q8ProjectionNoBlock,
            ),
            operations_submitted: 0,
            operations_completed: 0,
            work_completed: false,
            deadline_expired: false,
            cancelled: false,
        }
    }

    /// Complete selected work and explicit cleanup; no serving eligibility follows.
    #[must_use]
    pub const fn completed_cycle(self) -> bool {
        self.queue.is_q8_projection()
            && self.work_completed
            && !self.deadline_expired
            && !self.cancelled
            && self.operations_submitted > 0
            && self.operations_submitted <= self.operation_limit
            && self.operations_submitted == self.operations_completed
            && self.queue.completed_cycle()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn q8_native_receipt_does_not_inherit_fixed_matrix_acceptance() -> Result<(), Error> {
        let projection = Q8Matmul::new(128, 32, 2, 100_000)?;
        let identity = DispatchIdentity {
            model: [1; 32],
            model_bytes: 100_000,
            module: [2; 32],
            catalog: [3; 32],
            driver: [4; 32],
            source: [5; 20],
        };
        let plan = HybridPlan::new(
            identity,
            &[crate::TensorPlacement::Htp0Q8 {
                tensor: 2,
                offset: 32,
                projection,
            }],
            crate::CacheOwner::Cpu,
        )?;
        let limits = Q8Projection::new(
            &plan,
            2,
            2,
            Instant::now() + std::time::Duration::from_secs(5),
            Arc::new(AtomicBool::new(false)),
        )?;
        let report = Q8ProjectionReport::new(&limits);
        assert_eq!(report.identity, identity);
        assert_eq!(report.tensor, 2);
        assert_eq!(report.projection, projection);
        assert_eq!(report.operation_limit, 2);
        assert!(report.queue.is_q8_projection());
        assert!(report.queue.is_nonblocking_write_control());
        assert!(!report.queue.is_guarded_q2_control());
        assert!(!report.completed_cycle());
        assert!(
            compute::fixture_image_for(compute::ComputeFixture::Q8ProjectionNoBlock, 4096, 7)
                .is_none()
        );
        assert!(
            compute::ComputeFixture::Q8ProjectionNoBlock
                .output_offset()
                .is_none()
        );
        assert!(!compute::output_valid_for(
            compute::ComputeFixture::Q8ProjectionNoBlock,
            &[0; 256]
        ));
        Ok(())
    }
}
