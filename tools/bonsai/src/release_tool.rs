//! Contributor release policy shared by the small PowerShell entry adapters.

use crate::Result;
use serde_json::{Value, json};
use std::{
    fs,
    path::{Component, Path, PathBuf},
};

mod assets;
mod notices;
mod process;
mod publication;

fn text<'a>(value: &'a Value, field: &str) -> Result<&'a str> {
    value[field]
        .as_str()
        .ok_or_else(|| format!("Missing release string: {field}"))
}

fn path(value: &Value, field: &str) -> Result<PathBuf> {
    Ok(PathBuf::from(text(value, field)?))
}

pub(crate) fn resolve(path: &Path, directory: bool, missing: bool) -> Result<PathBuf> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return Err("Release paths must be expanded and absolute.".into());
    }
    for (index, ancestor) in path.ancestors().enumerate() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) => {
                if bonsai_artifacts::is_redirect(&metadata) {
                    return Err(format!("Redirected release path: {}", ancestor.display()));
                }
                if index == 0
                    && (metadata.is_dir() != directory || (!directory && !metadata.is_file()))
                {
                    return Err("Unexpected release path kind.".into());
                }
            }
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound && (missing || index > 0) => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(path.components().collect())
}

pub(crate) fn bytes(path: &Path, maximum: u64) -> Result<Vec<u8>> {
    resolve(path, false, false)?;
    let file = crate::build_tools::input_file(path)?;
    crate::bounded_input::read(file, maximum)
        .map_err(|error| error.into_message("Release input exceeds byte bound."))
}

pub(crate) fn read_json(path: &Path) -> Result<Value> {
    serde_json::from_slice(&bytes(path, 2 * 1024 * 1024)?).map_err(|error| error.to_string())
}

pub(crate) fn version(tag: &str, revision: &str) -> Result<String> {
    let version = tag.strip_prefix('v').ok_or("Invalid release tag.")?;
    let parts: Vec<_> = version.split('.').collect();
    if parts.len() != 3
        || parts.iter().any(|part| {
            part.is_empty()
                || !part.bytes().all(|b| b.is_ascii_digit())
                || (part.len() > 1 && part.starts_with('0'))
        })
        || !crate::identity::is_lower_hex(revision, 40)
    {
        return Err("Invalid release identity.".into());
    }
    Ok(version.into())
}

/// Execute one offline release check or explicitly requested contributor command.
///
/// # Errors
/// Rejects malformed identities, redirected paths, changed assets, incomplete
/// publication metadata, failed commands and uncertain native cleanup.
pub fn run(value: &Value) -> Result<Value> {
    match text(value, "operation")? {
        "path" => Ok(json!(resolve(
            &path(value, "path")?,
            value["directory"].as_bool().ok_or("Missing path kind.")?,
            value["allowMissing"].as_bool().unwrap_or(false)
        )?)),
        "cleanup" => process::cleanup(value),
        "invoke" => process::invoke(value),
        "assets" => assets::validate(value),
        "remote-release" => {
            publication::validate(value)?;
            Ok(Value::Null)
        }
        "publish" => publication::publish(value),
        "catalog" => publication::catalog(value),
        "tag" => publication::tag(value),
        "tag-step" => publication::tag_step(value),
        "response" => publication::checked_response(value),
        "notice-provenance" => notices::provenance(value),
        "notice-tree" => notices::tree(value),
        "notice-blob" => notices::blob(value),
        "notice-sources" => notices::sources(value),
        _ => Err("Unknown release-tool operation.".into()),
    }
}
