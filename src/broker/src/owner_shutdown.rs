//! Private file cancellation for one native application's owned broker attempt.
//!
//! The caller creates a fresh ACL-protected private directory and retains its
//! directory handle. Fixed empty files carry only cancellation and confirmed
//! model cleanup; neither is a public service endpoint or credential.

use crate::launcher::LaunchError;
use bonsai_windows_credentials::{validate_external_path, validate_read_path};
use std::{
    ffi::OsString,
    fs::{self, OpenOptions},
    future::{pending, poll_fn},
    io,
    path::{Path, PathBuf},
    pin::pin,
    task::Poll,
    time::Duration,
};

const OPTION: &str = "--owner-shutdown-directory";
const REQUEST: &str = "shutdown.request";
const CONFIRMED: &str = "model-cleanup.confirmed";

/// Fixed private cancellation storage for one broker invocation.
///
/// The owner controls this directory. Validation does not claim isolation from
/// a hostile same-user process or mutable ancestor replacement.
pub struct OwnerShutdown {
    directory: PathBuf,
}

impl OwnerShutdown {
    /// Extract the optional private owner argument before ordinary launch parsing.
    ///
    /// # Errors
    /// Rejects repeated/missing options, unsafe paths, preexisting cleanup
    /// acknowledgments and unexpected files. An early empty request is accepted.
    pub fn take_arguments(args: Vec<OsString>) -> io::Result<(Vec<OsString>, Option<Self>)> {
        let mut remaining = Vec::with_capacity(args.len());
        let mut directory = None;
        let mut args = args.into_iter();
        while let Some(argument) = args.next() {
            if argument == OPTION {
                if directory.is_some() {
                    return Err(invalid());
                }
                directory = Some(PathBuf::from(args.next().ok_or_else(invalid)?));
            } else {
                remaining.push(argument);
            }
        }
        let control = directory.map(Self::open).transpose()?;
        Ok((remaining, control))
    }

    fn open(directory: PathBuf) -> io::Result<Self> {
        validate_external_path(&directory)?;
        if !directory.is_dir() {
            return Err(invalid());
        }
        for entry in fs::read_dir(&directory)? {
            if entry?.file_name() != REQUEST {
                return Err(invalid());
            }
            // Cancellation can race the child's first instruction. A request
            // created after spawn but before intake must remain observable.
            empty_signal(&directory.join(REQUEST))?;
        }
        Ok(Self { directory })
    }

    async fn requested(&self) -> io::Result<()> {
        loop {
            if empty_signal(&self.directory.join(REQUEST))? {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// Atomically acknowledge that no model was created or its Job stop succeeded.
    ///
    /// Call only after the launcher's explicit model cleanup proof. Outer broker
    /// Job accounting alone cannot prove release of a brokered PSEC model.
    ///
    /// # Errors
    /// Rejects unsafe storage, an existing acknowledgment or failed file creation.
    pub fn confirm_cleanup(&self) -> io::Result<()> {
        let path = self.directory.join(CONFIRMED);
        validate_external_path(&path)?;
        let _confirmation = OpenOptions::new().write(true).create_new(true).open(path)?;
        Ok(())
    }
}

/// Observe the same private cancellation and console signal across startup/serving.
///
/// # Errors
/// Returns console registration or private signal inspection failures. Callers
/// still complete explicit model cleanup before reporting any acknowledgment.
pub async fn shutdown(control: Option<&OwnerShutdown>) -> io::Result<()> {
    let mut console = pin!(tokio::signal::ctrl_c());
    let mut owner = pin!(async {
        match control {
            Some(control) => control.requested().await,
            None => pending().await,
        }
    });
    poll_fn(|cx| {
        if let Poll::Ready(result) = owner.as_mut().poll(cx) {
            return Poll::Ready(result);
        }
        console.as_mut().poll(cx)
    })
    .await
}

/// Whether a completed startup error establishes that no model Job remains live.
///
/// Native process creation errors and failed explicit stops remain unknown.
/// Every later startup failure explicitly stops its returned model owner.
#[must_use]
pub const fn startup_cleanup_confirmed(error: LaunchError) -> bool {
    matches!(
        error,
        LaunchError::Arguments
            | LaunchError::Path
            | LaunchError::Policy
            | LaunchError::Artifacts
            | LaunchError::NativeRuntime
            | LaunchError::Adapter
            | LaunchError::Credentials
            | LaunchError::Memory
            | LaunchError::Port
            | LaunchError::SandboxUnavailable
            | LaunchError::SandboxEnvironment
            | LaunchError::Backend
            | LaunchError::Cancelled
            | LaunchError::Service
    )
}

fn empty_signal(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            validate_read_path(path)?;
            if metadata.len() != 0 {
                return Err(invalid());
            }
            Ok(true)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "Invalid private owner control.",
    )
}

#[cfg(test)]
mod tests;
