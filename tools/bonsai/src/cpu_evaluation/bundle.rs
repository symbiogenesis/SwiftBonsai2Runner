//! Schema-2 registry CPU bundle replay. Historical packets retain their identities.

use crate::Result;
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs,
    io::Read as _,
    path::{Component, Path, PathBuf},
};

mod archive;
#[cfg(windows)]
mod owner;
mod prepare;
#[cfg(test)]
mod tests;

pub(super) const BASELINE: &str = "055c7430cb4771dfddb24ddaa30494af6ad182da";
pub(super) const REVISION: &str = "99bab71270b1e6a6fb6d47eec2e34b8097cdcd4c";
pub(super) const MODEL: &str = "5912bb739217cf25b4283a098c7b893d9baff36bdf2e72d3dbf4cb999d5633d5";
pub(super) const ADAPTER: &str = "f1669534803d340a496015f5c45125f3437b4d13ec764f40e34488ce83967f42";
pub(super) const SCHEDULE: [&str; 4] = ["baseline", "candidate", "candidate", "baseline"];
const PHASES: [&str; 6] = [
    "source_analysis",
    "portable_library",
    "state_fixture",
    "collector_analysis",
    "collector_controls",
    "collector_link",
];
const CONTROLS: [&str; 7] = [
    "tests::completion_requires_matching_terminal_runtime_counts_and_nonempty_output",
    "tests::deadlines_require_original_bounded_absolute_time",
    "tests::failure_receipt_retains_provisional_output_without_committing_or_overwriting",
    "tests::first_text_event_timing_ignores_empty_failed_and_subsequent_deliveries",
    "tests::output_rejects_overflow_and_any_delivery_after_terminal",
    "tests::records_hash_exact_utf8_and_keep_unentered_counts_unknown",
    "tests::selection_rejects_relative_paths_bad_commit_and_output_overflow",
];
const PACKAGES: [(&str, &str, &str); 6] = [
    (
        "prism-inference",
        "0.1.25",
        "0a117db7589a821b27c224e41aaababa5ddd2ac9852344c0a8121a0849046363",
    ),
    (
        "bonsai-neon-dot",
        "0.1.2",
        "f932c3309ef3645b055b91bdd9ee6986b7ef98c24bc1831701138e3ab9d3a31b",
    ),
    (
        "bonsai-native-hash",
        "0.1.3",
        "315284b1f3fc7e98f2cb0f749e63690853f1c441d2b3a3eec2ad68abe45180a2",
    ),
    (
        "bonsai-ternary",
        "0.1.3",
        "f6ea8b95000a131bb48e27453b776e9e42096194648a3a8ebde782d5cfe9f604",
    ),
    (
        "bonsai-gguf",
        "0.1.5",
        "fdd15a1ed0911fca0d987c349c3ffa02078b729b05148f46e78a285f89189b4b",
    ),
    (
        "bonsai-prism",
        "0.1.5",
        "9ecabe1b5c626d4ceaef666b27f11c5a906dea56234252d3cbb8f46125d976ea",
    ),
];

fn require(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}
fn integer(value: &Value, minimum: u64, maximum: u64) -> Result<u64> {
    value
        .as_u64()
        .filter(|value| (minimum..=maximum).contains(value))
        .ok_or_else(|| "Missing or out-of-range integer.".into())
}
fn positive(value: &Value) -> Result<f64> {
    value
        .as_f64()
        .filter(|value| value.is_finite() && *value > 0.0)
        .ok_or_else(|| "Missing positive finite observation.".into())
}
fn float(value: u64) -> Result<f64> {
    positive(&json!(value))
}
fn hex(value: &Value, length: usize) -> Result<&str> {
    value
        .as_str()
        .filter(|text| crate::identity::is_lower_hex(text, length))
        .ok_or_else(|| "Invalid lowercase identity.".into())
}
fn object(value: &Value) -> Result<&serde_json::Map<String, Value>> {
    value.as_object().ok_or_else(|| "Expected object.".into())
}
fn array(value: &Value) -> Result<&Vec<Value>> {
    value.as_array().ok_or_else(|| "Expected array.".into())
}
fn keys(value: &Value, expected: &[&str]) -> Result<()> {
    let names: BTreeSet<_> = object(value)?.keys().map(String::as_str).collect();
    require(
        names == expected.iter().copied().collect(),
        "Object field inventory differs.",
    )
}
fn matches(value: &Value, expected: &Value) -> Result<()> {
    require(
        value == expected,
        "Identity, settings or completion differs.",
    )
}
fn subset(value: &Value, expected: &Value) -> Result<()> {
    for (key, item) in object(expected)? {
        matches(&value[key], item)?;
    }
    Ok(())
}
pub(super) fn profile() -> Value {
    json!({"adapter_scale_bits":"3fa66666","row_workers":4,"backend_requested":"ExperimentalNeon","execution_device":"cpu","execution_basis":"direct-session-without-projection-executor","backend_statistics":null,"kv_cache":"f16","prompt_reuse":"disabled","seed":42,"thinking_enabled":false,"maximum_prompt_tokens":128})
}
pub(super) fn messages() -> Value {
    json!([{"role":"user","content":"Count from 1 to 10. Answer with only the numbers."}])
}

fn reject_redirects(path: &Path) -> Result<()> {
    require(path.is_absolute(), "Evidence path must be absolute.")?;
    for (depth, ancestor) in path.ancestors().enumerate() {
        require(depth < 65, "Path ancestry exceeds bound.")?;
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) => {
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt as _;
                    require(
                        metadata.file_attributes() & 0x400 == 0,
                        "Redirected evidence/source path.",
                    )?;
                }
                require(
                    !metadata.file_type().is_symlink(),
                    "Redirected evidence/source path.",
                )?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(())
}
fn member(root: &Path, relative: &Value) -> Result<PathBuf> {
    let name = relative
        .as_str()
        .filter(|name| !name.is_empty() && !name.contains([':', '\\', '\0']))
        .ok_or("Invalid relative member.")?;
    let path = Path::new(name);
    require(
        !path.is_absolute()
            && path
                .components()
                .all(|component| matches!(component, Component::Normal(_))),
        "Evidence member escapes root.",
    )?;
    let result = root.join(path);
    reject_redirects(&result)?;
    Ok(result)
}
fn bytes(path: &Path, maximum: u64) -> Result<Vec<u8>> {
    reject_redirects(path)?;
    let file = crate::build_tools::input_file(path)?;
    require(
        file.metadata().map_err(|error| error.to_string())?.len() <= maximum,
        "Input exceeds pre-read bound.",
    )?;
    crate::bounded_input::read(file, maximum)
        .map_err(|error| error.into_message("Input grew beyond bound."))
}
fn json_bytes(raw: &[u8]) -> Result<Value> {
    super::source_comparison::json_bytes(raw.strip_prefix(&[239, 187, 191]).unwrap_or(raw))
}
fn read(path: &Path) -> Result<Value> {
    let value = json_bytes(&bytes(path, 4_194_304)?)?;
    object(&value)?;
    Ok(value)
}
fn digest(path: &Path) -> Result<String> {
    reject_redirects(path)?;
    let mut file = crate::build_tools::input_file(path)?;
    let mut hasher = crate::identity::Hasher::new()?;
    let mut buffer = vec![0; 65_536].into_boxed_slice();
    loop {
        let count = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count])?;
    }
    hasher.finalize_hex()
}
fn checked(root: &Path, reference: &Value, seen: &mut BTreeSet<PathBuf>) -> Result<Value> {
    keys(reference, &["path", "sha256"])?;
    let path = member(root, &reference["path"])?;
    require(
        seen.insert(path.clone()),
        "Missing or reused arm evidence path.",
    )?;
    matches(&json!(digest(&path)?), &reference["sha256"])?;
    hex(&reference["sha256"], 64)?;
    read(&path)
}
fn power(value: &Value) -> Result<(bool, bool)> {
    subset(
        value,
        &json!({"status":"observed","source":"GetSystemPowerStatus"}),
    )?;
    let ac = value["ac_online"].as_bool().ok_or("Unknown AC source.")?;
    let saver = value["battery_saver"]
        .as_bool()
        .ok_or("Unknown battery saver.")?;
    require(
        value["battery_percent"].is_null()
            || value["battery_percent"]
                .as_f64()
                .is_some_and(|v| v.is_finite() && (0.0..=100.0).contains(&v)),
        "Invalid battery charge.",
    )?;
    Ok((ac, saver))
}
fn native_conditions(value: &Value, host: &Value) -> Result<Value> {
    subset(
        value,
        &json!({"schema_version":1,"kind":"runner-native-cpu-observations","status":"observed","architecture":"aarch64","scope":"current-process","source":"GetSystemCpuSetInformation"}),
    )?;
    matches(&value["host_identity"], host)?;
    subset(
        host,
        &json!({"status":"observed","source":crate::host_cpu::HOST_IDENTITY_SOURCE}),
    )?;
    for name in [
        "processor_name",
        "processor_identifier",
        "vendor_identifier",
    ] {
        require(
            host[name]
                .as_str()
                .is_some_and(|v| !v.trim().is_empty() && !v.trim().eq_ignore_ascii_case("unknown")),
            "Incomplete native host identity.",
        )?;
    }
    require(
        host["processor_name"]
            .as_str()
            .is_some_and(|v| v.contains("Snapdragon") && v.contains("X2")),
        "Fixed comparison requires selected X2 family.",
    )?;
    let version = array(&host["windows_version"])?;
    require(version.len() == 3, "Missing native kernel version.")?;
    for item in version {
        integer(item, 0, u64::from(u32::MAX))?;
    }
    integer(&version[0], 1, u64::from(u32::MAX))?;
    integer(&version[2], 1, u64::from(u32::MAX))?;
    let features = array(&host["processor_features"])?;
    require(features.len() <= 256, "Excessive native features.")?;
    let mut previous = None;
    for feature in features {
        let next = integer(feature, 0, 255)?;
        require(
            previous.is_none_or(|p| p < next),
            "Feature inventory unsorted or repeated.",
        )?;
        previous = Some(next);
    }
    integer(&value["topology"]["logical_processors"], 4, 4096)?;
    require(
        !array(&value["topology"]["cpu_sets"])?.is_empty(),
        "Missing CPU topology.",
    )?;
    let policy = &value["power_policy_snapshot"];
    subset(
        policy,
        &json!({"status":"observed","source":"PowerGetActiveScheme/PowerRegisterForEffectivePowerModeNotifications","notification_version":2}),
    )?;
    let scheme = policy["active_scheme"]
        .as_str()
        .ok_or("Missing active scheme.")?;
    require(
        scheme.len() == 36
            && scheme != "00000000-0000-0000-0000-000000000000"
            && scheme.bytes().enumerate().all(|(i, b)| {
                if [8, 13, 18, 23].contains(&i) {
                    b == b'-'
                } else {
                    b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
                }
            }),
        "Invalid active scheme.",
    )?;
    require(
        policy["effective_mode"].as_str().is_some_and(|v| {
            [
                "battery-saver",
                "better-battery",
                "balanced",
                "high-performance",
                "max-performance",
                "game-mode",
                "mixed-reality",
            ]
            .contains(&v)
        }),
        "Unknown effective power mode.",
    )?;
    let observed = power(&value["power_snapshot"])?;
    Ok(
        json!({"host":host,"topology":value["topology"],"power":[observed.0,observed.1],"scheme":scheme,"mode":policy["effective_mode"]}),
    )
}

#[expect(
    clippy::too_many_lines,
    reason = "Keep historical schema-2 identities and required control inventory in one auditable admission gate."
)]
fn validate_preparation(packet: &Value, path: &Path) -> Result<()> {
    subset(
        packet,
        &json!({"schema_version":2,"kind":"prism-cpu-bundle-preparation","target":"aarch64-pc-windows-msvc","features":["cli","experimental-neon","parallel-rows"],"schedule":SCHEDULE,"profile":profile(),"per_arm_timeout_seconds":60,"model_sha256":MODEL,"adapter_sha256":ADAPTER,"model_executed":false,"promotion_qualified":false,"evaluator_language":"PowerShell 7.6"}),
    )?;
    let root = path.parent().ok_or("Missing packet directory.")?;
    let messages_path = member(root, &packet["messages"]["path"])?;
    matches(
        &json!(digest(&messages_path)?),
        &packet["messages"]["sha256"],
    )?;
    matches(
        &json_bytes(&bytes(&messages_path, 4_194_304)?)?,
        &messages(),
    )?;
    matches(
        &json!(digest(&root.join("evaluator.ps1"))?),
        &packet["evaluator_sha256"],
    )?;
    native_evaluator(packet, root)?;
    let sources = object(&packet["collector_sources"])?;
    require(sources.len() >= 2, "Missing collector source identities.")?;
    for (name, hash) in sources {
        require(
            name == "examples/cpu_bundle_check.rs"
                || (name.starts_with("examples/cpu_bundle_check/")
                    && Path::new(name)
                        .extension()
                        .is_some_and(|extension| extension == "rs")
                    && !name.contains(':')),
            "Unreviewed collector member.",
        )?;
        hex(hash, 64)?;
    }
    keys(&packet["arms"], &["baseline", "candidate"])?;
    matches(
        &packet["arms"]["baseline"]["source"]["revision"],
        &json!(BASELINE),
    )?;
    for role in ["baseline", "candidate"] {
        let prepared = &packet["arms"][role];
        hex(&prepared["source"]["revision"], 40)?;
        hex(&prepared["executable"]["sha256"], 64)?;
        matches(&prepared["source"]["clean"], &json!(true))?;
        matches(&prepared["legacy_f16_build"], &json!(role == "baseline"))?;
        let checks = array(&prepared["checks"])?;
        require(
            checks.len() == PHASES.len(),
            "Incomplete preparation checks.",
        )?;
        for (check, phase) in checks.iter().zip(PHASES) {
            matches(&check["phase"], &json!(phase))?;
            integer(&check["exit_code"], 0, 0)?;
            require(
                !array(&check["command"])?.is_empty(),
                "Missing preparation command.",
            )?;
            let names = match phase {
                "state_fixture" => Some(vec![
                    "graph_parallel_rows_preserve_logits_checkpoints_and_recovery",
                ]),
                "collector_controls" => Some(CONTROLS.to_vec()),
                _ => None,
            };
            if let Some(names) = names {
                let summary = &check["test_summary"];
                for field in ["failed", "ignored", "measured"] {
                    integer(&summary[field], 0, 0)?;
                }
                matches(&summary["passed"], &json!(names.len()))?;
                matches(&summary["names"], &json!(names))?;
            }
            if phase == "source_analysis" {
                let helpers = array(&check["helper_checks"])?;
                require(helpers.len() == 2, "Missing helper analysis.")?;
                for (helper, name) in helpers
                    .iter()
                    .zip(["bonsai-neon-dot", "bonsai-native-hash"])
                {
                    matches(&helper["package"], &json!(name))?;
                    integer(&helper["exit_code"], 0, 0)?;
                    require(
                        !array(&helper["command"])?.is_empty(),
                        "Missing helper command.",
                    )?;
                    hex(&helper["manifest_sha256"], 64)?;
                    hex(&helper["lock_sha256"], 64)?;
                }
            }
        }
    }
    subset(
        &packet["arms"]["candidate"]["source"],
        &json!({"kind":"registry","revision":REVISION,"version":"0.1.25","archive_sha256":PACKAGES[0].2}),
    )?;
    keys(&packet["registry_packages"], &PACKAGES.map(|item| item.0))?;
    for (name, version, hash) in PACKAGES {
        subset(
            &packet["registry_packages"][name],
            &json!({"name":name,"version":version,"sha256":hash,"revision":REVISION}),
        )?;
    }
    integer(&packet["output_tokens"], 1, 32)?;
    hex(&packet["messages"]["sha256"], 64)?;
    Ok(())
}

fn native_evaluator(packet: &Value, root: &Path) -> Result<()> {
    if let Some(native) = packet.get("native_evaluator") {
        keys(
            native,
            &[
                "path",
                "bytes",
                "sha256",
                "source_revision",
                "language",
                "format",
            ],
        )?;
        subset(
            native,
            &json!({"path":"native-evaluator.exe","language":"Rust","format":"bonsai-tools-cpu-bundle-v1"}),
        )?;
        matches(
            &packet["retained_files"],
            &json!([
                "preparation.json",
                "messages.json",
                "evaluator.ps1",
                "native-evaluator.exe"
            ]),
        )?;
        matches(
            &native["source_revision"],
            &packet["orchestrator"]["revision"],
        )?;
        matches(
            &native["source_revision"],
            &packet["orchestrator"]["source"]["revision"],
        )?;
        hex(&native["source_revision"], 40)?;
        let path = member(root, &native["path"])?;
        let length = integer(&native["bytes"], 1, 268_435_456)?;
        require(
            fs::metadata(&path)
                .map_err(|error| error.to_string())?
                .len()
                == length,
            "Retained native evaluator extent differs.",
        )?;
        let hash = hex(&native["sha256"], 64)?;
        require(
            digest(&path)? == hash
                && digest(&std::env::current_exe().map_err(|error| error.to_string())?)? == hash,
            "Native replay requires the exact retained evaluator executable.",
        )?;
    } else {
        // Historical packets retain their complete original three-file replay
        // inventory and are never silently assigned a new executable identity.
        matches(
            &packet["retained_files"],
            &json!(["preparation.json", "messages.json", "evaluator.ps1"]),
        )?;
    }
    Ok(())
}

fn validate_arm(
    receipt: &Value,
    owner: &Value,
    packet: &Value,
    role: &str,
    deadline: u64,
) -> Result<()> {
    subset(receipt, &profile())?;
    subset(
        receipt,
        &json!({"schema_version":1,"kind":"swift-cpu-bundle-arm","source_commit_declaration":packet["arms"][role]["source"]["revision"],"executable_sha256":packet["arms"][role]["executable"]["sha256"],"input_sha256":packet["messages"]["sha256"],"legacy_f16_build":role=="baseline","output_tokens_reserved":packet["output_tokens"],"original_deadline_unix_milliseconds":deadline,"capture_status":"complete","arm_succeeded":true,"session_succeeded":true,"terminal_delivered":true,"stage":"complete","terminal_accounting_matches":true,"reused_tokens":0,"model_sha256":owner["model_sha256"],"adapter_sha256":owner["adapter_sha256"],"model_bytes":7_206_168_928u64,"weight_representation":"PQ2_0","effective_row_workers":4,"observed_adapter_sha256":owner["adapter_sha256"],"model_loaded":true,"output_committed":true,"receipt_reserve_milliseconds":2000,"sampler":"production SwiftSampler and model-declared stops"}),
    )?;
    integer(&receipt["prompt_tokens"], 1, 128)?;
    integer(
        &receipt["generated_tokens"],
        1,
        integer(&packet["output_tokens"], 1, 32)?,
    )?;
    require(
        receipt["finish"] == "stop" || receipt["finish"] == "length",
        "Missing terminal reason.",
    )?;
    let text = receipt["output_text"]
        .as_str()
        .filter(|text| !text.trim().is_empty())
        .ok_or("Missing complete returned text.")?;
    integer(&receipt["output_bytes"], 1, 65_536)?;
    matches(&receipt["output_bytes"], &json!(text.len()))?;
    matches(
        &receipt["output_sha256"],
        &json!(crate::identity::sha256(text.as_bytes())),
    )?;
    let load = positive(&receipt["load_milliseconds"])?;
    let prepare = positive(&receipt["preparation_milliseconds"])?;
    let request = positive(&receipt["request_milliseconds"])?;
    require(
        load + prepare + request <= positive(&receipt["elapsed_milliseconds"])?,
        "Operation durations exceed enclosing clock.",
    )?;
    if !receipt["first_text_delta_milliseconds"].is_null() {
        require(
            positive(&receipt["first_text_delta_milliseconds"])? <= request,
            "First text callback outside request.",
        )?;
    }
    if !receipt["generation_phases"].is_null() {
        let phases = array(&receipt["generation_phases"])?;
        require(!phases.is_empty(), "Empty phase telemetry.")?;
        let mut seen = BTreeSet::new();
        for phase in phases {
            let name = phase["phase"]
                .as_str()
                .filter(|name| !name.is_empty())
                .ok_or("Missing phase name.")?;
            require(seen.insert(name), "Repeated phase.")?;
            require(
                phase["milliseconds"]
                    .as_f64()
                    .is_some_and(|v| v.is_finite() && v >= 0.0),
                "Invalid phase time.",
            )?;
            let attempted = integer(&phase["attempted_calls"], 0, i64::MAX.unsigned_abs())?;
            integer(&phase["completed_calls"], 0, attempted)?;
            let tokens = integer(&phase["attempted_tokens"], 0, i64::MAX.unsigned_abs())?;
            integer(&phase["completed_tokens_lower_bound"], 0, tokens)?;
            require(
                phase["partial_progress_unknown"].is_boolean(),
                "Missing partial progress observation.",
            )?;
        }
    }
    Ok(())
}

#[expect(
    clippy::too_many_lines,
    reason = "Replay keeps the original aggregate clock, four arm clocks, matched conditions and output reconciliation together."
)]
fn evaluate(packet: &Value, owner: &Value, path: &Path) -> Result<Value> {
    subset(
        owner,
        &json!({"schema_version":1,"kind":"prism-cpu-bundle-owner","workload_predeclared":true,"completed":true,"model_sha256":MODEL,"adapter_sha256":ADAPTER}),
    )?;
    require(
        !object(&owner["host_identity"])?.is_empty(),
        "Missing host identity.",
    )?;
    let budget = integer(&owner["budget_seconds"], 1, 900)?;
    let frequency = integer(&owner["clock_frequency"], 1, 1_000_000_000_000)?;
    let begin = integer(&owner["started_tick"], 0, i64::MAX.unsigned_abs())?;
    let deadline = integer(&owner["deadline_tick"], 1, i64::MAX.unsigned_abs())?;
    let retained = integer(
        &owner["retention_completed_tick"],
        1,
        i64::MAX.unsigned_abs(),
    )?;
    require(
        u128::from(deadline) == u128::from(begin) + u128::from(budget) * u128::from(frequency)
            && begin < retained
            && retained <= deadline,
        "Work or retention escaped original aggregate allowance.",
    )?;
    let source_deadline = integer(
        &owner["original_deadline_unix_milliseconds"],
        1,
        i64::MAX.unsigned_abs(),
    )?;
    let expected_power = power(&owner["power_before"])?;
    require(
        power(&owner["power_after"])? == expected_power,
        "Unknown or mismatched aggregate power.",
    )?;
    let arms = array(&owner["arms"])?;
    require(arms.len() == 4, "Complete ABBA workload required.")?;
    let root = path.parent().ok_or("Missing owner directory.")?;
    let mut seen = BTreeSet::new();
    let mut previous = begin;
    let mut native = None;
    let mut output = None;
    let mut observations = Vec::new();
    let mut times = Vec::new();
    let mut complete = true;
    for (index, (arm, role)) in arms.iter().zip(SCHEDULE).enumerate() {
        subset(
            arm,
            &json!({"id":format!("arm-{}",index+1),"role":role,"host_identity":owner["host_identity"],"owned_job_empty":true,"cleanup_succeeded":true}),
        )?;
        let started = integer(&arm["started_tick"], previous, retained)?;
        let ended = integer(&arm["cleanup_completed_tick"], started + 1, retained)?;
        previous = ended;
        let execution = integer(&arm["execution_deadline_tick"], started + 1, deadline)?;
        let collector = integer(
            &arm["collector_deadline_unix_milliseconds"],
            1,
            i64::MAX.unsigned_abs(),
        )?;
        let offset = u128::from(deadline - execution) * 1000 / u128::from(frequency);
        require(
            execution - started <= 60 * frequency
                && (i128::from(source_deadline)
                    - i128::try_from(offset).map_err(|e| e.to_string())?
                    - i128::from(collector))
                .abs()
                    <= 2,
            "Arm deadline not derived from original clock.",
        )?;
        require(
            power(&arm["power_before"])? == expected_power
                && power(&arm["power_after"])? == expected_power,
            "Unknown or mismatched arm power.",
        )?;
        let receipt = checked(root, &arm["receipt"], &mut seen)?;
        let start = checked(root, &arm["started_receipt"], &mut seen)?;
        let supervisor = checked(root, &arm["supervision"], &mut seen)?;
        subset(
            &start,
            &json!({"source_commit_declaration":receipt["source_commit_declaration"],"original_deadline_unix_milliseconds":collector}),
        )?;
        subset(
            &supervisor,
            &json!({"Status":"completed","ModelProcessStarted":true,"ExitCode":0}),
        )?;
        require(
            supervisor
                .get("CancellationFailure")
                .is_none_or(|value| value.is_null() || value == false || value == ""),
            "Native cancellation failed.",
        )?;
        for label in ["Before", "After"] {
            let snapshot =
                native_conditions(&supervisor["CpuConditions"][label], &owner["host_identity"])?;
            require(
                native.as_ref().is_none_or(|value| value == &snapshot)
                    && snapshot["power"] == json!([expected_power.0, expected_power.1]),
                "Native CPU/effective power changed.",
            )?;
            native = Some(snapshot);
        }
        let wall = positive(&supervisor["WallMilliseconds"])?;
        let elapsed = float(ended - started)? * 1000.0 / float(frequency)?;
        require(
            wall <= float(execution - started)? * 1000.0 / float(frequency)? + 1.0,
            "Observed wall exceeded deadline.",
        )?;
        let minimum = integer(
            &supervisor["MinimumAvailablePhysicalBytes"],
            4_294_967_296,
            i64::MAX.unsigned_abs(),
        )?;
        let peak = integer(&supervisor["PeakJobMemoryBytes"], 1, 12_348_030_976)?;
        validate_arm(&receipt, owner, packet, role, collector)?;
        require(
            positive(&receipt["elapsed_milliseconds"])? <= wall + 1.0 && wall <= elapsed + 1.0,
            "Collector/supervisor/cleanup clocks inconsistent.",
        )?;
        let returned = json!([
            receipt["output_text"],
            receipt["output_sha256"],
            receipt["output_bytes"],
            receipt["prompt_tokens"],
            receipt["generated_tokens"],
            receipt["finish"]
        ]);
        require(
            output.as_ref().is_none_or(|value| value == &returned),
            "Output/counts/terminal reason differ across arms.",
        )?;
        output = Some(returned);
        complete &= receipt["finish"] == "stop";
        times.push((
            positive(&receipt["preparation_milliseconds"])?
                + positive(&receipt["request_milliseconds"])?,
            wall,
            elapsed,
        ));
        observations.push(json!({"id":arm["id"],"role":role,"source_revision":receipt["source_commit_declaration"],"prompt_tokens":receipt["prompt_tokens"],"generated_tokens":receipt["generated_tokens"],"finish":receipt["finish"],"load_milliseconds":receipt["load_milliseconds"],"preparation_milliseconds":receipt["preparation_milliseconds"],"request_milliseconds":receipt["request_milliseconds"],"process_wall_milliseconds":wall,"owned_arm_milliseconds":elapsed,"source_request_tokens_per_second":positive(&receipt["generated_tokens"])?*1000.0/positive(&receipt["request_milliseconds"])?,"source_request_tps_scope":"committed output tokens divided by SwiftSession completion time; includes prefill/sampling, excludes loading/preparation; not post-first-token decode TPS","first_text_delta_milliseconds":receipt["first_text_delta_milliseconds"],"first_text_delta_scope":"first nonempty text callback after completion entry; includes prefill/sampling; not a model token timestamp or broker HTTP TTFT","generation_phases":receipt["generation_phases"],"os_lifetime_peak_job_commit_bytes":peak,"minimum_available_physical_bytes":minimum,"process_memory":supervisor["ProcessMemory"],"memory_scope":"Job commit, process counters and available RAM overlap; never sum them or claim total RAM; sampled counters may miss short peaks"}));
    }
    matches(
        &packet["minimum_relative_request_improvement"],
        &json!(0.10),
    )?;
    matches(&packet["maximum_owned_arm_regression"], &json!(0.10))?;
    let request = [times[1].0 / times[0].0, times[2].0 / times[3].0];
    let wall = [times[1].1 / times[0].1, times[2].1 / times[3].1];
    let arm_ratios = [times[1].2 / times[0].2, times[2].2 / times[3].2];
    require(
        request
            .iter()
            .chain(&wall)
            .chain(&arm_ratios)
            .all(|value| value.is_finite() && *value > 0.0),
        "Nonfinite timing ratio.",
    )?;
    Ok(
        json!({"schema_version":2,"kind":"prism-cpu-bundle-comparison","status":"admitted","short_request_benefit_observed":wall.iter().all(|v|*v<=0.90) && arm_ratios.iter().all(|v|*v<=1.10),"primary_metric":"supervised_load_inclusive_process_wall_milliseconds","candidate_over_baseline_process_wall_ratios":wall,"candidate_over_baseline_request_ratios":request,"candidate_over_baseline_owned_arm_ratios":arm_ratios,"observations":observations,"ratio_range":[wall[0].min(wall[1]),wall[0].max(wall[1])],"process_wall_ratio_range":[wall[0].min(wall[1]),wall[0].max(wall[1])],"request_ratio_range":[request[0].min(request[1]),request[0].max(request[1])],"pairs":2,"complete_answer_observed":complete,"budget_seconds":budget,"aggregate_elapsed_seconds":float(retained-begin)?/float(frequency)?,"power_mode":if expected_power.0 {"ac"}else{"battery"},"continuous_power_stability_proven":false,"statistical_significance_qualified":false,"benefit_gate_qualified":false,"promotion_qualified":false,"DefaultMayChange":false,"individual_optimization_verdicts":false,"state_quality_sustained_acceptance_qualified":false,"scope":"Two matched reversed short-request pairs for the complete source bundle only; source/state/resource and uncertainty acceptance remain independently required."}),
    )
}

pub(super) fn run(request: &Value) -> Result<Value> {
    let allowed: &[&str] = match request["operation"].as_str() {
        Some("bundle-prepare") => &[
            "operation",
            "baselineRoot",
            "toolingRoot",
            "candidateRegistryVersion",
            "runnerRevision",
            "output",
            "runnerRoot",
        ],
        Some("bundle-evaluate") => &["operation", "preparationPath", "ownerPath"],
        Some("bundle-owner-start") => &[
            "operation",
            "startReceipt",
            "runId",
            "runnerCommit",
            "candidateCommit",
        ],
        Some("bundle-owner-finalize") => &["operation", "startReceipt"],
        Some("bundle-capture") => &[
            "operation",
            "startReceipt",
            "candidateCommit",
            "packetDirectory",
            "expectedPacketSha256",
            "toolingCommit",
            "evidenceDirectory",
            "parentProcessId",
            "parentCreationTimeFileTime",
        ],
        _ => return Err("Unknown CPU bundle operation.".into()),
    };
    require(
        object(request)?
            .keys()
            .all(|key| allowed.contains(&key.as_str())),
        "Unknown CPU bundle request field.",
    )?;
    match request["operation"].as_str() {
        Some("bundle-prepare") => prepare::run(request),
        Some("bundle-owner-start" | "bundle-owner-finalize" | "bundle-capture") => {
            #[cfg(windows)]
            {
                owner::run(request)
            }
            #[cfg(not(windows))]
            {
                Err("CPU bundle capture requires native Windows ARM64.".into())
            }
        }
        Some("bundle-evaluate") => {
            let preparation = PathBuf::from(
                request["preparationPath"]
                    .as_str()
                    .ok_or("Missing preparation path.")?,
            );
            let owner_path =
                PathBuf::from(request["ownerPath"].as_str().ok_or("Missing owner path.")?);
            let packet = read(&preparation)?;
            validate_preparation(&packet, &preparation)?;
            let owner = read(&owner_path)?;
            matches(&owner["preparation_sha256"], &json!(digest(&preparation)?))?;
            evaluate(&packet, &owner, &owner_path)
        }
        _ => Err("Unknown CPU bundle operation.".into()),
    }
}
