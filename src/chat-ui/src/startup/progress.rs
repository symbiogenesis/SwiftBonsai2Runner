//! Bounded setup observations read by the preparation worker, never the UI loop.
use std::{
    fs::File,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

/// Availability of the selected model at the beginning of this setup attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Kind {
    FirstRun,
    Existing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Activity {
    Checking,
    Downloading,
    Verifying,
    Preparing,
    Ready,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Status {
    pub(crate) message: String,
    pub(crate) kind: Option<Kind>,
    pub(crate) activity: Activity,
    pub(crate) downloaded: Option<u64>,
    pub(crate) expected: Option<u64>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
struct Report {
    schema_version: u32,
    stage: String,
    message: String,
    kind: Option<Kind>,
    activity: Option<Activity>,
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
    let bytes = crate::bounded_file::read(File::open(path).ok()?, 8192).ok()??;
    let report: Report = serde_json::from_slice(&bytes).ok()?;
    status(report, data)
}

fn status(report: Report, data: &Path) -> Option<Status> {
    if !matches!(report.schema_version, 1 | 2)
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
    // Legacy producers only report an intention. Neither their message nor a
    // leftover partial establishes that a network transfer is running now.
    let (activity, message, kind) = if report.schema_version == 1 {
        let (activity, message) = match report.stage.as_str() {
            "runtime" => (Activity::Preparing, "Preparing the runtime…"),
            "ready" => (Activity::Ready, "Starting your local model…"),
            "adapter" => (Activity::Checking, "Preparing model adapter files…"),
            _ => (Activity::Checking, "Preparing Swift model files…"),
        };
        (activity, message.to_owned(), None)
    } else {
        (report.activity?, report.message, report.kind)
    };
    let mut result = Status {
        message,
        kind,
        activity,
        downloaded: None,
        expected: None,
    };
    match (report.artifact_path, report.expected_bytes) {
        (Some(path), Some(expected)) => {
            if expected == 0 || path == data || !path.starts_with(data) {
                return None;
            }
            bonsai_windows_credentials::validate_external_path(&path).ok()?;
            if activity == Activity::Downloading {
                let filename = path.file_name()?.to_str()?;
                let partial = path.with_file_name(format!("{filename}.partial"));
                bonsai_windows_credentials::validate_external_path(&partial).ok()?;
                let downloaded = match std::fs::metadata(partial) {
                    Ok(metadata) if metadata.is_file() => metadata.len().min(expected),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
                    _ => return None,
                };
                result.downloaded = Some(downloaded);
                result.expected = Some(expected);
            }
        }
        (None, None) if activity != Activity::Downloading => {}
        _ => return None,
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_progress_tracks_explicit_activity_and_never_mistakes_cached_partials_for_transfers()
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
        let mut report = serde_json::json!({
            "SchemaVersion":2,"Stage":"model","Kind":"existing",
            "Activity":"verifying","Message":"Verifying local Swift model…",
            "ArtifactPath":model,"ExpectedBytes":10
        });
        std::fs::write(&model, [0; 10])?;
        std::fs::write(&partial, [0; 4])?;
        let read = |report: &serde_json::Value| -> std::io::Result<Status> {
            std::fs::write(&path, report.to_string())?;
            read_status(&data, &data).ok_or_else(|| std::io::Error::other("Missing progress"))
        };
        let local = read(&report)?;
        assert_eq!(local.kind, Some(Kind::Existing));
        assert_eq!(local.activity, Activity::Verifying);
        assert_eq!((local.downloaded, local.expected), (None, None));
        assert_eq!(local.message, "Verifying local Swift model…");

        report["Activity"] = "downloading".into();
        report["Message"] = "Resuming Swift model download…".into();
        let downloading = read(&report)?;
        assert_eq!(downloading.activity, Activity::Downloading);
        assert_eq!(
            (downloading.downloaded, downloading.expected),
            (Some(4), Some(10))
        );

        std::fs::write(&partial, [0; 10])?;
        report["Activity"] = "verifying".into();
        report["Message"] = "Verifying downloaded Swift model…".into();
        let verifying = read(&report)?;
        assert_eq!(verifying.activity, Activity::Verifying);
        assert_eq!((verifying.downloaded, verifying.expected), (None, None));
        assert_eq!(verifying.message, "Verifying downloaded Swift model…");

        report["SchemaVersion"] = 1.into();
        if let Some(value) = report.as_object_mut() {
            value.remove("Activity");
            value.remove("Kind");
        }
        report["Message"] = "Downloading Swift model…".into();
        let legacy = read(&report)?;
        assert_eq!(legacy.kind, None);
        assert_eq!(legacy.activity, Activity::Checking);
        assert_eq!((legacy.downloaded, legacy.expected), (None, None));
        assert!(!legacy.message.contains("Downloading"));
        std::fs::write(&path, "{".repeat(8193))?;
        assert!(read_status(&data, &data).is_none());
        std::fs::remove_dir_all(data)?;
        Ok(())
    }

    #[test]
    fn setup_progress_rejects_unbounded_unknown_and_external_artifacts() {
        let data = std::env::temp_dir().join("Bonsai-Progress");
        let make = || Report {
            schema_version: 2,
            stage: "model".into(),
            message: "Checking Swift model…".into(),
            kind: Some(Kind::FirstRun),
            activity: Some(Activity::Checking),
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
        let mut missing_activity = make();
        missing_activity.activity = None;
        assert!(status(missing_activity, &data).is_none());
        let mut missing_artifact = make();
        missing_artifact.activity = Some(Activity::Downloading);
        assert!(status(missing_artifact, &data).is_none());
        let mut missing_length = make();
        missing_length.artifact_path = Some(data.join("model.gguf"));
        assert!(status(missing_length, &data).is_none());
    }
}
