//! Native ownership and evidence lifecycle for the fixed numerical workloads.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bonsai_artifacts::{ArtifactError, VerifiedFile, validate_external_path};
use bonsai_windows_process::ParentProcess;
use serde_json::{Value, json};

use crate::identity::Hasher;

use super::{
    AdapterCondition, CHANNEL, CaptureMode, Kind, LIMIT, PENDING, Request, plan, read_json,
    remaining_seconds, request_timing, require_runtime_source, sample_arguments, save_report,
};

const ACTIVE_MODELS: &[&str] = &[
    "bonsai-chat.exe",
    "bonsai-broker.exe",
    "prism-inference-server.exe",
    "prism-inference.exe",
    "bonsai-swift-worker.exe",
    "bonsai-token-worker.exe",
    "llama-server.exe",
    "llama-completion.exe",
    "llama-bench.exe",
    "swift_model_check.exe",
    "swift-ptq1-reference.exe",
    "bonsai-speech-asr.exe",
    "bonsai-speech-tts.exe",
];

struct Context<'a> {
    request: &'a Request,
    pins: &'a Value,
    started: Instant,
    cap: Duration,
    deadline: u64,
    parent: ParentProcess,
    parent_id: u32,
    parent_creation: u64,
    data: PathBuf,
    bundle: PathBuf,
    evidence: PathBuf,
    report_path: PathBuf,
    report: Value,
    artifacts: Vec<Value>,
    guards: Vec<VerifiedFile>,
}

fn external(path: &Path) -> crate::Result<()> {
    validate_external_path(path).map_err(|error| error.to_string())
}

fn ci_guard(kind: Kind) -> crate::Result<()> {
    if !cfg!(target_arch = "aarch64") {
        return Err("Numerical qualification requires native ARM64 Windows.".into());
    }
    for (name, expected) in [
        ("GITHUB_ACTIONS", "true"),
        ("GITHUB_REF", "refs/heads/main"),
        ("GITHUB_REPOSITORY", "symbiogenesis/SwiftBonsai2Runner"),
        ("RUNNER_ARCH", "ARM64"),
    ] {
        if std::env::var(name).as_deref() != Ok(expected) {
            return Err("Numerical qualification requires this repository main native Windows ARM64 owner CI.".into());
        }
    }
    if kind == Kind::Rows && std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("self-hosted") {
        return Err("Row qualification requires the self-hosted owner lane.".into());
    }
    let revision = std::env::var("GITHUB_SHA").unwrap_or_default();
    let run = std::env::var("GITHUB_RUN_ID").unwrap_or_default();
    if !crate::identity::is_lower_hex(&revision, 40)
        || run.is_empty()
        || !run.bytes().all(|byte| byte.is_ascii_digit())
        || std::env::var("RUNNER_NAME")
            .unwrap_or_default()
            .trim()
            .is_empty()
    {
        return Err(
            "Numerical qualification requires an identified source-bound owner CI run.".into(),
        );
    }
    for name in [
        "ACTIONS_RUNTIME_TOKEN",
        "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
        "GITHUB_TOKEN",
        "GH_TOKEN",
        "ACTIONS_RUNNER_INPUT_TOKEN",
    ] {
        if std::env::var_os(name).is_some() {
            return Err(
                "Numerical qualification requires CI credentials omitted from the process tree."
                    .into(),
            );
        }
    }
    bonsai_cpu_policy::server::validate_environment(
        std::env::vars_os().map(|(name, _)| name.to_string_lossy().into_owned()),
    )
}

fn publish_evidence(path: &Path, request: &Request) -> crate::Result<()> {
    let destination =
        PathBuf::from(std::env::var_os("GITHUB_ENV").ok_or("Expected the CI environment file.")?);
    external(&destination)?;
    let value = path
        .to_str()
        .filter(|value| !value.contains(['\r', '\n']))
        .ok_or("Evidence path must be single-line Unicode.")?;
    let name = if request.capture_mode == CaptureMode::RequestTiming {
        "BONSAI_PTQ1_TIMING_EVIDENCE"
    } else if request.kind == Kind::Ptq1 {
        "BONSAI_PTQ1_NUMERICS_EVIDENCE"
    } else {
        "BONSAI_ROW_QUALIFICATION_EVIDENCE"
    };
    let mut file = OpenOptions::new()
        .append(true)
        .open(destination)
        .map_err(|error| error.to_string())?;
    writeln!(file, "{name}={value}").map_err(|error| error.to_string())
}

fn capture_sources(request: &Request, pins: &Value) -> crate::Result<()> {
    let workspace = PathBuf::from(
        std::env::var_os("GITHUB_WORKSPACE").ok_or("Expected the owning CI checkout.")?,
    );
    if !workspace.is_absolute() || !request.repository_directory.is_absolute() {
        return Err("Capture requires the absolute owning CI checkout.".into());
    }
    let repository =
        fs::canonicalize(&request.repository_directory).map_err(|error| error.to_string())?;
    let workspace = fs::canonicalize(workspace).map_err(|error| error.to_string())?;
    if repository != workspace {
        return Err(
            "Capture sources and comparators must come from the owning CI checkout.".into(),
        );
    }
    if request.kind == Kind::Ptq1 {
        let runtime: Value =
            serde_json::from_str(include_str!("../../../../config/prism-inference.lock.json"))
                .map_err(|error| error.to_string())?;
        require_runtime_source(&pins["rustSource"], &runtime)?;
    }
    Ok(())
}

impl Context<'_> {
    fn remaining(&self) -> crate::Result<u64> {
        self.parent
            .ensure_alive()
            .map_err(|error| error.to_string())?;
        let (now, frequency) =
            bonsai_windows_process::monotonic_ticks().map_err(|error| error.to_string())?;
        remaining_seconds(
            self.cap,
            self.started.elapsed(),
            self.deadline,
            now,
            frequency,
        )
    }

    fn checkpoint(&self) -> crate::Result<()> {
        self.remaining().map(|_| ())
    }

    fn phase(&mut self, phase: &str) -> crate::Result<()> {
        self.checkpoint()?;
        self.report["Phase"] = json!(phase);
        save_report(&self.report_path, &self.report)
    }

    fn model_sample(
        &mut self,
        executable: &Path,
        arguments: &Value,
        lane: &Path,
        mut sample: Value,
    ) -> crate::Result<()> {
        sample["powerBefore"] = crate::host_cpu::power_snapshot();
        sample["powerAfter"] = Value::Null;
        sample["Supervision"] = Value::Null;
        let samples = self.report["Samples"]
            .as_array_mut()
            .ok_or("Missing sample receipts.")?;
        let index = samples.len();
        samples.push(sample);
        // Persist the before observation even when process intake or capture fails.
        save_report(&self.report_path, &self.report)?;
        let result = self.completion(executable, arguments, lane, true);
        let sample = &mut self.report["Samples"][index];
        sample["powerAfter"] = crate::host_cpu::power_snapshot();
        sample["powerConditions"] = bonsai_cpu_policy::power::compare_endpoints(
            &sample["powerBefore"],
            &sample["powerAfter"],
        );
        if let Ok(supervision) = &result {
            sample["Supervision"] = supervision.clone();
        }
        self.report["powerSampleConditions"] = bonsai_cpu_policy::power::compare_samples(
            self.report["Samples"]
                .as_array()
                .ok_or("Missing sample receipts.")?,
        );
        save_report(&self.report_path, &self.report)?;
        result.map(|_| ())
    }

    fn hash_file(&self, path: &Path, maximum: u64) -> crate::Result<Value> {
        let mut file =
            bonsai_artifacts::open_locked_file(path).map_err(|error| error.to_string())?;
        let length = file.metadata().map_err(|error| error.to_string())?.len();
        if length == 0 || length > maximum {
            return Err("Qualification artifact exceeds its fixed byte bound.".into());
        }
        let mut hash = Hasher::new()?;
        let mut buffer = vec![0; 65_536];
        let mut total = 0_u64;
        loop {
            self.checkpoint()?;
            let count = file.read(&mut buffer).map_err(|error| error.to_string())?;
            if count == 0 {
                break;
            }
            total += u64::try_from(count).map_err(|error| error.to_string())?;
            if total > length {
                return Err("Qualification artifact grew during hashing.".into());
            }
            hash.update(&buffer[..count])?;
        }
        if total != length {
            return Err("Qualification artifact changed during hashing.".into());
        }
        Ok(json!({"path": path, "bytes": length, "sha256": hash.finalize_hex()?}))
    }

    fn hold(&mut self, artifact: Value) -> crate::Result<()> {
        let path = PathBuf::from(artifact["path"].as_str().ok_or("Missing artifact path.")?);
        let bytes = artifact["bytes"]
            .as_u64()
            .ok_or("Missing artifact length.")?;
        let digest = artifact["sha256"]
            .as_str()
            .ok_or("Missing artifact digest.")?;
        let guard = VerifiedFile::verify_with(&path, bytes, digest, || {
            self.checkpoint().map_err(|_| ArtifactError::Cancelled)
        })
        .map_err(|error| error.to_string())?;
        self.guards.push(guard);
        self.artifacts.push(artifact);
        Ok(())
    }

    fn completion(
        &self,
        executable: &Path,
        arguments: &Value,
        lane: &Path,
        model: bool,
    ) -> crate::Result<Value> {
        let remaining = self.remaining()?;
        // Reserve bounded teardown and verification time inside the same original
        // deadline. If the complete selected operation no longer fits it stays pending.
        if remaining <= 32 {
            return Err(
                "Insufficient complete qualification allowance for verification and owned cleanup."
                    .into(),
            );
        }
        crate::completion::run(
            json!({"schemaVersion": 1, "executable": executable, "arguments": arguments,
            "workingDirectory": lane, "evidenceDirectory": lane, "artifacts": self.artifacts,
            "memoryLimitBytes": if model { 12_348_030_976_u64 } else { 536_870_912 },
            "requiredAvailableBytes": 16_642_998_272_u64, "physicalReserveBytes": 4_294_967_296_u64,
            "timeoutSeconds": if model { remaining - 32 } else { 10_u64.min(remaining - 32) },
            "totalTimeoutSeconds": remaining, "stdoutLimitBytes": 65_536,
            "stderrLimitBytes": if model { 67_108_864 } else { 65_536 },
            "requireNonemptyStdout": self.request.capture_mode != CaptureMode::RequestTiming,
            "ciOnly": false, "parentProcessId": self.parent_id, "parentCreationTimeFileTime": self.parent_creation }),
        )
    }

    fn lane(&self, name: &str) -> crate::Result<PathBuf> {
        let lane = self.evidence.join(name);
        external(&lane)?;
        fs::create_dir(&lane).map_err(|error| error.to_string())?;
        Ok(lane)
    }

    fn compare(&mut self) -> crate::Result<()> {
        self.report["Status"] = json!("comparing");
        self.report[if self.request.kind == Kind::Ptq1 {
            "ActiveBackend"
        } else {
            "ActiveCondition"
        }] = Value::Null;
        self.phase("logits-and-complete-state-comparison")?;
        let output = self.evidence.join("comparison.json");
        let repo = &self.request.repository_directory;
        let arguments: Vec<std::ffi::OsString> = if self.request.kind == Kind::Ptq1 {
            vec![
                "-NoLogo".into(),
                "-NoProfile".into(),
                "-File".into(),
                repo.join("tools/native-reference/compare.ps1")
                    .into_os_string(),
                "-Native".into(),
                self.evidence.join("native").into_os_string(),
                "-Rust".into(),
                self.evidence
                    .join(&self.request.rust_backend)
                    .into_os_string(),
                "-Backend".into(),
                self.request.rust_backend.clone().into(),
                "-RowWorkers".into(),
                self.request.row_workers.to_string().into(),
                "-AdapterCondition".into(),
                self.request.adapter_condition.name().into(),
                "-Output".into(),
                output.as_os_str().to_owned(),
            ]
        } else {
            vec![
                "-NoLogo".into(),
                "-NoProfile".into(),
                "-File".into(),
                repo.join("tools/row-parallel/compare.ps1").into_os_string(),
                "-Serial".into(),
                self.evidence.join("serial").into_os_string(),
                "-Parallel".into(),
                self.evidence.join("parallel").into_os_string(),
                "-Output".into(),
                output.as_os_str().to_owned(),
            ]
        };
        let remaining = self
            .remaining()?
            .checked_sub(5)
            .filter(|value| *value > 0)
            .ok_or("No comparison/cleanup allowance remains.")?;
        let numeric_tool = std::env::current_exe()
            .map_err(|error| error.to_string())?
            .with_file_name("bonsai-reference-numerics.exe");
        let environment = BTreeMap::from([(
            "BONSAI_REFERENCE_NUMERIC_TOOL".to_owned(),
            numeric_tool
                .to_str()
                .ok_or("Numeric helper path must be Unicode.")?
                .to_owned(),
        )]);
        let result = crate::startup_process::run_with_checkpoint(
            std::ffi::OsStr::new("pwsh.exe"),
            &arguments,
            &self.evidence,
            &environment,
            Duration::from_secs(remaining),
            &self.evidence.join("comparison.txt"),
            || self.checkpoint(),
        )?;
        if result.code != 0 {
            return Err("Fixed-input numerical comparison failed.".into());
        }
        let comparison = read_json(&output, LIMIT)?;
        let expected = if self.request.kind == Kind::Ptq1 {
            "passed-bounded-one-token-smoke"
        } else {
            "fixed-step-serial-parallel-byte-exact"
        };
        if comparison["status"] != expected
            || comparison["defaultMayChange"].as_bool() != Some(false)
        {
            return Err("Numerical comparison did not report the fixed successful scope.".into());
        }
        self.report["Comparison"] = comparison;
        self.checkpoint()
    }
}

fn check_row_receipt(receipt: &Value, pins: &Value, revision: &str) -> crate::Result<()> {
    if receipt["SchemaVersion"] != 1
        || receipt["RunnerRevision"] != revision
        || receipt["RustToolchain"] != CHANNEL
        || receipt["BuildFeatures"] != json!(["parallel-rows"])
        || receipt["Files"].as_array().map(Vec::len) != Some(1)
    {
        return Err(
            "Hosted row checker differs from reviewed source/revision/feature pins.".into(),
        );
    }
    for field in ["commit", "url", "bytes", "sha256"] {
        if receipt["RustSource"][field] != pins["rustSource"][field] {
            return Err("Hosted row source pins changed.".into());
        }
    }
    Ok(())
}

fn intake(context: &mut Context<'_>) -> crate::Result<Value> {
    context.phase("capture-tool-intake")?;
    let names: &[&str] = if context.request.kind == Kind::Ptq1 {
        &["swift-ptq1-reference.exe", "swift_model_check.exe"]
    } else {
        &["swift_model_check.exe"]
    };
    let entries = fs::read_dir(&context.bundle)
        .map_err(|error| error.to_string())?
        .take(names.len() + 2)
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(|error| error.to_string())?;
    if entries.len() != names.len() + 1
        || entries.iter().any(|entry| {
            !fs::symlink_metadata(entry.path()).is_ok_and(|metadata| {
                metadata.is_file() && !bonsai_artifacts::is_redirect(&metadata)
            }) || !entry
                .file_name()
                .to_str()
                .is_some_and(|name| name == "capture-tools.json" || names.contains(&name))
        })
    {
        return Err("Capture bundle contains unselected, missing or redirected entries.".into());
    }
    let receipt_path = context.bundle.join("capture-tools.json");
    let receipt_pin = context.hash_file(&receipt_path, 65_536)?;
    context.hold(receipt_pin)?;
    let revision = std::env::var("GITHUB_SHA").map_err(|error| error.to_string())?;
    let receipt = if context.request.kind == Kind::Ptq1 {
        crate::ptq1_capture::run(
            json!({"operation": "receipt", "bundleDirectory": context.bundle,
            "repositoryDirectory": context.request.repository_directory, "revision": revision,
            "pins": context.pins, "rowWorkers": context.request.row_workers}),
        )?
    } else {
        let receipt = read_json(&receipt_path, 65_536)?;
        check_row_receipt(&receipt, context.pins, &revision)?;
        receipt
    };
    for name in names {
        let files = receipt["Files"]
            .as_array()
            .ok_or("Missing capture inventory.")?;
        let found: Vec<_> = files
            .iter()
            .filter(|entry| entry["Name"] == *name)
            .collect();
        if found.len() != 1 {
            return Err("Capture inventory is missing or duplicates a required tool.".into());
        }
        let entry = found[0];
        if !entry["Bytes"]
            .as_u64()
            .is_some_and(|bytes| (1..=134_217_728).contains(&bytes))
            || !entry["Sha256"]
                .as_str()
                .is_some_and(|hash| crate::identity::is_lower_hex(hash, 64))
        {
            return Err("Invalid capture-tool inventory identity.".into());
        }
        let path = context.bundle.join(name);
        context.hold(json!({"path": path, "bytes": entry["Bytes"], "sha256": entry["Sha256"]}))?;
        crate::startup_cache::assert_arm64(&path)?;
    }
    fs::copy(&receipt_path, context.evidence.join("capture-tools.json"))
        .map_err(|error| error.to_string())?;
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let maintenance_path = executable
        .parent()
        .ok_or("Missing maintenance directory.")?
        .join("receipt.json");
    let maintenance_pin = context.hash_file(&maintenance_path, 65_536)?;
    context.hold(maintenance_pin)?;
    let maintenance = read_json(&maintenance_path, 65_536)?;
    let executable_pin = context.hash_file(&executable, 134_217_728)?;
    if maintenance["SchemaVersion"] != 3
        || maintenance["Revision"] != revision
        || maintenance["Sha256"] != executable_pin["sha256"]
    {
        return Err("Maintenance executable differs from the checked Runner revision.".into());
    }
    context.hold(executable_pin)?;
    let numeric_path = executable.with_file_name("bonsai-reference-numerics.exe");
    let numeric_pin = context.hash_file(&numeric_path, 134_217_728)?;
    if maintenance["NumericSha256"] != numeric_pin["sha256"] {
        return Err("Numeric helper differs from the checked Runner receipt.".into());
    }
    context.hold(numeric_pin)?;
    crate::startup_cache::assert_arm64(&numeric_path)?;
    fs::copy(
        maintenance_path,
        context.evidence.join("maintenance-tools.json"),
    )
    .map_err(|error| error.to_string())?;
    Ok(receipt)
}

fn admission(context: &mut Context<'_>) -> crate::Result<Value> {
    context.phase("model-and-adapter-admission")?;
    let checked = crate::completion::cpu_check_report(
        json!({"operation": "plan", "dataDirectory": context.data,
        "profile": "swift-pq2-assessment", "useAbliterateLora": context.request.adapter_condition != AdapterCondition::NoAdapter,
        "useRepackedPtq1": context.request.kind == Kind::Ptq1}),
    )?;
    let report = &checked["Report"];
    if report["Status"] != "admission-passed"
        || report["ServiceCeilingBytes"] != 12_348_030_976_u64
        || report["RequiredAvailableBytes"] != 16_642_998_272_u64
        || report["AvailableMemoryFloorBytes"] != 4_294_967_296_u64
    {
        return Err("The unchanged Swift artifact/resource admission did not pass.".into());
    }
    context.artifacts.push(json!({"path": report["ModelPath"], "bytes": checked["Artifact"]["Bytes"], "sha256": checked["Artifact"]["Sha256"]}));
    if context.request.adapter_condition != AdapterCondition::NoAdapter {
        let adapter = &checked["AdapterLock"]["adapter"];
        context.artifacts.push(json!({"path": report["AdapterPath"], "bytes": adapter["bytes"], "sha256": adapter["sha256"]}));
    }
    context.report["Admission"] = report.clone();
    context.report["AdmissionScope"] = json!(
        "Resource planning only; publisher arguments are not executed. The one selected adapter condition and Rust controls are recorded separately."
    );
    Ok(checked)
}

fn runtime(context: &mut Context<'_>, receipt: &Value) -> crate::Result<Value> {
    context.phase("runtime-verification")?;
    let lock: Value = serde_json::from_str(include_str!(
        "../../../../config/swift-assessment.lock.json"
    ))
    .map_err(|error| error.to_string())?;
    if lock["runtime"]["commit"] != context.pins["nativeSource"]["commit"] {
        return Err("Native reference runtime differs from the reviewed source.".into());
    }
    let remaining = context
        .remaining()?
        .checked_sub(10)
        .filter(|seconds| *seconds != 0)
        .ok_or("No runtime verification/cleanup allowance remains.")?;
    let runtime = crate::runtime::run(
        json!({"schemaVersion": 1, "commit": lock["runtime"]["commit"],
        "runtime": crate::cpu_check::runtime_spec(&context.data, &lock)?, "timeoutSeconds": remaining,
        "parentProcessId": context.parent_id, "parentCreationTimeFileTime": context.parent_creation}),
    )?;
    let files = runtime["RuntimeFiles"]
        .as_array()
        .filter(|files| files.len() == 63)
        .ok_or("Expected the complete pinned native package inventory.")?;
    for file in files {
        context
            .artifacts
            .push(json!({"path": file["Path"], "bytes": file["Bytes"], "sha256": file["Sha256"]}));
    }
    context.artifacts.push(json!({"path": runtime["ArchivePath"], "bytes": lock["runtime"]["asset"]["bytes"], "sha256": lock["runtime"]["asset"]["sha256"]}));
    context.report["RuntimeFiles"] = runtime["RuntimeFiles"].clone();
    let collector = context.bundle.join("swift-ptq1-reference.exe");
    for (phase, flag, field) in [
        ("abi-preflight", "--abi-probe", "AbiPreflight"),
        ("abi-observations", "--abi-observations", "AbiObservations"),
    ] {
        context.phase(phase)?;
        let lane = context.lane(phase)?;
        context.report[field] = context.completion(
            &collector,
            &json!([flag, runtime["Runtime"]["runtimeDirectory"]]),
            &lane,
            false,
        )?;
        if field == "AbiPreflight" {
            let probe = read_json(&lane.join("stdout.txt"), 65_536)?;
            if probe["status"] != "native-default-parameter-abi-probe-passed"
                || probe["modelLoaded"].as_bool() != Some(false)
                || probe["modelParamsBytes"] != 80
                || probe["contextParamsBytes"] != 168
                || probe["batchBytes"] != 56
            {
                return Err("Exact native public-header ABI preflight failed.".into());
            }
        } else {
            crate::ptq1_capture::run(
                json!({"operation": "compare-abi", "actualPath": lane.join("stdout.txt"),
                "expectedPath": context.request.repository_directory.join("tools/native-reference/fixtures/collector-abi-v1.txt"),
                "expectedSha256": receipt["AbiControlSha256"]}),
            )?;
        }
        save_report(&context.report_path, &context.report)?;
    }
    Ok(runtime)
}

fn model_samples(
    context: &mut Context<'_>,
    checked: &Value,
    runtime: Option<&Value>,
) -> crate::Result<()> {
    let admission = &checked["Report"];
    let conditions = if context.request.kind == Kind::Ptq1 {
        [
            ("native".to_owned(), 0),
            (
                context.request.rust_backend.clone(),
                context.request.row_workers,
            ),
        ]
    } else {
        [("serial".into(), 1), ("parallel".into(), 4)]
    };
    for (name, workers) in conditions {
        let lane = context.lane(&name)?;
        let native = workers == 0;
        let executable = context.bundle.join(if native {
            "swift-ptq1-reference.exe"
        } else {
            "swift_model_check.exe"
        });
        let arguments = sample_arguments(context.request, admission, runtime, &lane, workers)?;
        save_report(&lane.join("arguments.json"), &arguments)?;
        save_report(&lane.join("artifacts.json"), &json!(context.artifacts))?;
        if context.request.kind == Kind::Rows {
            save_report(
                &lane.join("capture.json"),
                &json!({"SchemaVersion": 1, "RunnerRevision": context.report["Revision"],
                "RustSource": context.pins["rustSource"], "Condition": name, "RowWorkers": workers, "Representation": "PQ2_0",
                "ModelSha256": checked["Artifact"]["Sha256"], "AdapterSha256": checked["AdapterLock"]["adapter"]["sha256"],
                "AdapterScale": 1.3, "SuppliedTokens": [248_045], "Backend": "auto", "AppTpsQualified": false}),
            )?;
        }
        context.report["ModelTrialsRequested"] = json!(true);
        context.report["Status"] = json!("capturing");
        context.report[if context.request.kind == Kind::Ptq1 {
            "ActiveBackend"
        } else {
            "ActiveCondition"
        }] = json!(name);
        context.phase("model-capture")?;
        let sample = if context.request.kind == Kind::Ptq1 {
            json!({"Backend": name, "RustRowWorkers": if native { Value::Null } else { json!(workers) }})
        } else {
            json!({"Condition": name, "RowWorkers": workers})
        };
        context.model_sample(&executable, &arguments, &lane, sample)?;
    }
    Ok(())
}

fn execute(context: &mut Context<'_>) -> crate::Result<()> {
    if bonsai_windows_process::available_disk_bytes(&context.evidence)
        .map_err(|error| error.to_string())?
        < 1_073_741_824
    {
        return Err("Numerical qualification requires 1 GiB external disk headroom.".into());
    }
    let receipt = intake(context)?;
    let host = bonsai_windows_process::native_build_host().map_err(|error| error.to_string())?;
    if bonsai_cpu_policy::hardware::SnapdragonFamily::from_processor_name(&host.processor_name)
        .is_none()
    {
        return Err("Expected the native Snapdragon X/X2 Elite processor.".into());
    }
    context.report["ProcessorName"] = json!(host.processor_name);
    if bonsai_windows_process::has_running_process(ACTIVE_MODELS)
        .map_err(|error| error.to_string())?
    {
        return Err("Close the existing Bonsai app and project model processes before qualification; no process was interrupted.".into());
    }
    // Source attribution executes one Rust checker only. Numerical mode retains
    // its exact native runtime, public-header ABI controls and comparison pair.
    if context.request.capture_mode == CaptureMode::RequestTiming {
        let checked = admission(context)?;
        return timing_sample(context, &checked, &receipt);
    }
    let runtime = if context.request.kind == Kind::Ptq1 {
        Some(runtime(context, &receipt)?)
    } else {
        None
    };
    let checked = admission(context)?;
    model_samples(context, &checked, runtime.as_ref())?;
    context.compare()
}

fn timing_sample(context: &mut Context<'_>, checked: &Value, tools: &Value) -> crate::Result<()> {
    context.phase("request-timing-preparation")?;
    let lane = context.lane("request-timing")?;
    let messages = context.evidence.join("public-messages.json");
    save_report(&messages, &request_timing::messages())?;
    let input = context.hash_file(&messages, 65_536)?;
    context.hold(input.clone())?;
    let tool_image = tools["Files"]
        .as_array()
        .and_then(|files| {
            files
                .iter()
                .find(|file| file["Name"] == "swift_model_check.exe")
        })
        .ok_or("Missing reviewed request timing checker identity.")?;
    let now = u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_millis(),
    )
    .map_err(|error| error.to_string())?;
    // Translate the original remaining monotonic allowance once. File intake,
    // verification, process startup and loading cannot renew this child deadline.
    // Native Job supervision still enforces the original monotonic owner bound.
    let deadline = request_timing::deadline(context.remaining()?, now)?;
    let expected = request_timing::expected(
        context.request,
        context.pins,
        checked,
        &tool_image["Sha256"],
        &input["sha256"],
        deadline,
    )?;
    let arguments = request_timing::arguments(
        context.request,
        checked,
        &lane,
        &messages,
        deadline,
        &context.pins["rustSource"],
    );
    save_report(&lane.join("arguments.json"), &arguments)?;
    save_report(&lane.join("artifacts.json"), &json!(context.artifacts))?;
    context.report["ModelTrialsRequested"] = json!(true);
    context.report["TimingReceiptStatus"] = json!("pending-unverified");
    context.report["ExpectedTimingIdentity"] = expected.clone();
    context.phase("source-session-request-timing")?;
    context.model_sample(
        &context.bundle.join("swift_model_check.exe"),
        &arguments,
        &lane,
        json!({"Backend":context.request.rust_backend,"RustRowWorkers":context.request.row_workers}),
    )?;
    context.phase("request-timing-receipt-verification")?;
    let mut artifacts = Vec::new();
    for name in ["request-timing-started.json", "request-timing.json"] {
        let artifact = context.hash_file(&lane.join(name), 65_536)?;
        context.hold(artifact.clone())?;
        artifacts.push(artifact);
    }
    let receipt = read_json(&lane.join("request-timing.json"), 65_536)?;
    request_timing::validate(&receipt, &expected)?;
    context.report["TimingArtifacts"] = json!(artifacts);
    context.report["TimingReceiptStatus"] = json!("verified-source-session-attribution");
    context.report["TimingScope"] = json!(
        "Fixed bounded public request; source-session operation brackets and committed-output counters only. No HTTP, decode TPS, isolated kernel timing, complete answer, service acceptance or release promotion."
    );
    save_report(&context.report_path, &context.report)
}

pub(super) fn capture(request: &Request, pins: &Value) -> crate::Result<Value> {
    let started = Instant::now();
    let seconds = request
        .work_budget_seconds
        .filter(|seconds| (1..=240).contains(seconds))
        .ok_or("Provide the remaining complete owner-job work allowance in 1..240 seconds.")?;
    let deadline = request
        .work_deadline_tick
        .filter(|ticks| *ticks != 0)
        .ok_or("Provide the original monotonic owner-job work deadline.")?;
    let (now, frequency) =
        bonsai_windows_process::monotonic_ticks().map_err(|error| error.to_string())?;
    remaining_seconds(
        Duration::from_secs(seconds),
        started.elapsed(),
        deadline,
        now,
        frequency,
    )?;
    ci_guard(request.kind)?;
    capture_sources(request, pins)?;
    let data = request
        .data_directory
        .clone()
        .ok_or("Provide the existing model data directory.")?;
    let bundle = request
        .capture_tools_directory
        .clone()
        .ok_or("Provide hosted-prepared capture tools; owner builds are disabled.")?;
    external(&data)?;
    external(&bundle)?;
    let (id, creation) = crate::cpu_check::parent_identity(
        request.parent_process_id,
        request.parent_creation_time_file_time,
    )?;
    let parent = ParentProcess::open(id, creation).map_err(|error| error.to_string())?;
    let evidence = std::env::temp_dir().join(format!(
        "Bonsai-NativeQualification-{}",
        crate::native_startup::nonce()?
    ));
    external(&evidence)?;
    fs::create_dir(&evidence).map_err(|error| error.to_string())?;
    publish_evidence(&evidence, request)?;
    let mut plan = plan(request, pins);
    plan["WorkBudgetSeconds"] = json!(seconds);
    let mut report = json!({"SchemaVersion": 1, "Status": "preparing", "Phase": "capture-tool-intake", "Plan": plan,
        "CaptureMode":request.capture_mode.name(),
        "Revision": std::env::var("GITHUB_SHA").unwrap_or_default(),
        "RunUrl": format!("https://github.com/symbiogenesis/SwiftBonsai2Runner/actions/runs/{}", std::env::var("GITHUB_RUN_ID").unwrap_or_default()),
        "RunAttempt": std::env::var("GITHUB_RUN_ATTEMPT").ok(), "ProcessorName": null, "WorkDeadlineTick": deadline,
        "ModelTrialsRequested": false, "WriterBasisQualified": false, "AppTpsQualified": false, "DefaultMayChange": false, "Samples": [] });
    report["powerBefore"] = crate::host_cpu::power_snapshot();
    report["powerAfter"] = Value::Null;
    if request.kind == Kind::Ptq1 {
        report["PendingControls"] = json!(PENDING);
    }
    if request.capture_mode == CaptureMode::RequestTiming {
        report["TimingReceiptStatus"] = json!("pending-unverified");
        report["IncompleteTimingReceiptMeans"] =
            json!("unknown work; no emitted-token inference or qualification");
    }
    let report_path = evidence.join(if request.capture_mode == CaptureMode::RequestTiming {
        "request-timing-owner.json"
    } else if request.kind == Kind::Ptq1 {
        "numerics.json"
    } else {
        "qualification.json"
    });
    save_report(&report_path, &report)?;
    let mut context = Context {
        request,
        pins,
        started,
        cap: Duration::from_secs(seconds),
        deadline,
        parent,
        parent_id: id,
        parent_creation: creation,
        data,
        bundle,
        evidence,
        report_path,
        report,
        artifacts: vec![],
        guards: vec![],
    };
    let result = execute(&mut context);
    finish(&mut context, result)
}

fn finish(context: &mut Context<'_>, result: crate::Result<()>) -> crate::Result<Value> {
    context.report["powerAfter"] = crate::host_cpu::power_snapshot();
    context.report["powerConditions"] = bonsai_cpu_policy::power::compare_endpoints(
        &context.report["powerBefore"],
        &context.report["powerAfter"],
    );
    context.report["WorkElapsedSeconds"] = json!(context.started.elapsed().as_secs_f64());
    match &result {
        Ok(()) => {
            context.report["Status"] = json!(if context.request.capture_mode
                == CaptureMode::RequestTiming
            {
                "completed-source-request-timing"
            } else if context.request.kind == Kind::Ptq1 {
                "completed-one-token-smoke"
            } else {
                "completed-one-token-row-control"
            });
            context.report["Phase"] = json!("completed");
        }
        Err(error) => {
            context.report["Status"] = json!("failed");
            context.report["Failure"] = json!(error.chars().take(4096).collect::<String>());
        }
    }
    save_report(&context.report_path, &context.report)?;
    result.map_err(|error| {
        format!(
            "{error} Evidence retained at {}",
            context.evidence.display()
        )
    })?;
    Ok(
        json!({"Status": context.report["Status"], "CaptureMode":context.request.capture_mode.name(), "EvidenceDirectory": context.evidence,
        "PendingControls": context.report["PendingControls"], "AppTpsQualified": false, "DefaultMayChange": false}),
    )
}
