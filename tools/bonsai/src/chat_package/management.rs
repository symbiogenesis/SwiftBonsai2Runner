//! Package creation and single-writer activation; no package code is executed.

use std::io::{Read as _, Write as _};

use super::{FILES, Manifest, PackageFile, Selected, Selection, Value, fs};
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Create {
    output_directory: PathBuf,
    version: String,
    source_revision: String,
    chat_path: PathBuf,
    broker_path: PathBuf,
    asr_path: PathBuf,
    tts_path: PathBuf,
    tool_path: PathBuf,
    third_party_notice_path: PathBuf,
}

fn external(path: &Path) -> crate::Result<()> {
    bonsai_artifacts::validate_external_path(path).map_err(|error| error.to_string())
}

fn create_file(path: &Path) -> crate::Result<fs::File> {
    external(path)?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        options.share_mode(0);
    }
    options.open(path).map_err(|error| error.to_string())
}

fn write_new(path: &Path, bytes: &[u8]) -> crate::Result<()> {
    let mut file = create_file(path)?;
    file.write_all(bytes).map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())
}

fn copy_new(source: &Path, destination: &Path, limit: u64) -> crate::Result<()> {
    let source = super::open(source)?;
    let length = source.metadata().map_err(|error| error.to_string())?.len();
    if length == 0 || length > limit {
        return Err("Package input exceeds its byte bounds.".into());
    }
    let mut destination = create_file(destination)?;
    let copied = std::io::copy(&mut source.take(limit + 1), &mut destination)
        .map_err(|error| error.to_string())?;
    if copied != length {
        return Err("Package input changed during copying.".into());
    }
    destination.sync_all().map_err(|error| error.to_string())
}

struct Stage(PathBuf);

impl Stage {
    fn new(parent: &Path) -> crate::Result<Self> {
        let path = parent.join(format!(
            "package-stage-{}",
            crate::startup_process::unique()
        ));
        external(&path)?;
        fs::create_dir(&path).map_err(|error| error.to_string())?;
        Ok(Self(path))
    }

    fn publish(&self, destination: &Path) -> crate::Result<()> {
        external(destination)?;
        if fs::exists(destination).map_err(|error| error.to_string())? {
            return Err("Package output already exists.".into());
        }
        fs::rename(&self.0, destination).map_err(|error| error.to_string())
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        // Only the private, newly created stage is removed; published versions remain.
        if external(&self.0).is_ok() {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}

pub(super) fn create(request: Create) -> crate::Result<Value> {
    external(&request.output_directory)?;
    if fs::exists(&request.output_directory).map_err(|error| error.to_string())? {
        return Err("Package output already exists.".into());
    }
    if !super::version(&request.version)
        || !crate::identity::is_lower_hex(&request.source_revision, 40)
    {
        return Err("Invalid package version or source revision.".into());
    }
    let parent = request
        .output_directory
        .parent()
        .ok_or("Missing output parent.")?;
    external(parent)?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let stage = Stage::new(parent)?;
    fs::create_dir(stage.0.join("bin")).map_err(|error| error.to_string())?;
    for (name, source) in [
        ("bin/bonsai-chat.exe", &request.chat_path),
        ("bin/bonsai-broker.exe", &request.broker_path),
        ("bin/bonsai-asr.exe", &request.asr_path),
        ("bin/bonsai-tts.exe", &request.tts_path),
        ("bin/bonsai-tools.exe", &request.tool_path),
    ] {
        copy_new(source, &stage.0.join(name), 512 * 1024 * 1024)?;
        crate::startup_cache::assert_arm64(&stage.0.join(name))?;
    }
    write_new(
        &stage.0.join("LICENSE"),
        include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../LICENSE")),
    )?;
    write_new(
        &stage.0.join("NOTICE"),
        include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../NOTICE")),
    )?;
    copy_new(
        &request.third_party_notice_path,
        &stage.0.join("THIRD-PARTY-NOTICES.txt"),
        16 * 1024 * 1024,
    )?;
    let files = FILES
        .iter()
        .map(|name| {
            let path = stage.0.join(name);
            Ok(PackageFile {
                name: (*name).into(),
                bytes: fs::metadata(&path)
                    .map_err(|error| error.to_string())?
                    .len(),
                sha256: crate::startup_cache::digest_file(&path)?,
            })
        })
        .collect::<crate::Result<Vec<_>>>()?;
    let manifest = Manifest {
        schema_version: 2,
        kind: "bonsai-chat-package".into(),
        version: request.version,
        source_revision: request.source_revision,
        target: "aarch64-pc-windows-msvc".into(),
        files,
    };
    let bytes = serde_json::to_vec(&manifest).map_err(|error| error.to_string())?;
    let digest = crate::identity::sha256(&bytes);
    write_new(&stage.0.join("package.json"), &bytes)?;
    super::checked(&stage.0, &digest)?;
    stage.publish(&request.output_directory)?;
    super::checked(&request.output_directory, &digest)
}

fn identity(package: &Value) -> crate::Result<Selected> {
    Ok(Selected {
        id: package["Id"]
            .as_str()
            .ok_or("Missing package identity.")?
            .into(),
        manifest_sha256: package["ManifestSha256"]
            .as_str()
            .ok_or("Missing manifest digest.")?
            .into(),
    })
}

fn install(root: &Path, source: &Path, digest: &str) -> crate::Result<Value> {
    let package = super::checked(source, digest)?;
    let selected = identity(&package)?;
    let versions = root.join("versions");
    external(&versions)?;
    fs::create_dir_all(&versions).map_err(|error| error.to_string())?;
    let destination = versions.join(&selected.id);
    external(&destination)?;
    if !fs::exists(&destination).map_err(|error| error.to_string())? {
        let stage = Stage::new(&versions)?;
        let schema = package["Manifest"]["schemaVersion"]
            .as_u64()
            .ok_or("Missing manifest schema.")?;
        for name in super::filenames(schema)?
            .iter()
            .copied()
            .chain(std::iter::once("package.json"))
        {
            let path = stage.0.join(name);
            fs::create_dir_all(path.parent().ok_or("Missing package directory.")?)
                .map_err(|error| error.to_string())?;
            let limit = if name.starts_with("bin/") {
                512 * 1024 * 1024
            } else {
                16 * 1024 * 1024
            };
            copy_new(&source.join(name), &path, limit)?;
        }
        super::checked(&stage.0, &selected.manifest_sha256)?;
        stage.publish(&destination)?;
    }
    super::checked(&destination, &selected.manifest_sha256)
}

fn write_selection(root: &Path, selection: &Selection) -> crate::Result<()> {
    let pointer = root.join("active.json");
    external(&pointer)?;
    let temporary = root.join(format!(
        "selection-{}.json",
        crate::startup_process::unique()
    ));
    let bytes = serde_json::to_vec(selection).map_err(|error| error.to_string())?;
    let result = write_new(&temporary, &bytes)
        .and_then(|()| fs::rename(&temporary, &pointer).map_err(|error| error.to_string()));
    // A failed publication preserves active.json; remove only our own temporary.
    if external(&temporary).is_ok() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub(super) fn activate(root: &Path, source: Option<(&Path, &str)>) -> crate::Result<Value> {
    external(root)?;
    fs::create_dir_all(root).map_err(|error| error.to_string())?;
    let _guard = crate::startup_cache::lock_file(&root.join("package.lock"))
        .map_err(|error| format!("Cannot own package installation: {error}"))?;
    let pointer = root.join("active.json");
    external(&pointer)?;
    let old = if fs::exists(&pointer).map_err(|error| error.to_string())? {
        Some(super::selection(root)?)
    } else {
        None
    };
    let package = if let Some((directory, digest)) = source {
        install(root, directory, digest)?
    } else {
        let previous = old
            .as_ref()
            .and_then(|item| item.previous.as_ref())
            .ok_or("No previous package is available for rollback.")?;
        let package = super::checked(
            &root.join("versions").join(&previous.id),
            &previous.manifest_sha256,
        )?;
        if package["Id"] != previous.id {
            return Err("Previous package identity disagrees with its manifest.".into());
        }
        package
    };
    let active = identity(&package)?;
    if old
        .as_ref()
        .and_then(|item| item.active.as_ref())
        .is_some_and(|item| item.id == active.id)
    {
        return Ok(package);
    }
    write_selection(
        root,
        &Selection {
            schema_version: 1,
            active: Some(active),
            previous: old.and_then(|item| item.active),
        },
    )?;
    Ok(package)
}

#[cfg(test)]
mod tests;
