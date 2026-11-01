//! Native maintenance, provisioning and qualification commands for Bonsai.
#![forbid(unsafe_code)]

/// A command result suitable for a JSON receipt or a diagnostic on stderr.
pub type Result<T> = std::result::Result<T, String>;

/// Commands exposed by the maintenance executable and its checked bootstrap receipts.
pub const COMMANDS: &[&str] = &[
    "chat",
    "hexagon-tools",
    "build-tools",
    "provision",
    "artifact",
    "doctor",
    "credentials",
    "memory",
    "host-cpu",
    "host-gpu",
    "host-opencl",
    "cpu-evaluation",
    "cpu-check-plan",
    "cpu-check-report",
    "cpu-check",
    "server-plan",
    "server-policy",
    "runtime-stage",
    "runtime-verify",
    "completion",
    "qualification",
    "runtime-promotion",
    "speech-models",
    "inference-lock",
    "inference-capabilities",
    "speech-build",
    "speech-evaluation",
    "source-build",
    "chat-launch",
    "chat-prepare",
    "chat-package",
    "gguf",
    "ptq1-capture",
    "native-qualification",
    "ptq1-packing",
    "packing-qualification",
    "host-capabilities",
    "native-acceptance-budget",
    "startup-diagnostics",
    "hexagon-policy",
    "publisher-startup",
];

/// Returns the native maintenance protocol capabilities without reading a request.
#[must_use]
pub fn version() -> serde_json::Value {
    serde_json::json!({
        "Name": "bonsai-tools", "Version": env!("CARGO_PKG_VERSION"),
        "ProtocolVersion": 5, "StartupProtocolVersion": 1,
        "BootstrapProtocolVersion": 1, "ChatLaunchProtocolVersion": 1, "Commands": COMMANDS
    })
}

/// Private credential creation through the existing audited Windows boundary.
pub mod credentials;

/// Bounded completion supervision with retained process and artifact ownership.
pub mod completion;

/// Model-free native CPU topology and current-process availability observations.
pub mod host_cpu;

/// Bounded model-free Vulkan adapter observations, without backend selection.
pub mod host_gpu;

/// Bounded model-free `OpenCL` feasibility observations, without backend selection.
pub mod host_opencl;

/// Bounded paired CPU tuning plans for the X2 Elite Extreme CI lane.
pub mod cpu_evaluation;

/// Offline runtime verification and bounded probes with retained artifact guards.
pub mod runtime;

/// Fixed qualification plans, canonical identities, and offline gate scoring.
pub mod qualification;

/// Exact standard-profile promotion policy and offline evidence completeness review.
pub mod runtime_promotion;

/// Pure server preflight policy shared with the development broker.
pub mod server;

/// Native CPU admission, verification and bounded decode checks.
pub mod cpu_check;

/// Offline runtime archive staging through the shared artifact policy.
pub mod install;

/// Native speech artifact plans shared with the provisioning wrappers.
pub mod speech;

/// Offline review of a release receipt, exact source ZIP and ARM64 bundle.
pub mod inference_lock;

/// Model-free capability discovery from the exact pinned inference release.
pub mod inference_capabilities;

/// Native speech build locks and bounded source archive staging.
pub mod speech_build;

/// CI-only paired speech plans and bounded native memory-capture sessions.
pub mod speech_evaluation;

/// Checked source builds for the maintenance tool and native chat executables.
pub mod source_build;

/// Native chat validation, provisioning and launch orchestration.
pub mod chat_launch;

/// Installed native chat package selection and integrity checks.
pub mod chat_package;

/// Verified GGUF inspection, pinned Swift export and hosted integration fixtures.
pub mod gguf;

/// Source-bound PTQ1 collector receipts and complete unsigned ABI comparisons.
pub mod ptq1_capture;

/// Fixed native numerical and row-worker qualification with original owner budgets.
pub mod native_qualification;

/// Independent complete current-weight PTQ1 packing and preserved-byte checks.
pub mod ptq1_packing;

/// Hosted-only complete PTQ1 reproduction, worker deadlines and evidence retention.
pub mod packing_qualification;

/// Pure host capability projection and canonical CPU observation identities.
pub mod host_capabilities;

/// Aggregate owner and job deadline calculations for native acceptance.
pub mod native_acceptance_budget;

/// Bounded and credential-checked native startup log publication.
pub mod startup_diagnostics;

/// Offline Hexagon plans, source bindings and native evidence validation.
pub mod hexagon_policy;

/// Fixed archived publisher startup probes and bounded original-PQ2 smoke.
pub mod publisher_startup;

/// Retained caller identity for cancellation of native startup work.
pub mod startup_cancel;

/// Direct native command-line input and chat launch options.
pub mod cli;

/// Hosted Hexagon source builds and bounded SDK inventory.
pub mod hexagon_tools;

/// Source-bound engine, capture collector and publisher candidate builds.
pub mod build_tools;

/// Locked-artifact acquisition, verification and explicit runtime provisioning.
pub mod provision;

/// Model-free native host, driver and bounded tool inventory.
pub mod doctor;

mod identity;
mod native_startup;
#[cfg(windows)]
mod runtime_probe;
mod startup_cache;
mod startup_download;
mod startup_process;
