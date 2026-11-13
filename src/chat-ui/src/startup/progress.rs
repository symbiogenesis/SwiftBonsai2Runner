//! Bounded setup observations read by the preparation worker, never the UI loop.
use std::{
    fs::File,
    io::Read as _,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Status {
    pub(crate) message: String,
    pub(crate) downloaded: Option<u64>,
    pub(crate) expected: Option<u64>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
struct Report {
    schema_version: u32,
    stage: String,
    message: String,
    artifact_path: Option<PathBuf>,
    expected_bytes: Option<u64>,
}

#[derive(Clone, Default)]
pub(super) struct Shared(Arc<Mutex<Option<Status>>>);

impl Shared {
    pub(super) fn observe(&self, working: &Path, data: &Path) {
        if let Some(status) = read_status(working, data)
            && let Ok(mut latest) = self.0.lock()
        {
            *latest = Some(status);
        }
    }

    pub(super) fn latest(&self) -> Option<Status> {
        self.0.try_lock().ok().and_then(|status| status.clone())
    }
}

fn read_status(working: &Path, data: &Path) -> Option<Status> {
    let path = working.join("chat-setup-progress.json");
    bonsai_windows_credentials::validate_external_path(&path).ok()?;
    let mut bytes = Vec::new();
    File::open(path)
        .ok()?
        .take(8193)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > 8192 {
        return None;
    }
    let report: Report = serde_json::from_slice(&bytes).ok()?;
    status(report, data)
}

fn status(report: Report, data: &Path) -> Option<Status> {
    if report.schema_version != 1
        || !matches!(
            report.stage.as_str(),
            "model" | "adapter" | "runtime" | "ready"
        )
        || report.message.is_empty()
        || report.message.len() > 512
        || report.message.chars().any(char::is_control)
    {
        return None;
    }
    let mut result = Status {
        message: report.message,
        downloaded: None,
        expected: None,
    };
    if let (Some(path), Some(expected)) = (report.artifact_path, report.expected_bytes) {
        if expected == 0 || path == data || !path.starts_with(data) {
            return None;
        }
        bonsai_windows_credentials::validate_external_path(&path).ok()?;
        let filename = path.file_name()?.to_str()?;
        let partial = path.with_file_name(format!("{filename}.partial"));
        bonsai_windows_credentials::validate_external_path(&partial).ok()?;
        if let Ok(metadata) = std::fs::metadata(partial)
            && metadata.is_file()
        {
            result.downloaded = Some(metadata.len().min(expected));
            result.expected = Some(expected);
            result.message = if metadata.len() >= expected {
                "Verifying downloaded files…".into()
            } else {
                "Downloading files for Swift…".into()
            };
        }
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_progress_rejects_unbounded_unknown_and_external_artifacts() {
        let data = std::env::temp_dir().join("Bonsai-Progress");
        let make = || Report {
            schema_version: 1,
            stage: "model".into(),
            message: "Checking Swift model…".into(),
            artifact_path: None,
            expected_bytes: None,
        };
        assert!(status(make(), &data).is_some());
        let mut unknown = make();
        unknown.stage = "unreviewed".into();
        assert!(status(unknown, &data).is_none());
        let mut long = make();
        long.message = "x".repeat(513);
        assert!(status(long, &data).is_none());
        let mut external = make();
        external.artifact_path = Some(std::env::temp_dir().join("other.bin"));
        external.expected_bytes = Some(10);
        assert!(status(external, &data).is_none());
    }
}
