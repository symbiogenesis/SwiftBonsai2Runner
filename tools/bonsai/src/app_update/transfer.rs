//! Bounded HTTPS transfers and immutable local staging files.

use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write as _,
    path::{Path, PathBuf},
    sync::OnceLock,
    time::Duration,
};

#[cfg(test)]
use std::{
    fs::File,
    io::{Read as _, Seek as _, SeekFrom},
};

use super::{MAX_METADATA, external, policy::Asset};

mod http;

static CURL: OnceLock<PathBuf> = OnceLock::new();

struct TemporaryBody(PathBuf);

impl Drop for TemporaryBody {
    fn drop(&mut self) {
        // Remove only this request's unpublished regular file. Keep diagnostics
        // separately; neither failed HTTP responses nor partial JSON are cache.
        if external(&self.0).is_ok() {
            let _removed = fs::remove_file(&self.0);
        }
    }
}

fn curl(working: &Path) -> crate::Result<PathBuf> {
    if let Some(path) = CURL.get() {
        return Ok(path.clone());
    }
    let path = crate::startup_download::system_tool("curl.exe")?;
    let result = crate::startup_process::run(
        path.as_os_str(),
        &["--disable".into(), "--version".into()],
        working,
        &BTreeMap::new(),
        Duration::from_secs(3),
        &working.join(format!(
            "curl-version-{}.log",
            crate::startup_process::unique()
        )),
    )?;
    let words: Vec<_> = result.stdout.split_whitespace().take(2).collect();
    let version: Vec<_> = words
        .get(1)
        .unwrap_or(&"")
        .split('.')
        .take(2)
        .map(str::parse::<u32>)
        .collect();
    if result.code != 0
        || words.first() != Some(&"curl")
        || !matches!(version.as_slice(), [Ok(major), Ok(minor)] if *major > 8 || (*major == 8 && *minor >= 4))
    {
        return Err(
            "Application updates require Windows curl 8.4 or newer for bounded downloads.".into(),
        );
    }
    let _ = CURL.set(path.clone());
    Ok(path)
}

pub(super) fn read_bounded(path: &Path, maximum: u64) -> crate::Result<Vec<u8>> {
    external(path)?;
    let file = bonsai_artifacts::open_locked_file(path).map_err(|error| error.to_string())?;
    if file.metadata().map_err(|error| error.to_string())?.len() > maximum {
        return Err("Update metadata exceeds its byte limit.".into());
    }
    crate::bounded_input::read(file, maximum)
        .map_err(|error| error.into_message("Update metadata exceeds its byte limit."))
}

#[cfg(test)]
fn verify(file: &mut File, bytes: u64, sha256: &str) -> crate::Result<()> {
    if bytes == 0
        || file.metadata().map_err(|error| error.to_string())?.len() != bytes
        || !crate::identity::is_lower_hex(sha256, 64)
    {
        return Err("Update file has an invalid byte length or SHA-256 identity.".into());
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|error| error.to_string())?;
    let mut hasher = crate::identity::Hasher::new()?;
    let mut buffer = vec![0; 65_536];
    loop {
        crate::startup_cancel::checkpoint()?;
        let count = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count])?;
    }
    if hasher.finalize_hex()? != sha256 {
        return Err(
            "Update SHA-256 differs from the published release; no installer was started.".into(),
        );
    }
    Ok(())
}

pub(super) fn write_exclusive(path: &Path, bytes: &[u8]) -> crate::Result<()> {
    external(path)?;
    let temporary = path.with_extension(format!("{}.partial", crate::startup_process::unique()));
    external(&temporary)?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        options.share_mode(0).custom_flags(0x0020_0000);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|error| error.to_string())?;
    let temporary = TemporaryBody(temporary);
    let written = file
        .write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| error.to_string());
    drop(file);
    written?;
    external(path)?;
    // Publish complete, durable bytes without replacing an existing handoff or
    // receipt. A short write must never leave a corrupt final cache identity.
    fs::hard_link(&temporary.0, path).map_err(|error| error.to_string())
}

pub(super) fn write_new_or_identical(path: &Path, bytes: &[u8]) -> crate::Result<()> {
    if fs::exists(path).map_err(|error| error.to_string())? {
        if read_bounded(path, MAX_METADATA)? != bytes {
            return Err("A different update already uses this version's staging directory.".into());
        }
        return Ok(());
    }
    write_exclusive(path, bytes)
}

/// Publishes newly verified metadata or a freshly confirmed handoff. Existing
/// cache bytes never authenticate this replacement; the caller supplies a fresh
/// published identity and retains its update-stage lock through this operation.
pub(super) fn write_current(path: &Path, bytes: &[u8]) -> crate::Result<()> {
    crate::startup_cancel::checkpoint()?;
    external(path)?;
    if u64::try_from(bytes.len()).map_err(|error| error.to_string())? > MAX_METADATA {
        return Err("Update metadata exceeds its byte limit.".into());
    }
    let _parents = super::portable::hold_parents(path)?;
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !bonsai_artifacts::is_redirect(&metadata) => {}
        Ok(_) => return Err("Update metadata destination is not an ordinary file.".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    let temporary = path.with_extension(format!("{}.partial", crate::startup_process::unique()));
    external(&temporary)?;
    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        options
            .access_mode(0xc001_0000)
            .share_mode(0)
            .custom_flags(0x0020_0000);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|error| error.to_string())?;
    let temporary = TemporaryBody(temporary);
    let published = (|| {
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|error| error.to_string())?;
        crate::startup_cancel::checkpoint()?;
        external(path)?;
        #[cfg(windows)]
        let result = bonsai_windows_process::replace_open_file(&file, path);
        #[cfg(not(windows))]
        let result = fs::rename(&temporary.0, path);
        result.map_err(|error| error.to_string())
    })();
    // Close the exclusive file before the temporary guard cleans an unpublished
    // file. Successful handle-based publication has already consumed its name.
    drop(file);
    drop(temporary);
    published
}

fn require_successful_http(code: u32, status: &str) -> crate::Result<()> {
    if code != 0 || status.trim() != "200" {
        return Err(format!(
            "GitHub update check failed (HTTP {}, tool {}). Your installed app is unchanged.",
            status.trim(),
            code
        ));
    }
    Ok(())
}

pub(super) fn metadata(url: &str, working: &Path) -> crate::Result<Vec<u8>> {
    http::metadata(url, working, None, false)
}

pub(super) fn cached_metadata(url: &str, working: &Path) -> crate::Result<Vec<u8>> {
    http::metadata(url, working, None, true)
}

pub(super) fn asset_metadata(asset: &Asset, working: &Path) -> crate::Result<Vec<u8>> {
    checked_asset(
        asset,
        http::metadata(&asset.url, working, Some(asset), false)?,
    )
}

pub(super) fn cached_asset_metadata(asset: &Asset, working: &Path) -> crate::Result<Vec<u8>> {
    checked_asset(
        asset,
        http::metadata(&asset.url, working, Some(asset), true)?,
    )
}

fn checked_asset(asset: &Asset, bytes: Vec<u8>) -> crate::Result<Vec<u8>> {
    if u64::try_from(bytes.len()).map_err(|error| error.to_string())? != asset.bytes
        || crate::identity::sha256(&bytes) != asset.sha256
    {
        return Err("Release metadata differs from GitHub's published asset digest.".into());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_http_failures_never_masquerade_as_an_empty_release_catalog() -> crate::Result<()> {
        require_successful_http(0, "200\n")?;
        for (code, status) in [
            (0, "404"),
            (0, "403"),
            (0, "429"),
            (0, "503"),
            (28, "200"),
            (6, "000"),
        ] {
            let error = require_successful_http(code, status)
                .err()
                .ok_or("Accepted a failed update lookup.")?;
            assert!(error.contains("update check failed"));
            assert!(error.contains("installed app is unchanged"));
        }
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn refreshed_metadata_is_atomic_and_keeps_previous_bytes_on_sharing_failure()
    -> crate::Result<()> {
        let root = std::env::temp_dir().join(format!(
            "Bonsai-Update-Refresh-{}",
            crate::startup_process::unique()
        ));
        external(&root)?;
        fs::create_dir(&root).map_err(|error| error.to_string())?;
        let path = root.join("confirmed.json");
        write_current(&path, b"first confirmation")?;
        let guard = bonsai_artifacts::open_locked_file(&path).map_err(|error| error.to_string())?;
        assert!(write_current(&path, b"new confirmation").is_err());
        assert_eq!(read_bounded(&path, MAX_METADATA)?, b"first confirmation");
        assert_eq!(
            fs::read_dir(&root)
                .map_err(|error| error.to_string())?
                .count(),
            1
        );
        drop(guard);
        write_current(&path, b"new confirmation")?;
        assert_eq!(read_bounded(&path, MAX_METADATA)?, b"new confirmation");
        assert_eq!(
            fs::read_dir(&root)
                .map_err(|error| error.to_string())?
                .count(),
            1
        );
        fs::remove_file(path)
            .and_then(|()| fs::remove_dir(root))
            .map_err(|error| error.to_string())
    }

    #[test]
    fn staged_bytes_reject_tampering_and_metadata_bounds() -> crate::Result<()> {
        let root = std::env::temp_dir().join(format!(
            "Bonsai-Update-{}",
            crate::startup_process::unique()
        ));
        fs::create_dir(&root).map_err(|error| error.to_string())?;
        let path = root.join("fixture.msi");
        write_exclusive(&path, b"original")?;
        assert!(write_exclusive(&path, b"replacement").is_err());
        write_new_or_identical(&path, b"original")?;
        assert!(write_new_or_identical(&path, b"different").is_err());
        assert!(read_bounded(&path, 7).is_err());
        let digest = crate::identity::sha256(b"original");
        let mut file = File::open(&path).map_err(|error| error.to_string())?;
        verify(&mut file, 8, &digest)?;
        drop(file);
        fs::write(&path, b"tampered").map_err(|error| error.to_string())?;
        let mut file = File::open(&path).map_err(|error| error.to_string())?;
        assert!(verify(&mut file, 8, &digest).is_err());
        drop(file);
        fs::remove_file(&path)
            .and_then(|()| fs::remove_dir(&root))
            .map_err(|error| error.to_string())
    }

    #[test]
    fn temporary_bodies_are_removed_after_early_return() -> crate::Result<()> {
        let root = std::env::temp_dir().join(format!(
            "Bonsai-Update-Cleanup-{}",
            crate::startup_process::unique()
        ));
        fs::create_dir(&root).map_err(|error| error.to_string())?;
        let path = root.join("response.json");
        fs::write(&path, b"partial response").map_err(|error| error.to_string())?;
        {
            let _body = TemporaryBody(path.clone());
            assert!(read_bounded(&path, 1).is_err());
        }
        assert!(!fs::exists(path).map_err(|error| error.to_string())?);
        fs::remove_dir(root).map_err(|error| error.to_string())
    }
}
