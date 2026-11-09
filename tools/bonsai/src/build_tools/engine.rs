//! Locked CPU comparator source build and shared `MSBuild` scheduling limits.

use super::{
    BTreeMap, OsString, Path, Value, execute, external, fs, is_redirect, json, ordinary, read_json,
    save, scratch, text,
};

const OPTIONS: &[&str] = &[
    "GGML_CPU=ON",
    "GGML_NATIVE=ON",
    "GGML_BACKEND_DL=OFF",
    "GGML_CPU_ALL_VARIANTS=OFF",
    "CMAKE_C_STANDARD=11",
    "CMAKE_C_STANDARD_REQUIRED=ON",
    "CMAKE_C_EXTENSIONS=OFF",
    "CMAKE_CXX_STANDARD=17",
    "CMAKE_CXX_STANDARD_REQUIRED=ON",
    "CMAKE_CXX_EXTENSIONS=OFF",
    // These diagnostics belong to the immutable external comparator source.
    "CMAKE_COMPILE_WARNING_AS_ERROR=OFF",
    "LLAMA_ALL_WARNINGS=ON",
    "LLAMA_FATAL_WARNINGS=OFF",
    "CMAKE_CXX_FLAGS_INIT=/permissive- /sdl",
    "CMAKE_C_FLAGS_INIT=/sdl",
    "GGML_OPENMP=OFF",
    "GGML_OPENMP_FETCH=OFF",
    "GGML_CPU_KLEIDIAI=OFF",
    "GGML_BLAS=OFF",
    "GGML_LLAMAFILE=OFF",
    "GGML_CUDA=OFF",
    "GGML_HIP=OFF",
    "GGML_MUSA=OFF",
    "GGML_CANN=OFF",
    "GGML_VULKAN=OFF",
    "GGML_OPENCL=OFF",
    "GGML_HEXAGON=OFF",
    "GGML_RPC=OFF",
    "GGML_SYCL=OFF",
    "GGML_METAL=OFF",
    "GGML_OPENVINO=OFF",
    "GGML_WEBGPU=OFF",
    "GGML_BUILD_TESTS=OFF",
    "GGML_ACCELERATE=OFF",
    "GGML_ZDNN=OFF",
    "GGML_VIRTGPU=OFF",
    "GGML_VIRTGPU_BACKEND=OFF",
    "GGML_ET=OFF",
    "GGML_ET_SYSEMU=OFF",
    "GGML_ZENDNN=OFF",
    "GGML_BUILD_EXAMPLES=OFF",
    "GGML_CCACHE=OFF",
    "LLAMA_BUILD_COMMON=ON",
    "LLAMA_BUILD_TOOLS=ON",
    "LLAMA_BUILD_SERVER=ON",
    "LLAMA_BUILD_APP=OFF",
    "LLAMA_BUILD_TESTS=OFF",
    "LLAMA_BUILD_EXAMPLES=OFF",
    "LLAMA_BUILD_UI=OFF",
    "LLAMA_USE_PREBUILT_UI=OFF",
    "LLAMA_OPENSSL=OFF",
    "LLAMA_BUILD_BORINGSSL=OFF",
    "LLAMA_BUILD_LIBRESSL=OFF",
    "LLAMA_SUBPROCESS=OFF",
    "LLAMA_LLGUIDANCE=OFF",
    "FETCHCONTENT_FULLY_DISCONNECTED=ON",
];

fn parallelism(requested: u8, cpus: usize, available: Option<u64>) -> crate::Result<u8> {
    if requested > 12 || cpus == 0 {
        return Err("Invalid build parallelism input.".into());
    }
    if requested > 0 {
        return Ok(requested);
    }
    let cpu_budget = cpus.saturating_sub(2).clamp(1, 8);
    let memory_budget = available
        .map_or(2, |bytes| {
            bytes.saturating_sub(2_147_483_648) / 1_073_741_824
        })
        .max(1);
    u8::try_from(
        u64::try_from(cpu_budget)
            .map_err(|error| error.to_string())?
            .min(memory_budget),
    )
    .map_err(|error| error.to_string())
}

fn build_arguments(build: &Path, jobs: u8) -> crate::Result<Vec<OsString>> {
    if !(1..=12).contains(&jobs) {
        return Err("Build jobs must be between 1 and 12.".into());
    }
    Ok(vec![
        "--build".into(),
        build.as_os_str().to_owned(),
        "--config".into(),
        "Release".into(),
        "--target".into(),
        "llama-server".into(),
        "--parallel".into(),
        jobs.to_string().into(),
        "--".into(),
        "/p:UseMultiToolTask=true".into(),
        "/p:EnforceProcessCountAcrossBuilds=true".into(),
        format!("/p:CL_MPCount={jobs}").into(),
        format!("/p:MultiProcMaxCount={jobs}").into(),
    ])
}

fn forbidden(name: &str) -> bool {
    let name = name.to_ascii_uppercase();
    name.starts_with("CMAKE_")
        || name.starts_with("GIT_CONFIG")
        || [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_COMMON_DIR",
            "GIT_INDEX_FILE",
            "GIT_OBJECT_DIRECTORY",
            "GIT_ALTERNATE_OBJECT_DIRECTORIES",
            "CC",
            "CXX",
            "CL",
            "_CL_",
            "LINK",
            "CFLAGS",
            "CXXFLAGS",
            "LDFLAGS",
        ]
        .contains(&name.as_str())
}

fn available() -> Option<u64> {
    #[cfg(windows)]
    {
        bonsai_windows_process::available_memory_bytes().ok()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "One engine build transaction keeps source, configuration and output identities together."
)]
pub(super) fn build(
    repository: &Path,
    data: &Path,
    git: &Path,
    cmake: &Path,
    generator: &str,
    parallel: u8,
    fetch: bool,
) -> crate::Result<Value> {
    if !cfg!(all(windows, target_arch = "aarch64")) {
        return Err("Build in a native ARM64 process on a Windows ARM64 host.".into());
    }
    if !["Visual Studio 18 2026", "Visual Studio 17 2022"].contains(&generator) {
        return Err("Unsupported engine build generator.".into());
    }
    external(data)?;
    for path in [git, cmake] {
        if !path.is_absolute()
            || path
                .extension()
                .is_none_or(|extension| !extension.eq_ignore_ascii_case("exe"))
        {
            return Err("Build tools must name existing absolute executable paths.".into());
        }
        ordinary(path)?;
    }
    if std::env::vars_os().any(|(name, _)| name.to_str().is_some_and(forbidden)) {
        return Err(
            "Build override environment is unsupported; no environment was changed.".into(),
        );
    }
    let cpus = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    let memory = if parallel == 0 { available() } else { None };
    let jobs = parallelism(parallel, cpus, memory)?;
    let lock = read_json(&repository.join("config/runtime.lock.json"))?;
    let commit = text(&lock["runtime"], "commit")?;
    let uri = text(&lock["runtime"], "repository")?;
    if !crate::identity::is_lower_hex(commit, 40)
        || uri != "https://github.com/ggml-org/llama.cpp.git"
    {
        return Err("Unsupported runtime lock identity.".into());
    }
    let working = scratch("EngineBuild")?;
    let hooks = working.join("empty-hooks");
    fs::create_dir(&hooks).map_err(|error| error.to_string())?;
    let invoke =
        |executable: &Path, arguments: &[OsString], name: &str, timeout| -> crate::Result<String> {
            let output = execute(
                executable,
                arguments,
                &working,
                &BTreeMap::new(),
                name,
                timeout,
            )?;
            Ok(format!("{}{}", output.stdout, output.stderr).trim().into())
        };
    let invoke_git = |args: &[OsString], name: &str| -> crate::Result<String> {
        let mut arguments = vec![
            "-c".into(),
            format!("core.hooksPath={}", hooks.display()).into(),
            "-c".into(),
            "submodule.recurse=false".into(),
            "-c".into(),
            "core.longpaths=true".into(),
        ];
        arguments.extend_from_slice(args);
        invoke(git, &arguments, name, 3600)
    };
    let git_version = invoke_git(&["--version".into()], "git-version.txt")?;
    let cmake_version = invoke(cmake, &["--version".into()], "cmake-version.txt", 30)?;
    let capabilities: Value = serde_json::from_str(&invoke(
        cmake,
        &["-E".into(), "capabilities".into()],
        "capabilities.txt",
        30,
    )?)
    .map_err(|error| error.to_string())?;
    if capabilities["generators"]
        .as_array()
        .is_none_or(|values| !values.iter().any(|value| value["name"] == generator))
    {
        return Err("Installed CMake does not support the selected generator.".into());
    }
    let source = data.join("sources").join(commit);
    let build = data.join("builds").join(commit).join("cpu");
    external(&build)?;
    if !source.exists() {
        external(&source)?;
        if !fetch {
            return Err(
                "Source is absent. Set fetchSource to explicitly fetch the locked commit.".into(),
            );
        }
        fs::create_dir_all(source.parent().ok_or("Source has no parent.")?)
            .map_err(|error| error.to_string())?;
        invoke_git(
            &[
                "init".into(),
                "--quiet".into(),
                format!("--template={}", hooks.display()).into(),
                source.clone().into_os_string(),
            ],
            "git-init.txt",
        )?;
        invoke_git(
            &[
                "-C".into(),
                source.clone().into_os_string(),
                "fetch".into(),
                "--no-tags".into(),
                "--depth=1".into(),
                uri.into(),
                commit.into(),
            ],
            "git-fetch.txt",
        )?;
        invoke_git(
            &[
                "-C".into(),
                source.clone().into_os_string(),
                "checkout".into(),
                "--quiet".into(),
                "--detach".into(),
                commit.into(),
            ],
            "git-checkout.txt",
        )?;
    }
    if !source.join(".git").is_dir() {
        return Err(
            "Existing source must be a dedicated Git clone; it will not be overwritten.".into(),
        );
    }
    // Git source itself is intentionally a checkout. Check its ancestry for redirects
    // without applying the external artifact policy's checkout prohibition.
    for path in source.ancestors() {
        if let Ok(metadata) = fs::symlink_metadata(path)
            && is_redirect(&metadata)
        {
            return Err("Reparse paths are unsupported.".into());
        }
    }
    let top = invoke_git(
        &[
            "-C".into(),
            source.clone().into_os_string(),
            "rev-parse".into(),
            "--show-toplevel".into(),
        ],
        "git-root.txt",
    )?;
    let head = invoke_git(
        &[
            "-C".into(),
            source.clone().into_os_string(),
            "rev-parse".into(),
            "HEAD".into(),
        ],
        "git-head.txt",
    )?;
    if !top
        .replace('\\', "/")
        .eq_ignore_ascii_case(&source.to_string_lossy().replace('\\', "/"))
        || head != commit
    {
        return Err("Existing source is not the exact locked checkout.".into());
    }
    let status_arguments = [
        "-C".into(),
        source.clone().into_os_string(),
        "status".into(),
        "--porcelain".into(),
        "--untracked-files=all".into(),
        "--ignored=matching".into(),
    ];
    if !invoke_git(&status_arguments, "git-status-before.txt")?.is_empty() {
        return Err(
            "Existing source contains changes or extra files; it will not be modified.".into(),
        );
    }
    let configuration = json!({"repository":uri, "commit":commit, "source":source, "generator":generator,
        "platform":"ARM64", "configuration":"Release", "options":OPTIONS});
    let marker = build.join("swiftbonsai-build.json");
    if build.exists()
        && fs::read_dir(&build)
            .map_err(|error| error.to_string())?
            .next()
            .is_some()
        && read_json(&marker).ok().as_ref() != Some(&configuration)
    {
        return Err("Build directory has no matching ownership/configuration record; choose a fresh data directory.".into());
    }
    fs::create_dir_all(&build).map_err(|error| error.to_string())?;
    let _guard = crate::startup_cache::lock_file(&build.join("engine-build.lock"))?;
    save(&marker, &configuration, false)?;
    let mut configure = vec![
        "-S".into(),
        source.into_os_string(),
        "-B".into(),
        build.clone().into_os_string(),
        "-G".into(),
        generator.into(),
        "-A".into(),
        "ARM64".into(),
        format!("-DGIT_EXECUTABLE={}", git.display()).into(),
    ];
    configure.extend(OPTIONS.iter().map(|value| format!("-D{value}").into()));
    invoke(cmake, &configure, "configure.txt", 3600)?;
    let cache =
        fs::read_to_string(build.join("CMakeCache.txt")).map_err(|error| error.to_string())?;
    if !cache
        .lines()
        .any(|line| line == "CMAKE_GENERATOR_PLATFORM:INTERNAL=ARM64")
        || !cache
            .lines()
            .any(|line| line == format!("CMAKE_GENERATOR:INTERNAL={generator}"))
    {
        return Err("Configured generator/platform mismatch.".into());
    }
    invoke(cmake, &build_arguments(&build, jobs)?, "build.txt", 3600)?;
    if !invoke_git(&status_arguments, "git-status-after.txt")?.is_empty() {
        return Err("Build changed the source checkout; inspect it before reuse.".into());
    }
    let engine = build.join("bin/Release/llama-server.exe");
    crate::startup_cache::assert_arm64(&engine)?;
    let version = invoke(&engine, &["--version".into()], "engine-version.txt", 30)?;
    let reported = version
        .split_once("commit ")
        .and_then(|(_, rest)| rest.split_once(')'))
        .map(|(value, _)| value);
    if reported.is_none_or(|value| {
        !(7..=40).contains(&value.len())
            || !crate::identity::is_lower_hex(value, value.len())
            || !commit.starts_with(value)
    }) {
        return Err("Engine version does not identify the locked commit.".into());
    }
    let mut compiler = Vec::new();
    for entry in fs::read_dir(build.join("CMakeFiles")).map_err(|error| error.to_string())? {
        let path = entry
            .map_err(|error| error.to_string())?
            .path()
            .join("CMakeCXXCompiler.cmake");
        if path.is_file() {
            ordinary(&path)?;
            compiler.push(json!({"path":path, "content":fs::read_to_string(&path).map_err(|error| error.to_string())?}));
        }
    }
    let receipt = json!({"schemaVersion":1, "configuration":configuration,
        "scheduling":{"requestedParallel":parallel, "parallelJobs":jobs, "availableMemoryBytes":memory},
        "git":{"path":git, "version":git_version}, "cmake":{"path":cmake, "version":cmake_version}, "compiler":compiler,
        "engine":{"path":engine, "sha256":crate::startup_cache::digest_file(&engine)?, "version":version},
        "cacheSha256":crate::startup_cache::digest_file(&build.join("CMakeCache.txt"))?, "logs":working,
        "qualification":"build-and-version-only; inference, UI and performance unqualified"});
    let receipt_path = build.join(format!("receipt-{}.json", crate::startup_process::unique()));
    save(&receipt_path, &receipt, true)?;
    Ok(
        json!({"Status":"arm64-cpu-engine-built", "EnginePath":engine, "ReceiptPath":receipt_path, "ParallelJobs":jobs}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_parallelism_preserves_interactive_reserves_and_explicit_overrides()
    -> crate::Result<()> {
        for (cpus, available, jobs) in [
            (12, Some(32_212_254_720), 8),
            (usize::MAX, Some(u64::MAX), 8),
            (4, Some(u64::MAX), 2),
            (1, Some(u64::MAX), 1),
            (12, Some(5_368_709_120), 3),
            (12, Some(5_368_709_119), 2),
            (12, Some(0), 1),
            (12, Some(2_147_483_648), 1),
            (12, None, 2),
            (1, None, 1),
        ] {
            assert_eq!(parallelism(0, cpus, available)?, jobs);
        }
        assert_eq!(parallelism(12, 1, Some(0))?, 12);
        assert!(parallelism(13, 12, None).is_err());
        assert!(parallelism(0, 0, None).is_err());
        Ok(())
    }
    #[test]
    fn build_arguments_keep_literal_paths_and_one_shared_compiler_budget() -> crate::Result<()> {
        let path = Path::new("C:\\external data & fixtures\\build");
        let arguments = build_arguments(path, 3)?;
        assert_eq!(arguments.len(), 13);
        assert_eq!(arguments[1], path.as_os_str());
        for value in [
            "/p:UseMultiToolTask=true",
            "/p:EnforceProcessCountAcrossBuilds=true",
            "/p:CL_MPCount=3",
            "/p:MultiProcMaxCount=3",
        ] {
            assert!(arguments.iter().any(|argument| argument == value));
        }
        assert!(build_arguments(path, 0).is_err());
        assert!(build_arguments(path, 13).is_err());
        Ok(())
    }
}
