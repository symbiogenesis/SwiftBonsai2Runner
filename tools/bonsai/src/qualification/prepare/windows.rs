//! Compiled preparation envelope and retained model-free worker preflight.

use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use bonsai_artifacts::{ArtifactError, VerifiedFile, validate_external_path};
use bonsai_windows_process::ParentProcess;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::native_qualification::save_report;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    data_directory: PathBuf,
    #[serde(default = "standard_profile")]
    qualification_profile: String,
    #[serde(default)]
    use_repacked_ptq1: bool,
    parent_process_id: Option<u32>,
    parent_creation_time_file_time: Option<u64>,
}

fn standard_profile() -> String {
    "standard-scale-one".into()
}

struct Context {
    request: Request,
    screen: bool,
    plan: Value,
    root: PathBuf,
    report: Value,
    preflight_report: Option<Value>,
    processor: String,
    parent: ParentProcess,
    parent_id: u32,
    parent_creation: u64,
}

fn external(path: &Path) -> crate::Result<()> {
    validate_external_path(path).map_err(|error| error.to_string())
}

fn publish(root: &Path, screen: bool) -> crate::Result<()> {
    let destination =
        PathBuf::from(std::env::var_os("GITHUB_ENV").ok_or("Expected the CI environment file.")?);
    external(&destination)?;
    let path = root
        .to_str()
        .filter(|path| !path.contains(['\r', '\n']))
        .ok_or("Expected a single-line Unicode evidence path.")?;
    let name = if screen {
        "BONSAI_PTQ1_EVIDENCE"
    } else {
        "BONSAI_LORA_EVIDENCE"
    };
    let mut file = OpenOptions::new()
        .append(true)
        .open(destination)
        .map_err(|error| error.to_string())?;
    writeln!(file, "{name}={path}").map_err(|error| error.to_string())
}

impl Context {
    fn checkpoint(&self) -> crate::Result<()> {
        self.parent
            .ensure_alive()
            .map_err(|error| error.to_string())
    }

    fn phase(&mut self, phase: &str) -> crate::Result<()> {
        self.checkpoint()?;
        self.report["Phase"] = json!(phase);
        save_report(&self.root.join("preparation.json"), &self.report)
    }

    fn admission(&self, repacked: bool) -> crate::Result<Value> {
        let admission =
            crate::cpu_check::run(json!({"dataDirectory": self.request.data_directory,
            "profile": "swift-pq2-assessment", "useAbliterateLora": true,
            "useRepackedPtq1": repacked, "checkOnly": true}))?;
        if admission["Status"] != "admission-passed" {
            return Err(
                "The unchanged shared Swift available-memory admission requirement was not met."
                    .into(),
            );
        }
        Ok(admission)
    }

    fn runtime(&mut self) -> crate::Result<Value> {
        self.phase("runtime-verification")?;
        crate::runtime::run(
            json!({"schemaVersion": 1, "commit": self.plan["Lock"]["runtime"]["commit"],
            "runtime": crate::cpu_check::runtime_spec(&self.request.data_directory, &self.plan["Lock"])?,
            "timeoutSeconds": 60, "parentProcessId": self.parent_id, "parentCreationTimeFileTime": self.parent_creation}),
        )
    }

    fn adapter(&mut self) -> crate::Result<(Value, VerifiedFile)> {
        self.phase("adapter-verification")?;
        let adapter = &self.plan["AdapterLock"]["adapter"];
        let path = Path::new(
            self.plan["AdapterPath"]
                .as_str()
                .ok_or("Missing compiled adapter path.")?,
        );
        let guard = VerifiedFile::verify_with(
            path,
            adapter["bytes"].as_u64().ok_or("Missing adapter length.")?,
            adapter["sha256"]
                .as_str()
                .ok_or("Missing adapter digest.")?,
            || self.checkpoint().map_err(|_| ArtifactError::Cancelled),
        )
        .map_err(|error| error.to_string())?;
        Ok((
            json!({"Status": "verified", "AdapterPath": path, "Repository": adapter["repository"],
            "Revision": adapter["revision"], "Sha256": adapter["sha256"]}),
            guard,
        ))
    }

    fn preflight(&mut self, runtime: &Value) -> crate::Result<()> {
        self.phase("model-free-worker-preflight")?;
        let lane = self.root.join("worker-preflight");
        fs::create_dir(&lane).map_err(|error| error.to_string())?;
        let path = self.root.join("worker-preflight.json");
        let mut report = json!({"SchemaVersion": 1, "Kind": "model-free-worker-startup", "Status": "running",
            "Revision": std::env::var("GITHUB_SHA").ok(), "RunUrl": self.report["RunUrl"],
            "RunAttempt": std::env::var("GITHUB_RUN_ATTEMPT").ok(), "Runner": std::env::var("RUNNER_NAME").ok(),
            "ProcessorName": self.processor, "RuntimeCommit": runtime["Commit"], "Arguments": ["--version"],
            "ModelLoaded": false, "ModelTrialsRequested": false, "WorkerPolicy": "required-headless", "DefaultMayChange": false});
        save_report(&path, &report)?;
        let result = crate::completion::run(preflight_request(
            runtime,
            &self.plan,
            &lane,
            self.parent_id,
            self.parent_creation,
        )?);
        match &result {
            Ok(supervision) => {
                report["Status"] = json!("passed");
                report["Supervision"] = supervision.clone();
            }
            Err(error) => {
                report["Status"] = json!("failed");
                report["Failure"] = json!(error);
            }
        }
        save_report(&path, &report)?;
        self.preflight_report = Some(report);
        result.map(|_| ())
    }
}

fn preflight_request(
    runtime: &Value,
    plan: &Value,
    lane: &Path,
    parent: u32,
    creation: u64,
) -> crate::Result<Value> {
    Ok(json!({"schemaVersion": 1,
        "executable": Path::new(runtime["Runtime"]["runtimeDirectory"].as_str().ok_or("Missing verified runtime directory.")?).join("llama-completion.exe"),
        "arguments": ["--version"], "workingDirectory": lane, "evidenceDirectory": lane,
        "runtime": runtime["Runtime"], "artifacts": [], "memoryLimitBytes": 536_870_912,
        "requiredAvailableBytes": plan["Policy"]["RequiredAvailableBytes"], "physicalReserveBytes": plan["Policy"]["AvailableMemoryFloorBytes"],
        "timeoutSeconds": 10, "totalTimeoutSeconds": 60, "stdoutLimitBytes": 65_536, "stderrLimitBytes": 65_536,
        "requireNonemptyStdout": false, "ciOnly": false, "parentProcessId": parent, "parentCreationTimeFileTime": creation}))
}

trait Preparation {
    fn runtime(&mut self) -> crate::Result<Value>;
    fn preflight(&mut self, runtime: &Value) -> crate::Result<()>;
    fn admission(&mut self, repacked: bool) -> crate::Result<Value>;
}

impl Preparation for Context {
    fn runtime(&mut self) -> crate::Result<Value> {
        Self::runtime(self)
    }
    fn preflight(&mut self, runtime: &Value) -> crate::Result<()> {
        Self::preflight(self, runtime)
    }
    fn admission(&mut self, repacked: bool) -> crate::Result<Value> {
        self.phase("model-admission")?;
        Self::admission(self, repacked)
    }
}

fn prepared_admissions(
    steps: &mut impl Preparation,
    screen: bool,
    repacked: bool,
) -> crate::Result<(Value, Value, Option<Value>)> {
    // Screen worker failure must precede both model admissions and adapter hashing.
    if screen {
        let runtime = steps.runtime()?;
        steps.preflight(&runtime)?;
        Ok((
            runtime,
            steps.admission(false)?,
            Some(steps.admission(true)?),
        ))
    } else {
        let admission = steps.admission(repacked)?;
        Ok((steps.runtime()?, admission, None))
    }
}

fn execute(context: &mut Context) -> crate::Result<Value> {
    let screen = context.screen;
    let repacked = context.request.use_repacked_ptq1;
    let (runtime, admission, ptq1) = prepared_admissions(context, screen, repacked)?;
    let (adapter, _adapter_guard) = context.adapter()?;
    context.phase("capture")?;
    context.report["ModelTrialsRequested"] = json!(true);
    if let Some(preflight) = &mut context.preflight_report {
        preflight["ModelTrialsRequested"] = json!(true);
        save_report(&context.root.join("worker-preflight.json"), preflight)?;
    }
    save_report(&context.root.join("preparation.json"), &context.report)?;
    let mut request = json!({"operation": if context.screen { "screen-capture" } else { "capture" },
        "dataDirectory": context.request.data_directory, "lock": context.plan["Lock"], "runtime": runtime, "adapter": adapter,
        "evidenceDirectory": context.root, "processorName": context.processor,
        "runId": crate::native_startup::nonce()?, "startedUtc": bonsai_windows_process::utc_now(),
        "parentProcessId": context.parent_id, "parentCreationTimeFileTime": context.parent_creation});
    if let Some(ptq1) = ptq1 {
        request["pq2Admission"] = admission;
        request["ptq1Admission"] = ptq1;
    } else {
        request["admission"] = admission;
        request["qualificationProfile"] = json!(context.request.qualification_profile);
        request["weightRepresentation"] = json!(if context.request.use_repacked_ptq1 {
            "current-repacked-PTQ1_0"
        } else {
            "PQ2_0"
        });
    }
    crate::qualification::run(request)
}

pub(super) fn run(value: Value, screen: bool) -> crate::Result<Value> {
    let request: Request = serde_json::from_value(value).map_err(|error| error.to_string())?;
    if !["standard-scale-one", "app-scale-1.3"].contains(&request.qualification_profile.as_str())
        || (screen
            && (request.use_repacked_ptq1 || request.qualification_profile != "standard-scale-one"))
    {
        return Err("Select one fixed paired contract; the screen fixes both representations and scale 1.3.".into());
    }
    external(&request.data_directory)?;
    let host = bonsai_windows_process::native_build_host().map_err(|error| error.to_string())?;
    crate::qualification::capture::screen_context(&host.processor_name)?;
    let plan = bonsai_cpu_policy::run(
        json!({"operation": "plan", "dataDirectory": request.data_directory,
        "profile": "swift-pq2-assessment", "useAbliterateLora": true, "useRepackedPtq1": request.use_repacked_ptq1}),
    )?;
    let (parent_id, parent_creation) = crate::cpu_check::parent_identity(
        request.parent_process_id,
        request.parent_creation_time_file_time,
    )?;
    let parent =
        ParentProcess::open(parent_id, parent_creation).map_err(|error| error.to_string())?;
    let root = std::env::temp_dir().join(format!(
        "Bonsai-Qualification-{}",
        crate::native_startup::nonce()?
    ));
    external(&root)?;
    fs::create_dir(&root).map_err(|error| error.to_string())?;
    publish(&root, screen)?;
    let report = json!({"SchemaVersion": 1, "Status": "preparing", "ModelTrialsRequested": false,
        "DefaultMayChange": false, "SourceRevision": std::env::var("GITHUB_SHA").ok(),
        "RunUrl": format!("https://github.com/symbiogenesis/SwiftBonsai2Runner/actions/runs/{}", std::env::var("GITHUB_RUN_ID").unwrap_or_default())});
    let mut context = Context {
        request,
        screen,
        plan,
        root,
        report,
        preflight_report: None,
        processor: host.processor_name,
        parent,
        parent_id,
        parent_creation,
    };
    let result = execute(&mut context);
    match &result {
        Ok(receipt) => {
            context.report["Status"] = json!("prepared-capture-completed");
            context.report["CaptureReceipt"] = receipt.clone();
        }
        Err(error) => {
            context.report["Status"] = json!("failed");
            context.report["Failure"] = json!(error);
        }
    }
    save_report(&context.root.join("preparation.json"), &context.report)?;
    result.map_err(|error| format!("{error} Preparation evidence: {}", context.root.display()))
}

#[cfg(test)]
mod tests {
    use super::{Preparation, preflight_request, prepared_admissions};
    use serde_json::{Value, json};
    use std::path::Path;

    #[test]
    fn worker_preflight_preserves_the_model_free_headless_contract() -> crate::Result<()> {
        let request = preflight_request(
            &json!({"Runtime": {"runtimeDirectory": "C:\\runtime"}}),
            &json!({"Policy": {"RequiredAvailableBytes": 16_642_998_272_u64, "AvailableMemoryFloorBytes": 4_294_967_296_u64}}),
            Path::new("C:\\evidence"),
            12,
            34,
        )?;
        assert_eq!(request["arguments"], json!(["--version"]));
        assert_eq!(request["artifacts"], json!([]));
        assert_eq!(request["memoryLimitBytes"], 536_870_912);
        assert_eq!(request["timeoutSeconds"], 10);
        assert_eq!(request["stdoutLimitBytes"], 65_536);
        assert_eq!(request["stderrLimitBytes"], 65_536);
        assert_eq!(request["requiredAvailableBytes"], 16_642_998_272_u64);
        assert_eq!(request["physicalReserveBytes"], 4_294_967_296_u64);
        assert_eq!(request["parentProcessId"], 12);
        assert_eq!(request["parentCreationTimeFileTime"], 34);
        Ok(())
    }

    struct Steps {
        failed: bool,
        events: Vec<&'static str>,
    }
    impl Preparation for Steps {
        fn runtime(&mut self) -> crate::Result<Value> {
            self.events.push("runtime");
            Ok(json!({}))
        }
        fn preflight(&mut self, _: &Value) -> crate::Result<()> {
            self.events.push("preflight");
            if self.failed {
                Err("Fixture worker startup failure.".into())
            } else {
                Ok(())
            }
        }
        fn admission(&mut self, repacked: bool) -> crate::Result<Value> {
            self.events.push(if repacked { "ptq1" } else { "pq2" });
            Ok(json!({}))
        }
    }

    #[test]
    fn failed_worker_preflight_never_reaches_either_model_admission() -> crate::Result<()> {
        let mut failed = Steps {
            failed: true,
            events: vec![],
        };
        assert!(prepared_admissions(&mut failed, true, false).is_err());
        assert_eq!(failed.events, ["runtime", "preflight"]);
        let mut passed = Steps {
            failed: false,
            events: vec![],
        };
        prepared_admissions(&mut passed, true, false)?;
        assert_eq!(passed.events, ["runtime", "preflight", "pq2", "ptq1"]);
        Ok(())
    }
}
