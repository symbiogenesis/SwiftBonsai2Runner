//! Native portable-release assembly and independent, model-free executable smoke.

use crate::{
    Result,
    release_tool::{bytes, resolve, version},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::Write as _,
    path::{Path, PathBuf},
};

#[cfg(test)]
mod tests;

const TARGET: &str = "aarch64-pc-windows-msvc";
const FILES: [(&str, &str); 5] = [
    ("maintenance", "bonsai-tools.exe"),
    ("core", "bonsai-broker.exe"),
    ("ui", "bonsai-chat.exe"),
    ("asr", "bonsai-speech-asr.exe"),
    ("tts", "bonsai-speech-tts.exe"),
];

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "kebab-case", deny_unknown_fields)]
enum Request {
    #[serde(rename_all = "camelCase")]
    Package {
        repository_root: PathBuf,
        input_directory: PathBuf,
        output_directory: PathBuf,
        evidence_directory: PathBuf,
        tag: String,
        source_revision: String,
    },
    #[serde(rename_all = "camelCase")]
    ExecutableSmoke {
        executable_path: PathBuf,
        manifest_sha256: String,
        evidence_directory: PathBuf,
    },
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Build {
    schema_version: u64,
    source_revision: String,
    target: String,
    lane: String,
    file: String,
    bytes: u64,
    sha256: String,
}

fn require(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}
fn native() -> Result<()> {
    require(
        cfg!(all(windows, target_arch = "aarch64")),
        "Portable executable operations require native Windows ARM64.",
    )
}
fn save(path: &Path, value: &Value) -> Result<()> {
    resolve(path, false, true)?;
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    output
        .write_all(&bytes)
        .and_then(|()| output.sync_all())
        .map_err(|error| error.to_string())
}
fn command(executable: &Path, arguments: &Value, seconds: u64, build: bool) -> Result<Value> {
    let result = crate::release_tool::run(
        &json!({"operation":"invoke","filePath":executable,"arguments":arguments,"timeoutSeconds":seconds,"buildCommand":build}),
    )?;
    serde_json::from_str(result["stdout"].as_str().ok_or("Missing command output.")?)
        .map_err(|error| error.to_string())
}
fn cleanup(path: &Path) -> Result<()> {
    crate::release_tool::run(&json!({"operation":"cleanup","path":path,"parentDirectory":path.parent().ok_or("Missing cleanup parent.")?,"leafName":path.file_name().and_then(|name|name.to_str()).ok_or("Missing cleanup leaf.")?,"timeoutMilliseconds":5000})).map(|_|())
}
fn completed<T>(primary: Result<T>, cleanup: Result<()>) -> Result<T> {
    match (primary, cleanup) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(primary), Ok(())) => Err(primary),
        (Ok(_), Err(cleanup)) => Err(cleanup),
        (Err(primary), Err(cleanup)) => {
            Err(format!("{primary}; release cleanup also failed: {cleanup}"))
        }
    }
}
fn new_directory(path: &Path) -> Result<()> {
    resolve(path, true, true)?;
    require(!path.exists(), "Release output must use a new directory.")?;
    fs::create_dir_all(path).map_err(|error| error.to_string())
}
fn checked_builds(input: &Path, revision: &str) -> Result<(BTreeMap<String, PathBuf>, Vec<Build>)> {
    let mut executables = BTreeMap::new();
    let mut receipts = Vec::new();
    for (lane, file) in FILES {
        let directory = resolve(&input.join(format!("release-build-{lane}")), true, false)?;
        let receipt: Build =
            serde_json::from_slice(&bytes(&directory.join("build-manifest.json"), 2_097_152)?)
                .map_err(|error| error.to_string())?;
        let path = resolve(&directory.join(file), false, false)?;
        require(
            receipt.schema_version == 1
                && receipt.source_revision == revision
                && receipt.target == TARGET
                && receipt.lane == lane
                && receipt.file == file
                && (1..=536_870_912).contains(&receipt.bytes)
                && crate::identity::is_lower_hex(&receipt.sha256, 64)
                && fs::metadata(&path)
                    .map_err(|error| error.to_string())?
                    .len()
                    == receipt.bytes
                && crate::startup_cache::digest_file(&path)? == receipt.sha256,
            "Release input source, lane, extent or digest differs.",
        )?;
        executables.insert(lane.into(), path);
        receipts.push(receipt);
    }
    Ok((executables, receipts))
}

fn smoke(executable: &Path, manifest: &str, evidence: &Path) -> Result<Value> {
    native()?;
    require(
        crate::identity::is_lower_hex(manifest, 64),
        "Invalid package manifest digest.",
    )?;
    let executable = resolve(executable, false, false)?;
    crate::chat_package::bundle::require_windowless(&mut crate::build_tools::input_file(
        &executable,
    )?)?;
    new_directory(evidence)?;
    let installation = evidence.join("extracted");
    let result = (|| {
        let extraction = json!({"operation":"extract","executablePath":executable,"installationDirectory":installation});
        let bundle = command(
            &executable,
            &json!([
                "chat-bundle",
                "--json",
                serde_json::to_string(&extraction).map_err(|error| error.to_string())?
            ]),
            60,
            false,
        )?;
        require(
            bundle["ManifestSha256"] == manifest
                && bundle["Manifest"]["target"] == TARGET
                && bundle["Manifest"]["schemaVersion"].as_u64() == Some(2)
                && bundle["Manifest"]["files"]
                    .as_array()
                    .is_some_and(|files| files.len() == 8),
            "Single executable has unexpected package identity/inventory.",
        )?;
        let tool = resolve(
            &PathBuf::from(
                bundle["Directory"]
                    .as_str()
                    .ok_or("Missing extracted package directory.")?,
            )
            .join("bin/bonsai-tools.exe"),
            false,
            false,
        )?;
        let inspection = json!({"operation":"installed","installationDirectory":installation});
        let installed = command(
            &tool,
            &json!([
                "chat-package",
                "--json",
                serde_json::to_string(&inspection).map_err(|error| error.to_string())?
            ]),
            60,
            false,
        )?;
        require(
            installed["ManifestSha256"] == manifest && installed["Id"] == bundle["Id"],
            "Extracted inventory differs from bundled package.",
        )?;
        Ok(bundle)
    })();
    let bundle = completed(result, cleanup(&installation))?;
    let receipt = json!({"schemaVersion":1,"version":bundle["Manifest"]["version"],"sourceRevision":bundle["Manifest"]["sourceRevision"],"manifestSha256":manifest,"executableSha256":crate::startup_cache::digest_file(&executable)?,"guiSubsystemVerified":true,"inventoryVerified":true,"extractionVerified":true,"maintenanceExecutableStarted":true,"extractedFilesRemoved":true,"modelStarted":false,"softwareInstalled":false,"actualModelAcceptance":"pending","cleanMachineAcceptance":"pending"});
    save(&evidence.join("smoke-receipt.json"), &receipt)?;
    Ok(receipt)
}

fn package(
    repository: &Path,
    input: &Path,
    output: &Path,
    evidence: &Path,
    tag: &str,
    revision: &str,
) -> Result<Value> {
    native()?;
    require(
        std::env::var("RUNNER_ENVIRONMENT").as_deref() == Ok("github-hosted"),
        "Release packaging requires disposable hosted Windows ARM64.",
    )?;
    let version = version(tag, revision)?;
    resolve(repository, true, false)?;
    resolve(input, true, false)?;
    let (executables, builds) = checked_builds(input, revision)?;
    let notices = resolve(
        &input.join("release-build-tts/THIRD-PARTY-NOTICES.txt"),
        false,
        false,
    )?;
    require(
        fs::metadata(&notices)
            .map_err(|error| error.to_string())?
            .len()
            >= 1024,
        "Complete third-party notice bundle missing.",
    )?;
    new_directory(output)?;
    new_directory(evidence)?;
    let stage = std::env::temp_dir().join(format!(
        "SwiftBonsai-ReleasePackage-{}",
        crate::startup_process::unique()
    ));
    new_directory(&stage)?;
    let result = (|| {
        let package = stage.join("package");
        let creation = crate::chat_package::run(
            json!({"operation":"create","outputDirectory":package,"version":version,"sourceRevision":revision,"chatPath":executables["ui"],"brokerPath":executables["core"],"asrPath":executables["asr"],"ttsPath":executables["tts"],"toolPath":executables["maintenance"],"thirdPartyNoticePath":notices}),
        )?;
        let manifest = crate::startup_cache::digest_file(&package.join("package.json"))?;
        require(
            creation["ManifestSha256"] == manifest,
            "Package creation receipt differs from manifest.",
        )?;
        let bundle = output.join(format!("SwiftBonsai2Runner-{version}-windows-arm64.exe"));
        crate::chat_package::bundle::run(
            json!({"operation":"create","packageDirectory":package,"manifestSha256":manifest,"outputPath":bundle}),
        )?;
        smoke(&bundle, &manifest, &evidence.join("executable-smoke"))?;
        let asset = json!({"name":bundle.file_name().and_then(|name|name.to_str()).ok_or("Missing bundle name.")?,"bytes":fs::metadata(&bundle).map_err(|error|error.to_string())?.len(),"sha256":crate::startup_cache::digest_file(&bundle)?});
        let receipt = json!({"schemaVersion":1,"distribution":"portable-executable","version":version,"tag":tag,"sourceRevision":revision,"target":TARGET,"toolchain":"nightly-2026-11-12-aarch64-pc-windows-msvc","manifestSha256":manifest,"signed":false,"runUrl":format!("https://github.com/{}/actions/runs/{}",std::env::var("GITHUB_REPOSITORY").unwrap_or_default(),std::env::var("GITHUB_RUN_ID").unwrap_or_default()),"executableSmoke":"passed-inspect-extract-verify-without-model","builds":builds,"assets":[asset]});
        save(&output.join("release-manifest.json"), &receipt)?;
        let mut checksums = String::new();
        for name in [
            bundle
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or("Missing bundle name.")?,
            "release-manifest.json",
        ] {
            use std::fmt::Write as _;
            writeln!(
                checksums,
                "{}  {name}",
                crate::startup_cache::digest_file(&output.join(name))?
            )
            .map_err(|error| error.to_string())?;
        }
        fs::write(output.join("SHA256SUMS.txt"), checksums).map_err(|error| error.to_string())?;
        crate::release_tool::run(
            &json!({"operation":"assets","assetDirectory":output,"tag":tag,"sourceRevision":revision}),
        )
    })();
    completed(result, cleanup(&stage))
}

/// Create a reviewed portable release or independently inspect/extract its image.
///
/// # Errors
/// Rejects foreign sources, incomplete lane receipts, changed bytes, malformed
/// GUI images, failed owned commands and uncertain private-directory cleanup.
pub fn run(value: Value) -> Result<Value> {
    let request: Request = serde_json::from_value(value).map_err(|error| error.to_string())?;
    match request {
        Request::Package {
            repository_root,
            input_directory,
            output_directory,
            evidence_directory,
            tag,
            source_revision,
        } => package(
            &repository_root,
            &input_directory,
            &output_directory,
            &evidence_directory,
            &tag,
            &source_revision,
        ),
        Request::ExecutableSmoke {
            executable_path,
            manifest_sha256,
            evidence_directory,
        } => smoke(&executable_path, &manifest_sha256, &evidence_directory),
    }
}
