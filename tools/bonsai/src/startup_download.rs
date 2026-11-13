//! Verified, resumable artifact provisioning for native startup.
#![expect(
    clippy::redundant_pub_crate,
    reason = "Sibling startup modules share an explicit crate-local implementation API."
)]

use std::{
    cell::Cell,
    collections::BTreeMap,
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{Seek as _, SeekFrom},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use bonsai_artifacts::{
    ArtifactError, hash_reader, is_redirect, open_locked_file, validate_external_path,
};

thread_local! {
    static REPAIR_DEADLINE: Cell<Option<Instant>> = const { Cell::new(None) };
}

struct RepairDeadline(Option<Instant>);

impl Drop for RepairDeadline {
    fn drop(&mut self) {
        REPAIR_DEADLINE.with(|deadline| deadline.set(self.0));
    }
}

fn repair_checkpoint() -> crate::Result<()> {
    crate::startup_cancel::checkpoint()?;
    REPAIR_DEADLINE.with(|deadline| {
        if deadline.get().is_some_and(|until| Instant::now() >= until) {
            Err("Pinned download repair exceeded its aggregate allowance. Retry resumes downloads or reuses verified files.".into())
        } else {
            Ok(())
        }
    })
}

fn with_repair_deadline<T>(
    timeout: Duration,
    operation: impl FnOnce(Instant) -> crate::Result<T>,
) -> crate::Result<T> {
    let requested = Instant::now()
        .checked_add(timeout)
        .ok_or("Invalid aggregate repair allowance.")?;
    let previous = REPAIR_DEADLINE.with(Cell::get);
    let until = previous.map_or(requested, |outer| outer.min(requested));
    let _restore = RepairDeadline(previous);
    REPAIR_DEADLINE.with(|deadline| deadline.set(Some(until)));
    repair_checkpoint()?;
    let result = operation(until);
    repair_checkpoint()?;
    result
}

fn repair_remaining(until: Instant) -> crate::Result<Duration> {
    repair_checkpoint()?;
    until
        .checked_duration_since(Instant::now())
        .filter(|duration| !duration.is_zero())
        .ok_or_else(|| "Pinned download repair exceeded its aggregate allowance.".into())
}

fn repair_process_allowance(until: Instant) -> crate::Result<Duration> {
    // startup_process retains up to five seconds for owned descendant teardown.
    repair_remaining(until)?
        .checked_sub(Duration::from_secs(5))
        .filter(|duration| !duration.is_zero())
        .ok_or_else(|| {
            "Download repair has insufficient time left for safe process cleanup.".into()
        })
}

fn expected(bytes: u64, sha256: &str) -> crate::Result<()> {
    if bytes == 0
        || bytes > i64::MAX.unsigned_abs()
        || sha256.len() != 64
        || !sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("A positive locked length and SHA-256 are required.".into());
    }
    Ok(())
}

fn digest_matches(file: &mut File, bytes: u64, sha256: &str) -> crate::Result<bool> {
    repair_checkpoint()?;
    expected(bytes, sha256)?;
    if file.metadata().map_err(|error| error.to_string())?.len() != bytes {
        return Err("Artifact byte length differs from its lock.".into());
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|error| error.to_string())?;
    let mut buffer = vec![0; 1024 * 1024];
    let hash = hash_reader(file, bytes, &mut buffer, &mut || {
        repair_checkpoint().map_err(|_| ArtifactError::Cancelled)
    })
    .map_err(|error| error.to_string())?;
    Ok(hash.eq_ignore_ascii_case(sha256))
}

fn check(file: &mut File, bytes: u64, sha256: &str) -> crate::Result<()> {
    crate::startup_cancel::checkpoint()?;
    if !digest_matches(file, bytes, sha256)? {
        return Err("Artifact SHA-256 differs from its lock; invalid bytes are retained.".into());
    }
    Ok(())
}

fn open_read(path: &Path) -> crate::Result<File> {
    open_locked_file(path).map_err(|error| error.to_string())
}

/// Verifies a complete pinned artifact without downloading or modifying it.
pub(crate) fn verify(path: &Path, bytes: u64, sha256: &str) -> crate::Result<()> {
    check(&mut open_read(path)?, bytes, sha256)
}

/// Checks only cached length; native speech verifies the complete hash on use.
pub(crate) fn verify_length(path: &Path, bytes: u64) -> crate::Result<()> {
    if open_read(path)?
        .metadata()
        .map_err(|error| error.to_string())?
        .len()
        != bytes
    {
        return Err("Artifact byte length differs from its lock.".into());
    }
    Ok(())
}

fn download_uri(uri: &str) -> crate::Result<()> {
    let authority = uri
        .strip_prefix("https://")
        .and_then(|suffix| suffix.split(['/', '?', '#']).next())
        .filter(|authority| !authority.is_empty())
        .ok_or("A public HTTPS artifact URL is required.")?;
    if authority.contains(['@', '%', '\\'])
        || uri.chars().any(char::is_whitespace)
        || uri.chars().any(char::is_control)
    {
        return Err("Artifact URL contains credentials or ambiguous characters.".into());
    }
    Ok(())
}

pub(crate) fn system_tool(name: &str) -> crate::Result<PathBuf> {
    let root = PathBuf::from(std::env::var_os("SystemRoot").ok_or("Missing Windows directory.")?);
    validate_external_path(&root).map_err(|error| error.to_string())?;
    let path = root.join("System32").join(name);
    if !path.is_file() {
        return Err(format!(
            "Required Windows tool is unavailable: {}",
            path.display()
        ));
    }
    Ok(path)
}

fn curl_arguments(uri: &str, partial: &Path, offset: u64, bytes: u64) -> Vec<OsString> {
    let mut arguments: Vec<OsString> = [
        "--disable",
        "--fail",
        "--silent",
        "--show-error",
        "--location",
        "--max-redirs",
        "10",
        "--disallow-username-in-url",
        "--proto",
        "=https",
        "--proto-redir",
        "=https",
        "--header",
        "Accept-Encoding: identity",
        "--connect-timeout",
        "60",
        "--max-time",
        "7200",
        "--max-filesize",
    ]
    .into_iter()
    .map(OsString::from)
    .collect();
    arguments.push(bytes.saturating_sub(offset).to_string().into());
    if offset > 0 {
        arguments.extend([OsString::from("--continue-at"), offset.to_string().into()]);
    }
    arguments.extend([
        OsString::from("--output"),
        partial.as_os_str().to_owned(),
        uri.into(),
    ]);
    arguments
}

fn compatible_curl(output: &str) -> bool {
    let mut words = output.split_whitespace();
    if words.next() != Some("curl") {
        return false;
    }
    let mut version = words.next().unwrap_or_default().split('.');
    matches!((version.next().and_then(|part| part.parse::<u64>().ok()), version.next().and_then(|part| part.parse::<u64>().ok())),
        (Some(major), Some(minor)) if major > 8 || (major == 8 && minor >= 4))
}

fn download_tool(working: &Path) -> crate::Result<PathBuf> {
    let curl = system_tool("curl.exe")?;
    let result = crate::startup_process::run(
        curl.as_os_str(),
        &["--disable".into(), "--version".into()],
        working,
        &BTreeMap::new(),
        Duration::from_secs(10),
        &working.join(format!(
            "curl-version-{}.log",
            crate::native_startup::nonce()?
        )),
    )?;
    if result.code != 0 || !compatible_curl(&result.stdout) {
        return Err(
            "Artifact provisioning requires Windows curl 8.4 or newer for bounded downloads."
                .into(),
        );
    }
    Ok(curl)
}

fn transfer(
    uri: &str,
    partial: &Path,
    offset: u64,
    bytes: u64,
    working: &Path,
) -> crate::Result<()> {
    let curl = download_tool(working)?;
    let log = working.join(format!("download-{}.log", crate::native_startup::nonce()?));
    let mut result = crate::startup_process::run(
        curl.as_os_str(),
        &curl_arguments(uri, partial, offset, bytes),
        working,
        &BTreeMap::new(),
        Duration::from_secs(7_230),
        &log,
    )?;
    if result.code == 33 && offset > 0 {
        // curl refuses to append a full response when a server ignores Range.
        // Restart this same locked partial rather than combining responses.
        validate_external_path(partial).map_err(|error| error.to_string())?;
        OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(partial)
            .map_err(|error| error.to_string())?;
        result = crate::startup_process::run(
            curl.as_os_str(),
            &curl_arguments(uri, partial, 0, bytes),
            working,
            &BTreeMap::new(),
            Duration::from_secs(7_230),
            &working.join(format!(
                "download-restart-{}.log",
                crate::native_startup::nonce()?
            )),
        )?;
    }
    if result.code != 0 {
        return Err(format!(
            "Pinned download failed ({}): {}; partial bytes retained at {}",
            result.code,
            result.stderr.trim(),
            partial.display()
        ));
    }
    Ok(())
}

/// Downloads a pinned artifact, resuming partial bytes and publishing exclusively.
pub(crate) fn receive(
    uri: &str,
    path: &Path,
    bytes: u64,
    sha256: &str,
    working: &Path,
) -> crate::Result<()> {
    receive_with(uri, path, bytes, sha256, working, transfer)
}

/// Observable repair phases, emitted only when that work is about to start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RepairActivity {
    /// Checking the selected existing file, before considering any partial.
    VerifyingExisting,
    /// Fetching missing bytes; the offset distinguishes resume from a fresh fetch.
    Downloading { offset: u64 },
    /// Checking the completed partial, with no network transfer in progress.
    VerifyingDownloaded,
    /// Publishing the verified replacement into its final location.
    Publishing,
}

/// Prepares a pinned artifact with explicit transfer and local-work observations.
pub(crate) fn receive_repair_observed(
    uri: &str,
    path: &Path,
    bytes: u64,
    sha256: &str,
    working: &Path,
    observe: &mut dyn FnMut(RepairActivity) -> crate::Result<()>,
) -> crate::Result<()> {
    receive_repair_observed_with(uri, path, bytes, sha256, working, transfer, observe)
}

/// Shares one absolute allowance across verification, resume and fresh restart.
pub(crate) fn receive_repair_with_timeout(
    uri: &str,
    path: &Path,
    bytes: u64,
    sha256: &str,
    working: &Path,
    timeout: Duration,
) -> crate::Result<()> {
    with_repair_deadline(timeout, |until| {
        receive_repair_with(
            uri,
            path,
            bytes,
            sha256,
            working,
            |uri, partial, offset, bytes, working| {
                transfer_repair_until(uri, partial, offset, bytes, working, until)
            },
        )
    })
}

fn repair_download_tool(working: &Path, until: Instant) -> crate::Result<PathBuf> {
    let curl = system_tool("curl.exe")?;
    let allowance = repair_process_allowance(until)?.min(Duration::from_secs(10));
    let result = crate::startup_process::run_with_checkpoint(
        curl.as_os_str(),
        &["--disable".into(), "--version".into()],
        working,
        &BTreeMap::new(),
        allowance,
        &working.join(format!(
            "curl-version-{}.log",
            crate::startup_process::unique()
        )),
        || repair_remaining(until).map(|_| ()),
    )?;
    if result.code != 0 || !compatible_curl(&result.stdout) {
        return Err(
            "Artifact provisioning requires Windows curl 8.4 or newer for bounded downloads."
                .into(),
        );
    }
    Ok(curl)
}

fn transfer_repair_until(
    uri: &str,
    partial: &Path,
    mut offset: u64,
    bytes: u64,
    working: &Path,
    until: Instant,
) -> crate::Result<()> {
    let curl = repair_download_tool(working, until)?;
    loop {
        let allowance = repair_process_allowance(until)?;
        let mut arguments = curl_arguments(uri, partial, offset, bytes);
        let position = arguments
            .iter()
            .position(|value| value == "--max-time")
            .ok_or("Missing download deadline argument.")?;
        arguments[position + 1] = allowance.as_secs_f64().to_string().into();
        let result = crate::startup_process::run_with_checkpoint(
            curl.as_os_str(),
            &arguments,
            working,
            &BTreeMap::new(),
            allowance,
            &working.join(format!("download-{}.log", crate::startup_process::unique())),
            || repair_remaining(until).map(|_| ()),
        )?;
        repair_remaining(until)?;
        if result.code == 33 && offset > 0 {
            // A server that ignores Range gets one fresh request within this
            // same absolute allowance; no partial responses are concatenated.
            repair_file(partial)?
                .set_len(0)
                .map_err(|error| error.to_string())?;
            offset = 0;
            continue;
        }
        if result.code != 0 {
            return Err(format!(
                "Pinned download failed ({}): {}; partial bytes retained at {}",
                result.code,
                result.stderr.trim(),
                partial.display()
            ));
        }
        return Ok(());
    }
}

enum CachedArtifact {
    Missing,
    Verified,
    Invalid(File),
}

fn cached_artifact(path: &Path, bytes: u64, sha256: &str) -> crate::Result<CachedArtifact> {
    repair_checkpoint()?;
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !is_redirect(&metadata) => {}
        Ok(_) => return Err("The model download location is not an ordinary file.".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(CachedArtifact::Missing);
        }
        Err(error) => return Err(format!("Cannot inspect the model download: {error}")),
    }
    let mut file = open_read(path).map_err(|error| {
        format!(
            "Cannot read the downloaded file at {}. Close other apps using it and check this folder's permissions: {error}",
            path.display()
        )
    })?;
    if file.metadata().map_err(|error| error.to_string())?.len() == bytes
        && digest_matches(&mut file, bytes, sha256)?
    {
        Ok(CachedArtifact::Verified)
    } else {
        // Only a successful read with a length/hash mismatch permits repair.
        // Access, cancellation and hashing I/O errors have already propagated.
        Ok(CachedArtifact::Invalid(file))
    }
}

fn repair_parents(path: &Path) -> crate::Result<Vec<File>> {
    #[cfg(windows)]
    let mut guards = Vec::new();
    #[cfg(not(windows))]
    let guards = Vec::new();
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        for parent in path.ancestors().skip(1) {
            let directory = OpenOptions::new()
                .read(true)
                .access_mode(0x81)
                .share_mode(3)
                .custom_flags(0x0220_0000)
                .open(parent)
                .map_err(|error| format!("Cannot protect the download folder: {error}"))?;
            let metadata = directory.metadata().map_err(|error| error.to_string())?;
            if !metadata.is_dir() || is_redirect(&metadata) {
                return Err("The download folder is redirected or is not a directory.".into());
            }
            guards.push(directory);
        }
    }
    #[cfg(not(windows))]
    let _ = path;
    Ok(guards)
}

fn repair_file(path: &Path) -> crate::Result<File> {
    repair_checkpoint()?;
    validate_external_path(path).map_err(|error| error.to_string())?;
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        // Retain read/write/delete authority and exclusive ownership from the
        // final hash through handle-based publication, without reopening a name.
        options
            .access_mode(0xc001_0000)
            .share_mode(0)
            .custom_flags(0x0820_0000);
    }
    let file = options
        .open(path)
        .map_err(|error| format!("Cannot open the partial download for verification: {error}"))?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file() || is_redirect(&metadata) {
        return Err("The partial download is not an ordinary file.".into());
    }
    Ok(file)
}

fn repair_partial_offset(path: &Path, bytes: u64) -> crate::Result<u64> {
    repair_checkpoint()?;
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !is_redirect(&metadata) => {
            let file = repair_file(path)?;
            let length = file.metadata().map_err(|error| error.to_string())?.len();
            if length > bytes {
                // A larger file cannot match the selected pin. Restart this
                // one owned partial instead of accumulating quarantine copies.
                repair_checkpoint()?;
                file.set_len(0).map_err(|error| error.to_string())?;
                Ok(0)
            } else {
                Ok(length)
            }
        }
        Ok(_) => Err("The partial download is not an ordinary file.".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(error.to_string()),
    }
}

fn repair_disk_required(bytes: u64, offset: u64) -> crate::Result<u64> {
    bytes
        .checked_sub(offset)
        .and_then(|remaining| remaining.checked_add(16 * 1024 * 1024))
        .ok_or_else(|| "Invalid partial download or disk-space requirement.".into())
}

fn require_repair_disk(partial: &Path, bytes: u64, offset: u64) -> crate::Result<()> {
    let required = repair_disk_required(bytes, offset)?;
    #[cfg(windows)]
    {
        let parent = partial
            .parent()
            .ok_or("Download has no parent directory.")?;
        let available = bonsai_windows_process::available_disk_bytes(parent)
            .map_err(|error| format!("Cannot check free space in the download folder: {error}"))?;
        if available < required {
            return Err(format!(
                "Not enough free disk space to finish setup. Free at least {} more bytes on the drive containing {} and select Retry setup. Existing model and partial download bytes were kept.",
                required - available,
                parent.display()
            ));
        }
    }
    #[cfg(not(windows))]
    let _ = (partial, required);
    Ok(())
}

fn verified_repair_partial(
    uri: &str,
    partial: &Path,
    bytes: u64,
    sha256: &str,
    working: &Path,
    mut transfer: impl FnMut(&str, &Path, u64, u64, &Path) -> crate::Result<()>,
    observe: &mut dyn FnMut(RepairActivity) -> crate::Result<()>,
) -> crate::Result<File> {
    let mut offset = repair_partial_offset(partial, bytes)?;
    let mut restart_resumed = offset > 0;
    loop {
        repair_checkpoint()?;
        if offset < bytes {
            // Free space already excludes the old file and any resumed bytes.
            // Publication renames this file, so no full-size copy is needed.
            require_repair_disk(partial, bytes, offset)?;
            observe(RepairActivity::Downloading { offset })?;
            transfer(uri, partial, offset, bytes, working).map_err(|error| {
                format!(
                    "Setup could not finish the download. Check the internet connection, free disk space and folder permissions, then select Retry setup. Partial downloads resume automatically. {error}"
                )
            })?;
        }
        observe(RepairActivity::VerifyingDownloaded)?;
        let mut completed = repair_file(partial)?;
        if completed
            .metadata()
            .map_err(|error| error.to_string())?
            .len()
            == bytes
            && digest_matches(&mut completed, bytes, sha256)?
        {
            return Ok(completed);
        }
        if !restart_resumed {
            return Err(format!(
                "The downloaded file failed verification. Your existing file was kept. Check the connection and select Retry setup; downloaded bytes remain at {}.",
                partial.display()
            ));
        }
        // Resume can expose an old corrupt prefix. Permit one fresh transfer;
        // a newly downloaded mismatch is never retried in an endless loop.
        repair_checkpoint()?;
        completed.set_len(0).map_err(|error| error.to_string())?;
        drop(completed);
        offset = 0;
        restart_resumed = false;
    }
}

fn publish_repair(file: &File, partial: &Path, path: &Path) -> crate::Result<()> {
    repair_checkpoint()?;
    validate_external_path(path).map_err(|error| error.to_string())?;
    #[cfg(windows)]
    let result = bonsai_windows_process::replace_open_file(file, path);
    #[cfg(not(windows))]
    let result = {
        // Production setup is Windows-only; keep tiny offline fixtures usable
        // on other hosts without adding an unsupported native launch path.
        let _ = file;
        fs::rename(partial, path)
    };
    #[cfg(windows)]
    let _ = partial;
    result.map_err(|error| {
        format!(
            "The replacement passed verification but could not be saved. Close other apps using the model, check free disk space and folder permissions, then select Retry setup. The existing file and completed download were kept: {error}"
        )
    })
}

// Call only while holding the destination's download lock and after verifying
// its final bytes. A leftover partial then has no resume or recovery purpose.
fn remove_redundant_download(path: &Path) -> crate::Result<()> {
    repair_checkpoint()?;
    validate_external_path(path).map_err(|error| error.to_string())?;
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !is_redirect(&metadata) => {}
        Ok(_) => {
            return Err(
                "Refusing to clean a download cache entry that is not an ordinary file.".into(),
            );
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
    }
    fs::remove_file(path).map_err(|error| {
        format!(
            "The artifact is verified, but its obsolete download cache at {} could not be removed. Close other apps using it and retry: {error}",
            path.display()
        )
    })
}

fn receive_repair_with(
    uri: &str,
    path: &Path,
    bytes: u64,
    sha256: &str,
    working: &Path,
    transfer: impl FnMut(&str, &Path, u64, u64, &Path) -> crate::Result<()>,
) -> crate::Result<()> {
    receive_repair_observed_with(uri, path, bytes, sha256, working, transfer, &mut |_| Ok(()))
}

fn receive_repair_observed_with(
    uri: &str,
    path: &Path,
    bytes: u64,
    sha256: &str,
    working: &Path,
    transfer: impl FnMut(&str, &Path, u64, u64, &Path) -> crate::Result<()>,
    observe: &mut dyn FnMut(RepairActivity) -> crate::Result<()>,
) -> crate::Result<()> {
    repair_checkpoint()?;
    expected(bytes, sha256)?;
    download_uri(uri)?;
    validate_external_path(path).map_err(|error| error.to_string())?;
    validate_external_path(working).map_err(|error| error.to_string())?;
    fs::create_dir_all(path.parent().ok_or("Artifact has no parent directory.")?)
        .map_err(|error| format!("Cannot create the model download folder: {error}"))?;
    fs::create_dir_all(working).map_err(|error| error.to_string())?;
    validate_external_path(path).map_err(|error| error.to_string())?;
    let _parents = repair_parents(path)?;
    let filename = path.file_name().ok_or("Artifact has no filename.")?;
    let lock = path.with_file_name(format!("{}.download.lock", filename.to_string_lossy()));
    let _guard = crate::startup_cache::lock_file(&lock)?;
    let partial = path.with_file_name(format!("{}.partial", filename.to_string_lossy()));
    observe(RepairActivity::VerifyingExisting)?;
    let original = match cached_artifact(path, bytes, sha256)? {
        CachedArtifact::Verified => return remove_redundant_download(&partial),
        CachedArtifact::Invalid(file) => Some(file),
        CachedArtifact::Missing => None,
    };
    validate_external_path(&partial).map_err(|error| error.to_string())?;
    let completed =
        verified_repair_partial(uri, &partial, bytes, sha256, working, transfer, observe)?;
    observe(RepairActivity::Publishing)?;
    // Windows denies replacing an open read-locked original. The verified new
    // handle and all destination ancestors remain owned during publication.
    drop(original);
    publish_repair(&completed, &partial, path)
}

/// Retains one aggregate transfer allowance across resume/restart attempts.
pub(crate) fn receive_with_timeout(
    uri: &str,
    path: &Path,
    bytes: u64,
    sha256: &str,
    working: &Path,
    timeout: Duration,
) -> crate::Result<()> {
    let started = Instant::now();
    receive_with(
        uri,
        path,
        bytes,
        sha256,
        working,
        |uri, partial, offset, bytes, working| {
            let remaining = || {
                crate::startup_cancel::checkpoint()?;
                timeout
                    .checked_sub(started.elapsed())
                    .filter(|value| !value.is_zero())
                    .ok_or_else(|| "Pinned download exceeded its aggregate allowance.".to_owned())
            };
            remaining()?;
            let curl = download_tool(working)?;
            let mut current_offset = offset;
            loop {
                let allowance = remaining()?;
                let mut arguments = curl_arguments(uri, partial, current_offset, bytes);
                let position = arguments
                    .iter()
                    .position(|value| value == "--max-time")
                    .ok_or("Missing download deadline argument.")?;
                arguments[position + 1] = allowance.as_secs_f64().to_string().into();
                let result = crate::startup_process::run_with_checkpoint(
                    curl.as_os_str(),
                    &arguments,
                    working,
                    &BTreeMap::new(),
                    allowance,
                    &working.join(format!("download-{}.log", crate::startup_process::unique())),
                    || remaining().map(|_| ()),
                )?;
                if result.code == 33 && current_offset > 0 {
                    validate_external_path(partial).map_err(|error| error.to_string())?;
                    OpenOptions::new()
                        .write(true)
                        .truncate(true)
                        .open(partial)
                        .map_err(|error| error.to_string())?;
                    current_offset = 0;
                    continue;
                }
                if result.code != 0 {
                    return Err(format!(
                        "Pinned download failed ({}): {}; partial bytes retained at {}",
                        result.code,
                        result.stderr.trim(),
                        partial.display()
                    ));
                }
                return Ok(());
            }
        },
    )
}

fn receive_with(
    uri: &str,
    path: &Path,
    bytes: u64,
    sha256: &str,
    working: &Path,
    mut transfer: impl FnMut(&str, &Path, u64, u64, &Path) -> crate::Result<()>,
) -> crate::Result<()> {
    crate::startup_cancel::checkpoint()?;
    expected(bytes, sha256)?;
    download_uri(uri)?;
    validate_external_path(path).map_err(|error| error.to_string())?;
    validate_external_path(working).map_err(|error| error.to_string())?;
    fs::create_dir_all(path.parent().ok_or("Artifact has no parent directory.")?)
        .map_err(|error| error.to_string())?;
    fs::create_dir_all(working).map_err(|error| error.to_string())?;
    let lock = path.with_file_name(format!(
        "{}.download.lock",
        path.file_name()
            .ok_or("Artifact has no filename.")?
            .to_string_lossy()
    ));
    let _guard = crate::startup_cache::lock_file(&lock)?;
    let partial = path.with_file_name(format!(
        "{}.partial",
        path.file_name()
            .ok_or("Artifact has no filename.")?
            .to_string_lossy()
    ));
    if path.exists() {
        verify(path, bytes, sha256)?;
        return remove_redundant_download(&partial);
    }
    validate_external_path(&partial).map_err(|error| error.to_string())?;
    let mut offset = match fs::symlink_metadata(&partial) {
        Ok(metadata) if metadata.is_file() && !is_redirect(&metadata) => metadata.len(),
        Ok(_) => return Err("Partial artifact is not an ordinary file.".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
        Err(error) => return Err(error.to_string()),
    };
    if offset > bytes {
        return Err("Partial artifact exceeds its locked length; bytes are retained.".into());
    }
    let mut quarantined: Option<PathBuf> = None;
    let completed = loop {
        crate::startup_cancel::checkpoint()?;
        if offset < bytes {
            transfer(uri, &partial, offset, bytes, working)?;
        }
        let mut completed = open_read(&partial)?;
        if digest_matches(&mut completed, bytes, sha256)? {
            break completed;
        }
        if let Some(quarantined) = &quarantined {
            return Err(format!(
                "Fresh artifact SHA-256 differs from its lock; invalid bytes retained at {} and {}.",
                partial.display(),
                quarantined.display()
            ));
        }
        let quarantine = partial.with_file_name(format!(
            ".bonsai-invalid-download-{}",
            crate::startup_process::unique()
        ));
        validate_external_path(&partial).map_err(|error| error.to_string())?;
        validate_external_path(&quarantine).map_err(|error| error.to_string())?;
        // Link exclusively while the checked read handle still denies writes.
        // A crash before unlinking only leaves another safe evidence alias.
        fs::hard_link(&partial, &quarantine)
            .map_err(|error| format!("Cannot preserve invalid download bytes: {error}"))?;
        drop(completed);
        fs::remove_file(&partial).map_err(|error| {
            format!(
                "Cannot restart partial download; invalid bytes retained at {}: {error}",
                quarantine.display()
            )
        })?;
        quarantined = Some(quarantine);
        offset = 0;
    };
    crate::startup_cancel::checkpoint()?;
    match fs::hard_link(&partial, path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            verify(path, bytes, sha256)?;
        }
        Err(error) => return Err(format!("Cannot publish verified artifact: {error}")),
    }
    drop(completed);
    remove_redundant_download(&partial)?;
    if let Some(quarantined) = quarantined {
        remove_redundant_download(&quarantined)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest as _, Sha256};
    use std::io::Read as _;

    fn recovery_fixture() -> crate::Result<(PathBuf, PathBuf, PathBuf, String)> {
        let working = std::env::temp_dir().join(format!(
            "Bonsai-DownloadRecovery-{}",
            crate::native_startup::nonce()?
        ));
        validate_external_path(&working).map_err(|error| error.to_string())?;
        fs::create_dir(&working).map_err(|error| error.to_string())?;
        let path = working.join("artifact.bin");
        let partial = working.join("artifact.bin.partial");
        fs::write(&partial, b"wrong").map_err(|error| error.to_string())?;
        let hash = crate::identity::hex(&Sha256::digest(b"right"));
        Ok((working, path, partial, hash))
    }

    fn quarantine_files(working: &Path) -> crate::Result<Vec<PathBuf>> {
        fs::read_dir(working)
            .map_err(|error| error.to_string())?
            .filter_map(|entry| match entry {
                Ok(entry)
                    if entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".bonsai-invalid-download-") =>
                {
                    Some(Ok(entry.path()))
                }
                Ok(_) => None,
                Err(error) => Some(Err(error.to_string())),
            })
            .collect()
    }

    #[test]
    fn setup_repair_observations_distinguish_local_hash_resume_and_fresh_repair()
    -> crate::Result<()> {
        use RepairActivity::{Downloading, Publishing, VerifyingDownloaded, VerifyingExisting};
        for case in 0..3 {
            let (working, path, partial, hash) = recovery_fixture()?;
            if case == 0 {
                fs::write(&path, b"right").map_err(|error| error.to_string())?;
            } else if case == 1 {
                fs::write(&partial, b"right").map_err(|error| error.to_string())?;
            } else {
                fs::write(&partial, b"wr").map_err(|error| error.to_string())?;
            }
            let mut observations = Vec::new();
            let mut transfers = Vec::new();
            receive_repair_observed_with(
                "https://example.invalid/artifact",
                &path,
                5,
                &hash,
                &working,
                |_, destination, offset, _, _| {
                    transfers.push(offset);
                    let content: &[u8] = if offset == 2 { b"wrong" } else { b"right" };
                    fs::write(destination, content).map_err(|error| error.to_string())
                },
                &mut |phase| {
                    observations.push(phase);
                    Ok(())
                },
            )?;
            let expected = match case {
                0 => vec![VerifyingExisting],
                1 => vec![VerifyingExisting, VerifyingDownloaded, Publishing],
                _ => vec![
                    VerifyingExisting,
                    Downloading { offset: 2 },
                    VerifyingDownloaded,
                    Downloading { offset: 0 },
                    VerifyingDownloaded,
                    Publishing,
                ],
            };
            assert_eq!(observations, expected);
            assert_eq!(transfers, if case == 2 { vec![2, 0] } else { Vec::new() });
            verify(&path, 5, &hash)?;
            fs::remove_dir_all(&working).map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    #[test]
    fn setup_repair_timeout_rejects_expired_work_and_restores_outer_deadline() -> crate::Result<()>
    {
        let (working, path, partial, hash) = recovery_fixture()?;
        fs::write(&path, b"old").map_err(|error| error.to_string())?;
        assert!(
            receive_repair_with_timeout(
                "https://example.invalid/artifact",
                &path,
                5,
                &hash,
                &working,
                Duration::ZERO,
            )
            .is_err()
        );
        assert_eq!(fs::read(&path).map_err(|error| error.to_string())?, b"old");
        assert_eq!(
            fs::read(&partial).map_err(|error| error.to_string())?,
            b"wrong"
        );
        assert_eq!(REPAIR_DEADLINE.with(Cell::get), None);
        with_repair_deadline(Duration::from_secs(60), |outer| {
            with_repair_deadline(Duration::from_secs(120), |inner| {
                assert_eq!(inner, outer);
                Ok(())
            })?;
            assert_eq!(REPAIR_DEADLINE.with(Cell::get), Some(outer));
            assert!(repair_process_allowance(Instant::now() + Duration::from_secs(4)).is_err());
            Ok(())
        })?;
        assert_eq!(REPAIR_DEADLINE.with(Cell::get), None);
        fs::write(&path, b"right").map_err(|error| error.to_string())?;
        receive_repair_with_timeout(
            "https://example.invalid/artifact",
            &path,
            5,
            &hash,
            &working,
            Duration::from_secs(30),
        )?;
        Ok(())
    }

    #[test]
    fn setup_repair_timeout_stops_before_hash_retry_or_publication() -> crate::Result<()> {
        for replacement in [b"wrong".as_slice(), b"right"] {
            let (working, path, partial, hash) = recovery_fixture()?;
            fs::write(&path, b"old").map_err(|error| error.to_string())?;
            fs::write(&partial, b"wr").map_err(|error| error.to_string())?;
            let mut transfers = 0;
            let result = with_repair_deadline(Duration::from_secs(60), |_| {
                receive_repair_with(
                    "https://example.invalid/artifact",
                    &path,
                    5,
                    &hash,
                    &working,
                    |_, destination, offset, _, _| {
                        transfers += 1;
                        assert_eq!(offset, 2);
                        fs::write(destination, replacement).map_err(|error| error.to_string())?;
                        // Simulate the transfer consuming all its allowance
                        // without depending on sleeps or scheduler timing.
                        REPAIR_DEADLINE.with(|deadline| deadline.set(Some(Instant::now())));
                        Ok(())
                    },
                )
            });
            assert!(result.is_err());
            assert_eq!(transfers, 1);
            assert_eq!(fs::read(&path).map_err(|error| error.to_string())?, b"old");
            assert_eq!(
                fs::read(&partial).map_err(|error| error.to_string())?,
                replacement
            );
            assert_eq!(REPAIR_DEADLINE.with(Cell::get), None);
        }
        Ok(())
    }

    #[test]
    fn setup_repair_timeout_preserves_cancellation_and_verified_cache() -> crate::Result<()> {
        use std::sync::{Arc, atomic::AtomicBool};
        let (working, path, partial, hash) = recovery_fixture()?;
        fs::write(&path, b"right").map_err(|error| error.to_string())?;
        let cancelled = Arc::new(AtomicBool::new(true));
        crate::startup_cancel::with_peer_cancellation(&cancelled, || {
            assert!(
                receive_repair_with_timeout(
                    "https://example.invalid/artifact",
                    &path,
                    5,
                    &hash,
                    &working,
                    Duration::from_secs(60),
                )
                .is_err()
            );
        });
        assert_eq!(
            fs::read(&path).map_err(|error| error.to_string())?,
            b"right"
        );
        assert_eq!(
            fs::read(&partial).map_err(|error| error.to_string())?,
            b"wrong"
        );
        assert_eq!(REPAIR_DEADLINE.with(Cell::get), None);
        Ok(())
    }

    #[test]
    fn setup_repair_reuses_verified_files_without_downloading() -> crate::Result<()> {
        let (working, path, partial, hash) = recovery_fixture()?;
        fs::write(&path, b"right").map_err(|error| error.to_string())?;
        receive_repair_with(
            "https://example.invalid/artifact",
            &path,
            5,
            &hash,
            &working,
            |_, _, _, _, _| Err("Verified model must not trigger a download.".into()),
        )?;
        assert_eq!(
            fs::read(&path).map_err(|error| error.to_string())?,
            b"right"
        );
        assert_eq!(
            fs::read(&partial).map_err(|error| error.to_string())?,
            b"wrong"
        );
        Ok(())
    }

    #[test]
    fn setup_repair_keeps_invalid_original_until_verified_replacement() -> crate::Result<()> {
        for previous in [b"bad".as_slice(), b"wrong"] {
            let (working, path, partial, hash) = recovery_fixture()?;
            fs::remove_file(&partial).map_err(|error| error.to_string())?;
            fs::write(&path, previous).map_err(|error| error.to_string())?;
            let mut transfers = 0;
            receive_repair_with(
                "https://example.invalid/artifact",
                &path,
                5,
                &hash,
                &working,
                |_, destination, offset, bytes, _| {
                    transfers += 1;
                    assert_eq!((offset, bytes), (0, 5));
                    assert_eq!(
                        fs::read(&path).map_err(|error| error.to_string())?,
                        previous
                    );
                    fs::write(destination, b"right").map_err(|error| error.to_string())
                },
            )?;
            assert_eq!(transfers, 1);
            verify(&path, 5, &hash)?;
            assert!(!partial.exists());
            assert_eq!(quarantine_files(&working)?, Vec::<PathBuf>::new());
        }
        Ok(())
    }

    #[test]
    fn setup_repair_retains_original_and_partial_on_transfer_or_identity_failure()
    -> crate::Result<()> {
        for fail_transfer in [true, false] {
            let (working, path, partial, hash) = recovery_fixture()?;
            fs::remove_file(&partial).map_err(|error| error.to_string())?;
            fs::write(&path, b"old").map_err(|error| error.to_string())?;
            let mut transfers = 0;
            assert!(
                receive_repair_with(
                    "https://example.invalid/artifact",
                    &path,
                    5,
                    &hash,
                    &working,
                    |_, destination, _, _, _| {
                        transfers += 1;
                        fs::write(destination, b"wrong").map_err(|error| error.to_string())?;
                        if fail_transfer {
                            Err("Synthetic interrupted connection.".into())
                        } else {
                            Ok(())
                        }
                    },
                )
                .is_err()
            );
            assert_eq!(transfers, 1);
            assert_eq!(fs::read(&path).map_err(|error| error.to_string())?, b"old");
            assert_eq!(
                fs::read(&partial).map_err(|error| error.to_string())?,
                b"wrong"
            );
            assert_eq!(quarantine_files(&working)?, Vec::<PathBuf>::new());
        }
        Ok(())
    }

    #[test]
    fn setup_repair_resumes_partial_then_restarts_corrupt_prefix_once() -> crate::Result<()> {
        let (working, path, partial, hash) = recovery_fixture()?;
        fs::write(&partial, b"wr").map_err(|error| error.to_string())?;
        let mut offsets = Vec::new();
        receive_repair_with(
            "https://example.invalid/artifact",
            &path,
            5,
            &hash,
            &working,
            |_, destination, offset, _, _| {
                offsets.push(offset);
                let content: &[u8] = if offset == 2 { b"wrong" } else { b"right" };
                fs::write(destination, content).map_err(|error| error.to_string())
            },
        )?;
        assert_eq!(offsets, [2, 0]);
        verify(&path, 5, &hash)?;
        assert!(!partial.exists());
        assert_eq!(quarantine_files(&working)?, Vec::<PathBuf>::new());
        Ok(())
    }

    #[test]
    fn setup_repair_complete_corrupt_partial_retries_once_then_stops() -> crate::Result<()> {
        let (working, path, partial, hash) = recovery_fixture()?;
        fs::write(&path, b"old").map_err(|error| error.to_string())?;
        let mut transfers = 0;
        assert!(
            receive_repair_with(
                "https://example.invalid/artifact",
                &path,
                5,
                &hash,
                &working,
                |_, destination, offset, _, _| {
                    transfers += 1;
                    assert_eq!(offset, 0);
                    fs::write(destination, b"again").map_err(|error| error.to_string())
                },
            )
            .is_err()
        );
        assert_eq!(transfers, 1);
        assert_eq!(fs::read(&path).map_err(|error| error.to_string())?, b"old");
        assert_eq!(
            fs::read(&partial).map_err(|error| error.to_string())?,
            b"again"
        );
        assert_eq!(quarantine_files(&working)?, Vec::<PathBuf>::new());
        Ok(())
    }

    #[test]
    fn setup_repair_handles_oversized_and_complete_partials_without_extra_copies()
    -> crate::Result<()> {
        for previous in [b"too long".as_slice(), b"right"] {
            let (working, path, partial, hash) = recovery_fixture()?;
            fs::write(&partial, previous).map_err(|error| error.to_string())?;
            let mut transfers = 0;
            receive_repair_with(
                "https://example.invalid/artifact",
                &path,
                5,
                &hash,
                &working,
                |_, destination, offset, _, _| {
                    transfers += 1;
                    assert_eq!(offset, 0);
                    assert_eq!(
                        fs::metadata(destination)
                            .map_err(|error| error.to_string())?
                            .len(),
                        0
                    );
                    fs::write(destination, b"right").map_err(|error| error.to_string())
                },
            )?;
            assert_eq!(transfers, usize::from(previous.len() > 5));
            verify(&path, 5, &hash)?;
            assert!(!partial.exists());
            assert_eq!(quarantine_files(&working)?, Vec::<PathBuf>::new());
        }
        assert_eq!(repair_disk_required(11, 7)?, 16 * 1024 * 1024 + 4);
        assert!(repair_disk_required(5, 6).is_err());
        Ok(())
    }

    #[test]
    fn setup_repair_rejects_nonfiles_and_download_lock_contention() -> crate::Result<()> {
        let (working, path, _, hash) = recovery_fixture()?;
        fs::create_dir(&path).map_err(|error| error.to_string())?;
        let mut transfers = 0;
        assert!(
            receive_repair_with(
                "https://example.invalid/artifact",
                &path,
                5,
                &hash,
                &working,
                |_, _, _, _, _| {
                    transfers += 1;
                    Ok(())
                },
            )
            .is_err()
        );
        assert!(path.is_dir());
        fs::remove_dir(&path).map_err(|error| error.to_string())?;
        let _guard = crate::startup_cache::lock_file(&working.join("artifact.bin.download.lock"))?;
        assert!(
            receive_repair_with(
                "https://example.invalid/artifact",
                &path,
                5,
                &hash,
                &working,
                |_, _, _, _, _| {
                    transfers += 1;
                    Ok(())
                },
            )
            .is_err()
        );
        assert_eq!(transfers, 0);
        assert!(!path.exists());
        Ok(())
    }

    #[test]
    fn setup_repair_rejects_unsafe_paths_and_cancellation_without_mutation() -> crate::Result<()> {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        let (working, path, partial, hash) = recovery_fixture()?;
        fs::write(&path, b"old").map_err(|error| error.to_string())?;
        let cancelled = Arc::new(AtomicBool::new(true));
        let mut transfers = 0;
        crate::startup_cancel::with_peer_cancellation(&cancelled, || {
            assert!(
                receive_repair_with(
                    "https://example.invalid/artifact",
                    &path,
                    5,
                    &hash,
                    &working,
                    |_, _, _, _, _| {
                        transfers += 1;
                        Ok(())
                    },
                )
                .is_err()
            );
        });
        cancelled.store(false, Ordering::Release);
        fs::remove_file(&partial).map_err(|error| error.to_string())?;
        crate::startup_cancel::with_peer_cancellation(&cancelled, || {
            assert!(
                receive_repair_with(
                    "https://example.invalid/artifact",
                    &path,
                    5,
                    &hash,
                    &working,
                    |_, destination, _, _, _| {
                        transfers += 1;
                        fs::write(destination, b"right").map_err(|error| error.to_string())?;
                        cancelled.store(true, Ordering::Release);
                        Ok(())
                    },
                )
                .is_err()
            );
        });
        assert_eq!(transfers, 1);
        assert_eq!(fs::read(&path).map_err(|error| error.to_string())?, b"old");
        assert_eq!(
            fs::read(&partial).map_err(|error| error.to_string())?,
            b"right"
        );
        assert!(
            receive_repair_with(
                "https://example.invalid/artifact",
                Path::new("relative-model.bin"),
                5,
                &hash,
                &working,
                |_, _, _, _, _| Err("Unsafe paths must not download.".into()),
            )
            .is_err()
        );
        fs::create_dir(working.join(".git")).map_err(|error| error.to_string())?;
        assert!(
            receive_repair_with(
                "https://example.invalid/artifact",
                &path,
                5,
                &hash,
                &working,
                |_, _, _, _, _| Err("Git storage must not download.".into()),
            )
            .is_err()
        );
        assert_eq!(fs::read(&path).map_err(|error| error.to_string())?, b"old");
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn setup_repair_rejects_writer_owned_files_without_download() -> crate::Result<()> {
        use std::os::windows::fs::OpenOptionsExt as _;
        let (working, path, _, hash) = recovery_fixture()?;
        fs::write(&path, b"old").map_err(|error| error.to_string())?;
        let writer = OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(0)
            .open(&path)
            .map_err(|error| error.to_string())?;
        let mut transfers = 0;
        assert!(
            receive_repair_with(
                "https://example.invalid/artifact",
                &path,
                5,
                &hash,
                &working,
                |_, _, _, _, _| {
                    transfers += 1;
                    Ok(())
                },
            )
            .is_err()
        );
        assert_eq!(transfers, 0);
        drop(writer);
        assert_eq!(fs::read(&path).map_err(|error| error.to_string())?, b"old");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn setup_repair_rejects_redirected_files_and_ancestors() -> crate::Result<()> {
        use std::os::unix::fs::symlink;
        let (working, path, _, hash) = recovery_fixture()?;
        let actual = working.join("actual.bin");
        fs::write(&actual, b"old").map_err(|error| error.to_string())?;
        symlink(&actual, &path).map_err(|error| error.to_string())?;
        let alias = working.join("redirected-folder");
        symlink(&working, &alias).map_err(|error| error.to_string())?;
        for redirected in [path, alias.join("actual.bin")] {
            assert!(
                receive_repair_with(
                    "https://example.invalid/artifact",
                    &redirected,
                    5,
                    &hash,
                    &working,
                    |_, _, _, _, _| Err("Redirected paths must not download.".into()),
                )
                .is_err()
            );
        }
        assert_eq!(fs::read(actual).map_err(|error| error.to_string())?, b"old");
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn setup_repair_keeps_completed_download_when_existing_reader_blocks_publication()
    -> crate::Result<()> {
        let (working, path, partial, hash) = recovery_fixture()?;
        fs::write(&path, b"old").map_err(|error| error.to_string())?;
        fs::write(&partial, b"right").map_err(|error| error.to_string())?;
        let reader = open_read(&path)?;
        assert!(
            receive_repair_with(
                "https://example.invalid/artifact",
                &path,
                5,
                &hash,
                &working,
                |_, _, _, _, _| Err("Completed verified bytes must not download again.".into()),
            )
            .is_err()
        );
        assert_eq!(fs::read(&path).map_err(|error| error.to_string())?, b"old");
        verify(&partial, 5, &hash)?;
        drop(reader);
        receive_repair_with(
            "https://example.invalid/artifact",
            &path,
            5,
            &hash,
            &working,
            |_, _, _, _, _| Err("Retry must reuse the completed verified download.".into()),
        )?;
        verify(&path, 5, &hash)?;
        assert!(!partial.exists());
        Ok(())
    }

    #[test]
    fn download_cache_cleanup_discards_partial_only_after_verified_success() -> crate::Result<()> {
        for repair in [false, true] {
            let (working, path, partial, hash) = recovery_fixture()?;
            fs::write(&path, b"wrong").map_err(|error| error.to_string())?;
            if !repair {
                assert!(
                    receive_with(
                        "https://example.invalid/artifact",
                        &path,
                        5,
                        &hash,
                        &working,
                        |_, _, _, _, _| Err("Existing corruption must be reported.".into()),
                    )
                    .is_err()
                );
                assert_eq!(
                    fs::read(&partial).map_err(|error| error.to_string())?,
                    b"wrong"
                );
            }
            fs::write(&path, b"right").map_err(|error| error.to_string())?;
            let transfer = |_: &str, _: &Path, _: u64, _: u64, _: &Path| {
                Err("Verified cache hits must not download.".into())
            };
            let lock = working.join("artifact.bin.download.lock");
            let guard = crate::startup_cache::lock_file(&lock)?;
            assert!(
                receive_with(
                    "https://example.invalid/artifact",
                    &path,
                    5,
                    &hash,
                    &working,
                    transfer
                )
                .is_err()
            );
            assert!(partial.is_file());
            drop(guard);
            if repair {
                receive_repair_with(
                    "https://example.invalid/artifact",
                    &path,
                    5,
                    &hash,
                    &working,
                    transfer,
                )?;
            } else {
                receive_with(
                    "https://example.invalid/artifact",
                    &path,
                    5,
                    &hash,
                    &working,
                    transfer,
                )?;
            }
            assert!(!partial.exists());
            verify(&path, 5, &hash)?;
            assert!(lock.is_file());
            drop(crate::startup_cache::lock_file(&lock)?);
            // A directory at the reserved name is never recursively deleted.
            fs::create_dir(&partial).map_err(|error| error.to_string())?;
            assert!(
                receive_with(
                    "https://example.invalid/artifact",
                    &path,
                    5,
                    &hash,
                    &working,
                    transfer
                )
                .is_err()
            );
            assert!(partial.is_dir());
            fs::remove_dir_all(working).map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    #[test]
    fn download_cache_cleanup_removes_quarantine_after_fresh_retry_publishes() -> crate::Result<()>
    {
        let (working, path, partial, hash) = recovery_fixture()?;
        let mut transfers = 0;
        receive_with(
            "https://example.invalid/artifact",
            &path,
            5,
            &hash,
            &working,
            |_, destination, offset, bytes, _| {
                transfers += 1;
                assert_eq!((offset, bytes), (0, 5));
                assert_eq!(destination, partial);
                assert!(!destination.exists());
                fs::write(destination, b"right").map_err(|error| error.to_string())
            },
        )?;
        assert_eq!(transfers, 1);
        verify(&path, 5, &hash)?;
        assert!(!partial.exists());
        assert!(quarantine_files(&working)?.is_empty());
        fs::remove_dir_all(working).map_err(|error| error.to_string())?;
        Ok(())
    }

    #[test]
    fn corrupt_fresh_retry_stops_without_publishing_or_replacing_existing_artifacts()
    -> crate::Result<()> {
        let (working, path, partial, hash) = recovery_fixture()?;
        let mut transfers = 0;
        assert!(
            receive_with(
                "https://example.invalid/artifact",
                &path,
                5,
                &hash,
                &working,
                |_, destination, offset, _, _| {
                    transfers += 1;
                    assert_eq!(offset, 0);
                    fs::write(destination, b"again").map_err(|error| error.to_string())
                },
            )
            .is_err()
        );
        assert_eq!(transfers, 1);
        assert!(!path.exists());
        assert_eq!(
            fs::read(&partial).map_err(|error| error.to_string())?,
            b"again"
        );
        let evidence = quarantine_files(&working)?;
        assert_eq!(evidence.len(), 1);
        assert_eq!(
            fs::read(evidence.first().ok_or("Missing quarantined download.")?)
                .map_err(|error| error.to_string())?,
            b"wrong"
        );
        fs::write(&path, b"final").map_err(|error| error.to_string())?;
        assert!(
            receive_with(
                "https://example.invalid/artifact",
                &path,
                5,
                &hash,
                &working,
                |_, _, _, _, _| {
                    transfers += 1;
                    Err("Published artifacts must never trigger a replacement transfer.".into())
                },
            )
            .is_err()
        );
        assert_eq!(transfers, 1);
        assert_eq!(fs::read(path).map_err(|error| error.to_string())?, b"final");
        assert_eq!(quarantine_files(&working)?, evidence);
        fs::remove_dir_all(working).map_err(|error| error.to_string())?;
        Ok(())
    }

    #[test]
    fn locked_download_contract_rejects_unsafe_urls_and_bounds() {
        for uri in [
            "http://example.com/file",
            "https://user@example.com/file",
            "https://example.com/a b",
            "https://",
        ] {
            assert!(download_uri(uri).is_err());
        }
        assert!(download_uri("https://codeload.github.com/owner/repo/zip/revision").is_ok());
        assert!(expected(0, &"0".repeat(64)).is_err());
        assert!(expected(1, "bad").is_err());
        assert!(compatible_curl("curl 8.4.0 (Windows) libcurl/8.4.0"));
        assert!(compatible_curl("curl 9.0.0"));
        assert!(!compatible_curl("curl 8.3.1"));
        assert!(!compatible_curl("unknown"));
        let arguments = curl_arguments("https://example.com/file", Path::new("partial"), 7, 11);
        assert!(
            arguments
                .array_windows()
                .any(|pair| *pair == ["--max-filesize", "4"])
        );
        assert!(
            arguments
                .array_windows()
                .any(|pair| *pair == ["--continue-at", "7"])
        );
        assert!(
            arguments
                .array_windows()
                .any(|pair| *pair == ["--proto-redir", "=https"])
        );
    }

    #[test]
    fn cached_artifacts_reject_corruption_and_retain_oversized_partials() -> crate::Result<()> {
        let working = std::env::temp_dir().join(format!(
            "Bonsai-DownloadTest-{}",
            crate::native_startup::nonce()?
        ));
        validate_external_path(&working).map_err(|error| error.to_string())?;
        fs::create_dir(&working).map_err(|error| error.to_string())?;
        let path = working.join("artifact.bin");
        let hash = crate::identity::hex(&Sha256::digest(b"hello"));
        fs::write(&path, b"hello").map_err(|error| error.to_string())?;
        receive(
            "https://example.invalid/artifact",
            &path,
            5,
            &hash,
            &working,
        )?;
        assert!(working.join("artifact.bin.download.lock").is_file());
        fs::write(&path, b"jello").map_err(|error| error.to_string())?;
        verify_length(&path, 5)?;
        assert!(verify(&path, 5, &hash).is_err());
        assert!(
            receive(
                "https://example.invalid/artifact",
                &path,
                5,
                &hash,
                &working
            )
            .is_err()
        );
        assert_eq!(
            fs::read(&path).map_err(|error| error.to_string())?,
            b"jello"
        );
        let missing = working.join("missing.bin");
        let partial = working.join("missing.bin.partial");
        fs::write(&partial, b"too long").map_err(|error| error.to_string())?;
        assert!(
            receive(
                "https://example.invalid/artifact",
                &missing,
                5,
                &hash,
                &working
            )
            .is_err()
        );
        assert!(!missing.exists());
        assert_eq!(
            fs::read(&partial).map_err(|error| error.to_string())?,
            b"too long"
        );
        fs::remove_dir_all(working).map_err(|error| error.to_string())?;
        Ok(())
    }

    #[cfg(windows)]
    fn range_request(stream: &mut std::net::TcpStream) -> crate::Result<String> {
        // Windows accepts can retain the listener's nonblocking mode. A read
        // timeout only bounds blocking reads; explicitly restore that mode.
        stream
            .set_nonblocking(false)
            .map_err(|error| error.to_string())?;
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .map_err(|error| error.to_string())?;
        stream
            .set_write_timeout(Some(Duration::from_secs(3)))
            .map_err(|error| error.to_string())?;
        let mut request = [0; 4096];
        let mut count = 0;
        while !request[..count].ends_with(b"\r\n\r\n") {
            if count == request.len() {
                return Err("Range fixture request headers exceeded their bound.".into());
            }
            match stream.read(&mut request[count..]) {
                Ok(0) => return Err("Range fixture request headers ended early.".into()),
                Ok(read) => count += read,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error.to_string()),
            }
        }
        String::from_utf8(request[..count].to_vec()).map_err(|error| error.to_string())
    }

    #[test]
    fn range_fixture_waits_for_fragmented_request_headers() -> crate::Result<()> {
        use std::{io::Write as _, net::TcpListener, thread};
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|error| error.to_string())?;
        let mut client =
            std::net::TcpStream::connect(listener.local_addr().map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())?;
        let (mut accepted, _) = listener.accept().map_err(|error| error.to_string())?;
        accepted
            .set_nonblocking(true)
            .map_err(|error| error.to_string())?;
        let server = thread::spawn(move || range_request(&mut accepted));
        for fragment in [
            b"GET /artifact HTTP/1.1\r\nRan".as_slice(),
            b"ge: bytes=5-\r\n\r\n",
        ] {
            thread::sleep(Duration::from_millis(20));
            client
                .write_all(fragment)
                .map_err(|error| error.to_string())?;
        }
        let request = server.join().map_err(|_| "Range fixture panicked.")??;
        assert_eq!(request, "GET /artifact HTTP/1.1\r\nRange: bytes=5-\r\n\r\n");
        Ok(())
    }

    #[test]
    fn curl_resumes_with_a_bound_on_remaining_response_bytes() -> crate::Result<()> {
        use std::{io::Write as _, net::TcpListener, thread, time::Instant};
        let working = std::env::temp_dir().join(format!(
            "Bonsai-RangeTest-{}",
            crate::native_startup::nonce()?
        ));
        validate_external_path(&working).map_err(|error| error.to_string())?;
        fs::create_dir(&working).map_err(|error| error.to_string())?;
        let partial = working.join("artifact.partial");
        fs::write(&partial, b"hello").map_err(|error| error.to_string())?;
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|error| error.to_string())?;
        let port = listener
            .local_addr()
            .map_err(|error| error.to_string())?
            .port();
        listener
            .set_nonblocking(true)
            .map_err(|error| error.to_string())?;
        let server = thread::spawn(move || -> crate::Result<String> {
            let deadline = Instant::now() + Duration::from_secs(10);
            while Instant::now() < deadline {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let request = range_request(&mut stream)?;
                        stream.write_all(b"HTTP/1.1 206 Partial Content\r\nContent-Length: 5\r\nContent-Range: bytes 5-9/10\r\nConnection: close\r\n\r\nworld").map_err(|error| error.to_string())?;
                        return Ok(request);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(error) => return Err(error.to_string()),
                }
            }
            Err("Range fixture received no request.".into())
        });
        let uri = format!("http://127.0.0.1:{port}/artifact");
        let mut arguments = curl_arguments(&uri, &partial, 5, 10);
        // Isolate curl's resume semantics on loopback. Production arguments
        // remain HTTPS-only and the receiver rejects this fixture's URL.
        for argument in &mut arguments {
            if argument == "=https" {
                *argument = "=http".into();
            }
        }
        let output = crate::startup_process::run(
            system_tool("curl.exe")?.as_os_str(),
            &arguments,
            &working,
            &BTreeMap::new(),
            Duration::from_secs(10),
            &working.join("range.log"),
        )?;
        let request = server.join().map_err(|_| "Range fixture panicked.")??;
        assert_eq!(output.code, 0, "{}", output.stderr);
        assert!(request.to_ascii_lowercase().contains("range: bytes=5-"));
        assert_eq!(
            fs::read(&partial).map_err(|error| error.to_string())?,
            b"helloworld"
        );
        Ok(())
    }
}
