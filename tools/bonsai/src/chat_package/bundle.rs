//! Single-file distributions use the existing ZIP and verified package contracts.
use super::{
    FILES,
    management::{Stage, create_file},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    ffi::OsString,
    fs::{self, File},
    io::{Read as _, Seek as _},
    path::{Path, PathBuf},
};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

/// Exact compatibility marker retained by the maintenance version receipt.
pub const LAUNCHER_PROTOCOL: &str = "Bonsai executable bundle launcher protocol 1";
const MAX_IMAGE_BYTES: u64 = 512 * 1024 * 1024;

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

fn archive(executable: &Path) -> crate::Result<(ZipArchive<File>, String)> {
    let file = super::open(executable)?;
    if file.metadata().map_err(|error| error.to_string())?.len()
        > 6 * MAX_IMAGE_BYTES + 50 * 1024 * 1024
    {
        return Err("Executable bundle exceeds its byte limit.".into());
    }
    let mut archive = ZipArchive::new(file).map_err(|error| error.to_string())?;
    let comment = std::str::from_utf8(archive.comment()).map_err(|error| error.to_string())?;
    let digest = comment
        .strip_prefix(&format!("{LAUNCHER_PROTOCOL}:"))
        .filter(|digest| crate::identity::is_lower_hex(digest, 64))
        .ok_or("Invalid bundle identity.")?
        .to_owned();
    if archive.len() != FILES.len() + 1 {
        return Err("Unexpected bundled package inventory.".into());
    }
    let mut names = std::collections::BTreeSet::new();
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|error| error.to_string())?;
        let name = entry.name().map_err(|error| error.to_string())?;
        let limit = if name == "package.json" {
            super::MAX_METADATA_BYTES
        } else if name.starts_with("bin/") {
            MAX_IMAGE_BYTES
        } else {
            16 * 1024 * 1024
        };
        if !(FILES.contains(&name.as_ref()) || name == "package.json")
            || !names.insert(name.into_owned())
            || !entry.is_file()
            || entry.encrypted()
            || !matches!(
                entry.compression(),
                CompressionMethod::Stored | CompressionMethod::Deflated
            )
            || entry.size() == 0
            || entry.size() > limit
            || (entry.compression() == CompressionMethod::Stored
                && entry.compressed_size() != entry.size())
        {
            return Err("Invalid bundled package entry.".into());
        }
    }
    Ok((archive, digest))
}

pub(crate) fn require_windowless(file: &mut File) -> crate::Result<()> {
    file.rewind().map_err(|error| error.to_string())?;
    let mut dos = [0; 64];
    file.read_exact(&mut dos)
        .map_err(|error| error.to_string())?;
    let offset = u64::from(u32::from_le_bytes(
        dos[60..64].try_into().map_err(|_| "Invalid PE offset.")?,
    ));
    if dos[..2] != *b"MZ" || offset < 64 {
        return Err("Invalid launcher DOS header.".into());
    }
    file.seek(std::io::SeekFrom::Start(offset))
        .map_err(|error| error.to_string())?;
    let mut pe = [0; 94];
    file.read_exact(&mut pe)
        .map_err(|error| error.to_string())?;
    if pe[..6] != [b'P', b'E', 0, 0, 0x64, 0xaa]
        || u16::from_le_bytes([pe[20], pe[21]]) < 70
        || pe[24..26] != [0x0b, 0x02]
        || pe[92..94] != [2, 0]
    {
        return Err("Bundling requires the Windows GUI-subsystem bonsai-launcher image.".into());
    }
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

fn unpack(executable: &Path, directory: &Path) -> crate::Result<Value> {
    let (mut archive, digest) = archive(executable)?;
    let prefix = archive
        .by_index(0)
        .map_err(|error| error.to_string())?
        .header_start();
    fs::create_dir(directory.join("bin")).map_err(|error| error.to_string())?;
    for index in 0..archive.len() {
        crate::startup_cancel::checkpoint()?;
        let mut entry = archive.by_index(index).map_err(|error| error.to_string())?;
        let size = entry.size();
        let mut output = create_file(
            &directory.join(entry.name().map_err(|error| error.to_string())?.as_ref()),
        )?;
        if std::io::copy(&mut entry.by_ref().take(size + 1), &mut output)
            .map_err(|error| error.to_string())?
            != size
        {
            return Err("Bundled package entry length differs.".into());
        }
        output.sync_all().map_err(|error| error.to_string())?;
    }
    let package = super::checked(directory, &digest)?;
    if package["Manifest"]["schemaVersion"] != 2 {
        return Err("Bundling requires a schema-2 package.".into());
    }
    let tool = super::open(&directory.join("bin/bonsai-tools.exe"))?;
    if prefix != tool.metadata().map_err(|error| error.to_string())?.len() {
        return Err("Bundle launcher length differs.".into());
    }
    let mut image = archive.into_inner();
    image.rewind().map_err(|error| error.to_string())?;
    let mut image = image.take(prefix);
    let mut hash = crate::identity::Hasher::new()?;
    let mut buffer = vec![0; 1024 * 1024];
    loop {
        crate::startup_cancel::checkpoint()?;
        let count = image.read(&mut buffer).map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count])?;
    }
    if hash.finalize_hex()?
        != crate::startup_cache::digest_file(&directory.join("bin/bonsai-tools.exe"))?
    {
        return Err("Bundle launcher differs from its embedded maintenance executable.".into());
    }
    Ok(package)
}

fn inspect(executable: &Path) -> crate::Result<Value> {
    let stage = Stage::new(&std::env::temp_dir())?;
    let mut package = unpack(executable, &stage.0)?;
    package
        .as_object_mut()
        .ok_or("Invalid package receipt.")?
        .remove("Directory");
    package["ExecutablePath"] = json!(executable);
    Ok(package)
}

fn create(directory: &Path, digest: &str, output: &Path) -> crate::Result<Value> {
    let package = super::checked(directory, digest)?;
    if package["Manifest"]["schemaVersion"] != 2 {
        return Err("Bundling requires a schema-2 package.".into());
    }
    for name in FILES.iter().filter(|name| name.starts_with("bin/")) {
        super::portable::verify(&directory.join(name))?;
    }
    bonsai_artifacts::validate_external_path(output).map_err(|error| error.to_string())?;
    if fs::exists(output).map_err(|error| error.to_string())? {
        return Err("Executable bundle output already exists.".into());
    }
    let stage = Stage::new(output.parent().ok_or("Missing bundle output parent.")?)?;
    let path = stage.0.join("application.exe");
    let mut image = super::open(&directory.join("bin/bonsai-tools.exe"))?;
    require_windowless(&mut image)?;
    if !supports_bundle(&mut image)? {
        return Err("Maintenance executable does not support the bundle launcher protocol.".into());
    }
    image.rewind().map_err(|error| error.to_string())?;
    let mut destination = create_file(&path)?;
    std::io::copy(&mut image, &mut destination).map_err(|error| error.to_string())?;
    let mut writer = ZipWriter::new(destination);
    writer
        .set_comment(format!("{LAUNCHER_PROTOCOL}:{digest}"))
        .map_err(|error| error.to_string())?;
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for name in FILES.iter().copied().chain(std::iter::once("package.json")) {
        writer
            .start_file(name, options)
            .map_err(|error| error.to_string())?;
        std::io::copy(&mut super::open(&directory.join(name))?, &mut writer)
            .map_err(|error| error.to_string())?;
    }
    writer
        .finish()
        .and_then(|file| file.sync_all().map_err(Into::into))
        .map_err(|error| error.to_string())?;
    let mut receipt = inspect(&path)?;
    if fs::exists(output).map_err(|error| error.to_string())? {
        return Err("Executable bundle output already exists.".into());
    }
    fs::rename(&path, output).map_err(|error| error.to_string())?;
    receipt["ExecutablePath"] = json!(output);
    Ok(receipt)
}

fn extract(executable: &Path, installation: &Path) -> crate::Result<Value> {
    let stage = Stage::new(installation)?;
    let package = unpack(executable, &stage.0)?;
    let digest = package["ManifestSha256"]
        .as_str()
        .ok_or("Missing package digest.")?;
    let mut receipt = super::management::activate(installation, Some((&stage.0, digest)))?;
    receipt["InstallationDirectory"] = json!(installation);
    Ok(receipt)
}

/// Creates, inspects or extracts a verified single-file distribution.
///
/// # Errors
/// Rejects unsafe paths, changed bytes, invalid inventories and unsupported images.
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

/// Reports whether the current image has a recognized bundle comment.
///
/// # Errors
/// Rejects an unavailable running executable.
pub fn current_is_bundled() -> crate::Result<bool> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let file = super::open(&executable)?;
    Ok(ZipArchive::new(file)
        .is_ok_and(|archive| archive.comment().starts_with(LAUNCHER_PROTOCOL.as_bytes())))
}

pub(crate) fn update_identity() -> crate::Result<(PathBuf, String)> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let (_, digest) = archive(&executable)?;
    let root = PathBuf::from(
        std::env::var_os("LOCALAPPDATA").ok_or("Bundled chat requires LOCALAPPDATA.")?,
    )
    .join("SwiftBonsai2/application-bundles")
    .join(&digest[..24]);
    Ok((root, digest))
}

/// Extracts the current distribution and enters the existing owned chat launcher.
///
/// # Errors
/// Rejects invalid bundle bytes, cached-package corruption and unsafe launch options.
pub fn launch(arguments: &[OsString]) -> crate::Result<Value> {
    if arguments
        .iter()
        .any(|argument| argument == "--installation-directory")
    {
        return Err("Bundled chat owns its application cache directory.".into());
    }
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let (_, digest) = archive(&executable)?;
    let installation = PathBuf::from(
        std::env::var_os("LOCALAPPDATA").ok_or("Bundled chat requires LOCALAPPDATA.")?,
    )
    .join("SwiftBonsai2/application-bundles")
    .join(&digest[..24]);
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
        Ok((
            root,
            PathBuf::from(package["Directory"].as_str().ok_or("Missing directory.")?),
            package["ManifestSha256"]
                .as_str()
                .ok_or("Missing digest.")?
                .into(),
        ))
    }

    #[test]
    fn zip_bundle_roundtrip_and_cached_tampering() -> crate::Result<()> {
        let (root, directory, digest) = fixture()?;
        let executable = root.join("one application.exe");
        assert_eq!(
            create(&directory, &digest, &executable)?,
            inspect(&executable)?
        );
        let installation = root.join("new installation");
        let receipt = extract(&executable, &installation)?;
        assert_eq!(extract(&executable, &installation)?, receipt);
        let cached = PathBuf::from(receipt["Directory"].as_str().ok_or("Missing directory.")?);
        fs::write(cached.join("NOTICE"), b"tampered").map_err(|error| error.to_string())?;
        assert!(extract(&executable, &installation).is_err());
        assert!(create(&directory, &digest, &executable).is_err());
        fs::remove_dir_all(root).map_err(|error| error.to_string())?;
        Ok(())
    }

    #[test]
    fn bundle_rejects_console_launcher_before_publication() -> crate::Result<()> {
        let (root, directory, _) = fixture()?;
        let image_path = directory.join("bin/bonsai-tools.exe");
        let mut image = fs::read(&image_path).map_err(|error| error.to_string())?;
        image[220..222].copy_from_slice(&3_u16.to_le_bytes());
        fs::write(&image_path, &image).map_err(|error| error.to_string())?;
        let manifest_path = directory.join("package.json");
        let mut manifest: Value =
            serde_json::from_slice(&fs::read(&manifest_path).map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())?;
        let files = manifest["files"].as_array_mut().ok_or("Missing files.")?;
        let entry = files
            .iter_mut()
            .find(|entry| entry["name"] == "bin/bonsai-tools.exe")
            .ok_or("Missing launcher.")?;
        entry["sha256"] = json!(crate::identity::sha256(&image));
        let bytes = serde_json::to_vec(&manifest).map_err(|error| error.to_string())?;
        fs::write(manifest_path, &bytes).map_err(|error| error.to_string())?;
        let output = root.join("console.exe");
        let error = create(&directory, &crate::identity::sha256(&bytes), &output)
            .err()
            .ok_or("A console launcher was accepted.")?;
        assert!(error.contains("GUI-subsystem"));
        assert!(!output.exists());
        fs::remove_dir_all(root).map_err(|error| error.to_string())?;
        Ok(())
    }

    #[test]
    fn zip_bundle_rejects_corruption_truncation_and_unsafe_inventory() -> crate::Result<()> {
        let (root, directory, digest) = fixture()?;
        let executable = root.join("application.exe");
        create(&directory, &digest, &executable)?;
        let original = fs::read(&executable).map_err(|error| error.to_string())?;
        let (mut archive, _) = archive(&executable)?;
        let data = usize::try_from(
            archive
                .by_index(0)
                .map_err(|error| error.to_string())?
                .data_start()
                .ok_or("Missing ZIP data offset.")?,
        )
        .map_err(|error| error.to_string())?;
        drop(archive);
        for position in [200, data, original.len() - 1] {
            let mut changed = original.clone();
            if position == data {
                // BTYPE=3 is reserved; changing BFINAL alone can preserve decoded bytes.
                changed[position] = (changed[position] & !0x06) | 0x06;
            } else {
                changed[position] ^= 1;
            }
            fs::write(&executable, changed).map_err(|error| error.to_string())?;
            assert!(inspect(&executable).is_err());
            assert!(extract(&executable, &root.join("rejected")).is_err());
        }
        fs::write(&executable, &original[..original.len() - 1])
            .map_err(|error| error.to_string())?;
        assert!(inspect(&executable).is_err());
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
        fs::remove_dir_all(root).map_err(|error| error.to_string())?;
        Ok(())
    }
}
