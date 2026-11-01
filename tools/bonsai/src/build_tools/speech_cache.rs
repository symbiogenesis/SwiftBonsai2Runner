//! Source/host-bound static speech libraries shared by hosted Cargo phases.

use super::{
    BTreeMap, CHANNEL, OsString, Path, PathBuf, Value, execute, external, fs, github_environment,
    image, json, read_json, save, stage_output,
};

const LIBRARIES: &[(&str, &str)] = &[
    ("qwen-core", "Release/qwen-core.lib"),
    ("ggml", "ggml/src/Release/ggml.lib"),
    ("ggml-base", "ggml/src/Release/ggml-base.lib"),
    ("ggml-cpu", "ggml/src/Release/ggml-cpu.lib"),
];

fn identities(source: &str, host: &str) -> crate::Result<()> {
    if !crate::identity::is_lower_hex(source, 64) || !crate::identity::is_lower_hex(host, 64) {
        return Err("Static speech libraries require exact source and host identities.".into());
    }
    if crate::native_startup::host()?["HostCacheKey"] != host {
        return Err(
            "Static speech library host identity differs from the current native host.".into(),
        );
    }
    Ok(())
}

pub(super) fn environment(data: &Path, working: &Path) -> crate::Result<Value> {
    let host = std::env::var("NATIVE_SPEECH_HOST_KEY").map_err(|error| error.to_string())?;
    if !crate::identity::is_lower_hex(&host, 64)
        || crate::native_startup::host()?["HostCacheKey"] != host
    {
        return Err("Native preparation requires the checked current host cache identity.".into());
    }
    let native = crate::native_startup::environment(data, working)?;
    for (name, value) in native["Environment"]
        .as_object()
        .ok_or("Native preparation omitted its compiler environment.")?
    {
        github_environment(
            name,
            value
                .as_str()
                .ok_or("Native compiler environment is not text.")?,
        )?;
    }
    let evidence = working.join("SwiftBonsai-NativePreparation");
    external(&evidence)?;
    github_environment("NATIVE_SPEECH_EVIDENCE", &evidence.to_string_lossy())?;
    Ok(native)
}

fn validate_receipt(
    package: &Path,
    receipt: &Value,
    source: &str,
    host: &str,
) -> crate::Result<()> {
    if !crate::identity::is_lower_hex(source, 64)
        || !crate::identity::is_lower_hex(host, 64)
        || receipt["SchemaVersion"] != 2
        || receipt["HostCacheKey"] != host
        || receipt["SourceId"] != source
        || receipt["Files"].as_object().map(serde_json::Map::len) != Some(LIBRARIES.len())
    {
        return Err("Native static library cache source or host identity differs.".into());
    }
    for (_, relative) in LIBRARIES {
        if receipt["Files"][relative] != image(&package.join(relative))?["Sha256"] {
            return Err("Native static library cache digest differs.".into());
        }
    }
    Ok(())
}

pub(super) fn verify(package: &Path, source: &str, host: &str) -> crate::Result<Value> {
    external(package)?;
    identities(source, host)?;
    let receipt = read_json(&package.join("receipt.json"))?;
    validate_receipt(package, &receipt, source, host)?;
    github_environment("BONSAI_TTS_NATIVE_BUILD_DIR", &package.to_string_lossy())?;
    Ok(
        json!({"Status":"native-static-libraries-verified","PackageDirectory":package,"SourceId":source,"HostCacheKey":host}),
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "The hosted static-cache request supplies separate checked input/output roots and immutable identities."
)]
pub(super) fn build(
    repository: &Path,
    working: &Path,
    target: &Path,
    package: &Path,
    evidence: &Path,
    source: &str,
    host: &str,
) -> crate::Result<Value> {
    identities(source, host)?;
    for path in [working, target, package, evidence] {
        external(path)?;
    }
    fs::create_dir_all(evidence).map_err(|error| error.to_string())?;
    stage_output(package, true)?;
    let cargo = crate::startup_process::resolve("cargo.exe".as_ref())?;
    let arguments: Vec<OsString> = vec![
        format!("+{CHANNEL}").into(),
        "build".into(),
        "--manifest-path".into(),
        repository.join("Cargo.toml").into_os_string(),
        "--package".into(),
        "bonsai-speech-tts".into(),
        "--locked".into(),
        "--release".into(),
        "--all-features".into(),
        "--message-format=json".into(),
        "--target-dir".into(),
        target.as_os_str().to_owned(),
    ];
    let build = execute(
        &cargo,
        &arguments,
        working,
        &BTreeMap::new(),
        "static-build.log",
        1800,
    );
    // Native compiler output stays bounded by the shared 16 MiB process policy;
    // only its last 64 KiB enters the selected evidence artifact.
    let build_receipt = json!({"SchemaVersion":1,"Scope":"hosted-speech-static-library-build-only","SourceId":source,"HostCacheKey":host,"Passed":build.is_ok(),"ModelExecuted":false});
    super::save_evidence(working, evidence, &build_receipt, &["static-build.log"])?;
    let build = build?;
    let mut outputs = Vec::new();
    for line in build.stdout.lines().filter(|line| !line.is_empty()) {
        let record: Value = serde_json::from_str(line).map_err(|error| error.to_string())?;
        if record["reason"] == "build-script-executed"
            && record["package_id"]
                .as_str()
                .is_some_and(|value| value.contains("#bonsai-speech-tts@"))
        {
            outputs.push(PathBuf::from(super::text(&record, "out_dir")?));
        }
    }
    if outputs.len() != 1 {
        return Err("Expected exactly one native speech build-script output.".into());
    }
    external(&outputs[0])?;
    if !outputs[0].starts_with(target) {
        return Err("Native speech build output is outside the checked target directory.".into());
    }
    let mut files = serde_json::Map::new();
    for (name, relative) in LIBRARIES {
        let source_path = outputs[0].join(format!("{name}.lib"));
        super::ordinary(&source_path)?;
        let destination = package.join(relative);
        fs::create_dir_all(
            destination
                .parent()
                .ok_or("Static library has no parent.")?,
        )
        .map_err(|error| error.to_string())?;
        fs::copy(&source_path, &destination).map_err(|error| error.to_string())?;
        files.insert((*relative).into(), image(&destination)?["Sha256"].clone());
    }
    let receipt = json!({"SchemaVersion":2,"SourceId":source,"HostCacheKey":host,"Files":files});
    validate_receipt(package, &receipt, source, host)?;
    save(&package.join("receipt.json"), &receipt, true)?;
    Ok(
        json!({"Status":"native-static-libraries-built","PackageDirectory":package,"SourceId":source,"HostCacheKey":host}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn static_cache_rejects_changed_source_host_inventory_and_bytes() -> crate::Result<()> {
        let package = super::super::scratch("StaticCacheTest")?;
        let source = "a".repeat(64);
        let host = "b".repeat(64);
        let mut files = serde_json::Map::new();
        for (_, relative) in LIBRARIES {
            let path = package.join(relative);
            fs::create_dir_all(path.parent().ok_or("Missing static fixture parent.")?)
                .map_err(|error| error.to_string())?;
            fs::write(&path, relative).map_err(|error| error.to_string())?;
            files.insert((*relative).into(), image(&path)?["Sha256"].clone());
        }
        let receipt =
            json!({"SchemaVersion":2,"SourceId":source,"HostCacheKey":host,"Files":files});
        validate_receipt(&package, &receipt, &source, &host)?;
        for key in ["SourceId", "HostCacheKey", "SchemaVersion", "Files"] {
            let mut changed = receipt.clone();
            changed[key] = Value::Null;
            assert!(validate_receipt(&package, &changed, &source, &host).is_err());
        }
        fs::write(package.join(LIBRARIES[0].1), "tampered").map_err(|error| error.to_string())?;
        assert!(validate_receipt(&package, &receipt, &source, &host).is_err());
        fs::remove_dir_all(package).map_err(|error| error.to_string())
    }
}
