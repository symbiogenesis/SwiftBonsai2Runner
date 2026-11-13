//! Hosted native application and speech packages, without model provisioning.

use std::io::Write as _;

use super::{
    BTreeMap, CHANNEL, OpenOptions, OsString, Path, PathBuf, Value, capture, environment,
    execute_strict, external, fs, github_environment, hosted_context, image, json, read_json, save,
    save_evidence, stage_output, text, text_arguments,
};

fn checked_maintenance(environment: &BTreeMap<String, String>) -> crate::Result<(PathBuf, Value)> {
    let path = PathBuf::from(
        environment
            .get("BONSAI_TOOLS_PATH")
            .ok_or("Checked maintenance executable is unavailable.")?,
    );
    external(&path)?;
    crate::startup_cache::assert_arm64(&path)?;
    let receipt = read_json(
        &path
            .parent()
            .ok_or("Maintenance executable has no parent.")?
            .join("receipt.json"),
    )?;
    if receipt["SchemaVersion"] != 4
        || receipt["SourceId"].as_str()
            != environment
                .get("BONSAI_TOOLS_SOURCE_ID")
                .map(String::as_str)
        || !receipt["SourceId"]
            .as_str()
            .is_some_and(|value| crate::identity::is_lower_hex(value, 64))
        || receipt["Toolchain"] != CHANNEL
        || receipt["Sha256"] != crate::startup_cache::digest_file(&path)?
        || crate::startup_cache::digest_file(
            &std::env::current_exe().map_err(|error| error.to_string())?,
        )? != receipt["Sha256"]
    {
        return Err("Checked maintenance source, toolchain or executable identity changed.".into());
    }
    Ok((path, receipt))
}

fn candidate_image(path: &Path) -> crate::Result<Value> {
    let value = image(path)?;
    if value["Bytes"]
        .as_u64()
        .is_none_or(|bytes| bytes > 67_108_864)
    {
        return Err("Candidate outputs must be bounded regular files.".into());
    }
    Ok(value)
}

fn copy(source: &Path, destination: &Path) -> crate::Result<()> {
    let mut source = super::input_file(source)?;
    external(destination)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|error| error.to_string())?;
    std::io::copy(&mut source, &mut output)
        .and_then(|_| output.flush())
        .and_then(|()| output.sync_all())
        .map_err(|error| error.to_string())
}

fn input_inventory(repository: &Path) -> crate::Result<Value> {
    [
        "Cargo.toml",
        "Cargo.lock",
        "rust-toolchain.toml",
        "config/native-speech.lock.json",
        "src/speech-asr/Cargo.toml",
        "src/speech-asr/model.lock.json",
        "src/speech-tts/Cargo.toml",
        "src/speech-tts/model.lock.json",
        "src/speech-tts/model-q4.lock.json",
    ]
    .into_iter()
    .map(|relative| {
        let identity = candidate_image(&repository.join(relative))?;
        Ok(json!({"Path":relative,"Bytes":identity["Bytes"],"Sha256":identity["Sha256"]}))
    })
    .collect::<crate::Result<Vec<_>>>()
    .map(Value::Array)
}

fn retain_logs(output: &Path, evidence: &Path, receipt: &Value) -> crate::Result<()> {
    save_evidence(
        output,
        evidence,
        receipt,
        &["rustc.txt", "cargo-path.txt", "rustc-path.txt", "build.txt"],
    )?;
    let working = output.join("work");
    if working.is_dir() {
        let names = fs::read_dir(&working)
            .map_err(|error| error.to_string())?
            .collect::<std::io::Result<Vec<_>>>()
            .map_err(|error| error.to_string())?
            .into_iter()
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| {
                name.starts_with("source-build-")
                    && Path::new(name)
                        .extension()
                        .is_some_and(|extension| extension.eq_ignore_ascii_case("log"))
            })
            .take(4)
            .collect::<Vec<_>>();
        save_evidence(
            &working,
            evidence,
            receipt,
            &names.iter().map(String::as_str).collect::<Vec<_>>(),
        )?;
    }
    Ok(())
}

#[expect(
    clippy::too_many_lines,
    reason = "Speech packaging checks the complete source-build receipt and both immutable service outputs as one transaction."
)]
fn speech(
    repository: &Path,
    output: &Path,
    package: &Path,
    rustc: &Path,
    linker: &Path,
    receipt: &mut Value,
) -> crate::Result<()> {
    let working = output.join("work");
    let data = output.join("build-inputs");
    for path in [&working, &data] {
        fs::create_dir(path).map_err(|error| error.to_string())?;
    }
    let pin_path = repository.join("config/native-speech.lock.json");
    receipt["NativeDependencyPins"] = read_json(&pin_path)?;
    let inputs = input_inventory(repository)?;
    save(&package.join("source-inputs.json"), &inputs, true)?;
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
    let result = crate::native_startup::build_with_environment(
        repository,
        &data,
        &working,
        "All",
        false,
        crate::native_startup::AsrBuildTarget::Portable,
        &overlay,
    )?;
    receipt["NativeBuildHost"] = result["NativeBuildHost"].clone();
    receipt["AsrCpuArchitecture"] = result["AsrCpuArchitecture"].clone();
    let asr = PathBuf::from(text(&result, "AsrPath")?);
    let target = asr
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .ok_or("Native speech build returned an invalid target.")?;
    if !target.starts_with(&data) {
        return Err("Native speech build returned an unexpected target directory.".into());
    }
    let native_receipts = fs::read_dir(target)
        .map_err(|error| error.to_string())?
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|entry| {
            entry.file_name().to_str().is_some_and(|name| {
                name.starts_with("startup-build-")
                    && Path::new(name)
                        .extension()
                        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
            })
        })
        .collect::<Vec<_>>();
    if native_receipts.len() != 1 {
        return Err("Exactly one bounded native speech build receipt is required.".into());
    }
    let native_path = native_receipts[0].path();
    let native = read_json(&native_path)?;
    if native["version"] != 2
        || !native["fingerprint"]
            .as_str()
            .is_some_and(|value| crate::identity::is_lower_hex(value, 64))
        || native["outputs"].as_object().map(serde_json::Map::len) != Some(2)
    {
        return Err("Native build receipt identity is invalid.".into());
    }
    let identity = crate::identity::sha256(text(&native, "identity")?.as_bytes());
    if native_path.file_name().and_then(|name| name.to_str())
        != Some(format!("startup-build-{identity}.json").as_str())
    {
        return Err("Native build receipt identity digest differs.".into());
    }
    for (name, key) in [
        ("bonsai-speech-asr", "AsrPath"),
        ("bonsai-speech-tts", "TtsPath"),
    ] {
        let expected = target.join(format!("aarch64-pc-windows-msvc/release/{name}.exe"));
        if Path::new(text(&result, key)?) != expected
            || Path::new(text(&native["outputs"][name], "path")?) != expected
        {
            return Err("Speech output path differs from its source-build receipt.".into());
        }
        crate::startup_cache::assert_arm64(&expected)?;
        if candidate_image(&expected)?["Sha256"] != native["outputs"][name]["sha256"] {
            return Err("Speech output digest differs from its source-build receipt.".into());
        }
        copy(&expected, &package.join(format!("{name}.exe")))?;
    }
    if input_inventory(repository)? != inputs {
        return Err("Speech source inputs changed during compilation.".into());
    }
    copy(&native_path, &package.join("native-build-receipt.json"))?;
    copy(&pin_path, &package.join("native-speech.lock.json"))?;
    save(&package.join("native-build-result.json"), &result, true)?;
    receipt["NativeBuildReceipt"] = json!({"IdentitySha256":identity,"Fingerprint":native["fingerprint"],
        "File":candidate_image(&package.join("native-build-receipt.json"))?});
    let names = [
        "bonsai-speech-asr.exe",
        "bonsai-speech-tts.exe",
        "native-build-receipt.json",
        "native-build-result.json",
        "native-speech.lock.json",
        "source-inputs.json",
    ];
    receipt["Files"] = Value::Array(
        names
            .iter()
            .map(|name| candidate_image(&package.join(name)))
            .collect::<crate::Result<Vec<_>>>()?,
    );
    Ok(())
}

#[expect(
    clippy::too_many_lines,
    reason = "One hosted candidate build keeps admission, immutable identities and failure evidence in one transaction."
)]
pub(super) fn build(repository: &Path, output: &Path, speech_build: bool) -> crate::Result<Value> {
    let environment = environment();
    hosted_context(cfg!(all(windows, target_arch = "aarch64")), &environment)?;
    if speech_build
        && !environment
            .get("BONSAI_SPEECH_SOURCE_ID")
            .is_some_and(|value| crate::identity::is_lower_hex(value, 64))
    {
        return Err("Speech candidates require an exact source identity.".into());
    }
    if speech_build
        && [
            "ACTIONS_RUNTIME_TOKEN",
            "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
            "GITHUB_TOKEN",
            "GH_TOKEN",
            "ACTIONS_RUNNER_INPUT_TOKEN",
        ]
        .iter()
        .any(|name| std::env::var_os(name).is_some())
    {
        return Err("Speech candidates require a credential-free build environment.".into());
    }
    stage_output(output, false)?;
    let package = output.join("package");
    let evidence = output.join("evidence");
    for path in [&package, &evidence] {
        fs::create_dir(path).map_err(|error| error.to_string())?;
    }
    let prefix = if speech_build {
        "BONSAI_SPEECH_CANDIDATE"
    } else {
        "BONSAI_CHAT_CANDIDATE"
    };
    github_environment(&format!("{prefix}_PACKAGE"), &package.to_string_lossy())?;
    github_environment(&format!("{prefix}_EVIDENCE"), &evidence.to_string_lossy())?;
    let mut receipt = json!({"SchemaVersion":1,"Scope":if speech_build {"hosted-native-speech-build-only"} else {"hosted-native-chat-and-broker-build-only"},
        "Revision":environment.get("GITHUB_SHA"),"GitRef":environment.get("GITHUB_REF"),"Repository":environment.get("GITHUB_REPOSITORY"),
        "RunId":environment.get("GITHUB_RUN_ID"),"RunAttempt":environment.get("GITHUB_RUN_ATTEMPT"),
        "RunUrl":format!("https://github.com/symbiogenesis/SwiftBonsai2Runner/actions/runs/{}",environment.get("GITHUB_RUN_ID").map_or("",String::as_str)),
        "ImageOS":environment.get("ImageOS"),"ImageVersion":environment.get("ImageVersion"),"Target":"aarch64-pc-windows-msvc",
        "Toolchain":CHANNEL,"Files":[],"Status":"preparing","Failure":null,"ModelExecuted":false,"GuiExecuted":false,"BrokerExecuted":false,
        "SpeechBuildRequested":speech_build,"SpeechBuiltOrLaunched":false,"SpeechCompanionsExecuted":false,"ModelWeightsFetched":false,
        "DistributionQualified":false,"SigningQualified":false,"SpeechQualified":false,"PerformanceQualified":false,"DefaultMayChange":false,
        "SourceId":environment.get("BONSAI_SPEECH_SOURCE_ID")});
    retain_logs(output, &evidence, &receipt)?;
    let result: crate::Result<Value> = (|| {
        let (cargo, rustc, linker, compiler) = capture::toolchain(output)?;
        if let Some(pin) = environment.get("PINNED_TOOLCHAIN_PATH")
            && Path::new(pin).join("bin/rustc.exe") != rustc
        {
            return Err("Resolved compiler differs from the selected pinned toolchain.".into());
        }
        receipt["ToolImages"] = json!([image(&cargo)?, image(&rustc)?, image(&linker)?]);
        receipt["CompilerIdentity"] = compiler.into();
        let (tool, tool_receipt) = checked_maintenance(&environment)?;
        receipt["CheckedMaintenance"] = tool_receipt;
        receipt["Status"] = "building".into();
        retain_logs(output, &evidence, &receipt)?;
        if speech_build {
            speech(repository, output, &package, &rustc, &linker, &mut receipt)?;
        } else {
            let runtime = read_json(&repository.join("config/prism-inference.lock.json"))?;
            receipt["RuntimePin"] = json!({"Version":runtime["version"],"SourceRevision":runtime["commit"],"ArchiveSha256":runtime["asset"]["sha256"]});
            receipt["WorkspaceSha256"] =
                crate::startup_cache::digest_file(&repository.join("Cargo.toml"))?.into();
            receipt["CargoLockSha256"] =
                crate::startup_cache::digest_file(&repository.join("Cargo.lock"))?.into();
            let target = output.join("target");
            let arguments: Vec<OsString> = vec![
                "build".into(),
                "--locked".into(),
                "--release".into(),
                "-p".into(),
                "bonsai-chat".into(),
                "-p".into(),
                "bonsai-broker".into(),
                "--bins".into(),
                "--manifest-path".into(),
                repository.join("Cargo.toml").into_os_string(),
                "--target-dir".into(),
                target.clone().into_os_string(),
                "--jobs".into(),
                "4".into(),
            ];
            receipt["CargoArguments"] = json!(text_arguments(&arguments)?);
            retain_logs(output, &evidence, &receipt)?;
            let overlay = BTreeMap::from([
                ("RUSTC".into(), rustc.to_string_lossy().into_owned()),
                ("RUSTC_WRAPPER".into(), String::new()),
                ("RUSTC_WORKSPACE_WRAPPER".into(), String::new()),
                (
                    "CARGO_TARGET_DIR".into(),
                    target.to_string_lossy().into_owned(),
                ),
                (
                    "CARGO_ENCODED_RUSTFLAGS".into(),
                    format!(
                        "-C\u{1f}linker={}\u{1f}-C\u{1f}linker-flavor=lld-link",
                        linker.display()
                    ),
                ),
            ]);
            execute_strict(&cargo, &arguments, output, &overlay, "build.txt", 1200)?;
            let mut files = Vec::new();
            for name in ["bonsai-chat.exe", "bonsai-broker.exe"] {
                let source = target.join("release").join(name);
                crate::startup_cache::assert_arm64(&source)?;
                copy(&source, &package.join(name))?;
                files.push(candidate_image(&package.join(name))?);
            }
            copy(&tool, &package.join("bonsai-tools.exe"))?;
            files.push(candidate_image(&package.join("bonsai-tools.exe"))?);
            receipt["Files"] = json!(files);
        }
        receipt["Status"] = "completed".into();
        retain_logs(output, &evidence, &receipt)?;
        copy(
            &evidence.join("build-receipt.json"),
            &package.join("receipt.json"),
        )?;
        Ok(
            json!({"Status":"hosted-native-candidate-built","PackageDirectory":package,"EvidenceDirectory":evidence,"ModelExecuted":false}),
        )
    })();
    if let Err(error) = &result {
        receipt["Status"] = "failed".into();
        receipt["Failure"] = error.chars().take(4096).collect::<String>().into();
    }
    retain_logs(output, &evidence, &receipt)?;
    result
}
