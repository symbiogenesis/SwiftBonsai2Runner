//! Request-boundary policy over independently reviewed profile observations.
//!
//! This prepares an admission decision, not a process or device. The caller must
//! obtain fresh budgets and backend facts, verify evidence and hold artifact and
//! resource owners through execution. Serving does not consume this policy yet.

use super::{FallbackReason, StateUse, ranking};
use ranking::{Backend, Observation, Placement, Recommendation};

/// Complete conservative peak charges, including allocations outside the Job.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceCharge {
    /// Job-wide committed bytes, including startup and state/checkpoint overlap.
    pub job_commit_bytes: u64,
    /// Total system commit; includes Job commit and driver/private commit outside it.
    pub system_commit_bytes: u64,
    /// Unique physical bytes across CPU, shared mappings and device storage.
    ///
    /// This is the total, not an amount to add to the Job charge. Count shared
    /// pages once and include packing, compile, staging and load overlap peaks.
    pub physical_bytes: u64,
    /// Peak allocation against the independently observed Adreno budget.
    pub adreno_bytes: u64,
    /// Peak allocation against the independently observed Hexagon budget.
    pub hexagon_bytes: u64,
}

/// Fresh capacity observations; unknown capacity cannot authorize an allocation.
#[derive(Clone, Copy, Debug)]
pub struct LiveBudgets {
    /// Available physical RAM before the request, excluding no reserve yet.
    pub available_physical_bytes: Option<u64>,
    /// Available system commit before the request.
    pub available_commit_bytes: Option<u64>,
    /// Enforced aggregate Job commit ceiling.
    pub job_limit_bytes: u64,
    /// Conservative live Adreno allocation allowance, not advertised heap size.
    pub adreno_available_bytes: Option<u64>,
    /// Conservative live Hexagon allowance, including mappings outside the Job.
    pub hexagon_available_bytes: Option<u64>,
}

/// Current verified readiness for the exact profile's driver/runtime tuple.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendStatus {
    /// Independently verified supported backend with completed initialization.
    Ready,
    /// Required capability or initialization result is unknown.
    Unknown,
    /// Backend or signed deployment is unavailable.
    Unavailable,
    /// Current runtime identity differs from the reviewed profile.
    RuntimeChanged,
    /// Current driver identity differs from the reviewed profile.
    DriverChanged,
}

/// One trusted catalog entry; public receipt assertions cannot construct trust.
#[derive(Clone, Copy)]
pub struct Profile<'a> {
    /// Reviewed complete-request observations and exact current identity.
    pub observation: Observation<'a>,
    /// Independently reviewed complete peak resource ledger for this profile.
    pub resources: ResourceCharge,
    /// Current backend readiness, observed outside receipt parsing.
    pub backend: BackendStatus,
}

/// The entire requested workload; selection never truncates these values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reservation {
    /// Canonical complete prompt length, including template/history overhead.
    pub prompt_tokens: u32,
    /// Full requested output reservation, including reasoning where applicable.
    pub output_tokens: u32,
    /// Exact fresh or reused complete-state treatment.
    pub state: StateUse,
}

/// Backend intent for a complete request, fixed before work starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Intent {
    /// Choose the best currently eligible reviewed profile.
    Automatic,
    /// Require exactly this reviewed phase placement; never fall back.
    Strict(Placement),
}

/// A policy decision that must be rechecked immediately before execution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection<'a> {
    /// Ranked complete-request profile and effective phase placement.
    pub recommendation: Recommendation<'a>,
    /// Unchanged complete requested reservation.
    pub reservation: Reservation,
    /// All peak charges for the selected profile.
    pub resources: ResourceCharge,
    /// Why an otherwise preferred measured plan was excluded, when applicable.
    pub fallback_reason: Option<FallbackReason>,
}

fn uses(placement: Placement, backend: Backend) -> bool {
    placement.prefill == backend || placement.decode == backend
}

fn capacity(charge: u64, available: Option<u64>) -> bool {
    charge == 0 || available.is_some_and(|bytes| charge <= bytes)
}

fn resource_admission(profile: &Profile<'_>, budgets: LiveBudgets) -> bool {
    let charge = profile.resources;
    charge.job_commit_bytes > 0
        && charge.system_commit_bytes >= charge.job_commit_bytes
        && charge.physical_bytes > 0
        && charge.job_commit_bytes <= budgets.job_limit_bytes
        && capacity(charge.system_commit_bytes, budgets.available_commit_bytes)
        && charge
            .physical_bytes
            .checked_add(super::super::PHYSICAL_RESERVE_BYTES)
            .is_some_and(|required| capacity(required, budgets.available_physical_bytes))
        && (!uses(profile.observation.placement, Backend::Adreno) || charge.adreno_bytes > 0)
        && (!uses(profile.observation.placement, Backend::Hexagon) || charge.hexagon_bytes > 0)
        && capacity(charge.adreno_bytes, budgets.adreno_available_bytes)
        && capacity(charge.hexagon_bytes, budgets.hexagon_available_bytes)
}

fn accepts(profile: &Profile<'_>, reservation: Reservation) -> bool {
    let workload = &profile.observation.current.0.workload;
    reservation.prompt_tokens >= workload.minimum_prompt_tokens
        && reservation.prompt_tokens <= workload.maximum_prompt_tokens
        && reservation.output_tokens > 0
        && reservation.output_tokens <= workload.maximum_output_tokens
        && reservation
            .prompt_tokens
            .checked_add(reservation.output_tokens)
            .is_some_and(|tokens| tokens <= workload.context_tokens)
        && reservation.state == workload.state
}

fn exclusion(profile: &Profile<'_>, budgets: LiveBudgets) -> Option<FallbackReason> {
    match profile.backend {
        BackendStatus::Unknown => Some(FallbackReason::UnknownCapability),
        BackendStatus::Unavailable => Some(FallbackReason::UnavailableBackend),
        BackendStatus::RuntimeChanged => Some(FallbackReason::IncompatibleRuntime),
        BackendStatus::DriverChanged => Some(FallbackReason::IncompatibleDriver),
        BackendStatus::Ready => {
            (!resource_admission(profile, budgets)).then_some(FallbackReason::InsufficientMemory)
        }
    }
}

/// Select a qualified recommendation after rechecking full-workload resources.
///
/// Ranking retains exact evidence keys, common comparison identities, conservative
/// benefit thresholds and deterministic ties. Strict intent applies the same
/// evidence and benefit gates and refuses CPU fallback. Only whole-request plans
/// compete; independently fast phases are never spliced together.
///
/// # Errors
/// Rejects more than 32 candidates, duplicate IDs, a missing/unreviewed CPU
/// control, unmatched workloads, unavailable full CPU reservation or an unmet
/// strict placement. This is not a serving permit or a fallback during a request.
pub fn select<'a>(
    control: &Profile<'a>,
    candidates: &[Profile<'a>],
    reservation: Reservation,
    budgets: LiveBudgets,
    intent: Intent,
) -> Result<Selection<'a>, String> {
    if candidates.len() > 32 || !accepts(control, reservation) {
        return Err("A bounded catalog and exact complete workload are required.".into());
    }
    if exclusion(control, budgets).is_some() || !control.observation.ready_and_admitted {
        return Err("The reviewed CPU control cannot reserve this complete request.".into());
    }
    // First rank with the previously reviewed admission facts. This validates
    // IDs and supplies a deterministic reason if live conditions remove the winner.
    let original: Vec<_> = candidates
        .iter()
        .map(|profile| profile.observation)
        .collect();
    let preferred = ranking::recommend(&control.observation, &original)?;
    let current: Vec<_> = candidates
        .iter()
        .map(|profile| {
            let mut observation = profile.observation;
            observation.ready_and_admitted &= exclusion(profile, budgets).is_none()
                && accepts(profile, reservation)
                && match intent {
                    Intent::Automatic => true,
                    Intent::Strict(placement) => observation.placement == placement,
                };
            observation
        })
        .collect();
    let recommendation = ranking::recommend(&control.observation, &current)?;
    if matches!(intent, Intent::Strict(placement) if recommendation.placement != placement) {
        return Err("strict-backend-unavailable".into());
    }
    let selected = if recommendation.profile_id == control.observation.id {
        control
    } else {
        candidates
            .iter()
            .find(|profile| profile.observation.id == recommendation.profile_id)
            .ok_or("Selected profile is absent from the bounded catalog.")?
    };
    let fallback_reason = if preferred.profile_id == recommendation.profile_id {
        None
    } else {
        candidates
            .iter()
            .find(|profile| profile.observation.id == preferred.profile_id)
            .and_then(|profile| exclusion(profile, budgets))
    };
    Ok(Selection {
        recommendation,
        reservation,
        resources: selected.resources,
        fallback_reason,
    })
}

#[cfg(test)]
mod tests;
