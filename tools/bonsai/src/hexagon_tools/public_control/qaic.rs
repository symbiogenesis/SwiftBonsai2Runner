//! Independent public-source QAIC generation and mock wire compiler lane.
use super::*;
fn resolved(name: &str) -> Result<PathBuf> {
    fs::canonicalize(startup_process::resolve(std::ffi::OsStr::new(name))?)
        .map_err(|e| e.to_string())
}
fn ghc_runtime(control: &mut Control, ghc: &Path) -> Result<Value> {
    let libdir = control.run(
        "ghc-libdir",
        vec![ghc.as_os_str().to_owned(), "--print-libdir".into()],
        30,
    )?;
    let libdir = PathBuf::from(libdir.trim());
    let root = ghc
        .parent()
        .and_then(Path::parent)
        .ok_or("GHC installation root absent.")?;
    let mut result = json!({"reportedLibdir":libdir,"selectedToolRoot":root,"scope":"selected-installation-compiler/runtime-executable-candidates; not-complete-tool-or-OS-closure","scanTruncated":false,"entriesObserved":0,"candidates":[]});
    if !libdir.starts_with(root) {
        result["scope"] =
            json!("reported-libdir-outside-selected-wrapper-installation; runtime-closure-pending");
        return Ok(result);
    }
    let mut pending = std::collections::VecDeque::from([root.to_owned()]);
    let mut entries = 0_u64;
    let mut selected = Vec::new();
    'scan: while let Some(directory) = pending.pop_front() {
        for entry in fs::read_dir(directory).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            entries += 1;
            if entries > 50000 {
                result["scanTruncated"] = json!(true);
                break 'scan;
            }
            let metadata = fs::symlink_metadata(entry.path()).map_err(|e| e.to_string())?;
            if bonsai_artifacts::is_redirect(&metadata) {
                continue;
            }
            if metadata.is_dir() {
                pending.push_back(entry.path());
                continue;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name != "ghc"
                && Some(name.as_ref()) != ghc.file_name().and_then(|n| n.to_str())
                && !name.starts_with("ghc-iserv")
            {
                continue;
            }
            let path = entry.path();
            let mut item = identity(&path)?;
            let mut header = [0; 4];
            let count = File::open(&path)
                .map_err(|e| e.to_string())?
                .read(&mut header)
                .map_err(|e| e.to_string())?;
            item["elfExecutable"] = json!(count == 4 && header == *b"\x7fELF");
            item["path"] = json!(path);
            item["discoveredAs"] = json!(path);
            selected.push(item);
        }
    }
    result["entriesObserved"] = json!(entries);
    result["candidates"] = json!(selected);
    Ok(result)
}
fn audit_source(
    control: &mut Control,
    source: &Path,
    pin: &Value,
    report: &mut Value,
) -> Result<()> {
    let git = resolved("git")?;
    let invoke = |control: &mut Control, name: &str, args: &[&str]| {
        let mut arguments = vec![
            git.clone().into_os_string(),
            "-C".into(),
            source.as_os_str().to_owned(),
        ];
        arguments.extend(args.iter().map(OsString::from));
        control.run(name, arguments, 30)
    };
    let root = invoke(control, "source-root", &["rev-parse", "--show-toplevel"])?;
    let revision = invoke(control, "source-revision", &["rev-parse", "HEAD"])?;
    let status = invoke(control, "source-clean", &["status", "--porcelain"])?;
    require(
        Path::new(root.trim()) == source
            && revision.trim() == string(pin, "qaicCommit")?
            && status.trim().is_empty(),
        "QAIC checkout differs from exact clean reviewed source.",
    )?;
    let mut selected = Vec::new();
    for entry in list(pin, "sourceFiles")? {
        let name = string(entry, "path")?;
        let mut actual = pinned(&source.join(name), entry)?;
        actual["name"] = json!(name);
        selected.push(actual);
    }
    report["sourceFiles"] = json!(selected);
    let tracked = invoke(control, "source-inventory", &["ls-files"])?;
    let mut complete = Vec::new();
    let mut total = 0_u64;
    for name in tracked.lines().filter(|line| !line.is_empty()) {
        require(
            complete.len() < 4096
                && !Path::new(name).is_absolute()
                && !name.split('/').any(|part| part == ".."),
            "QAIC tracked source inventory exceeds safe bounds.",
        )?;
        let mut item = identity(&source.join(name))?;
        total = total
            .checked_add(item["bytes"].as_u64().ok_or("Source extent absent.")?)
            .ok_or("Source extent overflow.")?;
        require(
            total <= 64 * 1024 * 1024,
            "QAIC tracked source bytes exceed bound.",
        )?;
        item["name"] = json!(name);
        complete.push(item);
    }
    save(
        &control.review.join("qaic-source-files.json"),
        &json!(complete),
    )?;
    Ok(())
}
fn projects(directory: &Path, name: &str, review: &Path) -> Result<Vec<Value>> {
    let mut results = Vec::new();
    for filename in [
        "cabal.project",
        "cabal.project.freeze",
        "cabal.project.local",
        "dist-newstyle/cache/plan.json",
    ] {
        let path = directory.join(filename);
        if !path.is_file() {
            require(
                filename == "cabal.project.local",
                "Source build omitted exact dependency project/freeze/plan.",
            )?;
            continue;
        }
        require(
            fs::metadata(&path).map_err(|e| e.to_string())?.len() <= 2 * 1024 * 1024,
            "Dependency project/freeze/plan exceeds bound.",
        )?;
        results.push(copy(
            &path,
            &review.join(format!("{name}-{}", filename.replace('/', "-"))),
        )?);
    }
    Ok(results)
}
fn build(control: &mut Control, source: &Path, pin: &Value, report: &mut Value) -> Result<PathBuf> {
    prepare_inputs(control, source, pin, report)?;
    let cabal = toolchain(control, pin, report)?;
    build_dependencies(control, source, pin, report, &cabal)
}

fn prepare_inputs(control: &Control, source: &Path, pin: &Value, report: &mut Value) -> Result<()> {
    let inputs = control.review.join("inputs-and-notices");
    copy(
        &source.join("LICENSE.txt"),
        &inputs.join("LICENSE-QAIC.txt"),
    )?;
    for name in ["CStub.c", "Stub.c", "Env.c", "Allocator.c", "Slim.c"] {
        copy(
            &source.join("src/Language/SO").join(name),
            &inputs.join(format!("QAIC-template-{name}")),
        )?;
    }
    let mut public = Vec::new();
    for input in list(pin, "publicInputs")? {
        public.push(control.receive(input, &inputs, 65625)?);
    }
    report["publicInputs"] = json!(public);
    report["stage"] = json!("toolchain-identity");
    Ok(())
}

fn build_dependencies(
    control: &mut Control,
    source: &Path,
    pin: &Value,
    report: &mut Value,
    cabal: &Path,
) -> Result<PathBuf> {
    let source_directory = source.join("src");
    let helper = source_directory.join("file2hs");
    let project = format!(
        "packages: .\nindex-state: {}\nactive-repositories: hackage.haskell.org\n",
        string(pin, "hackageIndexState")?
    );
    for directory in [&source_directory, &helper] {
        for name in [
            "cabal.project",
            "cabal.project.freeze",
            "cabal.project.local",
        ] {
            require(
                !directory.join(name).exists(),
                "Upstream source unexpectedly contains a project override.",
            )?;
        }
        fs::write(directory.join("cabal.project"), &project).map_err(|e| e.to_string())?;
    }
    report["stage"] = json!("pinned-dependency-resolution");
    control.command(
        "hackage-update",
        vec![cabal.as_os_str().to_owned(), "update".into()],
        &source_directory,
        150,
        false,
    )?;
    for (name, directory) in [("file2hs", &helper), ("qaic", &source_directory)] {
        control.command(
            &format!("{name}-freeze"),
            vec![cabal.as_os_str().to_owned(), "freeze".into()],
            directory,
            60,
            false,
        )?;
        let freeze = directory.join("cabal.project.freeze");
        require(
            fs::metadata(&freeze).map_err(|e| e.to_string())?.len() <= 65536,
            "Cabal omitted bounded dependency freeze.",
        )?;
        pinned(&freeze, &pin["dependencyFreezePins"][name])?;
        copy(
            &freeze,
            &control.review.join(format!("{name}-before-build.freeze")),
        )?;
    }
    report["stage"] = json!("source-generator-build");
    let arguments = list(pin, "buildArguments")?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(OsString::from)
                .ok_or_else(|| "Fixed build arguments must be strings.".into())
        })
        .collect::<Result<Vec<_>>>()?;
    control.command("upstream-make", arguments, source, 420, false)?;
    let mut dependencies = projects(&helper, "file2hs", &control.review)?;
    dependencies.extend(projects(&source_directory, "qaic", &control.review)?);
    report["dependencyResolution"] = json!(dependencies);
    let plan = json_file(
        &source_directory.join("dist-newstyle/cache/plan.json"),
        2 * 1024 * 1024,
    )?;
    let external = list(&plan, "install-plan")?
        .iter()
        .filter(|item| item["pkg-src"]["type"] == "repo-tar")
        .collect::<Vec<_>>();
    require(
        external.len() == 1,
        "External source dependency differs from reviewed control.",
    )?;
    for (name, expected) in pin["externalPackagePin"]
        .as_object()
        .ok_or("External package pin missing.")?
    {
        require(
            external[0][name] == *expected,
            "External source dependency differs from reviewed pin.",
        )?;
    }
    report["externalPackage"] = pin["externalPackagePin"].clone();
    for (name, directory) in [("file2hs", &helper), ("qaic", &source_directory)] {
        require(
            identity(&directory.join("cabal.project.freeze"))?["sha256"]
                == identity(&control.review.join(format!("{name}-before-build.freeze")))?["sha256"],
            "Build changed preselected dependency freeze.",
        )?;
    }
    Ok(source_directory.join("Linux_ReleaseG/ship/qaic"))
}
fn generate(
    control: &mut Control,
    executable: &Path,
    pin: &Value,
    report: &mut Value,
) -> Result<PathBuf> {
    let inputs = control.review.join("inputs-and-notices");
    let generated = control.output.join("generated");
    report["generatorExecutable"] = identity(executable)?;
    report["generatorAttempted"] = json!(true);
    report["generatorExecuted"] = Value::Null;
    report["versionQueryAttempted"] = json!(true);
    control.run(
        "source-generator-version",
        vec![executable.as_os_str().to_owned(), "--version".into()],
        30,
    )?;
    report["generatorExecuted"] = json!(true);
    report["versionQueryCompleted"] = json!(true);
    report["stage"] = json!("independent-IDL-generation");
    report["idlGenerationAttempted"] = json!(true);
    control.run(
        "independent-generation",
        vec![
            executable.as_os_str().to_owned(),
            "--map-dll".into(),
            "--standard-types".into(),
            format!("--include-path={}", inputs.display()).into(),
            format!("--output-path={}", generated.display()).into(),
            inputs.join("htp_iface.idl").into_os_string(),
        ],
        15,
    )?;
    for name in [
        "generatorExecuted",
        "generatorCompleted",
        "idlGenerationCompleted",
    ] {
        report[name] = json!(true);
    }
    let mut files = fs::read_dir(&generated)
        .map_err(|e| e.to_string())?
        .map(|entry| entry.map(|entry| entry.path()).map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>>>()?;
    files.sort();
    let expected = list(pin, "expectedGeneratedFiles")?
        .iter()
        .filter_map(Value::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    require(
        files.len() == expected.len()
            && files.iter().all(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| expected.contains(name))
            }),
        "Public generator output differs from fixed IDL contract.",
    )?;
    let retained = control.review.join("generated-public-source");
    directory(&retained)?;
    let mut identities = Vec::new();
    for file in files {
        let data = read(&file, 1_048_576)?;
        let text = String::from_utf8_lossy(&data);
        let mut item = identity(&file)?;
        item["copyrightMarkerPresent"] = json!(text.to_lowercase().contains("copyright"));
        item["bsdClearIdentifierPresent"] = json!(text.contains("BSD-3-Clause-Clear"));
        copy(
            &file,
            &retained.join(file.file_name().ok_or("Generated name absent.")?),
        )?;
        identities.push(item);
    }
    report["generatedFiles"] = json!(identities);
    fs::write(control.review.join("NOTICES.txt"),"Independent public-source QAIC output only.\nQAIC compiler/templates: BSD-3-Clause-Clear; preserve LICENSE-QAIC.txt and template notices.\nPublic FastRPC IDL imports/runtime headers: BSD-3-Clause; preserve LICENSE-FastRPC.txt and original notices.\nTarget htp_iface.idl: llama.cpp MIT; preserve LICENSE-llama.cpp.txt and original input.\nSDK6.7 generator/import equivalence and device invocation remain unqualified.\n").map_err(|e|e.to_string())?;
    report["stage"] = json!("independent-public-generation-complete");
    report["outcome"] = json!("generated-independent-oracle-only");
    Ok(generated)
}
fn wire_capture(
    control: &mut Control,
    pin: &Value,
    generated: &Path,
    report: &mut Value,
) -> Result<()> {
    let selection = &pin["wireCapture"];
    require(
        selection["enabled"] == true,
        "Fixed public mock wire control must be selected.",
    )?;
    flags_false(
        selection,
        &["sdkLinked", "deviceInvocationAuthorized", "profileEligible"],
    )?;
    let inputs = control.review.join("wire-public-inputs");
    directory(&inputs)?;
    let mut headers = Vec::new();
    for header in list(selection, "headers")? {
        headers.push(control.receive(header, &inputs, 65625)?);
    }
    for input in [&selection["harness"], &selection["expected"]] {
        let name = string(input, "name")?;
        pinned(&control.source(name), input)?;
        copy(&control.source(name), &inputs.join(name))?;
    }
    for input in list(selection, "reviewedGeneratedFiles")? {
        pinned(&generated.join(string(input, "name")?), input)?;
    }
    let (executable, exe_pin) =
        compile_mock(control, selection, &inputs, generated, &headers, report)?;
    let text = control.run("wire-capture", vec![executable.clone().into_os_string()], 5)?;
    report["wireCapture"]["mockExecuted"] = json!(true);
    report["wireCapture"]["mockCompleted"] = json!(true);
    require(
        identity(&executable)? == exe_pin,
        "Mock executable changed during bounded capture.",
    )?;
    let actual = wire::parse(&text)?;
    let keys = [
        "schemaVersion",
        "kind",
        "testVectorId",
        "handleHex",
        "start",
        "stop",
        "callCount",
        "unexpectedCalls",
        "returnCodes",
        "layout",
    ];
    require(
        actual.as_object().is_some_and(|object| {
            object.len() == keys.len() && keys.iter().all(|key| object.contains_key(*key))
        }) && actual["kind"] == "public-qaic-mock-wire-capture",
        "Mock capture emitted unsupported receipt shape.",
    )?;
    wire::capture(
        &actual,
        &json_file(
            &inputs.join(string(&selection["expected"], "name")?),
            1_048_576,
        )?,
    )?;
    report["wireCapture"]["observed"] = actual;
    report["wireCapture"]["captureReceipt"] = identity(&control.review.join("wire-capture.log"))?;
    for field in [
        "independentVectorMatched",
        "compilerIdentityRechecked",
        "passed",
    ] {
        report["wireCapture"][field] = json!(true);
    }
    report["stage"] = json!("independent-public-mock-wire-complete");
    report["outcome"] = json!("captured-independent-public-wire-only");
    Ok(())
}
pub(super) fn run(request: &Request) -> Result<Value> {
    let mut control = Control::new(request, 660, false)?;
    let source = request
        .source_directory
        .as_ref()
        .ok_or("Exact public QAIC source checkout required.")?;
    plain(source)?;
    let path = request
        .pin_path
        .clone()
        .unwrap_or_else(|| control.source("public-qaic.lock.json"));
    let pin = json_file(&path, 1_048_576)?;
    require(
        pin["schemaVersion"] == 1,
        "Unsupported public source-control schema.",
    )?;
    flags_false(&pin, &["deviceInvocationAuthorized", "profileEligible"])?;
    directory(&control.review.join("inputs-and-notices"))?;
    directory(&control.output.join("generated"))?;
    let cabal = control.output.join("cabal");
    directory(&cabal)?;
    let config = cabal.join("config");
    fs::write(&config,format!("repository hackage.haskell.org\n  url: https://hackage.haskell.org/\n  secure: True\nremote-repo-cache: {}\nstore-dir: {}\n",cabal.join("packages").display(),cabal.join("store").display())).map_err(|e|e.to_string())?;
    control
        .environment
        .insert("CABAL_DIR".into(), cabal.to_string_lossy().into_owned());
    control
        .environment
        .insert("CABAL_CONFIG".into(), config.to_string_lossy().into_owned());
    control
        .environment
        .insert("GHC_ENVIRONMENT".into(), "-".into());
    let mut report = json!({"schemaVersion":1,"kind":"public-QAIC-independent-source-oracle","runnerRevision":std::env::var("GITHUB_SHA").unwrap_or_default(),"runId":std::env::var("GITHUB_RUN_ID").unwrap_or_default(),"runAttempt":std::env::var("GITHUB_RUN_ATTEMPT").unwrap_or_default(),"qaicCommit":pin["qaicCommit"],"sourceSha256":identity(&control.root.join("tools/bonsai/src/hexagon_tools/public_control/qaic.rs"))?["sha256"],"pinSha256":identity(&path)?["sha256"],"outcome":"pending","stage":"source-identity","generatorExecuted":false,"generatorAttempted":false,"generatorCompleted":false,"versionQueryAttempted":false,"versionQueryCompleted":false,"idlGenerationAttempted":false,"idlGenerationCompleted":false,"hackageIndexState":pin["hackageIndexState"],"sourceFiles":[],"publicInputs":[],"toolchain":{},"dependencyResolution":[],"generatedFiles":[],"wireCapture":null,"sdkExecutableUsed":false,"sdkImportsUsed":false,"sdkQaicEquivalenceQualified":false,"sdkImportEquivalenceQualified":false,"deviceCodeLoaded":false,"deviceInvocationAuthorized":false,"profileEligible":false});
    control.provenance(&mut report)?;
    let result: Result<()> = (|| {
        audit_source(&mut control, source, &pin, &mut report)?;
        let executable = build(&mut control, source, &pin, &mut report)?;
        let generated = generate(&mut control, &executable, &pin, &mut report)?;
        let review = pin["generationReviewOnly"]
            .as_bool()
            .ok_or("Public generation review mode must be an explicit boolean.")?;
        if review {
            report["stage"] = json!("public-standard-type-generation-review");
            report["outcome"] = json!("public-standard-generated-source-awaiting-review");
            report["wireCapture"] = json!({"passed":false,"executionSelected":false,"reason":"new-standard-type-output-requires-independent-review"});
        } else {
            wire_capture(&mut control, &pin, &generated, &mut report)?;
            wire::compare_rust(&mut control, &pin, &mut report, &request.rustc)?;
        }
        Ok(())
    })();
    report["commands"] = json!(control.commands);
    if let Err(error) = &result {
        report["outcome"] = json!("failed");
        report["failure"] = json!(error.chars().take(512).collect::<String>());
    }
    save(&control.review.join("host.json"), &report)?;
    result.map(|()|json!({"outcome":report["outcome"],"qaicCommit":report["qaicCommit"],"sdkExecutableUsed":false,"deviceInvocationAuthorized":false}))
}

fn toolchain(control: &mut Control, pin: &Value, report: &mut Value) -> Result<PathBuf> {
    let ghc = resolved("ghc")?;
    let cabal = resolved("cabal")?;
    let ghc_version = control.run(
        "ghc-version",
        vec![ghc.clone().into_os_string(), "--numeric-version".into()],
        30,
    )?;
    let cabal_version = control.run(
        "cabal-version",
        vec![cabal.as_os_str().to_owned(), "--numeric-version".into()],
        30,
    )?;
    require(
        ghc_version.trim() == string(pin, "ghcVersion")?
            && cabal_version.trim() == string(pin, "cabalVersion")?,
        "GHC/Cabal identity differs from pinned versions.",
    )?;
    report["toolchain"] = json!({"ghcVersion":ghc_version.trim(),"cabalVersion":cabal_version.trim(),"ghcExecutable":identity(&ghc)?,"cabalExecutable":identity(&cabal)?,"setupAction":pin["setupAction"]});
    control.run(
        "ghc-info",
        vec![ghc.clone().into_os_string(), "--info".into()],
        30,
    )?;
    control.run(
        "ghc-package-db",
        vec![resolved("ghc-pkg")?.into_os_string(), "dump".into()],
        30,
    )?;
    report["toolchain"]["selectedGhcRuntime"] = ghc_runtime(control, &ghc)?;
    Ok(cabal)
}

fn compile_mock(
    control: &mut Control,
    selection: &Value,
    inputs: &Path,
    generated: &Path,
    headers: &[Value],
    report: &mut Value,
) -> Result<(PathBuf, Value)> {
    let compiler = resolved("/usr/bin/gcc")?;
    let original = identity(&compiler)?;
    let version = control.run(
        "wire-gcc-version",
        vec![
            compiler.clone().into_os_string(),
            "-dumpfullversion".into(),
            "-dumpversion".into(),
        ],
        30,
    )?;
    let target = control.run(
        "wire-gcc-target",
        vec![compiler.clone().into_os_string(), "-dumpmachine".into()],
        30,
    )?;
    require(
        version
            .trim()
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
            && (2..=3).contains(&version.trim().split('.').count())
            && target.trim() == "x86_64-linux-gnu",
        "Mock wire requires fixed native Linux C target.",
    )?;
    control.run(
        "wire-gcc-banner",
        vec![compiler.clone().into_os_string(), "--version".into()],
        30,
    )?;
    control.run(
        "wire-gcc-specs",
        vec![compiler.clone().into_os_string(), "-dumpspecs".into()],
        30,
    )?;
    let mut components = Vec::new();
    for name in ["cc1", "as", "ld"] {
        let candidate = control.run(
            &format!("wire-gcc-{name}"),
            vec![
                compiler.clone().into_os_string(),
                format!("-print-prog-name={name}").into(),
            ],
            30,
        )?;
        let path = resolved(candidate.trim())?;
        let mut component = identity(&path)?;
        component["path"] = json!(path);
        component["role"] = json!(name);
        components.push(component);
    }
    require(
        selection["compilerFlags"]
            == json!([
                "-std=gnu11",
                "-O0",
                "-fno-lto",
                "-fno-strict-aliasing",
                "-include",
                "{checked-public-AEEStdDef.h}"
            ]),
        "Fixed mock-only compiler flags changed.",
    )?;
    let executable = control.output.join("public-qaic-wire-mock");
    let map = control.review.join("wire-link.map");
    let arguments = vec![
        compiler.clone().into_os_string(),
        "-std=gnu11".into(),
        "-O0".into(),
        "-fno-lto".into(),
        "-fno-strict-aliasing".into(),
        "-include".into(),
        inputs.join("AEEStdDef.h").into_os_string(),
        format!("-I{}", inputs.display()).into(),
        format!("-I{}", generated.display()).into(),
        generated.join("htp_iface_stub.c").into_os_string(),
        inputs
            .join(string(&selection["harness"], "name")?)
            .into_os_string(),
        format!("-Wl,-Map={}", map.display()).into(),
        "-o".into(),
        executable.clone().into_os_string(),
    ];
    control.run("wire-compile", arguments, 30)?;
    require(
        identity(&compiler)? == original,
        "GCC driver changed during bounded compilation.",
    )?;
    for component in &components {
        pinned(Path::new(string(component, "path")?), component)?;
    }
    require(
        fs::metadata(&map).map_err(|e| e.to_string())?.len() <= 1_048_576,
        "Mock link map omitted or exceeded bound.",
    )?;
    let exe_pin = identity(&executable)?;
    report["wireCapture"] = json!({"kind":"public-generated-C-mock-wire-control","passed":false,"headers":headers,"compiler":{"name":original["name"],"bytes":original["bytes"],"sha256":original["sha256"],"path":compiler,"version":version.trim(),"target":target.trim(),"components":components,"identityPinScope":"observed-and-rechecked-within-this-hosted-run","closure":"driver/cc1/as/ld identities; not-complete-OS-closure"},"mockExecutionAttempted":true,"mockExecuted":null,"mockCompleted":false,"sdkLinked":false,"driverLoaded":false,"deviceCodeLoaded":false,"sdkEquivalenceQualified":false,"WindowsArm64AbiQualified":false,"scope":"public-generated-stub-and-public-headers-on-hosted-Linux-LP64-only","linkMap":identity(&map)?,"executable":exe_pin});
    Ok((executable, exe_pin))
}
