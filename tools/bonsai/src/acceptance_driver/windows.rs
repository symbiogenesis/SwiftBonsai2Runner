//! Native Windows owner workflow execution without PowerShell child supervision.

use super::{
    Bridge, REMOVED_CREDENTIALS, apply_budget, bridge, carries_workload_verdict, number, profile,
    startup_markers,
};
use bonsai_windows_process::{OwnedProcess, ToolProcessSpec};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::Write as _,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_default()
}
fn required(name: &str) -> crate::Result<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("Required owner environment is missing: {name}"))
}
fn ticks() -> crate::Result<(u64, u64)> {
    bonsai_windows_process::monotonic_ticks().map_err(|error| error.to_string())
}
fn external(path: &Path) -> crate::Result<()> {
    bonsai_artifacts::validate_external_path(path).map_err(|error| error.to_string())
}
fn bool_env(name: &str) -> crate::Result<bool> {
    match required(name)?.as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(format!("Owner flag must be true or false: {name}")),
    }
}

fn github_env(name: &str, value: &str) -> crate::Result<()> {
    let path = PathBuf::from(required("GITHUB_ENV")?);
    external(&path)?;
    let mut file = OpenOptions::new()
        .append(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    writeln!(file, "{name}={value}").map_err(|error| error.to_string())
}

fn write_json(path: &Path, value: &Value) -> crate::Result<()> {
    external(path)?;
    let temporary = path.with_extension(format!("{}.partial", crate::startup_process::unique()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| error.to_string())?;
    serde_json::to_writer_pretty(&mut file, value).map_err(|error| error.to_string())?;
    file.write_all(b"\n")
        .and_then(|()| file.sync_all())
        .map_err(|error| error.to_string())?;
    fs::rename(temporary, path).map_err(|error| error.to_string())
}

fn ci_guard() -> crate::Result<()> {
    for (name, expected) in [
        ("GITHUB_ACTIONS", "true"),
        ("GITHUB_REPOSITORY", "symbiogenesis/SwiftBonsai2Runner"),
        ("GITHUB_REF", "refs/heads/main"),
        ("RUNNER_OS", "Windows"),
        ("RUNNER_ARCH", "ARM64"),
    ] {
        if env(name) != expected {
            return Err(
                "Native owner execution requires its exact main-only Windows ARM64 CI lane.".into(),
            );
        }
    }
    if std::env::consts::ARCH != "aarch64" {
        return Err("Owner acceptance requires a native ARM64 executable.".into());
    }
    Ok(())
}

fn budget(prefix: &str, maximum: u64) -> crate::Result<(u64, u64, Value)> {
    let (snapshot, frequency) = ticks()?;
    let start = required(&format!("{prefix}_JOB_STARTED_TICK"))?
        .parse::<u64>()
        .map_err(|_| "Original whole-job monotonic start is invalid.")?;
    let limits = crate::native_acceptance_budget::run(
        json!({"jobStartedUtc":required(&format!("{prefix}_JOB_STARTED_UTC"))?,"jobStartedTick":start,"nowUtc":bonsai_windows_process::utc_now(),"nowTick":snapshot,"tickFrequency":frequency,"runId":required("GITHUB_RUN_ID")?,"revision":required("GITHUB_SHA")?,"wholeJobMaximumSeconds":maximum,
        "ownerDeadline":{"RunId":env("BONSAI_NATIVE_OWNER_RUN_ID"),"Revision":env("BONSAI_NATIVE_OWNER_REVISION"),"StartedTick":env("BONSAI_NATIVE_OWNER_STARTED_TICK"),"StartedUtc":env("BONSAI_NATIVE_OWNER_STARTED_UTC"),"WorkSeconds":env("BONSAI_NATIVE_OWNER_WORK_SECONDS")}}),
    )?;
    Ok((snapshot, frequency, limits))
}

fn remaining(deadline: u64, frequency: u64) -> crate::Result<Duration> {
    crate::startup_cancel::checkpoint()?;
    let (now, current_frequency) = ticks()?;
    if frequency == 0 || current_frequency != frequency || now >= deadline {
        return Err("Original owner process allowance expired.".into());
    }
    let elapsed = deadline - now;
    Ok(Duration::from_secs(elapsed / frequency)
        + Duration::from_nanos(
            u64::try_from(u128::from(elapsed % frequency) * 1_000_000_000 / u128::from(frequency))
                .map_err(|error| error.to_string())?,
        ))
}

struct ChildResult {
    code: i32,
    stdout: String,
    stderr: String,
    capture: PathBuf,
}

fn child_failure(message: &str, child: &ChildResult) -> String {
    let diagnostic = child.stderr.chars().take(4096).collect::<String>();
    format!(
        "{message} Exit code: {}. Capture: {}. {diagnostic}",
        child.code,
        child.capture.display()
    )
}

fn supervise(
    executable: &Path,
    arguments: Vec<OsString>,
    working: &Path,
    environment: BTreeMap<String, String>,
    deadline: u64,
    frequency: u64,
) -> crate::Result<ChildResult> {
    remaining(deadline, frequency)?;
    external(executable)?;
    external(working)?;
    let capture = working.join(format!(
        "acceptance-driver-{}",
        crate::startup_process::unique()
    ));
    fs::create_dir(&capture).map_err(|error| error.to_string())?;
    let stdout = capture.join("stdout.txt");
    let stderr = capture.join("stderr.txt");
    let process = OwnedProcess::start_tool_with_removed(
        &ToolProcessSpec {
            executable: executable.to_owned(),
            arguments,
            working_directory: working.to_owned(),
            stdout: stdout.clone(),
            stderr: stderr.clone(),
            environment,
        },
        &REMOVED_CREDENTIALS
            .iter()
            .map(|name| (*name).to_owned())
            .collect::<Vec<_>>(),
    )
    .map_err(|error| error.to_string())?;
    let result = (|| -> crate::Result<ChildResult> {
        let code = loop {
            let wait = remaining(deadline, frequency)?.min(Duration::from_millis(20));
            if fs::metadata(&stdout)
                .map_err(|error| error.to_string())?
                .len()
                > 16 * 1024 * 1024
                || fs::metadata(&stderr)
                    .map_err(|error| error.to_string())?
                    .len()
                    > 64 * 1024 * 1024
            {
                return Err("Owner process output exceeded its bounded capture allowance.".into());
            }
            if let Some(code) = process
                .wait_for_exit(wait)
                .map_err(|error| error.to_string())?
            {
                remaining(deadline, frequency)?;
                break i32::from_ne_bytes(code.to_ne_bytes());
            }
        };
        // Native root completion can precede Job accounting retirement. Charge
        // the same short grace as other native tools to the original deadline.
        let grace = Instant::now();
        while process
            .active_processes()
            .map_err(|error| error.to_string())?
            != 0
        {
            if grace.elapsed() >= Duration::from_millis(250) {
                return Err("Owner root exited with surviving process-tree members.".into());
            }
            std::thread::sleep(remaining(deadline, frequency)?.min(Duration::from_millis(10)));
        }
        let read = |path: &Path, maximum: u64| -> crate::Result<String> {
            let file =
                bonsai_artifacts::open_locked_file(path).map_err(|error| error.to_string())?;
            let bytes = crate::bounded_input::read(file, maximum).map_err(|error| {
                error.into_message("Owner process output exceeded its byte bound.")
            })?;
            String::from_utf8(bytes).map_err(|_| "Owner process output is not UTF-8.".into())
        };
        let output = ChildResult {
            code,
            stdout: read(&stdout, 16 * 1024 * 1024)?,
            stderr: read(&stderr, 64 * 1024 * 1024)?,
            capture: capture.clone(),
        };
        if code == 0 {
            fs::remove_file(&stdout)
                .and_then(|()| fs::remove_file(&stderr))
                .map_err(|error| error.to_string())?;
            fs::remove_dir(&capture).map_err(|error| error.to_string())?;
        } else {
            for path in [&stdout, &stderr] {
                if fs::metadata(path).is_ok_and(|metadata| metadata.len() == 0) {
                    fs::remove_file(path).map_err(|error| error.to_string())?;
                }
            }
        }
        Ok(output)
    })();
    if result.is_err() || !process.active_processes().is_ok_and(|count| count == 0) {
        process
            .stop(Duration::from_secs(10))
            .map_err(|error| format!("Native owner process-tree cleanup failed: {error}"))?;
        for path in [&stdout, &stderr] {
            if fs::metadata(path).is_ok_and(|metadata| metadata.len() == 0) {
                fs::remove_file(path).map_err(|error| error.to_string())?;
            }
        }
        if fs::read_dir(&capture).is_ok_and(|mut entries| entries.next().is_none()) {
            fs::remove_dir(&capture).map_err(|error| error.to_string())?;
        }
    }
    result
}

fn selected_bridge(limits: &Value, snapshot: u64, frequency: u64) -> crate::Result<Bridge> {
    let (now, current_frequency) = ticks()?;
    if frequency != current_frequency {
        return Err("Owner monotonic clock frequency changed.".into());
    }
    bridge(limits, snapshot, now, frequency)
}

struct CheckedPackage {
    _receipt: File,
    _files: Vec<bonsai_artifacts::VerifiedFile>,
}

fn hold_package(test: &Path, deadline: u64, frequency: u64) -> crate::Result<CheckedPackage> {
    let root = test
        .parent()
        .ok_or("Acceptance package directory missing.")?;
    if test
        .file_name()
        .is_none_or(|name| name != "native-model-acceptance.exe")
        || std::env::current_exe().map_err(|error| error.to_string())?
            != root.join("bonsai-tools.exe")
    {
        return Err("Acceptance must use the exact source-bound hosted package.".into());
    }
    let names = [
        "native-model-acceptance.exe",
        "bonsai-tools.exe",
        "bonsai-ci.exe",
        "strict-release-analyzers.json",
        "workflow-contract.json",
    ];
    let mut expected = names
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    expected.insert("receipt.json");
    let actual = fs::read_dir(root)
        .map_err(|error| error.to_string())?
        .map(|entry| {
            entry.map_err(|error| error.to_string()).and_then(|entry| {
                let metadata =
                    fs::symlink_metadata(entry.path()).map_err(|error| error.to_string())?;
                if !metadata.is_file() || bonsai_artifacts::is_redirect(&metadata) {
                    return Err("Redirected or nested acceptance package entry.".into());
                }
                entry
                    .file_name()
                    .into_string()
                    .map_err(|_| "Acceptance package member must be Unicode.".into())
            })
        })
        .collect::<crate::Result<std::collections::BTreeSet<_>>>()?;
    if actual
        .iter()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>()
        != expected
    {
        return Err("Hosted acceptance package inventory differs.".into());
    }
    let mut receipt = bonsai_artifacts::open_locked_file(&root.join("receipt.json"))
        .map_err(|error| error.to_string())?;
    let raw = crate::bounded_input::read(&mut receipt, 65_536)
        .map_err(|error| error.into_message("Acceptance package receipt exceeds its bound."))?;
    let value: Value = serde_json::from_slice(&raw).map_err(|error| error.to_string())?;
    let files = value["Files"]
        .as_array()
        .filter(|items| items.len() == 5)
        .ok_or("Hosted acceptance package file inventory differs.")?;
    super::package_identity(
        &value,
        &required("GITHUB_SHA")?,
        &required("GITHUB_RUN_ID")?,
        &required("GITHUB_RUN_ATTEMPT")?,
    )?;
    let mut held = Vec::new();
    for name in names {
        let selected = files
            .iter()
            .filter(|entry| entry["Name"] == name)
            .collect::<Vec<_>>();
        if selected.len() != 1 {
            return Err("Hosted acceptance binary digest inventory differs.".into());
        }
        let file = &selected[0];
        let length = number(file, "Bytes")?;
        if !(1..=134_217_728).contains(&length) {
            return Err("Hosted acceptance binary extent differs.".into());
        }
        let hash = file["Sha256"]
            .as_str()
            .filter(|s| crate::identity::is_lower_hex(s, 64))
            .ok_or("Invalid hosted acceptance digest.")?;
        held.push(
            bonsai_artifacts::VerifiedFile::verify_with(&root.join(name), length, hash, || {
                remaining(deadline, frequency)
                    .map(|_| ())
                    .map_err(|_| bonsai_artifacts::ArtifactError::Cancelled)
            })
            .map_err(|error| error.to_string())?,
        );
        if Path::new(name)
            .extension()
            .is_some_and(|extension| extension == "exe")
        {
            crate::startup_cache::assert_arm64(&root.join(name))?;
        }
    }
    Ok(CheckedPackage {
        _receipt: receipt,
        _files: held,
    })
}

fn ptq1() -> crate::Result<Value> {
    let temporary = PathBuf::from(required("RUNNER_TEMP")?);
    external(&temporary)?;
    let report_path = temporary.join("ptq1-owner-budget.json");
    github_env(
        "BONSAI_PTQ1_OWNER_BUDGET",
        report_path
            .to_str()
            .ok_or("Owner budget path must be Unicode.")?,
    )?;
    let capture_mode = required("BONSAI_PTQ1_CAPTURE_MODE")?;
    let mut report = json!({"Revision":required("GITHUB_SHA")?,"RunId":required("GITHUB_RUN_ID")?,"RunAttempt":required("GITHUB_RUN_ATTEMPT")?,"Status":"preparing","CaptureMode":capture_mode,"WholeJobMaximumSeconds":300,"DefaultMayChange":false,"PowerBefore":crate::host_cpu::power_snapshot(),"PowerAfter":null,"PowerObservationScope":"Native driver entry/exit; pre-bootstrap setup power remains independently observed or unknown."});
    write_json(&report_path, &report)?;
    let result = (|| -> crate::Result<Value> {
        let (snapshot, frequency, limits) = budget("BONSAI_PTQ1", 300)?;
        let bridge = selected_bridge(&limits, snapshot, frequency)?;
        report["Status"] = "budget-established".into();
        report["Limits"] = limits;
        report["WorkBudgetSeconds"] = bridge.case_seconds.into();
        report["WorkDeadlineTick"] = bridge.work_deadline.into();
        report["HardDeadlineTick"] = bridge.hard_deadline.into();
        write_json(&report_path, &report)?;
        let executable = std::env::current_exe().map_err(|error| error.to_string())?;
        let row_workers = required("BONSAI_PTQ1_ROW_WORKERS")?;
        if !["1", "4"].contains(&row_workers.as_str()) {
            return Err("Select one exact Rust capture row-worker count.".into());
        }
        let request = json!({"operation":"capture","kind":"ptq1","repositoryDirectory":required("GITHUB_WORKSPACE")?,"captureMode":capture_mode,"dataDirectory":PathBuf::from(required("LOCALAPPDATA")?).join("SwiftBonsai2"),"captureToolsDirectory":temporary.join("SwiftBonsai-Ptq1CaptureTools"),"workBudgetSeconds":bridge.case_seconds,"workDeadlineTick":bridge.work_deadline,"rustBackend":required("BONSAI_PTQ1_RUST_BACKEND")?,"rowWorkers":row_workers.parse::<u64>().map_err(|_|"Invalid row-worker count.")?,"adapterCondition":required("BONSAI_PTQ1_ADAPTER_CONDITION")?});
        // The child's existing typed planner validates all capture controls before
        // any model work. Its caller identity is this retained native supervisor.
        crate::native_qualification::run(
            json!({"operation":"plan","kind":"ptq1","repositoryDirectory":required("GITHUB_WORKSPACE")?,"captureMode":request["captureMode"],"rustBackend":request["rustBackend"],"rowWorkers":request["rowWorkers"],"adapterCondition":request["adapterCondition"]}),
        )?;
        let result = supervise(
            &executable,
            vec![
                "native-qualification".into(),
                "--json".into(),
                serde_json::to_string(&request)
                    .map_err(|error| error.to_string())?
                    .into(),
            ],
            &temporary,
            BTreeMap::new(),
            bridge.hard_deadline,
            frequency,
        )?;
        report["AcceptanceProcessExitCode"] = result.code.into();
        if result.code != 0 {
            return Err(child_failure(
                "The selected PTQ1 capture failed; inspect retained evidence.",
                &result,
            ));
        }
        Ok(
            json!({"Status":"completed-selected-capture","OwnerBudgetReceipt":report_path,"DefaultMayChange":false}),
        )
    })();
    if result.is_err() {
        report["Status"] = if report["Status"] == "preparing" {
            "budget-rejected"
        } else {
            "capture-rejected"
        }
        .into();
        report["AggregateBudgetExpired"] = report["HardDeadlineTick"]
            .as_u64()
            .map_or(Value::Null, |deadline| {
                json!(ticks().is_ok_and(|(now, _)| now >= deadline))
            });
    }
    report["PowerAfter"] = crate::host_cpu::power_snapshot();
    write_json(&report_path, &report)?;
    result
}

fn diagnostics(evidence: &Path, name: &str, report: &mut Value) -> crate::Result<()> {
    github_env("BONSAI_NATIVE_DIAGNOSTICS_READY", "false")?;
    let result = crate::startup_diagnostics::run(
        json!({"evidenceDirectory":evidence,"acceptanceExitCode":report["AcceptanceProcessExitCode"],"budgetExpired":report["AggregateBudgetExpired"]}),
    );
    let (ready, classification) = result.map_or_else(
        |_| (false, "collection-failed".into()),
        |diagnostics| {
            if carries_workload_verdict(name)
                && diagnostics["AllRequestedWorkloadsPassed"].is_boolean()
            {
                report["AllRequestedWorkloadsPassed"] =
                    diagnostics["AllRequestedWorkloadsPassed"].clone();
            }
            (
                diagnostics["Ready"] == true,
                diagnostics["Classification"]
                    .as_str()
                    .unwrap_or("collection-failed")
                    .to_owned(),
            )
        },
    );
    report["StartupDiagnosticsReady"] = ready.into();
    report["StartupDiagnosticsClassification"] = classification.into();
    report["PowerAfter"] = crate::host_cpu::power_snapshot();
    github_env(
        "BONSAI_NATIVE_DIAGNOSTICS_READY",
        if ready { "true" } else { "false" },
    )?;
    write_json(&evidence.join("host.json"), report)
}

#[expect(
    clippy::too_many_lines,
    reason = "One failure-retaining native owner operation encloses original-budget admission, exact case selection, process containment and public diagnostics."
)]
fn acceptance() -> crate::Result<Value> {
    let evidence = PathBuf::from(required("BONSAI_NATIVE_EVIDENCE")?);
    let working = PathBuf::from(required("BONSAI_NATIVE_WORK")?);
    external(&evidence)?;
    external(&working)?;
    let name = required("BONSAI_NATIVE_PROFILE")?;
    let extended = bool_env("BONSAI_NATIVE_EXTENDED")?;
    let stalled = bool_env("BONSAI_NATIVE_STALLED_CONSUMER")?;
    let maximum = required("BONSAI_NATIVE_WHOLE_JOB_SECONDS")?
        .parse::<u64>()
        .map_err(|_| "Invalid whole-job maximum.")?;
    let reason = env("BONSAI_NATIVE_BUDGET_REASON");
    if ![300, 600, 900].contains(&maximum)
        || (maximum != 300 && reason.trim().is_empty())
        || reason.encode_utf16().count() > 512
        || reason.chars().any(|ch| ch <= '\u{1f}' || ch == '\u{7f}')
    {
        return Err("Owner budget declaration is invalid.".into());
    }
    let host = bonsai_windows_process::native_build_host().map_err(|error| error.to_string())?;
    let mut report = json!({"SchemaVersion":1,"Revision":required("GITHUB_SHA")?,"RunId":required("GITHUB_RUN_ID")?,"RunAttempt":required("GITHUB_RUN_ATTEMPT")?,"Cpu":host.processor_name,"CpuIdentitySource":"HKLM/HARDWARE/DESCRIPTION/System/CentralProcessor/0","Os":host.windows_version,"Architecture":"Arm64","Profile":name,"BoundedHistoryRequested":extended,"StalledConsumerRequested":stalled,"AcceptanceProcessExitCode":null,"AggregateBudgetExpired":false,"OwnerJobStartedTick":env("BONSAI_NATIVE_JOB_STARTED_TICK"),"OwnerJobStartedUtc":env("BONSAI_NATIVE_JOB_STARTED_UTC"),"WholeJobMaximumSeconds":maximum,"PostProcessReserveSeconds":30,"BudgetReason":reason,"PreparationElapsedMilliseconds":null,"ModelCaseBudgetSeconds":null,"PreparationWallElapsedMilliseconds":null,"PreparationMonotonicElapsedMilliseconds":null,"TestProcessHardLimitSeconds":null,"OwnerDeadlineApplied":false,"OwnerElapsedMilliseconds":null,"OwnerWorkSeconds":null,"OwnerProcessExitReserveSeconds":null,"StartupDiagnosticsReceipt":"startup-diagnostics.json","StartupDiagnosticsReady":false,"StartupDiagnosticsClassification":"not-collected","StartupFailureMarkers":[],"Passed":false,"DefaultRuntimeQualified":false,"AllRequestedWorkloadsPassed":null,"PowerBefore":crate::host_cpu::power_snapshot(),"PowerAfter":null,"PowerObservationScope":"Native driver entry/exit; pre-bootstrap setup power remains independently observed or unknown."});
    let result = (|| -> crate::Result<Value> {
        if bonsai_cpu_policy::hardware::SnapdragonFamily::from_processor_name(&host.processor_name)
            .is_none()
        {
            return Err(
                "Actual Snapdragon X Elite or X2 Elite Extreme identity is required.".into(),
            );
        }
        let selected = profile(&name, extended, stalled)?;
        let test = PathBuf::from(required("BONSAI_NATIVE_TEST")?);
        let (snapshot, frequency, limits) = budget("BONSAI_NATIVE", maximum).inspect_err(|_| {
            report["AggregateBudgetExpired"] = true.into();
        })?;
        let first = selected_bridge(&limits, snapshot, frequency)?;
        let _package = hold_package(&test, first.hard_deadline, frequency)?;
        let listed = supervise(
            &test,
            vec!["--list".into(), "--ignored".into()],
            &working,
            BTreeMap::new(),
            first.hard_deadline,
            frequency,
        )?;
        if listed.code != 0
            || !listed
                .stdout
                .lines()
                .any(|line| line == format!("{selected}: test"))
        {
            return Err("Required actual-model case was not found.".into());
        }
        let budget = selected_bridge(&limits, snapshot, frequency).inspect_err(|_| {
            report["AggregateBudgetExpired"] = true.into();
        })?;
        apply_budget(&mut report, &limits, &budget);
        let environment = BTreeMap::from([
            (
                "BONSAI_NATIVE_CASE_BUDGET_SECONDS".into(),
                budget.case_seconds.to_string(),
            ),
            (
                "BONSAI_NATIVE_WHOLE_JOB_SECONDS".into(),
                number(&limits, "WholeJobMaximumSeconds")?.to_string(),
            ),
        ]);
        let child = supervise(
            &test,
            vec![
                selected.into(),
                "--exact".into(),
                "--ignored".into(),
                "--nocapture".into(),
            ],
            &working,
            environment,
            budget.hard_deadline,
            frequency,
        )
        .inspect_err(|_| {
            report["AggregateBudgetExpired"] = ticks()
                .is_ok_and(|(now, _)| now >= budget.hard_deadline)
                .into();
        })?;
        report["AcceptanceProcessExitCode"] = child.code.into();
        report["StartupFailureMarkers"] = startup_markers(&child.stderr).into();
        if child.code != 0 {
            return Err(child_failure(
                "Actual-model application acceptance failed.",
                &child,
            ));
        }
        let receipt = evidence.join("acceptance.json");
        if !receipt.is_file() {
            return Err("Actual-model acceptance receipt is missing.".into());
        }
        report["Passed"] = true.into();
        Ok(
            json!({"Passed":true,"HostReceipt":evidence.join("host.json"),"DefaultRuntimeQualified":false}),
        )
    })();
    report["PowerAfter"] = crate::host_cpu::power_snapshot();
    diagnostics(&evidence, &name, &mut report)?;
    result
}

pub(super) fn run(operation: &str) -> crate::Result<Value> {
    ci_guard()?;
    match operation {
        "ptq1" => ptq1(),
        "acceptance" => acceptance(),
        _ => Err("Unknown native owner operation.".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn arguments() -> Vec<OsString> {
        vec![
            "acceptance_driver::windows::tests::owned_native_child".into(),
            "--exact".into(),
            "--nocapture".into(),
        ]
    }

    #[test]
    fn owned_native_child() -> crate::Result<()> {
        let mode = env("BONSAI_ACCEPTANCE_DRIVER_FIXTURE");
        if mode.is_empty() {
            return Ok(());
        }
        if mode == "wait" {
            std::thread::sleep(Duration::from_secs(2));
            return Ok(());
        }
        if mode == "leaf" {
            for name in REMOVED_CREDENTIALS {
                assert!(std::env::var_os(name).is_none());
            }
            assert!(std::env::var_os("SystemRoot").is_some());
            assert!(std::env::var_os("USERPROFILE").is_some());
            std::io::stdout()
                .lock()
                .write_all("native-tree-🌳\n".as_bytes())
                .map_err(|error| error.to_string())?;
            return Ok(());
        }
        assert_eq!(mode, "parent");
        assert_eq!(env("GH_TOKEN"), "synthetic-driver-credential");
        let (now, frequency) = ticks()?;
        let executable = std::env::current_exe().map_err(|error| error.to_string())?;
        let working = std::env::current_dir().map_err(|error| error.to_string())?;
        let output = supervise(
            &executable,
            arguments(),
            &working,
            BTreeMap::from([("BONSAI_ACCEPTANCE_DRIVER_FIXTURE".into(), "leaf".into())]),
            now + frequency * 5,
            frequency,
        )?;
        assert_eq!(output.code, 0);
        assert!(output.stdout.contains("native-tree-🌳"));
        assert_eq!(env("GH_TOKEN"), "synthetic-driver-credential");
        Ok(())
    }

    #[test]
    fn native_owner_supervision_removes_credentials_only_from_child() -> crate::Result<()> {
        let root = std::env::temp_dir().join(format!(
            "acceptance-driver-fixture-{}",
            crate::startup_process::unique()
        ));
        fs::create_dir(&root).map_err(|error| error.to_string())?;
        let executable = root.join("native-driver-fixture.exe");
        fs::copy(
            std::env::current_exe().map_err(|error| error.to_string())?,
            &executable,
        )
        .map_err(|error| error.to_string())?;
        let before = std::env::var_os("GH_TOKEN");
        let (now, frequency) = ticks()?;
        let result = supervise(
            &executable,
            arguments(),
            &root,
            BTreeMap::from([
                ("BONSAI_ACCEPTANCE_DRIVER_FIXTURE".into(), "parent".into()),
                ("GH_TOKEN".into(), "synthetic-driver-credential".into()),
            ]),
            now + frequency * 10,
            frequency,
        );
        assert_eq!(std::env::var_os("GH_TOKEN"), before);
        let output = result?;
        assert_eq!(output.code, 0, "{}", output.stderr);
        fs::remove_file(&executable)
            .and_then(|()| fs::remove_dir(&root))
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    #[test]
    fn native_owner_supervision_stops_at_original_deadline() -> crate::Result<()> {
        let root = std::env::temp_dir().join(format!(
            "acceptance-driver-timeout-{}",
            crate::startup_process::unique()
        ));
        fs::create_dir(&root).map_err(|error| error.to_string())?;
        let executable = root.join("native-driver-fixture.exe");
        fs::copy(
            std::env::current_exe().map_err(|error| error.to_string())?,
            &executable,
        )
        .map_err(|error| error.to_string())?;
        let (now, frequency) = ticks()?;
        let result = supervise(
            &executable,
            arguments(),
            &root,
            BTreeMap::from([("BONSAI_ACCEPTANCE_DRIVER_FIXTURE".into(), "wait".into())]),
            now + frequency / 10,
            frequency,
        );
        assert!(
            result
                .err()
                .is_some_and(|error| error.contains("allowance expired"))
        );
        // The supervisor has completed native Job teardown before removing its
        // unique test-only capture selection, including the expected timeout.
        external(&root)?;
        fs::remove_dir_all(&root).map_err(|error| error.to_string())?;
        Ok(())
    }
}
