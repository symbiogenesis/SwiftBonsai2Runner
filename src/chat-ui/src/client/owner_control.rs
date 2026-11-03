//! One private shutdown request and model-cleanup acknowledgment per attempt.

use bonsai_windows_credentials::{PrivateDirectory, validate_external_path, validate_read_path};
use bonsai_windows_process::OwnedProcess;
use std::{
    fs::{self, OpenOptions},
    io,
    path::Path,
    thread,
    time::{Duration, Instant},
};

const REQUEST: &str = "shutdown.request";
const CONFIRMED: &str = "model-cleanup.confirmed";
const OUTER_STOP_RESERVE: Duration = Duration::from_secs(3);

pub(super) struct OwnerControl {
    directory: PrivateDirectory,
}

impl OwnerControl {
    pub(super) fn create(parent: &Path) -> io::Result<Self> {
        PrivateDirectory::create(parent).map(|directory| Self { directory })
    }

    pub(super) fn path(&self) -> &Path {
        self.directory.path()
    }

    pub(super) fn stop(&self, broker: &OwnedProcess, deadline: Instant) -> io::Result<()> {
        // The PSEC model may be brokered outside the UI's tool Job. Ask its
        // broker to run explicit model cleanup before terminating the outer Job.
        let graceful = self.request().and_then(|()| {
            let confirmation_deadline = deadline
                .checked_sub(OUTER_STOP_RESERVE)
                .ok_or_else(expired)?;
            self.wait_for_confirmation(broker, confirmation_deadline)
        });
        // Even on absent acknowledgment, stop the outer broker tree within the
        // original deadline. Unknown PSEC release remains terminal, never Retry.
        finish_shutdown(graceful, deadline, |remaining| broker.stop(remaining))
    }

    fn request(&self) -> io::Result<()> {
        let path = self.path().join(REQUEST);
        validate_external_path(&path)?;
        let _request = OpenOptions::new().write(true).create_new(true).open(path)?;
        Ok(())
    }

    fn wait_for_confirmation(&self, broker: &OwnedProcess, deadline: Instant) -> io::Result<()> {
        loop {
            if broker.try_wait()?.is_some() {
                return self.confirmed();
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(expired());
            }
            thread::sleep(remaining.min(super::IO_POLL));
        }
    }

    fn confirmed(&self) -> io::Result<()> {
        let path = self.path().join(CONFIRMED);
        validate_read_path(&path)?;
        if fs::metadata(path)?.len() != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid private model cleanup acknowledgment.",
            ));
        }
        Ok(())
    }
}

fn finish_shutdown(
    graceful: io::Result<()>,
    deadline: Instant,
    stop: impl FnOnce(Duration) -> io::Result<()>,
) -> io::Result<()> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(expired());
    }
    let outer = stop(remaining);
    graceful?;
    outer?;
    if Instant::now() >= deadline {
        return Err(expired());
    }
    Ok(())
}

fn expired() -> io::Error {
    io::Error::new(
        io::ErrorKind::TimedOut,
        "Owned model cleanup was not confirmed.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owned_broker_cleanup_acknowledgment_requires_exact_fresh_empty_file() -> io::Result<()> {
        let parent: std::path::PathBuf = std::env::temp_dir().components().collect();
        let control = OwnerControl::create(&parent)?;
        let path = control.path().join(CONFIRMED);
        assert!(control.confirmed().is_err());
        fs::write(&path, b"unconfirmed")?;
        assert!(control.confirmed().is_err());
        fs::write(&path, [])?;
        control.confirmed()?;
        let other = OwnerControl::create(&parent)?;
        assert!(other.confirmed().is_err());
        assert_ne!(other.path(), control.path());
        fs::remove_file(&path)?;
        fs::create_dir(path)?;
        assert!(control.confirmed().is_err());
        Ok(())
    }

    #[test]
    fn owned_broker_signal_failure_still_stops_outer_tree_inside_original_deadline() {
        for kind in [io::ErrorKind::PermissionDenied, io::ErrorKind::NotFound] {
            let mut allowance = Duration::ZERO;
            let result = finish_shutdown(
                Err(io::Error::from(kind)),
                Instant::now() + Duration::from_secs(1),
                |remaining| {
                    allowance = remaining;
                    Ok(())
                },
            );
            assert_eq!(result.err().map(|error| error.kind()), Some(kind));
            assert!(!allowance.is_zero() && allowance <= Duration::from_secs(1));
        }
        let mut stopped = false;
        let result = finish_shutdown(Ok(()), Instant::now(), |_| {
            stopped = true;
            Ok(())
        });
        assert_eq!(
            result.err().map(|error| error.kind()),
            Some(io::ErrorKind::TimedOut)
        );
        assert!(!stopped);
    }
}
