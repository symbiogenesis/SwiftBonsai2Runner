//! Native owned publisher startup execution and atomic evidence retention.

use std::{
    fs::{self, OpenOptions},
    io::{Read as _, Write as _},
    path::{Path, PathBuf},
    time::Instant,
};

use bonsai_windows_process::ParentProcess;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    MODEL, PIN, allowance, cases, external, inventory, observation, pinned, smoke_deadline,
    smoke_plan,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    candidate_directory: PathBuf,
    output_directory: PathBuf,
    #[serde(default)]
    model_smoke: bool,
    data_directory: Option<PathBuf>,
    #[serde(default = "budget")]
    work_budget_seconds: u64,
    parent_process_id: Option<u32>,
    parent_creation_time_file_time: Option<u64>,
}

const fn budget() -> u64 {
    90
}

fn save(path: &Path, report: &Value) -> crate::Result<()> {
    external(path)?;
    let bytes = serde_json::to_vec_pretty(report).map_err(|error| error.to_string())?;
    if bytes.len() > 1_048_576 {
        return Err("Startup receipt exceeds its byte bound.".into());
    }
    let temporary = path.with_extension(format!("{}.tmp", crate::native_startup::nonce()?));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| error.to_string())?;
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .map_err(|error| error.to_string())?;
        drop(file);
        fs::rename(&temporary, path).map_err(|error| error.to_string())
    })();
    if temporary.exists() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn separated(candidate: &Path, output: &Path) -> crate::Result<()> {
    external(candidate)?;
    external(output)?;
    let key = |path: &Path| {
        path.to_string_lossy()
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_uppercase()
    };
    let candidate = key(candidate);
    let output = key(output);
    if candidate == output || output.starts_with(&(candidate + "\\")) {
        return Err("Startup evidence must be separate from the archived candidate.".into());
    }
    Ok(())
}

struct Context<'a> {
    request: &'a Request,
    started: Instant,
    parent: ParentProcess,
    parent_id: u32,
    parent_creation: u64,
    receipt: Value,
    receipt_path: PathBuf,
}

impl Context<'_> {
    fn remaining(&self) -> crate::Result<u64> {
        self.parent
            .ensure_alive()
            .map_err(|error| error.to_string())?;
        allowance(self.request.work_budget_seconds, self.started.elapsed())
    }

    fn persist(&self) -> crate::Result<()> {
        save(&self.receipt_path, &self.receipt)
    }

    fn execute(&mut self, pin: &Value, bundle: &Path, mut artifacts: Value) -> crate::Result<()> {
        let executable = std::env::current_exe().map_err(|error| error.to_string())?;
        let mut guard =
            bonsai_artifacts::open_locked_file(&executable).map_err(|error| error.to_string())?;
        let length = guard.metadata().map_err(|error| error.to_string())?.len();
        let digest =
            bonsai_artifacts::hash_reader(&mut guard, length, &mut vec![0; 65_536], &mut || {
                self.remaining()
                    .map(|_| ())
                    .map_err(|_| bonsai_artifacts::ArtifactError::Cancelled)
            })
            .map_err(|error| error.to_string())?;
        self.receipt["SupervisorSha256"] = json!(digest);
        self.persist()?;
        self.remaining()?;
        let native = crate::host_cpu::run(&json!({}))?;
        self.receipt["HostIdentity"] = native.clone();
        if native["status"] != "observed"
            || native["architecture"] != "aarch64"
            || !native["host_identity"]["processor_name"]
                .as_str()
                .is_some_and(|name| name.starts_with("Snapdragon") && name.contains(" X2"))
        {
            return Err(
                "Candidate startup requires observed native Snapdragon X2 identity.".into(),
            );
        }
        let mut probes = cases();
        let mut cpu = Value::Null;
        if self.request.model_smoke {
            self.remaining()?;
            cpu = crate::completion::cpu_check_plan(
                json!({"operation":"plan", "dataDirectory":self.request.data_directory,
                "profile":"swift-pq2-assessment", "useAbliterateLora":false, "useRepackedPtq1":false}),
            )?;
            let model: Value = serde_json::from_str(MODEL).map_err(|error| error.to_string())?;
            if model["runtime"]["commit"] != pin["source"]["base"]["commit"] {
                return Err("Candidate and model base revisions disagree.".into());
            }
            let probe = smoke_plan(&cpu, &model)?;
            artifacts
                .as_array_mut()
                .ok_or("Missing artifact list.")?
                .push(probe["Artifact"].clone());
            probes = vec![probe];
            self.receipt["ModelIdentity"] = model["model"].clone();
            self.receipt["TokenCap"] = json!(4);
            self.receipt["ThinkingRequestedEnabled"] = json!(false);
            self.receipt["EffectiveThinkingMode"] = json!("unqualified-template-behavior");
            self.receipt["ConfiguredMemoryPolicy"] = cpu["Policy"].clone();
            self.receipt["NumericalStateAdapterQualityAndPerformanceGates"] = json!("pending");
        } else {
            fs::write(
                self.request
                    .output_directory
                    .join("literal-wildcard-fixture.gguf"),
                "Synthetic argv sentinel; not a model.",
            )
            .map_err(|error| error.to_string())?;
        }
        self.receipt["Status"] = json!("probing");
        for probe in probes {
            self.probe(&probe, bundle, &artifacts, &cpu)?;
        }
        self.parent
            .ensure_alive()
            .map_err(|error| error.to_string())?;
        if self.started.elapsed().as_secs() >= self.request.work_budget_seconds.saturating_sub(10) {
            return Err("The complete startup allowance expired before reporting.".into());
        }
        self.receipt
            .as_object_mut()
            .ok_or("Missing receipt.")?
            .remove("PendingProbe");
        self.receipt["Status"] = json!("passed");
        Ok(())
    }

    fn probe(
        &mut self,
        probe: &Value,
        bundle: &Path,
        artifacts: &Value,
        cpu: &Value,
    ) -> crate::Result<()> {
        let model = self.request.model_smoke;
        let lane = self
            .request
            .output_directory
            .join(probe["Name"].as_str().ok_or("Missing probe name.")?);
        fs::create_dir(&lane).map_err(|error| error.to_string())?;
        self.receipt["PendingProbe"] = probe["Name"].clone();
        self.persist()?;
        let remaining = self.remaining()?;
        let result = crate::completion::run(json!({"schemaVersion":1,
            "executable":bundle.join(probe["Executable"].as_str().ok_or("Missing probe executable.")?),
            "arguments":probe["Arguments"], "workingDirectory":self.request.output_directory, "evidenceDirectory":lane,
            "artifacts":artifacts, "memoryLimitBytes":if model { cpu["Policy"]["ServiceCeilingBytes"].clone() } else { json!(536_870_912) },
            "requiredAvailableBytes":if model { cpu["Policy"]["RequiredAvailableBytes"].clone() } else { json!(536_870_912) },
            "physicalReserveBytes":if model { cpu["Policy"]["AvailableMemoryFloorBytes"].clone() } else { json!(0) },
            "timeoutSeconds":if model { smoke_deadline(remaining)? } else { 10 }, "totalTimeoutSeconds":remaining - 1,
            "stdoutLimitBytes":65_536, "stderrLimitBytes":65_536, "requireNonemptyStdout":model, "ciOnly":false,
            "parentProcessId":self.parent_id, "parentCreationTimeFileTime":self.parent_creation}))?;
        self.receipt["Probes"].as_array_mut().ok_or("Missing probes.")?.push(json!({"Name":probe["Name"], "Arguments":probe["Arguments"], "Result":result,
            "ModelLoaded":if model { Value::Null } else { json!(false) }, "InferenceVerification":if model { "pending" } else { "not-requested" }}));
        self.persist()?;
        if model {
            if result["Status"] != "completed" || result["ExitCode"] != 0 {
                return Err("Candidate smoke did not complete owned execution.".into());
            }
            self.receipt["InferenceObservation"] = observation(
                &read_output(&lane.join("stdout.txt"))?,
                &read_output(&lane.join("stderr.txt"))?,
            )?;
            let last = self.receipt["Probes"]
                .as_array_mut()
                .and_then(|probes| probes.last_mut())
                .ok_or("Missing probe record.")?;
            last["InferenceVerification"] = json!("passed");
            last["ModelLoaded"] = json!(true);
            self.receipt["ModelLoaded"] = json!(true);
        }
        Ok(())
    }
}

fn read_output(path: &Path) -> crate::Result<String> {
    let file = bonsai_artifacts::open_locked_file(path).map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    file.take(65_537)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > 65_536 {
        return Err("Candidate output exceeds its byte bound.".into());
    }
    String::from_utf8(bytes).map_err(|error| error.to_string())
}

pub(super) fn run(value: Value) -> crate::Result<Value> {
    let started = Instant::now();
    let request: Request = serde_json::from_value(value).map_err(|error| error.to_string())?;
    if !cfg!(target_arch = "aarch64")
        || !(45..=180).contains(&request.work_budget_seconds)
        || request.model_smoke != request.data_directory.is_some()
    {
        return Err(
            "Require native ARM64, 45..180 seconds and paired modelSmoke/dataDirectory options."
                .into(),
        );
    }
    bonsai_cpu_policy::server::validate_environment(
        std::env::vars_os().map(|(name, _)| name.to_string_lossy().into_owned()),
    )?;
    for name in [
        "ACTIONS_RUNTIME_TOKEN",
        "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
        "GITHUB_TOKEN",
        "GH_TOKEN",
        "ACTIONS_RUNNER_INPUT_TOKEN",
    ] {
        if std::env::var_os(name).is_some() {
            return Err("Candidate startup requires omitted CI credentials.".into());
        }
    }
    separated(&request.candidate_directory, &request.output_directory)?;
    if let Some(data) = &request.data_directory {
        external(data)?;
    }
    if request.output_directory.exists() {
        return Err("Use a fresh external startup evidence directory.".into());
    }
    let pin = pinned()?;
    let bundle = request.candidate_directory.join("bundle");
    let artifacts = inventory(&bundle, &pin["results"]["packageFiles"])?;
    let (id, creation) = crate::cpu_check::parent_identity(
        request.parent_process_id,
        request.parent_creation_time_file_time,
    )?;
    let parent = ParentProcess::open(id, creation).map_err(|error| error.to_string())?;
    parent.ensure_alive().map_err(|error| error.to_string())?;
    allowance(request.work_budget_seconds, started.elapsed())?;
    fs::create_dir(&request.output_directory).map_err(|error| error.to_string())?;
    let receipt_path = request.output_directory.join("startup.json");
    let receipt = json!({"SchemaVersion":2, "Status":"preparing",
        "Scope":if request.model_smoke { "x2-hosted-candidate-pq2-model-smoke-only" } else { "x2-hosted-candidate-model-free-startup-only" },
        "HostedRunId":pin["execution"]["runId"], "HostedArtifactSha256":pin["artifact"]["sha256"],
        "HostedEvidenceSha256":crate::identity::sha256(PIN.as_bytes()),
        "Source":pin["source"], "PackageFiles":pin["results"]["packageFiles"], "SupervisorSha256":null,
        "WorkerPolicy":"ordinary-required-headless", "WorkBudgetSeconds":request.work_budget_seconds,
        "StartedAtUtc":bonsai_windows_process::utc_now(), "RequestedProbes":if request.model_smoke { json!(["pq2-four-token-smoke"]) } else { json!(cases().iter().map(|probe| &probe["Name"]).collect::<Vec<_>>()) },
        "Probes":[], "ModelLoaded":if request.model_smoke { Value::Null } else { json!(false) },
        "ProductionRuntimePinChanged":false, "NumericalOrPerformanceQualified":false});
    let mut context = Context {
        request: &request,
        started,
        parent,
        parent_id: id,
        parent_creation: creation,
        receipt,
        receipt_path,
    };
    context.persist()?;
    let result = context.execute(&pin, &bundle, artifacts);
    if let Err(error) = &result {
        context.receipt["Status"] = json!("failed-or-pending");
        context.receipt["Failure"] = json!(error.chars().take(4096).collect::<String>());
    }
    context.receipt["ElapsedMilliseconds"] =
        json!(u64::try_from(started.elapsed().as_millis()).map_err(|error| error.to_string())?);
    context.persist()?;
    result.map_err(|error| {
        format!(
            "{error} Evidence retained at {}",
            request.output_directory.display()
        )
    })?;
    Ok(
        json!({"Status":context.receipt["Status"], "EvidenceDirectory":request.output_directory,
        "ModelLoaded":context.receipt["ModelLoaded"], "RuntimePinChanged":false}),
    )
}
