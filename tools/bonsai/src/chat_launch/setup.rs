//! First-run Swift artifacts bound to the same immutable policy as the broker.

use super::{Request, external};
use serde_json::{Value, json};
use std::{
    fs,
    io::Write as _,
    path::{Path, PathBuf},
};

const SWIFT: &str = include_str!("../../../../config/swift-assessment.lock.json");
const ADAPTER: &str = include_str!("../../../../config/swift-lora.lock.json");
const PTQ1: &str = include_str!("../../../../config/swift.ptq1-export.json");

struct Model {
    path: PathBuf,
    bytes: u64,
    sha256: String,
    url: Option<String>,
}

fn text<'a>(value: &'a Value, key: &str) -> crate::Result<&'a str> {
    value[key]
        .as_str()
        .ok_or_else(|| format!("Missing compiled Swift setup identity: {key}."))
}

fn model(request: &Request) -> crate::Result<Model> {
    let lock: Value = serde_json::from_str(SWIFT).map_err(|error| error.to_string())?;
    let derived: Value = serde_json::from_str(PTQ1).map_err(|error| error.to_string())?;
    let pin = bonsai_cpu_policy::select_artifact(
        &lock,
        bonsai_cpu_policy::Profile::SwiftPq2Assessment,
        request.use_repacked_ptq1,
        request.use_repacked_ptq1.then_some(&derived),
    )?;
    let source = &lock["model"];
    let filename = text(&pin, "Filename")?;
    Ok(Model {
        path: request
            .data_directory
            .join("models")
            .join(text(source, "revision")?)
            .join(filename),
        bytes: pin["Bytes"]
            .as_u64()
            .ok_or("Missing compiled Swift model length.")?,
        sha256: text(&pin, "Sha256")?.into(),
        url: (!request.use_repacked_ptq1).then(|| {
            format!(
                "https://huggingface.co/{}/resolve/{}/{filename}",
                source["repository"].as_str().unwrap_or_default(),
                source["revision"].as_str().unwrap_or_default(),
            )
        }),
    })
}

pub(super) fn status(
    working: &Path,
    stage: &str,
    message: &str,
    kind: &str,
    activity: &str,
    artifact: Option<(&Path, u64)>,
) -> crate::Result<()> {
    let path = working.join("chat-setup-progress.json");
    external(&path)?;
    let temporary = working.join(format!(
        ".chat-setup-progress-{}.json",
        crate::startup_process::unique()
    ));
    external(&temporary)?;
    let mut report =
        json!({"SchemaVersion":2,"Stage":stage,"Message":message,"Kind":kind,"Activity":activity});
    if let Some((path, bytes)) = artifact {
        report["ArtifactPath"] = json!(path);
        report["ExpectedBytes"] = json!(bytes);
    }
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| format!("Cannot write setup progress: {error}"))?;
    serde_json::to_writer(&mut output, &report).map_err(|error| error.to_string())?;
    output.flush().map_err(|error| error.to_string())?;
    drop(output);
    fs::rename(&temporary, &path).map_err(|error| format!("Cannot publish setup progress: {error}"))
}

fn preflight(data: &Path, model: &Model) -> crate::Result<()> {
    if model.path.exists() || model.url.is_none() {
        return Ok(());
    }
    #[cfg(windows)]
    {
        let partial = model.path.with_file_name(format!(
            "{}.partial",
            model
                .path
                .file_name()
                .ok_or("Missing model filename.")?
                .to_string_lossy()
        ));
        external(&partial)?;
        let downloaded = fs::symlink_metadata(&partial)
            .ok()
            .filter(|metadata| metadata.is_file() && !bonsai_artifacts::is_redirect(metadata))
            .map_or(0, |metadata| metadata.len().min(model.bytes));
        let needed = model
            .bytes
            .saturating_sub(downloaded)
            .saturating_add(256 * 1024 * 1024);
        let available = bonsai_windows_process::available_disk_bytes(data).map_err(|error| {
            format!(
                "Swift could not check free storage in {}: {error}",
                data.display()
            )
        })?;
        if available < needed {
            return Err(format!(
                "Swift needs at least {} MiB of free storage in {} for model setup; {} MiB is available. Free space, then select Retry setup.",
                needed.div_ceil(1024 * 1024),
                data.display(),
                available / (1024 * 1024),
            ));
        }
    }
    #[cfg(not(windows))]
    let _ = data;
    Ok(())
}

// Availability describes this selected model, not a persistent first-launch receipt.
pub(super) fn kind(request: &Request) -> crate::Result<&'static str> {
    let selected = model(request)?;
    Ok(if selected.path.exists() {
        "existing"
    } else {
        "first-run"
    })
}

fn publisher(request: &Request, working: &Path) -> crate::Result<()> {
    let kind = kind(request)?;
    runtime_status(
        working,
        kind,
        crate::startup_download::RepairActivity::Publishing,
        None,
    )?;
    // Portable packages do not carry a checkout. Materialize only the compiled
    // lock for the existing strict provisioner, then remove this private scratch.
    let path = working.join(format!(
        "chat-swift-lock-{}.json",
        crate::startup_process::unique()
    ));
    external(&path)?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| error.to_string())?;
    file.write_all(SWIFT.as_bytes())
        .map_err(|error| error.to_string())?;
    drop(file);
    let result = crate::provision::prepare_chat_runtime_observed(
        json!({
            "operation":"engine", "dataDirectory":request.data_directory,
            "workingDirectory":working, "lockPath":path,
        }),
        &mut |phase, artifact| runtime_status(working, kind, phase, artifact),
    );
    let cleanup = fs::remove_file(&path);
    result.map_err(|error| format!("The publisher runtime could not be prepared in {}: {error}. Select Retry setup after fixing the reported problem.", request.data_directory.display()))?;
    cleanup.map_err(|error| error.to_string())?;
    Ok(())
}

pub(super) fn prepare(request: &Request, working: &Path) -> crate::Result<()> {
    crate::startup_cancel::checkpoint()?;
    if !request.native_engine {
        publisher(request, working)?;
    }
    let selected = model(request)?;
    let kind = kind(request)?;
    prepare_weights(
        request,
        working,
        |url, path, bytes, hash, working| {
            let stage = if path == selected.path {
                "model"
            } else {
                "adapter"
            };
            crate::startup_download::receive_repair_observed(
                url,
                path,
                bytes,
                hash,
                working,
                &mut |activity| artifact_status(working, stage, kind, path, bytes, activity),
            )
        },
        crate::startup_download::verify,
    )
}

fn prepare_weights(
    request: &Request,
    working: &Path,
    mut receive: impl FnMut(&str, &Path, u64, &str, &Path) -> crate::Result<()>,
    mut verify: impl FnMut(&Path, u64, &str) -> crate::Result<()>,
) -> crate::Result<()> {
    let selected = model(request)?;
    preflight(&request.data_directory, &selected)?;
    let kind = kind(request)?;
    let (activity, message) = if selected.url.is_none() {
        ("verifying", "Verifying the selected PTQ1 model…")
    } else {
        ("checking", "Checking Swift model files…")
    };
    status(
        working,
        "model",
        message,
        kind,
        activity,
        Some((&selected.path, selected.bytes)),
    )?;
    if let Some(url) = selected.url {
        receive(&url, &selected.path, selected.bytes, &selected.sha256, working)
            .map_err(|error| format!("Swift model setup failed at {}: {error}. Check the internet connection, free storage and folder permissions, then select Retry setup. Partial downloads are kept for resume.", selected.path.display()))?;
    } else {
        verify(&selected.path, selected.bytes, &selected.sha256)
            .map_err(|error| format!("The explicitly selected PTQ1 export at {} could not be verified: {error}. Automatic setup cannot create this export. Supply the matching current-weight export, or close and relaunch with original PQ2 selected.", selected.path.display()))?;
    }
    if !request.no_abliterate_lora {
        let selected = bonsai_cpu_policy::abliterate_lora_artifact(&request.data_directory)?;
        let lock: Value = serde_json::from_str(ADAPTER).map_err(|error| error.to_string())?;
        status(
            working,
            "adapter",
            "Checking model adapter files…",
            kind,
            "checking",
            Some((&selected.path, selected.bytes)),
        )?;
        receive(text(&lock["adapter"], "url")?, &selected.path, selected.bytes, &selected.sha256, working)
            .map_err(|error| format!("Model adapter setup failed at {}: {error}. Check the internet connection, free storage and folder permissions, then select Retry setup.", selected.path.display()))?;
    }
    status(
        working,
        "ready",
        "Starting your local model\u{2026}",
        kind,
        "ready",
        None,
    )
}

pub(super) fn runtime_status(
    working: &Path,
    kind: &str,
    phase: crate::startup_download::RepairActivity,
    artifact: Option<(&Path, u64)>,
) -> crate::Result<()> {
    use crate::startup_download::RepairActivity;
    let (activity, message) = match phase {
        RepairActivity::VerifyingExisting => ("verifying", "Checking local runtime files\u{2026}"),
        RepairActivity::Downloading { offset: 0 } => {
            ("downloading", "Downloading the runtime\u{2026}")
        }
        RepairActivity::Downloading { .. } => ("downloading", "Resuming runtime download\u{2026}"),
        RepairActivity::VerifyingDownloaded => {
            ("verifying", "Verifying the downloaded runtime\u{2026}")
        }
        RepairActivity::Publishing => ("preparing", "Preparing the runtime\u{2026}"),
    };
    status(working, "runtime", message, kind, activity, artifact)
}

fn artifact_status(
    working: &Path,
    stage: &str,
    kind: &str,
    path: &Path,
    bytes: u64,
    phase: crate::startup_download::RepairActivity,
) -> crate::Result<()> {
    use crate::startup_download::RepairActivity;
    let name = if stage == "model" {
        "Swift model"
    } else {
        "model adapter"
    };
    let (activity, message) = match phase {
        RepairActivity::VerifyingExisting if path.exists() => {
            ("verifying", format!("Verifying local {name}…"))
        }
        RepairActivity::VerifyingExisting => ("checking", format!("Checking {name} files…")),
        RepairActivity::Downloading { offset: 0 } => {
            ("downloading", format!("Downloading {name}…"))
        }
        RepairActivity::Downloading { .. } => ("downloading", format!("Resuming {name} download…")),
        RepairActivity::VerifyingDownloaded => {
            ("verifying", format!("Verifying downloaded {name}…"))
        }
        RepairActivity::Publishing => ("preparing", format!("Preparing verified {name}…")),
    };
    status(
        working,
        stage,
        &message,
        kind,
        activity,
        Some((path, bytes)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_model_identity_matches_swift_policy_and_never_downloads_ptq1() -> crate::Result<()> {
        let data = std::env::temp_dir().join("Bonsai-setup-model-pin-only");
        let mut request: Request = serde_json::from_value(json!({
            "DataDirectory":data,"RepositoryRoot":data.join("source"),
        }))
        .map_err(|error| error.to_string())?;
        let pq2 = model(&request)?;
        assert_eq!(pq2.bytes, 7_206_168_928);
        assert_eq!(
            pq2.sha256,
            "5912bb739217cf25b4283a098c7b893d9baff36bdf2e72d3dbf4cb999d5633d5"
        );
        assert!(pq2.path.ends_with(
            "models/a3bdac086bbb6b04d87d0108d18943b5f63868f7/Swift-Bonsai-2-PQ2_0.gguf"
        ));
        assert_eq!(
            pq2.url.as_deref(),
            Some(
                "https://huggingface.co/ukisai/Swift-Bonsai-2-GGUF/resolve/a3bdac086bbb6b04d87d0108d18943b5f63868f7/Swift-Bonsai-2-PQ2_0.gguf"
            )
        );
        request.use_repacked_ptq1 = true;
        let ptq1 = model(&request)?;
        assert!(ptq1.url.is_none());
        assert_eq!(ptq1.bytes, 5_946_648_928);
        assert_ne!(ptq1.path, pq2.path);
        Ok(())
    }

    #[test]
    fn setup_progress_replaces_complete_reports_and_missing_ptq1_never_fetches_pq2()
    -> crate::Result<()> {
        let working = std::env::temp_dir().join(format!(
            "Bonsai-SetupProgress-{}",
            crate::startup_process::unique()
        ));
        fs::create_dir(&working).map_err(|error| error.to_string())?;
        let data = working.join("data");
        fs::create_dir(&data).map_err(|error| error.to_string())?;
        let request: Request = serde_json::from_value(json!({
            "DataDirectory":data,"RepositoryRoot":working.join("source"),
            "NativeEngine":true,"UseRepackedPtq1":true,"NoAbliterateLora":true,
        }))
        .map_err(|error| error.to_string())?;
        status(
            &working,
            "runtime",
            "Checking runtime",
            "first-run",
            "preparing",
            None,
        )?;
        let error = prepare(&request, &working)
            .err()
            .ok_or("Missing PTQ1 unexpectedly succeeded.")?;
        assert!(error.contains("explicitly selected PTQ1 export"));
        assert!(error.contains("Automatic setup cannot create this export"));
        assert!(!data.join("models").exists());
        assert!(!data.join("downloads").exists());
        let report: Value = serde_json::from_slice(
            &fs::read(working.join("chat-setup-progress.json"))
                .map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        assert_eq!(report["SchemaVersion"], 2);
        assert_eq!(report["Kind"], "first-run");
        assert_eq!(report["Activity"], "verifying");
        assert_eq!(report["Stage"], "model");
        assert_eq!(report["ExpectedBytes"], 5_946_648_928_u64);
        assert!(
            report["ArtifactPath"]
                .as_str()
                .is_some_and(|path| path.ends_with("Swift-Bonsai-2-current-repacked-PTQ1_0.gguf"))
        );
        assert_eq!(
            fs::read_dir(&working)
                .map_err(|error| error.to_string())?
                .count(),
            2
        );
        fs::remove_dir_all(working).map_err(|error| error.to_string())
    }

    #[test]
    fn automatic_weight_setup_downloads_only_selected_pins_and_stops_at_first_failure()
    -> crate::Result<()> {
        let working = std::env::temp_dir().join(format!(
            "Bonsai-SetupDownloads-{}",
            crate::startup_process::unique()
        ));
        fs::create_dir(&working).map_err(|error| error.to_string())?;
        let data = working.join("data");
        fs::create_dir(&data).map_err(|error| error.to_string())?;
        let mut request: Request = serde_json::from_value(json!({
            "DataDirectory":data,"RepositoryRoot":working.join("source"),"NativeEngine":true,
        }))
        .map_err(|error| error.to_string())?;
        // A tiny placeholder keeps this pure callback control independent of
        // the owner's free storage; no model bytes or network are consumed.
        let selected = model(&request)?;
        fs::create_dir_all(selected.path.parent().ok_or("Missing parent.")?)
            .map_err(|error| error.to_string())?;
        fs::write(&selected.path, b"intentionally invalid model-free fixture")
            .map_err(|error| error.to_string())?;
        for adapter in [false, true] {
            request.no_abliterate_lora = !adapter;
            let mut received = Vec::new();
            prepare_weights(
                &request,
                &working,
                |url, path, bytes, hash, _| {
                    received.push((url.to_owned(), path.to_owned(), bytes, hash.to_owned()));
                    Ok(())
                },
                |_, _, _| Err("PQ2 must use verified download preparation.".into()),
            )?;
            assert_eq!(received.len(), if adapter { 2 } else { 1 });
            assert_eq!(received[0].1, selected.path);
            assert_eq!(received[0].2, selected.bytes);
            assert_eq!(received[0].3, selected.sha256);
            if adapter {
                let expected = bonsai_cpu_policy::abliterate_lora_artifact(&data)?;
                assert_eq!(received[1].1, expected.path);
                assert_eq!(received[1].2, expected.bytes);
                assert_eq!(received[1].3, expected.sha256);
            }
        }
        request.no_abliterate_lora = false;
        let mut count = 0;
        let error = prepare_weights(
            &request,
            &working,
            |_, _, _, _, _| {
                count += 1;
                Err("Synthetic interrupted download".into())
            },
            |_, _, _| Ok(()),
        )
        .err()
        .ok_or("Download failure was ignored.")?;
        assert!(error.contains("Synthetic interrupted download"));
        assert_eq!(count, 1);

        request.use_repacked_ptq1 = true;
        request.no_abliterate_lora = true;
        let mut verified = Vec::new();
        prepare_weights(
            &request,
            &working,
            |_, _, _, _, _| Err("PTQ1 must never fetch substitute weights.".into()),
            |path, bytes, hash| {
                verified.push((path.to_owned(), bytes, hash.to_owned()));
                Ok(())
            },
        )?;
        assert_eq!(verified.len(), 1);
        assert_eq!(verified[0].1, 5_946_648_928);
        assert!(
            verified[0]
                .0
                .ends_with("Swift-Bonsai-2-current-repacked-PTQ1_0.gguf")
        );
        fs::remove_dir_all(working).map_err(|error| error.to_string())
    }
}
