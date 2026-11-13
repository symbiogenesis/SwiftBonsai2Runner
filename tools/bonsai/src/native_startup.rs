//! Native speech provisioning and compiler orchestration outside presentation.
#![expect(
    clippy::redundant_pub_crate,
    reason = "Sibling startup modules share an explicit crate-local implementation API."
)]

use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use bonsai_artifacts::validate_external_path;
use serde_json::{Value, json};

mod asr_cpu;
pub(crate) use asr_cpu::AsrBuildTarget;

const TARGET: &str = "aarch64-pc-windows-msvc";
static NEXT_NONCE: AtomicU64 = AtomicU64::new(0);

pub(crate) fn nonce() -> crate::Result<String> {
    Ok(format!(
        "{}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos(),
        NEXT_NONCE.fetch_add(1, Ordering::Relaxed)
    ))
}

fn text<'a>(value: &'a Value, key: &str) -> crate::Result<&'a str> {
    value[key]
        .as_str()
        .ok_or_else(|| format!("Missing native startup field: {key}"))
}

fn bytes(value: &Value) -> crate::Result<u64> {
    value["bytes"]
        .as_u64()
        .ok_or_else(|| "Missing pinned artifact length.".into())
}

fn external(path: &Path) -> crate::Result<()> {
    validate_external_path(path).map_err(|error| error.to_string())
}

fn require_host() -> crate::Result<()> {
    if !cfg!(all(windows, target_arch = "aarch64")) {
        return Err("Native speech builds require native ARM64 Windows.".into());
    }
    Ok(())
}

#[cfg(any(windows, test))]
fn host_report(identity: &Value) -> Value {
    let digest =
        crate::identity::sha256(format!("runner-native-speech-host-v1\n{identity}").as_bytes());
    json!({"SchemaVersion": 1, "Kind": "runner-native-speech-host",
        "HostCacheKey": digest, "Host": identity})
}

/// Reports checked build-host identity without provisioning or compiling anything.
pub(crate) fn host() -> crate::Result<Value> {
    require_host()?;
    #[cfg(windows)]
    {
        let host =
            bonsai_windows_process::native_build_host().map_err(|error| error.to_string())?;
        Ok(host_report(&json!({
            "Target": TARGET, "ProcessorName": host.processor_name,
            "ProcessorIdentifier": host.processor_identifier,
            "VendorIdentifier": host.vendor_identifier,
            "WindowsVersion": host.windows_version, "ProcessorFeatures": host.processor_features,
        })))
    }
    #[cfg(not(windows))]
    Err("Native speech host identity requires native ARM64 Windows.".into())
}

fn host_cache_directory(
    data: &Path,
    repo: &Path,
    channel: &str,
    host_key: &str,
    asr_architecture: &str,
) -> PathBuf {
    host_cache_in(
        &data.join("build"),
        repo,
        channel,
        host_key,
        asr_architecture,
    )
}

fn host_cache_in(
    target_root: &Path,
    repo: &Path,
    channel: &str,
    host_key: &str,
    asr_architecture: &str,
) -> PathBuf {
    // A changed receipt alone cannot invalidate Cargo's already compiled native
    // objects. Partition the complete target directory, keeping its label short
    // enough for the MSVC archive paths on ordinary Windows user profiles.
    target_root
        .join("native-speech")
        .join(crate::source_build::cache_key(
            repo,
            &format!("{channel}\n{host_key}\nasr={asr_architecture}"),
        ))
}

fn checked_process(
    executable: &Path,
    arguments: &[OsString],
    working: &Path,
    name: &str,
    timeout: u64,
) -> crate::Result<String> {
    let log = working.join(format!("{name}-{}.log", nonce()?));
    let result = crate::startup_process::run(
        executable.as_os_str(),
        arguments,
        working,
        &BTreeMap::new(),
        Duration::from_secs(timeout),
        &log,
    )?;
    if result.code != 0 {
        return Err(format!(
            "Native build preparation failed ({name}); see {}: {}",
            log.display(),
            result.stderr.trim()
        ));
    }
    Ok(result.stdout)
}

fn receive(artifact: &Value, destination: &Path, working: &Path) -> crate::Result<()> {
    crate::startup_download::receive(
        text(artifact, "url")?,
        destination,
        bytes(artifact)?,
        text(artifact, "sha256")?,
        working,
    )
}

fn install_compiler(data: &Path, working: &Path, pin: &Value) -> crate::Result<PathBuf> {
    let directory = data.join("tools").join(text(pin, "directory")?);
    let archive = directory.join(text(pin, "filename")?);
    let root = directory.join(text(pin, "root")?);
    external(&root)?;
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let _guard = crate::startup_cache::lock_file(&directory.join("compiler-provision.lock"))?;
    if !root.exists() {
        receive(pin, &archive, working)?;
        let tar = crate::startup_download::system_tool("tar.exe")?;
        let listing = checked_process(
            &tar,
            &["-tvf".into(), archive.as_os_str().to_owned()],
            working,
            "llvm-types",
            600,
        )?;
        if listing
            .lines()
            .filter(|line| !line.is_empty())
            .any(|line| !line.starts_with(['-', 'd']))
        {
            return Err("LLVM archive contains a link or special file.".into());
        }
        let names = checked_process(
            &tar,
            &["-tf".into(), archive.as_os_str().to_owned()],
            working,
            "llvm-paths",
            600,
        )?;
        crate::speech_build::run(json!({
            "operation": "validate-names", "archiveRoot": text(pin, "root")?,
            "names": names.lines().filter(|line| !line.is_empty()).collect::<Vec<_>>()
        }))?;
        let stage = directory.join(format!("extract-{}", nonce()?));
        external(&stage)?;
        fs::create_dir(&stage).map_err(|error| error.to_string())?;
        checked_process(
            &tar,
            &[
                "-xf".into(),
                archive.as_os_str().to_owned(),
                "-C".into(),
                stage.as_os_str().to_owned(),
            ],
            working,
            "llvm-extract",
            1_200,
        )?;
        let extracted = stage.join(text(pin, "root")?);
        external(&extracted)?;
        fs::rename(&extracted, &root).map_err(|error| {
            format!(
                "Cannot publish pinned LLVM; evidence remains at {}: {error}",
                stage.display()
            )
        })?;
    }
    let executables = pin["executables"]
        .as_object()
        .ok_or("Missing compiler executable pins.")?;
    for (name, identity) in executables {
        let path = root.join("bin").join(name);
        crate::startup_download::verify(&path, bytes(identity)?, text(identity, "sha256")?)?;
        crate::startup_cache::assert_arm64(&path)?;
    }
    Ok(root)
}

fn setup_environment(
    output: &str,
    compiler: &Path,
    cmake: &Path,
    ninja: &Path,
    asr_architecture: &str,
) -> crate::Result<BTreeMap<String, String>> {
    let mut environment = BTreeMap::new();
    for line in output.lines() {
        let Some((name, value)) = line.split_once('=') else {
            continue;
        };
        if let Some(canonical) = [
            "PATH",
            "INCLUDE",
            "LIB",
            "LIBPATH",
            "VSINSTALLDIR",
            "VCINSTALLDIR",
            "VCToolsInstallDir",
            "WindowsSdkDir",
            "WindowsSDKVersion",
            "UniversalCRTSdkDir",
            "UCRTVersion",
            "VSCMD_ARG_TGT_ARCH",
            "VSCMD_ARG_HOST_ARCH",
        ]
        .into_iter()
        .find(|allowed| allowed.eq_ignore_ascii_case(name))
        {
            environment.insert(canonical.to_owned(), value.to_owned());
        }
    }
    if environment
        .get("VSCMD_ARG_TGT_ARCH")
        .is_none_or(|value| value != "arm64")
        || environment.get("INCLUDE").is_none_or(String::is_empty)
        || environment.get("LIB").is_none_or(String::is_empty)
    {
        return Err("Visual Studio did not provide ARM64 compiler and SDK libraries.".into());
    }
    let bin = compiler.join("bin");
    let clang = bin.join("clang-cl.exe").to_string_lossy().into_owned();
    let inherited = environment
        .get("PATH")
        .ok_or("Visual Studio omitted PATH.")?;
    environment.insert(
        "PATH".into(),
        format!(
            "{};{};{};{inherited}",
            bin.display(),
            cmake.parent().ok_or("CMake has no parent.")?.display(),
            ninja.parent().ok_or("Ninja has no parent.")?.display()
        ),
    );
    for name in ["CC", "CXX", "CMAKE_C_COMPILER", "CMAKE_CXX_COMPILER"] {
        environment.insert(name.into(), clang.clone());
    }
    for name in ["CMAKE_C_COMPILER_TARGET", "CMAKE_CXX_COMPILER_TARGET"] {
        environment.insert(name.into(), TARGET.into());
    }
    let transcribe_cmake_args = transcribe_cmake_args(&environment, asr_architecture);
    for (name, value) in [
        ("CMAKE_GENERATOR", "Ninja"),
        // transcribe-cpp applies inherited CMAKE_ARGS after its dedicated args.
        // Shadow that escape hatch so it cannot defeat the selected ISA/receipt.
        ("CMAKE_ARGS", ""),
        ("CL", "/EHsc"),
        ("CARGO_BUILD_JOBS", "4"),
        ("CMAKE_BUILD_PARALLEL_LEVEL", "4"),
    ] {
        environment.insert(name.into(), value.into());
    }
    environment.insert("TRANSCRIBE_CMAKE_ARGS".into(), transcribe_cmake_args);
    environment.insert("CMAKE".into(), cmake.to_string_lossy().into_owned());
    environment.insert(
        "RC".into(),
        bin.join("llvm-rc.exe").to_string_lossy().into_owned(),
    );
    Ok(environment)
}

fn transcribe_cmake_args(environment: &BTreeMap<String, String>, asr_architecture: &str) -> String {
    let mut args = asr_cpu::cmake_args(asr_architecture);
    if let Some(vc_tools) = environment.get("VCToolsInstallDir") {
        let vc_bin = Path::new(vc_tools).join("bin/Hostarm64/arm64");
        let archiver = vc_bin.join("lib.exe");
        let linker = vc_bin.join("link.exe");
        if archiver.is_file() && linker.is_file() {
            let ar_arg = archiver.to_string_lossy().replace('\\', "/");
            let link_arg = linker.to_string_lossy().replace('\\', "/");
            let _ = write!(
                args,
                " -DCMAKE_AR:FILEPATH=\"{ar_arg}\" -DCMAKE_LINKER:FILEPATH=\"{link_arg}\""
            );
        }
    }
    if let (Some(sdk_dir), Some(sdk_ver)) = (
        environment.get("WindowsSdkDir"),
        environment.get("WindowsSDKVersion"),
    ) {
        let manifest_tool = Path::new(sdk_dir)
            .join("bin")
            .join(sdk_ver.trim_matches(['\\', '/']))
            .join("arm64/mt.exe");
        if manifest_tool.is_file() {
            let mt_arg = manifest_tool.to_string_lossy().replace('\\', "/");
            let _ = write!(args, " -DCMAKE_MT:FILEPATH=\"{mt_arg}\"");
        }
    }
    args
}

/// Prepares ASR compiler inputs and returns a child-only environment.
pub(crate) fn environment(data: &Path, working: &Path) -> crate::Result<Value> {
    environment_for(data, working, host_asr_architecture(AsrBuildTarget::Host))
}

fn host_asr_architecture(target: AsrBuildTarget) -> &'static str {
    #[cfg(windows)]
    let observed = bonsai_windows_process::arm64_isa()
        .ok()
        .map(|isa| (isa.dotprod, isa.i8mm));
    #[cfg(not(windows))]
    let observed = None;
    target.architecture(observed)
}

fn environment_for(data: &Path, working: &Path, asr_architecture: &str) -> crate::Result<Value> {
    crate::startup_cancel::checkpoint()?;
    require_host()?;
    external(data)?;
    external(working)?;
    fs::create_dir_all(working).map_err(|error| error.to_string())?;
    let pin = crate::speech_build::build_plan()?;
    let compiler = install_compiler(data, working, &pin["compiler"])?;
    let program_files = PathBuf::from(
        std::env::var_os("ProgramFiles(x86)")
            .ok_or("Missing Windows x86 Program Files directory.")?,
    );
    external(&program_files)?;
    let vswhere = program_files.join("Microsoft Visual Studio/Installer/vswhere.exe");
    let visual_studio = PathBuf::from(
        checked_process(
            &vswhere,
            &[
                "-latest".into(),
                "-products".into(),
                "*".into(),
                "-property".into(),
                "installationPath".into(),
            ],
            working,
            "visual-studio-discovery",
            60,
        )?
        .trim(),
    );
    external(&visual_studio)?;
    let devcmd = visual_studio.join("Common7/Tools/VsDevCmd.bat");
    let cmake =
        visual_studio.join("Common7/IDE/CommonExtensions/Microsoft/CMake/CMake/bin/cmake.exe");
    let ninja = visual_studio.join("Common7/IDE/CommonExtensions/Microsoft/CMake/Ninja/ninja.exe");
    for path in [&devcmd, &cmake, &ninja] {
        if !path.is_file() {
            return Err(format!(
                "Visual Studio ARM64 build prerequisite is missing: {}",
                path.display()
            ));
        }
    }
    let command = devcmd
        .to_str()
        .ok_or("Visual Studio path is not Unicode.")?;
    if command.contains(['"', '%', '\r', '\n']) {
        return Err("Unsupported Visual Studio installation path.".into());
    }
    let batch = working.join(format!("visual-studio-env-{}.cmd", nonce()?));
    external(&batch)?;
    fs::write(&batch, format!("@call \"{command}\" -no_logo -arch=arm64 -host_arch=arm64 >nul\r\n@if errorlevel 1 exit /b 1\r\n@set\r\n")).map_err(|error| error.to_string())?;
    let output = checked_process(
        &crate::startup_download::system_tool("cmd.exe")?,
        &["/d".into(), "/c".into(), batch.as_os_str().to_owned()],
        working,
        "visual-studio-environment",
        60,
    )?;
    let environment = setup_environment(&output, &compiler, &cmake, &ninja, asr_architecture)?;
    Ok(json!({ "Environment": environment,
        "CompilerPath": compiler.join("bin/clang-cl.exe"), "CMakePath": cmake,
        "AsrCpuArchitecture": asr_architecture }))
}

fn packages(model: &str) -> crate::Result<Vec<String>> {
    match model.to_ascii_lowercase().as_str() {
        "all" => Ok(vec!["bonsai-speech-asr".into(), "bonsai-speech-tts".into()]),
        "qwenasr" | "canary" => Ok(vec!["bonsai-speech-asr".into()]),
        "qwentts" => Ok(vec!["bonsai-speech-tts".into()]),
        _ => Err("Unsupported native speech build selection.".into()),
    }
}

/// Reuses checked native outputs before provisioning any compiler or sources.
pub(crate) fn build(
    repo: &Path,
    data: &Path,
    working: &Path,
    model: &str,
    force: bool,
) -> crate::Result<Value> {
    build_with_environment(
        repo,
        data,
        working,
        model,
        force,
        AsrBuildTarget::Host,
        &BTreeMap::new(),
    )
}

/// Hosted candidate builds bind exact compiler/linker overlays to the receipt.
pub(crate) fn build_with_environment(
    repo: &Path,
    data: &Path,
    working: &Path,
    model: &str,
    force: bool,
    asr_target: AsrBuildTarget,
    build_environment: &BTreeMap<String, String>,
) -> crate::Result<Value> {
    build_configured(
        repo,
        data,
        working,
        model,
        force,
        &BuildOptions {
            asr_target,
            environment: build_environment,
            target_directory: None,
        },
    )
}

struct BuildOptions<'a> {
    asr_target: AsrBuildTarget,
    environment: &'a BTreeMap<String, String>,
    target_directory: Option<&'a Path>,
}

/// Builds Rust TTS and, when selected, the separate native ASR companion.
/// The legacy include-sources switch is accepted without staging retired TTS code.
pub(crate) fn build_for_package(
    repo: &Path,
    data: &Path,
    working: &Path,
    model: &str,
    force: bool,
    target_directory: Option<&Path>,
    _include_sources: bool,
) -> crate::Result<Value> {
    build_configured(
        repo,
        data,
        working,
        model,
        force,
        &BuildOptions {
            asr_target: AsrBuildTarget::Host,
            environment: &BTreeMap::new(),
            target_directory,
        },
    )
}

fn build_configured(
    repo: &Path,
    data: &Path,
    working: &Path,
    model: &str,
    force: bool,
    options: &BuildOptions<'_>,
) -> crate::Result<Value> {
    require_host()?;
    external(data)?;
    external(working)?;
    let channel = crate::source_build::channel(repo)?;
    let selected_packages = packages(model)?;
    let needs_native = selected_packages
        .iter()
        .any(|package| package == "bonsai-speech-asr");
    let native_host = if needs_native {
        host()?
    } else {
        json!({"Kind":"rust-tts-portable", "Target":TARGET})
    };
    let asr_target = options.asr_target;
    let asr_architecture = if needs_native {
        host_asr_architecture(asr_target)
    } else {
        "none"
    };
    let host_key = if needs_native {
        text(&native_host, "HostCacheKey")?
    } else {
        "rust-tts-v1"
    };
    let target_directory = if let Some(target) = options.target_directory {
        external(target)?;
        host_cache_in(target, repo, &channel, host_key, asr_architecture)
    } else {
        host_cache_directory(data, repo, &channel, host_key, asr_architecture)
    };
    let build_environment = options.environment;
    let mut identity = format!(
        "{}\n{}\n{native_host}\nasr-target={asr_target:?}\nasr={asr_architecture}",
        if needs_native {
            crate::speech_build::build_plan()?
        } else {
            json!({"Engine":"rust-qwen-tts-v1"})
        },
        crate::startup_cache::digest_file(
            &std::env::current_exe().map_err(|error| error.to_string())?
        )?
    );
    if !build_environment.is_empty() {
        write!(identity, "\n{build_environment:?}").map_err(|error| error.to_string())?;
    }
    let spec = crate::source_build::BuildSpec {
        repository_root: repo.to_path_buf(),
        working_directory: working.to_path_buf(),
        target_directory,
        packages: selected_packages,
        channel,
        target: Some(TARGET.into()),
        identity,
        force,
    };
    let outputs = crate::source_build::cached_build_with_timeout(
        &spec,
        || {
            let mut overlay: BTreeMap<String, String> = if needs_native {
                let native = environment_for(data, working, asr_architecture)?;
                serde_json::from_value(native["Environment"].clone())
                    .map_err(|error| error.to_string())?
            } else {
                BTreeMap::new()
            };
            overlay.extend(build_environment.clone());
            Ok(overlay)
        },
        Duration::from_secs(if build_environment.is_empty() {
            3600
        } else {
            1200
        }),
    )?;
    let result = json!({"AsrPath": outputs.get("bonsai-speech-asr"), "TtsPath": outputs.get("bonsai-speech-tts"),
        "NativeBuildHost": native_host, "AsrCpuArchitecture": if needs_native { Some(asr_architecture) } else { None }});
    Ok(result)
}

/// Provisions selected speech models, deferring cached hashes only for launch use.
pub(crate) fn models(
    data: &Path,
    model: &str,
    quantization: &str,
    verify_only: bool,
    defer: bool,
    working: &Path,
) -> crate::Result<Value> {
    if verify_only && defer {
        return Err("VerifyOnly cannot be combined with DeferCachedVerification.".into());
    }
    external(data)?;
    external(working)?;
    let quantization = quantization.to_ascii_uppercase();
    let result = crate::speech::run(
        json!({"dataDirectory": data, "model": model, "ttsQuantization": quantization}),
    )?;
    provision_models(result, verify_only, defer, working)
}

fn provision_models(
    result: Value,
    verify_only: bool,
    defer: bool,
    working: &Path,
) -> crate::Result<Value> {
    provision_models_with(result, |plan| {
        let path = Path::new(text(plan, "Path")?);
        let bytes = plan["Bytes"]
            .as_u64()
            .ok_or("Missing speech model length.")?;
        if defer && path.exists() {
            crate::startup_download::verify_length(path, bytes)
        } else if verify_only {
            crate::startup_download::verify(path, bytes, text(plan, "Sha256")?)
        } else {
            crate::startup_download::receive(
                text(plan, "Uri")?,
                path,
                bytes,
                text(plan, "Sha256")?,
                working,
            )
        }
    })
}

struct ModelPreparation {
    next: AtomicUsize,
    cancelled: Arc<AtomicBool>,
    failure: Mutex<Option<String>>,
}

impl ModelPreparation {
    fn fail(&self, error: String) {
        if let Ok(mut failure) = self.failure.lock()
            && failure.is_none()
        {
            *failure = Some(error);
        }
        self.cancelled.store(true, Ordering::Release);
    }
}

struct PreparationWorker<'a> {
    group: &'a ModelPreparation,
    finished: bool,
}

impl Drop for PreparationWorker<'_> {
    fn drop(&mut self) {
        if !self.finished {
            self.group
                .fail("Speech artifact preparation worker panicked.".into());
        }
    }
}

fn provision_models_with(
    mut result: Value,
    provision: impl Fn(&Value) -> crate::Result<()> + Sync,
) -> crate::Result<Value> {
    let plans = result["Plans"]
        .as_array()
        .ok_or("Missing speech model plan.")?;
    let mut outputs = BTreeSet::new();
    for plan in plans {
        let path = Path::new(text(plan, "Path")?);
        external(path)?;
        let name = path
            .file_name()
            .ok_or("Speech artifact has no filename.")?
            .to_string_lossy();
        // Also reject an output colliding with another artifact's partial/lock.
        // Windows accepts either separator and ordinarily folds filename case.
        for output in [
            path.to_path_buf(),
            path.with_file_name(format!("{name}.partial")),
            path.with_file_name(format!("{name}.download.lock")),
        ] {
            if !outputs.insert(output.to_string_lossy().replace('\\', "/").to_uppercase()) {
                return Err("Speech artifact output paths overlap.".into());
            }
        }
    }
    let group = ModelPreparation {
        next: AtomicUsize::new(0),
        cancelled: Arc::new(AtomicBool::new(false)),
        failure: Mutex::new(None),
    };
    thread::scope(|scope| {
        let mut workers = Vec::with_capacity(plans.len().min(2));
        for index in 0..plans.len().min(2) {
            let group = &group;
            let provision = &provision;
            let worker = thread::Builder::new()
                .name(format!("speech-artifacts-{index}"))
                .spawn_scoped(scope, move || {
                    let mut guard = PreparationWorker {
                        group,
                        finished: false,
                    };
                    crate::startup_cancel::with_peer_cancellation(&group.cancelled, || {
                        while !group.cancelled.load(Ordering::Acquire) {
                            let Some(plan) = plans.get(group.next.fetch_add(1, Ordering::Relaxed))
                            else {
                                break;
                            };
                            if let Err(error) =
                                crate::startup_cancel::checkpoint().and_then(|()| provision(plan))
                            {
                                group.fail(error);
                                break;
                            }
                        }
                    });
                    guard.finished = true;
                });
            match worker {
                Ok(worker) => workers.push(worker),
                Err(error) => {
                    group.fail(format!(
                        "Cannot start speech artifact preparation worker: {error}"
                    ));
                    break;
                }
            }
        }
        for worker in workers {
            if worker.join().is_err() {
                group.fail("Speech artifact preparation worker panicked.".into());
            }
        }
    });
    if let Some(error) = group
        .failure
        .into_inner()
        .map_err(|_| "Speech artifact preparation result was poisoned.")?
    {
        return Err(error);
    }
    result
        .as_object_mut()
        .ok_or("Missing speech model result.")?
        .remove("Plans");
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parallel_plan() -> crate::Result<Value> {
        let root = std::env::temp_dir().join(format!("Bonsai-ParallelSpeech-{}", nonce()?));
        Ok(json!({"Retained": true, "Plans": (0..4).map(|index|
            json!({"Path": root.join(format!("artifact-{index}.gguf")), "Index": index})
        ).collect::<Vec<_>>()}))
    }

    fn meet_peer(gate: &(Mutex<usize>, std::sync::Condvar)) -> crate::Result<()> {
        let mut arrivals = gate.0.lock().map_err(|error| error.to_string())?;
        *arrivals += 1;
        gate.1.notify_all();
        let timeout;
        (arrivals, timeout) = gate
            .1
            .wait_timeout_while(arrivals, Duration::from_secs(2), |arrivals| *arrivals < 2)
            .map_err(|error| error.to_string())?;
        drop(arrivals);
        if timeout.timed_out() {
            return Err("Independent speech artifact preparation did not overlap.".into());
        }
        Ok(())
    }

    #[test]
    fn speech_artifact_preparation_overlaps_with_exactly_two_workers() -> crate::Result<()> {
        let active = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);
        let hits: [_; 4] = std::array::from_fn(|_| AtomicUsize::new(0));
        let gate = (Mutex::new(0), std::sync::Condvar::new());
        let result = provision_models_with(parallel_plan()?, |plan| {
            let index = usize::try_from(plan["Index"].as_u64().ok_or("Missing artifact index.")?)
                .map_err(|error| error.to_string())?;
            hits[index].fetch_add(1, Ordering::Relaxed);
            peak.fetch_max(active.fetch_add(1, Ordering::AcqRel) + 1, Ordering::Relaxed);
            let met = meet_peer(&gate);
            active.fetch_sub(1, Ordering::Release);
            met
        })?;
        assert_eq!(peak.load(Ordering::Relaxed), 2);
        assert!(hits.iter().all(|hits| hits.load(Ordering::Relaxed) == 1));
        assert!(result.get("Plans").is_none());
        assert_eq!(result["Retained"], true);
        Ok(())
    }

    #[test]
    fn speech_artifact_failure_and_panic_cancel_peer_before_joining() -> crate::Result<()> {
        for panic in [false, true] {
            let plan = parallel_plan()?;
            let gate = (Mutex::new(0), std::sync::Condvar::new());
            let peer_cancelled = AtomicBool::new(false);
            let calls = AtomicUsize::new(0);
            let result = provision_models_with(plan, |plan| {
                calls.fetch_add(1, Ordering::Relaxed);
                meet_peer(&gate)?;
                if plan["Index"] == 0 {
                    if panic {
                        std::panic::resume_unwind(Box::new("Synthetic speech artifact panic."));
                    }
                    return Err("Synthetic speech artifact failure.".into());
                }
                let started = std::time::Instant::now();
                while started.elapsed() < Duration::from_secs(2) {
                    if let Err(error) = crate::startup_cancel::checkpoint() {
                        peer_cancelled.store(true, Ordering::Release);
                        return Err(error);
                    }
                    thread::yield_now();
                }
                Err("Peer cancellation was not observed.".into())
            });
            let error = result.err().ok_or("Expected speech artifact failure.")?;
            assert_eq!(
                error,
                if panic {
                    "Speech artifact preparation worker panicked."
                } else {
                    "Synthetic speech artifact failure."
                }
            );
            assert!(peer_cancelled.load(Ordering::Acquire));
            assert_eq!(calls.load(Ordering::Relaxed), 2);
            crate::startup_cancel::checkpoint()?;
        }
        Ok(())
    }

    #[test]
    fn speech_artifact_output_collisions_fail_before_any_work() -> crate::Result<()> {
        let original = parallel_plan()?;
        let path = Path::new(text(&original["Plans"][0], "Path")?);
        for duplicate in [
            path.to_path_buf(),
            PathBuf::from(path.to_string_lossy().to_uppercase()),
            path.with_file_name(format!(
                "{}.partial",
                path.file_name().ok_or("Missing name.")?.to_string_lossy()
            )),
            path.with_file_name(format!(
                "{}.download.lock",
                path.file_name().ok_or("Missing name.")?.to_string_lossy()
            )),
        ] {
            let mut plan = original.clone();
            plan["Plans"][1]["Path"] = json!(duplicate);
            let calls = AtomicUsize::new(0);
            let result = provision_models_with(plan, |_| {
                calls.fetch_add(1, Ordering::Relaxed);
                Ok(())
            });
            assert!(result.is_err_and(|error| error.contains("output paths overlap")));
            assert_eq!(calls.load(Ordering::Relaxed), 0);
        }
        Ok(())
    }

    #[test]
    fn native_host_identity_partitions_complete_cargo_output_directories() -> crate::Result<()> {
        let identity = json!({"Target": TARGET,
            "ProcessorName": "Snapdragon X2 Elite Extreme - X2E94100",
            "ProcessorIdentifier": "ARMv8 (64-bit) Family 8 Model 2 Revision 201",
            "VendorIdentifier": "Qualcomm Technologies Inc",
            "WindowsVersion": [10, 0, 26300], "ProcessorFeatures": [19, 43, 66]});
        let report = host_report(&identity);
        assert_eq!(report, host_report(&identity));
        let key = text(&report, "HostCacheKey")?;
        assert!(crate::identity::is_lower_hex(key, 64));
        let data = Path::new("C:/data");
        let repo = Path::new("C:/source");
        let channel = "nightly-2026-11-12-aarch64-pc-windows-msvc";
        let architecture = "armv8-a+dotprod+i8mm";
        let directory = host_cache_directory(data, repo, channel, key, architecture);
        let target = Path::new("C:/restored cargo cache");
        let overridden = host_cache_in(target, repo, channel, key, architecture);
        assert_eq!(
            overridden.parent(),
            Some(target.join("native-speech").as_path())
        );
        assert_eq!(directory.file_name(), overridden.file_name());
        assert_eq!(
            directory
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::len),
            Some(24)
        );
        assert_ne!(
            directory,
            data.join("build/native-speech")
                .join(crate::source_build::cache_key(repo, channel))
        );
        for (field, value) in [
            ("ProcessorName", json!("Snapdragon X Elite - X1E78100")),
            (
                "ProcessorIdentifier",
                json!("ARMv8 (64-bit) Family 8 Model 1 Revision 1"),
            ),
            ("VendorIdentifier", json!("Another vendor")),
            ("WindowsVersion", json!([10, 0, 26301])),
            ("ProcessorFeatures", json!([19, 43])),
        ] {
            let mut changed = identity.clone();
            changed[field] = value;
            let changed = host_report(&changed);
            let changed_key = text(&changed, "HostCacheKey")?;
            assert_ne!(key, changed_key);
            assert_ne!(
                directory,
                host_cache_directory(data, repo, channel, changed_key, architecture)
            );
            assert_ne!(
                overridden,
                host_cache_in(target, repo, channel, changed_key, architecture)
            );
        }
        assert_ne!(
            directory,
            host_cache_directory(data, repo, channel, key, "armv8-a")
        );
        assert_ne!(
            overridden,
            host_cache_in(target, repo, channel, key, "armv8-a")
        );
        assert!(
            crate::speech_build::run(json!({"operation": "host", "processorName": "injected"}))
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn native_environment_preserves_system_settings_and_pinned_compilers() -> crate::Result<()> {
        let output = "Path=original\r\nInclude=include\r\nLIB=lib\r\nVSCMD_ARG_TGT_ARCH=arm64\r\nSystemRoot=unsafe override\r\nAPPDATA=unsafe override\r\n";
        let result = setup_environment(
            output,
            Path::new("compiler"),
            Path::new("cmake/bin/cmake.exe"),
            Path::new("ninja/ninja.exe"),
            "armv8-a+dotprod+i8mm",
        )?;
        assert!(result["PATH"].ends_with(";original"));
        assert_eq!(result["CC"], result["CXX"]);
        assert!(!result.keys().any(|name| name.starts_with("BONSAI_TTS")));
        assert_eq!(result["CMAKE_ARGS"], "");
        assert_eq!(result["CMAKE_C_COMPILER_TARGET"], TARGET);
        for option in [
            "-DGGML_NATIVE=OFF",
            "-DCMAKE_C_FLAGS=/clang:-march=armv8-a+dotprod+i8mm",
            "-DCMAKE_CXX_FLAGS=/clang:-march=armv8-a+dotprod+i8mm",
        ] {
            assert!(
                result["TRANSCRIBE_CMAKE_ARGS"]
                    .split_ascii_whitespace()
                    .any(|arg| arg == option)
            );
        }
        assert!(!result.contains_key("SystemRoot"));
        assert!(!result.contains_key("APPDATA"));
        assert!(
            setup_environment(
                "VSCMD_ARG_TGT_ARCH=x64",
                Path::new("compiler"),
                Path::new("cmake"),
                Path::new("ninja"),
                "armv8-a",
            )
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn native_selection_and_model_modes_reject_unsupported_requests() {
        assert_eq!(packages("All").map(|value| value.len()), Ok(2));
        assert_eq!(packages("Canary").map(|value| value.len()), Ok(1));
        assert_eq!(packages("QwenAsr"), packages("Canary"));
        assert!(packages("other").is_err());
        assert!(
            models(
                Path::new("relative"),
                "All",
                "Q8_0",
                true,
                true,
                Path::new("relative")
            )
            .is_err()
        );
    }

    #[test]
    fn ordinary_model_setup_defers_only_cached_hashes() -> crate::Result<()> {
        use sha2::{Digest as _, Sha256};
        let working = std::env::temp_dir().join(format!("Bonsai-ModelStartupTest-{}", nonce()?));
        external(&working)?;
        fs::create_dir(&working).map_err(|error| error.to_string())?;
        let path = working.join("model.gguf");
        fs::write(&path, b"wrong").map_err(|error| error.to_string())?;
        let plan = json!({"CanaryModelPath": path, "Plans": [{"Path": path, "Bytes": 5,
            "Sha256": crate::identity::hex(&Sha256::digest(b"right")), "Uri": "https://example.invalid/model"}]});
        assert!(
            provision_models(plan.clone(), false, true, &working)?
                .get("Plans")
                .is_none()
        );
        assert!(provision_models(plan.clone(), true, false, &working).is_err());
        assert!(provision_models(plan.clone(), false, false, &working).is_err());
        fs::write(&path, b"shorter").map_err(|error| error.to_string())?;
        assert!(provision_models(plan, false, true, &working).is_err());
        Ok(())
    }
}
