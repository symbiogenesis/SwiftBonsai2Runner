//! Fixed broker startup brackets without model, path or credential values.

use serde::Serialize;
use std::time::Instant;

/// A broker startup boundary, independent of inference-internal work.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum StartupPhase {
    /// Path, sandbox-support, policy and physical-memory admission checks.
    PolicyAndAdmission,
    /// Verification and retained read ownership of the selected native runtime.
    NativeRuntimeVerification,
    /// Selected model verification, plus runtime verification for publisher profiles.
    ModelVerification,
    /// Verification and retained read ownership of the optional pinned adapter.
    AdapterVerification,
    /// Private credentials and process specification using retained runtime ownership.
    CredentialsAndRuntime,
    /// Owned process creation and required native containment.
    ProcessCreation,
    /// Health/authentication polling, including unobserved backend loading.
    AuthenticatedReadiness,
    /// Exact native runtime, model, adapter and worker-policy identity exchange.
    NativeIdentity,
    /// Frontend binding and attachment of the verified backend policy.
    FrontendBinding,
}

/// One fixed startup observation suitable for credential-free qualification receipts.
///
/// Durations bracket broker operations and observer overhead. Readiness does not
/// separate backend hashing, tensor loading, graph construction or polling. A
/// phase with no completed observation remains unfinished after cancellation or
/// failure; synchronous verification cannot be preempted by an async timeout.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupProgress {
    /// Fixed operation being observed; contains no caller-provided values.
    pub phase: StartupPhase,
    /// Whether this observation closes the broker operation's timing bracket.
    pub completed: bool,
    /// Monotonic milliseconds since this broker startup observer was created.
    pub elapsed_milliseconds: f64,
    /// Completed bracket duration, absent when the operation has only started.
    pub phase_milliseconds: Option<f64>,
}

pub(super) struct StartupTimer<F> {
    started: Instant,
    phase_started: Instant,
    phase: Option<StartupPhase>,
    observe: F,
}

impl<F: FnMut(StartupProgress)> StartupTimer<F> {
    pub(super) fn new(observe: F) -> Self {
        let started = Instant::now();
        Self {
            started,
            phase_started: started,
            phase: None,
            observe,
        }
    }

    pub(super) fn phase(&mut self, phase: StartupPhase) {
        let now = Instant::now();
        self.complete_at(now);
        self.phase = Some(phase);
        self.phase_started = now;
        (self.observe)(StartupProgress {
            phase,
            completed: false,
            elapsed_milliseconds: now.duration_since(self.started).as_secs_f64() * 1000.0,
            phase_milliseconds: None,
        });
    }

    pub(super) fn finish(&mut self) {
        self.complete_at(Instant::now());
    }

    fn complete_at(&mut self, now: Instant) {
        if let Some(phase) = self.phase.take() {
            let phase_milliseconds = now.duration_since(self.phase_started).as_secs_f64() * 1000.0;
            (self.observe)(StartupProgress {
                phase,
                completed: true,
                elapsed_milliseconds: now.duration_since(self.started).as_secs_f64() * 1000.0,
                phase_milliseconds: Some(phase_milliseconds),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{StartupPhase, StartupTimer};

    #[test]
    fn startup_observer_keeps_fixed_brackets_and_unfinished_phase() -> Result<(), serde_json::Error>
    {
        let mut observations = Vec::new();
        {
            let mut timer = StartupTimer::new(|observation| observations.push(observation));
            timer.phase(StartupPhase::PolicyAndAdmission);
            timer.phase(StartupPhase::ModelVerification);
            timer.phase(StartupPhase::AdapterVerification);
            // Dropping a failed/cancelled startup never reports its last phase
            // completed and never substitutes a native teardown duration.
        }
        assert_eq!(observations.len(), 5);
        let serialized = serde_json::to_value(&observations)?;
        assert_eq!(serialized[0]["phase"], "policy-and-admission");
        assert_eq!(serialized[1]["phase"], "policy-and-admission");
        assert_eq!(serialized[2]["phase"], "model-verification");
        assert_eq!(serialized[3]["phase"], "model-verification");
        assert_eq!(serialized[4]["phase"], "adapter-verification");
        for (index, observation) in observations.iter().enumerate() {
            assert_eq!(observation.completed, index % 2 == 1);
            assert_eq!(
                observation.phase_milliseconds.is_some(),
                observation.completed
            );
            assert!(observation.elapsed_milliseconds.is_finite());
            assert!(observation.elapsed_milliseconds >= 0.0);
        }
        for pair in observations.windows(2) {
            assert!(pair[0].elapsed_milliseconds <= pair[1].elapsed_milliseconds);
        }
        let mut finished = Vec::new();
        {
            let mut timer = StartupTimer::new(|observation| finished.push(observation));
            timer.phase(StartupPhase::FrontendBinding);
            timer.finish();
        }
        assert_eq!(finished.len(), 2);
        assert!(finished[1].completed);
        assert_eq!(
            serde_json::to_value(finished)?[1]["phase"],
            "frontend-binding"
        );
        Ok(())
    }
}
