//! Immutable file-level native retirement scope; no source or compiler executes.

use std::{collections::BTreeSet, ffi::OsString, fs, path::Path};

use serde::Deserialize;
use sha2::{Digest as _, Sha256};

use super::native_source_inventory::{digest, lower_hex, plain_ancestors, read_bounded};
use crate::Result;

const USAGE: &str =
    "Usage: bonsai-ci native-boundary-verify --root PATH --manifest PATH --expected-sha256 SHA256";
const MAX_DOCUMENT: u64 = 2 * 1024 * 1024;
const MAX_FILE: u64 = 32 * 1024 * 1024;
const MAX_TOTAL: u64 = 128 * 1024 * 1024;
const MAX_ENTRIES: usize = 100_000;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Family {
    id: String,
    obligations: Vec<String>,
    retirement_qualified: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Entry {
    path: String,
    bytes: u64,
    sha256: String,
    family: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Exclusion {
    path: String,
    reason: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    schema_version: u32,
    kind: String,
    repository: String,
    source_revision: String,
    exclusions: Vec<Exclusion>,
    families: Vec<Family>,
    files: Vec<Entry>,
    edges: Vec<Entry>,
}

fn ordinary_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 1024
        && value.is_ascii()
        && !value.bytes().any(|byte| byte.is_ascii_control())
        && !value.contains(['\\', ':', '<', '>', '"', '|', '?', '*'])
        && value.split('/').all(|part| {
            let stem = part
                .split('.')
                .next()
                .unwrap_or_default()
                .to_ascii_uppercase();
            !part.is_empty()
                && part != "."
                && part != ".."
                && !part.ends_with(['.', ' '])
                && !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                && !((stem.starts_with("COM") || stem.starts_with("LPT"))
                    && stem.len() == 4
                    && matches!(stem.as_bytes()[3], b'1'..=b'9'))
        })
}

fn native_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            ["c", "cc", "cpp", "cxx", "h", "hh", "hpp", "hxx", "m", "mm"]
                .contains(&extension.to_ascii_lowercase().as_str())
        })
}

fn excluded(path: &str, exclusions: &[Exclusion]) -> bool {
    exclusions
        .iter()
        .any(|entry| path == entry.path || path.starts_with(&format!("{}/", entry.path)))
}

fn discover(root: &Path, exclusions: &[Exclusion]) -> Result<BTreeSet<String>> {
    let mut pending = vec![root.to_path_buf()];
    let mut paths = BTreeSet::new();
    let mut visited = 0_usize;
    while let Some(directory) = pending.pop() {
        for item in fs::read_dir(directory).map_err(|error| error.to_string())? {
            let item = item.map_err(|error| error.to_string())?;
            visited = visited
                .checked_add(1)
                .filter(|count| *count <= MAX_ENTRIES)
                .ok_or("Native boundary scan exceeded its entry bound.")?;
            let path = item.path();
            let relative = path
                .strip_prefix(root)
                .map_err(|error| error.to_string())?
                .to_str()
                .ok_or("Native boundary path is not UTF-8.")?
                .replace('\\', "/");
            if excluded(&relative, exclusions) {
                continue;
            }
            if !ordinary_path(&relative) {
                return Err("Native boundary scan found a nonportable path.".into());
            }
            plain_ancestors(&path)?;
            let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
            if metadata.is_dir() {
                if relative.split('/').count() > 64 {
                    return Err("Native boundary scan exceeded its depth bound.".into());
                }
                pending.push(path);
            } else if !metadata.is_file() {
                return Err("Native boundary scan requires ordinary files and directories.".into());
            } else if native_path(&path) && !paths.insert(relative) {
                return Err("Native boundary scan found a duplicate path.".into());
            }
        }
    }
    Ok(paths)
}

fn validate_manifest(manifest: &Manifest) -> Result<BTreeSet<&str>> {
    if manifest.schema_version != 1
        || manifest.kind != "checked-in-native-retirement-boundary"
        || !matches!(manifest.repository.as_str(), "runner" | "prism")
        || !lower_hex(&manifest.source_revision, 40)
        || manifest.files.is_empty()
        || manifest.files.len() > 4096
        || manifest.edges.is_empty()
        || manifest.edges.len() > 4096
        || manifest.families.is_empty()
        || manifest.families.len() > 64
    {
        return Err("Invalid native retirement boundary manifest.".into());
    }
    let expected_exclusions: &[&str] = if manifest.repository == "runner" {
        &[".git", ".worktrees", "target", "third_party"]
    } else {
        &[".git", ".worktrees", "target"]
    };
    if manifest
        .exclusions
        .iter()
        .map(|entry| entry.path.as_str())
        .collect::<Vec<_>>()
        != expected_exclusions
        || manifest
            .exclusions
            .iter()
            .any(|entry| entry.reason.trim().is_empty())
    {
        return Err("Native boundary exclusions must explicitly retain the reviewed data/vendor/build boundary.".into());
    }
    let mut families = BTreeSet::new();
    for family in &manifest.families {
        if !ordinary_path(&family.id)
            || !families.insert(family.id.as_str())
            || family.retirement_qualified
            || family.obligations.is_empty()
            || family.obligations.len() > 64
            || family
                .obligations
                .iter()
                .any(|value| value.trim().is_empty() || value.len() > 2048)
        {
            return Err(
                "Native inventory cannot grant retirement or omit replacement obligations.".into(),
            );
        }
    }
    Ok(families)
}

fn verify(root: &Path, manifest: &Manifest) -> Result<serde_json::Value> {
    if !root.is_absolute() || !root.is_dir() {
        return Err("Native boundary requires an absolute repository directory.".into());
    }
    plain_ancestors(root)?;
    let families = validate_manifest(manifest)?;
    let mut all_paths = BTreeSet::new();
    let mut native_paths = BTreeSet::new();
    let mut represented = BTreeSet::new();
    let mut total = 0_u64;
    for (entries, source) in [(&manifest.files, true), (&manifest.edges, false)] {
        let mut previous = "";
        for entry in entries {
            if !ordinary_path(&entry.path)
                || entry.path.as_str() <= previous
                || !all_paths.insert(entry.path.to_ascii_lowercase())
                || excluded(&entry.path, &manifest.exclusions)
                || !families.contains(entry.family.as_str())
                || !lower_hex(&entry.sha256, 64)
                || entry.bytes > MAX_FILE
                || (source != native_path(Path::new(&entry.path)))
            {
                return Err("Native boundary entries must be sorted, unique, bounded and assigned to a reviewed family.".into());
            }
            previous = &entry.path;
            let path = root.join(&entry.path);
            plain_ancestors(&path)?;
            total = total
                .checked_add(entry.bytes)
                .filter(|bytes| *bytes <= MAX_TOTAL)
                .ok_or("Native boundary exceeded its aggregate byte bound.")?;
            if digest(&path, MAX_FILE)? != (entry.bytes, entry.sha256.clone()) {
                return Err(format!("Native boundary identity changed: {}", entry.path));
            }
            represented.insert(entry.family.as_str());
            if source {
                native_paths.insert(entry.path.clone());
            }
        }
    }
    if families != represented {
        return Err("Native boundary family has no retained source or operative edge.".into());
    }
    if native_paths != discover(root, &manifest.exclusions)? {
        return Err("Native boundary source set changed: added, omitted or deleted native files require reviewed inventory changes; deletion does not establish retirement.".into());
    }
    Ok(serde_json::json!({
        "schemaVersion":1, "kind":"checked-in-native-retirement-boundary-verification",
        "repository":manifest.repository, "sourceRevision":manifest.source_revision,
        "nativeFiles":manifest.files.len(), "operativeAndContractEdges":manifest.edges.len(),
        "families":manifest.families.len(), "verifiedBytes":total,
        "sourceExecuted":false, "gitRevisionVerified":false,
        "externalSourceClosureQualified":false, "compiledClosureQualified":false,
        "independentParityQualified":false, "nativeRetirementQualified":false
    }))
}

pub(super) fn run(arguments: &[OsString]) -> Result<String> {
    if arguments.len() != 7
        || arguments[1] != "--root"
        || arguments[3] != "--manifest"
        || arguments[5] != "--expected-sha256"
    {
        return Err(USAGE.into());
    }
    let root = Path::new(&arguments[2]);
    let path = Path::new(&arguments[4]);
    let expected = arguments[6]
        .to_str()
        .filter(|value| lower_hex(value, 64))
        .ok_or(USAGE)?;
    if !path.is_absolute() {
        return Err("Native boundary requires an absolute manifest path.".into());
    }
    plain_ancestors(path)?;
    let bytes = read_bounded(path, MAX_DOCUMENT)?;
    if super::native_source_inventory::hex(&Sha256::digest(&bytes)) != expected {
        return Err(
            "Native boundary manifest differs from its independently reviewed digest.".into(),
        );
    }
    let manifest = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    serde_json::to_string_pretty(&verify(root, &manifest)?).map_err(|error| error.to_string())
}

#[cfg(test)]
#[path = "native_boundary_tests.rs"]
mod tests;
