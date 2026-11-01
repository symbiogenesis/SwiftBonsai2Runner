//! Explicit locked-artifact provisioning without a PowerShell implementation.

use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};

use bonsai_artifacts::{is_redirect, open_locked_file, validate_external_path};
use serde::Deserialize;
use serde_json::{Value, json};
use zip::ZipArchive;

use crate::identity::is_lower_hex;

const REFERENCE: &str = include_str!("../../../config/runtime.lock.json");
const INFERENCE: &str = include_str!("../../../config/prism-inference.lock.json");
const ADAPTER: &str = include_str!("../../../config/swift-lora.lock.json");
const MAX_LOCK_BYTES: u64 = 1024 * 1024;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    operation: Operation,
    data_directory: PathBuf,
    lock_path: Option<PathBuf>,
    working_directory: Option<PathBuf>,
    #[serde(default)]
    verify_only: bool,
    #[serde(default)]
    include_capabilities: bool,
    #[serde(default)]
    include_help: bool,
    #[serde(default = "all_models")]
    model: String,
    #[serde(default = "q8")]
    tts_quantization: String,
}

#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Operation {
    Model,
    Adapter,
    Engine,
    Inference,
    Speech,
    SpeechReference,
}

fn all_models() -> String {
    "All".into()
}

fn q8() -> String {
    "Q8_0".into()
}

fn text<'a>(value: &'a Value, key: &str) -> crate::Result<&'a str> {
    value[key]
        .as_str()
        .ok_or_else(|| format!("Missing locked artifact field: {key}"))
}

fn atom(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
}

fn repository(value: &str) -> bool {
    value
        .split_once('/')
        .is_some_and(|(owner, name)| atom(owner) && atom(name))
}

fn artifact(value: &Value, filename_key: &str) -> crate::Result<()> {
    if !atom(text(value, filename_key)?)
        || value["bytes"]
            .as_u64()
            .is_none_or(|bytes| bytes == 0 || bytes > i64::MAX.unsigned_abs())
        || !is_lower_hex(text(value, "sha256")?, 64)
    {
        return Err("Each locked artifact requires a simple filename, positive length and lowercase SHA-256.".into());
    }
    Ok(())
}

fn read_lock(path: Option<&Path>, compiled: &str) -> crate::Result<Value> {
    let value: Value = if let Some(path) = path {
        if !path.is_absolute() {
            return Err("LockPath must be expanded and absolute.".into());
        }
        let file = fs::File::open(path).map_err(|error| error.to_string())?;
        if file.metadata().map_err(|error| error.to_string())?.len() > MAX_LOCK_BYTES {
            return Err("Artifact lock exceeds its byte bound.".into());
        }
        let mut bytes = Vec::new();
        file.take(MAX_LOCK_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        if u64::try_from(bytes.len()).map_err(|error| error.to_string())? > MAX_LOCK_BYTES {
            return Err("Artifact lock exceeds its byte bound.".into());
        }
        serde_json::from_slice(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes))
            .map_err(|error| error.to_string())?
    } else {
        serde_json::from_str(compiled).map_err(|error| error.to_string())?
    };
    if !value.is_object() || value["schemaVersion"] != 1 {
        return Err("Artifact lock must be a version-one JSON object.".into());
    }
    Ok(value)
}

fn model_lock(lock: &Value) -> crate::Result<()> {
    let model = &lock["model"];
    let runtime = &lock["runtime"];
    if lock["schemaVersion"] != 1
        || !repository(text(model, "repository")?)
        || !is_lower_hex(text(model, "revision")?, 40)
        || !is_lower_hex(text(runtime, "commit")?, 40)
        || !atom(text(runtime, "release")?)
        || !text(runtime, "repository")?
            .strip_prefix("https://github.com/")
            .and_then(|value| value.strip_suffix(".git"))
            .is_some_and(repository)
    {
        return Err("Invalid or mutable model/runtime lock identity.".into());
    }
    artifact(model, "filename")?;
    artifact(&runtime["asset"], "filename")?;
    let expected = format!(
        "{}/releases/download/{}/{}",
        text(runtime, "repository")?
            .strip_suffix(".git")
            .ok_or("Invalid runtime repository.")?,
        text(runtime, "release")?,
        text(&runtime["asset"], "filename")?
    );
    if text(&runtime["asset"], "url")? != expected {
        return Err("Runtime repository, release and asset URL disagree.".into());
    }
    Ok(())
}

fn working_directory(request: &Request) -> crate::Result<PathBuf> {
    let path = request.working_directory.clone().unwrap_or_else(|| {
        std::env::temp_dir().join(format!(
            "Bonsai-Provision-{}",
            crate::startup_process::unique()
        ))
    });
    validate_external_path(&path).map_err(|error| error.to_string())?;
    Ok(path)
}

fn receive(
    request: &Request,
    pin: &Value,
    url: &str,
    path: &Path,
    working: &Path,
) -> crate::Result<()> {
    let bytes = pin["bytes"].as_u64().ok_or("Missing artifact length.")?;
    let hash = text(pin, "sha256")?;
    if request.verify_only {
        crate::startup_download::verify(path, bytes, hash)
    } else {
        crate::startup_download::receive(url, path, bytes, hash, working)
    }
}

fn model(request: &Request, working: &Path) -> crate::Result<Value> {
    let lock = read_lock(request.lock_path.as_deref(), REFERENCE)?;
    model_lock(&lock)?;
    let pin = &lock["model"];
    let path = request
        .data_directory
        .join("models")
        .join(text(pin, "revision")?)
        .join(text(pin, "filename")?);
    let url = format!(
        "https://huggingface.co/{}/resolve/{}/{}",
        text(pin, "repository")?,
        text(pin, "revision")?,
        text(pin, "filename")?
    );
    receive(request, pin, &url, &path, working)?;
    Ok(
        json!({"Status": "verified", "Model": pin["repository"], "Revision": pin["revision"], "Path": path}),
    )
}

fn adapter(request: &Request, working: &Path) -> crate::Result<Value> {
    if !request.data_directory.is_dir() {
        return Err("The external DataDirectory must already exist.".into());
    }
    let lock = read_lock(None, ADAPTER)?;
    let pin = &lock["adapter"];
    let selected = bonsai_cpu_policy::abliterate_lora_artifact(&request.data_directory)?;
    let url = format!(
        "https://raw.githubusercontent.com/{}/{}/{}",
        text(pin, "repository")?,
        text(pin, "revision")?,
        text(pin, "path")?
    );
    if text(pin, "url")? != url {
        return Err("Adapter repository, revision, path and URL disagree.".into());
    }
    if request.verify_only {
        crate::startup_download::verify(&selected.path, selected.bytes, &selected.sha256)?;
    } else {
        crate::startup_download::receive_with_timeout(
            &url,
            &selected.path,
            selected.bytes,
            &selected.sha256,
            working,
            std::time::Duration::from_secs(300),
        )?;
    }
    Ok(
        json!({"Status": "verified", "AdapterPath": selected.path, "Repository": pin["repository"],
        "Revision": pin["revision"], "Sha256": selected.sha256,
        "Qualification": "pretrained artifact integrity only; Swift behavior, memory and performance remain unqualified"}),
    )
}

#[cfg(windows)]
fn engine(request: &Request, working: &Path) -> crate::Result<Value> {
    let lock = read_lock(request.lock_path.as_deref(), REFERENCE)?;
    model_lock(&lock)?;
    let runtime = &lock["runtime"];
    let pin = &runtime["asset"];
    let archive = request
        .data_directory
        .join("downloads")
        .join(text(pin, "filename")?);
    let destination = request
        .data_directory
        .join("runtime")
        .join(text(runtime, "commit")?);
    if !request.verify_only {
        receive(request, pin, text(pin, "url")?, &archive, working)?;
    }
    let spec = json!({"archivePath": archive, "runtimeDirectory": destination,
        "archiveBytes": pin["bytes"], "archiveSha256": pin["sha256"]});
    if !destination.exists() {
        if request.verify_only {
            return Err("Runtime is not installed.".into());
        }
        let staged = crate::install::run(json!({"schemaVersion": 1, "runtime": spec}))?;
        let staging = Path::new(text(&staged, "StagingDirectory")?);
        validate_external_path(&destination).map_err(|error| error.to_string())?;
        fs::rename(staging, &destination).map_err(|error| {
            format!(
                "Cannot publish runtime; staging retained at {}: {error}",
                staging.display()
            )
        })?;
    }
    let (id, creation) = crate::cpu_check::parent_identity(None, None)?;
    crate::runtime::run(
        json!({"schemaVersion": 1, "runtime": spec, "commit": runtime["commit"],
        "includeHelp": request.include_help, "parentProcessId": id, "parentCreationTimeFileTime": creation}),
    )
}

#[cfg(not(windows))]
fn engine(_request: &Request, _working: &Path) -> crate::Result<Value> {
    Err("Engine provisioning requires Windows.".into())
}

#[derive(Deserialize)]
struct InferenceFile {
    name: String,
    bytes: u64,
    sha256: String,
}

fn inference_lock(lock: &Value) -> crate::Result<Vec<InferenceFile>> {
    const REQUIRED: [&str; 4] = [
        "prism-inference.exe",
        "bonsai-token-worker.exe",
        "bonsai-swift-worker.exe",
        "prism-inference-server.exe",
    ];
    const OPTIONAL: [&str; 5] = [
        "bonsai-gguf.exe",
        "LICENSE",
        "NOTICE",
        "PRISM-LICENSE.txt",
        "CAMELID-LICENSE.txt",
    ];
    let version = text(lock, "version")?;
    let numbers = version
        .split_once('-')
        .map_or(version, |(numbers, _)| numbers);
    let parts: Vec<_> = numbers.split('.').collect();
    if lock["schemaVersion"] != 1
        || lock["package"] != "prism-inference"
        || parts.len() != 3
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
        || !version
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b".-".contains(&byte))
        || lock["repository"] != "https://github.com/symbiogenesis/prism-inference"
        || !is_lower_hex(text(lock, "commit")?, 40)
        || lock["target"] != "aarch64-pc-windows-msvc"
        || !atom(text(lock, "release")?)
    {
        return Err("Invalid Prism Inference release identity.".into());
    }
    let filename = format!("prism-inference-{version}-aarch64-pc-windows-msvc.zip");
    let asset = &lock["asset"];
    artifact(asset, "filename")?;
    if asset["filename"] != filename
        || asset["bytes"]
            .as_u64()
            .is_none_or(|bytes| bytes > 1024 * 1024 * 1024)
        || text(asset, "url")?
            != format!(
                "{}/releases/download/{}/{filename}",
                text(lock, "repository")?,
                text(lock, "release")?
            )
    {
        return Err("Invalid Prism Inference archive identity.".into());
    }
    let files: Vec<InferenceFile> =
        serde_json::from_value(lock["files"].clone()).map_err(|error| error.to_string())?;
    let mut names = BTreeSet::new();
    let mut total = 0_u64;
    for file in &files {
        if !REQUIRED.contains(&file.name.as_str()) && !OPTIONAL.contains(&file.name.as_str())
            || !names.insert(file.name.as_str())
            || file.bytes == 0
            || file.bytes > 512 * 1024 * 1024
            || !is_lower_hex(&file.sha256, 64)
        {
            return Err("Invalid or duplicate Prism Inference file identity.".into());
        }
        total += file.bytes;
    }
    if total > 1024 * 1024 * 1024 || REQUIRED.iter().any(|name| !names.contains(name)) {
        return Err(
            "Inference release requires all four executables within its size bound.".into(),
        );
    }
    Ok(files)
}

fn verify_inference(
    lock: &Value,
    files: &[InferenceFile],
    destination: &Path,
) -> crate::Result<Value> {
    validate_external_path(destination).map_err(|error| error.to_string())?;
    let entries = fs::read_dir(destination)
        .map_err(|error| error.to_string())?
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(|error| error.to_string())?;
    if entries.len() != files.len()
        || entries.iter().any(|entry| {
            !files
                .iter()
                .any(|file| entry.file_name() == std::ffi::OsStr::new(&file.name))
        })
    {
        return Err("Installed inference inventory differs from its lock.".into());
    }
    for file in files {
        let path = destination.join(&file.name);
        crate::startup_download::verify(&path, file.bytes, &file.sha256)?;
        if file.name.ends_with(".exe") {
            crate::startup_cache::assert_arm64(&path)?;
        }
    }
    Ok(
        json!({"Package": lock["package"], "Version": lock["version"], "Commit": lock["commit"], "Target": lock["target"],
        "Directory": destination, "Executable": destination.join("prism-inference.exe"),
        "TokenWorker": destination.join("bonsai-token-worker.exe"), "SwiftWorker": destination.join("bonsai-swift-worker.exe"),
        "Server": destination.join("prism-inference-server.exe"),
        "Gguf": files.iter().any(|file| file.name == "bonsai-gguf.exe").then(|| destination.join("bonsai-gguf.exe")), "Verified": true}),
    )
}

fn extract_inference(
    archive_path: &Path,
    pin: &Value,
    files: &[InferenceFile],
    staging: &Path,
) -> crate::Result<()> {
    let mut file = open_locked_file(archive_path).map_err(|error| error.to_string())?;
    let bytes = pin["bytes"]
        .as_u64()
        .ok_or("Missing inference archive length.")?;
    if file.metadata().map_err(|error| error.to_string())?.len() != bytes {
        return Err("Inference archive length differs from its lock.".into());
    }
    let digest =
        bonsai_artifacts::hash_reader(&mut file, bytes, &mut vec![0; 65536], &mut || Ok(()))
            .map_err(|error| error.to_string())?;
    if digest != text(pin, "sha256")? {
        return Err("Inference archive SHA-256 differs from its lock.".into());
    }
    let mut archive = ZipArchive::new(file).map_err(|error| error.to_string())?;
    if archive.len() != files.len() {
        return Err("Inference archive inventory differs from its lock.".into());
    }
    fs::create_dir(staging).map_err(|error| error.to_string())?;
    let mut names = BTreeSet::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| error.to_string())?;
        let name = entry
            .name()
            .map_err(|error| error.to_string())?
            .into_owned();
        let expected = files
            .iter()
            .find(|file| file.name == name)
            .ok_or("Unexpected inference archive entry.")?;
        if !names.insert(name)
            || entry.size() != expected.bytes
            || !entry.is_file()
            || entry
                .unix_mode()
                .is_some_and(|mode| mode & 0xf000 == 0xa000)
        {
            return Err(
                "Inference archive contains a duplicate, linked or invalid-sized entry.".into(),
            );
        }
        let path = staging.join(&expected.name);
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|error| error.to_string())?;
        let copied = std::io::copy(&mut entry.by_ref().take(expected.bytes + 1), &mut output)
            .map_err(|error| error.to_string())?;
        if copied != expected.bytes {
            return Err("Inference archive entry exceeded its bound or was truncated.".into());
        }
        output.flush().map_err(|error| error.to_string())?;
        output.sync_all().map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn inference(request: &Request, working: &Path) -> crate::Result<Value> {
    let lock = read_lock(request.lock_path.as_deref(), INFERENCE)?;
    let files = inference_lock(&lock)?;
    let destination = request
        .data_directory
        .join("inference")
        .join(text(&lock, "commit")?)
        .join(text(&lock, "target")?);
    validate_external_path(&destination).map_err(|error| error.to_string())?;
    if !destination.exists() {
        if request.verify_only {
            return Err("Prism Inference is not installed.".into());
        }
        let pin = &lock["asset"];
        let archive = request
            .data_directory
            .join("downloads")
            .join(text(pin, "filename")?);
        receive(request, pin, text(pin, "url")?, &archive, working)?;
        let parent = destination
            .parent()
            .ok_or("Inference installation has no parent.")?;
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        let staging = parent.join(format!(".staging-{}", crate::startup_process::unique()));
        extract_inference(&archive, pin, &files, &staging)?;
        verify_inference(&lock, &files, &staging)?;
        fs::rename(&staging, &destination).map_err(|error| {
            format!(
                "Cannot publish inference installation; staging retained at {}: {error}",
                staging.display()
            )
        })?;
    }
    let mut installed = verify_inference(&lock, &files, &destination)?;
    if request.include_capabilities {
        let discovered = capabilities(&request.data_directory)?;
        installed["Capabilities"] = discovered["Capabilities"].clone();
        installed["CapabilityVerification"] = discovered;
    }
    Ok(installed)
}

#[cfg(windows)]
fn capabilities(data: &Path) -> crate::Result<Value> {
    let (id, creation) = crate::cpu_check::parent_identity(None, None)?;
    crate::inference_capabilities::run(json!({"schemaVersion": 1,
        "dataDirectory": data, "parentProcessId": id, "parentCreationTimeFileTime": creation}))
}

#[cfg(not(windows))]
fn capabilities(_data: &Path) -> crate::Result<Value> {
    Err("Inference capability discovery requires Windows.".into())
}

fn reference_plan(data: &Path, selection: &str) -> crate::Result<Vec<Value>> {
    let selected = selection.to_ascii_lowercase();
    if !["all", "canary", "qwentts"].contains(&selected.as_str()) {
        return Err("Unsupported reference speech model selection.".into());
    }
    let mut snapshots = Vec::new();
    for (kind, directory, source, model, revision, names) in [
        (
            "Canary",
            "canary",
            include_str!("../../canary-asr/model.lock.json"),
            "nvidia/canary-qwen-2.5b",
            "b1469e1bba1cfe140205529c79c434ca47180960",
            &["config.json", "model.safetensors"][..],
        ),
        (
            "CanaryTokenizer",
            "canary-tokenizer",
            include_str!("../../canary-asr/tokenizer.lock.json"),
            "Qwen/Qwen3-1.7B",
            "70d244cc86ccca08cf5af4e1e306ecf908b1ad5e",
            &[
                "config.json",
                "generation_config.json",
                "merges.txt",
                "tokenizer_config.json",
                "vocab.json",
                "tokenizer.json",
            ][..],
        ),
        (
            "QwenTts",
            "qwen-tts",
            include_str!("../../qwen-tts/model.lock.json"),
            "Qwen/Qwen3-TTS-12Hz-1.7B-VoiceDesign",
            "5ecdb67327fd37bb2e042aab12ff7391903235d3",
            &[
                "config.json",
                "generation_config.json",
                "merges.txt",
                "tokenizer_config.json",
                "vocab.json",
                "model.safetensors",
                "preprocessor_config.json",
                "speech_tokenizer/config.json",
                "speech_tokenizer/configuration.json",
                "speech_tokenizer/model.safetensors",
                "speech_tokenizer/preprocessor_config.json",
            ][..],
        ),
    ] {
        if selected == "canary" && kind == "QwenTts" || selected == "qwentts" && kind != "QwenTts" {
            continue;
        }
        let lock: Value = serde_json::from_str(source).map_err(|error| error.to_string())?;
        let files = lock["files"]
            .as_object()
            .ok_or("Invalid reference speech file inventory.")?;
        if lock["schema_version"] != 1
            || lock["model"] != model
            || lock["revision"] != revision
            || files.len() != names.len()
            || names.iter().any(|name| !files.contains_key(*name))
        {
            return Err("Invalid immutable reference speech model lock.".into());
        }
        let snapshot = data.join("speech").join(directory).join(revision);
        let mut plans = Vec::new();
        for (name, pin) in files {
            let url = format!("https://huggingface.co/{model}/resolve/{revision}/{name}");
            if pin["url"] != url
                || pin["bytes"]
                    .as_u64()
                    .is_none_or(|bytes| bytes == 0 || bytes > i64::MAX.unsigned_abs())
                || !is_lower_hex(text(pin, "sha256")?, 64)
            {
                return Err(
                    "Reference speech artifact differs from its immutable URL, length or SHA-256."
                        .into(),
                );
            }
            plans.push(json!({"Name": name, "Path": snapshot.join(name), "Uri": url, "Bytes": pin["bytes"], "Sha256": pin["sha256"]}));
        }
        snapshots.push(json!({"Kind": kind, "SnapshotPath": snapshot, "Files": plans}));
    }
    Ok(snapshots)
}

fn snapshot_entries(snapshot: &Value, partial: bool) -> crate::Result<()> {
    let root = Path::new(text(snapshot, "SnapshotPath")?);
    validate_external_path(root).map_err(|error| error.to_string())?;
    if !root.exists() {
        return Ok(());
    }
    let files = snapshot["Files"]
        .as_array()
        .ok_or("Missing reference speech plan.")?;
    let names = files
        .iter()
        .map(|file| text(file, "Name"))
        .collect::<crate::Result<BTreeSet<_>>>()?;
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let metadata = fs::symlink_metadata(entry.path()).map_err(|error| error.to_string())?;
            if is_redirect(&metadata) {
                return Err("Reference speech snapshots cannot contain redirect entries.".into());
            }
            let path = entry.path();
            let relative = path
                .strip_prefix(root)
                .map_err(|error| error.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            if metadata.is_dir() {
                if !names
                    .iter()
                    .any(|name| name.starts_with(&format!("{relative}/")))
                {
                    return Err("Unexpected reference speech snapshot directory.".into());
                }
                pending.push(path);
            } else if !metadata.is_file()
                || !(names.contains(relative.as_str())
                    || partial
                        && relative
                            .strip_suffix(".partial")
                            .is_some_and(|name| names.contains(name))
                    || relative
                        .strip_suffix(".download.lock")
                        .is_some_and(|name| names.contains(name))
                    || !relative.contains('/')
                        && [
                            "README.md",
                            "LICENSE",
                            "LICENSE.txt",
                            "NOTICE",
                            "NOTICE.txt",
                            ".gitattributes",
                        ]
                        .contains(&relative.as_str()))
            {
                return Err("Unexpected reference speech snapshot file; preserve it outside the snapshot before retrying.".into());
            }
        }
    }
    Ok(())
}

fn reference_speech(request: &Request, working: &Path) -> crate::Result<Value> {
    let snapshots = reference_plan(&request.data_directory, &request.model)?;
    for snapshot in &snapshots {
        snapshot_entries(snapshot, !request.verify_only)?;
    }
    let mut result = json!({"Status": "verified", "CanarySnapshotPath": null,
        "CanaryTokenizerSnapshotPath": null, "TtsSnapshotPath": null, "TotalBytes": 0});
    let mut total = 0_u64;
    for snapshot in &snapshots {
        let property = match text(snapshot, "Kind")? {
            "Canary" => "CanarySnapshotPath",
            "CanaryTokenizer" => "CanaryTokenizerSnapshotPath",
            _ => "TtsSnapshotPath",
        };
        result[property] = snapshot["SnapshotPath"].clone();
        for file in snapshot["Files"]
            .as_array()
            .ok_or("Missing reference speech plan.")?
        {
            let pin = json!({"bytes": file["Bytes"], "sha256": file["Sha256"]});
            receive(
                request,
                &pin,
                text(file, "Uri")?,
                Path::new(text(file, "Path")?),
                working,
            )?;
            total = total
                .checked_add(file["Bytes"].as_u64().ok_or("Invalid speech length.")?)
                .ok_or("Reference speech artifact total overflow.")?;
        }
        // Download locks are persistent synchronization files, never model artifacts.
        snapshot_entries(snapshot, false)?;
    }
    result["TotalBytes"] = json!(total);
    Ok(result)
}

/// Provision or verify a selected locked artifact using native ownership and ZIP policy.
///
/// # Errors
/// Rejects unsafe paths, mutable or malformed locks, mismatched existing files,
/// invalid archives, unsupported options and failed bounded downloads/probes.
pub fn run(value: Value) -> crate::Result<Value> {
    let request: Request = serde_json::from_value(value).map_err(|error| error.to_string())?;
    validate_external_path(&request.data_directory).map_err(|error| error.to_string())?;
    if request.lock_path.is_some()
        && !matches!(
            request.operation,
            Operation::Model | Operation::Engine | Operation::Inference
        )
        || request.include_capabilities
            && (request.operation != Operation::Inference || request.lock_path.is_some())
        || request.include_help && request.operation != Operation::Engine
        || request.model != "All"
            && !matches!(
                request.operation,
                Operation::Speech | Operation::SpeechReference
            )
        || request.tts_quantization != "Q8_0" && request.operation != Operation::Speech
    {
        return Err("Provisioning options do not apply to the selected operation.".into());
    }
    let working = working_directory(&request)?;
    match request.operation {
        Operation::Model => model(&request, &working),
        Operation::Adapter => adapter(&request, &working),
        Operation::Engine => engine(&request, &working),
        Operation::Inference => inference(&request, &working),
        Operation::Speech => crate::native_startup::models(
            &request.data_directory,
            &request.model,
            &request.tts_quantization,
            request.verify_only,
            false,
            &working,
        ),
        Operation::SpeechReference => reference_speech(&request, &working),
    }
}

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "kebab-case", deny_unknown_fields)]
enum ArtifactRequest {
    Path {
        path: PathBuf,
    },
    Arm64 {
        path: PathBuf,
    },
    Verify {
        path: PathBuf,
        bytes: u64,
        sha256: String,
    },
    #[serde(rename_all = "camelCase")]
    Receive {
        uri: String,
        path: PathBuf,
        bytes: u64,
        sha256: String,
        working_directory: PathBuf,
        timeout_seconds: Option<u64>,
    },
    RuntimeLock {
        path: Option<PathBuf>,
    },
}

/// Validate paths, verify/download a locked file or inspect a runtime lock in Rust.
///
/// # Errors
/// Rejects unsafe storage, unsupported requests, invalid identities and failed
/// bounded artifact transfer or verification. Existing files are never replaced.
pub fn artifact_run(value: Value) -> crate::Result<Value> {
    match serde_json::from_value(value).map_err(|error| error.to_string())? {
        ArtifactRequest::Path { path } => {
            validate_external_path(&path).map_err(|error| error.to_string())?;
            Ok(json!({"Path": path}))
        }
        ArtifactRequest::Arm64 { path } => {
            crate::startup_cache::assert_arm64(&path)?;
            Ok(json!({"Status": "verified", "Path": path, "Architecture": "Arm64"}))
        }
        ArtifactRequest::Verify {
            path,
            bytes,
            sha256,
        } => {
            crate::startup_download::verify(&path, bytes, &sha256)?;
            Ok(json!({"Status": "verified", "Path": path, "Bytes": bytes, "Sha256": sha256}))
        }
        ArtifactRequest::Receive {
            uri,
            path,
            bytes,
            sha256,
            working_directory,
            timeout_seconds,
        } => {
            let timeout = timeout_seconds.unwrap_or(7200);
            if !(1..=86400).contains(&timeout) {
                return Err("Artifact timeout must be between one and 86400 seconds.".into());
            }
            crate::startup_download::receive_with_timeout(
                &uri,
                &path,
                bytes,
                &sha256,
                &working_directory,
                std::time::Duration::from_secs(timeout),
            )?;
            Ok(json!({"Status": "verified", "Path": path, "Bytes": bytes, "Sha256": sha256}))
        }
        ArtifactRequest::RuntimeLock { path } => {
            let lock = read_lock(path.as_deref(), REFERENCE)?;
            model_lock(&lock)?;
            Ok(lock)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiled_reference_metadata_and_memory_estimates_reconcile_with_artifact_lock()
    -> crate::Result<()> {
        let lock = read_lock(None, REFERENCE)?;
        let metadata: Value =
            serde_json::from_str(include_str!("../../../config/model.metadata.json"))
                .map_err(|error| error.to_string())?;
        let profile: Value =
            serde_json::from_str(include_str!("../../../config/profiles/reference-cpu.json"))
                .map_err(|error| error.to_string())?;
        assert_eq!(metadata["ModelRevision"], lock["model"]["revision"]);
        assert_eq!(metadata["ArtifactSha256"], lock["model"]["sha256"]);
        assert_eq!(metadata["FileBytes"], lock["model"]["bytes"]);
        assert_eq!(metadata["TensorPayloadRead"], false);
        let count = metadata["TensorTypeHistogram"]
            .as_array()
            .ok_or("Missing tensor histogram.")?
            .iter()
            .map(|row| {
                row["Count"]
                    .as_u64()
                    .ok_or_else(|| "Invalid tensor count.".to_owned())
            })
            .collect::<crate::Result<Vec<_>>>()?
            .iter()
            .sum::<u64>();
        assert_eq!(metadata["TensorCount"], count);
        assert_eq!(profile["launchEnabled"], false);
        assert_eq!(profile["estimatedWeightBytes"], lock["model"]["bytes"]);
        let ceiling = [
            "estimatedWeightBytes",
            "estimatedKvBytes",
            "estimatedRecurrentBytes",
            "estimatedRuntimeHeadroomBytes",
        ]
        .iter()
        .map(|name| {
            profile[*name]
                .as_u64()
                .ok_or_else(|| "Missing memory estimate.".to_owned())
        })
        .collect::<crate::Result<Vec<_>>>()?
        .iter()
        .sum::<u64>();
        assert_eq!(profile["serviceCeilingBytes"], ceiling);
        Ok(())
    }

    #[test]
    fn provisioning_rejects_inapplicable_and_custom_capability_options_before_io() {
        let data = std::env::temp_dir().join("Bonsai-Provision-RejectedOptions");
        for extra in [
            json!({"operation": "inference", "includeCapabilities": true, "lockPath": data.join("untrusted.json")}),
            json!({"operation": "model", "includeHelp": true}),
            json!({"operation": "adapter", "lockPath": data.join("untrusted.json")}),
            json!({"operation": "speech", "model": "Arbitrary"}),
            json!({"operation": "inference", "arguments": ["--tools"]}),
        ] {
            let mut request = extra;
            request["dataDirectory"] = json!(data);
            assert!(run(request).is_err());
        }
        assert!(!data.exists());
    }

    #[test]
    fn immutable_provisioning_locks_reject_mutable_and_unsafe_identities() -> crate::Result<()> {
        let original: Value = serde_json::from_str(REFERENCE).map_err(|error| error.to_string())?;
        model_lock(&original)?;
        for (pointer, value) in [
            ("/model/revision", json!("main")),
            ("/runtime/commit", json!("HEAD")),
            ("/model/filename", json!("../model.gguf")),
            (
                "/runtime/asset/url",
                json!("https://github.com/other/release.zip"),
            ),
            ("/model/bytes", json!(1.5)),
            ("/model/sha256", json!("bad")),
        ] {
            let mut invalid = original.clone();
            *invalid
                .pointer_mut(pointer)
                .ok_or("Missing lock fixture field.")? = value;
            assert!(model_lock(&invalid).is_err(), "{pointer}");
        }
        for invalid in [json!([]), json!(null), json!(true)] {
            assert!(model_lock(&invalid).is_err());
        }
        Ok(())
    }

    #[test]
    fn inference_release_lock_rejects_missing_duplicate_and_unbound_files() -> crate::Result<()> {
        let original: Value = serde_json::from_str(INFERENCE).map_err(|error| error.to_string())?;
        assert_eq!(
            inference_lock(&original)?.len(),
            original["files"]
                .as_array()
                .ok_or("Missing inventory.")?
                .len()
        );
        for change in 0..5 {
            let mut invalid = original.clone();
            match change {
                0 => invalid["commit"] = json!("main"),
                1 => invalid["files"][0]["name"] = json!("../escape.exe"),
                2 => invalid["files"][0] = invalid["files"][1].clone(),
                3 => invalid["asset"]["url"] = json!("https://example.invalid/archive.zip"),
                _ => invalid["files"][0]["bytes"] = json!(536_870_913_u64),
            }
            assert!(inference_lock(&invalid).is_err());
        }
        Ok(())
    }

    #[test]
    fn reference_speech_selection_keeps_recognizer_and_tokenizer_together() -> crate::Result<()> {
        let root = std::env::temp_dir().join("Bonsai-reference-plan-only");
        let all = reference_plan(&root, "All")?;
        let canary = reference_plan(&root, "Canary")?;
        assert_eq!(all.len(), 3);
        assert_eq!(canary.len(), 2);
        assert_eq!(canary[0]["Kind"], "Canary");
        assert_eq!(canary[1]["Kind"], "CanaryTokenizer");
        assert_eq!(reference_plan(&root, "QwenTts")?.len(), 1);
        assert!(reference_plan(&root, "CanaryTokenizer").is_err());
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn inference_installation_verifies_cached_archive_and_rejects_changed_workers()
    -> crate::Result<()> {
        use sha2::{Digest as _, Sha256};
        use std::io::Cursor;
        use zip::{ZipWriter, write::SimpleFileOptions};

        let root = std::env::temp_dir().join(format!(
            "Bonsai-InferenceInstall-{}",
            crate::startup_process::unique()
        ));
        fs::create_dir_all(root.join("downloads")).map_err(|error| error.to_string())?;
        let mut image = vec![0; 128];
        image[..2].copy_from_slice(b"MZ");
        image[60..64].copy_from_slice(&64_u32.to_le_bytes());
        image[64..70].copy_from_slice(&[b'P', b'E', 0, 0, 0x64, 0xaa]);
        let names = [
            "prism-inference.exe",
            "bonsai-token-worker.exe",
            "bonsai-swift-worker.exe",
            "prism-inference-server.exe",
        ];
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        for name in names {
            zip.start_file(name, SimpleFileOptions::default())
                .map_err(|error| error.to_string())?;
            zip.write_all(&image).map_err(|error| error.to_string())?;
        }
        let bytes = zip
            .finish()
            .map_err(|error| error.to_string())?
            .into_inner();
        let filename = "prism-inference-0.1.0-aarch64-pc-windows-msvc.zip";
        fs::write(root.join("downloads").join(filename), &bytes)
            .map_err(|error| error.to_string())?;
        let lock = json!({"schemaVersion": 1, "package": "prism-inference", "version": "0.1.0",
            "repository": "https://github.com/symbiogenesis/prism-inference", "release": "v0.1.0",
            "commit": "1234567890123456789012345678901234567890", "target": "aarch64-pc-windows-msvc",
            "asset": {"filename": filename, "bytes": bytes.len(), "sha256": crate::identity::hex(&Sha256::digest(&bytes)),
                "url": format!("https://github.com/symbiogenesis/prism-inference/releases/download/v0.1.0/{filename}")},
            "files": names.iter().map(|name| json!({"name": name, "bytes": image.len(), "sha256": crate::identity::hex(&Sha256::digest(&image))})).collect::<Vec<_>>()});
        let lock_path = root.join("lock.json");
        fs::write(
            &lock_path,
            serde_json::to_vec(&lock).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        let mut request = json!({"operation": "inference", "dataDirectory": root, "lockPath": lock_path, "verifyOnly": true});
        assert!(run(request.clone()).is_err());
        assert!(!root.join("inference").exists());
        request["verifyOnly"] = json!(false);
        let installed = run(request.clone())?;
        assert_eq!(installed["Verified"], true);
        request["verifyOnly"] = json!(true);
        assert_eq!(run(request.clone())?["Directory"], installed["Directory"]);
        let worker = Path::new(text(&installed, "SwiftWorker")?);
        fs::write(worker, b"changed worker").map_err(|error| error.to_string())?;
        assert!(run(request).is_err());
        assert_eq!(
            fs::read(worker).map_err(|error| error.to_string())?,
            b"changed worker"
        );
        fs::remove_dir_all(root).map_err(|error| error.to_string())
    }

    #[cfg(windows)]
    #[test]
    fn inference_archive_rejects_traversal_nested_entries_and_duplicate_inventory()
    -> crate::Result<()> {
        use sha2::{Digest as _, Sha256};
        use std::io::Cursor;
        use zip::{ZipWriter, write::SimpleFileOptions};

        let root = std::env::temp_dir().join(format!(
            "Bonsai-InferenceArchive-{}",
            crate::startup_process::unique()
        ));
        fs::create_dir(&root).map_err(|error| error.to_string())?;
        let names = [
            "prism-inference.exe",
            "bonsai-token-worker.exe",
            "bonsai-swift-worker.exe",
            "prism-inference-server.exe",
        ];
        let files = names
            .iter()
            .map(|name| InferenceFile {
                name: (*name).into(),
                bytes: 1,
                sha256: crate::identity::hex(&Sha256::digest([0])),
            })
            .collect::<Vec<_>>();
        for (index, first) in [
            "../escape.exe",
            "bonsai-token-worker.exe",
            "nested/prism-inference.exe",
        ]
        .iter()
        .enumerate()
        {
            let mut archive = ZipWriter::new(Cursor::new(Vec::new()));
            for name in [*first, names[1], names[2], names[3]] {
                archive
                    .start_file(name, SimpleFileOptions::default())
                    .map_err(|error| error.to_string())?;
                archive.write_all(&[0]).map_err(|error| error.to_string())?;
            }
            let bytes = archive
                .finish()
                .map_err(|error| error.to_string())?
                .into_inner();
            let archive_path = root.join(format!("archive-{index}.zip"));
            fs::write(&archive_path, &bytes).map_err(|error| error.to_string())?;
            let pin = json!({"bytes": bytes.len(), "sha256": crate::identity::hex(&Sha256::digest(&bytes))});
            assert!(
                extract_inference(
                    &archive_path,
                    &pin,
                    &files,
                    &root.join(format!("staging-{index}"))
                )
                .is_err()
            );
            assert!(!root.join("escape.exe").exists());
        }
        fs::remove_dir_all(root).map_err(|error| error.to_string())
    }

    #[cfg(windows)]
    #[test]
    fn reference_speech_snapshot_rejects_extra_files_and_incomplete_cache_entries()
    -> crate::Result<()> {
        let root = std::env::temp_dir().join(format!(
            "Bonsai-ReferenceInventory-{}",
            crate::startup_process::unique()
        ));
        fs::create_dir(&root).map_err(|error| error.to_string())?;
        let snapshot = json!({"SnapshotPath": root, "Files": [{"Name": "model.bin"}, {"Name": "codec/config.json"}]});
        fs::write(root.join("model.bin.partial"), b"partial").map_err(|error| error.to_string())?;
        snapshot_entries(&snapshot, true)?;
        assert!(snapshot_entries(&snapshot, false).is_err());
        fs::remove_file(root.join("model.bin.partial")).map_err(|error| error.to_string())?;
        fs::write(root.join("model.bin.download.lock"), []).map_err(|error| error.to_string())?;
        snapshot_entries(&snapshot, false)?;
        fs::write(root.join("unreviewed.bin"), []).map_err(|error| error.to_string())?;
        assert!(snapshot_entries(&snapshot, true).is_err());
        fs::remove_dir_all(root).map_err(|error| error.to_string())
    }
}
