//! Bounded HTTPS transfers and immutable local staging files.

use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read as _, Seek as _, SeekFrom, Write as _},
    path::Path,
    sync::OnceLock,
    time::Duration,
};

use super::{MAX_METADATA, external, policy::Asset};

static CURL: OnceLock<std::path::PathBuf> = OnceLock::new();

fn curl(working: &Path) -> crate::Result<std::path::PathBuf> {
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
    let mut bytes = Vec::new();
    file.take(maximum + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if u64::try_from(bytes.len()).map_err(|error| error.to_string())? > maximum {
        return Err("Update metadata exceeds its byte limit.".into());
    }
    Ok(bytes)
}

pub(super) fn verify(file: &mut File, bytes: u64, sha256: &str) -> crate::Result<()> {
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
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        options.share_mode(0).custom_flags(0x0020_0000);
    }
    let mut file = options.open(path).map_err(|error| error.to_string())?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| error.to_string())
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

pub(super) fn metadata(url: &str, working: &Path) -> crate::Result<Vec<u8>> {
    let path = working.join(format!(
        "metadata-{}.json",
        crate::startup_process::unique()
    ));
    let log = path.with_extension("log");
    external(&path)?;
    // The public API has no authentication headers. Artifact URLs are admitted
    // only by the offline canonical-project release policy before reaching here.
    let arguments = [
        "--disable".into(),
        "--silent".into(),
        "--show-error".into(),
        "--location".into(),
        "--max-redirs".into(),
        "3".into(),
        "--proto".into(),
        "=https".into(),
        "--proto-redir".into(),
        "=https".into(),
        "--disallow-username-in-url".into(),
        "--connect-timeout".into(),
        "5".into(),
        "--max-time".into(),
        "10".into(),
        "--max-filesize".into(),
        MAX_METADATA.to_string().into(),
        "--header".into(),
        "Accept: application/vnd.github+json".into(),
        "--header".into(),
        format!(
            "User-Agent: SwiftBonsai2Runner/{}",
            env!("CARGO_PKG_VERSION")
        )
        .into(),
        "--header".into(),
        "Accept-Encoding: identity".into(),
        "--write-out".into(),
        "%{http_code}".into(),
        "--output".into(),
        path.as_os_str().to_owned(),
        url.into(),
    ];
    let curl = curl(working)?;
    let result = crate::startup_process::run(
        curl.as_os_str(),
        &arguments,
        working,
        &BTreeMap::new(),
        Duration::from_secs(11),
        &log,
    )?;
    if result.code == 0
        && result.stdout.trim() == "404"
        && url
            == format!(
                "https://api.github.com/repos/{}/releases?per_page=100",
                super::REPOSITORY
            )
    {
        fs::remove_file(&path).map_err(|error| error.to_string())?;
        return Ok(b"[]".to_vec());
    }
    if result.code != 0 || result.stdout.trim() != "200" {
        return Err(format!(
            "GitHub update check failed (HTTP {}, tool {}). Your installed app is unchanged.",
            result.stdout.trim(),
            result.code
        ));
    }
    let bytes = read_bounded(&path, MAX_METADATA)?;
    fs::remove_file(&path).map_err(|error| error.to_string())?;
    Ok(bytes)
}

pub(super) fn asset_metadata(asset: &Asset, working: &Path) -> crate::Result<Vec<u8>> {
    let bytes = metadata(&asset.url, working)?;
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
    fn staged_bytes_reject_tampering_and_metadata_bounds() -> crate::Result<()> {
        let root = std::env::temp_dir().join(format!(
            "Bonsai-Update-{}",
            crate::startup_process::unique()
        ));
        fs::create_dir(&root).map_err(|error| error.to_string())?;
        let path = root.join("fixture.msi");
        write_exclusive(&path, b"original")?;
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
}
