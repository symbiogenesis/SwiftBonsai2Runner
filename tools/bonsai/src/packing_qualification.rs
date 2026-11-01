//! Hosted-only complete PTQ1 reproduction with bounded owned worker processes.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::{Read as _, Write as _},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use bonsai_artifacts::{VerifiedFile, is_redirect, validate_external_path};
use serde::Deserialize;
use serde_json::{Value, json};

const EXPECTED: &str = include_str!("../../../config/swift.ptq1-export.json");
const ASSESSMENT: &str = include_str!("../../../config/swift-assessment.lock.json");
const RUNTIME: &str = include_str!("../../../config/prism-inference.lock.json");
const MAX_RECEIPT: usize = 1024 * 1024;
const SOURCE_SECONDS: u64 = 1800;
const RUNTIME_SECONDS: u64 = 120;
const EXPORT_SECONDS: u64 = 600;
const SCAN_SECONDS: u64 = 300;
const SCAN_PROCESS_SECONDS: u64 = 305;

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "kebab-case", deny_unknown_fields)]
enum Request {
    Limits,
    Execute {
        #[serde(rename = "dataDirectory")]
        data_directory: PathBuf,
        #[serde(rename = "evidenceDirectory")]
        evidence_directory: PathBuf,
    },
    Download {
        #[serde(rename = "dataDirectory")]
        data_directory: PathBuf,
        artifact: Artifact,
    },
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Artifact {
    Source,
    Runtime,
}

fn limits() -> Value {
    json!({"originalDownloadSeconds": SOURCE_SECONDS, "runtimeDownloadSeconds": RUNTIME_SECONDS,
        "exporterProcessSeconds": EXPORT_SECONDS, "independentVerificationSeconds": SCAN_SECONDS,
        "independentProcessSeconds": SCAN_PROCESS_SECONDS,
        "workflowStepMinutes": 50, "workflowJobMinutes": 60, "retries": 0})
}

fn context(native: bool, environment: &BTreeMap<String, String>) -> crate::Result<()> {
    let value = |key: &str| environment.get(key).map_or("", String::as_str);
    let number = |key| {
        let text = value(key);
        matches!(text.as_bytes().first(), Some(b'1'..=b'9'))
            && text.bytes().all(|byte| byte.is_ascii_digit())
            && text.parse::<u64>().is_ok()
    };
    if !native
        || value("GITHUB_ACTIONS") != "true"
        || value("RUNNER_ENVIRONMENT") != "github-hosted"
        || value("GITHUB_REPOSITORY") != "symbiogenesis/SwiftBonsai2Runner"
        || value("GITHUB_REF") != "refs/heads/main"
        || value("RUNNER_ARCH") != "ARM64"
        || !crate::identity::is_lower_hex(value("GITHUB_SHA"), 40)
        || !number("GITHUB_RUN_ID")
        || !number("GITHUB_RUN_ATTEMPT")
        || !crate::identity::is_lower_hex(value("BONSAI_TOOLS_SOURCE_ID"), 64)
    {
        return Err("PTQ1 packing qualification requires exact-main GitHub-hosted native Windows ARM64 CI; owner-device execution is disabled.".into());
    }
    Ok(())
}

fn environment() -> BTreeMap<String, String> {
    [
        "GITHUB_ACTIONS",
        "RUNNER_ENVIRONMENT",
        "GITHUB_REPOSITORY",
        "GITHUB_REF",
        "RUNNER_ARCH",
        "GITHUB_SHA",
        "GITHUB_RUN_ID",
        "GITHUB_RUN_ATTEMPT",
        "BONSAI_TOOLS_SOURCE_ID",
    ]
    .into_iter()
    .map(|name| (name.into(), std::env::var(name).unwrap_or_default()))
    .collect()
}

fn text<'a>(value: &'a Value, field: &str) -> crate::Result<&'a str> {
    value[field]
        .as_str()
        .ok_or_else(|| format!("Missing compiled packing field: {field}"))
}

fn count(value: &Value, field: &str) -> crate::Result<u64> {
    value[field]
        .as_u64()
        .ok_or_else(|| format!("Missing compiled packing count: {field}"))
}

fn external(path: &Path) -> crate::Result<()> {
    validate_external_path(path).map_err(|error| error.to_string())
}

struct Temporary(PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn save(path: &Path, value: &Value) -> crate::Result<()> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    if bytes.len() > MAX_RECEIPT {
        return Err("Packing qualification receipt exceeds one MiB.".into());
    }
    external(path)?;
    let temporary =
        Temporary(path.with_extension(format!("{}.tmp", crate::startup_process::unique())));
    external(&temporary.0)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary.0)
        .map_err(|error| error.to_string())?;
    output
        .write_all(&bytes)
        .and_then(|()| output.sync_all())
        .map_err(|error| error.to_string())?;
    drop(output);
    fs::rename(&temporary.0, path).map_err(|error| error.to_string())
}

fn partial(path: &Path, expected: u64) -> Value {
    let mut result = json!({"status":"absent", "expectedBytes":expected, "partialBytes":null});
    match fs::symlink_metadata(path) {
        Ok(metadata)
            if metadata.is_file()
                && !is_redirect(&metadata)
                && expected > 0
                && metadata.len() <= expected =>
        {
            result["status"] = "bounded-partial".into();
            result["partialBytes"] = metadata.len().into();
        }
        Ok(_) => result["status"] = "not-regular-or-oversized".into(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => result["status"] = "unavailable".into(),
    }
    result
}

fn matches(expected: &Value, export: &Value, packing: &Value, digest: &str) -> crate::Result<()> {
    for name in [
        "SchemaVersion",
        "CodecCommit",
        "SourceBytes",
        "OutputBytes",
        "SourceSha256",
        "OutputSha256",
        "SourceRevision",
        "ConvertedTensorCount",
        "ConvertedBlocks",
        "ExactPq2ReconstructionVerified",
        "NonPq2TensorBytesCopiedVerbatim",
        "TokenizerMetadataSha256",
    ] {
        if export.get(name).is_none() || export[name] != expected[name] {
            return Err(format!(
                "The reproduced PTQ1 export differs from its committed receipt: {name}."
            ));
        }
    }
    if !crate::identity::is_lower_hex(digest, 64) {
        return Err("A complete export receipt digest is required.".into());
    }
    let required = json!({"schemaVersion":1, "kind":"independent-current-weight-ptq1-packing", "status":"matched",
        "sourceBytes":expected["SourceBytes"], "sourceSha256":expected["SourceSha256"],
        "outputBytes":expected["OutputBytes"], "outputSha256":expected["OutputSha256"],
        "convertedTensors":expected["ConvertedTensorCount"], "reconstructedBlocks":expected["ConvertedBlocks"],
        "exportReceiptSha256":digest, "headerBytesPreservedExceptDeclaredEdits":true,
        "unchangedOtherTensorBytes":57_767_936, "workingPayloadBufferBytes":131_072});
    for (name, value) in required
        .as_object()
        .ok_or("Invalid packing requirements.")?
    {
        if packing.get(name) != Some(value) {
            return Err(format!(
                "Independent PTQ1 packing differs from its complete committed inventory: {name}."
            ));
        }
    }
    Ok(())
}

fn runtime_directory(data: &Path, runtime: &Value) -> crate::Result<PathBuf> {
    Ok(data
        .join("inference")
        .join(text(runtime, "commit")?)
        .join(text(runtime, "target")?))
}

fn retained_runtime(directory: &Path, runtime: &Value) -> crate::Result<Vec<VerifiedFile>> {
    external(directory)?;
    let pins = runtime["files"]
        .as_array()
        .ok_or("Missing runtime inventory.")?;
    let mut names = BTreeSet::new();
    let mut guards = Vec::new();
    for pin in pins {
        let name = text(pin, "name")?;
        if !names.insert(name) || Path::new(name).components().count() != 1 {
            return Err("Invalid runtime inventory.".into());
        }
        let path = directory.join(name);
        guards.push(
            VerifiedFile::verify(&path, count(pin, "bytes")?, text(pin, "sha256")?)
                .map_err(|error| error.to_string())?,
        );
        if name.ends_with(".exe") {
            crate::startup_cache::assert_arm64(&path)?;
        }
    }
    let entries = fs::read_dir(directory)
        .map_err(|error| error.to_string())?
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(|error| error.to_string())?;
    if entries.len() != names.len()
        || entries.iter().any(|entry| {
            entry
                .file_name()
                .to_str()
                .is_none_or(|name| !names.contains(name))
        })
    {
        return Err("Installed inference inventory differs from its compiled lock.".into());
    }
    Ok(guards)
}

fn stage_runtime(data: &Path, runtime: &Value, archive_path: &Path) -> crate::Result<()> {
    let destination = runtime_directory(data, runtime)?;
    if destination.exists() {
        retained_runtime(&destination, runtime)?;
        return Ok(());
    }
    let asset = &runtime["asset"];
    let _guard = VerifiedFile::verify(archive_path, count(asset, "bytes")?, text(asset, "sha256")?)
        .map_err(|error| error.to_string())?;
    let input =
        bonsai_artifacts::open_locked_file(archive_path).map_err(|error| error.to_string())?;
    let mut archive = zip::ZipArchive::new(input).map_err(|error| error.to_string())?;
    let pins = runtime["files"]
        .as_array()
        .ok_or("Missing runtime inventory.")?;
    if archive.len() != pins.len() {
        return Err("Inference archive inventory differs from its compiled lock.".into());
    }
    let stage =
        destination.with_file_name(format!(".staging-{}", crate::startup_process::unique()));
    external(&stage)?;
    fs::create_dir_all(stage.parent().ok_or("Runtime has no parent.")?)
        .map_err(|error| error.to_string())?;
    fs::create_dir(&stage).map_err(|error| error.to_string())?;
    let mut seen = BTreeSet::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| error.to_string())?;
        let pin = pins
            .iter()
            .find(|pin| pin["name"].as_str() == Some(entry.name()))
            .ok_or("Unpinned inference archive entry.")?;
        let name = text(pin, "name")?;
        let length = count(pin, "bytes")?;
        if !seen.insert(name)
            || !entry.is_file()
            || entry.is_symlink()
            || entry.size() != length
            || Path::new(name).components().count() != 1
        {
            return Err("Unexpected, duplicate or linked inference archive entry.".into());
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(stage.join(name))
            .map_err(|error| error.to_string())?;
        let copied = std::io::copy(&mut entry.by_ref().take(length + 1), &mut file)
            .map_err(|error| error.to_string())?;
        if copied != length {
            return Err("Inference archive entry exceeded its locked size.".into());
        }
        file.sync_all().map_err(|error| error.to_string())?;
    }
    retained_runtime(&stage, runtime)?;
    fs::rename(stage, destination).map_err(|error| error.to_string())
}

fn download(data: &Path, artifact: Artifact) -> crate::Result<Value> {
    external(data)?;
    let expected: Value = serde_json::from_str(EXPECTED).map_err(|error| error.to_string())?;
    let assessment: Value = serde_json::from_str(ASSESSMENT).map_err(|error| error.to_string())?;
    let runtime: Value = serde_json::from_str(RUNTIME).map_err(|error| error.to_string())?;
    let working = data.join("work");
    fs::create_dir_all(&working).map_err(|error| error.to_string())?;
    let (path, uri, length, digest) = match artifact {
        Artifact::Source => (
            data.join(text(&expected, "SourceFilename")?),
            format!(
                "https://huggingface.co/{}/resolve/{}/{}",
                text(&assessment["model"], "repository")?,
                text(&assessment["model"], "revision")?,
                text(&assessment["model"], "filename")?
            ),
            count(&expected, "SourceBytes")?,
            text(&expected, "SourceSha256")?,
        ),
        Artifact::Runtime => (
            data.join("downloads")
                .join(text(&runtime["asset"], "filename")?),
            text(&runtime["asset"], "url")?.into(),
            count(&runtime["asset"], "bytes")?,
            text(&runtime["asset"], "sha256")?,
        ),
    };
    crate::startup_download::receive(&uri, &path, length, digest, &working)?;
    if matches!(artifact, Artifact::Runtime) {
        stage_runtime(data, &runtime, &path)?;
    }
    Ok(json!({"status":"verified"}))
}

fn command(
    executable: &Path,
    arguments: &[std::ffi::OsString],
    work: &Path,
    timeout: u64,
    name: &str,
) -> crate::Result<Value> {
    let output = crate::startup_process::run(
        executable.as_os_str(),
        arguments,
        work,
        &BTreeMap::new(),
        Duration::from_secs(timeout),
        &work.join(format!("{name}.log")),
    )?;
    if output.code != 0 {
        return Err(format!(
            "Packing phase {name} failed; inspect retained process logs."
        ));
    }
    let result: Value = serde_json::from_str(&output.stdout).map_err(|error| error.to_string())?;
    if !result.is_object() {
        return Err("Packing worker must return a JSON object.".into());
    }
    Ok(result)
}

fn worker(
    tool: &Path,
    work: &Path,
    command_name: &str,
    request: &Value,
    timeout: u64,
    phase: &str,
) -> crate::Result<Value> {
    let path = work.join(format!("{phase}-request.json"));
    save(&path, request)?;
    command(
        tool,
        &[
            command_name.into(),
            "--request".into(),
            path.into_os_string(),
        ],
        work,
        timeout,
        phase,
    )
}

struct Receipt {
    value: Value,
    clock: Instant,
    phase_started: Duration,
    path: PathBuf,
}
impl Receipt {
    fn phase(&mut self, name: &str) -> crate::Result<()> {
        let now = self.clock.elapsed();
        let previous = json!({"phase":self.value["phase"], "startedElapsedSeconds":self.phase_started.as_secs_f64(), "elapsedSeconds":(now - self.phase_started).as_secs_f64()});
        self.value["phaseDurations"]
            .as_array_mut()
            .ok_or("Missing phase attribution.")?
            .push(previous);
        self.value["phase"] = name.into();
        self.value["phaseStartedElapsedSeconds"] = now.as_secs_f64().into();
        self.phase_started = now;
        save(&self.path, &self.value)
    }
}

fn execute_steps(
    data: &Path,
    evidence: &Path,
    receipt: &mut Receipt,
    tool: &Path,
    expected: &Value,
    runtime: &Value,
) -> crate::Result<()> {
    let assessment: Value = serde_json::from_str(ASSESSMENT).map_err(|error| error.to_string())?;
    for (field, expected_field) in [
        ("sha256", "SourceSha256"),
        ("bytes", "SourceBytes"),
        ("revision", "SourceRevision"),
        ("repository", "SourceRepository"),
    ] {
        if assessment["model"][field] != expected[expected_field] {
            return Err(
                "Complete export receipt differs from the compiled original PQ2 identity.".into(),
            );
        }
    }
    fs::write(evidence.join("expected-export.json"), EXPECTED)
        .map_err(|error| error.to_string())?;
    crate::startup_cache::assert_arm64(tool)?;
    receipt.value["expectedExportReceiptSha256"] =
        crate::identity::sha256(EXPECTED.as_bytes()).into();
    receipt.value["maintenanceTool"] = json!({"sha256":crate::startup_cache::digest_file(tool)?, "bytes":fs::metadata(tool).map_err(|error| error.to_string())?.len(), "sourceId":std::env::var("BONSAI_TOOLS_SOURCE_ID").unwrap_or_default()});
    receipt.value["exporterRuntime"] = json!({"revision":runtime["commit"], "archiveSha256":runtime["asset"]["sha256"], "gguf":runtime["files"].as_array().ok_or("Missing exporter pins.")?.iter().filter(|pin| pin["name"] == "bonsai-gguf.exe").collect::<Vec<_>>()});
    fs::create_dir_all(data).map_err(|error| error.to_string())?;
    #[cfg(windows)]
    if bonsai_windows_process::available_disk_bytes(data).map_err(|error| error.to_string())?
        < count(expected, "SourceBytes")? + count(expected, "OutputBytes")? + 536_870_912
    {
        return Err(
            "Hosted packing qualification needs both complete artifacts and 512 MiB reserve."
                .into(),
        );
    }
    let work = data.join("work");
    fs::create_dir_all(&work).map_err(|error| error.to_string())?;
    for (phase, artifact, timeout) in [
        ("original-pq2-download", "source", SOURCE_SECONDS),
        ("pinned-exporter-provisioning", "runtime", RUNTIME_SECONDS),
    ] {
        receipt.phase(phase)?;
        worker(
            tool,
            &work,
            "packing-qualification",
            &json!({"operation":"download", "dataDirectory":data, "artifact":artifact}),
            timeout,
            phase,
        )?;
    }
    let directory = runtime_directory(data, runtime)?;
    let _guards = retained_runtime(&directory, runtime)?;
    let source = data.join(text(expected, "SourceFilename")?);
    let output = data.join(text(expected, "OutputFilename")?);
    receipt.phase("ptq1-reproduction")?;
    let mut export = command(
        &directory.join("bonsai-gguf.exe"),
        &[
            "export-ptq1".into(),
            "--model".into(),
            source.as_os_str().to_owned(),
            "--output".into(),
            output.as_os_str().to_owned(),
            "--expected-bytes".into(),
            count(expected, "SourceBytes")?.to_string().into(),
            "--expected-sha256".into(),
            text(expected, "SourceSha256")?.into(),
            "--timeout-seconds".into(),
            EXPORT_SECONDS.to_string().into(),
        ],
        &work,
        EXPORT_SECONDS,
        "export",
    )?;
    export["SourceRevision"] = expected["SourceRevision"].clone();
    save(&evidence.join("reproduced-export.json"), &export)?;
    receipt.phase("independent-complete-packing")?;
    let packing = worker(
        tool,
        &work,
        "ptq1-packing",
        &json!({"sourcePath":source, "outputPath":output, "timeoutSeconds":SCAN_SECONDS}),
        SCAN_PROCESS_SECONDS,
        "scan",
    )?;
    save(&evidence.join("independent-packing.json"), &packing)?;
    matches(
        expected,
        &export,
        &packing,
        &crate::identity::sha256(EXPECTED.as_bytes()),
    )
}

fn manifest(evidence: &Path, environment: &BTreeMap<String, String>) -> crate::Result<()> {
    let mut files = Vec::new();
    for name in [
        "expected-export.json",
        "independent-packing.json",
        "qualification.json",
        "reproduced-export.json",
    ] {
        let path = evidence.join(name);
        if !path.exists() {
            continue;
        }
        files.push(json!({"file":name, "bytes":fs::metadata(&path).map_err(|error| error.to_string())?.len(), "sha256":crate::startup_cache::digest_file(&path)?}));
    }
    save(
        &evidence.join("manifest.json"),
        &json!({"schemaVersion":1, "revision":environment["GITHUB_SHA"], "runId":environment["GITHUB_RUN_ID"], "files":files}),
    )
}

fn execute(
    data: &Path,
    evidence: &Path,
    environment: &BTreeMap<String, String>,
) -> crate::Result<Value> {
    external(data)?;
    external(evidence)?;
    if data.starts_with(evidence) || evidence.starts_with(data) {
        return Err("Packing data and evidence directories must be separate.".into());
    }
    fs::create_dir(evidence)
        .map_err(|error| format!("Use a new packing evidence directory: {error}"))?;
    let mut receipt = Receipt {
        clock: Instant::now(),
        phase_started: Duration::ZERO,
        path: evidence.join("qualification.json"),
        value: json!({
            "schemaVersion":1, "kind":"hosted-current-weight-ptq1-packing-qualification", "status":"preparing", "phase":"intake",
            "limits":limits(), "phaseStartedElapsedSeconds":0.0, "phaseDurations":[],
            "execution":{"repository":environment["GITHUB_REPOSITORY"], "revision":environment["GITHUB_SHA"], "gitRef":environment["GITHUB_REF"],
                "runId":environment["GITHUB_RUN_ID"], "runAttempt":environment["GITHUB_RUN_ATTEMPT"].parse::<u64>().map_err(|error| error.to_string())?,
                "architecture":"ARM64", "deviceModel":"GitHub-hosted Windows ARM64", "selfHosted":false},
            "workflowPath":".github/workflows/ptq1-packing-qualification.yml",
            "runUrl":format!("https://github.com/{}/actions/runs/{}/attempts/{}", environment["GITHUB_REPOSITORY"], environment["GITHUB_RUN_ID"], environment["GITHUB_RUN_ATTEMPT"]),
            "modelProcessStarted":false, "ownerDeviceExecuted":false, "defaultMayChange":false,
            "scope":"Complete weight packing only. Activation, logits, live state, adapter basis, quality, resources and performance remain separate gates."
        }),
    };
    save(&receipt.path, &receipt.value)?;
    let expected: Value = serde_json::from_str(EXPECTED).map_err(|error| error.to_string())?;
    let runtime: Value = serde_json::from_str(RUNTIME).map_err(|error| error.to_string())?;
    let result = std::env::current_exe()
        .map_err(|error| error.to_string())
        .and_then(|tool| execute_steps(data, evidence, &mut receipt, &tool, &expected, &runtime));
    receipt.value["status"] = if result.is_ok() { "matched" } else { "failed" }.into();
    if result.is_err() {
        receipt.value["failure"] = "The selected hosted packing phase failed; inspect the phase and workflow log. No promotion is inferred.".into();
        let observation = match receipt.value["phase"].as_str() {
            Some("original-pq2-download") => Some(partial(
                &data.join(format!("{}.partial", text(&expected, "SourceFilename")?)),
                count(&expected, "SourceBytes")?,
            )),
            Some("pinned-exporter-provisioning") => Some(partial(
                &data
                    .join("downloads")
                    .join(format!("{}.partial", text(&runtime["asset"], "filename")?)),
                count(&runtime["asset"], "bytes")?,
            )),
            _ => None,
        };
        if let Some(observation) = observation {
            receipt.value["partialDownload"] = observation;
        }
    }
    let final_phase = if result.is_ok() {
        "complete".into()
    } else {
        text(&receipt.value, "phase")?.to_owned()
    };
    receipt.phase(&final_phase)?;
    receipt.value["elapsedSeconds"] = receipt.clock.elapsed().as_secs_f64().into();
    save(&receipt.path, &receipt.value)?;
    manifest(evidence, environment)?;
    result?;
    Ok(receipt.value)
}

/// Execute the compiled hosted packing policy, or inspect its fixed limits.
///
/// # Errors
/// Rejects owner-device/unbound execution before artifact access, unsafe paths,
/// changed identities, failed workers and incomplete packing receipts.
pub fn run(value: Value) -> crate::Result<Value> {
    let request: Request = serde_json::from_value(value).map_err(|error| error.to_string())?;
    if matches!(request, Request::Limits) {
        return Ok(limits());
    }
    let environment = environment();
    context(cfg!(all(windows, target_arch = "aarch64")), &environment)?;
    match request {
        Request::Limits => Ok(limits()),
        Request::Execute {
            data_directory,
            evidence_directory,
        } => execute(&data_directory, &evidence_directory, &environment),
        Request::Download {
            data_directory,
            artifact,
        } => download(&data_directory, artifact),
    }
}

#[cfg(test)]
mod tests;
