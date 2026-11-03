//! One explicit local timing owner; no automatic retries or runtime fallback.
//! Native historical file pins are caller-reviewed retained release identities.
#![forbid(unsafe_code)]

#[cfg(windows)]
mod windows {
    use bonsai_artifacts::{ArtifactError, RuntimeSpec, VerifiedFile, VerifiedRuntime};
    use bonsai_broker::launch_policy::{LaunchPlan, LaunchProfile};
    use bonsai_windows_credentials::{PrivateCredentials, validate_external_path};
    use bonsai_windows_process::{
        OwnedProcess, ParentProcess, ProcessSpec, SandboxPolicy, available_memory_bytes,
        monotonic_ticks, native_sandbox_available,
    };
    use serde::Deserialize;
    use serde_json::{Value, json};
    use std::{ffi::OsString, fs, io::{self, Write as _}, path::PathBuf, thread, time::{Duration, Instant}};

    type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct FilePin { name: String, bytes: u64, sha256: String }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Request {
        data_directory: PathBuf,
        evidence_directory: PathBuf,
        shutdown_directory: PathBuf,
        mode: String,
        native_version: Option<String>,
        row_workers: u8,
        use_repacked_ptq1: bool,
        port: u16,
        maximum_seconds: u64,
        deadline_ticks: u64,
        parent_process_id: u32,
        parent_creation_time_file_time: u64,
        runtime: Option<RuntimeSpec>,
        #[serde(default)]
        native_runtime_files: Vec<FilePin>,
    }

    fn invalid(message: &'static str) -> io::Error { io::Error::other(message) }
    fn save(path: &std::path::Path, value: &Value) -> Result<()> {
        let temporary = path.with_extension("json.partial");
        let mut output = fs::OpenOptions::new().write(true).create_new(true).open(&temporary)?;
        output.write_all(&serde_json::to_vec_pretty(value)?)?;
        output.sync_all()?;
        drop(output);
        fs::rename(temporary, path)?;
        Ok(())
    }

    fn checkpoint(request: &Request, parent: &ParentProcess, started: Instant) -> Result<()> {
        parent.ensure_alive()?;
        let (ticks, _) = monotonic_ticks()?;
        let signal = request.shutdown_directory.join("shutdown.request");
        let requested = match fs::symlink_metadata(&signal) {
            Ok(metadata) => {
                validate_external_path(&signal)?;
                if !metadata.is_file() || metadata.len() != 0 {
                    return Err(invalid("Shutdown signal must be an empty ordinary file").into());
                }
                true
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => false,
            Err(error) => return Err(error.into()),
        };
        if ticks >= request.deadline_ticks
            || started.elapsed() >= Duration::from_secs(request.maximum_seconds)
            || requested
        { return Err(invalid("Owner cancelled or original deadline exhausted").into()); }
        Ok(())
    }

    pub(super) fn run() -> Result<()> {
        let args: Vec<_> = std::env::args_os().skip(1).collect();
        let [path] = args.as_slice() else { return Err(invalid("Expected one absolute request JSON path").into()); };
        let request_path = PathBuf::from(path);
        validate_external_path(&request_path)?;
        let request: Request = serde_json::from_slice(&fs::read(request_path)?)?;
        let started = Instant::now();
        if !cfg!(target_arch = "aarch64") {
            return Err(invalid("Timing owner requires native ARM64").into());
        }
        for path in [&request.data_directory, &request.evidence_directory, &request.shutdown_directory] {
            validate_external_path(path)?;
            if !path.is_dir() { return Err(invalid("External directories must exist").into()); }
        }
        if !(1..=300).contains(&request.maximum_seconds) || request.port < 1024
            || !matches!(request.row_workers, 1 | 4)
            || !matches!(request.mode.as_str(), "native" | "publisher" | "qwen")
        { return Err(invalid("Unsupported timing selection or limits").into()); }
        let parent = ParentProcess::open(request.parent_process_id, request.parent_creation_time_file_time)?;
        checkpoint(&request, &parent, started)?;
        native_sandbox_available()?;
        let profile = if request.mode == "qwen" { LaunchProfile::ReferenceCpu } else { LaunchProfile::SwiftPq2Assessment };
        if request.mode != "native" && request.row_workers != 1
            || request.mode == "qwen" && request.use_repacked_ptq1
        { return Err(invalid("Incompatible worker or weight selection").into()); }
        if available_memory_bytes()? < profile.required_available_bytes() {
            return Err(invalid("Physical memory admission failed").into());
        }
        let credentials = PrivateCredentials::create(&request.evidence_directory)?;
        let worker = credentials.directory().join("worker");
        fs::create_dir(&worker)?;
        let plan = LaunchPlan::for_profile_with_options(
            &request.data_directory, request.port, credentials.backend_path(), profile,
            request.mode != "qwen", request.use_repacked_ptq1,
        )?;
        let mut guards: Vec<VerifiedFile> = Vec::new();
        let mut check = || checkpoint(&request, &parent, started).map_err(|_| ArtifactError::Cancelled);
        let model_pin: Value = if request.mode == "qwen" {
            serde_json::from_str(include_str!("../../../config/runtime.lock.json"))?
        } else if request.use_repacked_ptq1 {
            let pin: Value = serde_json::from_str(include_str!("../../../config/swift.ptq1-export.json"))?;
            json!({"model":{"bytes":pin["OutputBytes"],"sha256":pin["OutputSha256"]}})
        } else { serde_json::from_str(include_str!("../../../config/swift-assessment.lock.json"))? };
        guards.push(VerifiedFile::verify_with(
            &plan.model_path, model_pin["model"]["bytes"].as_u64().ok_or_else(|| invalid("Model size missing"))?,
            model_pin["model"]["sha256"].as_str().ok_or_else(|| invalid("Model digest missing"))?, &mut check,
        )?);
        if let Some(adapter) = &plan.adapter {
            guards.push(VerifiedFile::verify_with(&adapter.path, adapter.bytes, &adapter.sha256, &mut check)?);
        }
        let mut runtime_guard = None;
        let mut readonly = vec![plan.model_path.clone(), credentials.backend_path().to_owned()];
        readonly.extend(plan.adapter.as_ref().map(|adapter| adapter.path.clone()));
        let (executable, arguments) = if request.mode == "native" {
            let version = request.native_version.as_deref().ok_or_else(|| invalid("Native version missing"))?;
            if !matches!(version, "0.1.9" | "0.1.10" | "0.1.13" | "0.1.14") {
                return Err(invalid("Unreviewed native release").into());
            }
            let runtime = request.runtime.as_ref().ok_or_else(|| invalid("Native runtime pins missing"))?;
            let expected_archive = request.data_directory.join("downloads")
                .join(format!("prism-inference-{version}-aarch64-pc-windows-msvc.zip"));
            if runtime.archive_path != expected_archive || runtime.runtime_directory.parent().and_then(std::path::Path::parent)
                != Some(request.data_directory.join("inference").as_path())
                || request.native_runtime_files.is_empty() || request.native_runtime_files.len() > 32
            { return Err(invalid("Native runtime location or file inventory mismatch").into()); }
            guards.push(VerifiedFile::verify_with(&runtime.archive_path, runtime.archive_bytes, &runtime.archive_sha256, &mut check)?);
            let executable = runtime.runtime_directory.join("prism-inference-server.exe");
            let mut server_seen = false;
            for pin in &request.native_runtime_files {
                if pin.name.is_empty() || !pin.name.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)) {
                    return Err(invalid("Unsafe native runtime file name").into());
                }
                let file = runtime.runtime_directory.join(&pin.name);
                guards.push(VerifiedFile::verify_with(&file, pin.bytes, &pin.sha256, &mut check)?);
                server_seen |= file == executable;
                readonly.push(file);
            }
            if !server_seen { return Err(invalid("Missing pinned native server").into()); }
            let mut args: Vec<OsString> = vec!["--native-engine-child".into(), "--model".into(), plan.model_path.as_os_str().to_owned(),
                "--port".into(), request.port.to_string().into(), "--key-file".into(), credentials.backend_path().as_os_str().to_owned(),
                "--seed".into(), "42".into(), "--row-workers".into(), request.row_workers.to_string().into()];
            if request.use_repacked_ptq1 { args.push("--use-repacked-ptq1".into()); }
            if let Some(adapter) = &plan.adapter { args.extend(["--abliterate-lora".into(), adapter.path.as_os_str().to_owned()]); }
            (executable, args)
        } else {
            let pin: Value = if request.mode == "qwen" { serde_json::from_str(include_str!("../../../config/runtime.lock.json"))? }
                else { serde_json::from_str(include_str!("../../../config/swift-assessment.lock.json"))? };
            let runtime = RuntimeSpec { archive_path:plan.archive_path.clone(), runtime_directory:plan.runtime_directory.clone(),
                archive_bytes:pin["runtime"]["asset"]["bytes"].as_u64().ok_or_else(|| invalid("Runtime size missing"))?,
                archive_sha256:pin["runtime"]["asset"]["sha256"].as_str().ok_or_else(|| invalid("Runtime digest missing"))?.into() };
            let verified = VerifiedRuntime::verify_with(&runtime, &mut check)?;
            if verified.executable() != plan.executable { return Err(invalid("Pinned executable mismatch").into()); }
            readonly.extend(verified.files().iter().map(|file| file.path().to_owned()));
            runtime_guard = Some(verified);
            (plan.executable.clone(), plan.arguments.clone())
        };
        readonly.push(executable.clone()); readonly.sort(); readonly.dedup();
        checkpoint(&request, &parent, started)?;
        if available_memory_bytes()? < profile.required_available_bytes() { return Err(invalid("Admission changed before start").into()); }
        let spec = ProcessSpec { executable, arguments, working_directory:worker.clone(),
            stdout:request.evidence_directory.join("backend.stdout"), stderr:request.evidence_directory.join("backend.stderr"),
            memory_limit_bytes:profile.memory_limit_bytes(), active_process_limit:1 };
        let process = OwnedProcess::start_sandboxed(&spec, &SandboxPolicy { readonly_paths:readonly, writable_directory:worker })?;
        let ready = json!({"status":"started","processId":process.id(),"port":request.port,
            "credentialDirectory":credentials.directory(),"nativeSandbox":true,"runtimeVersion":request.native_version,
            "modelPath":plan.model_path,"rowWorkersConfigured":request.row_workers});
        let ready_result = save(&request.evidence_directory.join("owner.json"), &ready);
        if ready_result.is_ok() { println!("{ready}"); }
        let monitor = (|| -> Result<&'static str> {
            if ready_result.is_err() { return Ok("readiness-receipt-failed"); }
            loop {
                if checkpoint(&request, &parent, started).is_err() { return Ok("cancelled-parent-or-deadline"); }
                if process.try_wait()?.is_some() { return Ok("backend-exited"); }
                let memory = process.memory_usage()?;
                if memory.private_bytes > profile.memory_limit_bytes() || memory.working_set_bytes > profile.memory_limit_bytes()
                    || available_memory_bytes()? < 4 * 1024 * 1024 * 1024 { return Ok("resource-guard"); }
                thread::sleep(Duration::from_millis(50));
            }
        })();
        let peak = process.peak_job_memory_bytes().ok();
        let stop = process.stop(Duration::from_secs(10));
        let active = process.active_processes().ok();
        let cleanup = json!({"ownedJobStopped":stop.is_ok() && active==Some(0),"activeProcesses":active,
            "peakJobMemoryBytes":peak,"reason":monitor.as_ref().copied().unwrap_or("monitor-failed"),
            "elapsedSeconds":started.elapsed().as_secs_f64()});
        save(&request.evidence_directory.join("cleanup.json"), &cleanup)?;
        drop(runtime_guard); drop(guards); drop(credentials);
        stop?;
        if active != Some(0) { return Err(invalid("Owned Job remains active").into()); }
        ready_result?;
        monitor?;
        Ok(())
    }
}

fn main() -> std::process::ExitCode {
    #[cfg(windows)]
    match windows::run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => { eprintln!("Timing owner failed: {error}"); std::process::ExitCode::FAILURE }
    }
    #[cfg(not(windows))]
    { eprintln!("Timing owner requires native Windows ARM64"); std::process::ExitCode::FAILURE }
}
