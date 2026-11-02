//! Artifact-bound, externally supervised original-Swift projection qualification.
//!
//! This diagnostic retains exact extracted PQ2/sign identities, accepts one
//! explicitly identified pre-transform F32 vector, and compares every returned
//! limb and the final projection against CPU controls. It never qualifies model
//! activation provenance, logits/state, performance, or serving admission.
#![forbid(unsafe_code)]
#![deny(warnings)]
#![recursion_limit = "256"]

use hexagon_native_rpc::{
    Driver, DriverLocation, GuardedQ2Submitter, Q2ProjectionLimits, Q2ProjectionReport,
};
use packed_projection::{
    GuardedQ2Operation, GuardedQ2Plan, Plan, guarded_q2_projection, tiled_projection,
    transform_input,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read as _, Write as _};
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

const COLUMNS: usize = 5120;
const ROWS: usize = 17408;
const WEIGHT_BYTES: usize = 23_674_880;
const WEIGHT_SHA256: &str = "a0f21e1d4492de2496309b6383b958cba53c54f05c9de710348facdd31706354";
const SIGNS_SHA256: &str = "5ab93190ef5cd4f8322d52c1006783b25a3e118453bda0d0f90b1bf301a67197";
const ARTIFACT_SHA256: &str = "5912bb739217cf25b4283a098c7b893d9baff36bdf2e72d3dbf4cb999d5633d5";
const ALL_SIGNS_SHA256: &str = "3d1ea9136a6044cd680cc1e92cde65e9dd7238e1288fe2effabec5d201c48bed";
const MAX_REQUEST_BYTES: usize = 8192;
const MAX_BINDING_BYTES: usize = 8192;

type Result<T> = std::result::Result<T, &'static str>;

#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Execution {
    CpuControl,
    DeviceGuardedQ2,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    binding_directory: PathBuf,
    activation_file: PathBuf,
    activation_sha256: String,
    output_directory: PathBuf,
    rows: usize,
    tile_rows: usize,
    maximum_seconds: u32,
    execution: Execution,
    package_directory: Option<PathBuf>,
}

impl Request {
    fn validate_shape(&self) -> Result<()> {
        if !(32..=ROWS).contains(&self.rows)
            || !self.rows.is_multiple_of(32)
            || !(32..=self.rows).contains(&self.tile_rows)
            || !self.tile_rows.is_multiple_of(32)
            || !(1..=225).contains(&self.maximum_seconds)
            || (self.execution == Execution::DeviceGuardedQ2
                && !(4..=120).contains(&self.maximum_seconds))
            || !lower_digest(&self.activation_sha256)
            || (self.execution == Execution::DeviceGuardedQ2) != self.package_directory.is_some()
        {
            return Err("request-contract-rejected");
        }
        Ok(())
    }

    fn validate(&self) -> Result<()> {
        self.validate_shape()?;
        external(&self.binding_directory)?;
        external(&self.activation_file)?;
        external(&self.output_directory)?;
        if let Some(package) = &self.package_directory {
            external(package)?;
        }
        Ok(())
    }
}

fn lower_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .flat_map(|byte| [byte >> 4, byte & 15])
        .map(|value| char::from(b"0123456789abcdef"[usize::from(value)]))
        .collect()
}

fn check_time(deadline: Instant) -> Result<()> {
    if Instant::now() >= deadline {
        Err("projection-deadline-expired")
    } else {
        Ok(())
    }
}

fn external(path: &Path) -> Result<()> {
    if !path.is_absolute()
        || path.components().any(|part| match part {
            Component::CurDir | Component::ParentDir => true,
            Component::Normal(name) => {
                name.to_string_lossy().contains(':')
                    || name.to_string_lossy().chars().any(char::is_control)
            }
            _ => false,
        })
    {
        return Err("expanded-external-path-required");
    }
    #[cfg(windows)]
    if !matches!(path.components().next(), Some(Component::Prefix(prefix))
        if matches!(prefix.kind(), std::path::Prefix::Disk(_)))
    {
        return Err("local-drive-path-required");
    }
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err("redirected-storage-rejected");
                }
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt as _;
                    if metadata.file_attributes() & 0x400 != 0 {
                        return Err("redirected-storage-rejected");
                    }
                }
                if metadata.is_dir()
                    && ancestor
                        .join(".git")
                        .try_exists()
                        .map_err(|_| "storage-inspection-failed")?
                {
                    return Err("external-storage-outside-git-required");
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("storage-inspection-failed"),
        }
    }
    Ok(())
}

// On Windows, every input stays open against writes/deletion through final
// publication. Re-reading from a reopened path never supplies an identity.
fn read_held(
    path: &Path,
    maximum: usize,
    exact: bool,
    deadline: Instant,
) -> Result<(File, Vec<u8>)> {
    external(path)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        options.share_mode(1).custom_flags(0x0020_0000);
    }
    let mut file = options.open(path).map_err(|_| "input-open-failed")?;
    let metadata = file.metadata().map_err(|_| "input-metadata-failed")?;
    if !metadata.is_file()
        || metadata.len() == 0
        || metadata.len() > maximum as u64
        || (exact && metadata.len() != maximum as u64)
    {
        return Err("input-extent-rejected");
    }
    let size = usize::try_from(metadata.len()).map_err(|_| "input-extent-rejected")?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(size)
        .map_err(|_| "input-allocation-refused")?;
    bytes.resize(size, 0);
    for chunk in bytes.chunks_mut(65_536) {
        check_time(deadline)?;
        file.read_exact(chunk).map_err(|_| "input-read-failed")?;
    }
    let mut extra = [0];
    if file.read(&mut extra).map_err(|_| "input-read-failed")? != 0 {
        return Err("input-extent-changed");
    }
    check_time(deadline)?;
    Ok((file, bytes))
}

fn f32_values(bytes: &[u8], count: usize) -> Result<Vec<f32>> {
    if bytes.len() != count * size_of::<f32>() {
        return Err("activation-extent-rejected");
    }
    let mut values = Vec::new();
    values
        .try_reserve_exact(count)
        .map_err(|_| "activation-allocation-refused")?;
    for chunk in bytes.chunks_exact(4) {
        let value = f32::from_le_bytes(chunk.try_into().map_err(|_| "activation-extent-rejected")?);
        if !value.is_finite() {
            return Err("activation-nonfinite");
        }
        values.push(value);
    }
    Ok(values)
}

fn binding(bytes: &[u8]) -> Result<()> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| "binding-json-rejected")?;
    // Payload SHA-256 pins below are the authoritative identity. This manifest
    // is independently checked descriptive metadata, never a caller-selected pin.
    for (pointer, expected) in [
        ("/schemaVersion", json!(1)),
        ("/kind", json!("swift-packed-first-projection-binding")),
        ("/artifactBytes", json!(7_206_168_928_u64)),
        ("/artifactSha256", json!(ARTIFACT_SHA256)),
        (
            "/artifactVerification",
            json!("complete-size-and-sha256-on-retained-handle"),
        ),
        ("/tensor/name", json!("blk.0.ffn_up.weight")),
        ("/tensor/typeId", json!(142)),
        ("/tensor/columns", json!(COLUMNS)),
        ("/tensor/rows", json!(ROWS)),
        ("/tensor/bytes", json!(WEIGHT_BYTES)),
        ("/tensor/file", json!("weights.pq2")),
        ("/tensor/sha256", json!(WEIGHT_SHA256)),
        ("/signs/file", json!("signs.i8")),
        ("/signs/sha256", json!(SIGNS_SHA256)),
        ("/signs/sourceKey", json!("prism.hadamard.sign_values")),
        ("/signs/sourceElementStart", json!(0)),
        ("/signs/count", json!(COLUMNS)),
        ("/signs/completeArrayElements", json!(28672)),
        ("/signs/encoding", json!("twos-complement-i8")),
        (
            "/signs/completeArrayCanonicalI32LeSha256",
            json!(ALL_SIGNS_SHA256),
        ),
        ("/transform/version", json!(1)),
        ("/transform/blockSize", json!(1024)),
        (
            "/transform/name",
            json!("normalized-sylvester-walsh-hadamard"),
        ),
        ("/transform/axis", json!("input-last-dimension")),
        ("/transform/signMode", json!("explicit")),
        ("/transform/signsAppliedBeforeRotation", json!(true)),
        ("/transform/owner", json!("CPU")),
    ] {
        if value.pointer(pointer) != Some(&expected) {
            return Err("binding-identity-rejected");
        }
    }
    Ok(())
}

fn exact_values(expected: &[f32], actual: &[f32]) -> Result<()> {
    if expected.len() != actual.len()
        || expected
            .iter()
            .zip(actual)
            .any(|(left, right)| left.to_bits() != right.to_bits())
    {
        Err("exact-numerical-parity-rejected")
    } else {
        Ok(())
    }
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        options.share_mode(1);
    }
    let mut file = options.open(path).map_err(|_| "output-create-failed")?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|_| "output-sync-failed")?;
    Ok(file)
}

fn native_json(report: &Q2ProjectionReport) -> Value {
    let queue = &report.queue;
    json!({
        "stage": queue.stage,
        "architecture": queue.metadata.architecture,
        "effectiveDomain": queue.metadata.effective_domain,
        "processInitializationResult": queue.process_initialization_result,
        "unsignedProcessRequested": queue.unsigned_process_requested,
        "moduleOpenResult": queue.module_open_result,
        "mappingResult": queue.mapping_result,
        "queueCreateResult": queue.queue_create_result,
        "queueExportResult": queue.queue_export_result,
        "queueStartResult": queue.queue_start_result,
        "lastQueueWriteResult": queue.queue_write_result,
        "lastQueueReadResult": queue.queue_read_result,
        "queueStopResult": queue.queue_stop_result,
        "queueCloseResult": queue.queue_close_result,
        "unmappingResult": queue.unmapping_result,
        "moduleCloseResult": queue.module_close_result,
        "freeCompleted": queue.free_completed,
        "ownerPinnedUntilExit": queue.owner_pinned_until_exit,
        "poisoned": queue.poisoned,
        "operationsSubmitted": report.operations_submitted,
        "operationsCompleted": report.operations_completed,
        "workCompleted": report.work_completed,
        "deadlineExpired": report.deadline_expired,
        "completedCycle": report.completed_cycle(),
        "completeVendorTeardownQualified": false,
        "selectedDspBytesIndependentlyObserved": false,
    })
}

fn emit_progress(stage: &str, completed: u32) -> bool {
    match stage {
        "projection-queue-write-entered" | "projection-queue-read-entered" => completed == 0,
        "projection-operation-complete" => completed == 1 || completed.is_multiple_of(256),
        _ => true,
    }
}

fn overrides(package: Option<&Path>) -> Result<()> {
    let forbidden = [
        "ADSP_", "CDSP_", "FASTRPC_", "RPCMEM_", "GGML_", "GENIEX_", "QNN_", "QAIRT_", "HEXAGON_",
    ];
    if std::env::vars_os().any(|(name, value)| {
        let name = name.to_string_lossy().to_ascii_uppercase();
        forbidden.iter().any(|prefix| name.starts_with(prefix))
            && !(name == "ADSP_LIBRARY_PATH"
                && package.is_some_and(|path| path.as_os_str() == value))
    }) {
        return Err("inherited-runtime-override-rejected");
    }
    Ok(())
}

fn projection(request: Request, started: Instant) -> Result<Value> {
    request.validate()?;
    let deadline = started + Duration::from_secs(u64::from(request.maximum_seconds));
    check_time(deadline)?;
    overrides(request.package_directory.as_deref())?;
    if request
        .output_directory
        .try_exists()
        .map_err(|_| "output-inspection-failed")?
        || !request.output_directory.parent().is_some_and(Path::is_dir)
    {
        return Err("new-external-output-directory-required");
    }
    let output_name = request
        .output_directory
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("output-name-rejected")?;
    let pending_output = request
        .output_directory
        .with_file_name(format!("{output_name}.pending"));
    external(&pending_output)?;
    fs::create_dir(&pending_output).map_err(|_| "new-pending-output-directory-required")?;
    let (_manifest_hold, manifest) = read_held(
        &request.binding_directory.join("binding.json"),
        MAX_BINDING_BYTES,
        false,
        deadline,
    )?;
    binding(&manifest)?;
    let (_weight_hold, weights) = read_held(
        &request.binding_directory.join("weights.pq2"),
        WEIGHT_BYTES,
        true,
        deadline,
    )?;
    let (_sign_hold, sign_bytes) = read_held(
        &request.binding_directory.join("signs.i8"),
        COLUMNS,
        true,
        deadline,
    )?;
    let (_activation_hold, activation_bytes) =
        read_held(&request.activation_file, COLUMNS * 4, true, deadline)?;
    if digest(&weights) != WEIGHT_SHA256
        || digest(&sign_bytes) != SIGNS_SHA256
        || digest(&activation_bytes) != request.activation_sha256
    {
        return Err("input-sha256-rejected");
    }
    check_time(deadline)?;
    let input = f32_values(&activation_bytes, COLUMNS)?;
    let signs: Vec<i8> = sign_bytes.iter().map(|byte| byte.cast_signed()).collect();
    let transformed = transform_input(&input, &signs).map_err(|_| "cpu-transform-rejected")?;
    let selected = &weights[..request.rows * COLUMNS / 128 * 34];
    let cpu_plan =
        Plan::new(COLUMNS, request.rows, 32, 4, usize::MAX).map_err(|_| "cpu-plan-rejected")?;
    let mut oracle = Vec::new();
    tiled_projection(cpu_plan, selected, &transformed, &mut oracle, || {
        Instant::now() < deadline
    })
    .map_err(|_| "cpu-oracle-rejected")?;
    let plan = GuardedQ2Plan::new(COLUMNS, request.rows, request.tile_rows, 1_470_160)
        .map_err(|_| "guarded-plan-rejected")?;
    let ledger = plan.ledger();
    // Count and scratch are bounded before any driver is opened. This second
    // CPU path independently decodes every transmitted tile and validates every
    // integer limb, before final original-scale/FMA projection comparison.
    let mut expected_dots = vec![0.0; 2 * request.tile_rows];
    let mut output = Vec::new();
    let mut checked_operations = 0_usize;
    let mut numeric_failure = None;
    let mut native = None;
    let mut driver_path = None;
    let mut native_error = None;
    let mut work = |mut submitter: Option<&mut dyn GuardedQ2Submitter>| -> Result<()> {
        guarded_q2_projection(
            plan,
            selected,
            &transformed,
            &mut output,
            |operation: &GuardedQ2Operation, dots| {
                if let Some(device) = submitter.as_deref_mut() {
                    device
                        .submit(
                            operation.repacked_weights(),
                            operation.activation_rows(),
                            dots,
                        )
                        .map_err(|_| packed_projection::Error::Protocol)?;
                } else {
                    operation.cpu_control_dots(dots)?;
                }
                let expected = &mut expected_dots[..dots.len()];
                operation.cpu_control_dots(expected)?;
                exact_values(expected, dots).map_err(|_| packed_projection::Error::Protocol)?;
                checked_operations += 1;
                Ok(())
            },
            || Instant::now() < deadline,
        )
        .map_err(|_| "guarded-projection-rejected")?;
        exact_values(&oracle, &output)?;
        if checked_operations != ledger.operation_count {
            return Err("operation-count-rejected");
        }
        check_time(deadline)
    };
    let passed = match request.execution {
        Execution::CpuControl => match work(None) {
            Ok(()) => true,
            Err(error) => {
                numeric_failure = Some(error);
                false
            }
        },
        Execution::DeviceGuardedQ2 => {
            check_time(deadline)?;
            if deadline.saturating_duration_since(Instant::now()) < Duration::from_secs(4) {
                return Err("native-cleanup-reserve-unavailable");
            }
            let package = request
                .package_directory
                .as_deref()
                .ok_or("signed-package-required")?;
            let driver = Driver::open_system().map_err(|_| "native-driver-open-rejected")?;
            driver_path = Some(driver.path().to_owned());
            let limits = Q2ProjectionLimits::new(
                request.tile_rows,
                u32::try_from(ledger.operation_count).map_err(|_| "operation-count-rejected")?,
                deadline.saturating_duration_since(Instant::now()),
            )
            .map_err(|_| "native-limits-rejected")?;
            let progress = |snapshot: &Q2ProjectionReport| {
                if !emit_progress(snapshot.queue.stage, snapshot.operations_completed) {
                    return;
                }
                let mut stderr = std::io::stderr().lock();
                let _ = writeln!(stderr, "hexagon-projection:stage={}", snapshot.queue.stage);
                let _ = stderr.flush();
            };
            match driver.reference_q2_projection(package, limits, progress, |submitter| {
                work(Some(submitter)).map_err(|error| {
                    numeric_failure = Some(error);
                    hexagon_native_rpc::Error::Contract
                })
            }) {
                Ok(report) => {
                    let passed = report.completed_cycle();
                    native = Some(native_json(&report));
                    passed
                }
                Err(error) => {
                    native_error = Some(error.to_string());
                    false
                }
            }
        }
    };
    // The result is published only after exact full-result parity and, for a
    // device attempt, successful explicit cleanup. An error leaves no result.
    external(&pending_output)?;
    let mut result_identity = Value::Null;
    let _result_hold = if passed {
        check_time(deadline)?;
        let bytes: Vec<u8> = output
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        result_identity = json!({"file":"result.f32le","bytes":bytes.len(),"sha256":digest(&bytes),"encoding":"little-endian-f32"});
        Some(write_new(&pending_output.join("result.f32le"), &bytes)?)
    } else {
        output.clear();
        None
    };
    if passed {
        check_time(deadline)?;
    }
    let receipt = json!({
        "schemaVersion": 1,
        "kind": "swift-original-pq2-guarded-q2-projection",
        "outcome": if passed { "passed" } else { "pending" },
        "passed": passed,
        "execution": if request.execution == Execution::CpuControl { "cpu-control" } else { "device-guarded-q2" },
        "probeSourceSha256": digest(include_bytes!("projection_probe.rs")),
        "artifactSha256": ARTIFACT_SHA256,
        "bindingManifestSha256": digest(&manifest),
        "tensor": "blk.0.ffn_up.weight",
        "columns": COLUMNS,
        "completeTensorRows": ROWS,
        "selectedPrefixRows": request.rows,
        "tileRows": request.tile_rows,
        "weightBytesVerified": WEIGHT_BYTES,
        "weightSha256": WEIGHT_SHA256,
        "signsSha256": SIGNS_SHA256,
        "activation": {"bytes": activation_bytes.len(), "sha256": request.activation_sha256,
            "basis": "pre-Hadamard-input-last-dimension", "provenance": "externally-supplied-unqualified",
            "sourceModelActivationQualified": false},
        "transformOwner": "CPU",
        "quantization": "original-pinned-Q8_K-on-CPU",
        "reduction": "original-ordered-per-128-PQ2-block-FMA-on-CPU",
        "expectedOperations": ledger.operation_count,
        "exactLimbOperationsChecked": checked_operations,
        "ownedGuardedPayloadBoundBytes": ledger.owned_payload_bytes,
        "repackedHostWeightTransferBoundBytes": ledger.repacked_weight_transfer_bytes,
        "activationTransferBoundBytes": ledger.activation_transfer_bytes,
        "limbReturnBoundBytes": ledger.limb_return_bytes,
        "boundsAreObservedTransfers": false,
        "driverDll": driver_path,
        "native": native,
        "nativeError": native_error,
        "numericalFailure": numeric_failure,
        "result": result_identity,
        "maximumSeconds": request.maximum_seconds,
        "elapsedMillisecondsBeforePublication": started.elapsed().as_millis(),
        "trustedSupervisorRequiredForDeviceExecution": true,
        "sameArtifactPayloadBound": true,
        "everyLimbBitwiseParity": passed,
        "completeSelectedProjectionBitwiseParity": passed,
        "modelInferenceExecuted": false,
        "activationParityQualified": false,
        "logitStateParityQualified": false,
        "adapterParityQualified": false,
        "performanceQualified": false,
        "profileEligible": false,
        "trustChanged": false,
    });
    let mut encoded = serde_json::to_vec_pretty(&receipt).map_err(|_| "receipt-encoding-failed")?;
    encoded.push(b'\n');
    let pending = pending_output.join("receipt.pending");
    drop(write_new(&pending, &encoded)?);
    fs::rename(pending, pending_output.join("receipt.json"))
        .map_err(|_| "receipt-publication-failed")?;
    drop(_result_hold);
    if passed {
        check_time(deadline)?;
    }
    fs::rename(&pending_output, &request.output_directory)
        .map_err(|_| "result-directory-publication-failed")?;
    Ok(receipt)
}

fn run() -> Result<Value> {
    let started = Instant::now();
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() == 1 && args[0] == "--inspect" {
        let location =
            DriverLocation::inspect_system().map_err(|_| "native-driver-inspection-rejected")?;
        return Ok(
            json!({"schemaVersion":1,"kind":"native-fastrpc-inspection","driverDll":location.path(),
            "driverLoaded":false,"dspCodeLoaded":false,"profileEligible":false}),
        );
    }
    if args.len() == 1 && args[0] == "--self-test" {
        self_check()?;
        return Ok(
            json!({"schemaVersion":1,"kind":"swift-projection-probe-self-check","passed":true,
            "driverLoaded":false,"deviceExecuted":false,"modelInferenceExecuted":false,"profileEligible":false}),
        );
    }
    if args.len() != 2 || args[0] != "--request" {
        return Err("use-self-test-inspect-or-request-absolute-json");
    }
    let (_request_hold, bytes) = read_held(
        Path::new(&args[1]),
        MAX_REQUEST_BYTES,
        false,
        started + Duration::from_secs(225),
    )?;
    let request: Request = serde_json::from_slice(&bytes).map_err(|_| "request-json-rejected")?;
    projection(request, started)
}

fn self_check() -> Result<()> {
    if digest(b"abc") != "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        || !lower_digest(WEIGHT_SHA256)
        || lower_digest(&"A".repeat(64))
        || f32_values(&f32::INFINITY.to_le_bytes(), 1).is_ok()
        || f32_values(&[0; 3], 1).is_ok()
        || exact_values(&[0.0], &[-0.0]).is_ok()
        || binding(b"{}").is_ok()
    {
        return Err("projection-self-check-failed");
    }
    Ok(())
}

fn main() -> ExitCode {
    let receipt = match run() {
        Ok(receipt) => receipt,
        Err(stage) => json!({"schemaVersion":1,"kind":"swift-original-pq2-guarded-q2-projection",
            "outcome":"pending","passed":false,"stage":stage,"profileEligible":false}),
    };
    let passed = receipt["passed"].as_bool().unwrap_or(true);
    match serde_json::to_writer(std::io::stdout().lock(), &receipt) {
        Ok(()) if passed => ExitCode::SUCCESS,
        _ => ExitCode::FAILURE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_identity_nonfinite_and_exact_comparison_controls() -> Result<()> {
        self_check()
    }

    #[test]
    fn duplicate_unknown_and_narrowed_request_fields_reject() {
        let request = r#"{"bindingDirectory":"C:\\binding","activationFile":"C:\\activation.bin","activationSha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","outputDirectory":"C:\\result","rows":32,"tileRows":32,"maximumSeconds":15,"execution":"cpu-control"}"#;
        assert!(serde_json::from_str::<Request>(request).is_ok());
        for replacement in [
            "\"rows\":32,\"rows\":64",
            "\"rows\":32,\"unreviewed\":true",
            "\"rows\":-1",
        ] {
            let invalid = request.replace("\"rows\":32", replacement);
            assert!(serde_json::from_str::<Request>(&invalid).is_err());
        }
    }

    #[test]
    fn device_shapes_duration_and_package_modes_fail_closed()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let mut request: Request = serde_json::from_value(json!({
            "bindingDirectory":"C:\\binding", "activationFile":"C:\\activation.bin",
            "activationSha256": "a".repeat(64), "outputDirectory":"C:\\result",
            "rows":17408,"tileRows":17408,"maximumSeconds":120,
            "execution":"device-guarded-q2", "packageDirectory":"C:\\package"
        }))?;
        assert!(request.validate_shape().is_ok());
        for rows in [0, 1, 31, 33, 17409, usize::MAX] {
            request.rows = rows;
            assert!(request.validate_shape().is_err());
        }
        request.rows = ROWS;
        for tile_rows in [0, 1, 31, 33, ROWS + 32, usize::MAX] {
            request.tile_rows = tile_rows;
            assert!(request.validate_shape().is_err());
        }
        request.tile_rows = 32;
        request.maximum_seconds = 121;
        assert!(request.validate_shape().is_err());
        request.maximum_seconds = 120;
        request.package_directory = None;
        assert!(request.validate_shape().is_err());
        request.execution = Execution::CpuControl;
        assert!(request.validate_shape().is_ok());
        request.package_directory = Some("C:\\package".into());
        assert!(request.validate_shape().is_err());
        Ok(())
    }
    #[test]
    fn progress_remains_bounded_for_small_row_full_projection_schedule() {
        let mut bytes = 0;
        for completed in 0..21760 {
            for stage in [
                "projection-queue-write-entered",
                "projection-queue-read-entered",
                "projection-operation-complete",
            ] {
                if emit_progress(stage, completed) {
                    bytes += "hexagon-projection:stage=".len() + stage.len() + 1;
                }
            }
        }
        assert!(bytes < 8192);
        for stage in [
            "projection-response-refused-pinned",
            "projection-work-expired-pinned",
            "compute-free-entered",
        ] {
            assert!(emit_progress(stage, 21759));
        }
    }
    #[test]
    fn activation_bits_preserve_signed_zero_and_reject_nonfinite() -> Result<()> {
        let bytes: Vec<u8> = [0.0_f32, -0.0, 1.25, -127.0]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect();
        let decoded = f32_values(&bytes, 4)?;
        exact_values(&[0.0, -0.0, 1.25, -127.0], &decoded)?;
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(f32_values(&bad.to_le_bytes(), 1).is_err());
        }
        Ok(())
    }
}
