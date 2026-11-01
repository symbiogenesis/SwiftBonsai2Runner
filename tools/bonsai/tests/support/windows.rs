//! Shared Windows shell and process identity setup for supervision fixtures.
#![expect(
    clippy::redundant_pub_crate,
    reason = "Shared fixture helpers stay visible only inside their owning integration test."
)]

use std::path::{Path, PathBuf};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub(crate) fn shell() -> Result<PathBuf> {
    Ok(
        PathBuf::from(std::env::var_os("SystemRoot").ok_or("Missing Windows system root.")?)
            .join("System32/WindowsPowerShell/v1.0/powershell.exe"),
    )
}

pub(crate) fn caller_identity(_directory: &Path) -> Result<u64> {
    Ok(bonsai_windows_process::current_process_creation_time()?)
}
