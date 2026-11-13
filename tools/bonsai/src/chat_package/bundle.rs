//! A native maintenance image followed by one fixed, digest-checked package.
//!
//! The payload has no caller-controlled paths or compression. It contains a
//! length-prefixed package manifest followed by FILES in the contract's order.
//! Its footer binds the payload offset, length and manifest digest. The original
//! maintenance image must equal the embedded maintenance executable byte for byte.

use std::{
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{Read as _, Seek as _, SeekFrom, Write as _},
    path::{Path, PathBuf},
};

use serde::Deserialize;
use serde_json::{Value, json};

use super::{FILES, Manifest, PackageFile};
use crate::identity::Hasher;

const MAGIC: &[u8; 16] = b"BONSAI-BUNDLE-V1";
const FOOTER_BYTES: u64 = 96;
const MAX_IMAGE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_PAYLOAD_BYTES: u64 = 5 * MAX_IMAGE_BYTES + 49 * 1024 * 1024;

/// Exact compatibility marker retained by the maintenance version receipt.
pub const LAUNCHER_PROTOCOL: &str = "Bonsai executable bundle launcher protocol 1";

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "kebab-case", deny_unknown_fields)]
enum Request {
    #[serde(rename_all = "camelCase")]
    Create {
        package_directory: PathBuf,
        manifest_sha256: String,
        output_path: PathBuf,
    },
    #[serde(rename_all = "camelCase")]
    Inspect { executable_path: PathBuf },
    #[serde(rename_all = "camelCase")]
    Extract {
        executable_path: PathBuf,
        installation_directory: PathBuf,
    },
}

struct Layout {
    offset: u64,
    manifest_bytes: Vec<u8>,
    manifest: Manifest,
    digest: String,
}

impl Layout {
    fn receipt(&self, executable: &Path) -> Value {
        json!({
            "ExecutablePath": executable, "ManifestSha256": self.digest,
            "Manifest": self.manifest,
            "Id": format!("{}-{}-{}", self.manifest.version, self.manifest.source_revision, self.digest),
        })
    }

    fn entry(&self, name: &str) -> crate::Result<&PackageFile> {
        self.manifest
            .files
            .iter()
            .find(|entry| entry.name == name)
            .ok_or_else(|| "Missing bundled package file.".into())
    }
}

fn external(path: &Path) -> crate::Result<()> {
    bonsai_artifacts::validate_external_path(path).map_err(|error| error.to_string())
}

fn read_u64(file: &mut File) -> crate::Result<u64> {
    let mut bytes = [0; 8];
    file.read_exact(&mut bytes)
        .map_err(|error| error.to_string())?;
    Ok(u64::from_le_bytes(bytes))
}

fn footer(file: &mut File) -> crate::Result<Option<(u64, u64, String)>> {
    let length = file.metadata().map_err(|error| error.to_string())?.len();
    if length < FOOTER_BYTES {
        return Ok(None);
    }
    file.seek(SeekFrom::End(-16))
        .map_err(|error| error.to_string())?;
    let mut magic = [0; 16];
    file.read_exact(&mut magic)
        .map_err(|error| error.to_string())?;
    if magic != *MAGIC {
        return Ok(None);
    }
    file.seek(SeekFrom::Start(length - FOOTER_BYTES))
        .map_err(|error| error.to_string())?;
    let offset = read_u64(file)?;
    let payload_bytes = read_u64(file)?;
    let mut digest = [0; 64];
    file.read_exact(&mut digest)
        .map_err(|error| error.to_string())?;
    let digest = String::from_utf8(digest.to_vec()).map_err(|error| error.to_string())?;
    if !(64..=MAX_IMAGE_BYTES).contains(&offset)
        || payload_bytes > MAX_PAYLOAD_BYTES
        || offset.checked_add(payload_bytes) != Some(length - FOOTER_BYTES)
        || !crate::identity::is_lower_hex(&digest, 64)
    {
        return Err("Invalid executable bundle footer.".into());
    }
    Ok(Some((offset, payload_bytes, digest)))
}

fn layout(file: &mut File) -> crate::Result<Layout> {
    let (offset, payload_bytes, digest) =
        footer(file)?.ok_or("Executable has no bundled application.")?;
    file.seek(SeekFrom::Start(offset))
        .map_err(|error| error.to_string())?;
    let metadata_bytes = read_u64(file)?;
    if metadata_bytes == 0 || metadata_bytes > super::MAX_METADATA_BYTES {
        return Err("Bundled package manifest exceeds its byte limit.".into());
    }
    let mut bytes = vec![0; usize::try_from(metadata_bytes).map_err(|error| error.to_string())?];
    file.read_exact(&mut bytes)
        .map_err(|error| error.to_string())?;
    if crate::identity::sha256(&bytes) != digest {
        return Err("Bundled package manifest SHA-256 mismatch.".into());
    }
    let manifest: Manifest =
        serde_json::from_slice(super::json_bytes(&bytes)).map_err(|error| error.to_string())?;
    if manifest.schema_version != 2
        || manifest.kind != "bonsai-chat-package"
        || manifest.target != "aarch64-pc-windows-msvc"
        || !super::version(&manifest.version)
        || !crate::identity::is_lower_hex(&manifest.source_revision, 40)
        || manifest.files.len() != FILES.len()
    {
        return Err("Unsupported bundled package identity or inventory.".into());
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut expected = metadata_bytes + 8;
    for entry in &manifest.files {
        let limit = if entry.name.starts_with("bin/") {
            MAX_IMAGE_BYTES
        } else {
            16 * 1024 * 1024
        };
        if !FILES.contains(&entry.name.as_str())
            || !seen.insert(&entry.name)
            || entry.bytes == 0
            || entry.bytes > limit
            || !crate::identity::is_lower_hex(&entry.sha256, 64)
        {
            return Err("Invalid bundled package file identity.".into());
        }
        expected = expected
            .checked_add(entry.bytes)
            .ok_or("Bundle length overflow.")?;
    }
    if expected != payload_bytes {
        return Err("Bundled package length differs from its inventory.".into());
    }
    Ok(Layout {
        offset,
        manifest_bytes: bytes,
        manifest,
        digest,
    })
}

fn transfer(
    file: &mut File,
    length: u64,
    mut output: impl std::io::Write,
) -> crate::Result<String> {
    let mut remaining = length;
    let mut digest = Hasher::new()?;
    let mut buffer = vec![0; 1024 * 1024];
    while remaining > 0 {
        crate::startup_cancel::checkpoint()?;
        let count =
            usize::try_from(remaining.min(1024 * 1024)).map_err(|error| error.to_string())?;
        file.read_exact(&mut buffer[..count])
            .map_err(|error| error.to_string())?;
        digest.update(&buffer[..count])?;
        output
            .write_all(&buffer[..count])
            .map_err(|error| error.to_string())?;
        remaining -= u64::try_from(count).map_err(|error| error.to_string())?;
    }
    digest.finalize_hex()
}

fn verify_image(file: &mut File, layout: &Layout) -> crate::Result<()> {
    let entry = layout.entry("bin/bonsai-tools.exe")?;
    file.rewind().map_err(|error| error.to_string())?;
    if entry.bytes != layout.offset
        || transfer(file, layout.offset, std::io::sink())? != entry.sha256
    {
        return Err("Bundle launcher differs from its embedded maintenance executable.".into());
    }
    file.seek(SeekFrom::Start(
        layout.offset
            + 8
            + u64::try_from(layout.manifest_bytes.len()).map_err(|error| error.to_string())?,
    ))
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn supports_bundle(file: &mut File) -> crate::Result<bool> {
    file.rewind().map_err(|error| error.to_string())?;
    let marker = LAUNCHER_PROTOCOL.as_bytes();
    let mut buffer = vec![0; 65_536 + marker.len()];
    let mut overlap = 0;
    loop {
        let count = file
            .read(&mut buffer[overlap..overlap + 65_536])
            .map_err(|error| error.to_string())?;
        if count == 0 {
            return Ok(false);
        }
        let end = overlap + count;
        if buffer[..end]
            .windows(marker.len())
            .any(|window| window == marker)
        {
            return Ok(true);
        }
        overlap = (marker.len() - 1).min(end);
        buffer.copy_within(end - overlap..end, 0);
    }
}

struct Stage(PathBuf);

impl Stage {
    fn new(parent: &Path) -> crate::Result<Self> {
        external(parent)?;
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        let path = parent.join(format!("bundle-stage-{}", crate::startup_process::unique()));
        external(&path)?;
        fs::create_dir(&path).map_err(|error| error.to_string())?;
        Ok(Self(path))
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        if external(&self.0).is_ok() {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}

fn create_file(path: &Path) -> crate::Result<File> {
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

fn create(directory: &Path, digest: &str, output: &Path) -> crate::Result<Value> {
    let package = super::checked(directory, digest)?;
    if package["Manifest"]["schemaVersion"] != 2 {
        return Err("Executable bundling requires a schema-2 package.".into());
    }
    external(output)?;
    if fs::exists(output).map_err(|error| error.to_string())? {
        return Err("Executable bundle output already exists.".into());
    }
    let manifest_bytes = super::metadata(&directory.join("package.json"))?;
    if crate::identity::sha256(&manifest_bytes) != digest {
        return Err("Package manifest changed during bundling.".into());
    }
    let manifest: Manifest = serde_json::from_slice(super::json_bytes(&manifest_bytes))
        .map_err(|error| error.to_string())?;
    let stage = Stage::new(output.parent().ok_or("Missing bundle output parent.")?)?;
    let path = stage.0.join("application.exe");
    let mut destination = create_file(&path)?;
    let mut image = super::open(&directory.join("bin/bonsai-tools.exe"))?;
    if footer(&mut image)?.is_some() {
        return Err("Nested executable bundles are unsupported.".into());
    }
    if !supports_bundle(&mut image)? {
        return Err("Maintenance executable does not support the bundle launcher protocol.".into());
    }
    image.rewind().map_err(|error| error.to_string())?;
    let offset = image.metadata().map_err(|error| error.to_string())?.len();
    let info = Layout {
        offset,
        manifest_bytes,
        manifest,
        digest: digest.into(),
    };
    if transfer(&mut image, offset, &mut destination)? != info.entry("bin/bonsai-tools.exe")?.sha256
    {
        return Err("Maintenance executable changed during bundling.".into());
    }
    destination
        .write_all(
            &u64::try_from(info.manifest_bytes.len())
                .map_err(|error| error.to_string())?
                .to_le_bytes(),
        )
        .and_then(|()| destination.write_all(&info.manifest_bytes))
        .map_err(|error| error.to_string())?;
    let mut payload_bytes =
        8 + u64::try_from(info.manifest_bytes.len()).map_err(|error| error.to_string())?;
    for name in FILES {
        let entry = info.entry(name)?;
        let mut source = super::open(&directory.join(name))?;
        if source.metadata().map_err(|error| error.to_string())?.len() != entry.bytes
            || transfer(&mut source, entry.bytes, &mut destination)? != entry.sha256
        {
            return Err(format!("Package input changed during bundling: {name}"));
        }
        payload_bytes += entry.bytes;
    }
    for bytes in [
        offset.to_le_bytes().as_slice(),
        payload_bytes.to_le_bytes().as_slice(),
        digest.as_bytes(),
        MAGIC,
    ] {
        destination
            .write_all(bytes)
            .map_err(|error| error.to_string())?;
    }
    destination.sync_all().map_err(|error| error.to_string())?;
    drop(destination);
    inspect(&path)?;
    external(output)?;
    if fs::exists(output).map_err(|error| error.to_string())? {
        return Err("Executable bundle output already exists.".into());
    }
    fs::rename(&path, output).map_err(|error| error.to_string())?;
    Ok(info.receipt(output))
}

fn inspect(executable: &Path) -> crate::Result<Value> {
    let mut file = super::open(executable)?;
    let info = layout(&mut file)?;
    crate::startup_cache::assert_arm64(executable)?;
    verify_image(&mut file, &info)?;
    for name in FILES {
        let entry = info.entry(name)?;
        if transfer(&mut file, entry.bytes, std::io::sink())? != entry.sha256 {
            return Err(format!("Bundled package SHA-256 mismatch: {name}"));
        }
    }
    Ok(info.receipt(executable))
}

fn extract(executable: &Path, installation: &Path) -> crate::Result<Value> {
    let mut file = super::open(executable)?;
    let info = layout(&mut file)?;
    crate::startup_cache::assert_arm64(executable)?;
    verify_image(&mut file, &info)?;
    external(installation)?;
    fs::create_dir_all(installation).map_err(|error| error.to_string())?;
    let _guard = crate::startup_cache::lock_file(&installation.join("bundle.lock"))?;
    if fs::exists(installation.join("active.json")).map_err(|error| error.to_string())? {
        let mut cached = super::installed(installation)?;
        if cached["ManifestSha256"] == info.digest {
            for name in FILES {
                let entry = info.entry(name)?;
                if transfer(&mut file, entry.bytes, std::io::sink())? != entry.sha256 {
                    return Err(format!("Bundled package SHA-256 mismatch: {name}"));
                }
            }
            cached["InstallationDirectory"] = json!(installation);
            return Ok(cached);
        }
    }
    let stage = Stage::new(installation)?;
    fs::create_dir(stage.0.join("bin")).map_err(|error| error.to_string())?;
    for name in FILES {
        let entry = info.entry(name)?;
        let mut output = create_file(&stage.0.join(name))?;
        if transfer(&mut file, entry.bytes, &mut output)? != entry.sha256 {
            return Err(format!("Bundled package SHA-256 mismatch: {name}"));
        }
        output.sync_all().map_err(|error| error.to_string())?;
    }
    let mut metadata = create_file(&stage.0.join("package.json"))?;
    metadata
        .write_all(&info.manifest_bytes)
        .and_then(|()| metadata.sync_all())
        .map_err(|error| error.to_string())?;
    drop(metadata);
    let mut result = super::management::activate(installation, Some((&stage.0, &info.digest)))?;
    result["InstallationDirectory"] = json!(installation);
    Ok(result)
}

/// Creates, inspects or extracts one native single-file distribution.
///
/// # Errors
/// Rejects changed bytes, malformed identities, unsupported inventories, unsafe
/// paths, oversized payloads and existing output files. Extraction executes no code.
pub fn run(value: Value) -> crate::Result<Value> {
    match serde_json::from_value(value).map_err(|error| error.to_string())? {
        Request::Create {
            package_directory,
            manifest_sha256,
            output_path,
        } => create(&package_directory, &manifest_sha256, &output_path),
        Request::Inspect { executable_path } => inspect(&executable_path),
        Request::Extract {
            executable_path,
            installation_directory,
        } => extract(&executable_path, &installation_directory),
    }
}

/// Reports whether the running maintenance image has an application payload.
///
/// # Errors
/// Rejects an unreadable executable or a recognized, malformed bundle footer.
pub fn current_is_bundled() -> crate::Result<bool> {
    let mut file = super::open(&std::env::current_exe().map_err(|error| error.to_string())?)?;
    Ok(footer(&mut file)?.is_some())
}

/// Extracts the running distribution and enters the existing owned chat launcher.
///
/// # Errors
/// Rejects unsafe launch overrides, unavailable per-user storage and bundle or
/// cached-package corruption. Model/runtime preparation retains installed policy.
pub fn launch(arguments: &[OsString]) -> crate::Result<Value> {
    if arguments
        .iter()
        .any(|argument| argument == "--installation-directory")
    {
        return Err("Bundled chat owns its application cache directory.".into());
    }
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let mut file = super::open(&executable)?;
    let info = layout(&mut file)?;
    let installation = PathBuf::from(
        std::env::var_os("LOCALAPPDATA").ok_or("Bundled chat requires LOCALAPPDATA.")?,
    )
    .join("SwiftBonsai2/application-bundles")
    .join(&info.digest);
    let mut options = vec![
        OsString::from("--installation-directory"),
        installation.clone().into_os_string(),
    ];
    options.extend_from_slice(arguments);
    let request = crate::cli::installed_chat(&options)?;
    extract(&executable, &installation)?;
    super::run(request)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> crate::Result<(PathBuf, PathBuf, String)> {
        let root = super::super::tests::fixture()?;
        let package = super::super::installed(&root)?;
        let directory = PathBuf::from(package["Directory"].as_str().ok_or("Missing directory.")?);
        let digest = package["ManifestSha256"]
            .as_str()
            .ok_or("Missing digest.")?
            .to_owned();
        Ok((root, directory, digest))
    }

    #[test]
    fn bundle_roundtrip_preserves_package_and_rejects_cached_tampering() -> crate::Result<()> {
        let (root, directory, digest) = fixture()?;
        let executable = root.join("one application.exe");
        let created = create(&directory, &digest, &executable)?;
        let inspected = inspect(&executable)?;
        assert_eq!(created, inspected);
        assert_eq!(inspected["ManifestSha256"], digest);
        let installation = root.join("new installation");
        let extracted = extract(&executable, &installation)?;
        assert_eq!(extracted["Id"], created["Id"]);
        assert_eq!(extract(&executable, &installation)?, extracted);
        let target = PathBuf::from(
            extracted["Directory"]
                .as_str()
                .ok_or("Missing directory.")?,
        );
        fs::write(target.join("NOTICE"), b"tampered").map_err(|error| error.to_string())?;
        assert!(extract(&executable, &installation).is_err());
        assert!(create(&directory, &digest, &executable).is_err());
        fs::remove_dir_all(root).map_err(|error| error.to_string())?;
        Ok(())
    }

    #[test]
    fn bundle_rejects_payload_bootstrap_footer_and_truncation_corruption() -> crate::Result<()> {
        let (root, directory, digest) = fixture()?;
        let executable = root.join("application.exe");
        create(&directory, &digest, &executable)?;
        let original = fs::read(&executable).map_err(|error| error.to_string())?;
        let mut input = super::super::open(&executable)?;
        let info = layout(&mut input)?;
        drop(input);
        let manifest_start = usize::try_from(info.offset).map_err(|error| error.to_string())? + 8;
        for position in [
            200,
            manifest_start,
            manifest_start + info.manifest_bytes.len() + 200,
            original.len() - 96,
        ] {
            let mut changed = original.clone();
            changed[position] ^= 1;
            fs::write(&executable, &changed).map_err(|error| error.to_string())?;
            assert!(inspect(&executable).is_err());
            assert!(extract(&executable, &root.join("rejected")).is_err());
        }
        fs::write(&executable, &original[..original.len() - 1])
            .map_err(|error| error.to_string())?;
        assert!(inspect(&executable).is_err());
        fs::remove_dir_all(root).map_err(|error| error.to_string())?;
        Ok(())
    }

    #[test]
    fn bundle_rejects_unsafe_paths_and_inventory_before_publication() -> crate::Result<()> {
        let (root, directory, digest) = fixture()?;
        assert!(create(&directory, &digest, Path::new("relative.exe")).is_err());
        let manifest_path = directory.join("package.json");
        let mut manifest: Value =
            serde_json::from_slice(&fs::read(&manifest_path).map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())?;
        manifest["files"][0]["name"] = json!("../outside.exe");
        let bytes = serde_json::to_vec(&manifest).map_err(|error| error.to_string())?;
        fs::write(&manifest_path, &bytes).map_err(|error| error.to_string())?;
        assert!(
            create(
                &directory,
                &crate::identity::sha256(&bytes),
                &root.join("rejected.exe")
            )
            .is_err()
        );
        assert!(!root.join("rejected.exe").exists());
        fs::remove_dir_all(root).map_err(|error| error.to_string())?;
        Ok(())
    }
}
