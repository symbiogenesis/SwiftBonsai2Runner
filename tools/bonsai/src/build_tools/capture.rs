//! Hosted row and PTQ1 capture preparation with retained bounded failure evidence.

use std::io::Write as _;

use super::{
    BTreeMap, CHANNEL, OpenOptions, OsString, Path, PathBuf, Value, download, environment, execute,
    execute_strict, external, extract_zip, fs, hosted_context, image, json, ordinary, read_json,
    save, save_evidence, scratch, stage_output, text, text_arguments,
};

const LOGS: &[&str] = &[
    "cargo-path.txt",
    "rustc-path.txt",
    "rustc.txt",
    "collector-build.txt",
    "collector-test-build.txt",
    "collector-tests.txt",
    "rust-build.txt",
];

pub(super) fn toolchain(working: &Path) -> crate::Result<(PathBuf, PathBuf, PathBuf, String)> {
    let rustup = crate::startup_process::resolve("rustup.exe".as_ref())?;
    let resolve = |name: &str| -> crate::Result<PathBuf> {
        let result = execute(
            &rustup,
            &[
                "which".into(),
                "--toolchain".into(),
                CHANNEL.into(),
                name.into(),
            ],
            working,
            &BTreeMap::new(),
            &format!("{name}-path.txt"),
            30,
        )?;
        let path = PathBuf::from(result.stdout.trim());
        external(&path)?;
        Ok(path)
    };
    let cargo = resolve("cargo")?;
    let rustc = resolve("rustc")?;
    let bin = rustc.parent().ok_or("Compiler has no directory.")?;
    if cargo != bin.join("cargo.exe") || rustc.file_name().is_none_or(|name| name != "rustc.exe") {
        return Err("Cargo and rustc must belong to the same exact pinned toolchain.".into());
    }
    let linker = bin
        .parent()
        .ok_or("Compiler has no toolchain root.")?
        .join("lib/rustlib/aarch64-pc-windows-msvc/bin/rust-lld.exe");
    for path in [&cargo, &rustc, &linker] {
        crate::startup_cache::assert_arm64(path)?;
    }
    let compiler = execute(
        &rustc,
        &["-vV".into()],
        working,
        &BTreeMap::new(),
        "rustc.txt",
        30,
    )?;
    if !compiler
        .stdout
        .lines()
        .any(|line| line == "host: aarch64-pc-windows-msvc")
    {
        return Err("The pinned native ARM64 Rust compiler is unavailable.".into());
    }
    Ok((cargo, rustc, linker, compiler.stdout.trim().into()))
}

fn abi_controls(repository: &Path, pins: &Value) -> crate::Result<(String, String, String)> {
    let source = repository.join("tools/native-reference/swift_ptq1_reference.rs");
    let control = repository.join("tools/native-reference/fixtures/collector-abi-v1.txt");
    let provenance_path =
        repository.join("tools/native-reference/fixtures/collector-abi-v1.provenance.json");
    ordinary(&control)?;
    if fs::metadata(&control)
        .map_err(|error| error.to_string())?
        .len()
        > 65_536
    {
        return Err("Native ABI controls exceed their fixed pre-read bound.".into());
    }
    let provenance = read_json(&provenance_path)?;
    let source_digest = crate::startup_cache::digest_file(&source)?;
    let control_digest = crate::startup_cache::digest_file(&control)?;
    let provenance_digest = crate::startup_cache::digest_file(&provenance_path)?;
    if provenance["schema_version"] != 1
        || provenance["source"]["revision"] != pins["nativeSource"]["commit"]
        || provenance["source"]["headers"]["include/llama.h"]["sha256"]
            != pins["nativeSource"]["headerSha256"]
        || provenance["fixture"]["file"] != "collector-abi-v1.txt"
        || provenance["fixture"]["bytes"].as_u64()
            != Some(
                fs::metadata(&control)
                    .map_err(|error| error.to_string())?
                    .len(),
            )
        || provenance["fixture"]["sha256"] != control_digest
    {
        return Err("Native ABI controls differ from the reviewed source pins.".into());
    }
    Ok((source_digest, control_digest, provenance_digest))
}

#[expect(
    clippy::too_many_lines,
    reason = "One hosted capture transaction retains its phase-specific failure receipt through packaging."
)]
pub(super) fn build(repository: &Path, output: &Path, ptq1: bool) -> crate::Result<Value> {
    let environment = environment();
    hosted_context(cfg!(all(windows, target_arch = "aarch64")), &environment)?;
    let pins = crate::native_qualification::run(json!({"operation":"source-pins",
        "kind":if ptq1 { "ptq1" } else { "rows" }, "repositoryDirectory":repository}))?;
    stage_output(output, true)?;
    let working = scratch(if ptq1 {
        "HostedPtq1Build"
    } else {
        "HostedRowBuild"
    })?;
    let evidence = working.join("evidence");
    fs::create_dir(&evidence).map_err(|error| error.to_string())?;
    if let Some(path) = environment
        .get("GITHUB_ENV")
        .filter(|path| !path.is_empty())
    {
        let mut file = OpenOptions::new()
            .append(true)
            .open(path)
            .map_err(|error| error.to_string())?;
        writeln!(
            file,
            "{}={}",
            if ptq1 {
                "BONSAI_PTQ1_BUILD_EVIDENCE"
            } else {
                "BONSAI_ROW_BUILD_EVIDENCE"
            },
            evidence.display()
        )
        .map_err(|error| error.to_string())?;
    }
    let source = &pins["rustSource"];
    let mut receipt = json!({"SchemaVersion":1, "Scope":if ptq1 {"hosted-ptq1-capture-build-only"} else {"hosted-row-checker-build-only"},
        "RunnerRevision":environment.get("GITHUB_SHA"), "RustSource":source, "RustToolchain":CHANNEL,
        "CheckerFeatures":["parallel-rows"], "SupportedRowWorkers":[1,4], "BuildFeatures":["parallel-rows"],
        "Phase":"download", "Status":"running", "ModelProcessStarted":false,
        "DescendantOwnership":"kill-on-close-job; complete-tree-cleanup-before-result"});
    if ptq1 {
        receipt["NativeSource"] = pins["nativeSource"].clone();
    }
    save_evidence(&working, &evidence, &receipt, LOGS)?;
    let result: crate::Result<Value> = (|| {
        let archive = working.join("source.zip");
        download(source, &archive, &working, 180)?;
        let extracted = working.join("source");
        extract_zip(&archive, &extracted)?;
        let manifest = extracted.join(format!(
            "prism-inference-{}/Cargo.toml",
            text(source, "commit")?
        ));
        ordinary(&manifest)?;
        receipt["Phase"] = "toolchain".into();
        save_evidence(&working, &evidence, &receipt, LOGS)?;
        let (cargo, rustc, linker, compiler_version) = toolchain(&working)?;
        receipt["ToolImages"] = json!([image(&cargo)?, image(&rustc)?, image(&linker)?]);
        receipt["LinkerFlavor"] = "lld-link".into();
        let compiler_options: Vec<OsString> = [
            "--edition=2024",
            "--target",
            "aarch64-pc-windows-msvc",
            "--deny",
            "warnings",
            "-C",
            "opt-level=2",
            "-C",
            "overflow-checks=on",
            "-C",
            "panic=unwind",
            "-C",
        ]
        .into_iter()
        .map(OsString::from)
        .chain([
            format!("linker={}", linker.display()).into(),
            "-C".into(),
            "linker-flavor=lld-link".into(),
        ])
        .collect();
        let mut collector_identity = None;
        let mut compiler_arguments = Vec::new();
        let collector = working.join("swift-ptq1-reference.exe");
        if ptq1 {
            let identities = abi_controls(repository, &pins)?;
            let source_path = repository.join("tools/native-reference/swift_ptq1_reference.rs");
            compiler_arguments.clone_from(&compiler_options);
            compiler_arguments.extend([
                source_path.clone().into_os_string(),
                "-o".into(),
                collector.clone().into_os_string(),
            ]);
            receipt["Phase"] = "collector-build-and-tests".into();
            save_evidence(&working, &evidence, &receipt, LOGS)?;
            execute_strict(
                &rustc,
                &compiler_arguments,
                &working,
                &BTreeMap::new(),
                "collector-build.txt",
                120,
            )?;
            crate::startup_cache::assert_arm64(&collector)?;
            let tests = working.join("collector-tests.exe");
            let mut arguments = compiler_options;
            arguments.extend([
                "--test".into(),
                source_path.into_os_string(),
                "-o".into(),
                tests.clone().into_os_string(),
            ]);
            execute_strict(
                &rustc,
                &arguments,
                &working,
                &BTreeMap::new(),
                "collector-test-build.txt",
                120,
            )?;
            execute_strict(
                &tests,
                &["--test-threads=1".into()],
                &working,
                &BTreeMap::new(),
                "collector-tests.txt",
                30,
            )?;
            if abi_controls(repository, &pins)? != identities {
                return Err("Collector source or ABI controls changed during compilation.".into());
            }
            collector_identity = Some(identities);
        }
        let target = working.join("rust-build");
        receipt["Phase"] = "checker-build".into();
        save_evidence(&working, &evidence, &receipt, LOGS)?;
        let overlay = BTreeMap::from([
            ("RUSTC".into(), rustc.to_string_lossy().into_owned()),
            ("RUSTC_WRAPPER".into(), String::new()),
            ("RUSTC_WORKSPACE_WRAPPER".into(), String::new()),
            (
                "CARGO_ENCODED_RUSTFLAGS".into(),
                format!(
                    "-C\u{1f}linker={}\u{1f}-C\u{1f}linker-flavor=lld-link",
                    linker.display()
                ),
            ),
        ]);
        let arguments = [
            "build".into(),
            "--locked".into(),
            "--release".into(),
            "--manifest-path".into(),
            manifest.into_os_string(),
            "-p".into(),
            "prism-inference".into(),
            "--features".into(),
            "parallel-rows".into(),
            "--example".into(),
            "swift_model_check".into(),
            "--target-dir".into(),
            target.clone().into_os_string(),
        ];
        execute_strict(
            &cargo,
            &arguments,
            &working,
            &overlay,
            "rust-build.txt",
            1200,
        )?;
        let checker = target.join("release/examples/swift_model_check.exe");
        crate::startup_cache::assert_arm64(&checker)?;
        receipt["Phase"] = "package".into();
        save_evidence(&working, &evidence, &receipt, LOGS)?;
        let mut files = Vec::new();
        for path in if ptq1 {
            vec![collector, checker]
        } else {
            vec![checker]
        } {
            let destination =
                output.join(path.file_name().ok_or("Capture output has no filename.")?);
            let mut input =
                bonsai_artifacts::open_locked_file(&path).map_err(|error| error.to_string())?;
            let mut destination_file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&destination)
                .map_err(|error| error.to_string())?;
            std::io::copy(&mut input, &mut destination_file).map_err(|error| error.to_string())?;
            destination_file
                .sync_all()
                .map_err(|error| error.to_string())?;
            drop(destination_file);
            files.push(image(&destination)?);
        }
        let mut capture = json!({"SchemaVersion":if ptq1 {3} else {1}, "RunnerRevision":environment.get("GITHUB_SHA"),
            "RustSource":source, "RustToolchain":CHANNEL, "BuildFeatures":["parallel-rows"], "Files":files});
        if let Some((source_digest, control_digest, provenance_digest)) = collector_identity {
            capture["NativeSource"] = pins["nativeSource"].clone();
            capture["CheckerFeatures"] = json!(["parallel-rows"]);
            capture["SupportedRowWorkers"] = json!([1, 4]);
            capture["CollectorLanguage"] = "rust".into();
            capture["CollectorSourcePath"] =
                "tools/native-reference/swift_ptq1_reference.rs".into();
            capture["CollectorSourceSha256"] = source_digest.into();
            capture["AbiControlSha256"] = control_digest.into();
            capture["AbiProvenanceSha256"] = provenance_digest.into();
            capture["Compiler"] = json!({"Path":rustc, "Sha256":crate::startup_cache::digest_file(&rustc)?,
                "LinkerSha256":crate::startup_cache::digest_file(&linker)?, "CollectorArguments":text_arguments(&compiler_arguments)?,
                "RustcVerbose":compiler_version});
        }
        let path = output.join("capture-tools.json");
        save(&path, &capture, true)?;
        receipt["CaptureReceiptSha256"] = crate::startup_cache::digest_file(&path)?.into();
        receipt["Status"] = "completed".into();
        receipt["Phase"] = "completed".into();
        Ok(
            json!({"Status":if ptq1 {"hosted-capture-tools-built"} else {"hosted-row-capture-tools-built"},
            "OutputDirectory":output, "ReceiptPath":path, "EvidenceDirectory":evidence, "ModelProcessStarted":false}),
        )
    })();
    if let Err(error) = &result {
        receipt["Status"] = "failed".into();
        receipt["Failure"] = error.chars().take(4096).collect::<String>().into();
    }
    save_evidence(&working, &evidence, &receipt, LOGS)?;
    result
}
