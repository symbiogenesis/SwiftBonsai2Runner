//! Pure Swift context sizing from a prelaunch available-physical-memory snapshot.
//!
//! This is conservative admission arithmetic, not measured peak memory. Callers
//! recheck the resulting requirement immediately before starting the owned Job.

/// Swift's verified training context; available memory may select less.
pub const MODEL_CONTEXT_TOKENS: usize = 262_144;
/// Physical RAM retained for Windows and other applications.
pub const HOST_RESERVE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
/// Selection-only slack for startup allocations between observation and child admission.
/// The final admission check still requires the Job allowance plus the 4 GiB host reserve.
pub const STARTUP_RESERVE_BYTES: u64 = 256 * 1024 * 1024;
/// Existing Swift allowance excluding its reference 8K FP16 KV payload.
///
/// This retains 11 GiB for weights, recurrence, adapters, runtime and headroom;
/// it is conservative source accounting, not a measured peak.
pub const FIXED_RUNTIME_BYTES: u64 = 11 * 1024 * 1024 * 1024;
/// FP16 attention KV payload per token across Swift's 16 attention layers.
pub const KV_BYTES_PER_TOKEN: u64 = 65_536;
/// Maximum publisher score rows: the fixed 64-token microbatch and 24 heads.
pub const MAX_CONTEXT_SCORE_ROWS: u64 = 64 * 24;
/// Live and rollback KV, publisher attention scratch and bounded token metadata.
/// Native sequential attention uses less scratch; no cache-saving assumption is made.
pub const CONTEXT_BYTES_PER_TOKEN: u64 = 2 * KV_BYTES_PER_TOKEN + MAX_CONTEXT_SCORE_ROWS * 4 + 128;
/// One graph's KV, sequential native attention scores and token/page metadata.
pub const STATE_BYTES_PER_TOKEN: u64 = KV_BYTES_PER_TOKEN + 24 * 4 + 128;
/// K4/V4 records, including their four-byte scales, across Swift's attention layers.
pub const RSQ4_KV_BYTES_PER_TOKEN: u64 = 16_896;
/// K4/V3 records, including their four-byte scales, across Swift's attention layers.
pub const RSQ4K3V_KV_BYTES_PER_TOKEN: u64 = 14_848;
/// The runtime's reviewed total graph-state allocation ceiling.
pub const STATE_LIMIT_BYTES: u64 = 4 * 1024 * 1024 * 1024;
/// Swift's F32 recurrent/convolution state independent of attention context.
pub const RECURRENT_BYTES: u64 = 156_893_184;
/// Context selection granularity, divisible by the runtime's 64-token cache pages.
pub const CONTEXT_PAGE_TOKENS: usize = 256;
/// Preserve room for the largest reply, template margin and a positive prompt.
pub const MIN_CONTEXT_TOKENS: usize =
    (4_096_usize + 64).div_ceil(CONTEXT_PAGE_TOKENS) * CONTEXT_PAGE_TOKENS;

/// Immutable cache and execution layout used to admit one Swift model lifetime.
///
/// Native profiles require a separately reviewed, source-bound runtime release.
/// The publisher retains its original FP16 cache and microbatch score allowance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwiftContextProfile {
    /// Publisher FP16 cache with 64-token microbatch attention scratch.
    PublisherF16,
    /// Native FP16 cache with sequential attention scratch.
    NativeF16,
    /// Native K4/V4 cache with sequential attention scratch.
    NativeRsq4,
    /// Explicit native K4/V3 capacity profile with sequential attention scratch.
    NativeRsq4K3v,
}

impl SwiftContextProfile {
    /// Attention payload per token, including packed record scales.
    #[must_use]
    pub const fn kv_bytes_per_token(self) -> u64 {
        match self {
            Self::PublisherF16 | Self::NativeF16 => KV_BYTES_PER_TOKEN,
            Self::NativeRsq4 => RSQ4_KV_BYTES_PER_TOKEN,
            Self::NativeRsq4K3v => RSQ4K3V_KV_BYTES_PER_TOKEN,
        }
    }

    /// Two complete caches, selected score scratch and bounded token metadata.
    ///
    /// Shared immutable pages do not reduce this general divergent-history bound.
    #[must_use]
    pub const fn context_bytes_per_token(self) -> u64 {
        let score_rows = match self {
            Self::PublisherF16 => MAX_CONTEXT_SCORE_ROWS,
            Self::NativeF16 | Self::NativeRsq4 | Self::NativeRsq4K3v => 24,
        };
        2 * self.kv_bytes_per_token() + score_rows * 4 + 128
    }

    /// One live graph's cache, sequential scores and token/page bookkeeping.
    #[must_use]
    pub const fn state_bytes_per_token(self) -> u64 {
        self.kv_bytes_per_token() + 24 * 4 + 128
    }
}

/// A context and its complete conservative Job/admission requirements.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwiftContextPlan {
    /// Fixed prompt, response and template reservation budget for one model lifetime.
    pub context_tokens: usize,
    /// Complete owned-Job memory ceiling including fixed runtime headroom.
    pub memory_limit_bytes: usize,
    /// Required available physical RAM, including the separate host reserve.
    pub required_available_bytes: u64,
}

impl SwiftContextPlan {
    /// Select the largest page-aligned context fitting physical RAM and graph state.
    ///
    /// This compatibility entry point preserves publisher FP16/microbatch sizing.
    /// Native source consumers must select their immutable layout with
    /// [`Self::for_profile`] after verifying the matching runtime contract.
    ///
    /// # Errors
    /// Returns a value-free error when the minimum usable context cannot fit or
    /// checked arithmetic cannot represent the selected complete allocation.
    pub fn for_available_memory(available_bytes: u64) -> Result<Self, String> {
        Self::for_profile(available_bytes, SwiftContextProfile::PublisherF16)
    }

    /// Select the largest page-aligned context for an explicit cache/execution layout.
    ///
    /// The two-cache process charge preserves rollback while an unrelated history
    /// is computed. Optional tiled prefill or speculation require their own additive
    /// reservations and are not admitted by these sequential native profiles.
    ///
    /// # Errors
    /// Rejects insufficient memory or an unrepresentable complete allocation.
    pub fn for_profile(available_bytes: u64, profile: SwiftContextProfile) -> Result<Self, String> {
        let allocation = available_bytes
            .checked_sub(HOST_RESERVE_BYTES)
            .and_then(|bytes| bytes.checked_sub(STARTUP_RESERVE_BYTES))
            .and_then(|bytes| bytes.checked_sub(FIXED_RUNTIME_BYTES))
            .ok_or_else(insufficient)?;
        let state_allocation = STATE_LIMIT_BYTES
            .checked_sub(RECURRENT_BYTES)
            .ok_or_else(insufficient)?;
        let process_bytes = profile.context_bytes_per_token();
        let state_bytes = profile.state_bytes_per_token();
        let tokens = (allocation / process_bytes).min(state_allocation / state_bytes);
        let tokens = usize::try_from(tokens)
            .map_err(|_| insufficient())?
            .min(MODEL_CONTEXT_TOKENS);
        let context_tokens = tokens / CONTEXT_PAGE_TOKENS * CONTEXT_PAGE_TOKENS;
        if context_tokens < MIN_CONTEXT_TOKENS {
            return Err(insufficient());
        }
        let context_bytes = u64::try_from(context_tokens)
            .ok()
            .and_then(|tokens| tokens.checked_mul(process_bytes))
            .ok_or_else(insufficient)?;
        let memory_limit = FIXED_RUNTIME_BYTES
            .checked_add(context_bytes)
            .ok_or_else(insufficient)?;
        let required_available_bytes = memory_limit
            .checked_add(HOST_RESERVE_BYTES)
            .ok_or_else(insufficient)?;
        Ok(Self {
            context_tokens,
            memory_limit_bytes: usize::try_from(memory_limit).map_err(|_| insufficient())?,
            required_available_bytes,
        })
    }
}

fn insufficient() -> String {
    "Available memory cannot admit Swift's context and reserved runtime resources.".into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    #[test]
    fn sizing_preserves_reserves_and_rejects_less_than_one_minimum_context()
    -> Result<(), Box<dyn Error>> {
        let minimum = HOST_RESERVE_BYTES
            + STARTUP_RESERVE_BYTES
            + FIXED_RUNTIME_BYTES
            + u64::try_from(MIN_CONTEXT_TOKENS)? * CONTEXT_BYTES_PER_TOKEN;
        assert!(SwiftContextPlan::for_available_memory(minimum - 1).is_err());
        let plan = SwiftContextPlan::for_available_memory(minimum)?;
        assert_eq!(plan.context_tokens, MIN_CONTEXT_TOKENS);
        assert_eq!(
            plan.required_available_bytes + STARTUP_RESERVE_BYTES,
            minimum
        );
        assert_eq!(
            plan.required_available_bytes - u64::try_from(plan.memory_limit_bytes)?,
            HOST_RESERVE_BYTES
        );
        for unavailable in [0, HOST_RESERVE_BYTES, FIXED_RUNTIME_BYTES] {
            assert!(SwiftContextPlan::for_available_memory(unavailable).is_err());
        }
        Ok(())
    }

    #[test]
    fn more_memory_selects_more_context_until_model_or_runtime_state_cap()
    -> Result<(), Box<dyn Error>> {
        let gib = 1024 * 1024 * 1024;
        let small = SwiftContextPlan::for_available_memory(16 * gib)?;
        let large = SwiftContextPlan::for_available_memory(18 * gib)?;
        let unlimited = SwiftContextPlan::for_available_memory(u64::MAX)?;
        assert!(small.context_tokens < 8_192);
        assert!(large.context_tokens > small.context_tokens);
        assert_eq!(unlimited.context_tokens, 62_720);
        assert!(unlimited.context_tokens <= MODEL_CONTEXT_TOKENS);
        let state_bytes =
            u64::try_from(unlimited.context_tokens)? * STATE_BYTES_PER_TOKEN + RECURRENT_BYTES;
        assert!(state_bytes <= STATE_LIMIT_BYTES);
        for (available, plan) in [(16 * gib, small), (18 * gib, large)] {
            assert!(plan.required_available_bytes <= available);
            assert!(available - plan.required_available_bytes >= STARTUP_RESERVE_BYTES);
            assert!(
                available - plan.required_available_bytes - STARTUP_RESERVE_BYTES
                    < 256 * CONTEXT_BYTES_PER_TOKEN
            );
        }
        Ok(())
    }

    #[test]
    fn native_cache_profiles_select_capacity_without_spending_host_or_startup_reserves()
    -> Result<(), Box<dyn Error>> {
        use SwiftContextProfile::{NativeF16, NativeRsq4, PublisherF16};
        let gib = 1024 * 1024 * 1024;
        assert_eq!(PublisherF16.context_bytes_per_token(), 137_344);
        assert_eq!(NativeF16.context_bytes_per_token(), 131_296);
        assert_eq!(NativeRsq4.context_bytes_per_token(), 34_016);
        assert_eq!(NativeRsq4.state_bytes_per_token(), 17_120);
        let publisher = SwiftContextPlan::for_profile(20 * gib, PublisherF16)?;
        assert_eq!(publisher.context_tokens, 37_120);
        assert_eq!(publisher, SwiftContextPlan::for_available_memory(20 * gib)?);
        for (available, expected) in [(18 * gib, 86_784), (20 * gib, 149_760), (24 * gib, 241_664)]
        {
            let plan = SwiftContextPlan::for_profile(available, NativeRsq4)?;
            assert_eq!(plan.context_tokens, expected);
            assert!(plan.required_available_bytes + STARTUP_RESERVE_BYTES <= available);
            assert_eq!(
                u64::try_from(plan.memory_limit_bytes)? + HOST_RESERVE_BYTES,
                plan.required_available_bytes
            );
            let tokens = u64::try_from(plan.context_tokens)?;
            assert!(
                tokens * NativeRsq4.state_bytes_per_token() + RECURRENT_BYTES <= STATE_LIMIT_BYTES
            );
        }
        for (profile, ceiling) in [(NativeF16, 62_720), (NativeRsq4, 241_664)] {
            let plan = SwiftContextPlan::for_profile(u64::MAX, profile)?;
            assert_eq!(plan.context_tokens, ceiling);
            let next_page = u64::try_from(plan.context_tokens + CONTEXT_PAGE_TOKENS)?;
            assert!(
                next_page * profile.state_bytes_per_token() + RECURRENT_BYTES > STATE_LIMIT_BYTES
            );
        }
        Ok(())
    }

    #[test]
    fn explicit_k4v3_capacity_preserves_two_caches_reserves_and_the_model_ceiling()
    -> Result<(), Box<dyn Error>> {
        let profile = SwiftContextProfile::NativeRsq4K3v;
        assert_eq!(profile.kv_bytes_per_token(), 14_848);
        assert_eq!(profile.context_bytes_per_token(), 29_920);
        assert_eq!(profile.state_bytes_per_token(), 15_072);
        let gib = 1024 * 1024 * 1024;
        for (available, expected) in [(18 * gib, 98_560), (20 * gib, 170_240), (24 * gib, 262_144)]
        {
            let plan = SwiftContextPlan::for_profile(available, profile)?;
            assert_eq!(plan.context_tokens, expected);
            assert!(plan.required_available_bytes + STARTUP_RESERVE_BYTES <= available);
            assert_eq!(
                u64::try_from(plan.memory_limit_bytes)? + HOST_RESERVE_BYTES,
                plan.required_available_bytes
            );
            assert!(
                u64::try_from(plan.context_tokens)? * profile.state_bytes_per_token()
                    + RECURRENT_BYTES
                    <= STATE_LIMIT_BYTES
            );
            if plan.context_tokens < MODEL_CONTEXT_TOKENS {
                let next_page = u64::try_from(plan.context_tokens + CONTEXT_PAGE_TOKENS)?;
                assert!(
                    FIXED_RUNTIME_BYTES
                        + next_page * profile.context_bytes_per_token()
                        + HOST_RESERVE_BYTES
                        + STARTUP_RESERVE_BYTES
                        > available
                );
            }
        }
        assert_eq!(
            SwiftContextPlan::for_profile(u64::MAX, profile)?.context_tokens,
            MODEL_CONTEXT_TOKENS
        );
        Ok(())
    }

    #[test]
    fn every_cache_profile_rejects_one_byte_below_its_minimum_launch_budget()
    -> Result<(), Box<dyn Error>> {
        for profile in [
            SwiftContextProfile::PublisherF16,
            SwiftContextProfile::NativeF16,
            SwiftContextProfile::NativeRsq4,
            SwiftContextProfile::NativeRsq4K3v,
        ] {
            let minimum = HOST_RESERVE_BYTES
                + STARTUP_RESERVE_BYTES
                + FIXED_RUNTIME_BYTES
                + u64::try_from(MIN_CONTEXT_TOKENS)? * profile.context_bytes_per_token();
            assert!(SwiftContextPlan::for_profile(minimum - 1, profile).is_err());
            let plan = SwiftContextPlan::for_profile(minimum, profile)?;
            assert_eq!(plan.context_tokens, MIN_CONTEXT_TOKENS);
            assert_eq!(
                plan.required_available_bytes + STARTUP_RESERVE_BYTES,
                minimum
            );
            assert!(SwiftContextPlan::for_profile(0, profile).is_err());
        }
        Ok(())
    }
}
