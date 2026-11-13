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
/// The runtime's reviewed total graph-state allocation ceiling.
pub const STATE_LIMIT_BYTES: u64 = 4 * 1024 * 1024 * 1024;
/// Swift's F32 recurrent/convolution state independent of attention context.
pub const RECURRENT_BYTES: u64 = 156_893_184;
/// Context granularity matches the runtime's paged attention allocation.
pub const CONTEXT_PAGE_TOKENS: usize = 256;
/// Preserve room for the largest reply, template margin and a positive prompt.
pub const MIN_CONTEXT_TOKENS: usize =
    (4_096_usize + 64).div_ceil(CONTEXT_PAGE_TOKENS) * CONTEXT_PAGE_TOKENS;

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
    /// FP16 is charged even when the selected runtime uses a smaller KV format;
    /// this keeps the admission estimate independent of unqualified cache savings.
    ///
    /// # Errors
    /// Returns a value-free error when the minimum usable context cannot fit or
    /// checked arithmetic cannot represent the selected complete allocation.
    pub fn for_available_memory(available_bytes: u64) -> Result<Self, String> {
        let allocation = available_bytes
            .checked_sub(HOST_RESERVE_BYTES)
            .and_then(|bytes| bytes.checked_sub(STARTUP_RESERVE_BYTES))
            .and_then(|bytes| bytes.checked_sub(FIXED_RUNTIME_BYTES))
            .ok_or_else(insufficient)?;
        let state_allocation = STATE_LIMIT_BYTES
            .checked_sub(RECURRENT_BYTES)
            .ok_or_else(insufficient)?;
        let tokens =
            (allocation / CONTEXT_BYTES_PER_TOKEN).min(state_allocation / STATE_BYTES_PER_TOKEN);
        let tokens = usize::try_from(tokens)
            .map_err(|_| insufficient())?
            .min(MODEL_CONTEXT_TOKENS);
        let context_tokens = tokens / CONTEXT_PAGE_TOKENS * CONTEXT_PAGE_TOKENS;
        if context_tokens < MIN_CONTEXT_TOKENS {
            return Err(insufficient());
        }
        let context_bytes = u64::try_from(context_tokens)
            .ok()
            .and_then(|tokens| tokens.checked_mul(CONTEXT_BYTES_PER_TOKEN))
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
}
