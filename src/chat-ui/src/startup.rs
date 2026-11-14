//! Cancellable native preparation while the existing chat window is available.
#![expect(
    clippy::redundant_pub_crate,
    reason = "Keep the private preparation worker visible only within the native application."
)]
use bonsai_windows_process::{OwnedProcess, ToolProcessSpec};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

mod progress;
pub(crate) use progress::{Activity as SetupActivity, Kind as SetupKind, Status as SetupStatus};

const MAX_OUTPUT: u64 = 1024 * 1024;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Preparation {
    Combined,
    Staged,
}

#[derive(Clone)]
pub(crate) struct Request {
    executable: PathBuf,
    payload: PathBuf,
    working: PathBuf,
    pub(crate) data: PathBuf,
    pub(crate) canary_configured: bool,
    pub(crate) tts_configured: bool,
    preparation: Preparation,
    pub(crate) portable_update: bool,
}

#[derive(Default, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RequestedSpeech {
    #[serde(default)]
    canary_key_file: Option<String>,
    #[serde(default)]
    tts_key_file: Option<String>,
    #[serde(default)]
    no_tts: bool,
    #[serde(default)]
    no_speech_models: bool,
    #[serde(default)]
    speech_models_only: bool,
    #[serde(default)]
    preparation_stage: Option<String>,
    #[serde(default)]
    portable_executable: Option<PathBuf>,
}

impl Request {
    pub(crate) fn update_tool(&self) -> &Path {
        &self.executable
    }

    pub(crate) fn update_handoff_path(&self) -> PathBuf {
        self.payload.with_extension("update.json")
    }

    pub(crate) fn parse(arguments: &[OsString]) -> io::Result<Self> {
        if arguments.len() != 6
            || arguments[0] != "--startup-tool"
            || arguments[2] != "--startup-request"
            || arguments[4] != "--startup-data-directory"
        {
            return Err(io::Error::other(
                "Use --startup-tool ABSOLUTE_EXE --startup-request ABSOLUTE_JSON --startup-data-directory ABSOLUTE_DIRECTORY.",
            ));
        }
        let executable = PathBuf::from(&arguments[1]);
        let request = PathBuf::from(&arguments[3]);
        for path in [&executable, &request] {
            bonsai_windows_credentials::validate_external_path(path)?;
            if !path.is_file() {
                return Err(io::Error::other(
                    "Native startup requires existing external tool and request files.",
                ));
            }
        }
        let payload = bounded_text(&request)?;
        let speech: RequestedSpeech = serde_json::from_str(&payload).map_err(io::Error::other)?;
        // New launchers advertise the stage contract in their serialized request.
        // An older maintenance executable can still build this UI from source;
        // retain its combined preparation protocol until it is rebuilt too.
        let preparation = if speech.preparation_stage.as_deref() == Some("all") {
            Preparation::Staged
        } else {
            Preparation::Combined
        };
        let portable_update = speech.portable_executable.as_deref() == Some(executable.as_path());
        let has_canary_key = speech
            .canary_key_file
            .as_deref()
            .is_some_and(|value| !value.is_empty());
        let has_tts_key = speech
            .tts_key_file
            .as_deref()
            .is_some_and(|value| !value.is_empty());
        let canary_configured =
            !speech.speech_models_only && (!speech.no_speech_models || has_canary_key);
        let tts_configured = !speech.speech_models_only
            && ((!speech.no_speech_models && !speech.no_tts) || has_tts_key);
        let working = request
            .parent()
            .ok_or_else(|| io::Error::other("Startup request needs an external parent directory."))?
            .to_owned();
        let data = PathBuf::from(&arguments[5]);
        bonsai_windows_credentials::validate_external_path(&data)?;
        Ok(Self {
            executable,
            payload: request,
            working,
            data,
            canary_configured,
            tts_configured,
            preparation,
            portable_update,
        })
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
struct Prepared {
    arguments: Vec<String>,
}

fn bounded_text(path: &Path) -> io::Result<String> {
    crate::bounded_file::text(
        path,
        MAX_OUTPUT,
        "Native startup output exceeded its bound.",
    )
}

fn configuration(output: &str) -> io::Result<crate::client::Config> {
    let prepared: Prepared = serde_json::from_str(output).map_err(io::Error::other)?;
    if prepared.arguments.len() > 64
        || prepared
            .arguments
            .iter()
            .any(|argument| argument.len() > 32768)
    {
        return Err(io::Error::other(
            "Native startup arguments exceeded their bound.",
        ));
    }
    crate::configuration(prepared.arguments.into_iter().map(OsString::from))
        .map_err(io::Error::other)
}

struct Process(OwnedProcess);

impl Drop for Process {
    fn drop(&mut self) {
        let _stopped = self.0.stop(Duration::from_secs(10));
    }
}

fn execute(
    request: &Request,
    stage: &str,
    broker: Option<&Path>,
    cancelled: &AtomicBool,
    progress: &progress::Shared,
) -> io::Result<crate::client::Config> {
    let mut payload: serde_json::Value =
        serde_json::from_str(&bounded_text(&request.payload)?).map_err(io::Error::other)?;
    let object = payload
        .as_object_mut()
        .ok_or_else(|| io::Error::other("Native startup requires an object."))?;
    object.insert(
        "WorkingDirectory".into(),
        serde_json::json!(request.working),
    );
    if stage != "all" {
        object.insert("PreparationStage".into(), stage.into());
    }
    if let Some(broker) = broker {
        object.insert("BrokerPath".into(), serde_json::json!(broker));
    }
    let path = request.working.join(format!("chat-prepare-{stage}.json"));
    bonsai_windows_credentials::validate_external_path(&path)?;
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    serde_json::to_writer(file, &payload).map_err(io::Error::other)?;
    let stdout = request.working.join(format!("chat-prepare-{stage}.stdout"));
    let stderr = request.working.join(format!("chat-prepare-{stage}.stderr"));
    let arguments = vec![
        "chat-prepare".into(),
        "--request".into(),
        path.clone().into_os_string(),
    ];
    let output = capture(
        &ToolProcessSpec {
            executable: request.executable.clone(),
            arguments,
            working_directory: request.working.clone(),
            stdout,
            stderr,
            environment: BTreeMap::new(),
        },
        cancelled,
        // Each download/build retains its existing deadline. In particular,
        // cold multi-gigabyte speech provisioning can legitimately exceed an
        // hour overall; only cancellation bounds the aggregate operation.
        Duration::ZERO,
        stage == "speech",
        Some((request, progress)),
    );
    // The original request remains the replay input; stage requests are scratch.
    let cleanup = fs::remove_file(path);
    let output = output?;
    cleanup?;
    let config = configuration(&output)?;
    if config.data != request.data {
        return Err(io::Error::other(
            "Native preparation returned a different data directory.",
        ));
    }
    Ok(config)
}

fn capture(
    spec: &ToolProcessSpec,
    cancelled: &AtomicBool,
    deadline: Duration,
    background: bool,
    progress: Option<(&Request, &progress::Shared)>,
) -> io::Result<String> {
    if cancelled.load(Ordering::Acquire) {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Native startup was cancelled.",
        ));
    }
    // Speech setup can compile native libraries while Swift verifies/loads.
    // Lower its complete owned Job's CPU priority so compiler descendants also
    // yield to model startup and text requests on smaller Snapdragon systems.
    let child = Process(if background {
        OwnedProcess::start_background_tool(spec)?
    } else {
        OwnedProcess::start_tool(spec)?
    });
    let started = Instant::now();
    let mut observed = Instant::now();
    let outcome = (|| loop {
        if cancelled.load(Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Native startup was cancelled.",
            ));
        }
        if fs::metadata(&spec.stdout)?.len() > MAX_OUTPUT
            || fs::metadata(&spec.stderr)?.len() > MAX_OUTPUT
        {
            return Err(io::Error::other(
                "Native startup output exceeded its bound.",
            ));
        }
        if let Some(status) = child.0.try_wait()? {
            break Ok(status);
        }
        if observed.elapsed() >= Duration::from_millis(250) {
            if let Some((request, progress)) = progress {
                progress.observe(&request.working, &request.data);
            }
            observed = Instant::now();
        }
        if !deadline.is_zero() && started.elapsed() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Native startup timed out.",
            ));
        }
        thread::sleep(Duration::from_millis(20));
    })();
    child.0.stop(Duration::from_secs(10)).map_err(|error| {
        io::Error::other(format!(
            "Setup process cleanup could not be confirmed. Close Swift before trying again. {error}"
        ))
    })?;
    let status = outcome?;
    if status != 0 {
        return Err(preparation_failure(
            status,
            bounded_text(&spec.stderr),
            &spec.working_directory,
        ));
    }
    bounded_text(&spec.stdout)
}

fn preparation_failure(
    status: u32,
    diagnostic: io::Result<String>,
    working_directory: &Path,
) -> io::Error {
    // Diagnostic intake must never erase the observed process exit. In
    // particular, Windows loader failures may produce no stderr at all.
    // Stdout contains the preparation protocol, including credential paths;
    // it is deliberately not used as a fallback diagnostic.
    let detail = match diagnostic {
        Ok(text) if !text.trim().is_empty() => text.trim().to_owned(),
        Ok(_) => "The preparation process wrote no standard-error diagnostics.".to_owned(),
        Err(error) => format!(
            "The preparation diagnostics could not be read ({:?}).",
            error.kind()
        ),
    };
    io::Error::other(format!(
        "Setup could not finish (process exited {status}, 0x{status:08X}). {detail} Diagnostics: {}",
        working_directory.display()
    ))
}

pub(crate) struct Session {
    incoming: Receiver<io::Result<Progress>>,
    cancelled: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    progress: progress::Shared,
}

pub(crate) enum Progress {
    Model {
        config: crate::client::Config,
        speech_pending: bool,
    },
    Speech(io::Result<crate::client::Config>),
}

fn prepare_stages(
    request: &Request,
    outgoing: &mpsc::SyncSender<io::Result<Progress>>,
    cancelled: &AtomicBool,
    mut prepare: impl FnMut(&str, Option<&Path>) -> io::Result<crate::client::Config>,
) {
    let staged = request.preparation == Preparation::Staged;
    let config = match prepare(if staged { "model" } else { "all" }, None) {
        Ok(config) => config,
        Err(error) => {
            let _sent = outgoing.send(Err(error));
            return;
        }
    };
    if cancelled.load(Ordering::Acquire) {
        let _sent = outgoing.send(Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Native startup was cancelled.",
        )));
        return;
    }
    let broker = config.broker.clone();
    let speech_pending = staged && (request.canary_configured || request.tts_configured);
    if outgoing
        .send(Ok(Progress::Model {
            config,
            speech_pending,
        }))
        .is_err()
        || !speech_pending
    {
        return;
    }
    if cancelled.load(Ordering::Acquire) {
        let _sent = outgoing.send(Ok(Progress::Speech(Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Native startup was cancelled.",
        )))));
        return;
    }
    let _sent = outgoing.send(Ok(Progress::Speech(prepare("speech", Some(&broker)))));
}

impl Session {
    #[cfg(test)]
    pub(crate) fn fixture() -> (Self, mpsc::SyncSender<io::Result<Progress>>) {
        let (outgoing, incoming) = mpsc::sync_channel(2);
        (
            Self {
                incoming,
                cancelled: Arc::new(AtomicBool::new(false)),
                worker: None,
                progress: progress::Shared::default(),
            },
            outgoing,
        )
    }

    pub(crate) fn start(request: Request) -> io::Result<Self> {
        Self::start_mode(request, None)
    }

    pub(crate) fn start_speech(request: Request, broker: PathBuf) -> io::Result<Self> {
        Self::start_mode(request, Some(broker))
    }

    fn start_mode(mut request: Request, broker: Option<PathBuf>) -> io::Result<Self> {
        // Exactly two results can be produced. Neither send can block teardown
        // when the window closes before consuming the model handoff.
        let (outgoing, incoming) = mpsc::sync_channel(2);
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancellation = Arc::clone(&cancelled);
        let progress = progress::Shared::default();
        let observation = progress.clone();
        let worker = thread::Builder::new()
            .name("swift-chat-prepare".into())
            .spawn(move || {
                let attempt = (|| {
                    let nonce = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_err(io::Error::other)?
                        .as_nanos();
                    request.working = request
                        .working
                        .join(format!("setup-{}-{nonce}", std::process::id()));
                    bonsai_windows_credentials::validate_external_path(&request.working)?;
                    fs::create_dir(&request.working)
                })();
                if let Err(error) = attempt {
                    let _sent = outgoing.send(Err(error));
                    return;
                }
                if let Some(broker) = broker {
                    let result = execute(
                        &request,
                        "speech",
                        Some(&broker),
                        &cancellation,
                        &observation,
                    );
                    let _sent = outgoing.send(Ok(Progress::Speech(result)));
                    return;
                }
                prepare_stages(&request, &outgoing, &cancellation, |stage, broker| {
                    execute(&request, stage, broker, &cancellation, &observation)
                });
            })?;
        Ok(Self {
            incoming,
            cancelled,
            worker: Some(worker),
            progress,
        })
    }

    pub(crate) fn progress(&self) -> Option<SetupStatus> {
        self.progress.latest()
    }

    pub(crate) fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub(crate) fn poll(&self) -> Option<io::Result<Progress>> {
        match self.incoming.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(io::Error::other(
                "Setup process cleanup could not be confirmed. The preparation worker stopped unexpectedly. Close Swift before trying again.",
            ))),
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _joined = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preparation_failure_preserves_unsigned_exit_with_empty_or_unreadable_stderr() {
        let directory = Path::new("C:\\startup-diagnostics");
        for diagnostic in [
            Ok(String::new()),
            Ok(" \r\n\t ".to_owned()),
            Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "private diagnostic read detail",
            )),
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "private diagnostic read detail",
            )),
        ] {
            let readable = diagnostic.is_ok();
            let error = preparation_failure(0xC000_0142, diagnostic, directory);
            let message = error.to_string();
            assert!(message.contains("process exited 3221225794, 0xC0000142"));
            assert!(message.contains("Diagnostics: C:\\startup-diagnostics"));
            assert!(!message.contains("private diagnostic read detail"));
            if readable {
                assert!(message.contains("wrote no standard-error diagnostics"));
            } else {
                assert!(message.contains("diagnostics could not be read"));
            }
        }
        let error = preparation_failure(7, Ok("\r\nSynthetic failure\r\n".into()), directory);
        assert_eq!(
            error.to_string(),
            "Setup could not finish (process exited 7, 0x00000007). Synthetic failure Diagnostics: C:\\startup-diagnostics"
        );
    }

    #[test]
    fn unexpected_setup_worker_exit_requires_close_before_retry() -> io::Result<()> {
        let (session, outgoing) = Session::fixture();
        drop(outgoing);
        let error = session
            .poll()
            .ok_or_else(|| io::Error::other("Missing worker failure"))?
            .err()
            .ok_or_else(|| io::Error::other("Unexpected setup success"))?;
        assert!(
            error
                .to_string()
                .contains("Setup process cleanup could not be confirmed.")
        );
        Ok(())
    }

    fn model_config() -> crate::client::Config {
        let data = std::env::temp_dir();
        crate::client::Config {
            broker: data.join("broker.exe"),
            data: data.clone(),
            evidence: data,
            frontend_port: 5173,
            backend_port: 8081,
            use_abliterate_lora: false,
            use_repacked_ptq1: false,
            native_engine: true,
            native_row_workers: 4,
            latest_prompt_reuse: false,
            kv_cache: None,
            placement: None,
            canary: None,
            qwen_tts: None,
        }
    }

    #[test]
    fn model_handoff_precedes_speech_completion_and_keeps_speech_failure_separate() -> io::Result<()>
    {
        let config = model_config();
        let broker = config.broker.clone();
        let request = Request {
            executable: config.data.join("tool.exe"),
            payload: config.data.join("request.json"),
            working: config.data.clone(),
            data: config.data.clone(),
            canary_configured: true,
            tts_configured: true,
            preparation: Preparation::Staged,
            portable_update: false,
        };
        let (outgoing, incoming) = mpsc::sync_channel(2);
        let (release, gate) = mpsc::channel();
        let worker = thread::spawn(move || {
            prepare_stages(
                &request,
                &outgoing,
                &AtomicBool::new(false),
                |stage, path| {
                    if stage == "model" {
                        assert!(path.is_none());
                        return Ok(config.clone());
                    }
                    assert_eq!(stage, "speech");
                    assert_eq!(path, Some(broker.as_path()));
                    gate.recv_timeout(Duration::from_secs(2))
                        .map_err(io::Error::other)?;
                    Err(io::Error::other("Synthetic speech failure"))
                },
            );
        });
        assert!(matches!(
            incoming.recv_timeout(Duration::from_secs(2)),
            Ok(Ok(Progress::Model {
                speech_pending: true,
                ..
            }))
        ));
        assert!(incoming.try_recv().is_err());
        release.send(()).map_err(io::Error::other)?;
        assert!(matches!(
            incoming.recv_timeout(Duration::from_secs(2)),
            Ok(Ok(Progress::Speech(Err(error)))) if error.to_string() == "Synthetic speech failure"
        ));
        worker
            .join()
            .map_err(|_| io::Error::other("Preparation fixture panicked"))?;
        Ok(())
    }

    #[test]
    fn staged_preparation_finishes_with_both_results_unread_and_skips_cancelled_speech() {
        let config = model_config();
        let mut request = Request {
            executable: config.data.join("tool.exe"),
            payload: config.data.join("request.json"),
            working: config.data.clone(),
            data: config.data.clone(),
            canary_configured: true,
            tts_configured: true,
            preparation: Preparation::Staged,
            portable_update: false,
        };
        for (cancelled, staged, speech) in [
            (false, true, true),
            (true, true, true),
            (false, false, true),
            (false, true, false),
        ] {
            request.preparation = if staged {
                Preparation::Staged
            } else {
                Preparation::Combined
            };
            request.canary_configured = speech;
            request.tts_configured = speech;
            let (outgoing, incoming) = mpsc::sync_channel(2);
            let mut stages = Vec::new();
            prepare_stages(
                &request,
                &outgoing,
                &AtomicBool::new(cancelled),
                |stage, _| {
                    stages.push(stage.to_owned());
                    Ok(config.clone())
                },
            );
            let prepares_speech = !cancelled && staged && speech;
            assert_eq!(stages.len(), if prepares_speech { 2 } else { 1 });
            assert_eq!(stages[0], if staged { "model" } else { "all" });
            let first = incoming.try_recv();
            if cancelled {
                assert!(
                    matches!(first, Ok(Err(error)) if error.kind() == io::ErrorKind::Interrupted)
                );
            } else {
                assert!(matches!(first, Ok(Ok(Progress::Model { .. }))));
            }
            assert_eq!(incoming.try_recv().is_ok(), prepares_speech);
        }
    }

    #[test]
    fn prepared_configuration_rejects_unknown_fields_and_unbounded_arguments() {
        assert!(configuration(r#"{"Arguments":[],"Extra":true}"#).is_err());
        assert!(
            configuration(&serde_json::json!({"Arguments": vec!["x";65]}).to_string()).is_err()
        );
        assert!(
            configuration(&serde_json::json!({"Arguments":["x".repeat(32769)]}).to_string())
                .is_err()
        );
    }

    fn scratch(name: &str) -> io::Result<PathBuf> {
        let directory = std::env::temp_dir().join(format!(
            "Bonsai-Startup-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(io::Error::other)?
                .as_nanos()
        ));
        fs::create_dir(&directory)?;
        Ok(directory)
    }

    fn quoted(path: &Path) -> String {
        path.to_string_lossy().replace('\'', "''")
    }

    fn powershell() -> io::Result<PathBuf> {
        let search =
            std::env::var_os("PATH").ok_or_else(|| io::Error::other("PATH unavailable."))?;
        std::env::split_paths(&search)
            .map(|directory| directory.join("pwsh.exe"))
            .find(|path| path.is_absolute() && path.is_file())
            .ok_or_else(|| {
                io::Error::other(
                    "PowerShell 7 is required for the synthetic startup process fixture.",
                )
            })
    }

    fn spec(directory: &Path, script: &str) -> io::Result<ToolProcessSpec> {
        let path = directory.join("prepare.ps1");
        fs::write(&path, script)?;
        Ok(ToolProcessSpec {
            executable: powershell()?,
            arguments: vec![
                "-NoLogo".into(),
                "-NoProfile".into(),
                "-NonInteractive".into(),
                "-File".into(),
                path.into_os_string(),
            ],
            working_directory: directory.into(),
            stdout: directory.join("prepare.stdout"),
            stderr: directory.join("prepare.stderr"),
            environment: BTreeMap::new(),
        })
    }

    #[test]
    fn cancelled_startup_stops_native_descendants_before_returning() -> io::Result<()> {
        let directory = scratch("Cancel")?;
        let marker = directory.join("descendant.txt");
        let leaf = directory.join("leaf.ps1");
        fs::write(
            &leaf,
            format!(
                "$process = Get-Process -Id $PID\n[IO.File]::WriteAllText('{marker}.partial', \"$PID`n$($process.StartTime.ToUniversalTime().ToFileTimeUtc())\")\n[IO.File]::Move('{marker}.partial', '{marker}')\nStart-Sleep -Seconds 60\n",
                marker = quoted(&marker)
            ),
        )?;
        let script = format!(
            "$ErrorActionPreference = 'Stop'\n$start = [Diagnostics.ProcessStartInfo]::new('{executable}')\n$start.UseShellExecute = $false\n$start.CreateNoWindow = $true\n$start.WorkingDirectory = '{directory}'\nforeach ($argument in @('-NoLogo', '-NoProfile', '-NonInteractive', '-File', '{leaf}')) {{ $start.ArgumentList.Add($argument) }}\n$child = [Diagnostics.Process]::Start($start)\n$child.WaitForExit()\n",
            executable = quoted(&powershell()?),
            directory = quoted(&directory),
            leaf = quoted(&leaf)
        );
        let spec = spec(&directory, &script)?;
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancellation = Arc::clone(&cancelled);
        let worker = thread::spawn(move || {
            capture(&spec, &cancellation, Duration::from_secs(30), false, None)
        });
        let started = Instant::now();
        while !marker.is_file() && started.elapsed() < Duration::from_secs(20) {
            thread::sleep(Duration::from_millis(20));
        }
        if !marker.is_file() {
            cancelled.store(true, Ordering::Release);
            let _joined = worker.join();
            return Err(io::Error::other(
                "Synthetic startup descendant did not start.",
            ));
        }
        let identity = fs::read_to_string(&marker)?;
        let (id, creation) = identity
            .split_once('\n')
            .ok_or_else(|| io::Error::other("Incomplete descendant identity."))?;
        let child = bonsai_windows_process::ParentProcess::open(
            id.trim().parse().map_err(io::Error::other)?,
            creation.trim().parse().map_err(io::Error::other)?,
        )?;
        cancelled.store(true, Ordering::Release);
        let result = worker
            .join()
            .map_err(|_| io::Error::other("Synthetic startup worker panicked."))?;
        assert!(result.is_err_and(|error| error.kind() == io::ErrorKind::Interrupted));
        assert!(
            child.ensure_alive().is_err(),
            "Startup descendant survived cancellation."
        );
        drop(child);
        fs::remove_dir_all(&directory)?;
        Ok(())
    }

    #[test]
    fn native_startup_bounds_output_and_reports_preparation_failure() -> io::Result<()> {
        for (name, script, expected) in [
            (
                "Failure",
                "[Console]::Error.WriteLine('Synthetic failure'); exit 7",
                "Synthetic failure",
            ),
            (
                "Bound",
                "[Console]::Write('x' * 1048577); Start-Sleep -Seconds 10",
                "exceeded its bound",
            ),
        ] {
            let directory = scratch(name)?;
            let result = capture(
                &spec(&directory, script)?,
                &AtomicBool::new(false),
                Duration::from_secs(30),
                false,
                None,
            );
            assert!(result.is_err_and(|error| error.to_string().contains(expected)));
            fs::remove_dir_all(&directory)?;
        }
        Ok(())
    }
}
