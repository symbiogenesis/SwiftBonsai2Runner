//! Confirmed portable replacement from a freshly verified staged application.

use std::{
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{self, Read as _, Seek as _},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use bonsai_artifacts::VerifiedFile;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{MAX_INSTALLER, external, policy::Asset};

const REPLACE_ALLOWANCE: Duration = Duration::from_secs(15);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Replacement {
    pub(super) target: PathBuf,
    pub(super) previous_sha256: String,
    pub(super) version: String,
    pub(super) arguments: Vec<String>,
    // Published v0.2.1 launchers do not supply this field when upgrading to a
    // newer helper. New launchers always bind their confirmed release identity.
    #[serde(default)]
    pub(super) release_identity: Option<String>,
    // Older launchers always reopened the application after replacement.
    #[serde(default = "super::restart_by_default")]
    pub(super) restart: bool,
}

struct Temporary(PathBuf);

impl Drop for Temporary {
    fn drop(&mut self) {
        // This unique sibling was created exclusively by this operation. Never
        // follow a redirect introduced at its former name after publication.
        if external(&self.0).is_ok()
            && fs::symlink_metadata(&self.0).is_ok_and(|metadata| {
                metadata.is_file() && !bonsai_artifacts::is_redirect(&metadata)
            })
        {
            let _removed = fs::remove_file(&self.0);
        }
    }
}

fn original(target: &Path, expected: &str) -> crate::Result<()> {
    external(target)?;
    let metadata = fs::symlink_metadata(target).map_err(|error| error.to_string())?;
    if !metadata.is_file()
        || bonsai_artifacts::is_redirect(&metadata)
        || metadata.len() == 0
        || metadata.len() > MAX_INSTALLER
        || !crate::identity::is_lower_hex(expected, 64)
        || crate::startup_cache::digest_file(target)? != expected
    {
        return Err("The original portable executable changed; no replacement was made.".into());
    }
    Ok(())
}

fn validate_restart_arguments(request: &Replacement) -> crate::Result<()> {
    let arguments: Vec<_> = request.arguments.iter().map(OsString::from).collect();
    let options = match arguments.as_slice() {
        [command, rest @ ..] if command == "bundled-chat" || command == "updated-chat" => rest,
        values => values,
    };
    if options
        .iter()
        .any(|argument| argument == "--installation-directory")
    {
        return Err("Portable restart cannot override its package cache directory.".into());
    }
    let mut validation = vec![
        OsString::from("--installation-directory"),
        request
            .target
            .parent()
            .ok_or("Portable replacement requires a parent directory.")?
            .as_os_str()
            .to_owned(),
    ];
    validation.extend_from_slice(options);
    crate::cli::installed_chat(&validation)?;
    Ok(())
}

fn publish(
    temporary: &File,
    target: &Path,
    previous_sha256: &str,
    mut rename: impl FnMut(&File, &Path) -> io::Result<()>,
    mut retry: impl FnMut(&io::Error) -> bool,
) -> crate::Result<()> {
    loop {
        // Release the read lock before the atomic rename; Windows must be able
        // to replace this image once the original launcher has exited.
        original(target, previous_sha256)?;
        match rename(temporary, target) {
            Ok(()) => return Ok(()),
            Err(error) if retry(&error) => {}
            Err(error) => {
                return Err(format!(
                    "Could not replace the portable executable; the original was retained: {error}"
                ));
            }
        }
    }
}

fn copy_and_replace(
    source: &VerifiedFile,
    asset: &Asset,
    target: &Path,
    previous_sha256: &str,
    rename: impl FnMut(&File, &Path) -> io::Result<()>,
    retry: impl FnMut(&io::Error) -> bool,
) -> crate::Result<()> {
    original(target, previous_sha256)?;
    let _parents = hold_parents(target)?;
    let temporary = target.with_file_name(format!(
        ".SwiftBonsai-update-{}.exe",
        crate::startup_process::unique()
    ));
    external(&temporary)?;
    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        // DELETE authority permits publication through this same handle. No
        // other opener may write, delete or swap the verified sibling.
        options
            .access_mode(0xc001_0000)
            .share_mode(0)
            .custom_flags(0x0020_0000);
    }
    let mut destination = options
        .open(&temporary)
        .map_err(|error| error.to_string())?;
    let _temporary = Temporary(temporary);
    let copied = (|| {
        let mut input =
            bonsai_artifacts::open_locked_file(source.path()).map_err(|error| error.to_string())?;
        if io::copy(&mut input, &mut destination).map_err(|error| error.to_string())? != asset.bytes
        {
            return Err("The portable replacement copy has an unexpected length.".into());
        }
        destination.sync_all().map_err(|error| error.to_string())
    })();
    let result = copied.and_then(|()| {
        destination.rewind().map_err(|error| error.to_string())?;
        let mut hasher = crate::identity::Hasher::new()?;
        let mut buffer = vec![0; 1024 * 1024];
        loop {
            let count = destination
                .read(&mut buffer)
                .map_err(|error| error.to_string())?;
            if count == 0 {
                break;
            }
            hasher.update(&buffer[..count])?;
        }
        if hasher.finalize_hex()? != asset.sha256 {
            return Err("The portable replacement copy failed SHA-256 verification.".into());
        }
        publish(&destination, target, previous_sha256, rename, retry)
    });
    // Keep exclusive byte/path ownership through native handle-based rename;
    // close before removing an unpublished temporary file on failure.
    drop(destination);
    result
}

pub(super) fn hold_parents(path: &Path) -> crate::Result<Vec<File>> {
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
                .map_err(|error| error.to_string())?;
            let metadata = directory.metadata().map_err(|error| error.to_string())?;
            if !metadata.is_dir() || bonsai_artifacts::is_redirect(&metadata) {
                return Err(
                    "Portable replacement parent is redirected or is not a directory.".into(),
                );
            }
            guards.push(directory);
        }
    }
    #[cfg(not(windows))]
    {
        let _ = path;
    }
    Ok(guards)
}

pub(super) fn replace(request: &Replacement) -> crate::Result<Value> {
    if !cfg!(all(windows, target_arch = "aarch64")) {
        return Err("Portable application replacement requires native Windows ARM64.".into());
    }
    if request.version != env!("CARGO_PKG_VERSION")
        || request
            .target
            .extension()
            .is_none_or(|extension| !extension.eq_ignore_ascii_case("exe"))
    {
        return Err(
            "The replacement helper does not match the confirmed executable version.".into(),
        );
    }
    original(&request.target, &request.previous_sha256)?;
    validate_restart_arguments(request)?;
    let directory = super::staged(&request.version)?;
    let _lock = crate::startup_cache::lock_file(&directory.join("update.lock"))?;
    let candidate = super::published_portable_release(&request.version, &directory)?;
    let published = super::verified_portable_release(&candidate, &directory)?;
    if let Some(identity) = &request.release_identity {
        published.verify_release_identity(identity)?;
    }
    let source = super::verify_stage(&directory, &request.version, &published)?;
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let canonical_source = fs::canonicalize(source.path()).map_err(|error| error.to_string())?;
    if fs::canonicalize(&executable).map_err(|error| error.to_string())? != canonical_source
        || fs::canonicalize(&request.target).map_err(|error| error.to_string())? == canonical_source
    {
        return Err("Portable replacement must run from the verified staged release.".into());
    }
    let deadline = Instant::now() + REPLACE_ALLOWANCE;
    copy_and_replace(
        &source,
        &published.asset,
        &request.target,
        &request.previous_sha256,
        replace_open_file,
        |error| {
            if !matches!(error.raw_os_error(), Some(5 | 32 | 33)) || Instant::now() >= deadline {
                return false;
            }
            thread::sleep(
                Duration::from_millis(100).min(deadline.saturating_duration_since(Instant::now())),
            );
            Instant::now() < deadline
        },
    )?;
    let mut command = Command::new(&request.target);
    let arguments = match request.arguments.as_slice() {
        [command, rest @ ..] if command == "bundled-chat" || command == "updated-chat" => rest,
        values => values,
    };
    command
        .arg("updated-chat")
        .args(arguments)
        .current_dir(
            request
                .target
                .parent()
                .ok_or("Missing executable parent.")?,
        )
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        command.creation_flags(0x0800_0000);
    }
    report_restart(request, || command.spawn().map(|process| process.id()))
}

fn report_restart(
    request: &Replacement,
    spawn: impl FnOnce() -> io::Result<u32>,
) -> crate::Result<Value> {
    if !request.restart {
        return Ok(json!({"Status": "updated", "Version": request.version}));
    }
    let process_id = spawn().map_err(|error| {
        format!(
            "The portable executable was updated, but restarting it failed: {error}. Open {} manually to start the updated app. Your data directory was preserved.",
            request.target.display()
        )
    })?;
    Ok(json!({"Status": "updated", "Version": request.version, "ProcessId": process_id}))
}

fn replace_open_file(file: &File, target: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        bonsai_windows_process::replace_open_file(file, target)
    }
    #[cfg(not(windows))]
    {
        let _ = (file, target);
        Err(io::Error::other("Portable replacement requires Windows."))
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn replacement_request_keeps_published_launcher_compatibility_and_new_confirmation_identity()
    -> crate::Result<()> {
        let legacy = json!({
            "target": std::env::temp_dir().join("portable.exe"),
            "previousSha256": "a".repeat(64), "version": env!("CARGO_PKG_VERSION"),
            "arguments": ["bundled-chat", "--no-speech-models"],
        });
        let request: Replacement =
            serde_json::from_value(legacy.clone()).map_err(|error| error.to_string())?;
        assert!(request.release_identity.is_none());
        assert!(request.restart);
        let mut bound = legacy;
        bound["releaseIdentity"] = "b".repeat(64).into();
        bound["restart"] = false.into();
        let request: Replacement =
            serde_json::from_value(bound).map_err(|error| error.to_string())?;
        assert_eq!(
            request.release_identity.as_deref(),
            Some("b".repeat(64).as_str())
        );
        assert!(!request.restart);
        let result = report_restart(&request, || {
            Err(io::Error::other(
                "A deferred close must never start a new application process",
            ))
        })?;
        assert_eq!(result["Status"], "updated");
        assert!(result.get("ProcessId").is_none());
        Ok(())
    }

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> crate::Result<Self> {
            let path = std::env::temp_dir().join(format!(
                "Bonsai-Portable-Replacement-{}",
                crate::startup_process::unique()
            ));
            external(&path)?;
            fs::create_dir(&path).map_err(|error| error.to_string())?;
            Ok(Self(path))
        }

        fn source(&self) -> crate::Result<(VerifiedFile, Asset)> {
            let path = self.0.join("new.exe");
            fs::write(&path, b"new executable").map_err(|error| error.to_string())?;
            let asset = Asset {
                name: "new.exe".into(),
                bytes: 14,
                sha256: crate::identity::sha256(b"new executable"),
                url: String::new(),
            };
            let verified = VerifiedFile::verify(&path, asset.bytes, &asset.sha256)
                .map_err(|error| error.to_string())?;
            Ok((verified, asset))
        }

        fn target(&self) -> crate::Result<PathBuf> {
            let path = self.0.join("application.exe");
            fs::write(&path, b"old executable").map_err(|error| error.to_string())?;
            Ok(path)
        }

        fn entries(&self) -> crate::Result<usize> {
            fs::read_dir(&self.0)
                .map(Iterator::count)
                .map_err(|error| error.to_string())
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _removed = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn portable_replacement_publishes_verified_bytes_and_rejects_changed_original()
    -> crate::Result<()> {
        let fixture = Fixture::new()?;
        let (source, asset) = fixture.source()?;
        let target = fixture.target()?;
        let previous = crate::identity::sha256(b"old executable");
        copy_and_replace(
            &source,
            &asset,
            &target,
            &previous,
            replace_open_file,
            |_| false,
        )?;
        assert_eq!(
            fs::read(&target).map_err(|error| error.to_string())?,
            b"new executable"
        );
        assert_eq!(fixture.entries()?, 2);
        assert!(
            copy_and_replace(
                &source,
                &asset,
                &target,
                &previous,
                replace_open_file,
                |_| false,
            )
            .is_err()
        );
        assert_eq!(fixture.entries()?, 2);
        assert_eq!(
            fs::read(target).map_err(|error| error.to_string())?,
            b"new executable"
        );
        Ok(())
    }

    #[test]
    fn portable_replacement_exhausted_retry_keeps_original_and_cleans_copy() -> crate::Result<()> {
        let fixture = Fixture::new()?;
        let (source, asset) = fixture.source()?;
        let target = fixture.target()?;
        let mut attempts = 0;
        let mut retries = 0;
        let error = copy_and_replace(
            &source,
            &asset,
            &target,
            &crate::identity::sha256(b"old executable"),
            |_, _| {
                attempts += 1;
                Err(io::Error::from_raw_os_error(32))
            },
            |_| {
                retries += 1;
                retries < 2
            },
        )
        .err()
        .ok_or("Accepted a failed atomic replacement.")?;
        assert!(error.contains("original was retained"));
        assert_eq!(attempts, 2);
        assert_eq!(retries, 2);
        assert_eq!(fixture.entries()?, 2);
        assert_eq!(
            fs::read(target).map_err(|error| error.to_string())?,
            b"old executable"
        );
        let incomplete = Asset {
            bytes: asset.bytes + 1,
            ..asset
        };
        assert!(
            copy_and_replace(
                &source,
                &incomplete,
                &fixture.0.join("application.exe"),
                &crate::identity::sha256(b"old executable"),
                replace_open_file,
                |_| false,
            )
            .is_err()
        );
        assert_eq!(fixture.entries()?, 2);
        Ok(())
    }

    #[test]
    fn portable_replacement_rechecks_original_between_sharing_retries() -> crate::Result<()> {
        let fixture = Fixture::new()?;
        let (source, asset) = fixture.source()?;
        let target = fixture.target()?;
        let mut attempts = 0;
        let error = copy_and_replace(
            &source,
            &asset,
            &target,
            &crate::identity::sha256(b"old executable"),
            |_, destination| {
                attempts += 1;
                fs::write(destination, b"changed externally")?;
                Err(io::Error::from_raw_os_error(32))
            },
            |_| true,
        )
        .err()
        .ok_or("Overwrote an original changed between retries.")?;
        assert!(error.contains("original portable executable changed"));
        assert_eq!(attempts, 1);
        assert_eq!(
            fs::read(&target).map_err(|error| error.to_string())?,
            b"changed externally"
        );
        assert_eq!(fixture.entries()?, 2);
        Ok(())
    }

    #[test]
    fn read_only_portable_target_is_retained_without_leftover_replacement() -> crate::Result<()> {
        let fixture = Fixture::new()?;
        let (source, asset) = fixture.source()?;
        let target = fixture.target()?;
        let permissions = fs::metadata(&target)
            .map_err(|error| error.to_string())?
            .permissions();
        let mut read_only = permissions.clone();
        read_only.set_readonly(true);
        fs::set_permissions(&target, read_only).map_err(|error| error.to_string())?;
        let result = copy_and_replace(
            &source,
            &asset,
            &target,
            &crate::identity::sha256(b"old executable"),
            replace_open_file,
            |_| false,
        );
        fs::set_permissions(&target, permissions).map_err(|error| error.to_string())?;
        assert!(result.is_err());
        assert_eq!(
            fs::read(&target).map_err(|error| error.to_string())?,
            b"old executable"
        );
        assert_eq!(fixture.entries()?, 2);
        Ok(())
    }

    #[test]
    fn corrupt_copy_never_reaches_publication_or_touches_user_data() -> crate::Result<()> {
        let fixture = Fixture::new()?;
        let (source, mut asset) = fixture.source()?;
        let target = fixture.target()?;
        let data = fixture.0.join("chat-recovery.dpapi");
        fs::write(&data, b"saved conversation bytes").map_err(|error| error.to_string())?;
        asset.sha256 = "0".repeat(64);
        let mut published = false;
        assert!(
            copy_and_replace(
                &source,
                &asset,
                &target,
                &crate::identity::sha256(b"old executable"),
                |_, _| {
                    published = true;
                    Ok(())
                },
                |_| false,
            )
            .is_err()
        );
        assert!(!published);
        assert_eq!(
            fs::read(&target).map_err(|error| error.to_string())?,
            b"old executable"
        );
        assert_eq!(
            fs::read(&data).map_err(|error| error.to_string())?,
            b"saved conversation bytes"
        );
        assert_eq!(fixture.entries()?, 3);
        Ok(())
    }

    #[test]
    fn restart_failure_preserves_verified_update_and_original_launch_options() -> crate::Result<()>
    {
        let fixture = Fixture::new()?;
        let (source, asset) = fixture.source()?;
        let target = fixture.target()?;
        let data = fixture.0.join("user-data");
        fs::create_dir(&data).map_err(|error| error.to_string())?;
        fs::write(data.join("model.bin"), b"existing model").map_err(|error| error.to_string())?;
        fs::write(data.join("chat-recovery.dpapi"), b"existing conversations")
            .map_err(|error| error.to_string())?;
        let previous = crate::identity::sha256(b"old executable");
        copy_and_replace(
            &source,
            &asset,
            &target,
            &previous,
            replace_open_file,
            |_| false,
        )?;
        let request = Replacement {
            target: target.clone(),
            previous_sha256: previous,
            version: env!("CARGO_PKG_VERSION").into(),
            arguments: vec![
                "bundled-chat".into(),
                "--data-directory".into(),
                data.to_string_lossy().into_owned(),
                "--no-speech-models".into(),
            ],
            release_identity: None,
            restart: true,
        };
        let arguments = request.arguments.clone();
        validate_restart_arguments(&request)?;
        let error = report_restart(&request, || Err(io::Error::from_raw_os_error(5)))
            .err()
            .ok_or("Accepted a failed restart.")?;
        assert!(error.contains("updated, but restarting it failed"));
        assert!(error.contains("manually"));
        assert_eq!(request.arguments, arguments);
        assert_eq!(
            fs::read(&target).map_err(|error| error.to_string())?,
            b"new executable"
        );
        assert_eq!(
            fs::read(data.join("model.bin")).map_err(|error| error.to_string())?,
            b"existing model"
        );
        assert_eq!(
            fs::read(data.join("chat-recovery.dpapi")).map_err(|error| error.to_string())?,
            b"existing conversations"
        );
        Ok(())
    }
}
