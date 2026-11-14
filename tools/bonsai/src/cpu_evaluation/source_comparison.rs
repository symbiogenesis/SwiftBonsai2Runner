//! Offline admission for the separate current-source collector protocol.
//!
//! Reviewed preparation/owner digests are supplied independently by the caller.
//! Their identities bind bytes; they do not authenticate a producer or its claims.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Component, Path, PathBuf},
};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::Result;

#[cfg(test)]
mod tests;

const MODEL: &str = "5912bb739217cf25b4283a098c7b893d9baff36bdf2e72d3dbf4cb999d5633d5";
const ADAPTER: &str = "f1669534803d340a496015f5c45125f3437b4d13ec764f40e34488ce83967f42";
const TARGET: &str = "aarch64-pc-windows-msvc";
const SCHEDULE: [&str; 4] = ["baseline", "candidate", "candidate", "baseline"];
const FEATURES: [&str; 3] = ["cli", "experimental-neon", "parallel-rows"];
const INPUTS: [&str; 8] = [
    "Cargo.lock",
    "Cargo.toml",
    "examples/cpu_bundle_check/collector.rs",
    "examples/cpu_bundle_check/candidate_tests.rs",
    "examples/cpu_candidate_check.rs",
    "tools/native-hash/Cargo.toml",
    "tools/neon-dot/Cargo.toml",
    "rust-toolchain.toml",
];
const CONTROLS: [&str; 3] = [
    "tests::source_candidate_failure_keeps_partial_output_and_distinct_create_new_receipts",
    "tests::source_candidate_receipts_separate_profile_and_unknown_observations",
    "tests::source_candidate_selection_requires_explicit_bounded_profile",
];
const STATE_CONTROL: &str = "graph_parallel_rows_preserve_logits_checkpoints_and_recovery";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    operation: String,
    preparation_path: PathBuf,
    preparation_sha256: String,
    owner_path: PathBuf,
    owner_sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reference {
    path: String,
    sha256: String,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Profile {
    kv_cache: String,
    context_tokens: u64,
    cpu_prefill_tokens: u64,
    cpu_prefill_scratch_bytes: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Check {
    phase: String,
    command: Vec<String>,
    exit_code: i32,
    passed: Vec<String>,
    failed: u64,
    ignored: u64,
    measured: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Build {
    source_revision: String,
    clean: bool,
    compiler: String,
    toolchain: String,
    rustc_sha256: String,
    cargo_sha256: String,
    helper_source_sha256: BTreeMap<String, String>,
    target: String,
    features: Vec<String>,
    inputs: BTreeMap<String, String>,
    executable_sha256: String,
    executable_bytes: u64,
    checks: Vec<Check>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Preparation {
    schema_version: u64,
    kind: String,
    runner_revision: String,
    evaluator_sha256: String,
    profile: Profile,
    messages: Reference,
    output_tokens: u64,
    model_sha256: String,
    adapter_sha256: String,
    schedule: Vec<String>,
    arms: BTreeMap<String, Build>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Arm {
    id: String,
    role: String,
    started_tick: u64,
    execution_deadline_tick: u64,
    collector_deadline_unix_milliseconds: u64,
    cleanup_completed_tick: u64,
    power_before: Value,
    power_after: Value,
    owned_job_empty: bool,
    cleanup_succeeded: bool,
    minimum_admission_bytes: u64,
    maximum_job_memory_bytes: u64,
    receipt: Reference,
    started_receipt: Reference,
    supervision: Reference,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Owner {
    schema_version: u64,
    kind: String,
    preparation_sha256: String,
    workload_predeclared: bool,
    completed: bool,
    budget_seconds: u64,
    fixed_work: String,
    estimated_seconds: u64,
    budget_reason: String,
    clock_frequency: u64,
    started_tick: u64,
    deadline_tick: u64,
    original_deadline_unix_milliseconds: u64,
    retention_completed_tick: u64,
    power_before: Value,
    power_after: Value,
    arms: Vec<Arm>,
}

fn require(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

fn digest(value: &str) -> Result<()> {
    require(
        crate::identity::is_lower_hex(value, 64),
        "Invalid SHA-256 identity.",
    )
}

fn member(root: &Path, name: &str) -> Result<PathBuf> {
    require(
        !name.is_empty() && name.len() <= 256 && !name.contains(['\\', ':', '%', '\0']),
        "Invalid confined evidence member.",
    )?;
    let path = Path::new(name);
    require(
        !path.is_absolute()
            && path
                .components()
                .all(|part| matches!(part, Component::Normal(_))),
        "Evidence member escapes its packet.",
    )?;
    Ok(root.join(path))
}

fn read(path: &Path, sha256: &str, maximum: u64) -> Result<Vec<u8>> {
    digest(sha256)?;
    bonsai_artifacts::validate_external_path(path).map_err(|error| error.to_string())?;
    let file = bonsai_artifacts::open_locked_file(path).map_err(|error| error.to_string())?;
    require(
        file.metadata().map_err(|error| error.to_string())?.len() <= maximum,
        "Evidence exceeds its pre-read bound.",
    )?;
    let bytes = crate::bounded_input::read(file, maximum)
        .map_err(|error| error.into_message("Evidence grew beyond its byte bound."))?;
    require(
        crate::identity::sha256(&bytes) == sha256,
        "Evidence differs from its independently supplied digest.",
    )?;
    Ok(bytes)
}

fn json_bytes(bytes: &[u8]) -> Result<Value> {
    // The protocol uses typed envelopes, and checks nested collector/supervisor
    // values for duplicate names before Value would silently discard them.
    let unique: Unique = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
    Ok(unique.0)
}

fn referenced(root: &Path, reference: &Reference, seen: &mut BTreeSet<PathBuf>) -> Result<Value> {
    let path = member(root, &reference.path)?;
    require(
        seen.insert(path.clone()),
        "Evidence members may not be reused.",
    )?;
    json_bytes(&read(&path, &reference.sha256, 4 * 1024 * 1024)?)
}

fn canonical_command(phase: &str, toolchain: &str) -> Vec<String> {
    let mut arguments = vec!["cargo".into(), format!("+{toolchain}")];
    let verb = match phase {
        "portable_library" => "check",
        "state_fixture" | "collector_controls" => "test",
        "collector_link" => "build",
        _ => "clippy",
    };
    arguments.extend(
        [
            verb,
            "--locked",
            "--offline",
            "--release",
            "--target",
            TARGET,
        ]
        .map(str::to_owned),
    );
    let package = match phase {
        "neon_helper_analysis" => "bonsai-neon-dot",
        "hash_helper_analysis" => "bonsai-native-hash",
        _ => "prism-inference",
    };
    arguments.extend(["--package", package].map(str::to_owned));
    if !matches!(
        phase,
        "neon_helper_analysis" | "hash_helper_analysis" | "portable_library"
    ) {
        arguments.extend(["--features", "parallel-rows"].map(str::to_owned));
    }
    let suffix: &[&str] = match phase {
        "source_analysis" | "neon_helper_analysis" | "hash_helper_analysis" => {
            &["--all-targets", "--", "-D", "warnings"]
        }
        "collector_analysis" => &["--example", "cpu_candidate_check", "--", "-D", "warnings"],
        "portable_library" => &["--lib", "--no-default-features"],
        "state_fixture" => &[
            "--test",
            "graph",
            STATE_CONTROL,
            "--",
            "--exact",
            "--color",
            "never",
        ],
        "collector_controls" => &["--example", "cpu_candidate_check", "--", "--color", "never"],
        _ => &["--example", "cpu_candidate_check"],
    };
    arguments.extend(suffix.iter().map(|argument| (*argument).to_owned()));
    arguments
}

fn checks(build: &Build) -> Result<()> {
    let phases = [
        "source_analysis",
        "neon_helper_analysis",
        "hash_helper_analysis",
        "portable_library",
        "state_fixture",
        "collector_analysis",
        "collector_controls",
        "collector_link",
    ];
    require(
        build
            .checks
            .iter()
            .map(|check| check.phase.as_str())
            .eq(phases),
        "Missing exact checked-build phase inventory.",
    )?;
    for check in &build.checks {
        require(
            check.exit_code == 0 && check.failed == 0 && check.ignored == 0 && check.measured == 0,
            "Build check failed, skipped or measured controls.",
        )?;
        let args = &check.command;
        require(
            *args == canonical_command(&check.phase, &build.toolchain),
            "Check command differs from the exact executable phase template.",
        )?;
        require(
            !args.is_empty()
                && args.len() <= 64
                && args
                    .iter()
                    .all(|arg| !arg.is_empty() && arg.len() <= 4096 && !arg.contains('\0')),
            "Missing bounded check command.",
        )?;
        let expected: &[&str] = match check.phase.as_str() {
            "collector_controls" => &CONTROLS,
            "state_fixture" => &[STATE_CONTROL],
            _ => &[],
        };
        require(
            check
                .passed
                .iter()
                .map(String::as_str)
                .eq(expected.iter().copied()),
            "Executed positive-control inventory differs.",
        )?;
    }
    Ok(())
}

fn preparation(packet: &Preparation) -> Result<()> {
    require(
        packet.schema_version == 1
            && packet.kind == "prism-cpu-source-comparison-preparation"
            && crate::identity::is_lower_hex(&packet.runner_revision, 40),
        "Unknown current-source preparation identity.",
    )?;
    digest(&packet.evaluator_sha256)?;
    let profile = &packet.profile;
    require(
        matches!(profile.kv_cache.as_str(), "f16" | "k4-v4")
            && profile.context_tokens == 8192
            && matches!(
                (
                    profile.cpu_prefill_tokens,
                    profile.cpu_prefill_scratch_bytes
                ),
                (1, 0) | (8 | 16 | 32, 1..=268_435_456)
            ),
        "Unmatched or unsupported current-source profile.",
    )?;
    require(
        packet.output_tokens == 32
            && packet.model_sha256 == MODEL
            && packet.adapter_sha256 == ADAPTER
            && packet.schedule.iter().map(String::as_str).eq(SCHEDULE),
        "Frozen public workload, artifact or ABBA policy differs.",
    )?;
    require(
        packet
            .arms
            .keys()
            .map(String::as_str)
            .eq(["baseline", "candidate"]),
        "Missing exact source arms.",
    )?;
    let baseline = &packet.arms["baseline"];
    let candidate = &packet.arms["candidate"];
    require(
        baseline.source_revision != candidate.source_revision
            && baseline.compiler == candidate.compiler
            && baseline.toolchain == candidate.toolchain
            && baseline.rustc_sha256 == candidate.rustc_sha256
            && baseline.cargo_sha256 == candidate.cargo_sha256
            && !baseline.compiler.trim().is_empty()
            && baseline.compiler.len() <= 4096,
        "Sources must differ and exact compiler identities must match.",
    )?;
    for build in packet.arms.values() {
        require(
            build.toolchain.starts_with("nightly-")
                && build.toolchain.ends_with("-aarch64-pc-windows-msvc")
                && build.toolchain.len() <= 128
                && build
                    .toolchain
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'),
            "Missing pinned native toolchain identity.",
        )?;
        digest(&build.rustc_sha256)?;
        digest(&build.cargo_sha256)?;
        require(
            build
                .helper_source_sha256
                .keys()
                .map(String::as_str)
                .eq(["bonsai-native-hash", "bonsai-neon-dot"]),
            "Missing exact helper source inventory.",
        )?;
        for hash in build.helper_source_sha256.values() {
            digest(hash)?;
        }
        require(
            crate::identity::is_lower_hex(&build.source_revision, 40)
                && build.clean
                && build.target == TARGET
                && build.features.iter().map(String::as_str).eq(FEATURES)
                && (1..=67_108_864).contains(&build.executable_bytes),
            "Missing clean source/compiler/target/executable binding.",
        )?;
        digest(&build.executable_sha256)?;
        require(
            build
                .inputs
                .keys()
                .map(String::as_str)
                .collect::<BTreeSet<_>>()
                == INPUTS.into_iter().collect(),
            "Build input inventory differs.",
        )?;
        for hash in build.inputs.values() {
            digest(hash)?;
        }
        checks(build)?;
    }
    for name in INPUTS
        .into_iter()
        .filter(|name| name.starts_with("examples/"))
    {
        require(
            baseline.inputs[name] == candidate.inputs[name],
            "Collector source bytes must match across source arms.",
        )?;
    }
    Ok(())
}

fn power(value: &Value) -> Result<(bool, bool)> {
    require(
        value["status"] == "observed" && value["source"] == "GetSystemPowerStatus",
        "Unknown power observation.",
    )?;
    let ac = value["ac_online"].as_bool().ok_or("Unknown AC source.")?;
    let saver = value["battery_saver"]
        .as_bool()
        .ok_or("Unknown battery saver.")?;
    require(
        value.get("battery_percent").is_some_and(|charge| {
            charge.is_null()
                || charge
                    .as_f64()
                    .is_some_and(|charge| charge.is_finite() && (0.0..=100.0).contains(&charge))
        }),
        "Invalid or missing charge observation.",
    )?;
    Ok((ac, saver))
}

fn positive(value: &Value) -> Result<f64> {
    value
        .as_f64()
        .filter(|value| value.is_finite() && *value > 0.0)
        .ok_or_else(|| "Missing positive finite timing.".into())
}

fn arm_receipt(receipt: &Value, started: &Value, packet: &Preparation, arm: &Arm) -> Result<()> {
    let build = &packet.arms[&arm.role];
    let expected = json!({
        "schema_version":2, "kind":"swift-cpu-source-candidate-arm",
        "source_commit_declaration":build.source_revision,
        "model_sha256":MODEL,"model_bytes":7_206_168_928_u64,"weight_representation":"PQ2_0",
        "adapter_sha256":ADAPTER,"adapter_scale_bits":"3fa66666","row_workers":4,
        "backend_requested":"ExperimentalNeon","execution_device":"cpu",
        "execution_basis":"direct-session-without-projection-executor","backend_statistics":null,
        "kv_cache":packet.profile.kv_cache,"context_tokens":8192,
        "cpu_prefill_tokens":packet.profile.cpu_prefill_tokens,
        "cpu_prefill_scratch_bytes":packet.profile.cpu_prefill_scratch_bytes,
        "legacy_f16_build":false,"prompt_reuse":"disabled","seed":42,"thinking_enabled":false,
        "maximum_prompt_tokens":128,"output_tokens_reserved":32,
        "original_deadline_unix_milliseconds":arm.collector_deadline_unix_milliseconds,
        "receipt_reserve_milliseconds":2000,"sampler":"production SwiftSampler and model-declared stops"
    });
    for (key, expected) in expected
        .as_object()
        .ok_or("Missing fixed collector identity.")?
    {
        require(
            receipt.get(key) == Some(expected) && started.get(key) == Some(expected),
            "Started/final collector identity differs from preparation.",
        )?;
    }
    for key in [
        "arm_succeeded",
        "session_succeeded",
        "terminal_delivered",
        "terminal_accounting_matches",
        "model_loaded",
        "output_committed",
    ] {
        require(
            receipt[key] == true && started[key] == false,
            "Collector success/start accounting differs.",
        )?;
    }
    require(
        started["capture_status"] == "incomplete"
            && started["stage"] == "started"
            && receipt["capture_status"] == "complete"
            && receipt["stage"] == "complete"
            && receipt["error"].is_null(),
        "Collector did not complete successfully.",
    )?;
    require(
        receipt["executable_sha256"] == build.executable_sha256
            && receipt["input_sha256"] == packet.messages.sha256
            && receipt["observed_adapter_sha256"] == ADAPTER
            && receipt["effective_row_workers"] == 4
            && receipt["reused_tokens"] == 0,
        "Observed collector/input/adapter/workers differ.",
    )?;
    require(
        receipt["prompt_tokens"]
            .as_u64()
            .is_some_and(|count| (1..=128).contains(&count))
            && receipt["generated_tokens"]
                .as_u64()
                .is_some_and(|count| (1..=32).contains(&count))
            && matches!(receipt["finish"].as_str(), Some("stop" | "length")),
        "Invalid prompt/output/terminal accounting.",
    )?;
    let text = receipt["output_text"]
        .as_str()
        .filter(|text| !text.trim().is_empty() && text.len() <= 65_536)
        .ok_or("Missing complete output text.")?;
    require(
        receipt["output_bytes"].as_u64() == u64::try_from(text.len()).ok()
            && receipt["output_sha256"] == crate::identity::sha256(text.as_bytes()),
        "Output bytes or digest differ.",
    )?;
    let load = positive(&receipt["load_milliseconds"])?;
    let prepare = positive(&receipt["preparation_milliseconds"])?;
    let request = positive(&receipt["request_milliseconds"])?;
    require(
        load + prepare + request <= positive(&receipt["elapsed_milliseconds"])? + 1.0,
        "Collector phases escape their enclosing clock.",
    )?;
    Ok(())
}

fn evaluate(packet: &Preparation, owner: &Owner, owner_root: &Path) -> Result<Value> {
    require(
        owner.schema_version == 1
            && owner.kind == "prism-cpu-source-comparison-owner"
            && owner.workload_predeclared
            && owner.completed
            && owner.budget_seconds == 600
            && (301..=600).contains(&owner.estimated_seconds)
            && !owner.fixed_work.trim().is_empty()
            && owner.fixed_work.len() <= 4096
            && !owner.budget_reason.trim().is_empty()
            && owner.budget_reason.len() <= 4096
            && (1..=1_000_000_000_000).contains(&owner.clock_frequency)
            && owner.arms.len() == 4,
        "The complete predeclared ten-minute ABBA workload is required.",
    )?;
    let frequency = u128::from(owner.clock_frequency);
    require(
        u128::from(owner.deadline_tick) == u128::from(owner.started_tick) + 600 * frequency
            && owner.started_tick < owner.retention_completed_tick
            && owner.retention_completed_tick <= owner.deadline_tick
            && owner.original_deadline_unix_milliseconds > 0,
        "Work/retention escaped its original aggregate clock.",
    )?;
    let expected_power = power(&owner.power_before)?;
    require(
        power(&owner.power_after)? == expected_power,
        "Aggregate power changed.",
    )?;
    let mut previous = owner.started_tick;
    let mut seen = BTreeSet::new();
    let mut reference_conditions: Option<Value> = None;
    let mut outputs = Vec::new();
    let mut process_times = Vec::new();
    let mut owned_times = Vec::new();
    let mut request_times = Vec::new();
    for (index, arm) in owner.arms.iter().enumerate() {
        require(
            arm.id == format!("arm-{}", index + 1)
                && arm.role == SCHEDULE[index]
                && previous <= arm.started_tick
                && arm.started_tick < arm.execution_deadline_tick
                && arm.execution_deadline_tick <= owner.deadline_tick
                && arm.started_tick < arm.cleanup_completed_tick
                && arm.cleanup_completed_tick <= owner.retention_completed_tick,
            "Arm ordering, deadlines or cleanup differ.",
        )?;
        previous = arm.cleanup_completed_tick;
        require(
            u128::from(arm.execution_deadline_tick - arm.started_tick) <= 60 * frequency
                && arm.cleanup_succeeded
                && arm.owned_job_empty,
            "Arm exceeded sixty seconds or cleanup is unknown.",
        )?;
        let offset =
            u128::from(owner.deadline_tick - arm.execution_deadline_tick) * 1000 / frequency;
        require(
            u128::from(arm.collector_deadline_unix_milliseconds) + offset
                == u128::from(owner.original_deadline_unix_milliseconds),
            "Collector deadline was not derived from the original aggregate clock.",
        )?;
        require(
            power(&arm.power_before)? == expected_power
                && power(&arm.power_after)? == expected_power,
            "Unknown or mismatched arm power.",
        )?;
        let scratch = packet.profile.cpu_prefill_scratch_bytes;
        require(
            arm.minimum_admission_bytes == 16_642_998_272 + scratch
                && arm.maximum_job_memory_bytes == 12_348_030_976 + scratch,
            "Full prefill scratch was not charged to admission and Job limits.",
        )?;
        let receipt = referenced(owner_root, &arm.receipt, &mut seen)?;
        let started = referenced(owner_root, &arm.started_receipt, &mut seen)?;
        let supervisor = referenced(owner_root, &arm.supervision, &mut seen)?;
        arm_receipt(&receipt, &started, packet, arm)?;
        require(
            supervisor["Status"] == "completed"
                && supervisor["ExitCode"] == 0
                && supervisor["ModelProcessStarted"] == true
                && supervisor
                    .get("CancellationFailure")
                    .is_none_or(Value::is_null),
            "Native supervision failed or cancellation was incomplete.",
        )?;
        require(
            supervisor["MinimumAvailablePhysicalBytes"]
                .as_u64()
                .is_some_and(|bytes| bytes >= 4_294_967_296)
                && supervisor["PeakJobMemoryBytes"]
                    .as_u64()
                    .is_some_and(|bytes| (1..=arm.maximum_job_memory_bytes).contains(&bytes)),
            "Missing native physical reserve/Job accounting.",
        )?;
        for boundary in ["Before", "After"] {
            let observation = &supervisor["CpuConditions"][boundary];
            let baseline = reference_conditions.get_or_insert_with(|| observation.clone());
            let comparison = super::conditions::compare(
                json!({"operation":"compare-conditions", "before":baseline,"after":observation}),
            )?;
            require(
                comparison["Comparable"] == true
                    && power(&observation["power_snapshot"])? == expected_power,
                "Native host/topology/power conditions are unknown or changed.",
            )?;
            require(observation["host_identity"]["processor_name"].as_str().is_some_and(|name| bonsai_cpu_policy::hardware::SnapdragonFamily::from_processor_name(name).is_some_and(|family| matches!(family, bonsai_cpu_policy::hardware::SnapdragonFamily::X2Elite | bonsai_cpu_policy::hardware::SnapdragonFamily::X2EliteExtreme))), "Current-source comparison requires observed X2 identity.")?;
        }
        let owned = milliseconds(
            arm.cleanup_completed_tick - arm.started_tick,
            owner.clock_frequency,
        )?;
        let process = positive(&supervisor["WallMilliseconds"])?;
        require(
            positive(&receipt["elapsed_milliseconds"])? <= process + 1.0
                && process <= owned + 1.0
                && process
                    <= milliseconds(
                        arm.execution_deadline_tick - arm.started_tick,
                        owner.clock_frequency,
                    )? + 1.0,
            "Collector, process and owned-arm clocks disagree.",
        )?;
        let output = json!([
            receipt["output_text"],
            receipt["output_sha256"],
            receipt["output_bytes"],
            receipt["prompt_tokens"],
            receipt["generated_tokens"],
            receipt["finish"]
        ]);
        require(
            outputs.first().is_none_or(|first| first == &output),
            "Output, rendered prompt counts or finish differ across source arms.",
        )?;
        outputs.push(output);
        process_times.push(process);
        owned_times.push(owned);
        request_times.push(
            positive(&receipt["preparation_milliseconds"])?
                + positive(&receipt["request_milliseconds"])?,
        );
    }
    let process = ratios(&process_times);
    let owned = ratios(&owned_times);
    Ok(
        json!({"schema_version":1,"kind":"prism-cpu-source-comparison","status":"admitted",
        "short_request_benefit_observed":process.iter().all(|ratio| *ratio <= 0.90) && owned.iter().all(|ratio| *ratio <= 1.10),
        "candidate_over_baseline_process_wall_ratios":process,"candidate_over_baseline_owned_arm_ratios":owned,
        "candidate_over_baseline_request_ratios":ratios(&request_times),"pairs":2,
        "complete_answer_observed":outputs.iter().all(|output| output[5] == "stop"),
        "continuous_power_stability_proven":false,"promotion_qualified":false,"benefit_gate_qualified":false,
        "default_may_change":false,"individual_optimization_verdicts":false,"long_prefill_qualified":false,
        "scope":"Two reversed bounded short-request source pairs with identical cache/tile/scratch. Build producer authentication, complete state/logits/adapters, quality, sustained use and promotion remain independent gates."}),
    )
}

fn milliseconds(ticks: u64, frequency: u64) -> Result<f64> {
    // serde's numerical conversion avoids unchecked integer-to-float casts.
    Ok(positive(&json!(ticks))? * 1000.0 / positive(&json!(frequency))?)
}

fn ratios(values: &[f64]) -> [f64; 2] {
    [values[1] / values[0], values[2] / values[3]]
}

pub(super) fn run(value: Value) -> Result<Value> {
    let request: Request = serde_json::from_value(value).map_err(|error| error.to_string())?;
    require(
        request.operation == "evaluate-source-comparison",
        "Unsupported source comparison operation.",
    )?;
    let packet_bytes = read(
        &request.preparation_path,
        &request.preparation_sha256,
        4 * 1024 * 1024,
    )?;
    let packet: Preparation =
        serde_json::from_value(json_bytes(&packet_bytes)?).map_err(|error| error.to_string())?;
    preparation(&packet)?;
    let evaluator = std::env::current_exe().map_err(|error| error.to_string())?;
    let _ = read(&evaluator, &packet.evaluator_sha256, 268_435_456)?;
    let root = request
        .preparation_path
        .parent()
        .ok_or("Missing preparation directory.")?;
    let messages = read(
        &member(root, &packet.messages.path)?,
        &packet.messages.sha256,
        65_536,
    )?;
    let expected =
        json!([{"role":"user","content":"Count from 1 to 10. Answer with only the numbers."}]);
    require(
        json_bytes(&messages)? == expected,
        "Public comparison workload changed.",
    )?;
    let owner: Owner = serde_json::from_value(json_bytes(&read(
        &request.owner_path,
        &request.owner_sha256,
        4 * 1024 * 1024,
    )?)?)
    .map_err(|error| error.to_string())?;
    require(
        owner.preparation_sha256 == request.preparation_sha256,
        "Owner references another preparation.",
    )?;
    evaluate(
        &packet,
        &owner,
        request
            .owner_path
            .parent()
            .ok_or("Missing owner directory.")?,
    )
}

struct Unique(Value);

impl<'de> Deserialize<'de> for Unique {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = Unique;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("JSON without duplicate object fields")
            }
            fn visit_bool<E: serde::de::Error>(
                self,
                value: bool,
            ) -> std::result::Result<Unique, E> {
                Ok(Unique(json!(value)))
            }
            fn visit_i64<E: serde::de::Error>(self, value: i64) -> std::result::Result<Unique, E> {
                Ok(Unique(json!(value)))
            }
            fn visit_u64<E: serde::de::Error>(self, value: u64) -> std::result::Result<Unique, E> {
                Ok(Unique(json!(value)))
            }
            fn visit_f64<E: serde::de::Error>(self, value: f64) -> std::result::Result<Unique, E> {
                if !value.is_finite() {
                    return Err(E::custom("Nonfinite JSON number."));
                }
                Ok(Unique(json!(value)))
            }
            fn visit_str<E: serde::de::Error>(self, value: &str) -> std::result::Result<Unique, E> {
                Ok(Unique(json!(value)))
            }
            fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Unique, E> {
                Ok(Unique(Value::Null))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut access: A,
            ) -> std::result::Result<Unique, A::Error> {
                let mut values = Vec::new();
                while let Some(Unique(value)) = access.next_element()? {
                    values.push(value);
                }
                Ok(Unique(Value::Array(values)))
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut access: A,
            ) -> std::result::Result<Unique, A::Error> {
                let mut fields = serde_json::Map::new();
                while let Some((name, Unique(value))) = access.next_entry::<String, Unique>()? {
                    if fields.insert(name, value).is_some() {
                        return Err(serde::de::Error::custom("Duplicate JSON field."));
                    }
                }
                Ok(Unique(Value::Object(fields)))
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}
