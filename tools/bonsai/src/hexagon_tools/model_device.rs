//! Explicit local-build intake and owned execution of a bounded Swift token sequence.
//!
//! This terminal diagnostic preserves the separate hosted projection intake and
//! enables no application profile, backend selection or performance claim.

use super::{TARGET, device, hold, plain, write_json};
use crate::{Result, identity, startup_cache, startup_process};
use bonsai_artifacts::{is_redirect, validate_external_path};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::Read as _;
use std::os::windows::fs::OpenOptionsExt as _;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

const MODEL_BYTES: u64 = 7_206_168_928;
const MODEL_SHA256: &str = "5912bb739217cf25b4283a098c7b893d9baff36bdf2e72d3dbf4cb999d5633d5";
const ADAPTER_BYTES: u64 = 9_682_464;
const ADAPTER_SHA256: &str = "f1669534803d340a496015f5c45125f3437b4d13ec764f40e34488ce83967f42";
const STANDARD_ADAPTER_SCALE_BITS: u32 = 0x3fa6_6666;
const CLEANUP_SECONDS: u64 = 75;
const CHILD_SECONDS: u64 = 195;
const REQUIRED_AVAILABLE_BYTES: u64 = 18 * 1024 * 1024 * 1024;
const STATE_BUDGET_BYTES: u64 = 512 * 1024 * 1024;
const STREAMING_MAPPED_BYTES: u64 = 1_191_936;
const RESIDENT_WEIGHT_BYTES: u64 = 25_067_520;
#[cfg(test)]
const RESIDENT_MAPPED_BYTES: u64 = STREAMING_MAPPED_BYTES + RESIDENT_WEIGHT_BYTES;
const BATCH_STREAMING_MAPPED_BYTES: u64 = 4_755_456;
#[cfg(test)]
const BATCH_RESIDENT_MAPPED_BYTES: u64 = BATCH_STREAMING_MAPPED_BYTES + RESIDENT_WEIGHT_BYTES;
const BATCH10_STREAMING_MAPPED_BYTES: u64 = 11_886_592;
#[cfg(test)]
const BATCH10_RESIDENT_MAPPED_BYTES: u64 = BATCH10_STREAMING_MAPPED_BYTES + RESIDENT_WEIGHT_BYTES;
const NATIVE_Q2_STREAMING_MAPPED_BYTES: u64 = 25_161_728;
#[cfg(test)]
const NATIVE_Q2_RESIDENT_MAPPED_BYTES: u64 =
    NATIVE_Q2_STREAMING_MAPPED_BYTES + RESIDENT_WEIGHT_BYTES;
const RESIDENT_METADATA_BYTES: u64 = 65_536;
const NATIVE_Q2_RESIDENT_METADATA_BYTES: u64 = 524_288;
// Exact logical state for the pinned original-PQ2 model: 48 recurrent
// layers plus the growing KV state of its 16 attention layers. The CPU arm
// retains every complete snapshot until native comparisons finish.
const CONTROL_FIXED_STATE_BYTES: u64 = 156_893_184;
const CONTROL_KV_BYTES_PER_TOKEN: u64 = 131_072;

fn required_control_state_bytes(token_count: usize) -> Result<u64> {
    if !(1..=8).contains(&token_count) {
        return Err("Control state accounting requires one to eight explicit tokens.".into());
    }
    let count = u64::try_from(token_count).map_err(|error| error.to_string())?;
    // The admitted count bound makes every product fit u64. Snapshot i
    // contains CONTROL_FIXED_STATE_BYTES + i * CONTROL_KV_BYTES_PER_TOKEN.
    Ok(count * CONTROL_FIXED_STATE_BYTES + CONTROL_KV_BYTES_PER_TOKEN * count * (count + 1) / 2)
}

#[derive(Clone, Copy, Default, Deserialize, PartialEq, Eq, Serialize)]
enum ExecutionBackend {
    #[default]
    #[serde(rename = "guarded-hvx")]
    GuardedHvx,
    #[serde(rename = "hmx-bitplane")]
    HmxBitplane,
    #[serde(rename = "native-q2")]
    NativeQ2,
}

impl ExecutionBackend {
    const fn integer_values_per_token(self) -> u64 {
        match self {
            Self::GuardedHvx => 1_392_640,
            Self::HmxBitplane => 5_570_560,
            Self::NativeQ2 => 0,
        }
    }

    const fn operations_per_projection(self) -> u64 {
        match self {
            Self::GuardedHvx | Self::HmxBitplane => 40,
            Self::NativeQ2 => 1,
        }
    }
}

#[derive(Clone, Copy, Default, Deserialize, PartialEq, Eq, Serialize)]
enum AdapterCase {
    #[default]
    #[serde(rename = "none")]
    None,
    #[serde(rename = "loaded-zero")]
    LoadedZero,
    #[serde(rename = "standard-scale-1.3")]
    Standard,
}

#[derive(Clone, Copy, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum CpuBackend {
    #[default]
    Scalar,
    Neon,
}

const fn default_row_workers() -> u32 {
    1
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Request {
    repository_directory: PathBuf,
    prism_repository_directory: PathBuf,
    expected_prism_revision: String,
    executable_path: PathBuf,
    executable_sha256: String,
    build_receipt_path: PathBuf,
    model_path: PathBuf,
    package_directory: PathBuf,
    signtool_path: PathBuf,
    output_directory: PathBuf,
    token_id: u32,
    #[serde(default)]
    token_ids: Option<Vec<u32>>,
    #[serde(default)]
    projections: Option<Vec<String>>,
    #[serde(default)]
    execution_backend: ExecutionBackend,
    #[serde(default)]
    resident_host_weights: bool,
    #[serde(default)]
    batch_four_operations: bool,
    #[serde(default)]
    batch_ten_operations: bool,
    #[serde(default)]
    adapter_path: Option<PathBuf>,
    #[serde(default)]
    adapter_case: AdapterCase,
    #[serde(default)]
    cpu_backend: CpuBackend,
    #[serde(default = "default_row_workers")]
    row_workers: u32,
    maximum_seconds: u32,
}

const NATIVE_Q2_MAX_RESIDENT_WEIGHT_BYTES: u64 = 4 * 480 * 1024 * 1024;

fn projection_dimensions(name: &str, backend: ExecutionBackend) -> Option<(u64, u64)> {
    let rest = name.strip_prefix("blk.")?;
    let (index, suffix) = rest.split_once('.')?;
    if index.is_empty() || index.len() > 2 || !index.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let layer = index.parse::<usize>().ok().filter(|&layer| layer < 64)?;
    let full_attention = (layer + 1).is_multiple_of(4);
    match backend {
        ExecutionBackend::GuardedHvx | ExecutionBackend::HmxBitplane => {
            matches!(suffix, "ffn_up.weight" | "ffn_gate.weight").then_some((5120, 17_408))
        }
        ExecutionBackend::NativeQ2 => match suffix {
            "ffn_up.weight" | "ffn_gate.weight" => Some((5120, 17_408)),
            "ffn_down.weight" => Some((17_408, 5120)),
            "attn_qkv.weight" if !full_attention => Some((5120, 10_240)),
            "attn_gate.weight" if !full_attention => Some((5120, 6144)),
            "ssm_out.weight" if !full_attention => Some((6144, 5120)),
            "attn_q.weight" if full_attention => Some((5120, 12_288)),
            "attn_k.weight" | "attn_v.weight" if full_attention => Some((5120, 1024)),
            "attn_output.weight" if full_attention => Some((6144, 5120)),
            _ => None,
        },
    }
}

fn expected_resident_weight_bytes(projections: &[&str], backend: ExecutionBackend) -> Result<u64> {
    let mut total = 0_u64;
    for &name in projections {
        let (columns, rows) = projection_dimensions(name, backend)
            .ok_or("The projection name or geometry is unsupported for this backend.")?;
        let bytes = (rows / 32)
            .checked_mul(columns / 64)
            .and_then(|tiles| tiles.checked_mul(576))
            .ok_or("Resident weight byte calculation overflowed.")?;
        total = total
            .checked_add(bytes)
            .ok_or("Resident weight byte total overflowed.")?;
    }
    Ok(total)
}

impl Request {
    fn tokens(&self) -> &[u32] {
        self.token_ids
            .as_deref()
            .unwrap_or_else(|| std::slice::from_ref(&self.token_id))
    }

    fn projections(&self) -> Vec<&str> {
        self.projections.as_ref().map_or_else(
            || vec!["blk.0.ffn_up.weight"],
            |names| names.iter().map(String::as_str).collect(),
        )
    }

    fn validate_tokens(&self) -> Result<()> {
        let tokens = self.tokens();
        if !(1..=8).contains(&tokens.len())
            || tokens.first() != Some(&self.token_id)
            || tokens.iter().any(|&token| token >= 248_320)
        {
            return Err("Select one to eight explicit Swift tokens beginning with tokenId.".into());
        }
        let required = required_control_state_bytes(tokens.len())?;
        if required > STATE_BUDGET_BYTES {
            return Err(format!(
                "The selected token sequence requires {required} retained CPU control-state bytes, exceeding its fixed {STATE_BUDGET_BYTES}-byte budget."
            ));
        }
        Ok(())
    }

    fn validate_selection(&self) -> Result<()> {
        let projections = self.projections();
        let max_projections = match self.execution_backend {
            ExecutionBackend::NativeQ2 => 112,
            ExecutionBackend::GuardedHvx | ExecutionBackend::HmxBitplane => {
                if self.resident_host_weights { 2 } else { 4 }
            }
        };
        let unique: BTreeSet<&str> = projections.iter().copied().collect();
        if !(1..=max_projections).contains(&projections.len())
            || projections.first() != Some(&"blk.0.ffn_up.weight")
            || unique.len() != projections.len()
            || projections
                .iter()
                .any(|&name| projection_dimensions(name, self.execution_backend).is_none())
        {
            return Err(
                "Select valid unique layer projections beginning with blk.0.ffn_up.weight within the backend's projection limit."
                    .into(),
            );
        }
        if self.resident_host_weights {
            let resident_bytes =
                expected_resident_weight_bytes(&projections, self.execution_backend)?;
            match self.execution_backend {
                ExecutionBackend::HmxBitplane if resident_bytes <= 2 * RESIDENT_WEIGHT_BYTES => {}
                ExecutionBackend::NativeQ2
                    if resident_bytes <= NATIVE_Q2_MAX_RESIDENT_WEIGHT_BYTES => {}
                _ => {
                    return Err(
                        "Resident host weights require the explicit HMX bitplane or native-q2 backend within its arena capacity."
                            .into(),
                    );
                }
            }
        }
        if self.batch_four_operations && self.batch_ten_operations {
            return Err("Select at most one explicit HMX batch mode.".into());
        }
        if (self.batch_four_operations || self.batch_ten_operations)
            && self.execution_backend != ExecutionBackend::HmxBitplane
        {
            return Err("Batched operations require the explicit HMX bitplane backend.".into());
        }
        if self.adapter_path.is_some() != (self.adapter_case != AdapterCase::None)
            || ![1, 4].contains(&self.row_workers)
        {
            return Err("Select the approved adapter path exactly for an adapter case, and one or four CPU row workers.".into());
        }
        Ok(())
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FilePin {
    path: String,
    bytes: u64,
    sha256: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExecutablePin {
    bytes: u64,
    sha256: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BuildReceipt {
    schema_version: u32,
    kind: String,
    hosted_build: bool,
    build_succeeded: bool,
    cargo_locked: bool,
    target: String,
    prism_revision: String,
    compiler: FilePin,
    executable: ExecutablePin,
    sources: Vec<FilePin>,
}

fn power() -> Value {
    bonsai_windows_process::power_status().map_or_else(
        |_| json!({"available":false,"acConnected":null,"batterySaver":null,"batteryPercent":null}),
        |status| json!({"available":true,"acConnected":status.ac_online,"batterySaver":status.battery_saver,"batteryPercent":status.battery_percent}),
    )
}

fn remaining(started: Instant, maximum: u32, per_process: u64) -> Result<Duration> {
    let remaining = Duration::from_secs(u64::from(maximum))
        .saturating_sub(started.elapsed())
        .saturating_sub(Duration::from_secs(CLEANUP_SECONDS));
    if remaining < Duration::from_secs(1) {
        return Err("The model probe has no remaining work and cleanup allowance.".into());
    }
    Ok(remaining.min(Duration::from_secs(per_process)))
}

fn source_names(root: &Path) -> Result<BTreeSet<String>> {
    let mut names = BTreeSet::from(["Cargo.toml".into(), "Cargo.lock".into()]);
    for name in ["build.rs", "rust-toolchain.toml", ".cargo/config.toml"] {
        if root.join(name).is_file() {
            names.insert(name.into());
        }
    }
    let mut pending = vec![root.join("src"), root.join("tools")];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
            if is_redirect(&metadata) {
                return Err("The local build source closure cannot contain redirects.".into());
            }
            if metadata.is_dir() {
                if !["target", ".git", ".worktrees"]
                    .contains(&entry.file_name().to_string_lossy().as_ref())
                {
                    pending.push(path);
                }
            } else if metadata.is_file() {
                let relative = path
                    .strip_prefix(root)
                    .map_err(|error| error.to_string())?
                    .to_str()
                    .ok_or("The local build source path is not Unicode.")?
                    .replace('\\', "/");
                // Empty source entries have no compilation payload. They remain
                // part of the closure and are verified separately below.
                names.insert(relative);
                if names.len() > 10_000 {
                    return Err("The local build source closure exceeds its file bound.".into());
                }
            } else {
                return Err("The local build source closure contains a non-file entry.".into());
            }
        }
    }
    Ok(names)
}

fn source_path(root: &Path, name: &str) -> Result<PathBuf> {
    let relative = Path::new(name);
    if name.contains('\\')
        || relative.is_absolute()
        || !relative
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
    {
        return Err("The local build source path must be relative and normalized.".into());
    }
    Ok(root.join(relative))
}

fn local_build(
    request: &Request,
    inputs: &mut device::Inputs,
    started: Instant,
    report: &mut Value,
) -> Result<()> {
    report["BuildReceiptSha256"] =
        json!(inputs.digest(&request.build_receipt_path, 4 * 1024 * 1024)?);
    let (mut receipt_hold, _) = hold(&request.build_receipt_path, 4 * 1024 * 1024)?;
    let mut receipt_text = String::new();
    receipt_hold
        .read_to_string(&mut receipt_text)
        .map_err(|error| error.to_string())?;
    // The closed typed schema rejects duplicate and unknown fields without
    // broadening the existing evidence JSON parser's independent 64 KiB limit.
    let receipt: BuildReceipt =
        serde_json::from_str(&receipt_text).map_err(|error| error.to_string())?;
    inputs.holds.push(receipt_hold);
    let receipt = serde_json::to_value(receipt).map_err(|error| error.to_string())?;
    if receipt["schemaVersion"] != 1
        || receipt["kind"] != "local-prism-hexagon-model-probe-build"
        || receipt["hostedBuild"] != false
        || receipt["buildSucceeded"] != true
        || receipt["cargoLocked"] != true
        || receipt["target"] != TARGET
        || receipt["prismRevision"] != request.expected_prism_revision
        || receipt["executable"]["sha256"] != request.executable_sha256
    {
        return Err("The model probe requires explicit successful local-build provenance.".into());
    }
    let compiler = PathBuf::from(
        receipt["compiler"]["path"]
            .as_str()
            .ok_or("The local compiler path is missing.")?,
    );
    inputs.pinned(
        &compiler,
        &receipt["compiler"]["bytes"],
        &receipt["compiler"]["sha256"],
    )?;
    let expected = source_names(&request.prism_repository_directory)?;
    let entries = receipt["sources"]
        .as_array()
        .filter(|entries| entries.len() == expected.len())
        .ok_or("The local build source closure is incomplete.")?;
    let mut seen = BTreeSet::new();
    let mut total = 0_u64;
    for entry in entries {
        remaining(started, request.maximum_seconds, 1)?;
        let name = entry["path"]
            .as_str()
            .ok_or("The local source name is missing.")?;
        if !expected.contains(name) || !seen.insert(name) {
            return Err(
                "The local source closure contains duplicate or unexpected entries.".into(),
            );
        }
        total = total
            .checked_add(
                entry["bytes"]
                    .as_u64()
                    .ok_or("The local source length is missing.")?,
            )
            .ok_or("The local source extent overflowed.")?;
        if total > 128 * 1024 * 1024 {
            return Err("The local source closure exceeds its aggregate byte bound.".into());
        }
        let path = source_path(&request.prism_repository_directory, name)?;
        if entry["bytes"] == 0 {
            let mut options = OpenOptions::new();
            options.read(true).share_mode(1).custom_flags(0x0020_0000);
            plain(&path)?;
            let file = options.open(&path).map_err(|error| error.to_string())?;
            let metadata = file.metadata().map_err(|error| error.to_string())?;
            if !metadata.is_file()
                || is_redirect(&metadata)
                || metadata.len() != 0
                || entry["sha256"] != identity::sha256(&[])
            {
                return Err(
                    "An empty local source file differs from its declared identity.".into(),
                );
            }
            inputs.holds.push(file);
        } else {
            inputs.pinned(&path, &entry["bytes"], &entry["sha256"])?;
        }
    }
    inputs.pinned(
        &request.executable_path,
        &receipt["executable"]["bytes"],
        &receipt["executable"]["sha256"],
    )?;
    startup_cache::assert_arm64(&request.executable_path)?;
    report["LocalBuildSourceFilesVerified"] = json!(entries.len());
    report["LocalBuildSourceBytesVerified"] = json!(total);
    report["ExecutableSha256"] = json!(request.executable_sha256);
    Ok(())
}

fn environment(package: &Path) -> Result<BTreeMap<String, String>> {
    let system = PathBuf::from(std::env::var_os("SystemRoot").ok_or("SystemRoot unavailable.")?)
        .join("System32");
    // Empty child-only overlays remove the effect of ambient vendor overrides.
    // Native driver loading still uses its fixed system path and checked package.
    let mut environment: BTreeMap<_, _> = device::removals()
        .into_iter()
        .map(|name| (name.to_ascii_uppercase(), String::new()))
        .collect();
    environment.insert("PATH".into(), system.to_string_lossy().into_owned());
    environment.insert(
        "ADSP_LIBRARY_PATH".into(),
        package.to_string_lossy().into_owned(),
    );
    Ok(environment)
}

fn capture(
    request: &Request,
    executable: &Path,
    args: &[std::ffi::OsString],
    seconds: u64,
    started: Instant,
    name: &str,
) -> Result<startup_process::ProcessOutput> {
    let output = startup_process::run_strict(
        executable.as_os_str(),
        args,
        &request.package_directory,
        &environment(&request.package_directory)?,
        remaining(started, request.maximum_seconds, seconds)?,
        &request.output_directory.join(name),
    )?;
    if output.stdout.len() > 256 * 1024 || output.stderr.len() > 64 * 1024 {
        return Err("The model probe exceeded its retained output bounds.".into());
    }
    Ok(output)
}

fn model_guard(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    options.share_mode(1).custom_flags(0x0020_0000);
    let file = options.open(path).map_err(|error| error.to_string())?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file() || is_redirect(&metadata) || metadata.len() != MODEL_BYTES {
        return Err("The model must match the original Swift PQ2 extent.".into());
    }
    // The exact source-bound probe performs immutable-model SHA-256 admission.
    // Keep this handle open through that admission and both comparison arms.
    Ok(file)
}

#[expect(
    clippy::too_many_lines,
    reason = "Keep backend, sequence, numerical, native cleanup and per-arm power acceptance together."
)]
fn result(
    text: &str,
    request_digest: &str,
    tokens: &[u32],
    backend: ExecutionBackend,
    resident_host_weights: bool,
    driver: &Value,
) -> Result<Value> {
    if !(1..=8).contains(&tokens.len()) {
        return Err("The model result needs one to eight admitted tokens.".into());
    }
    let events: Vec<Value> = text
        .lines()
        .map(|line| crate::hexagon_policy::run(json!({"operation":"json","json":line})))
        .collect::<Result<_>>()?;
    if events.iter().any(|value| {
        !matches!(
            value["event"].as_str(),
            Some(
                "started"
                    | "cpu-arm-finished"
                    | "cpu-control-complete"
                    | "native-progress"
                    | "native-arm-finished"
                    | "result"
            )
        )
    }) {
        return Err("The model probe retained a contradictory or unknown event.".into());
    }
    if !events
        .last()
        .is_some_and(|value| value["event"] == "result")
    {
        return Err("The model probe result must be its terminal event.".into());
    }
    let results: Vec<Value> = events
        .into_iter()
        .filter(|value| value["event"] == "result")
        .collect();
    let [value] = results.as_slice() else {
        return Err("The model probe omitted its unique terminal result.".into());
    };
    let token_count = u64::try_from(tokens.len()).map_err(|error| error.to_string())?;
    let (projection_count, expected_weights) = match value.get("projections") {
        None => (1_u64, RESIDENT_WEIGHT_BYTES),
        Some(projections) => {
            let list = projections
                .as_array()
                .ok_or("The model result projections field must be an array.")?;
            let max_projections = match backend {
                ExecutionBackend::NativeQ2 => 112,
                ExecutionBackend::GuardedHvx | ExecutionBackend::HmxBitplane => {
                    if resident_host_weights {
                        2
                    } else {
                        4
                    }
                }
            };
            let mut seen = BTreeSet::new();
            let mut names = Vec::with_capacity(list.len());
            if !(1..=max_projections).contains(&list.len())
                || list.first().and_then(Value::as_str) != Some("blk.0.ffn_up.weight")
            {
                return Err(
                    "The model result projections list is invalid or out of bounds.".into(),
                );
            }
            for item in list {
                let Some(name) = item.as_str() else {
                    return Err(
                        "The model result projections list is invalid or out of bounds.".into(),
                    );
                };
                if projection_dimensions(name, backend).is_none() || !seen.insert(name) {
                    return Err(
                        "The model result projections list is invalid or out of bounds.".into(),
                    );
                }
                names.push(name);
            }
            let resident_bytes = expected_resident_weight_bytes(&names, backend)?;
            if resident_host_weights {
                let max_bytes = match backend {
                    ExecutionBackend::NativeQ2 => NATIVE_Q2_MAX_RESIDENT_WEIGHT_BYTES,
                    ExecutionBackend::HmxBitplane => 2 * RESIDENT_WEIGHT_BYTES,
                    ExecutionBackend::GuardedHvx => 0,
                };
                if resident_bytes > max_bytes {
                    return Err(
                        "The model result projections list exceeds the resident arena bound."
                            .into(),
                    );
                }
            }
            (
                u64::try_from(list.len()).map_err(|error| error.to_string())?,
                resident_bytes,
            )
        }
    };
    let total_projections = projection_count * token_count;
    let batch_four = value.get("batchFourOperations") == Some(&json!(true));
    let batch_ten = value.get("batchTenOperations") == Some(&json!(true));
    residency_result(
        value,
        backend,
        resident_host_weights,
        token_count,
        projection_count,
        expected_weights,
    )?;
    batch_result(
        value,
        backend,
        resident_host_weights,
        batch_four,
        batch_ten,
        token_count,
        projection_count,
        expected_weights,
    )?;
    let compared_values = if resident_host_weights {
        0
    } else {
        backend.integer_values_per_token() * total_projections
    };
    let ops_per_proj = backend.operations_per_projection();
    let numerical_accepted = if backend == ExecutionBackend::NativeQ2 {
        value["logitsBitwiseEqual"].is_boolean()
            && value["stateBitwiseEqual"].is_boolean()
            && value["logitsBoundedTolerancePass"] == true
            && valid_numerical_tolerance(&value["numericalTolerance"])
    } else {
        value["logitsBitwiseEqual"] == true && value["stateBitwiseEqual"] == true
    };
    if value["schemaVersion"] != 1
        || value["completed"] != true
        || value["requestSha256"] != request_digest
        || value["modelSha256"] != MODEL_SHA256
        || value["projection"] != "blk.0.ffn_up.weight"
        || value["tokenId"] != tokens[0]
        || value["tokenIds"] != json!(tokens)
        || value["executionBackend"] != json!(backend)
        || value["nativeHmxBitplaneControl"] != (backend == ExecutionBackend::HmxBitplane)
        || value["maximumOperations"] != ops_per_proj * total_projections
        || value["sessionSeconds"] != 120
        || value["driverDll"] != driver["Path"]
        || value["nativeOperationsSubmitted"] != ops_per_proj * total_projections
        || value["nativeOperationsCompleted"] != ops_per_proj * total_projections
        || value["projectionsCompleted"] != total_projections
        || value["integerValuesCompared"] != compared_values
        || !numerical_accepted
        || value["nativeCompletedCycle"] != true
        || value["profileEligible"] != false
        || value["performanceMeasured"] != false
        || ["logitsSha256", "stateSha256"].iter().any(|name| {
            !value[*name]
                .as_str()
                .is_some_and(|digest| identity::is_lower_hex(digest, 64))
        })
    {
        return Err(
            "The model result lacks complete numerical, identity or cleanup evidence.".into(),
        );
    }
    let cleanup = &value["nativeCleanup"];
    if [
        "queueStopResult",
        "queueCloseResult",
        "unmappingResult",
        "moduleCloseResult",
    ]
    .iter()
    .any(|name| cleanup[*name] != 0)
        || cleanup["freeCompleted"] != true
        || cleanup["poisoned"] != false
        // Driver/module/catalog owners remain pinned through process exit even
        // after safe unmapping/free; this is the native completed_cycle policy.
        || cleanup["ownerPinnedUntilExit"] != true
    {
        return Err("The model result has incomplete or contradictory native cleanup.".into());
    }
    let comparisons = value["tokenComparisons"]
        .as_array()
        .filter(|comparisons| comparisons.len() == tokens.len())
        .ok_or("The model result omitted complete per-token parity evidence.")?;
    for (index, (comparison, &token)) in comparisons.iter().zip(tokens).enumerate() {
        let token_numerical = if backend == ExecutionBackend::NativeQ2 {
            comparison["logitsBitwiseEqual"].is_boolean()
                && comparison["stateBitwiseEqual"].is_boolean()
                && comparison["logitsBoundedTolerancePass"] == true
                && valid_numerical_tolerance(&comparison["numericalTolerance"])
        } else {
            comparison["logitsBitwiseEqual"] == true && comparison["stateBitwiseEqual"] == true
        };
        if comparison["index"] != index
            || comparison["tokenId"] != token
            || !token_numerical
            || ["logitsSha256", "stateSha256"].iter().any(|name| {
                !comparison[*name]
                    .as_str()
                    .is_some_and(|digest| identity::is_lower_hex(digest, 64))
            })
        {
            return Err("The model result has incomplete or reordered per-token parity.".into());
        }
    }
    let last = comparisons
        .last()
        .ok_or("The model result omitted its final token comparison.")?;
    if ["logitsSha256", "stateSha256"]
        .iter()
        .any(|name| value[*name] != last[*name])
        || (backend == ExecutionBackend::NativeQ2
            && value["numericalTolerance"] != last["numericalTolerance"])
    {
        return Err("The terminal hashes do not bind the final token comparison.".into());
    }
    for arm in [
        "cpuPowerBefore",
        "cpuPowerAfter",
        "npuPowerBefore",
        "npuPowerAfter",
    ] {
        let snapshot = &value[arm];
        if !snapshot["available"].is_boolean()
            || !["acConnected", "batterySaver"].iter().all(|name| {
                snapshot
                    .get(*name)
                    .is_some_and(|value| value.is_boolean() || value.is_null())
            })
            || !snapshot.get("batteryPercent").is_some_and(|value| {
                value.is_null() || value.as_u64().is_some_and(|percent| percent <= 100)
            })
            || (snapshot["available"] == false
                && ["acConnected", "batterySaver", "batteryPercent"]
                    .iter()
                    .any(|name| !snapshot[*name].is_null()))
        {
            return Err("The model result omitted a comparison-arm power observation.".into());
        }
    }
    Ok(value.clone())
}

fn valid_numerical_tolerance(tol: &Value) -> bool {
    let Some(cos) = tol["cosineSimilarity"].as_f64() else {
        return false;
    };
    let Some(rmse) = tol["rmse"].as_f64() else {
        return false;
    };
    let Some(max_diff) = tol["maxAbsDiff"].as_f64() else {
        return false;
    };
    let Some(mean_diff) = tol["meanAbsDiff"].as_f64() else {
        return false;
    };
    let Some(top5) = tol["top5Overlap"].as_u64() else {
        return false;
    };
    cos >= 0.995
        && rmse <= 0.5
        && max_diff <= 5.0
        && mean_diff >= 0.0
        && mean_diff <= max_diff
        && tol["cpuTop1Token"]
            .as_u64()
            .is_some_and(|tok| tok < 248_320)
        && tol["npuTop1Token"]
            .as_u64()
            .is_some_and(|tok| tok < 248_320)
        && tol["top1Match"].is_boolean()
        && top5 <= 5
}

fn residency_result(
    value: &Value,
    backend: ExecutionBackend,
    resident_host_weights: bool,
    token_count: u64,
    projection_count: u64,
    expected_weights: u64,
) -> Result<()> {
    // Historical full-integer controls predate these explicit fields. Their
    // absence means only the original streaming path, never resident execution.
    let mode = value.get("residentHostWeights");
    let audit = value.get("integerAudit");
    let expected_audit = if backend == ExecutionBackend::NativeQ2 {
        "bounded-float-tolerance"
    } else if resident_host_weights {
        "graph-parity-only"
    } else {
        "all-returned-integers"
    };
    if mode.is_some_and(|mode| mode != resident_host_weights)
        || audit.is_some_and(|audit| audit != expected_audit)
        || (backend == ExecutionBackend::NativeQ2 && audit != Some(&json!(expected_audit)))
        || (resident_host_weights
            && (!matches!(
                backend,
                ExecutionBackend::HmxBitplane | ExecutionBackend::NativeQ2
            ) || mode != Some(&json!(true))
                || audit != Some(&json!(expected_audit))))
    {
        return Err(
            "The model result does not bind its selected residency and integer audit mode.".into(),
        );
    }
    if resident_host_weights {
        let batch_four = value.get("batchFourOperations") == Some(&json!(true));
        let batch_ten = value.get("batchTenOperations") == Some(&json!(true));
        let base_mapped = if backend == ExecutionBackend::NativeQ2 {
            NATIVE_Q2_STREAMING_MAPPED_BYTES
        } else if batch_ten {
            BATCH10_STREAMING_MAPPED_BYTES
        } else if batch_four {
            BATCH_STREAMING_MAPPED_BYTES
        } else {
            STREAMING_MAPPED_BYTES
        };
        let expected_mapped = base_mapped + expected_weights;
        let resources = &value["nativeResources"];
        let residency = &value["hostResidency"];
        let max_metadata_bytes = if backend == ExecutionBackend::NativeQ2 {
            NATIVE_Q2_RESIDENT_METADATA_BYTES
        } else {
            RESIDENT_METADATA_BYTES
        };
        if resources["residentWeightBytesPrepared"] != expected_weights
            || resources["mappedAllocationBytes"] != expected_mapped
            || residency["tensorsPrepared"] != projection_count
            || residency["cacheHits"] != projection_count * (token_count - 1)
            || residency["weightPayloadBytesPrepared"] != expected_weights
            || !residency["metadataPayloadBytes"]
                .as_u64()
                .is_some_and(|bytes| (1..=max_metadata_bytes).contains(&bytes))
        {
            return Err(
                "The resident model result lacks its fixed mapped arena and cache accounting."
                    .into(),
            );
        }
    }
    Ok(())
}

#[expect(
    clippy::too_many_arguments,
    reason = "Keep batch, residency and dynamic weight-extent verification together."
)]
fn batch_result(
    value: &Value,
    backend: ExecutionBackend,
    resident_host_weights: bool,
    batch_four_operations: bool,
    batch_ten_operations: bool,
    token_count: u64,
    projection_count: u64,
    expected_weights: u64,
) -> Result<()> {
    if batch_four_operations && batch_ten_operations {
        return Err("The model result cannot enable both batch-four and batch-ten modes.".into());
    }
    let batch_four_mode = value.get("batchFourOperations");
    let batch_ten_mode = value.get("batchTenOperations");
    let batch_ops = value.get("nativeBatchOperations");
    let writes_sub = value.get("nativeQueueWritesSubmitted");
    let writes_comp = value.get("nativeQueueWritesCompleted");
    let expected_batch = if batch_ten_operations {
        10_u64
    } else if batch_four_operations {
        4
    } else {
        1
    };
    let ops_per_proj = backend.operations_per_projection();
    let expected_writes = (ops_per_proj / expected_batch) * projection_count * token_count;
    let base_mapped = if backend == ExecutionBackend::NativeQ2 {
        NATIVE_Q2_STREAMING_MAPPED_BYTES
    } else if batch_ten_operations {
        BATCH10_STREAMING_MAPPED_BYTES
    } else if batch_four_operations {
        BATCH_STREAMING_MAPPED_BYTES
    } else {
        STREAMING_MAPPED_BYTES
    };
    let expected_mapped = if resident_host_weights {
        base_mapped + expected_weights
    } else {
        base_mapped
    };
    if batch_four_mode.is_some_and(|mode| mode != batch_four_operations)
        || batch_ten_mode.is_some_and(|mode| mode != batch_ten_operations)
        || batch_ops.is_some_and(|ops| ops != expected_batch)
        || writes_sub.is_some_and(|writes| writes != expected_writes)
        || writes_comp.is_some_and(|writes| writes != expected_writes)
        || (backend == ExecutionBackend::NativeQ2
            && (batch_ops != Some(&json!(1))
                || writes_sub != Some(&json!(expected_writes))
                || writes_comp != Some(&json!(expected_writes))
                || value["nativeResources"]["mappedAllocationBytes"] != expected_mapped))
        || (batch_four_operations
            && (backend != ExecutionBackend::HmxBitplane
                || batch_four_mode != Some(&json!(true))
                || batch_ops != Some(&json!(4))
                || writes_sub != Some(&json!(expected_writes))
                || writes_comp != Some(&json!(expected_writes))
                || value["nativeResources"]["mappedAllocationBytes"] != expected_mapped))
        || (batch_ten_operations
            && (backend != ExecutionBackend::HmxBitplane
                || batch_ten_mode != Some(&json!(true))
                || batch_ops != Some(&json!(10))
                || writes_sub != Some(&json!(expected_writes))
                || writes_comp != Some(&json!(expected_writes))
                || value["nativeResources"]["mappedAllocationBytes"] != expected_mapped))
    {
        return Err(
            "The model result does not bind its selected batch-four and queue-write accounting."
                .into(),
        );
    }
    Ok(())
}

fn selection_result(value: &Value, request: &Request) -> Result<()> {
    let token_count = u64::try_from(request.tokens().len()).map_err(|error| error.to_string())?;
    let expected_projections = request.projections();
    let projection_count =
        u64::try_from(expected_projections.len()).map_err(|error| error.to_string())?;
    let expected_weights =
        expected_resident_weight_bytes(&expected_projections, request.execution_backend)?;
    let projections = value.get("projections");
    if projections.is_some_and(|actual| actual != &json!(expected_projections))
        || (expected_projections.len() > 1 && projections != Some(&json!(expected_projections)))
    {
        return Err("The model result does not bind the selected projection placement.".into());
    }
    batch_result(
        value,
        request.execution_backend,
        request.resident_host_weights,
        request.batch_four_operations,
        request.batch_ten_operations,
        token_count,
        projection_count,
        expected_weights,
    )?;
    let loaded = request.adapter_case != AdapterCase::None;
    let standard = request.adapter_case == AdapterCase::Standard;
    let scale_bits = if standard {
        STANDARD_ADAPTER_SCALE_BITS
    } else {
        0
    };
    let digest = if loaded {
        json!(ADAPTER_SHA256)
    } else {
        Value::Null
    };
    if value["adapterCase"] != json!(request.adapter_case)
        || value["adapterLoaded"] != loaded
        || value["adapterSha256"] != digest
        || value["adapterScaleBits"] != scale_bits
        || value["cpuBaselineAdapterLoaded"] != standard
        || value["cpuBaselineAdapterScaleBits"] != scale_bits
        || value["cpuBackend"] != json!(request.cpu_backend)
        || value["rowWorkers"] != request.row_workers
    {
        return Err(
            "The model result does not bind the selected adapter case and CPU policy.".into(),
        );
    }
    Ok(())
}

#[expect(
    clippy::too_many_lines,
    reason = "Keep one ordered terminal intake, signed catalog and driver verification, resource admission and owned graph comparison visible together."
)]
fn execute(request: &Request, started: Instant, report: &mut Value) -> Result<()> {
    let mut inputs = device::Inputs::default();
    local_build(request, &mut inputs, started, report)?;
    if let Some(path) = &request.adapter_path {
        inputs.pinned(path, &json!(ADAPTER_BYTES), &json!(ADAPTER_SHA256))?;
        report["AdapterIdentity"] = json!({"Path":path,"Bytes":ADAPTER_BYTES,"Sha256":ADAPTER_SHA256,"FileHeldAgainstMutation":true});
    }
    let dispatch_path = request
        .repository_directory
        .join("config/hexagon-dispatch.lock.json");
    let candidate_path = request
        .repository_directory
        .join("config/hexagon-signed-q2-candidate.lock.json");
    if fs::read(&dispatch_path).map_err(|error| error.to_string())?
        != include_bytes!("../../../../config/hexagon-dispatch.lock.json")
        || fs::read(&candidate_path).map_err(|error| error.to_string())?
            != include_bytes!("../../../../config/hexagon-signed-q2-candidate.lock.json")
    {
        return Err("The model probe runtime locks differ from the compiled review.".into());
    }
    let lock = inputs.json(&dispatch_path, 1024 * 1024)?;
    report["Stage"] = json!("signed-package");
    let members = device::compute_package(
        &mut inputs,
        &request.repository_directory,
        &request.package_directory,
        report,
    )?;
    inputs.pinned(
        &request.signtool_path,
        &lock["signtool"]["bytes"],
        &lock["signtool"]["sha256"],
    )?;
    let mut args = vec![
        "verify".into(),
        "/pa".into(),
        "/v".into(),
        "/c".into(),
        request
            .package_directory
            .join("libggml-htp.cat")
            .into_os_string(),
    ];
    args.extend(
        members
            .iter()
            .map(|name| request.package_directory.join(name).into_os_string()),
    );
    if capture(
        request,
        &request.signtool_path,
        &args,
        10,
        started,
        "catalog.txt",
    )?
    .code
        != 0
    {
        return Err("The model probe catalog membership is unverified.".into());
    }
    report["SignedPackageVerified"] = json!(true);
    report["CatalogMembersVerified"] = json!(members.len());
    let inspection = capture(
        request,
        &request.executable_path,
        &["--inspect".into()],
        10,
        started,
        "inspection.txt",
    )?;
    let inspected =
        crate::hexagon_policy::run(json!({"operation":"json","json":inspection.stdout}))?;
    if inspection.code != 0
        || inspected["event"] != "inspection"
        || inspected["kind"] != "native-fastrpc-inspection"
        || ["driverLoaded", "dspCodeLoaded", "profileEligible"]
            .iter()
            .any(|name| inspected[*name] != false)
    {
        return Err("The model probe driver inspection contract failed.".into());
    }
    let driver = device::driver(
        &PathBuf::from(
            inspected["driverDll"]
                .as_str()
                .ok_or("Driver path missing.")?,
        ),
        &mut inputs.holds,
    )?;
    report["DriverIdentity"] = driver.clone();
    let available =
        bonsai_windows_process::available_memory_bytes().map_err(|error| error.to_string())?;
    report["AvailablePhysicalBytesBefore"] = json!(available);
    if available < REQUIRED_AVAILABLE_BYTES {
        return Err(
            "The model probe cannot retain its 18 GiB physical-memory admission reserve.".into(),
        );
    }
    let _model_guard = model_guard(&request.model_path)?;
    let pid = std::process::id();
    let creation = bonsai_windows_process::current_process_creation_time()
        .map_err(|error| error.to_string())?;
    let ops_per_proj = usize::try_from(request.execution_backend.operations_per_projection())
        .map_err(|error| error.to_string())?;
    let probe_request = json!({"schemaVersion":1,"modelPath":request.model_path,"packageDirectory":request.package_directory,
        "tokenId":request.token_id,"tokenIds":request.tokens(),"projections":request.projections(),"executionBackend":request.execution_backend,
        "residentHostWeights":request.resident_host_weights,"batchFourOperations":request.batch_four_operations,"batchTenOperations":request.batch_ten_operations,
        "adapterPath":request.adapter_path,"adapterCase":request.adapter_case,"cpuBackend":request.cpu_backend,"rowWorkers":request.row_workers,
        "contextLimit":request.tokens().len(),"modelMemoryLimitBytes":12_u64*1024*1024*1024,
        "stateBudgetBytes":STATE_BUDGET_BYTES,"sessionSeconds":120,"maximumOperations":ops_per_proj*request.projections().len()*request.tokens().len(),
        "maximumOwnedPayloadBytes":64_u64*1024*1024,"expectedParentPid":pid,"expectedParentCreationTime":creation});
    let path = request.output_directory.join("model-request.json");
    write_json(&path, &probe_request)?;
    let (_request_hold, request_digest) = hold(&path, 8192)?;
    // Reserve the entire declared child request, including explicit cleanup,
    // before executing any model or driver work. No partial trial is restarted.
    if remaining(started, request.maximum_seconds, CHILD_SECONDS)?
        < Duration::from_secs(CHILD_SECONDS)
    {
        return Err(
            "The model probe no longer fits its 195-second child and 75-second cleanup allowances."
                .into(),
        );
    }
    report["Stage"] = json!("bounded-token-graph");
    report["ModelWorkAttempted"] = json!(true);
    report["OwnedJobTeardown"] = json!("unconfirmed");
    let output = capture(
        request,
        &request.executable_path,
        &["--request".into(), path.into_os_string()],
        CHILD_SECONDS,
        started,
        "model-probe.txt",
    )?;
    report["ProbeExitCode"] = json!(output.code);
    report["OwnedJobTeardown"] = json!("confirmed-empty");
    // Retain arm observations even when no terminal result is publishable.
    report["ProbeEvents"] = json!(
        output
            .stdout
            .lines()
            .map(|line| {
                crate::hexagon_policy::run(json!({"operation":"json","json":line}))
                    .unwrap_or_else(|_| json!({"event":"unparsed-line"}))
            })
            .collect::<Vec<_>>()
    );
    if output.code != 0 {
        return Err(
            "The terminal model probe failed; retained output must be reviewed before new work."
                .into(),
        );
    }
    let native_result = result(
        &output.stdout,
        &request_digest,
        request.tokens(),
        request.execution_backend,
        request.resident_host_weights,
        &driver,
    )?;
    selection_result(&native_result, request)?;
    report["NativeResult"] = native_result;
    report["Outcome"] = json!("passed");
    report["Stage"] = json!("bounded-token-graph-complete");
    Ok(())
}

pub(super) fn run(request: &Request) -> Result<Value> {
    let started = Instant::now();
    if !cfg!(target_arch = "aarch64")
        || request.maximum_seconds != 300
        || !identity::is_lower_hex(&request.expected_prism_revision, 40)
        || !identity::is_lower_hex(&request.executable_sha256, 64)
    {
        return Err(
            "Select one native ARM64 local-source-bound token-sequence probe with its fixed 300-second supervisor allowance."
                .into(),
        );
    }
    request.validate_tokens()?;
    request.validate_selection()?;
    plain(&request.repository_directory)?;
    plain(&request.prism_repository_directory)?;
    for path in [
        &request.executable_path,
        &request.build_receipt_path,
        &request.model_path,
        &request.package_directory,
        &request.signtool_path,
        &request.output_directory,
    ] {
        validate_external_path(path).map_err(|error| error.to_string())?;
    }
    if let Some(path) = &request.adapter_path {
        validate_external_path(path).map_err(|error| error.to_string())?;
    }
    fs::create_dir(&request.output_directory)
        .map_err(|_| "The model probe requires a new external evidence directory.".to_owned())?;
    let mut report = json!({"schemaVersion":1,"kind":"runner-local-hexagon-model-probe","Outcome":"pending","Stage":"local-build-intake",
        "LocalBuild":true,"HostedBuild":false,"PrismRevision":request.expected_prism_revision,"MaximumSeconds":request.maximum_seconds,
        "CleanupReserveSeconds":CLEANUP_SECONDS,"ModelWorkAttempted":false,"OwnedJobTeardown":"not-started","ProfileEligible":false,
        "PerformanceMeasured":false,"TrustChanged":false,"PowerBefore":power(),"RequiredAvailablePhysicalBytes":REQUIRED_AVAILABLE_BYTES,
        "ExecutionBackend":request.execution_backend,"ResidentHostWeights":request.resident_host_weights,"BatchFourOperations":request.batch_four_operations,"BatchTenOperations":request.batch_ten_operations,
        "Projections":request.projections(),"TokenIds":request.tokens(),"AdapterCase":request.adapter_case,
        "CpuBackend":request.cpu_backend,"RowWorkers":request.row_workers,
        "RequiredControlStateBytes":required_control_state_bytes(request.tokens().len())?,"ControlStateBudgetBytes":STATE_BUDGET_BYTES});
    let outcome = execute(request, started, &mut report);
    report["PowerAfter"] = power();
    report["ElapsedMilliseconds"] = json!(started.elapsed().as_millis());
    if let Err(error) = outcome {
        report["Failure"] = json!(error);
    }
    write_json(&request.output_directory.join("model-device.json"), &report)?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request_value() -> Value {
        json!({"repositoryDirectory":"repo","prismRepositoryDirectory":"prism",
            "expectedPrismRevision":"revision","executablePath":"exe","executableSha256":"sha",
            "buildReceiptPath":"receipt","modelPath":"model","packageDirectory":"package",
            "signtoolPath":"sign","outputDirectory":"output","tokenId":42,"maximumSeconds":300})
    }

    fn complete_result(tokens: &[u32], backend: ExecutionBackend) -> Value {
        let power =
            json!({"available":true,"acConnected":true,"batterySaver":false,"batteryPercent":100});
        let count = tokens.len();
        let comparisons: Vec<_> = tokens.iter().enumerate().map(|(index, token)| {
            json!({"index":index,"tokenId":token,"logitsBitwiseEqual":true,"stateBitwiseEqual":true,
                "logitsSha256":"b".repeat(64),"stateSha256":"c".repeat(64)})
        }).collect();
        json!({"event":"result","schemaVersion":1,"completed":true,"requestSha256":"a".repeat(64),
            "modelSha256":MODEL_SHA256,"projection":"blk.0.ffn_up.weight","tokenId":tokens[0],"driverDll":"fixed-driver",
            "tokenIds":tokens,"executionBackend":backend,"nativeHmxBitplaneControl":backend==ExecutionBackend::HmxBitplane,
            "maximumOperations":40*count,"sessionSeconds":120,"tokenComparisons":comparisons,
            "nativeOperationsSubmitted":40*count,"nativeOperationsCompleted":40*count,"projectionsCompleted":count,
            "integerValuesCompared":backend.integer_values_per_token()*u64::try_from(count).unwrap_or(0),"logitsBitwiseEqual":true,"stateBitwiseEqual":true,
            "nativeCompletedCycle":true,"profileEligible":false,"performanceMeasured":false,
            "nativeCleanup":{"queueStopResult":0,"queueCloseResult":0,"unmappingResult":0,"moduleCloseResult":0,
                "freeCompleted":true,"poisoned":false,"ownerPinnedUntilExit":true},
            "logitsSha256":"b".repeat(64),"stateSha256":"c".repeat(64),
            "cpuPowerBefore":power,"cpuPowerAfter":power,"npuPowerBefore":power,"npuPowerAfter":power})
    }

    #[test]
    fn terminal_parity_needs_complete_cleanup_identity_and_arm_observations() -> Result<()> {
        let driver = json!({"Path":"fixed-driver"});
        let value = complete_result(&[42], ExecutionBackend::GuardedHvx);
        result(
            &value.to_string(),
            &"a".repeat(64),
            &[42],
            ExecutionBackend::GuardedHvx,
            false,
            &driver,
        )?;
        for (pointer, wrong) in [
            ("/nativeCompletedCycle", json!(false)),
            ("/nativeOperationsCompleted", json!(39)),
            ("/integerValuesCompared", json!(1_392_639)),
            ("/stateBitwiseEqual", json!(false)),
            ("/modelSha256", json!("0".repeat(64))),
            ("/driverDll", json!("other-driver")),
            ("/npuPowerBefore/acConnected", json!("unknown")),
            ("/cpuPowerAfter/batteryPercent", json!(101)),
        ] {
            let mut changed = value.clone();
            *changed.pointer_mut(pointer).ok_or("Test field missing.")? = wrong;
            assert!(
                result(
                    &changed.to_string(),
                    &"a".repeat(64),
                    &[42],
                    ExecutionBackend::GuardedHvx,
                    false,
                    &driver
                )
                .is_err()
            );
        }
        let error_then_result = format!("{{\"event\":\"error\"}}\n{value}");
        assert!(
            result(
                &error_then_result,
                &"a".repeat(64),
                &[42],
                ExecutionBackend::GuardedHvx,
                false,
                &driver
            )
            .is_err()
        );
        assert!(
            result(
                &format!("{value}\n{value}"),
                &"a".repeat(64),
                &[42],
                ExecutionBackend::GuardedHvx,
                false,
                &driver
            )
            .is_err()
        );
        assert!(
            result(
                &format!("{value}\n{{\"event\":\"native-progress\"}}"),
                &"a".repeat(64),
                &[42],
                ExecutionBackend::GuardedHvx,
                false,
                &driver
            )
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn unknown_arm_power_stays_explicit_without_becoming_performance_evidence() -> Result<()> {
        let mut value = complete_result(&[42], ExecutionBackend::GuardedHvx);
        value["npuPowerBefore"] =
            json!({"available":false,"acConnected":null,"batterySaver":null,"batteryPercent":null});
        result(
            &value.to_string(),
            &"a".repeat(64),
            &[42],
            ExecutionBackend::GuardedHvx,
            false,
            &json!({"Path":"fixed-driver"}),
        )?;
        value["npuPowerBefore"]
            .as_object_mut()
            .ok_or("Test power missing.")?
            .remove("acConnected");
        assert!(
            result(
                &value.to_string(),
                &"a".repeat(64),
                &[42],
                ExecutionBackend::GuardedHvx,
                false,
                &json!({"Path":"fixed-driver"})
            )
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn hmx_sequence_requires_selected_backend_and_every_token_comparison() -> Result<()> {
        let driver = json!({"Path":"fixed-driver"});
        let value = complete_result(&[42, 43], ExecutionBackend::HmxBitplane);
        result(
            &value.to_string(),
            &"a".repeat(64),
            &[42, 43],
            ExecutionBackend::HmxBitplane,
            false,
            &driver,
        )?;
        for (pointer, wrong) in [
            ("/executionBackend", json!("guarded-hvx")),
            ("/nativeHmxBitplaneControl", json!(false)),
            ("/tokenIds", json!([42])),
            ("/nativeOperationsSubmitted", json!(40)),
            ("/nativeOperationsCompleted", json!(79)),
            ("/maximumOperations", json!(40)),
            ("/sessionSeconds", json!(121)),
            ("/projectionsCompleted", json!(1)),
            ("/integerValuesCompared", json!(2_785_280)),
            ("/tokenComparisons/0/tokenId", json!(43)),
            ("/tokenComparisons/1/index", json!(0)),
            ("/tokenComparisons/1/stateBitwiseEqual", json!(false)),
            ("/tokenComparisons/1/logitsSha256", json!("d".repeat(64))),
        ] {
            let mut changed = value.clone();
            *changed.pointer_mut(pointer).ok_or("Test field missing.")? = wrong;
            assert!(
                result(
                    &changed.to_string(),
                    &"a".repeat(64),
                    &[42, 43],
                    ExecutionBackend::HmxBitplane,
                    false,
                    &driver
                )
                .is_err()
            );
        }
        Ok(())
    }

    #[test]
    fn legacy_and_sequence_requests_keep_closed_token_and_backend_admission() -> Result<()> {
        let legacy = request_value();
        let request: Request =
            serde_json::from_value(legacy.clone()).map_err(|error| error.to_string())?;
        request.validate_tokens()?;
        assert_eq!(request.tokens(), &[42]);
        assert!(request.execution_backend == ExecutionBackend::GuardedHvx);
        for invalid in [
            json!([]),
            json!([43]),
            json!([42, 43, 44, 45, 46, 47, 48, 49, 50]),
            json!([42, 248_320]),
        ] {
            let mut changed = legacy.clone();
            changed["tokenIds"] = invalid;
            let request: Request =
                serde_json::from_value(changed).map_err(|error| error.to_string())?;
            assert!(request.validate_tokens().is_err());
        }
        let mut unknown = legacy;
        unknown["executionBackend"] = json!("auto");
        assert!(serde_json::from_value::<Request>(unknown).is_err());
        Ok(())
    }

    #[test]
    fn eight_token_result_requires_every_projection_and_comparison() -> Result<()> {
        let tokens = [42, 0, 248_319, 42, 43, 44, 45, 46];
        // This checks the result wire format independently of request resource
        // admission; the fixed capture budget cannot admit eight snapshots.
        let mut value = complete_result(&tokens, ExecutionBackend::HmxBitplane);
        let driver = json!({"Path":"fixed-driver"});
        result(
            &value.to_string(),
            &"a".repeat(64),
            &tokens,
            ExecutionBackend::HmxBitplane,
            false,
            &driver,
        )?;
        value["tokenComparisons"][7]["stateBitwiseEqual"] = json!(false);
        assert!(
            result(
                &value.to_string(),
                &"a".repeat(64),
                &tokens,
                ExecutionBackend::HmxBitplane,
                false,
                &driver
            )
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn control_snapshot_budget_admits_three_and_rejects_four_before_intake() -> Result<()> {
        for (count, expected, fits) in [
            (1, 157_024_256, true),
            (2, 314_179_584, true),
            (3, 471_465_984, true),
            (4, 628_883_456, false),
            (8, 1_259_864_064, false),
        ] {
            assert_eq!(required_control_state_bytes(count)?, expected);
            let mut value = request_value();
            value["tokenIds"] = json!(vec![42; count]);
            let request: Request =
                serde_json::from_value(value).map_err(|error| error.to_string())?;
            let admission = request.validate_tokens();
            if fits {
                admission?;
            } else {
                let reason = admission
                    .err()
                    .ok_or("Oversized snapshot control was admitted.")?;
                assert!(reason.contains(&expected.to_string()));
                assert!(reason.contains(&STATE_BUDGET_BYTES.to_string()));
            }
        }
        assert!(required_control_state_bytes(0).is_err());
        assert!(required_control_state_bytes(9).is_err());
        Ok(())
    }

    #[test]
    fn resident_hmx_result_binds_graph_parity_and_actual_cache_accounting() -> Result<()> {
        let driver = json!({"Path":"fixed-driver"});
        let tokens = [42, 43, 128];
        let mut value = complete_result(&tokens, ExecutionBackend::HmxBitplane);
        value["residentHostWeights"] = json!(true);
        value["integerAudit"] = json!("graph-parity-only");
        value["integerValuesCompared"] = json!(0);
        value["nativeResources"] = json!({
            "residentWeightBytesPrepared":RESIDENT_WEIGHT_BYTES,
            "mappedAllocationBytes":RESIDENT_MAPPED_BYTES,
        });
        value["hostResidency"] = json!({"tensorsPrepared":1,"cacheHits":2,
            "weightPayloadBytesPrepared":RESIDENT_WEIGHT_BYTES,"metadataPayloadBytes":640});
        result(
            &value.to_string(),
            &"a".repeat(64),
            &tokens,
            ExecutionBackend::HmxBitplane,
            true,
            &driver,
        )?;
        for (pointer, wrong) in [
            ("/residentHostWeights", json!(false)),
            ("/integerAudit", json!("all-returned-integers")),
            ("/integerValuesCompared", json!(1)),
            ("/nativeResources/residentWeightBytesPrepared", json!(0)),
            (
                "/nativeResources/mappedAllocationBytes",
                json!(RESIDENT_MAPPED_BYTES + 1),
            ),
            ("/hostResidency/tensorsPrepared", json!(2)),
            ("/hostResidency/cacheHits", json!(1)),
            ("/hostResidency/weightPayloadBytesPrepared", json!(0)),
            ("/hostResidency/metadataPayloadBytes", json!(0)),
            ("/tokenComparisons/2/stateBitwiseEqual", json!(false)),
        ] {
            let mut changed = value.clone();
            *changed.pointer_mut(pointer).ok_or("Test field missing.")? = wrong;
            assert!(
                result(
                    &changed.to_string(),
                    &"a".repeat(64),
                    &tokens,
                    ExecutionBackend::HmxBitplane,
                    true,
                    &driver,
                )
                .is_err()
            );
        }
        assert!(
            result(
                &value.to_string(),
                &"a".repeat(64),
                &tokens,
                ExecutionBackend::HmxBitplane,
                false,
                &driver,
            )
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn resident_request_requires_explicit_hmx_and_defaults_to_streaming() -> Result<()> {
        let mut value = request_value();
        let legacy: Request =
            serde_json::from_value(value.clone()).map_err(|error| error.to_string())?;
        assert!(!legacy.resident_host_weights);
        value["residentHostWeights"] = json!(true);
        let guarded: Request =
            serde_json::from_value(value.clone()).map_err(|error| error.to_string())?;
        assert!(guarded.validate_selection().is_err());
        value["executionBackend"] = json!("hmx-bitplane");
        let resident: Request = serde_json::from_value(value).map_err(|error| error.to_string())?;
        resident.validate_selection()?;
        Ok(())
    }

    #[test]
    fn adapter_request_rejects_unbound_paths_and_unsupported_worker_policies() -> Result<()> {
        for (adapter_case, path, workers) in [
            ("none", json!("adapter"), 1),
            ("loaded-zero", Value::Null, 4),
            ("standard-scale-1.3", Value::Null, 4),
            ("none", Value::Null, 2),
        ] {
            let mut value = request_value();
            value["adapterCase"] = json!(adapter_case);
            value["adapterPath"] = path;
            value["rowWorkers"] = json!(workers);
            let request: Request =
                serde_json::from_value(value).map_err(|error| error.to_string())?;
            assert!(request.validate_selection().is_err());
        }
        for (field, invalid) in [("adapterCase", "custom"), ("cpuBackend", "auto")] {
            let mut value = request_value();
            value[field] = json!(invalid);
            assert!(serde_json::from_value::<Request>(value).is_err());
        }
        Ok(())
    }

    #[test]
    fn adapter_result_binds_zero_identity_and_standard_application_to_cpu_policy() -> Result<()> {
        for (adapter_case, loaded, baseline_loaded, scale_bits) in [
            ("none", false, false, 0),
            ("loaded-zero", true, false, 0),
            (
                "standard-scale-1.3",
                true,
                true,
                STANDARD_ADAPTER_SCALE_BITS,
            ),
        ] {
            let mut value = request_value();
            value["adapterCase"] = json!(adapter_case);
            value["adapterPath"] = if loaded {
                json!("adapter")
            } else {
                Value::Null
            };
            value["cpuBackend"] = json!("neon");
            value["rowWorkers"] = json!(4);
            let request: Request =
                serde_json::from_value(value).map_err(|error| error.to_string())?;
            request.validate_selection()?;
            let value = json!({"adapterCase":adapter_case,"adapterLoaded":loaded,
                "adapterSha256":if loaded { json!(ADAPTER_SHA256) } else { Value::Null },
                "adapterScaleBits":scale_bits,"cpuBaselineAdapterLoaded":baseline_loaded,
                "cpuBaselineAdapterScaleBits":scale_bits,"cpuBackend":"neon","rowWorkers":4});
            selection_result(&value, &request)?;
            for (field, wrong) in [
                ("adapterCase", json!("custom")),
                ("adapterLoaded", json!(!loaded)),
                ("adapterSha256", json!("0".repeat(64))),
                ("adapterScaleBits", json!(1)),
                ("cpuBaselineAdapterLoaded", json!(!baseline_loaded)),
                ("cpuBaselineAdapterScaleBits", json!(1)),
                ("cpuBackend", json!("scalar")),
                ("rowWorkers", json!(1)),
            ] {
                let mut changed = value.clone();
                changed[field] = wrong;
                assert!(selection_result(&changed, &request).is_err());
            }
        }
        Ok(())
    }

    #[test]
    fn completed_cycle_requires_every_serialized_native_cleanup_result() -> Result<()> {
        let driver = json!({"Path":"fixed-driver"});
        let value = complete_result(&[42, 43], ExecutionBackend::HmxBitplane);
        for (pointer, wrong) in [
            ("/nativeCleanup", Value::Null),
            ("/nativeCleanup/queueStopResult", json!(1)),
            ("/nativeCleanup/queueCloseResult", Value::Null),
            ("/nativeCleanup/unmappingResult", json!(512)),
            ("/nativeCleanup/moduleCloseResult", json!("0")),
            ("/nativeCleanup/freeCompleted", json!(false)),
            ("/nativeCleanup/poisoned", json!(true)),
            ("/nativeCleanup/ownerPinnedUntilExit", json!(false)),
        ] {
            let mut changed = value.clone();
            *changed.pointer_mut(pointer).ok_or("Test field missing.")? = wrong;
            assert!(
                result(
                    &changed.to_string(),
                    &"a".repeat(64),
                    &[42, 43],
                    ExecutionBackend::HmxBitplane,
                    false,
                    &driver,
                )
                .is_err()
            );
        }
        Ok(())
    }

    #[test]
    fn retained_hmx_two_token_result_replays_native_owner_pinning_policy() -> Result<()> {
        // Exact terminal event from the single 2026-11-03 owner device attempt.
        // This offline parser replay never loads a model or opens the driver.
        let retained = include_str!("../../tests/fixtures/hexagon-hmx-two-token-result.json");
        let driver = json!({"Path":"C:\\WINDOWS\\System32\\DriverStore\\FileRepository\\qcnspmcdm8480.inf_arm64_4feba84cfc7ee278\\libcdsprpc.dll"});
        let accepted = result(
            retained,
            "c76737a14f141b4f8236ad3d19cae16e5c4934b03055eb1c428928451d405107",
            &[42, 43],
            ExecutionBackend::HmxBitplane,
            false,
            &driver,
        )?;
        assert_eq!(accepted["integerValuesCompared"], 11_141_120);
        assert_eq!(accepted["nativeOperationsCompleted"], 80);
        assert_eq!(accepted["nativeCleanup"]["ownerPinnedUntilExit"], true);
        assert_eq!(
            accepted["nativeResources"]["effectiveHmxPlacementVerified"],
            false
        );
        assert!(accepted["nativeResources"]["effectiveVtcmBytes"].is_null());
        Ok(())
    }

    #[test]
    fn batch_four_hmx_result_binds_queue_writes_and_mapped_allocation_extent() -> Result<()> {
        let driver = json!({"Path":"fixed-driver"});
        let tokens = [42, 43, 128];
        let mut req_val = request_value();
        req_val["batchFourOperations"] = json!(true);
        let invalid_req: Request =
            serde_json::from_value(req_val.clone()).map_err(|error| error.to_string())?;
        assert!(invalid_req.validate_selection().is_err());
        req_val["executionBackend"] = json!("hmx-bitplane");
        req_val["tokenIds"] = json!(tokens);
        let valid_req: Request =
            serde_json::from_value(req_val).map_err(|error| error.to_string())?;
        valid_req.validate_selection()?;

        for resident in [false, true] {
            let mut value = complete_result(&tokens, ExecutionBackend::HmxBitplane);
            value["batchFourOperations"] = json!(true);
            value["nativeBatchOperations"] = json!(4);
            value["nativeQueueWritesSubmitted"] = json!(30);
            value["nativeQueueWritesCompleted"] = json!(30);
            value["adapterCase"] = json!("none");
            value["adapterLoaded"] = json!(false);
            value["adapterSha256"] = Value::Null;
            value["adapterScaleBits"] = json!(0);
            value["cpuBaselineAdapterLoaded"] = json!(false);
            value["cpuBaselineAdapterScaleBits"] = json!(0);
            value["cpuBackend"] = json!("scalar");
            value["rowWorkers"] = json!(1);
            if resident {
                value["residentHostWeights"] = json!(true);
                value["integerAudit"] = json!("graph-parity-only");
                value["integerValuesCompared"] = json!(0);
                value["nativeResources"] = json!({
                    "residentWeightBytesPrepared":RESIDENT_WEIGHT_BYTES,
                    "mappedAllocationBytes":BATCH_RESIDENT_MAPPED_BYTES,
                });
                value["hostResidency"] = json!({"tensorsPrepared":1,"cacheHits":2,
                    "weightPayloadBytesPrepared":RESIDENT_WEIGHT_BYTES,"metadataPayloadBytes":640});
            } else {
                value["nativeResources"] = json!({
                    "residentWeightBytesPrepared":0,
                    "mappedAllocationBytes":BATCH_STREAMING_MAPPED_BYTES,
                });
            }
            let accepted = result(
                &value.to_string(),
                &"a".repeat(64),
                &tokens,
                ExecutionBackend::HmxBitplane,
                resident,
                &driver,
            )?;
            if !resident {
                selection_result(&accepted, &valid_req)?;
            }
            for (pointer, wrong) in [
                ("/nativeBatchOperations", json!(1)),
                ("/nativeQueueWritesSubmitted", json!(120)),
                ("/nativeQueueWritesCompleted", json!(29)),
                (
                    "/nativeResources/mappedAllocationBytes",
                    json!(RESIDENT_MAPPED_BYTES),
                ),
            ] {
                let mut changed = value.clone();
                *changed.pointer_mut(pointer).ok_or("Test field missing.")? = wrong;
                assert!(
                    result(
                        &changed.to_string(),
                        &"a".repeat(64),
                        &tokens,
                        ExecutionBackend::HmxBitplane,
                        resident,
                        &driver,
                    )
                    .is_err()
                );
            }
        }
        Ok(())
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "End-to-end batch-4 resident multi-projection contract test keeps admission and rejection cases together."
    )]
    fn multi_projection_batch4_resident_binds_two_tensors_arena_and_queue_writes() -> Result<()> {
        let driver = json!({"Path":"fixed-driver"});
        let tokens = [42, 43, 128];
        let projections = ["blk.0.ffn_up.weight", "blk.0.ffn_gate.weight"];
        let mut req_val = request_value();
        req_val["executionBackend"] = json!("hmx-bitplane");
        req_val["residentHostWeights"] = json!(true);
        req_val["batchFourOperations"] = json!(true);
        req_val["tokenIds"] = json!(tokens);
        req_val["projections"] = json!(projections);
        let valid_req: Request =
            serde_json::from_value(req_val.clone()).map_err(|error| error.to_string())?;
        valid_req.validate_tokens()?;
        valid_req.validate_selection()?;
        for invalid in [
            json!([]),
            json!(["blk.0.ffn_gate.weight"]),
            json!(["blk.0.ffn_up.weight", "blk.0.ffn_up.weight"]),
            json!(["blk.0.ffn_up.weight", "blk.0.ffn_down.weight"]),
            json!([
                "blk.0.ffn_up.weight",
                "blk.0.ffn_gate.weight",
                "blk.1.ffn_up.weight"
            ]),
        ] {
            let mut invalid_req = req_val.clone();
            invalid_req["projections"] = invalid;
            let parsed: Request =
                serde_json::from_value(invalid_req).map_err(|error| error.to_string())?;
            assert!(parsed.validate_selection().is_err());
        }
        let mut value = complete_result(&tokens, ExecutionBackend::HmxBitplane);
        for (key, val) in [
            ("projections", json!(projections)),
            ("residentHostWeights", json!(true)),
            ("batchFourOperations", json!(true)),
            ("nativeBatchOperations", json!(4)),
            ("integerAudit", json!("graph-parity-only")),
            ("integerValuesCompared", json!(0)),
            ("maximumOperations", json!(240)),
            ("nativeOperationsSubmitted", json!(240)),
            ("nativeOperationsCompleted", json!(240)),
            ("nativeQueueWritesSubmitted", json!(60)),
            ("nativeQueueWritesCompleted", json!(60)),
            ("projectionsCompleted", json!(6)),
            ("adapterCase", json!("none")),
            ("adapterLoaded", json!(false)),
            ("adapterSha256", Value::Null),
            ("adapterScaleBits", json!(0)),
            ("cpuBaselineAdapterLoaded", json!(false)),
            ("cpuBaselineAdapterScaleBits", json!(0)),
            ("cpuBackend", json!("scalar")),
            ("rowWorkers", json!(1)),
            (
                "nativeResources",
                json!({
                    "residentWeightBytesPrepared": 2 * RESIDENT_WEIGHT_BYTES,
                    "mappedAllocationBytes": BATCH_STREAMING_MAPPED_BYTES + 2 * RESIDENT_WEIGHT_BYTES,
                }),
            ),
            (
                "hostResidency",
                json!({
                    "tensorsPrepared": 2,
                    "cacheHits": 4,
                    "weightPayloadBytesPrepared": 2 * RESIDENT_WEIGHT_BYTES,
                    "metadataPayloadBytes": 1280,
                }),
            ),
        ] {
            value[key] = val;
        }
        let accepted = result(
            &value.to_string(),
            &"a".repeat(64),
            &tokens,
            ExecutionBackend::HmxBitplane,
            true,
            &driver,
        )?;
        selection_result(&accepted, &valid_req)?;
        for (pointer, wrong) in [
            ("/projections", json!(["blk.0.ffn_up.weight"])),
            ("/projectionsCompleted", json!(3)),
            ("/nativeOperationsCompleted", json!(120)),
            ("/nativeQueueWritesCompleted", json!(30)),
            ("/hostResidency/tensorsPrepared", json!(1)),
            ("/hostResidency/cacheHits", json!(2)),
            (
                "/nativeResources/residentWeightBytesPrepared",
                json!(RESIDENT_WEIGHT_BYTES),
            ),
            (
                "/nativeResources/mappedAllocationBytes",
                json!(BATCH_RESIDENT_MAPPED_BYTES),
            ),
        ] {
            let mut changed = value.clone();
            *changed.pointer_mut(pointer).ok_or("Test field missing.")? = wrong;
            assert!(
                result(
                    &changed.to_string(),
                    &"a".repeat(64),
                    &tokens,
                    ExecutionBackend::HmxBitplane,
                    true,
                    &driver
                )
                .is_err()
            );
        }
        Ok(())
    }

    #[test]
    fn multi_projection_batch10_resident_binds_two_tensors_arena_and_queue_writes() -> Result<()> {
        let driver = json!({"Path":"fixed-driver"});
        let tokens = [42, 43, 128];
        let projections = ["blk.0.ffn_up.weight", "blk.0.ffn_gate.weight"];
        let mut req_val = request_value();
        req_val["batchTenOperations"] = json!(true);
        let invalid_req: Request =
            serde_json::from_value(req_val.clone()).map_err(|error| error.to_string())?;
        assert!(invalid_req.validate_selection().is_err());
        req_val["executionBackend"] = json!("hmx-bitplane");
        req_val["residentHostWeights"] = json!(true);
        req_val["tokenIds"] = json!(tokens);
        req_val["projections"] = json!(projections);
        let valid_req: Request =
            serde_json::from_value(req_val.clone()).map_err(|error| error.to_string())?;
        valid_req.validate_tokens()?;
        valid_req.validate_selection()?;
        let mut conflicting = req_val;
        conflicting["batchFourOperations"] = json!(true);
        let conflicting_req: Request =
            serde_json::from_value(conflicting).map_err(|error| error.to_string())?;
        assert!(conflicting_req.validate_selection().is_err());

        let mut value = complete_result(&tokens, ExecutionBackend::HmxBitplane);
        for (key, val) in [
            ("projections", json!(projections)),
            ("residentHostWeights", json!(true)),
            ("batchFourOperations", json!(false)),
            ("batchTenOperations", json!(true)),
            ("nativeBatchOperations", json!(10)),
            ("integerAudit", json!("graph-parity-only")),
            ("integerValuesCompared", json!(0)),
            ("maximumOperations", json!(240)),
            ("nativeOperationsSubmitted", json!(240)),
            ("nativeOperationsCompleted", json!(240)),
            ("nativeQueueWritesSubmitted", json!(24)),
            ("nativeQueueWritesCompleted", json!(24)),
            ("projectionsCompleted", json!(6)),
            ("adapterCase", json!("none")),
            ("adapterLoaded", json!(false)),
            ("adapterSha256", Value::Null),
            ("adapterScaleBits", json!(0)),
            ("cpuBaselineAdapterLoaded", json!(false)),
            ("cpuBaselineAdapterScaleBits", json!(0)),
            ("cpuBackend", json!("scalar")),
            ("rowWorkers", json!(1)),
            (
                "nativeResources",
                json!({
                    "residentWeightBytesPrepared": 2 * RESIDENT_WEIGHT_BYTES,
                    "mappedAllocationBytes": BATCH10_STREAMING_MAPPED_BYTES + 2 * RESIDENT_WEIGHT_BYTES,
                }),
            ),
            (
                "hostResidency",
                json!({
                    "tensorsPrepared": 2,
                    "cacheHits": 4,
                    "weightPayloadBytesPrepared": 2 * RESIDENT_WEIGHT_BYTES,
                    "metadataPayloadBytes": 1280,
                }),
            ),
        ] {
            value[key] = val;
        }
        let accepted = result(
            &value.to_string(),
            &"a".repeat(64),
            &tokens,
            ExecutionBackend::HmxBitplane,
            true,
            &driver,
        )?;
        selection_result(&accepted, &valid_req)?;
        for (pointer, wrong) in [
            ("/nativeBatchOperations", json!(4)),
            ("/nativeQueueWritesSubmitted", json!(60)),
            ("/nativeQueueWritesCompleted", json!(23)),
            (
                "/nativeResources/mappedAllocationBytes",
                json!(BATCH10_RESIDENT_MAPPED_BYTES),
            ),
        ] {
            let mut changed = value.clone();
            *changed.pointer_mut(pointer).ok_or("Test field missing.")? = wrong;
            assert!(
                result(
                    &changed.to_string(),
                    &"a".repeat(64),
                    &tokens,
                    ExecutionBackend::HmxBitplane,
                    true,
                    &driver
                )
                .is_err()
            );
        }
        Ok(())
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "End-to-end native-q2 resident multi-layer contract test keeps admission and rejection cases together."
    )]
    fn native_q2_resident_result_binds_single_op_queue_writes_and_bounded_tolerance() -> Result<()>
    {
        let driver = json!({"Path":"fixed-driver"});
        let tokens = [42, 43, 128];
        let projections = ["blk.0.ffn_up.weight", "blk.0.ffn_gate.weight"];
        let mut req_val = request_value();
        req_val["executionBackend"] = json!("native-q2");
        req_val["residentHostWeights"] = json!(true);
        req_val["tokenIds"] = json!(tokens);
        req_val["projections"] = json!(projections);
        let valid_req: Request =
            serde_json::from_value(req_val.clone()).map_err(|error| error.to_string())?;
        valid_req.validate_tokens()?;
        valid_req.validate_selection()?;
        let mut invalid_batch = req_val;
        invalid_batch["batchFourOperations"] = json!(true);
        let invalid_req: Request =
            serde_json::from_value(invalid_batch).map_err(|error| error.to_string())?;
        assert!(invalid_req.validate_selection().is_err());

        let multi_layer = [
            "blk.0.ffn_up.weight",
            "blk.0.ffn_gate.weight",
            "blk.0.ffn_down.weight",
            "blk.0.attn_qkv.weight",
            "blk.0.attn_gate.weight",
            "blk.0.ssm_out.weight",
            "blk.3.ffn_up.weight",
            "blk.3.ffn_gate.weight",
            "blk.3.ffn_down.weight",
            "blk.3.attn_q.weight",
            "blk.3.attn_k.weight",
            "blk.3.attn_v.weight",
            "blk.3.attn_output.weight",
        ];
        assert_eq!(
            expected_resident_weight_bytes(&multi_layer, ExecutionBackend::NativeQ2)?,
            212_336_640
        );
        let full_arena = [
            "blk.0.ffn_up.weight",
            "blk.0.ffn_gate.weight",
            "blk.0.ffn_down.weight",
            "blk.0.attn_qkv.weight",
            "blk.0.attn_gate.weight",
            "blk.0.ssm_out.weight",
            "blk.1.ffn_up.weight",
            "blk.1.ffn_gate.weight",
            "blk.1.ffn_down.weight",
            "blk.1.attn_qkv.weight",
            "blk.1.attn_gate.weight",
            "blk.1.ssm_out.weight",
            "blk.2.ffn_up.weight",
            "blk.2.ffn_gate.weight",
            "blk.2.ffn_down.weight",
            "blk.2.attn_qkv.weight",
            "blk.2.attn_gate.weight",
            "blk.2.ssm_out.weight",
            "blk.3.ffn_up.weight",
            "blk.3.ffn_gate.weight",
            "blk.3.ffn_down.weight",
            "blk.3.attn_q.weight",
            "blk.3.attn_k.weight",
            "blk.3.attn_v.weight",
            "blk.3.attn_output.weight",
            "blk.4.ffn_up.weight",
            "blk.4.ffn_gate.weight",
            "blk.4.ffn_down.weight",
        ];
        assert_eq!(
            expected_resident_weight_bytes(&full_arena, ExecutionBackend::NativeQ2)?,
            502_824_960
        );
        let dual_arena = [
            "blk.0.ffn_up.weight",
            "blk.0.ffn_gate.weight",
            "blk.0.ffn_down.weight",
            "blk.0.attn_qkv.weight",
            "blk.0.attn_gate.weight",
            "blk.0.ssm_out.weight",
            "blk.1.ffn_up.weight",
            "blk.1.ffn_gate.weight",
            "blk.1.ffn_down.weight",
            "blk.1.attn_qkv.weight",
            "blk.1.attn_gate.weight",
            "blk.1.ssm_out.weight",
            "blk.2.ffn_up.weight",
            "blk.2.ffn_gate.weight",
            "blk.2.ffn_down.weight",
            "blk.2.attn_qkv.weight",
            "blk.2.attn_gate.weight",
            "blk.2.ssm_out.weight",
            "blk.3.ffn_up.weight",
            "blk.3.ffn_gate.weight",
            "blk.3.ffn_down.weight",
            "blk.3.attn_q.weight",
            "blk.3.attn_k.weight",
            "blk.3.attn_v.weight",
            "blk.3.attn_output.weight",
            "blk.4.ffn_up.weight",
            "blk.4.ffn_gate.weight",
            "blk.4.ffn_down.weight",
            "blk.4.attn_qkv.weight",
            "blk.4.attn_gate.weight",
            "blk.4.ssm_out.weight",
            "blk.5.ffn_up.weight",
            "blk.5.ffn_gate.weight",
            "blk.5.ffn_down.weight",
            "blk.5.attn_qkv.weight",
            "blk.5.attn_gate.weight",
            "blk.5.ssm_out.weight",
            "blk.6.ffn_up.weight",
            "blk.6.ffn_gate.weight",
            "blk.6.ffn_down.weight",
            "blk.6.attn_qkv.weight",
            "blk.6.attn_gate.weight",
            "blk.6.ssm_out.weight",
            "blk.7.ffn_up.weight",
            "blk.7.ffn_gate.weight",
            "blk.7.ffn_down.weight",
            "blk.7.attn_q.weight",
            "blk.7.attn_k.weight",
            "blk.7.attn_v.weight",
            "blk.7.attn_output.weight",
            "blk.8.ffn_up.weight",
            "blk.8.ffn_gate.weight",
            "blk.8.ffn_down.weight",
            "blk.8.attn_qkv.weight",
            "blk.8.attn_gate.weight",
            "blk.8.ssm_out.weight",
        ];
        assert_eq!(
            expected_resident_weight_bytes(&dual_arena, ExecutionBackend::NativeQ2)?,
            962_887_680
        );
        let quad_arena: Vec<String> = (0..18)
            .flat_map(|layer| {
                let suffixes: &[&str] = if (layer + 1) % 4 == 0 {
                    &[
                        "ffn_up.weight",
                        "ffn_gate.weight",
                        "ffn_down.weight",
                        "attn_q.weight",
                        "attn_k.weight",
                        "attn_v.weight",
                        "attn_output.weight",
                    ]
                } else {
                    &[
                        "ffn_up.weight",
                        "ffn_gate.weight",
                        "ffn_down.weight",
                        "attn_qkv.weight",
                        "attn_gate.weight",
                        "ssm_out.weight",
                    ]
                };
                suffixes
                    .iter()
                    .map(move |suffix| format!("blk.{layer}.{suffix}"))
            })
            .collect();
        let quad_arena_refs: Vec<&str> = quad_arena.iter().map(String::as_str).collect();
        assert_eq!(quad_arena_refs.len(), 112);
        assert_eq!(
            expected_resident_weight_bytes(&quad_arena_refs, ExecutionBackend::NativeQ2)?,
            1_925_775_360
        );
        let mut quad_req_val = request_value();
        quad_req_val["executionBackend"] = json!("native-q2");
        quad_req_val["residentHostWeights"] = json!(true);
        quad_req_val["tokenIds"] = json!(tokens);
        quad_req_val["projections"] = json!(quad_arena);
        let quad_req: Request =
            serde_json::from_value(quad_req_val).map_err(|error| error.to_string())?;
        quad_req.validate_selection()?;
        assert!(
            expected_resident_weight_bytes(&multi_layer, ExecutionBackend::GuardedHvx).is_err()
        );

        let tol = json!({"maxAbsDiff":0.12,"meanAbsDiff":0.015,"rmse":0.022,
            "cosineSimilarity":0.999_85,"cpuTop1Token":1024,"npuTop1Token":1024,
            "top1Match":true,"top5Overlap":5});
        let mut value = complete_result(&tokens, ExecutionBackend::NativeQ2);
        let comparisons: Vec<_> = tokens.iter().enumerate().map(|(index, token)| {
            json!({"index":index,"tokenId":token,"logitsBitwiseEqual":false,"stateBitwiseEqual":false,
                "logitsBoundedTolerancePass":true,"numericalTolerance":tol,
                "logitsSha256":"b".repeat(64),"stateSha256":"c".repeat(64)})
        }).collect();
        for (key, val) in [
            ("projections", json!(projections)),
            ("residentHostWeights", json!(true)),
            ("batchFourOperations", json!(false)),
            ("batchTenOperations", json!(false)),
            ("nativeBatchOperations", json!(1)),
            ("integerAudit", json!("bounded-float-tolerance")),
            ("integerValuesCompared", json!(0)),
            ("maximumOperations", json!(6)),
            ("nativeOperationsSubmitted", json!(6)),
            ("nativeOperationsCompleted", json!(6)),
            ("nativeQueueWritesSubmitted", json!(6)),
            ("nativeQueueWritesCompleted", json!(6)),
            ("projectionsCompleted", json!(6)),
            ("logitsBitwiseEqual", json!(false)),
            ("stateBitwiseEqual", json!(false)),
            ("logitsBoundedTolerancePass", json!(true)),
            ("numericalTolerance", tol),
            ("tokenComparisons", json!(comparisons)),
            ("adapterCase", json!("none")),
            ("adapterLoaded", json!(false)),
            ("adapterSha256", Value::Null),
            ("adapterScaleBits", json!(0)),
            ("cpuBaselineAdapterLoaded", json!(false)),
            ("cpuBaselineAdapterScaleBits", json!(0)),
            ("cpuBackend", json!("scalar")),
            ("rowWorkers", json!(1)),
            (
                "nativeResources",
                json!({"residentWeightBytesPrepared": 2 * RESIDENT_WEIGHT_BYTES,
                    "mappedAllocationBytes": NATIVE_Q2_STREAMING_MAPPED_BYTES + 2 * RESIDENT_WEIGHT_BYTES}),
            ),
            (
                "hostResidency",
                json!({"tensorsPrepared": 2, "cacheHits": 4,
                    "weightPayloadBytesPrepared": 2 * RESIDENT_WEIGHT_BYTES, "metadataPayloadBytes": 1280}),
            ),
        ] {
            value[key] = val;
        }
        let accepted = result(
            &value.to_string(),
            &"a".repeat(64),
            &tokens,
            ExecutionBackend::NativeQ2,
            true,
            &driver,
        )?;
        selection_result(&accepted, &valid_req)?;
        for (pointer, wrong) in [
            ("/maximumOperations", json!(240)),
            ("/nativeOperationsCompleted", json!(240)),
            ("/nativeQueueWritesCompleted", json!(24)),
            ("/integerAudit", json!("graph-parity-only")),
            ("/logitsBoundedTolerancePass", json!(false)),
            ("/numericalTolerance/cosineSimilarity", json!(0.990)),
            (
                "/tokenComparisons/1/logitsBoundedTolerancePass",
                json!(false),
            ),
            (
                "/nativeResources/mappedAllocationBytes",
                json!(NATIVE_Q2_RESIDENT_MAPPED_BYTES),
            ),
        ] {
            let mut changed = value.clone();
            *changed.pointer_mut(pointer).ok_or("Test field missing.")? = wrong;
            assert!(
                result(
                    &changed.to_string(),
                    &"a".repeat(64),
                    &tokens,
                    ExecutionBackend::NativeQ2,
                    true,
                    &driver
                )
                .is_err()
            );
        }
        Ok(())
    }
}
