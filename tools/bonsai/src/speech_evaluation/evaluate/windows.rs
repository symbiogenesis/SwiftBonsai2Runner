//! Hosted matrix owner; each model remains inside the existing memory supervisor.

use bonsai_artifacts::{ArtifactError, hash_reader, open_locked_file, validate_external_path};
use bonsai_windows_credentials::PrivateCredentials;
use bonsai_windows_process::{OwnedProcess, ToolProcessSpec};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read as _, Write as _},
    net::{Ipv4Addr, TcpListener},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use super::super::http::{Call, exchange};
use super::{MODEL, Request, TEXT, VOICE, comparisons, ready, thread_grid, wave};
use crate::native_qualification::save_report as save;

fn external(path: &Path) -> crate::Result<()> {
    validate_external_path(path).map_err(|error| error.to_string())
}

fn publish(evidence: &Path) -> crate::Result<()> {
    let destination = PathBuf::from(
        std::env::var_os("GITHUB_ENV").ok_or("Expected hosted evidence publication.")?,
    );
    external(&destination)?;
    let path = evidence
        .to_str()
        .filter(|path| !path.contains(['\r', '\n']))
        .ok_or("Evidence path must be single-line Unicode.")?;
    let mut file = OpenOptions::new()
        .append(true)
        .open(destination)
        .map_err(|error| error.to_string())?;
    writeln!(file, "BONSAI_TTS_EVIDENCE={path}").map_err(|error| error.to_string())
}

struct Prepared<'a> {
    request: &'a Request,
    evidence: &'a Path,
    executable: PathBuf,
    executable_bytes: u64,
    executable_sha256: String,
    _executable_handle: File,
    credentials: PrivateCredentials,
    tool: PathBuf,
}

fn prepare<'a>(
    request: &'a Request,
    evidence: &'a Path,
    report: &mut Value,
) -> crate::Result<Prepared<'a>> {
    crate::startup_cancel::checkpoint()?;
    let executable = if let Some(path) = &request.tts_path {
        path.clone()
    } else {
        report["Phase"] = json!("native-build");
        save(&evidence.join("tts-evaluation.json"), report)?;
        let build = crate::native_startup::build(
            &request.repository_root,
            &request.data_directory,
            evidence,
            "QwenTts",
            false,
        )?;
        PathBuf::from(
            build["TtsPath"]
                .as_str()
                .ok_or("Native speech build omitted its TTS executable.")?,
        )
    };
    external(&executable)?;
    let mut file = open_locked_file(&executable).map_err(|error| error.to_string())?;
    crate::startup_cache::assert_arm64(&executable)?;
    let executable_bytes = file.metadata().map_err(|error| error.to_string())?.len();
    let executable_sha256 = hash_reader(
        &mut file,
        executable_bytes,
        &mut vec![0; 16_384],
        &mut || crate::startup_cancel::checkpoint().map_err(|_| ArtifactError::Cancelled),
    )
    .map_err(|error| error.to_string())?;
    report["Executable"] = json!({"Bytes": executable_bytes, "Sha256": executable_sha256});
    report["Phase"] = json!("model-provisioning");
    save(&evidence.join("tts-evaluation.json"), report)?;
    // Download and validate both arms before timing either fresh-process trial.
    for quantization in ["Q8_0", "Q4_K_M"] {
        crate::startup_cancel::checkpoint()?;
        crate::native_startup::models(
            &request.data_directory,
            "QwenTts",
            quantization,
            false,
            false,
            evidence,
        )?;
    }
    let credentials = PrivateCredentials::create(evidence).map_err(|error| error.to_string())?;
    let tool = std::env::current_exe().map_err(|error| error.to_string())?;
    external(&tool)?;
    Ok(Prepared {
        request,
        evidence,
        executable,
        executable_bytes,
        executable_sha256,
        _executable_handle: file,
        credentials,
        tool,
    })
}

fn session_request(
    prepared: &Prepared<'_>,
    trial: &Value,
    lane: &Path,
    port: u16,
) -> crate::Result<Value> {
    Ok(
        json!({"operation": "session", "dataDirectory": prepared.request.data_directory,
        "executable": prepared.executable, "executableBytes": prepared.executable_bytes,
        "executableSha256": prepared.executable_sha256, "evidenceDirectory": lane,
        "tokenFile": prepared.credentials.frontend_path(), "port": port,
        "threads": trial["Threads"], "quantization": trial["Quantization"],
        "parentProcessId": std::process::id(),
        "parentCreationTimeFileTime": bonsai_windows_process::current_process_creation_time().map_err(|error| error.to_string())?}),
    )
}

struct Supervisor {
    process: OwnedProcess,
    lane: PathBuf,
}

impl Supervisor {
    fn start(
        prepared: &Prepared<'_>,
        trial: &Value,
        lane: &Path,
        port: u16,
    ) -> crate::Result<(Self, Instant)> {
        let request = lane.join("session-request.json");
        save(&request, &session_request(prepared, trial, lane, port)?)?;
        let spec = ToolProcessSpec {
            executable: prepared.tool.clone(),
            arguments: vec![
                "speech-evaluation".into(),
                "--request".into(),
                request.into_os_string(),
            ],
            working_directory: lane.into(),
            stdout: lane.join("supervisor.stdout"),
            stderr: lane.join("supervisor.stderr"),
            environment: BTreeMap::new(),
        };
        let cold = Instant::now();
        let process = OwnedProcess::start_tool(&spec).map_err(|error| error.to_string())?;
        Ok((
            Self {
                process,
                lane: lane.into(),
            },
            cold,
        ))
    }
    fn checkpoint(&self) -> crate::Result<()> {
        crate::startup_cancel::checkpoint()?;
        if self
            .process
            .try_wait()
            .map_err(|error| error.to_string())?
            .is_some()
        {
            return Err("Native TTS evaluation supervisor exited during a trial.".into());
        }
        for name in ["supervisor.stdout", "supervisor.stderr"] {
            if fs::metadata(self.lane.join(name))
                .map_err(|error| error.to_string())?
                .len()
                > 1_048_576
            {
                return Err("TTS supervisor diagnostics exceeded their bound.".into());
            }
        }
        Ok(())
    }
    fn finish(&self, complete: bool) -> crate::Result<Value> {
        let signal = fs::write(self.lane.join(if complete { "stop" } else { "abort" }), [])
            .map_err(|error| error.to_string());
        let waited = self
            .process
            .wait_for_exit(Duration::from_secs(20))
            .map_err(|error| error.to_string());
        let stopped = self
            .process
            .stop(Duration::from_secs(10))
            .map_err(|error| error.to_string());
        signal?;
        let exit = waited?;
        stopped?;
        if complete && exit != Some(0) {
            return Err("TTS supervisor did not complete its original cleanup allowance.".into());
        }
        let memory = read_memory(&self.lane.join("memory.json"))?;
        if complete && memory["Status"] != "completed" {
            return Err("TTS memory supervision rejected the trial; inspect its evidence.".into());
        }
        Ok(memory)
    }
}

fn read_memory(path: &Path) -> crate::Result<Value> {
    let mut bytes = Vec::new();
    let file = open_locked_file(path).map_err(|error| error.to_string())?;
    file.take(1_048_577)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > 1_048_576 {
        return Err("TTS memory receipt exceeded its byte bound.".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "Invalid TTS memory receipt.".into())
}

fn wait_ready(
    supervisor: &Supervisor,
    prepared: &Prepared<'_>,
    port: u16,
    cold: Instant,
) -> crate::Result<Duration> {
    let deadline = cold + Duration::from_secs(300);
    loop {
        supervisor.checkpoint()?;
        if Instant::now() >= deadline {
            return Err("The verified native TTS companion did not become ready.".into());
        }
        let call = Call {
            port,
            token: prepared.credentials.frontend_key().as_bytes(),
            body: None,
            media: "application/json",
            maximum: 65_536,
            deadline,
        };
        if let Ok(response) = exchange(&call, || supervisor.checkpoint()) {
            let health: Value = serde_json::from_slice(&response.bytes)
                .map_err(|_| "Invalid TTS readiness JSON.")?;
            if ready(&health) {
                return Ok(cold.elapsed());
            }
        }
        supervisor.checkpoint()?;
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn synthesis(
    prepared: &Prepared<'_>,
    supervisor: &Supervisor,
    port: u16,
    phase: &str,
) -> crate::Result<Value> {
    supervisor.checkpoint()?;
    fs::write(supervisor.lane.join(format!("phase-{phase}")), [])
        .map_err(|error| error.to_string())?;
    let format = prepared.request.response_format;
    let payload = serde_json::to_vec(&json!({"model": MODEL, "input": TEXT,
        "voice_description": VOICE, "language": "Auto", "response_format": format.name()}))
    .map_err(|error| error.to_string())?;
    let call = Call {
        port,
        token: prepared.credentials.frontend_key().as_bytes(),
        body: Some(&payload),
        media: format.media(),
        maximum: format.limit(),
        deadline: Instant::now() + Duration::from_secs(190),
    };
    let response = exchange(&call, || supervisor.checkpoint())?;
    let bytes = wave(format, &response.bytes)?;
    let seconds =
        f64::from(u32::try_from(bytes.len() - 44).map_err(|error| error.to_string())?) / 48_000.0;
    let clip = format!("{phase}.wav");
    fs::write(supervisor.lane.join(&clip), bytes).map_err(|error| error.to_string())?;
    Ok(
        json!({"Phase": phase, "FirstBodyMs": response.first_body.as_secs_f64() * 1000.0,
        "InferenceMs": response.elapsed.as_secs_f64() * 1000.0, "AudioSeconds": seconds,
        "RealTimeFactor": response.elapsed.as_secs_f64() / seconds, "CompletePcm16": true, "Clip": clip}),
    )
}

fn capture(
    prepared: &Prepared<'_>,
    trial: &Value,
    supervisor: &Supervisor,
    port: u16,
    cold: Instant,
    session: &mut Value,
    report: &mut Value,
) -> crate::Result<()> {
    let ready = wait_ready(supervisor, prepared, port, cold)?;
    for phase in ["bootstrap", "warm"] {
        session["CurrentPhase"] = json!(phase);
        let mut record = synthesis(prepared, supervisor, port, phase)?;
        for field in ["Quantization", "Threads", "PairPosition"] {
            record[field] = trial[field].clone();
        }
        record["Iteration"] = trial["Repetition"].clone();
        record["Session"] = trial["Id"].clone();
        record["ColdReadyMs"] = json!(ready.as_secs_f64() * 1000.0);
        record["Clip"] = json!(format!(
            "{}/{phase}.wav",
            trial["Id"].as_str().ok_or("Missing trial identity.")?
        ));
        session["CompletedPhases"]
            .as_array_mut()
            .ok_or("Missing completed phases.")?
            .push(json!(phase));
        report["Records"]
            .as_array_mut()
            .ok_or("Missing records.")?
            .push(record);
        let last = report["Sessions"]
            .as_array_mut()
            .and_then(|sessions| sessions.last_mut())
            .ok_or("Missing active session.")?;
        *last = session.clone();
        save(&prepared.evidence.join("tts-evaluation.json"), report)?;
    }
    Ok(())
}

fn trial(prepared: &Prepared<'_>, trial: &Value, report: &mut Value) -> crate::Result<()> {
    crate::startup_cancel::checkpoint()?;
    let id = trial["Id"]
        .as_str()
        .ok_or("Missing fixed TTS trial identity.")?;
    let lane = prepared.evidence.join(id);
    fs::create_dir(&lane).map_err(|error| error.to_string())?;
    let listener =
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|error| error.to_string())?;
    let port = listener
        .local_addr()
        .map_err(|error| error.to_string())?
        .port();
    drop(listener);
    let mut session = json!({"Id": id, "Quantization": trial["Quantization"], "Threads": trial["Threads"],
        "Repetition": trial["Repetition"], "PairPosition": trial["PairPosition"], "Status": "running", "Memory": null,
        "CurrentPhase": "startup", "CompletedPhases": [], "FailedPhase": null, "Failure": null});
    report["Sessions"]
        .as_array_mut()
        .ok_or("Missing session collection.")?
        .push(session.clone());
    save(&prepared.evidence.join("tts-evaluation.json"), report)?;
    let result = match Supervisor::start(prepared, trial, &lane, port) {
        Ok((supervisor, cold)) => {
            let captured = capture(
                prepared,
                trial,
                &supervisor,
                port,
                cold,
                &mut session,
                report,
            );
            let finished = supervisor.finish(captured.is_ok());
            // Retain failed memory observations even when cleanup itself rejects.
            if let Ok(memory) = read_memory(&lane.join("memory.json")) {
                session["Memory"] = memory;
            }
            let finished = finished.map(|memory| {
                session["Memory"] = memory;
            });
            captured.and(finished)
        }
        Err(error) => Err(error),
    };
    session["Status"] = json!(if result.is_ok() {
        "completed"
    } else {
        "failed"
    });
    if let Err(error) = &result {
        session["FailedPhase"] = session["CurrentPhase"].clone();
        session["Failure"] = json!(error);
    }
    let last = report["Sessions"]
        .as_array_mut()
        .and_then(|sessions| sessions.last_mut())
        .ok_or("Missing active session.")?;
    *last = session;
    save(&prepared.evidence.join("tts-evaluation.json"), report)?;
    result
}

fn execute(
    request: &Request,
    evidence: &Path,
    threads: &[usize],
    report: &mut Value,
) -> crate::Result<()> {
    let host = bonsai_windows_process::native_build_host().map_err(|error| error.to_string())?;
    report["Cpu"] = json!(host.processor_name);
    report["OS"] = json!(host.windows_version);
    let prepared = prepare(request, evidence, report)?;
    report["Status"] = json!("running");
    report["Phase"] = json!("paired-trials");
    let schedule = report["Plan"]["Schedule"]
        .as_array()
        .ok_or("Missing paired schedule.")?
        .clone();
    for item in &schedule {
        trial(&prepared, item, report)?;
    }
    report["Comparisons"] = json!(comparisons(
        report["Sessions"].as_array().ok_or("Missing sessions.")?,
        threads,
        request.repetitions
    )?);
    crate::startup_cancel::checkpoint()
}

pub(super) fn run(request: &Request) -> crate::Result<Value> {
    // This gate deliberately precedes planning, paths, provisioning and evidence.
    super::super::validate_context(std::env::consts::ARCH, |key| std::env::var(key).ok())?;
    let available = std::thread::available_parallelism()
        .map_err(|error| error.to_string())?
        .get();
    let threads = thread_grid(available)?;
    let plan = super::super::plan(&threads, request.repetitions)?;
    crate::startup_cancel::checkpoint()?;
    external(&request.data_directory)?;
    let workspace = PathBuf::from(
        std::env::var_os("GITHUB_WORKSPACE").ok_or("Expected the hosted source checkout.")?,
    );
    if !request.repository_root.is_absolute()
        || fs::canonicalize(&request.repository_root).map_err(|error| error.to_string())?
            != fs::canonicalize(workspace).map_err(|error| error.to_string())?
    {
        return Err("TTS builds require the current hosted source checkout.".into());
    }
    let evidence = match &request.evidence_directory {
        Some(path) => path.clone(),
        None => std::env::temp_dir().join(format!(
            "Bonsai-TtsEvaluation-{}",
            crate::native_startup::nonce()?
        )),
    };
    external(&evidence)?;
    fs::create_dir(&evidence).map_err(|error| error.to_string())?;
    let path = evidence.join("tts-evaluation.json");
    let mut report = json!({"SchemaVersion": 2, "Revision": std::env::var("GITHUB_SHA").ok(),
        "Status": "preparing", "Phase": "preparation", "QualityVerdict": "pending-human-listening",
        "Cpu": null, "OS": null, "LogicalProcessors": available, "Architecture": "Arm64",
        "Runner": std::env::var("RUNNER_NAME").ok(), "ResponseFormat": request.response_format.name(),
        "Repetitions": request.repetitions, "Plan": plan, "Records": [], "Sessions": [], "Comparisons": [],
        "ColdReadyDefinition": "Supervisor spawn, verified artifact hashing and new model process through readiness; operating-system file cache is not flushed.",
        "Sampling": "Fixed design reference phrase and seed; reusable Base speaker conditioning; upstream text sampling defaults.",
        "Text": TEXT, "VoiceDescription": VOICE });
    save(&path, &report)?;
    let result =
        publish(&evidence).and_then(|()| execute(request, &evidence, &threads, &mut report));
    report["Status"] = json!(if result.is_ok() {
        "complete"
    } else {
        "incomplete"
    });
    if let Err(error) = &result {
        report["Failure"] = json!(error);
    }
    save(&path, &report)?;
    result?;
    Ok(
        json!({"ReportPath": path, "QualityVerdict": "pending-human-listening",
        "Records": report["Records"].as_array().ok_or("Missing final records.")?.len()}),
    )
}
