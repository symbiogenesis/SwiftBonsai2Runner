//! Reviewed publisher source candidate, with model-free required-headless probes.

use std::time::{Duration, Instant};

use super::{
    BTreeMap, OsString, Path, PathBuf, Value, count, download, environment, external, extract_zip,
    fs, hosted_context, image, json, ordinary, read_json, save, save_evidence, scratch,
    stage_output, text, text_arguments,
};

const TOKENS: &[&str] = &[
    "ACTIONS_RUNTIME_TOKEN",
    "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
    "GITHUB_TOKEN",
    "GH_TOKEN",
    "ACTIONS_RUNNER_INPUT_TOKEN",
];
const TARGETS: &[&str] = &[
    "llama-completion",
    "llama-server",
    "llama-bench",
    "test-headless-argv",
];

fn cmake_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn extraction_arguments(
    archive: &Path,
    destination: &Path,
) -> crate::Result<(PathBuf, Vec<OsString>, Vec<OsString>)> {
    let value = archive
        .to_str()
        .ok_or("LLVM archive path must be Unicode.")?;
    if !value.ends_with(".tar.zst") {
        return Err("Expected the pinned LLVM tar.zst archive.".into());
    }
    let tar = PathBuf::from(
        value
            .strip_suffix(".zst")
            .ok_or("Missing LLVM compression suffix.")?,
    );
    if tar.exists() {
        return Err("LLVM decompression output must be new.".into());
    }
    Ok((
        tar.clone(),
        vec![
            "--decompress".into(),
            "--long=31".into(),
            "-o".into(),
            tar.clone().into_os_string(),
            archive.as_os_str().to_owned(),
        ],
        vec![
            "-xf".into(),
            tar.into_os_string(),
            "-C".into(),
            destination.as_os_str().to_owned(),
        ],
    ))
}

fn candidates_unchanged(source: &Path, manifest: &Value, patched: bool) -> crate::Result<()> {
    for file in manifest["files"]
        .as_array()
        .ok_or("Missing reviewed source inventory.")?
    {
        let relative = Path::new(text(file, "path")?);
        if relative
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
        {
            return Err("Reviewed patch source path is unsafe.".into());
        }
        let path = source.join(relative);
        let digest = &file[if patched {
            "candidateSha256"
        } else {
            "baseSha256"
        }];
        if let Some(expected) = digest.as_str() {
            if crate::startup_cache::digest_file(&path)? != expected {
                return Err("Reviewed source identity changed.".into());
            }
        } else if path.exists() {
            return Err("New patch target already exists.".into());
        }
    }
    Ok(())
}

fn sdk_environment(
    candidate: &Candidate<'_>,
    llvm: &Path,
) -> crate::Result<(BTreeMap<String, String>, String, String, PathBuf)> {
    let program_files = PathBuf::from(
        std::env::var_os("ProgramFiles(x86)").ok_or("Windows x86 Program Files is unavailable.")?,
    );
    external(&program_files)?;
    let vswhere = program_files.join("Microsoft Visual Studio/Installer/vswhere.exe");
    let result = candidate.tool(
        &vswhere,
        &[
            "-latest".into(),
            "-products".into(),
            "*".into(),
            "-requires".into(),
            "Microsoft.VisualStudio.Component.VC.Tools.ARM64".into(),
            "-property".into(),
            "installationPath".into(),
        ],
        "visual-studio.txt",
        30,
    )?;
    let visual_studio = PathBuf::from(result.trim());
    external(&visual_studio)?;
    let vc_version = fs::read_to_string(
        visual_studio.join("VC/Auxiliary/Build/Microsoft.VCToolsVersion.default.txt"),
    )
    .map_err(|error| error.to_string())?
    .trim()
    .to_owned();
    if vc_version.is_empty()
        || !vc_version
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'.')
    {
        return Err("Invalid installed Visual C++ tools version.".into());
    }
    let vc = visual_studio.join("VC/Tools/MSVC").join(&vc_version);
    let sdk = program_files.join("Windows Kits/10");
    let mut versions = Vec::new();
    for entry in fs::read_dir(sdk.join("Include")).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "SDK version is not Unicode.")?;
        let version = name
            .split('.')
            .map(str::parse::<u32>)
            .collect::<Result<Vec<_>, _>>();
        if let Ok(version) = version
            && version.len() == 4
            && version[..2] == [10, 0]
            && [
                format!("Lib/{name}/ucrt/arm64/ucrt.lib"),
                format!("Lib/{name}/um/arm64/kernel32.lib"),
                format!("Bin/{name}/arm64/rc.exe"),
                format!("Bin/{name}/arm64/mt.exe"),
            ]
            .iter()
            .all(|path| sdk.join(path).is_file())
        {
            versions.push((version, name));
        }
    }
    versions.sort_unstable();
    let (_, sdk_version) = versions
        .pop()
        .ok_or("An installed ARM64 Windows SDK is required.")?;
    let bin = sdk.join("Bin").join(&sdk_version).join("arm64");
    external(&bin)?;
    let include = format!(
        "{}/include;{}/Include/{sdk_version}/ucrt;{}/Include/{sdk_version}/shared;{}/Include/{sdk_version}/um",
        vc.display(),
        sdk.display(),
        sdk.display(),
        sdk.display()
    );
    let lib = format!(
        "{}/lib/arm64;{}/Lib/{sdk_version}/ucrt/arm64;{}/Lib/{sdk_version}/um/arm64",
        vc.display(),
        sdk.display(),
        sdk.display()
    );
    let path = format!(
        "{};{};{};{}",
        llvm.display(),
        vc.join("bin/Hostarm64/arm64").display(),
        bin.display(),
        std::env::var("PATH").map_err(|error| error.to_string())?
    );
    Ok((
        BTreeMap::from([
            ("INCLUDE".into(), include),
            ("LIB".into(), lib),
            ("PATH".into(), path),
        ]),
        vc_version,
        sdk_version,
        bin,
    ))
}

struct Candidate<'a> {
    working: &'a Path,
    output: &'a Path,
    environment: BTreeMap<String, String>,
    started: Instant,
}

impl Candidate<'_> {
    fn remaining(&self) -> crate::Result<Duration> {
        crate::startup_cancel::checkpoint()?;
        Duration::from_mins(35)
            .checked_sub(self.started.elapsed())
            .filter(|duration| !duration.is_zero())
            .ok_or_else(|| {
                "Hosted source-candidate preparation exceeded its aggregate deadline.".into()
            })
    }

    fn tool(
        &self,
        executable: &Path,
        arguments: &[OsString],
        name: &str,
        seconds: u64,
    ) -> crate::Result<String> {
        let result = crate::startup_process::run_strict_with_checkpoint(
            executable.as_os_str(),
            arguments,
            self.working,
            &self.environment,
            Duration::from_secs(seconds).min(self.remaining()?),
            &self.working.join(name),
            || self.remaining().map(|_| ()),
        );
        let logs = self.output.join("logs");
        fs::create_dir_all(&logs).map_err(|error| error.to_string())?;
        // The shared tool boundary persists a diagnostic even on timeout/cancellation.
        save_evidence(
            self.working,
            &logs,
            &json!({"Log":name,"Completed":result.is_ok()}),
            &[name],
        )?;
        let result = result?;
        for (stream, value) in [("stdout", &result.stdout), ("stderr", &result.stderr)] {
            let tail: String = value
                .chars()
                .rev()
                .take(16_384)
                .collect::<String>()
                .chars()
                .rev()
                .collect();
            fs::write(logs.join(format!("{name}.{stream}.txt")), tail)
                .map_err(|error| error.to_string())?;
        }
        if result.code != 0 {
            return Err(format!("Hosted candidate tool failed: {name}"));
        }
        Ok(result.stdout.trim().into())
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "One reviewed candidate transaction retains compiler, import, packaging and probe evidence together."
)]
pub(super) fn build(repository: &Path, output: &Path) -> crate::Result<Value> {
    let started = Instant::now();
    let environment = environment();
    hosted_context(cfg!(all(windows, target_arch = "aarch64")), &environment)?;
    if TOKENS.iter().any(|name| std::env::var_os(name).is_some()) {
        return Err(
            "Use a credential-free preparation child before any supervised worker probe.".into(),
        );
    }
    let manifest = read_json(&repository.join("docs/runtime-candidates/headless-ucrt-argv.json"))?;
    let prior =
        read_json(&repository.join("docs/runtime-candidates/headless-ucrt-argv.evidence.json"))?;
    let patch_name = text(&manifest["patch"], "filename")?;
    if Path::new(patch_name).components().count() != 1 {
        return Err("Reviewed patch filename is unsafe.".into());
    }
    let reviewed_patch = repository.join("docs/runtime-candidates").join(patch_name);
    let patch_guard = super::input_file(&reviewed_patch)?;
    if patch_guard
        .metadata()
        .map_err(|error| error.to_string())?
        .len()
        != count(&manifest["patch"], "bytes")?
        || crate::startup_cache::digest_file(&reviewed_patch)?
            != text(&manifest["patch"], "sha256")?
    {
        return Err("Reviewed headless patch identity changed.".into());
    }
    stage_output(output, false)?;
    let working = scratch("HostedHeadless")?;
    let mut candidate = Candidate {
        working: &working,
        output,
        environment: BTreeMap::new(),
        started,
    };
    let mut receipt = json!({"SchemaVersion":1, "Scope":"hosted-headless-publisher-build-and-model-free-startup-only",
        "RunnerRevision":environment.get("GITHUB_SHA"), "RunId":environment.get("GITHUB_RUN_ID"), "RunAttempt":environment.get("GITHUB_RUN_ATTEMPT"),
        "ImageOS":environment.get("ImageOS"), "ImageVersion":environment.get("ImageVersion"), "Source":manifest["base"], "Patch":manifest["patch"],
        "Passed":false, "Phase":"download", "ModelLoaded":false, "NumericalOrPerformanceQualified":false,
        "ProductionRuntimePinChanged":false, "WorkerPolicyRelaxed":false, "PackageFiles":[], "Probes":[]});
    let receipt_path = output.join("build-receipt.json");
    save(&receipt_path, &receipt, false)?;
    let result: crate::Result<Value> = (|| {
        let archive = working.join("source.zip");
        download(
            &manifest["base"],
            &archive,
            &working,
            180.min(candidate.remaining()?.as_secs()),
        )?;
        let extracted = working.join("source");
        extract_zip(&archive, &extracted)?;
        let source = extracted.join(format!("llama.cpp-{}", text(&manifest["base"], "commit")?));
        candidates_unchanged(&source, &manifest, false)?;
        let git = crate::startup_process::resolve("git.exe".as_ref())?;
        let arguments = [
            "-c".into(),
            "core.autocrlf=false".into(),
            "-c".into(),
            "core.eol=lf".into(),
            "-C".into(),
            source.clone().into_os_string(),
            "apply".into(),
        ];
        let mut check = arguments.to_vec();
        check.extend(["--check".into(), reviewed_patch.clone().into_os_string()]);
        candidate.tool(&git, &check, "patch-check.txt", 120)?;
        let mut apply = arguments.to_vec();
        apply.push(reviewed_patch.into_os_string());
        candidate.tool(&git, &apply, "patch-apply.txt", 120)?;
        drop(patch_guard);
        candidates_unchanged(&source, &manifest, true)?;
        if crate::startup_cache::digest_file(&source.join("LICENSE"))?
            != text(&manifest["license"], "sha256")?
        {
            return Err("Publisher license identity changed.".into());
        }
        let llvm_pin = &prior["compiler"];
        let llvm_name = text(llvm_pin, "archive")?;
        if Path::new(llvm_name).components().count() != 1 {
            return Err("LLVM archive filename is unsafe.".into());
        }
        let llvm_archive = working.join(llvm_name);
        let llvm_download = json!({"url":format!("https://github.com/llvm/llvm-project/releases/download/{}/{}", text(llvm_pin,"release")?, llvm_name),
            "bytes":count(llvm_pin,"archiveBytes")?, "sha256":text(llvm_pin,"archiveSha256")?});
        download(
            &llvm_download,
            &llvm_archive,
            &working,
            600.min(candidate.remaining()?.as_secs()),
        )?;
        let llvm_root = working.join("llvm");
        fs::create_dir(&llvm_root).map_err(|error| error.to_string())?;
        let zstd = crate::startup_process::resolve("zstd.exe".as_ref())?;
        let tar = crate::startup_download::system_tool("tar.exe")?;
        receipt["LlvmExtraction"] = json!({"ZstdPath":zstd,"ZstdSha256":crate::startup_cache::digest_file(&zstd)?,
            "ZstdVersion":candidate.tool(&zstd,&["--version".into()],"zstd-version.txt",30)?,
            "TarPath":tar,"TarSha256":crate::startup_cache::digest_file(&tar)?,"MaximumWindowLog":31});
        receipt["Phase"] = "llvm-extract".into();
        save(&receipt_path, &receipt, false)?;
        let (_, decompress, extract) = extraction_arguments(&llvm_archive, &llvm_root)?;
        candidate.tool(&zstd, &decompress, "llvm-decompress.txt", 300)?;
        candidate.tool(&tar, &extract, "llvm-extract.txt", 300)?;
        let roots = fs::read_dir(&llvm_root)
            .map_err(|error| error.to_string())?
            .collect::<std::io::Result<Vec<_>>>()
            .map_err(|error| error.to_string())?;
        if roots.len() != 1
            || !roots[0]
                .file_type()
                .map_err(|error| error.to_string())?
                .is_dir()
        {
            return Err("Expected one pinned LLVM directory.".into());
        }
        let llvm = roots[0].path().join("bin");
        external(&llvm)?;
        let compiler_pins: Vec<_> = llvm_pin["executables"]
            .as_array()
            .ok_or("Missing pinned compiler inventory.")?
            .iter()
            .filter(|pin| matches!(pin["file"].as_str(), Some("clang-cl.exe" | "lld-link.exe")))
            .collect();
        if compiler_pins.len() != 2 {
            return Err("Expected two reviewed native LLVM compiler identities.".into());
        }
        for pin in compiler_pins {
            let path = llvm.join(text(pin, "file")?);
            crate::startup_cache::assert_arm64(&path)?;
            crate::startup_download::verify(&path, count(pin, "bytes")?, text(pin, "sha256")?)?;
        }
        let (overlay, vc_version, sdk_version, sdk_bin) = sdk_environment(&candidate, &llvm)?;
        candidate.environment = overlay;
        let compiler = llvm.join("clang-cl.exe");
        let linker = llvm.join("lld-link.exe");
        let cmake = crate::startup_process::resolve("cmake.exe".as_ref())?;
        let powershell = crate::startup_process::resolve("pwsh.exe".as_ref())?;
        let build = working.join("build");
        let mut flags: Vec<OsString> = vec![
            "-S".into(),
            source.clone().into_os_string(),
            "-B".into(),
            build.clone().into_os_string(),
            "-G".into(),
            "Ninja".into(),
            "-DCMAKE_BUILD_TYPE=Release".into(),
        ];
        for (name, path) in [
            ("CMAKE_C_COMPILER", compiler.clone()),
            ("CMAKE_CXX_COMPILER", compiler.clone()),
            ("CMAKE_LINKER", linker),
            ("CMAKE_RC_COMPILER", sdk_bin.join("rc.exe")),
            ("CMAKE_MT", sdk_bin.join("mt.exe")),
        ] {
            flags.push(format!("-D{name}={}", cmake_path(&path)).into());
        }
        flags.extend([
            "-DCMAKE_C_FLAGS=/clang:-march=armv8.7-a /clang:-fvectorize /clang:-ffp-model=fast /clang:-fno-finite-math-only",
            "-DCMAKE_CXX_FLAGS=/EHsc /clang:-march=armv8.7-a /clang:-fvectorize /clang:-ffp-model=fast /clang:-fno-finite-math-only",
            "-DBUILD_SHARED_LIBS=ON", "-DGGML_CPU=ON", "-DGGML_NATIVE=OFF", "-DGGML_BACKEND_DL=OFF", "-DGGML_CPU_ALL_VARIANTS=OFF",
            "-DGGML_OPENMP=OFF", "-DGGML_CPU_KLEIDIAI=OFF", "-DGGML_LLAMAFILE=OFF", "-DGGML_RPC=OFF", "-DGGML_CUDA=OFF", "-DGGML_VULKAN=OFF", "-DGGML_OPENCL=OFF", "-DGGML_HEXAGON=OFF",
            "-DLLAMA_BUILD_TESTS=ON", "-DGGML_BUILD_TESTS=OFF", "-DLLAMA_BUILD_EXAMPLES=OFF", "-DLLAMA_BUILD_TOOLS=ON", "-DLLAMA_BUILD_SERVER=ON", "-DLLAMA_BUILD_APP=OFF", "-DLLAMA_BUILD_UI=OFF",
            "-DLLAMA_OPENSSL=OFF", "-DLLAMA_BUILD_BORINGSSL=OFF", "-DLLAMA_SUBPROCESS=OFF", "-DLLAMA_LLGUIDANCE=OFF", "-DLLAMA_BUILD_COMMIT=73bfe6c", "-DLLAMA_BUILD_NUMBER=10787",
        ].into_iter().map(OsString::from));
        receipt["Compiler"] = json!({"LLVM":llvm_pin,"VisualCppToolsVersion":vc_version,"WindowsSdkVersion":sdk_version,
            "ClangVersion":candidate.tool(&compiler,&["--version".into()],"compiler-version.txt",120)?,
            "CMakeVersion":candidate.tool(&cmake,&["--version".into()],"cmake-version.txt",120)?});
        let current_executable = std::env::current_exe().map_err(|error| error.to_string())?;
        receipt["Supervisor"] = json!({"SourceId":environment.get("BONSAI_TOOLS_SOURCE_ID"),"Sha256":crate::startup_cache::digest_file(&current_executable)?,
            "WorkerPolicy":"ordinary-required-headless","MemoryLimitBytes":536_870_912,"TimeoutSeconds":10,"OutputLimitBytesPerStream":65_536});
        receipt["CMakeArguments"] = json!(text_arguments(&flags)?);
        receipt["BuildTargets"] = json!(TARGETS);
        receipt["Phase"] = "configure".into();
        save(&receipt_path, &receipt, false)?;
        candidate.tool(&cmake, &flags, "configure.txt", 180)?;
        receipt["Phase"] = "compile".into();
        save(&receipt_path, &receipt, false)?;
        let mut arguments = vec![
            "--build".into(),
            build.clone().into_os_string(),
            "--parallel".into(),
            "4".into(),
            "--target".into(),
        ];
        arguments.extend(TARGETS.iter().map(OsString::from));
        candidate.tool(&cmake, &arguments, "compile.txt", 1500)?;
        candidates_unchanged(&source, &manifest, true)?;
        receipt["CMakeCacheSha256"] =
            crate::startup_cache::digest_file(&build.join("CMakeCache.txt"))?.into();
        let bundle = output.join("bundle");
        fs::create_dir(&bundle).map_err(|error| error.to_string())?;
        for entry in fs::read_dir(build.join("bin")).map_err(|error| error.to_string())? {
            let path = entry.map_err(|error| error.to_string())?.path();
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or("Candidate output is not Unicode.")?;
            if !path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("dll"))
                && !TARGETS.iter().any(|target| name == format!("{target}.exe"))
            {
                continue;
            }
            ordinary(&path)?;
            crate::startup_cache::assert_arm64(&path)?;
            fs::copy(&path, bundle.join(name)).map_err(|error| error.to_string())?;
        }
        for name in TARGETS {
            if !bundle.join(format!("{name}.exe")).is_file() {
                return Err("Required candidate comparator or fixture is missing.".into());
            }
        }
        fs::copy(source.join("LICENSE"), bundle.join("LICENSE"))
            .map_err(|error| error.to_string())?;
        receipt["Phase"] = "static-import-closure".into();
        save(&receipt_path, &receipt, false)?;
        candidate.tool(
            &powershell,
            &[
                "-NoLogo".into(),
                "-NoProfile".into(),
                "-File".into(),
                repository
                    .join("tools/runtime-candidates/pe_closure.ps1")
                    .into_os_string(),
                "-Bundle".into(),
                bundle.clone().into_os_string(),
                "-System".into(),
                crate::startup_download::system_tool("kernel32.dll")?
                    .parent()
                    .ok_or("Missing system directory.")?
                    .as_os_str()
                    .to_owned(),
                "-Output".into(),
                output.join("imports.json").into_os_string(),
            ],
            "static-imports.txt",
            120,
        )?;
        let mut files = Vec::new();
        let mut artifacts = Vec::new();
        for entry in fs::read_dir(&bundle).map_err(|error| error.to_string())? {
            let path = entry.map_err(|error| error.to_string())?.path();
            let pin = image(&path)?;
            artifacts.push(json!({"path":path,"bytes":pin["Bytes"],"sha256":pin["Sha256"]}));
            files.push(pin);
        }
        receipt["PackageFiles"] = json!(files);
        let mut cases = vec![
            (
                "completion-version".into(),
                "llama-completion.exe",
                vec!["--version".to_owned()],
            ),
            (
                "server-version".into(),
                "llama-server.exe",
                vec!["--version".to_owned()],
            ),
            (
                "bench-help".into(),
                "llama-bench.exe",
                vec!["--help".to_owned()],
            ),
        ];
        fs::write(
            working.join("literal-wildcard-fixture.gguf"),
            "Synthetic argv wildcard sentinel; not a model.",
        )
        .map_err(|error| error.to_string())?;
        for (index, value) in [
            "",
            "日本語 🪴 café",
            "C:\\notes dir\\ \"quoted\" tail\\\\",
            "line1\tline2\nline3",
            "*.gguf",
            "  leading and trailing  ",
            "slashes\\\\\\\"quote",
        ]
        .iter()
        .enumerate()
        {
            cases.push((
                format!("argv-{index}"),
                "test-headless-argv.exe",
                vec![
                    "--model".into(),
                    "模型/test.gguf".into(),
                    "--seed".into(),
                    index.to_string(),
                    "--prompt".into(),
                    (*value).into(),
                ],
            ));
        }
        receipt["Phase"] = "required-headless-model-free-probes".into();
        save(&receipt_path, &receipt, false)?;
        let mut probes = Vec::new();
        for (name, executable, arguments) in cases {
            let evidence = output.join("probes").join(&name);
            fs::create_dir_all(&evidence).map_err(|error| error.to_string())?;
            let probe = crate::completion::run(
                json!({"schemaVersion":1,"executable":bundle.join(executable),"arguments":arguments,
                "workingDirectory":working,"evidenceDirectory":evidence,"artifacts":artifacts,"memoryLimitBytes":536_870_912,
                "requiredAvailableBytes":536_870_912,"physicalReserveBytes":0,"timeoutSeconds":10,
                "stdoutLimitBytes":65_536,"stderrLimitBytes":65_536,"requireNonemptyStdout":false,"ciOnly":false}),
            )?;
            probes.push(
                json!({"Name":name,"Arguments":arguments,"Result":probe,"ModelLoaded":false}),
            );
            receipt["Probes"] = json!(probes);
            save(&receipt_path, &receipt, false)?;
            if probe["ExitCode"] != 0 {
                return Err("Required headless model-free probe failed.".into());
            }
        }
        receipt["Phase"] = "completed".into();
        receipt["Passed"] = true.into();
        Ok(
            json!({"Status":"hosted-headless-candidate-built","OutputDirectory":output,"ModelLoaded":false,"RuntimePinChanged":false}),
        )
    })();
    if let Err(error) = &result {
        receipt["Failure"] = error.chars().take(4096).collect::<String>().into();
    }
    save(&receipt_path, &receipt, false)?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn llvm_extraction_uses_one_bounded_decoder_then_plain_tar() -> crate::Result<()> {
        let archive = Path::new("C:/scratch/llvm.tar.zst");
        let destination = Path::new("C:/scratch/llvm");
        let (plain, decompress, extract) = extraction_arguments(archive, destination)?;
        assert_eq!(plain, Path::new("C:/scratch/llvm.tar"));
        assert_eq!(
            decompress,
            vec![
                OsString::from("--decompress"),
                "--long=31".into(),
                "-o".into(),
                plain.clone().into_os_string(),
                archive.as_os_str().to_owned()
            ]
        );
        assert_eq!(
            extract,
            vec![
                OsString::from("-xf"),
                plain.into_os_string(),
                "-C".into(),
                destination.as_os_str().to_owned()
            ]
        );
        assert!(extraction_arguments(Path::new("llvm.zip"), destination).is_err());
        Ok(())
    }
    #[test]
    fn cmake_paths_preserve_spaces_and_normalize_only_separators() {
        assert_eq!(
            cmake_path(Path::new(
                "C:\\Program Files (x86)\\Windows Kits\\10\\Bin\\rc.exe"
            )),
            "C:/Program Files (x86)/Windows Kits/10/Bin/rc.exe"
        );
        assert_eq!(
            cmake_path(Path::new("D:/LLVM/bin/clang-cl.exe")),
            "D:/LLVM/bin/clang-cl.exe"
        );
    }
}
