//! Hosted preparation of source-bound Hexagon diagnostics and data-only SDK review.
//!
//! These operations never load DSP code, change trust or admit a runtime profile.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::io::{Read as _, Seek as _, Write as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use bonsai_artifacts::{hash_reader, is_redirect, validate_external_path};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{Result, identity, startup_cache, startup_process};

mod device;
#[cfg(windows)]
mod model_device;
mod preflight;
mod projection_build;

const TOOLCHAIN: &str = "nightly-2026-11-09-aarch64-pc-windows-msvc";
const TARGET: &str = "aarch64-pc-windows-msvc";
const NATIVE_INPUTS: &[&str] = &[
    "lib.rs",
    "windows.rs",
    "mapping.rs",
    "session.rs",
    "session_native.rs",
    "session_mapping.rs",
    "session_mapping_native.rs",
    "compute.rs",
    "compute_native.rs",
    "projection.rs",
    "projection_native.rs",
    "query.rs",
    "sources.json",
];

#[derive(Deserialize)]
#[serde(
    tag = "operation",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum Request {
    BuildDispatch {
        repository_directory: PathBuf,
        output_directory: PathBuf,
    },
    BuildPackedArtifact {
        repository_directory: PathBuf,
        gguf_source_directory: PathBuf,
        output_directory: PathBuf,
    },
    SdkInventory {
        repository_directory: PathBuf,
        output_directory: PathBuf,
    },
    #[cfg(windows)]
    Device { request: device::Probe },
    #[cfg(windows)]
    DeviceWorker { request: device::Probe },
    #[cfg(windows)]
    ModelDevice { request: model_device::Request },
    Preflight {
        #[serde(default)]
        package_path: Option<PathBuf>,
        #[serde(default)]
        output_path: Option<PathBuf>,
        maximum_seconds: u32,
    },
    PreflightWorker {
        #[serde(default)]
        package_path: Option<PathBuf>,
    },
}

struct Context {
    repo: PathBuf,
    output: PathBuf,
    revision: String,
    run: String,
    attempt: String,
    holds: Vec<File>,
    environment: BTreeMap<String, String>,
}

fn digits(text: &str, maximum: usize) -> bool {
    !text.is_empty() && text.len() <= maximum && text.bytes().all(|byte| byte.is_ascii_digit())
}

fn hosted() -> Result<(String, String, String)> {
    if !cfg!(all(windows, target_arch = "aarch64"))
        || std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
        || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
    {
        return Err("Hexagon preparation requires hosted Windows ARM64; owner-device fallback is unsupported.".into());
    }
    let revision = std::env::var("GITHUB_SHA").unwrap_or_default();
    let run = std::env::var("GITHUB_RUN_ID").unwrap_or_default();
    let attempt = std::env::var("GITHUB_RUN_ATTEMPT").unwrap_or_default();
    if !identity::is_lower_hex(&revision, 40) || !digits(&run, 32) || !digits(&attempt, 8) {
        return Err("Hosted Hexagon preparation requires exact source and run identities.".into());
    }
    Ok((revision, run, attempt))
}

fn plain(path: &Path) -> Result<()> {
    if !path.is_absolute() || path.to_string_lossy().contains(['%', '\0']) {
        return Err("Use expanded absolute Hexagon paths.".into());
    }
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        if !matches!(path.components().next(),Some(Component::Prefix(prefix)) if matches!(prefix.kind(),Prefix::Disk(_)))
            || path
                .to_string_lossy()
                .get(2..)
                .is_none_or(|tail| tail.contains(':'))
        {
            return Err(
                "Use expanded local drive paths without device syntax or alternate streams.".into(),
            );
        }
    }
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) if is_redirect(&metadata) => {
                return Err("Hexagon paths cannot redirect.".into());
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(())
}

fn hold(path: &Path, maximum: u64) -> Result<(File, String)> {
    plain(path)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        options.share_mode(1).custom_flags(0x0020_0000);
    }
    let mut file = options.open(path).map_err(|error| error.to_string())?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file()
        || is_redirect(&metadata)
        || metadata.len() == 0
        || metadata.len() > maximum
    {
        return Err("Hexagon input exceeds its fixed file bound.".into());
    }
    let digest = hash_reader(&mut file, metadata.len(), &mut vec![0; 65_536], &mut || {
        Ok(())
    })
    .map_err(|error| error.to_string())?;
    file.rewind().map_err(|error| error.to_string())?;
    Ok((file, digest))
}

fn write_json(path: &Path, value: &Value) -> Result<()> {
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    serde_json::to_writer_pretty(&mut file, value).map_err(|error| error.to_string())?;
    file.write_all(b"\n")
        .and_then(|()| file.sync_all())
        .map_err(|error| error.to_string())
}

impl Context {
    fn new(repo: PathBuf, output: PathBuf) -> Result<Self> {
        let (revision, run, attempt) = hosted()?;
        plain(&repo)?;
        if !repo.join("tools/hexagon").is_dir() {
            return Err("Select the Runner repository root.".into());
        }
        validate_external_path(&output).map_err(|error| error.to_string())?;
        fs::create_dir(&output)
            .map_err(|_| "Hexagon output must be new external storage.".to_string())?;
        Ok(Self {
            repo,
            output,
            revision,
            run,
            attempt,
            holds: Vec::new(),
            environment: BTreeMap::from([("VSCMD_SKIP_SENDTELEMETRY".into(), "1".into())]),
        })
    }
    fn digest(&mut self, path: &Path, maximum: u64) -> Result<String> {
        let (file, digest) = hold(path, maximum)?;
        self.holds.push(file);
        Ok(digest)
    }
    fn json(&mut self, path: &Path, maximum: u64) -> Result<Value> {
        let (mut file, _) = hold(path, maximum)?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        self.holds.push(file);
        crate::hexagon_policy::run(
            json!({"operation":"json", "json":String::from_utf8(bytes).map_err(|error| error.to_string())?}),
        )
    }
    fn run(
        &self,
        executable: &OsStr,
        args: &[OsString],
        seconds: u64,
        log: &Path,
    ) -> Result<startup_process::ProcessOutput> {
        let output = startup_process::run_strict(
            executable,
            args,
            &self.output,
            &self.environment,
            Duration::from_secs(seconds),
            log,
        )?;
        if output.code != 0 {
            return Err(format!(
                "Hexagon preparation failed; inspect {}",
                log.display()
            ));
        }
        Ok(output)
    }
    fn rustc(&mut self) -> Result<(PathBuf, String)> {
        let rustup = std::env::var_os("RUSTUP_HOME")
            .map_or_else(
                || std::env::var_os("USERPROFILE").map(|path| PathBuf::from(path).join(".rustup")),
                |path| Some(path.into()),
            )
            .ok_or("Rustup home is unavailable.")?;
        let rustc = rustup
            .join("toolchains")
            .join(TOOLCHAIN)
            .join("bin/rustc.exe");
        self.digest(&rustc, 256 * 1024 * 1024)?;
        let compiler = self.run(
            rustc.as_os_str(),
            &["-vV".into()],
            20,
            &self.output.join("compiler.txt"),
        )?;
        if !compiler
            .stdout
            .lines()
            .any(|line| line.trim_end() == "host: aarch64-pc-windows-msvc")
        {
            return Err("Pinned native compiler host differs.".into());
        }
        Ok((rustc, identity::sha256(compiler.stdout.as_bytes())))
    }
    fn input_set(&mut self, root: &Path, names: &[&str]) -> Result<Vec<Value>> {
        names
            .iter()
            .map(|name| {
                Ok(json!({"Name":name,"Sha256":self.digest(&root.join(name),4*1024*1024)?}))
            })
            .collect()
    }
    fn executable(&mut self, path: &Path, name: &str) -> Result<Value> {
        self.image(path, name, 64 * 1024 * 1024)
    }
    fn image(&mut self, path: &Path, name: &str, maximum_bytes: u64) -> Result<Value> {
        startup_cache::assert_arm64(path)?;
        let bytes = fs::metadata(path).map_err(|error| error.to_string())?.len();
        Ok(json!({"Name":name,"Bytes":bytes,"Sha256":self.digest(path,maximum_bytes)?}))
    }
}

fn compile_arguments(manifest: &Path) -> Vec<OsString> {
    [
        "--edition=2024",
        "--target",
        TARGET,
        "-C",
        "opt-level=2",
        "-C",
        "overflow-checks=yes",
        "-C",
        "link-arg=/MANIFEST:EMBED",
        "-C",
    ]
    .map(OsString::from)
    .into_iter()
    .chain([
        OsString::from(format!("link-arg=/MANIFESTINPUT:{}", manifest.display())),
        "-D".into(),
        "warnings".into(),
    ])
    .collect()
}

#[expect(
    clippy::too_many_lines,
    reason = "The fixed hosted recipe retains all compiler/source/output identities through its three independent controls."
)]
fn dispatch(mut context: Context) -> Result<Value> {
    let manifest = context.repo.join("tools/hexagon/diagnostic.manifest");
    let manifest_digest = context.digest(&manifest, 65_536)?;
    let lock_digest = context.digest(
        &context.repo.join("config/hexagon-dispatch.lock.json"),
        1024 * 1024,
    )?;
    let source = context.repo.join("tools/hexagon/dispatch_fixture.rs");
    let source_digest = context.digest(&source, 1024 * 1024)?;
    let (rustc, compiler_digest) = context.rustc()?;
    let bundle = context.output.join("bundle");
    fs::create_dir(&bundle).map_err(|error| error.to_string())?;
    let compiled = context.output.join("hexagon-dispatch-fixture.exe");
    let mut args = compile_arguments(&manifest);
    args.extend([
        source.into_os_string(),
        "-o".into(),
        compiled.clone().into_os_string(),
    ]);
    context.run(
        rustc.as_os_str(),
        &args,
        120,
        &context.output.join("dispatch-build.txt"),
    )?;
    let exe = bundle.join("hexagon-dispatch-fixture.exe");
    fs::copy(compiled, &exe).map_err(|error| error.to_string())?;
    let check = context.run(
        exe.as_os_str(),
        &["--self-test".into()],
        15,
        &context.output.join("dispatch-self-test.json"),
    )?;
    if check.stdout.len() > 65_536 || check.stderr.len() > 65_536 {
        return Err("Hexagon self-check exceeded its output contract.".into());
    }
    let exe_identity = context.executable(&exe, "hexagon-dispatch-fixture.exe")?;
    let receipt = json!({"SchemaVersion":1,"RunnerRevision":context.revision,"RunId":context.run,"RunAttempt":context.attempt,"RustToolchain":TOOLCHAIN,"Target":TARGET,"HostedBuild":true,"BuildEnvironment":context.environment,"CompilerIdentitySha256":compiler_digest,"SourceSha256":source_digest,"ManifestSha256":manifest_digest,"LockSha256":lock_digest,"Executable":exe_identity,"DeviceCodeLoaded":false,"DeviceExecutionQualified":false});
    write_json(&bundle.join("fixture-build.json"), &receipt)?;
    let native = context.output.join("native-rpc");
    let native_bundle = context.output.join("native-rpc-bundle");
    fs::create_dir(&native)
        .and_then(|()| fs::create_dir(&native_bundle))
        .map_err(|error| error.to_string())?;
    let native_source = context.repo.join("tools/hexagon/native_rpc");
    let sources = context.input_set(&native_source, NATIVE_INPUTS)?;
    let library = native.join("libhexagon_native_rpc.rlib");
    let query = native.join("native-hexagon-query.exe");
    let contract = native.join("native-hexagon-contract.exe");
    for (name, extra) in [
        (
            "library",
            vec![
                "--crate-type".into(),
                "lib".into(),
                "--crate-name".into(),
                "hexagon_native_rpc".into(),
                native_source.join("lib.rs").into_os_string(),
                "-o".into(),
                library.clone().into_os_string(),
            ],
        ),
        (
            "query",
            vec![
                native_source.join("query.rs").into_os_string(),
                "--extern".into(),
                format!("hexagon_native_rpc={}", library.display()).into(),
                "-o".into(),
                query.clone().into_os_string(),
            ],
        ),
        (
            "contract",
            vec![
                "--test".into(),
                native_source.join("lib.rs").into_os_string(),
                "-o".into(),
                contract.clone().into_os_string(),
            ],
        ),
    ] {
        let mut args = compile_arguments(&manifest);
        args.extend(extra);
        context.run(
            rustc.as_os_str(),
            &args,
            120,
            &native.join(format!("{name}-build.txt")),
        )?;
    }
    context.run(
        query.as_os_str(),
        &["--self-test".into()],
        15,
        &native.join("self-test.json"),
    )?;
    context.run(
        contract.as_os_str(),
        &[],
        15,
        &native.join("contract-tests.txt"),
    )?;
    let exe_identity = context.executable(&query, "native-hexagon-query.exe")?;
    let native_receipt = json!({"SchemaVersion":1,"RunnerRevision":context.revision,"RunId":context.run,"RunAttempt":context.attempt,"RustToolchain":TOOLCHAIN,"Target":TARGET,"HostedBuild":true,"BuildEnvironment":context.environment,"CompilerIdentitySha256":compiler_digest,"SourceFiles":sources,"ManifestSha256":manifest_digest,"LockSha256":lock_digest,"Executable":exe_identity,"ExecutableSha256":exe_identity["Sha256"],"DeviceCodeLoaded":false,"DspCodeLoaded":false,"ProfileEligible":false});
    write_json(&native.join("native-build.json"), &native_receipt)?;
    fs::copy(query, native_bundle.join("native-hexagon-query.exe"))
        .and_then(|_| {
            fs::copy(
                native.join("native-build.json"),
                native_bundle.join("native-build.json"),
            )
        })
        .map_err(|error| error.to_string())?;
    let packed = context.output.join("packed-projection");
    fs::create_dir(&packed).map_err(|error| error.to_string())?;
    let packed_source = context.repo.join("tools/hexagon/packed_projection.rs");
    let source_digest = context.digest(&packed_source, 1024 * 1024)?;
    let packed_test = packed.join("packed-projection-contract.exe");
    let packed_inputs = context.input_set(
        &context.repo.join("tools/hexagon"),
        projection_build::INPUTS,
    )?;
    let mut args: Vec<OsString> = [
        "--edition=2024",
        "--target",
        TARGET,
        "--test",
        "-C",
        "opt-level=2",
        "-C",
        "overflow-checks=yes",
        "-D",
        "warnings",
    ]
    .map(Into::into)
    .into();
    args.extend([
        packed_source.into_os_string(),
        "-o".into(),
        packed_test.clone().into_os_string(),
    ]);
    context.run(rustc.as_os_str(), &args, 120, &packed.join("build.txt"))?;
    context.run(
        packed_test.as_os_str(),
        &[],
        30,
        &packed.join("contract-tests.txt"),
    )?;
    let exe_identity = context.executable(&packed_test, "packed-projection-contract.exe")?;
    write_json(
        &packed.join("build.json"),
        &json!({"SchemaVersion":1,"RunnerRevision":context.revision,"RunId":context.run,"BuildEnvironment":context.environment,"CompilerIdentitySha256":compiler_digest,"Target":TARGET,"SourceSha256":source_digest,"SourceFiles":packed_inputs,"ExecutableSha256":exe_identity["Sha256"],"ExecutionBackend":"CPU-control-only","DspCodeLoaded":false,"ProfileEligible":false}),
    )?;
    projection_build::build(&mut context, &rustc, &manifest, &compiler_digest, &library)?;
    Ok(receipt)
}

#[expect(
    clippy::too_many_lines,
    reason = "The source-bound extractor recipe retains exact dependency artifacts, compiler images and model-free controls together."
)]
fn packed_artifact(mut context: Context, gguf: &Path) -> Result<Value> {
    const PIN: &str = "7aca2251f09acb345d4afb53d5ce411f81017745";
    plain(gguf)?;
    let git = startup_process::resolve(OsStr::new("git.exe"))?;
    let git_query = |arguments: &[&str], log: &str| -> Result<String> {
        let mut args = vec![OsString::from("-C"), gguf.as_os_str().to_owned()];
        args.extend(arguments.iter().map(OsString::from));
        Ok(context
            .run(git.as_os_str(), &args, 10, &context.output.join(log))?
            .stdout
            .trim()
            .to_owned())
    };
    let root = git_query(&["rev-parse", "--show-toplevel"], "gguf-root.txt")?;
    if !Path::new(&root)
        .as_os_str()
        .to_string_lossy()
        .replace('/', "\\")
        .eq_ignore_ascii_case(&gguf.as_os_str().to_string_lossy().replace('/', "\\"))
        || git_query(&["rev-parse", "HEAD"], "gguf-revision.txt")? != PIN
        || !git_query(&["status", "--porcelain"], "gguf-status.txt")?.is_empty()
    {
        return Err("Prism must be the exact clean pinned GGUF checkout root.".into());
    }
    let (rustc, compiler_digest) = context.rustc()?;
    let bin = rustc.parent().ok_or("Missing compiler directory.")?;
    let cargo = bin.join("cargo.exe");
    let linker = bin
        .parent()
        .ok_or("Missing toolchain root.")?
        .join("lib/rustlib/aarch64-pc-windows-msvc/bin/rust-lld.exe");
    let mut images = Vec::new();
    for path in [&cargo, &rustc, &linker] {
        let name = path
            .file_name()
            .and_then(OsStr::to_str)
            .ok_or("Invalid tool image name.")?;
        images.push(context.image(path, name, 256 * 1024 * 1024)?);
    }
    let source = context.repo.join("tools/hexagon/packed_artifact.rs");
    let source_digest = context.digest(&source, 4 * 1024 * 1024)?;
    let manifest = context.repo.join("tools/hexagon/diagnostic.manifest");
    let manifest_digest = context.digest(&manifest, 65_536)?;
    let lock_digest = context.digest(&gguf.join("Cargo.lock"), 16 * 1024 * 1024)?;
    let target = context.output.join("target");
    let examples = gguf.join("tools/gguf/examples");
    fs::create_dir_all(&examples).map_err(|error| error.to_string())?;
    let example = examples.join("packed_artifact.rs");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&example)
        .map_err(|error| error.to_string())?;
    let mut source_file = File::open(&source).map_err(|error| error.to_string())?;
    std::io::copy(&mut source_file, &mut file).map_err(|error| error.to_string())?;
    drop(file);
    if context.digest(&example, 4 * 1024 * 1024)? != source_digest {
        return Err("Temporary extractor differs from reviewed source.".into());
    }
    context.environment.extend(BTreeMap::from([
        ("RUSTC".into(), rustc.to_string_lossy().into_owned()),
        ("RUSTC_WRAPPER".into(), String::new()),
        ("RUSTC_WORKSPACE_WRAPPER".into(), String::new()),
        (
            "CARGO_ENCODED_RUSTFLAGS".into(),
            [
                "-C".to_owned(),
                format!("linker={}", linker.display()),
                "-C".into(),
                "linker-flavor=lld-link".into(),
                "-C".into(),
                "link-arg=/MANIFEST:EMBED".into(),
                "-C".into(),
                format!("link-arg=/MANIFESTINPUT:{}", manifest.display()),
            ]
            .join("\u{1f}"),
        ),
        (
            "CARGO_TARGET_AARCH64_PC_WINDOWS_MSVC_LINKER".into(),
            linker.to_string_lossy().into_owned(),
        ),
    ]));
    let arguments: Vec<OsString> = vec![
        "--locked".into(),
        "--release".into(),
        "--no-default-features".into(),
        "--manifest-path".into(),
        gguf.join("Cargo.toml").into_os_string(),
        "--target-dir".into(),
        target.clone().into_os_string(),
        "-p".into(),
        "bonsai-gguf".into(),
        "--example".into(),
        "packed_artifact".into(),
    ];
    let mut build_arguments = vec![
        "build".into(),
        "--message-format=json-render-diagnostics".into(),
    ];
    build_arguments.extend(arguments.clone());
    let build = context.run(
        cargo.as_os_str(),
        &build_arguments,
        180,
        &context.output.join("extractor-build.txt"),
    )?;
    let mut executables = Vec::new();
    let mut dependencies = Vec::new();
    for line in build.stdout.lines().filter(|line| !line.is_empty()) {
        let message: Value = serde_json::from_str(line).map_err(|error| error.to_string())?;
        if message["reason"] != "compiler-artifact" {
            continue;
        }
        let kinds = message["target"]["kind"]
            .as_array()
            .ok_or("Cargo omitted target kind.")?;
        if message["target"]["name"] == "packed_artifact" && kinds.contains(&json!("example")) {
            if Path::new(
                message["manifest_path"]
                    .as_str()
                    .ok_or("Cargo omitted manifest path.")?,
            ) != gguf.join("tools/gguf/Cargo.toml")
            {
                return Err("Cargo compiled an unrelated extractor.".into());
            }
            executables.push(PathBuf::from(
                message["executable"]
                    .as_str()
                    .ok_or("Cargo omitted extractor executable.")?,
            ));
        }
        if kinds.contains(&json!("lib"))
            && [json!("bonsai_gguf"), json!("sha2")].contains(&message["target"]["name"])
        {
            for filename in message["filenames"]
                .as_array()
                .ok_or("Cargo omitted dependency files.")?
            {
                let path = PathBuf::from(filename.as_str().ok_or("Invalid Cargo artifact path.")?);
                if !path.starts_with(&target) {
                    return Err("Dependency artifact escaped owned target storage.".into());
                }
                dependencies.push(json!({"Crate":message["target"]["name"],"Name":path.file_name().map(|name| name.to_string_lossy()),"Sha256":context.digest(&path,256*1024*1024)?}));
            }
        }
    }
    if executables.len() != 1 || !executables[0].starts_with(&target) {
        return Err("The extractor executable is ambiguous or escaped its target.".into());
    }
    startup_cache::assert_arm64(&executables[0])?;
    let bundle = context.output.join("bundle");
    fs::create_dir(&bundle).map_err(|error| error.to_string())?;
    let exe = bundle.join("hexagon-packed-artifact.exe");
    fs::copy(&executables[0], &exe).map_err(|error| error.to_string())?;
    context.run(
        exe.as_os_str(),
        &["--self-test".into()],
        15,
        &context.output.join("self-test.txt"),
    )?;
    let mut test_arguments = vec!["test".into()];
    test_arguments.extend(arguments);
    test_arguments.extend([
        "--".into(),
        "--exact".into(),
        "tests::fixed_transform_sign_selection_and_malformed_control".into(),
    ]);
    let tested = context.run(
        cargo.as_os_str(),
        &test_arguments,
        120,
        &context.output.join("contract-tests.txt"),
    )?;
    if !tested.stdout.contains("test result: ok. 1 passed;") {
        return Err("The extraction contract must execute and pass exactly one named test.".into());
    }
    context.run(
        git.as_os_str(),
        &[
            "-C".into(),
            gguf.as_os_str().to_owned(),
            "diff".into(),
            "--exit-code".into(),
        ],
        10,
        &context.output.join("gguf-final-diff.txt"),
    )?;
    let executable = context.executable(&exe, "hexagon-packed-artifact.exe")?;
    let receipt = json!({"SchemaVersion":1,"RunnerRevision":context.revision,"RunId":context.run,"RunAttempt":context.attempt,"HostedBuild":true,"Target":TARGET,"RustToolchain":TOOLCHAIN,"GgufRevision":PIN,"GgufDefaultFeatures":false,"LinkerFlavor":"lld-link","ToolImages":images,"DependencyArtifacts":dependencies,"CompilerIdentitySha256":compiler_digest,"SourceSha256":source_digest,"AddedExampleSourceSha256":source_digest,"PinnedTrackedSourceUnchanged":true,"ManifestSha256":manifest_digest,"DependencyCargoLockSha256":lock_digest,"Executable":executable,"ModelLoaded":false,"DeviceCodeLoaded":false,"ProfileEligible":false});
    write_json(&bundle.join("artifact-build.json"), &receipt)?;
    Ok(receipt)
}

fn hold_pinned(path: &Path, bytes: u64, expected: &str) -> Result<File> {
    let (file, actual) = hold(path, bytes)?;
    if file.metadata().map_err(|error| error.to_string())?.len() != bytes || actual != expected {
        return Err("SDK input changed before acquiring its immutable file guard.".into());
    }
    Ok(file)
}

fn receive(context: &mut Context, pin: &Value, name: &str, seconds: u64) -> Result<PathBuf> {
    if name.is_empty()
        || name.len() > 128
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
    {
        return Err("Invalid bounded SDK filename.".into());
    }
    let bytes = pin["bytes"]
        .as_u64()
        .filter(|bytes| *bytes != 0)
        .ok_or("Invalid SDK byte pin.")?;
    let digest = pin["sha256"]
        .as_str()
        .filter(|digest| identity::is_lower_hex(digest, 64))
        .ok_or("Invalid SDK digest pin.")?;
    let url = pin["url"]
        .as_str()
        .filter(|url| url.starts_with("https://") && !url.contains(['\r', '\n']))
        .ok_or("SDK inputs require HTTPS.")?;
    let path = context.output.join(name);
    crate::startup_download::receive_with_timeout(
        url,
        &path,
        bytes,
        digest,
        &context.output,
        Duration::from_secs(seconds),
    )?;
    context.holds.push(hold_pinned(&path, bytes, digest)?);
    Ok(path)
}

fn sdk_inventory(mut context: Context) -> Result<Value> {
    let clock = Instant::now();
    let review = context.output.join("review");
    fs::create_dir(&review).map_err(|error| error.to_string())?;
    let pin_path = context.repo.join("tools/hexagon/sdk-inventory.lock.json");
    let source = context.repo.join("tools/hexagon/sdk_inventory.py");
    let pin = context.json(&pin_path, 1024 * 1024)?;
    if pin["schemaVersion"] != 1
        || pin["generatorExecutionAllowed"] != false
        || pin["deviceExecutionAllowed"] != false
        || pin["profileEligible"] != false
        || pin["asset"]["filename"] != "hexagon-sdk-v6.7.0.0-arm64-wos.tar.xz"
        || pin["asset"]["bytes"] != 900_461_020_u64
        || pin["asset"]["sha256"]
            != "9eb256cc7b102f721365151a470a33980069a18b9209e1d56a6d2fa430369759"
        || pin["asset"]["url"]
            != "https://github.com/snapdragon-toolchain/hexagon-sdk/releases/download/v6.7.0.0/hexagon-sdk-v6.7.0.0-arm64-wos.tar.xz"
    {
        return Err("Reviewed SDK archive or data-only boundary changed.".into());
    }
    let source_digest = context.digest(&source, 4 * 1024 * 1024)?;
    let pin_digest = context.digest(&pin_path, 1024 * 1024)?;
    let supervisor = context.digest(
        &context.repo.join("tools/bonsai/src/hexagon_tools.rs"),
        4 * 1024 * 1024,
    )?;
    let functional = context.digest(
        &context.repo.join("tools/hexagon/sdk_functional_review.py"),
        4 * 1024 * 1024,
    )?;
    let mut report = json!({"SchemaVersion":1,"Kind":"hexagon-hosted-sdk-inventory","RunnerRevision":context.revision,"RunId":context.run,"RunAttempt":context.attempt,"HostedOnly":true,"Outcome":"pending","Stage":"reviewed-source-inputs","ElapsedMilliseconds":0,"SourceSha256":source_digest,"SupervisorSha256":supervisor,"PinSha256":pin_digest,"FunctionalReviewSourceSha256":functional,"ReviewedInputs":[],"Archive":pin["asset"],"InventoryReceiptSha256":null,"PythonExecutableSha256":null,"SdkCodeExecuted":false,"GeneratorExecuted":false,"GeneratedSourceUploaded":false,"ModelLoaded":false,"DeviceCodeLoaded":false,"ProfileEligible":false,"LicenseReview":"pending","GeneratorInvocationReviewed":false});
    let result = (|| -> Result<()> {
        let inputs = pin["reviewedInputs"]
            .as_array()
            .filter(|inputs| inputs.len() <= 32)
            .ok_or("Invalid SDK reviewed input set.")?;
        for input in inputs {
            let name = input["name"].as_str().ok_or("SDK input name missing.")?;
            if input["bytes"]
                .as_u64()
                .is_none_or(|bytes| bytes == 0 || bytes > 1024 * 1024)
            {
                return Err("SDK reviewed source exceeds its fixed bound.".into());
            }
            receive(&mut context, input, name, 30)?;
            report["ReviewedInputs"].as_array_mut().ok_or("Invalid SDK report.")?.push(json!({"Name":name,"Url":input["url"],"Bytes":input["bytes"],"Sha256":input["sha256"]}));
        }
        report["Stage"] = json!("sdk-archive-download");
        let archive_name = pin["asset"]["filename"]
            .as_str()
            .ok_or("SDK archive name missing.")?;
        let archive = receive(&mut context, &pin["asset"], archive_name, 240)?;
        let python = startup_process::resolve(OsStr::new("python.exe"))?;
        report["PythonExecutableSha256"] = json!(context.digest(&python, 256 * 1024 * 1024)?);
        report["Stage"] = json!("sdk-data-only-inventory");
        let inventory = review.join("sdk-inventory.json");
        let output = context.run(
            python.as_os_str(),
            &[
                "-B".into(),
                source.into_os_string(),
                "--archive".into(),
                archive.into_os_string(),
                "--pin".into(),
                pin_path.into_os_string(),
                "--output".into(),
                inventory.clone().into_os_string(),
                "--public-input-directory".into(),
                context.output.clone().into_os_string(),
            ],
            360,
            &context.output.join("inventory.txt"),
        )?;
        if output.stdout.len() > 4096 || output.stderr.len() > 65_536 {
            return Err("SDK inventory exceeded its fixed process output contract.".into());
        }
        report["InventoryReceiptSha256"] = json!(context.digest(&inventory, 1024 * 1024)?);
        report["Stage"] = json!("sdk-data-only-inventory-complete");
        report["Outcome"] = json!("inventoried-generation-pending");
        Ok(())
    })();
    report["ElapsedMilliseconds"] = json!(clock.elapsed().as_millis());
    write_json(&review.join("host.json"), &report)?;
    result.map(|()| report)
}

/// Prepare hosted Hexagon controls with native process ownership and immutable inputs.
///
/// # Errors
/// Rejects owner-device execution, mutable identities, unsafe paths, changed pins,
/// failed bounded children and incomplete source-bound build receipts.
pub fn run(request: Value) -> Result<Value> {
    match serde_json::from_value(request).map_err(|error| error.to_string())? {
        Request::BuildDispatch {
            repository_directory,
            output_directory,
        } => dispatch(Context::new(repository_directory, output_directory)?),
        Request::BuildPackedArtifact {
            repository_directory,
            gguf_source_directory,
            output_directory,
        } => packed_artifact(
            Context::new(repository_directory, output_directory)?,
            &gguf_source_directory,
        ),
        Request::SdkInventory {
            repository_directory,
            output_directory,
        } => sdk_inventory(Context::new(repository_directory, output_directory)?),
        #[cfg(windows)]
        Request::Device { request } => device::supervise(&request),
        #[cfg(windows)]
        Request::DeviceWorker { request } => device::worker(&request),
        #[cfg(windows)]
        Request::ModelDevice { request } => model_device::run(&request),
        Request::Preflight {
            package_path,
            output_path,
            maximum_seconds,
        } => preflight::supervise(package_path.as_deref(), output_path, maximum_seconds),
        Request::PreflightWorker { package_path } => preflight::inventory(package_path.as_deref()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hosted_operations_reject_without_creating_output() {
        if hosted().is_ok() {
            return;
        }
        for operation in ["build-dispatch", "build-packed-artifact", "sdk-inventory"] {
            assert!(run(json!({"operation":operation,"repositoryDirectory":"relative","outputDirectory":"relative","ggufSourceDirectory":"relative"})).is_err());
        }
    }
    #[test]
    fn retained_sdk_inputs_recheck_exact_pins() -> Result<()> {
        let directory = std::env::temp_dir().join(format!(
            "bonsai-hexagon-sdk-guard-test-{}",
            startup_process::unique()
        ));
        fs::create_dir(&directory).map_err(|error| error.to_string())?;
        let path = directory.join("input.bin");
        fs::write(&path, [1, 2, 3]).map_err(|error| error.to_string())?;
        let expected = identity::sha256(&[1, 2, 3]);
        drop(hold_pinned(&path, 3, &expected)?);
        assert!(hold_pinned(&path, 4, &expected).is_err());
        fs::write(&path, [3, 2, 1]).map_err(|error| error.to_string())?;
        assert!(hold_pinned(&path, 3, &expected).is_err());
        fs::remove_dir_all(&directory).map_err(|error| error.to_string())?;
        Ok(())
    }

    #[test]
    fn identities_reject_coercions_and_extra_fields() {
        assert!(!digits("123x", 32));
        assert!(!digits("", 32));
        assert!(serde_json::from_value::<Request>(json!({"operation":"build-dispatch","repositoryDirectory":"C:/repo","outputDirectory":"C:/out","deviceExecutionAllowed":true})).is_err());
    }
}
