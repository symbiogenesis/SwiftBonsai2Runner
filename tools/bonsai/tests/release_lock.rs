//! Native release-review regressions formerly exercised through PowerShell fixtures.
#![cfg(windows)]
#![forbid(unsafe_code)]

use std::fs;
use std::io::{Cursor, Write as _};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use zip::{ZipWriter, write::SimpleFileOptions};

type TestResult = Result<(), Box<dyn std::error::Error>>;
const REVISION: &str = "1234567890123456789012345678901234567890";

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .flat_map(|byte| [byte >> 4, byte & 15])
        .map(|nibble| char::from(b"0123456789abcdef"[usize::from(nibble)]))
        .collect()
}

fn zip(entries: &[(&str, &[u8])]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        zip.start_file(*name, SimpleFileOptions::default())?;
        zip.write_all(bytes)?;
    }
    Ok(zip.finish()?.into_inner())
}

fn source(path: &Path, server_version: &str, malformed: bool) -> TestResult {
    let root = format!("prism-inference-{REVISION}");
    let root_name = format!("{root}/Cargo.toml");
    let server_name = format!("{root}/server/Cargo.toml");
    let toolchain_name = format!("{root}/rust-toolchain.toml");
    let server = format!(
        "\u{feff}[package]\nname = \"prism-inference-server\"\nversion = \"{server_version}\"\n"
    );
    let invalid = [0xff];
    fs::write(
        path,
        zip(&[
            (
                root_name.as_str(),
                b"\xef\xbb\xbf[package]\nname = \"prism-inference\"\nversion = \"0.1.0\"\n",
            ),
            (
                server_name.as_str(),
                if malformed {
                    &invalid
                } else {
                    server.as_bytes()
                },
            ),
            (
                toolchain_name.as_str(),
                b"\xef\xbb\xbf[toolchain]\nchannel = \"nightly-2026-10-22\"\n",
            ),
        ])?,
    )?;
    Ok(())
}

#[test]
fn release_review_binds_source_bundle_run_and_architecture_without_replacing_outputs() -> TestResult
{
    let root = std::env::temp_dir().join(format!(
        "Bonsai-ReleaseReview-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    ));
    fs::create_dir(&root)?;
    let mut image = vec![0; 128];
    image[..2].copy_from_slice(b"MZ");
    image[60..64].copy_from_slice(&64_u32.to_le_bytes());
    image[64..70].copy_from_slice(&[b'P', b'E', 0, 0, 0x64, 0xaa]);
    let names = [
        "prism-inference.exe",
        "bonsai-token-worker.exe",
        "bonsai-swift-worker.exe",
        "prism-inference-server.exe",
        "bonsai-gguf.exe",
        "LICENSE",
        "NOTICE",
        "PRISM-LICENSE.txt",
        "CAMELID-LICENSE.txt",
    ];
    let bundle = zip(&names
        .iter()
        .map(|name| (*name, image.as_slice()))
        .collect::<Vec<_>>())?;
    let archive = root.join("bundle.zip");
    fs::write(&archive, &bundle)?;
    let source_path = root.join("source.zip");
    source(&source_path, "0.1.0", false)?;
    let receipt_path = root.join("receipt.json");
    let receipt = json!({"SchemaVersion": 1, "Repository": "https://github.com/symbiogenesis/prism-inference",
        "SourceRevision": REVISION, "Version": "0.1.0", "Target": "aarch64-pc-windows-msvc",
        "Toolchain": "nightly-2026-10-22-aarch64-pc-windows-msvc", "RunId": "12345", "RunAttempt": "2",
        "Files": names.iter().map(|name| json!({"Name": name, "Bytes": image.len(), "Sha256": digest(&image)})).collect::<Vec<_>>(),
        "Archive": {"Name": "prism-inference-0.1.0-aarch64-pc-windows-msvc.zip", "Bytes": bundle.len(), "Sha256": digest(&bundle)}});
    fs::write(&receipt_path, serde_json::to_vec(&receipt)?)?;
    let output = root.join("candidate.json");
    let mut request = json!({"receiptPath": receipt_path, "archivePath": archive, "sourceArchivePath": source_path,
        "sourceRevision": REVISION, "version": "0.1.0", "runId": "12345", "runAttempt": 2, "outputPath": output});
    let reviewed = bonsai_tools::inference_lock::run(request.clone())?;
    let candidate: Value = serde_json::from_slice(&fs::read(&output)?)?;
    assert_eq!(reviewed["Verified"], true);
    assert_eq!(candidate["files"].as_array().map(Vec::len), Some(9));
    assert_eq!(candidate["Source"]["Commit"], REVISION);
    assert_eq!(
        candidate["Source"]["Sha256"],
        digest(&fs::read(&source_path)?)
    );
    assert!(bonsai_tools::inference_lock::run(request.clone()).is_err());
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(&output)?)?,
        candidate
    );
    for (field, invalid) in [
        (
            "SourceRevision",
            json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        ),
        ("Target", json!("x86_64-pc-windows-msvc")),
        ("RunAttempt", json!("1")),
        (
            "Toolchain",
            json!("nightly-2026-10-21-aarch64-pc-windows-msvc"),
        ),
    ] {
        let mut changed = receipt.clone();
        changed[field] = invalid;
        fs::write(&receipt_path, serde_json::to_vec(&changed)?)?;
        let rejected_output = root.join(format!("reject-{field}.json"));
        request["outputPath"] = json!(rejected_output);
        assert!(
            bonsai_tools::inference_lock::run(request.clone()).is_err(),
            "{field}"
        );
        assert!(!rejected_output.exists());
    }
    for field in ["Files", "Archive"] {
        let mut changed = receipt.clone();
        if field == "Files" {
            changed["Files"][0]["Sha256"] = json!("0".repeat(64));
        } else {
            changed["Archive"]["Sha256"] = json!("0".repeat(64));
        }
        fs::write(&receipt_path, serde_json::to_vec(&changed)?)?;
        request["outputPath"] = json!(root.join(format!("reject-{field}.json")));
        assert!(bonsai_tools::inference_lock::run(request.clone()).is_err());
    }
    fs::write(&receipt_path, serde_json::to_vec(&receipt)?)?;
    for (version, malformed) in [("9.9.9", false), ("0.1.0", true)] {
        source(&source_path, version, malformed)?;
        request["outputPath"] =
            json!(root.join(format!("reject-source-{version}-{malformed}.json")));
        assert!(bonsai_tools::inference_lock::run(request.clone()).is_err());
    }
    fs::remove_dir_all(root)?;
    Ok(())
}
