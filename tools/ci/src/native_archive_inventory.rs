//! File-by-file immutable source ZIP inventories; inspected bytes never execute.

use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    fs::File,
    io::{Read as _, Seek},
    path::{Component, Path},
};

use serde::Deserialize;
use sha2::{Digest as _, Sha256};
use zip::{CompressionMethod, ZipArchive};

use super::native_source_inventory::{hex, lower_hex, plain_ancestors, read_bounded};
use crate::Result;

const USAGE: &str = "Usage: bonsai-ci native-archive-inventory --archive PATH --pin PATH --expected-pin-sha256 SHA256";
const MAX_ARCHIVE: u64 = 256 * 1024 * 1024;
const MAX_FILE: u64 = 64 * 1024 * 1024;
const MAX_TOTAL: u64 = 1024 * 1024 * 1024;
const MAX_ENTRIES: usize = 32_768;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Pin {
    schema_version: u32,
    repository: String,
    revision: String,
    archive_root: String,
    archive_bytes: u64,
    archive_sha256: String,
}

// Reject platform-dependent names even though nothing is extracted. This makes
// an inventory usable by a future Windows replay consumer without aliases.
fn portable_path(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 1024
        && name.is_ascii()
        && !name.bytes().any(|byte| byte.is_ascii_control())
        && !name.contains(['\\', ':', '<', '>', '"', '|', '?', '*'])
        && name.split('/').all(|part| {
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
        && Path::new(name)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}

fn validate_pin(pin: &Pin) -> Result<()> {
    if pin.schema_version != 1
        || !pin.repository.starts_with("https://")
        || pin.repository.len() > 512
        || !pin.repository.is_ascii()
        || pin
            .repository
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
        || !lower_hex(&pin.revision, 40)
        || !portable_path(&pin.archive_root)
        || pin.archive_root.contains('/')
        || !(1..=MAX_ARCHIVE).contains(&pin.archive_bytes)
        || !lower_hex(&pin.archive_sha256, 64)
    {
        return Err("Unsupported native archive pin identity or bounds.".into());
    }
    Ok(())
}

fn inventory<R: std::io::Read + Seek>(reader: R, pin: &Pin) -> Result<serde_json::Value> {
    validate_pin(pin)?;
    let mut archive = ZipArchive::new(reader).map_err(|error| error.to_string())?;
    if archive.is_empty() || archive.len() > MAX_ENTRIES || archive.offset() != 0 {
        return Err("Native source archive has invalid entry count or a prepended payload.".into());
    }
    let prefix = format!("{}/", pin.archive_root);
    let mut entries = BTreeMap::new();
    let mut directories = BTreeSet::new();
    let mut folded = BTreeSet::new();
    let mut total = 0_u64;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| error.to_string())?;
        let raw = entry.name().map_err(|error| error.to_string())?;
        let name = raw.strip_suffix('/').unwrap_or(&raw);
        if !portable_path(name)
            || !folded.insert(name.to_ascii_lowercase())
            || entry.is_symlink()
            || entry.encrypted()
            || !matches!(
                entry.compression(),
                CompressionMethod::Stored | CompressionMethod::Deflated
            )
            || entry.unix_mode().is_some_and(|mode| {
                let kind = mode & 0o170_000;
                kind != 0 && kind != if entry.is_dir() { 0o040_000 } else { 0o100_000 }
            })
        {
            return Err(
                "Native source archive has an unsafe, aliased or unsupported entry.".into(),
            );
        }
        if entry.is_dir() {
            if (name != pin.archive_root && !name.starts_with(&prefix)) || entry.size() != 0 {
                return Err("Native source archive has an unexpected directory.".into());
            }
            directories.insert(name.to_ascii_lowercase());
            continue;
        }
        let relative = name
            .strip_prefix(&prefix)
            .ok_or("Native source archive entry is outside its pinned root.")?
            .to_owned();
        if entry.size() > MAX_FILE {
            return Err("Native source archive file exceeds its byte bound.".into());
        }
        total = total
            .checked_add(entry.size())
            .filter(|bytes| *bytes <= MAX_TOTAL)
            .ok_or("Native source archive exceeds its aggregate expansion bound.")?;
        let mut hash = Sha256::new();
        let mut bytes = 0_u64;
        let mut buffer = [0_u8; 16 * 1024];
        loop {
            let count = entry.read(&mut buffer).map_err(|error| error.to_string())?;
            if count == 0 {
                break;
            }
            bytes = bytes
                .checked_add(u64::try_from(count).map_err(|error| error.to_string())?)
                .filter(|bytes| *bytes <= MAX_FILE && *bytes <= entry.size())
                .ok_or("Native source archive expansion exceeded its declared length.")?;
            hash.update(&buffer[..count]);
        }
        if bytes != entry.size() {
            return Err("Native source archive expanded length differs.".into());
        }
        entries.insert(
            relative,
            serde_json::json!({"bytes": bytes, "sha256": hex(&hash.finalize())}),
        );
    }
    if entries.is_empty() {
        return Err("Native source archive contains no source files.".into());
    }
    let files: BTreeSet<_> = entries
        .keys()
        .map(|name| format!("{prefix}{name}").to_ascii_lowercase())
        .collect();
    // Include implicit directories when detecting file/directory collisions.
    for name in files.iter().chain(&directories) {
        let mut ancestor = name.as_str();
        while let Some((parent, _)) = ancestor.rsplit_once('/') {
            if files.contains(parent) {
                return Err("Native source archive has a file/directory collision.".into());
            }
            ancestor = parent;
        }
    }
    Ok(serde_json::json!({
        "schemaVersion": 1,
        "kind": "immutable-native-source-archive-inventory",
        "scope": "complete-source-archive-files-not-compiled-translation-units",
        "repository": pin.repository, "revision": pin.revision,
        "archiveRoot": pin.archive_root, "archiveBytes": pin.archive_bytes,
        "archiveSha256": pin.archive_sha256, "fileCount": entries.len(), "sourceBytes": total,
        "files": entries.into_iter().map(|(path, identity)| serde_json::json!({"path":path,"bytes":identity["bytes"],"sha256":identity["sha256"]})).collect::<Vec<_>>(),
        "sourceExecuted": false, "compiledTranslationUnitClosureQualified": false,
        "inferenceParityQualified": false, "nativeRetirementQualified": false
    }))
}

pub(super) fn run(arguments: &[OsString]) -> Result<String> {
    if arguments.len() != 7
        || arguments[1] != "--archive"
        || arguments[3] != "--pin"
        || arguments[5] != "--expected-pin-sha256"
    {
        return Err(USAGE.into());
    }
    let archive = Path::new(&arguments[2]);
    let pin_path = Path::new(&arguments[4]);
    let expected = arguments[6]
        .to_str()
        .filter(|value| lower_hex(value, 64))
        .ok_or(USAGE)?;
    for path in [archive, pin_path] {
        if !path.is_absolute() {
            return Err(USAGE.into());
        }
        plain_ancestors(path)?;
    }
    let pin_bytes = read_bounded(pin_path, 4096)?;
    if hex(&Sha256::digest(&pin_bytes)) != expected {
        return Err("Native source archive pin differs from its reviewed SHA-256.".into());
    }
    let pin: Pin = serde_json::from_slice(&pin_bytes).map_err(|error| error.to_string())?;
    validate_pin(&pin)?;
    let mut options = File::options();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        // Retain the verified archive while denying writers and rename/delete.
        options.share_mode(1);
    }
    let mut file = options.open(archive).map_err(|error| error.to_string())?;
    if !file
        .metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.len() == pin.archive_bytes)
    {
        return Err("Native source archive length differs from its pin.".into());
    }
    // Hash and parse the same open file, so replacing a pathname cannot substitute
    // an unverified archive between intake and ZIP parsing.
    let mut hasher = Sha256::new();
    let mut count = 0_u64;
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        count = count
            .checked_add(u64::try_from(read).map_err(|error| error.to_string())?)
            .filter(|bytes| *bytes <= MAX_ARCHIVE)
            .ok_or("Native source archive exceeded its intake bound.")?;
        hasher.update(&buffer[..read]);
    }
    if count != pin.archive_bytes || hex(&hasher.finalize()) != pin.archive_sha256 {
        return Err("Native source archive differs from its pinned bytes.".into());
    }
    file.rewind().map_err(|error| error.to_string())?;
    let mut result = inventory(file, &pin)?;
    result["pinSha256"] = expected.into();
    serde_json::to_string(&result).map_err(|error| error.to_string())
}

#[cfg(test)]
#[path = "native_archive_inventory_tests.rs"]
mod tests;
