//! Native admission and bounded CPU check orchestration.

#[cfg(windows)]
use serde::Deserialize;
use serde_json::Value;

#[cfg(windows)]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    data_directory: std::path::PathBuf,
    #[serde(default = "default_profile")]
    profile: String,
    #[serde(default)]
    use_abliterate_lora: bool,
    #[serde(default)]
    use_repacked_ptq1: bool,
    #[serde(default)]
    check_only: bool,
    parent_process_id: Option<u32>,
    parent_creation_time_file_time: Option<u64>,
}

#[cfg(windows)]
fn default_profile() -> String {
    "reference-cpu".into()
}

/// Observe fixed memory admission or run one owned, bounded CPU decode check.
///
/// # Errors
/// Rejects unknown options, unsupported hosts, unsafe storage, inherited backend
/// overrides, failed verification, exhausted deadlines and failed supervision.
pub fn run(value: Value) -> crate::Result<Value> {
    #[cfg(windows)]
    {
        let request: Request = serde_json::from_value(value).map_err(|error| error.to_string())?;
        windows::run(&request)
    }
    #[cfg(not(windows))]
    {
        let _ = value;
        Err("CPU checks require native ARM64 Windows.".into())
    }
}

#[cfg(windows)]
pub(crate) fn parent_identity(id: Option<u32>, creation: Option<u64>) -> crate::Result<(u32, u64)> {
    match (id, creation) {
        (Some(id), Some(creation)) if id != 0 && creation != 0 => Ok((id, creation)),
        (None, None) => Ok((
            std::process::id(),
            bonsai_windows_process::current_process_creation_time()
                .map_err(|error| error.to_string())?,
        )),
        _ => Err("Incomplete maintenance caller identity.".into()),
    }
}

#[cfg(windows)]
pub(crate) fn runtime_spec(data: &std::path::Path, lock: &Value) -> crate::Result<Value> {
    let asset = &lock["runtime"]["asset"];
    Ok(serde_json::json!({
        "archivePath": data.join("downloads").join(asset["filename"].as_str().ok_or("Missing archive filename.")?),
        "runtimeDirectory": data.join("runtime").join(lock["runtime"]["commit"].as_str().ok_or("Missing runtime commit.")?),
        "archiveBytes": asset["bytes"], "archiveSha256": asset["sha256"]
    }))
}

#[cfg(windows)]
mod windows {
    use super::{Request, parent_identity, runtime_spec};
    use serde_json::{Value, json};
    use std::fs;
    use std::time::Instant;

    pub(super) fn run(request: &Request) -> crate::Result<Value> {
        let started = Instant::now();
        if !cfg!(target_arch = "aarch64") {
            return Err("CPU checks require native ARM64 Windows.".into());
        }
        bonsai_artifacts::validate_external_path(&request.data_directory)
            .map_err(|error| error.to_string())?;
        bonsai_cpu_policy::server::validate_environment(
            std::env::vars_os().map(|(name, _)| name.to_string_lossy().into_owned()),
        )?;
        let plan = crate::completion::cpu_check_report(json!({
            "operation": "plan", "dataDirectory": request.data_directory, "profile": request.profile,
            "useAbliterateLora": request.use_abliterate_lora, "useRepackedPtq1": request.use_repacked_ptq1
        }))?;
        let mut report = plan["Report"].clone();
        report["RecordedAtUtc"] = json!(bonsai_windows_process::utc_now());
        if request.check_only || report["Status"] != "admission-passed" {
            return Ok(report);
        }
        let (id, creation) = parent_identity(
            request.parent_process_id,
            request.parent_creation_time_file_time,
        )?;
        let runtime = crate::runtime::run(json!({
            "schemaVersion": 1, "runtime": runtime_spec(&request.data_directory, &plan["Lock"])?,
            "commit": plan["Lock"]["runtime"]["commit"], "includeHelp": false,
            "parentProcessId": id, "parentCreationTimeFileTime": creation
        }))?;
        let directory = std::env::temp_dir().join(format!(
            "SwiftBonsai-CpuCheck-{}",
            crate::native_startup::nonce()?
        ));
        bonsai_artifacts::validate_external_path(&directory).map_err(|error| error.to_string())?;
        fs::create_dir(&directory).map_err(|error| error.to_string())?;
        report["EvidenceDirectory"] = json!(directory);
        let remaining = 240_u64.saturating_sub(started.elapsed().as_secs().saturating_add(1));
        let result = execute(
            &plan, &report, &runtime, &directory, remaining, id, creation,
        );
        match &result {
            Ok(supervision) => {
                report["ModelProcessStarted"] = supervision["ModelProcessStarted"].clone();
                report["ExitCode"] = supervision["ExitCode"].clone();
                report["WindowsReportedPeakJobMemoryBytes"] =
                    supervision["PeakJobMemoryBytes"].clone();
                report["Status"] = json!("bounded-load-and-decode-completed");
            }
            Err(error) => {
                report["Status"] = json!("failed");
                report["Failure"] = json!(error);
                if let Ok(bytes) = fs::read(directory.join("supervision.json"))
                    && let Ok(supervision) = serde_json::from_slice::<Value>(&bytes)
                {
                    for (source, target) in [
                        ("ModelProcessStarted", "ModelProcessStarted"),
                        ("PeakJobMemoryBytes", "WindowsReportedPeakJobMemoryBytes"),
                        ("CancellationFailure", "CancellationFailure"),
                    ] {
                        if let Some(value) = supervision.get(source) {
                            report[target] = value.clone();
                        }
                    }
                }
            }
        }
        fs::write(
            directory.join("result.json"),
            serde_json::to_vec_pretty(&report).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        result
            .map(|_| report)
            .map_err(|error| format!("{error} CPU check evidence: {}", directory.display()))
    }

    fn execute(
        plan: &Value,
        report: &Value,
        runtime: &Value,
        directory: &std::path::Path,
        remaining: u64,
        id: u32,
        creation: u64,
    ) -> crate::Result<Value> {
        // Include hashing and cleanup in the same allowance, leaving at least the
        // supervisor's eleven-second watchdog/teardown reserve before launch.
        if remaining <= 12 {
            return Err("The CPU check exhausted its aggregate setup budget.".into());
        }
        let mut artifacts = vec![
            json!({"path": report["ModelPath"], "bytes": plan["Artifact"]["Bytes"], "sha256": plan["Artifact"]["Sha256"]}),
        ];
        if !plan["AdapterLock"].is_null() {
            let adapter = &plan["AdapterLock"]["adapter"];
            artifacts.push(json!({"path": report["AdapterPath"], "bytes": adapter["bytes"], "sha256": adapter["sha256"]}));
        }
        let runtime_directory = runtime["Runtime"]["runtimeDirectory"]
            .as_str()
            .ok_or("Missing verified runtime directory.")?;
        crate::completion::run(json!({
            "schemaVersion": 1, "executable": std::path::Path::new(runtime_directory).join("llama-completion.exe"),
            "arguments": report["EffectiveArguments"], "workingDirectory": directory, "evidenceDirectory": directory,
            "artifacts": artifacts, "runtime": runtime["Runtime"],
            "memoryLimitBytes": report["ServiceCeilingBytes"], "requiredAvailableBytes": report["RequiredAvailableBytes"],
            "physicalReserveBytes": report["AvailableMemoryFloorBytes"], "timeoutSeconds": 180_u64.min(remaining - 12),
            "totalTimeoutSeconds": remaining, "stdoutLimitBytes": null, "stderrLimitBytes": null,
            "requireNonemptyStdout": true, "ciOnly": false, "parentProcessId": id, "parentCreationTimeFileTime": creation
        }))
    }
}
