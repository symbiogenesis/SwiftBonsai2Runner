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
            } else if result.message.starts_with("Checking") {
                "Checking files; a partial download is available to resume…".into()
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
    fn setup_progress_reads_resumable_bytes_without_calling_complete_a_verified_model()
    -> std::io::Result<()> {
        let data = std::env::temp_dir().join(format!(
            "Bonsai-SetupProgress-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(std::io::Error::other)?
                .as_nanos()
        ));
        std::fs::create_dir(&data)?;
        let model = data.join("model.gguf");
        let partial = data.join("model.gguf.partial");
        let path = data.join("chat-setup-progress.json");
        let report = serde_json::json!({"SchemaVersion":1,"Stage":"model","Message":"Downloading Swift model…","ArtifactPath":model,"ExpectedBytes":10});
        std::fs::write(&path, report.to_string())?;
        std::fs::write(&partial, [0; 4])?;
        let observed =
            read_status(&data, &data).ok_or_else(|| std::io::Error::other("Missing progress"))?;
        assert_eq!(
            (observed.downloaded, observed.expected),
            (Some(4), Some(10))
        );
        std::fs::write(&partial, [0; 10])?;
        let complete = read_status(&data, &data)
            .ok_or_else(|| std::io::Error::other("Missing complete progress"))?;
        assert_eq!(complete.message, "Verifying downloaded files…");
        assert!(
            !model.exists(),
            "Progress must never publish or qualify a model"
        );
        std::fs::write(&path, "{".repeat(8193))?;
        assert!(read_status(&data, &data).is_none());
        std::fs::remove_dir_all(data)?;
        Ok(())
    }

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
