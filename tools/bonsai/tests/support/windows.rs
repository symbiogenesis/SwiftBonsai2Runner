//! Shared Windows shell and process identity setup for supervision fixtures.
#![expect(
    clippy::redundant_pub_crate,
    reason = "Shared fixture helpers stay visible only inside their owning integration test."
)]

use std::path::Path;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub(crate) fn caller_identity(_directory: &Path) -> Result<u64> {
    Ok(bonsai_windows_process::current_process_creation_time()?)
}
