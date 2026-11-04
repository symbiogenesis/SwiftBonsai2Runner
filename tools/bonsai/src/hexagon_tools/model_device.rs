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
const CLEANUP_SECONDS: u64 = 75;
const CHILD_SECONDS: u64 = 195;
const REQUIRED_AVAILABLE_BYTES: u64 = 18 * 1024 * 1024 * 1024;

#[derive(Clone, Copy, Default, Deserialize, PartialEq, Eq, Serialize)]
enum ExecutionBackend {
    #[default]
    #[serde(rename = "guarded-hvx")]
    GuardedHvx,
    #[serde(rename = "hmx-bitplane")]
    HmxBitplane,
}

impl ExecutionBackend {
    const fn integer_values_per_token(self) -> u64 {
        match self {
            Self::GuardedHvx => 1_392_640,
            Self::HmxBitplane => 5_570_560,
        }
    }
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
    execution_backend: ExecutionBackend,
    maximum_seconds: u32,
}

impl Request {
    fn tokens(&self) -> &[u32] {
        self.token_ids
            .as_deref()
            .unwrap_or_else(|| std::slice::from_ref(&self.token_id))
    }

    fn validate_tokens(&self) -> Result<()> {
        let tokens = self.tokens();
        if !(1..=2).contains(&tokens.len())
            || tokens.first() != Some(&self.token_id)
            || tokens.iter().any(|&token| token >= 248_320)
        {
            return Err("Select one or two explicit Swift tokens beginning with tokenId.".into());
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
    driver: &Value,
) -> Result<Value> {
    if !(1..=2).contains(&tokens.len()) {
        return Err("The model result needs one or two admitted tokens.".into());
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
    if value["schemaVersion"] != 1
        || value["completed"] != true
        || value["requestSha256"] != request_digest
        || value["modelSha256"] != MODEL_SHA256
        || value["projection"] != "blk.0.ffn_up.weight"
        || value["tokenId"] != tokens[0]
        || value["tokenIds"] != json!(tokens)
        || value["executionBackend"] != json!(backend)
        || value["nativeHmxBitplaneControl"] != (backend == ExecutionBackend::HmxBitplane)
        || value["maximumOperations"] != 40 * token_count
        || value["sessionSeconds"] != 120
        || value["driverDll"] != driver["Path"]
        || value["nativeOperationsSubmitted"] != 40 * token_count
        || value["nativeOperationsCompleted"] != 40 * token_count
        || value["projectionsCompleted"] != token_count
        || value["integerValuesCompared"] != backend.integer_values_per_token() * token_count
        || value["logitsBitwiseEqual"] != true
        || value["stateBitwiseEqual"] != true
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
        if comparison["index"] != index
            || comparison["tokenId"] != token
            || comparison["logitsBitwiseEqual"] != true
            || comparison["stateBitwiseEqual"] != true
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

#[expect(
    clippy::too_many_lines,
    reason = "Keep one ordered terminal intake, signed catalog and driver verification, resource admission and owned graph comparison visible together."
)]
fn execute(request: &Request, started: Instant, report: &mut Value) -> Result<()> {
    let mut inputs = device::Inputs::default();
    local_build(request, &mut inputs, started, report)?;
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
    let probe_request = json!({"schemaVersion":1,"modelPath":request.model_path,"packageDirectory":request.package_directory,
        "tokenId":request.token_id,"tokenIds":request.tokens(),"executionBackend":request.execution_backend,
        "contextLimit":request.tokens().len(),"modelMemoryLimitBytes":12_u64*1024*1024*1024,
        "stateBudgetBytes":512_u64*1024*1024,"sessionSeconds":120,"maximumOperations":40*request.tokens().len(),
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
    report["NativeResult"] = result(
        &output.stdout,
        &request_digest,
        request.tokens(),
        request.execution_backend,
        &driver,
    )?;
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
    fs::create_dir(&request.output_directory)
        .map_err(|_| "The model probe requires a new external evidence directory.".to_owned())?;
    let mut report = json!({"schemaVersion":1,"kind":"runner-local-hexagon-model-probe","Outcome":"pending","Stage":"local-build-intake",
        "LocalBuild":true,"HostedBuild":false,"PrismRevision":request.expected_prism_revision,"MaximumSeconds":request.maximum_seconds,
        "CleanupReserveSeconds":CLEANUP_SECONDS,"ModelWorkAttempted":false,"OwnedJobTeardown":"not-started","ProfileEligible":false,
        "PerformanceMeasured":false,"TrustChanged":false,"PowerBefore":power(),"RequiredAvailablePhysicalBytes":REQUIRED_AVAILABLE_BYTES,
        "ExecutionBackend":request.execution_backend,"TokenIds":request.tokens()});
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
            &driver,
        )?;
        for (pointer, wrong) in [
            ("/nativeCompletedCycle", json!(false)),
            ("/nativeOperationsCompleted", json!(39)),
            ("/integerValuesCompared", json!(1392639)),
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
                    &driver
                )
                .is_err()
            );
        }
        Ok(())
    }

    #[test]
    fn legacy_and_sequence_requests_keep_closed_token_and_backend_admission() -> Result<()> {
        let legacy = json!({"repositoryDirectory":"repo","prismRepositoryDirectory":"prism",
            "expectedPrismRevision":"revision","executablePath":"exe","executableSha256":"sha",
            "buildReceiptPath":"receipt","modelPath":"model","packageDirectory":"package",
            "signtoolPath":"sign","outputDirectory":"output","tokenId":42,"maximumSeconds":300});
        let request: Request =
            serde_json::from_value(legacy.clone()).map_err(|error| error.to_string())?;
        request.validate_tokens()?;
        assert_eq!(request.tokens(), &[42]);
        assert!(request.execution_backend == ExecutionBackend::GuardedHvx);
        for invalid in [
            json!([]),
            json!([43]),
            json!([42, 43, 44]),
            json!([42, 248320]),
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
}
