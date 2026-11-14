//! Exact portable asset inventory and source-bound build receipts.
use super::{Result, bytes, path, read_json, resolve, text, version};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
};

pub(super) fn validate(request: &Value) -> Result<Value> {
    let tag = text(request, "tag")?;
    let revision = text(request, "sourceRevision")?;
    let version = version(tag, revision)?;
    let root = resolve(&path(request, "assetDirectory")?, true, false)?;
    let executable = format!("SwiftBonsai2Runner-{version}-windows-arm64.exe");
    let names = BTreeSet::from([
        executable.as_str(),
        "release-manifest.json",
        "SHA256SUMS.txt",
    ]);
    let mut seen = BTreeSet::new();
    for entry in fs::read_dir(&root).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "Non-Unicode release asset.")?;
        resolve(&entry.path(), false, false)?;
        if !names.contains(name.as_str()) || !seen.insert(name) {
            return Err("Unexpected release asset inventory.".into());
        }
    }
    if seen.len() != names.len() {
        return Err("Release asset inventory differs from portable inventory.".into());
    }
    let raw = bytes(&root.join("SHA256SUMS.txt"), 8192)?;
    let checksum_text = std::str::from_utf8(&raw).map_err(|error| error.to_string())?;
    let mut checksums = BTreeMap::new();
    for line in checksum_text.lines() {
        let (digest, name) = line
            .split_once("  ")
            .ok_or("Invalid release checksum record.")?;
        if !crate::identity::is_lower_hex(digest, 64)
            || name == "SHA256SUMS.txt"
            || !names.contains(name)
            || checksums.insert(name, digest).is_some()
        {
            return Err("Unexpected or duplicate checksum record.".into());
        }
        if crate::startup_cache::digest_file(&root.join(name))? != digest {
            return Err(format!("Release checksum differs: {name}"));
        }
    }
    if checksums.len() != 2 {
        return Err("Release checksums incomplete.".into());
    }
    let manifest = read_json(&root.join("release-manifest.json"))?;
    if manifest["schemaVersion"].as_u64() != Some(1)
        || manifest["distribution"] != "portable-executable"
        || manifest["tag"] != tag
        || manifest["version"] != version
        || manifest["sourceRevision"] != revision
        || manifest["target"] != "aarch64-pc-windows-msvc"
        || manifest["executableSmoke"] != "passed-inspect-extract-verify-without-model"
        || manifest["signed"].as_bool() != Some(false)
    {
        return Err("Portable release manifest source, architecture or checks differ.".into());
    }
    let assets = manifest["assets"]
        .as_array()
        .filter(|assets| assets.len() == 1)
        .ok_or("Portable release requires exactly one executable.")?;
    let asset = &assets[0];
    let extent = fs::metadata(root.join(&executable))
        .map_err(|error| error.to_string())?
        .len();
    if extent == 0
        || asset["name"] != executable
        || asset["bytes"].as_u64() != Some(extent)
        || asset["sha256"] != checksums[executable.as_str()]
    {
        return Err("Portable executable identity disagrees with checksum record.".into());
    }
    validate_builds(&manifest, revision)?;
    Ok(manifest)
}

fn validate_builds(manifest: &Value, revision: &str) -> Result<()> {
    let expected = BTreeMap::from([
        ("maintenance", "bonsai-tools.exe"),
        ("core", "bonsai-broker.exe"),
        ("ui", "bonsai-chat.exe"),
        ("asr", "bonsai-speech-asr.exe"),
        ("tts", "bonsai-speech-tts.exe"),
    ]);
    let builds = manifest["builds"]
        .as_array()
        .filter(|builds| builds.len() == 5)
        .ok_or("Release build inventory incomplete.")?;
    let mut lanes = BTreeSet::new();
    if !crate::identity::is_lower_hex(text(manifest, "manifestSha256")?, 64) {
        return Err("Invalid package manifest identity.".into());
    }
    for build in builds {
        let lane = text(build, "lane")?;
        if expected.get(lane).is_none_or(|file| build["file"] != *file)
            || !lanes.insert(lane)
            || build["schemaVersion"].as_u64() != Some(1)
            || build["sourceRevision"] != revision
            || build["target"] != "aarch64-pc-windows-msvc"
            || build["bytes"].as_u64().is_none_or(|size| size == 0)
            || !crate::identity::is_lower_hex(text(build, "sha256")?, 64)
        {
            return Err("Unexpected build lane, source or executable identity.".into());
        }
    }
    Ok(())
}
