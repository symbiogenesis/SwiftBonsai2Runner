//! Confirmed portable replacement from a freshly verified staged application.

use std::{
    ffi::OsString,
    fs::{self, OpenOptions},
    io,
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
        [command, rest @ ..] if command == "bundled-chat" => rest,
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
    temporary: &Path,
    target: &Path,
    previous_sha256: &str,
    mut rename: impl FnMut(&Path, &Path) -> io::Result<()>,
    mut retry: impl FnMut(&io::Error) -> bool,
) -> crate::Result<()> {
    loop {
        // Release the read lock before the atomic rename; Windows must be able
        // to replace this image once the original launcher has exited.
        original(target, previous_sha256)?;
        external(temporary)?;
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
    rename: impl FnMut(&Path, &Path) -> io::Result<()>,
    retry: impl FnMut(&io::Error) -> bool,
) -> crate::Result<()> {
    original(target, previous_sha256)?;
    let temporary = target.with_file_name(format!(
        ".SwiftBonsai-update-{}.exe",
        crate::startup_process::unique()
    ));
    external(&temporary)?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        options.share_mode(0).custom_flags(0x0020_0000);
    }
    let mut destination = options
        .open(&temporary)
        .map_err(|error| error.to_string())?;
    let temporary = Temporary(temporary);
    let copied = (|| {
        let mut input =
            bonsai_artifacts::open_locked_file(source.path()).map_err(|error| error.to_string())?;
        if io::copy(&mut input, &mut destination).map_err(|error| error.to_string())? != asset.bytes
        {
            return Err("The portable replacement copy has an unexpected length.".into());
        }
        destination.sync_all().map_err(|error| error.to_string())
    })();
    // Close the exclusive writer even when copying fails, before Temporary
    // removes the unpublished sibling on Windows.
    drop(destination);
    copied?;
    // The staged source remains locked, but the sibling's verification handle
    // must close before Windows can rename that sibling over the old image.
    drop(
        VerifiedFile::verify(&temporary.0, asset.bytes, &asset.sha256)
            .map_err(|error| error.to_string())?,
    );
    publish(&temporary.0, target, previous_sha256, rename, retry)
}

pub(super) fn replace(request: Replacement) -> crate::Result<Value> {
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
    validate_restart_arguments(&request)?;
    let directory = super::staged(&request.version)?;
    let _lock = crate::startup_cache::lock_file(&directory.join("update.lock"))?;
    let candidate = super::published_portable_release(&request.version, &directory)?;
    let published = super::verified_portable_release(&candidate, &directory)?;
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
        |from, to| fs::rename(from, to),
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
    command
        .args(request.arguments)
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
    let process = command.spawn().map_err(|error| {
        format!("The portable executable was updated, but restarting it failed: {error}")
    })?;
    Ok(json!({"Status": "updated", "Version": request.version, "ProcessId": process.id()}))
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

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
                .map(|entries| entries.count())
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
            |from, to| fs::rename(from, to),
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
                |from, to| fs::rename(from, to),
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
            ..asset.clone()
        };
        assert!(
            copy_and_replace(
                &source,
                &incomplete,
                &fixture.0.join("application.exe"),
                &crate::identity::sha256(b"old executable"),
                |from, to| fs::rename(from, to),
                |_| false,
            )
            .is_err()
        );
        assert_eq!(fixture.entries()?, 2);
        Ok(())
    }
}
