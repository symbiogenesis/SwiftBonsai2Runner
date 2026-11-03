//! Source-bound PTQ1 collector receipts and complete unsigned ABI comparisons.

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::Read as _;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::identity::{is_lower_hex, sha256};

const LIMIT: u64 = 65_536;
const COLLECTOR: &str = "tools/native-reference/swift_ptq1_reference.rs";
const FIXTURES: &str = "tools/native-reference/fixtures";
const PIN_MISMATCH: &str = "Hosted capture tools differ from reviewed source/revision pins.";
const BINARY_NAMES: [&str; 2] = ["swift-ptq1-reference.exe", "swift_model_check.exe"];
const BINARY_LIMIT: u64 = 134_217_728;
const COMPILER_VERSION: &str = "rustc 1.101.0-nightly (3e92e1a2d 2026-11-01)\n\
binary: rustc\n\
commit-hash: 3e92e1a2d20b991ef23a6c6064c34d57ee8c98d3\n\
commit-date: 2026-11-01\n\
host: aarch64-pc-windows-msvc\n\
release: 1.101.0-nightly\n\
LLVM version: 23.1.5";

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "kebab-case", deny_unknown_fields)]
enum Request {
    #[serde(rename_all = "camelCase")]
    Receipt {
        bundle_directory: PathBuf,
        repository_directory: PathBuf,
        revision: String,
        pins: Value,
        #[serde(default = "one_row_worker")]
        row_workers: u8,
    },
    #[serde(rename_all = "camelCase")]
    CompareAbi {
        actual_path: PathBuf,
        expected_path: PathBuf,
        expected_sha256: String,
    },
}

const fn one_row_worker() -> u8 {
    1
}

fn read_bounded(path: &Path, maximum: u64) -> crate::Result<Vec<u8>> {
    if !path.is_absolute() {
        return Err("Capture paths must be expanded absolute paths.".into());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        options.share_mode(1).custom_flags(0x0020_0000);
    }
    let file: File = options.open(path).map_err(|error| error.to_string())?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file() || bonsai_artifacts::is_redirect(&metadata) || metadata.len() > maximum {
        return Err("Capture input exceeds its fixed pre-read byte bound.".into());
    }
    let mut bytes = Vec::new();
    file.take(maximum + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if u64::try_from(bytes.len()).map_err(|error| error.to_string())? > maximum {
        return Err("Capture input exceeds its fixed pre-read byte bound.".into());
    }
    Ok(bytes)
}

fn observations(bytes: &[u8]) -> crate::Result<BTreeMap<&str, u64>> {
    let text = std::str::from_utf8(bytes).map_err(|error| error.to_string())?;
    let mut records = BTreeMap::new();
    let mut lines = text.split('\n').peekable();
    while let Some(line) = lines.next() {
        if line.is_empty() && lines.peek().is_none() {
            break;
        }
        let line = if lines.peek().is_some() {
            line.strip_suffix('\r').unwrap_or(line)
        } else {
            line
        };
        let (key, number) = line
            .split_once('=')
            .ok_or("Invalid ABI observation record.")?;
        if !key
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
            || !key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.'))
            || number.is_empty()
            || !number.bytes().all(|byte| byte.is_ascii_digit())
            || (number.len() > 1 && number.starts_with('0'))
        {
            return Err("Invalid ABI observation record.".into());
        }
        let value = number
            .parse()
            .map_err(|_| "Overflowing ABI observation record.")?;
        if records.insert(key, value).is_some() {
            return Err("Duplicate ABI observation record.".into());
        }
    }
    if records.is_empty() {
        return Err("Empty ABI observation set.".into());
    }
    Ok(records)
}

fn compare(actual: &[u8], expected: &[u8], digest: &str) -> crate::Result<Value> {
    if !is_lower_hex(digest, 64) || sha256(expected) != digest {
        return Err("ABI observation control bytes differ from the source-bound receipt.".into());
    }
    let actual = observations(actual)?;
    let expected = observations(expected)?;
    if actual.len() != expected.len() {
        return Err("Native/Rust ABI observation inventory differs.".into());
    }
    for (key, value) in &expected {
        if actual.get(key) != Some(value) {
            return Err(format!("Native/Rust ABI observation differs: {key}."));
        }
    }
    Ok(json!({"Status": "matched", "ObservationCount": expected.len()}))
}

fn validate_checker_capability(receipt: &Value, row_workers: u8) -> crate::Result<()> {
    let accepted = match receipt["SchemaVersion"].as_u64() {
        Some(2) => {
            row_workers == 1
                && receipt.get("CheckerFeatures").is_none()
                && receipt.get("SupportedRowWorkers").is_none()
        }
        Some(3) => {
            matches!(row_workers, 1 | 4)
                && receipt["BuildFeatures"] == json!(["parallel-rows"])
                && receipt["CheckerFeatures"] == json!(["parallel-rows"])
                && receipt["SupportedRowWorkers"] == json!([1, 4])
        }
        _ => false,
    };
    if !accepted {
        return Err(PIN_MISMATCH.into());
    }
    Ok(())
}

fn validate_binary_inventory(receipt: &Value) -> crate::Result<()> {
    let files = receipt["Files"]
        .as_array()
        .ok_or("Capture receipt requires its exact binary inventory.")?;
    // Exact membership is independent of locale and the producer's ordering.
    // Requiring one record per expected name also rejects duplicate records.
    if files.len() != BINARY_NAMES.len()
        || BINARY_NAMES
            .iter()
            .any(|name| files.iter().filter(|entry| entry["Name"] == *name).count() != 1)
        || files.iter().any(|entry| {
            !entry["Bytes"]
                .as_u64()
                .is_some_and(|bytes| (1..=BINARY_LIMIT).contains(&bytes))
                || !entry["Sha256"]
                    .as_str()
                    .is_some_and(|digest| is_lower_hex(digest, 64))
        })
    {
        return Err("Capture receipt binary names, lengths or SHA-256 identities differ.".into());
    }
    Ok(())
}

fn validate_compiler(receipt: &Value) -> crate::Result<()> {
    let compiler = &receipt["Compiler"];
    if !compiler["RustcVerbose"]
        .as_str()
        .is_some_and(|version| version.replace("\r\n", "\n").trim() == COMPILER_VERSION)
        || ["Sha256", "LinkerSha256"].iter().any(|&field| {
            !compiler[field]
                .as_str()
                .is_some_and(|digest| is_lower_hex(digest, 64))
        })
    {
        return Err(
            "Capture compiler identity differs from the pinned native ARM64 toolchain.".into(),
        );
    }
    Ok(())
}

fn receipt(
    bundle: &Path,
    repository: &Path,
    revision: &str,
    pins: &Value,
    row_workers: u8,
) -> crate::Result<Value> {
    let bytes = read_bounded(&bundle.join("capture-tools.json"), LIMIT)?;
    let receipt: Value = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    if !is_lower_hex(revision, 40)
        || receipt["RunnerRevision"] != revision
        || receipt["CollectorLanguage"] != "rust"
        || receipt["CollectorSourcePath"] != COLLECTOR
        || receipt["RustToolchain"] != "nightly-2026-11-02-aarch64-pc-windows-msvc"
    {
        return Err(PIN_MISMATCH.into());
    }
    validate_checker_capability(&receipt, row_workers)?;
    validate_binary_inventory(&receipt)?;
    validate_compiler(&receipt)?;
    for (receipt_key, pin_key, fields) in [
        (
            "NativeSource",
            "nativeSource",
            &["commit", "url", "sha256", "bytes", "headerSha256"][..],
        ),
        (
            "RustSource",
            "rustSource",
            &["commit", "url", "sha256", "bytes"][..],
        ),
    ] {
        for &field in fields {
            let expected = &pins[pin_key][field];
            if expected.is_null() || receipt[receipt_key][field] != *expected {
                return Err(PIN_MISMATCH.into());
            }
        }
    }
    for (field, relative, maximum) in [
        (
            "CollectorSourceSha256",
            COLLECTOR.to_owned(),
            4 * 1024 * 1024,
        ),
        (
            "AbiControlSha256",
            format!("{FIXTURES}/collector-abi-v1.txt"),
            LIMIT,
        ),
        (
            "AbiProvenanceSha256",
            format!("{FIXTURES}/collector-abi-v1.provenance.json"),
            LIMIT,
        ),
    ] {
        if receipt[field] != sha256(&read_bounded(&repository.join(relative), maximum)?) {
            return Err(PIN_MISMATCH.into());
        }
    }
    Ok(receipt)
}

/// Reviews source-bound collector metadata or compares complete ABI observations.
///
/// # Errors
/// Rejects malformed requests, oversized inputs, changed controls and incomplete ABI sets.
pub fn run(request: Value) -> crate::Result<Value> {
    match serde_json::from_value(request).map_err(|error| error.to_string())? {
        Request::Receipt {
            bundle_directory,
            repository_directory,
            revision,
            pins,
            row_workers,
        } => receipt(
            &bundle_directory,
            &repository_directory,
            &revision,
            &pins,
            row_workers,
        ),
        Request::CompareAbi {
            actual_path,
            expected_path,
            expected_sha256,
        } => compare(
            &read_bounded(&actual_path, LIMIT)?,
            &read_bounded(&expected_path, LIMIT)?,
            &expected_sha256,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        compare, observations, read_bounded, receipt, validate_binary_inventory,
        validate_checker_capability, validate_compiler,
    };
    use crate::identity::sha256;
    use serde_json::{Value, json};

    #[test]
    fn collector_receipt_binds_sources_pins_and_pre_read_limits() -> crate::Result<()> {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("Bonsai-Ptq1Receipt-{}-{nonce}", std::process::id()));
        let run = || -> crate::Result<()> {
            let fixtures = root.join(super::FIXTURES);
            std::fs::create_dir_all(&fixtures).map_err(|error| error.to_string())?;
            let source = b"reviewed Rust collector";
            let abi = b"model.size=80\n";
            let provenance = b"{}";
            for (relative, bytes) in [
                (super::COLLECTOR.to_owned(), source.as_slice()),
                (
                    format!("{}/collector-abi-v1.txt", super::FIXTURES),
                    abi.as_slice(),
                ),
                (
                    format!("{}/collector-abi-v1.provenance.json", super::FIXTURES),
                    provenance.as_slice(),
                ),
            ] {
                std::fs::write(root.join(relative), bytes).map_err(|error| error.to_string())?;
            }
            let pins = json!({
                "nativeSource": {"commit":"native", "url":"https://example.invalid/native", "sha256":"native-hash", "bytes":1, "headerSha256":"header"},
                "rustSource": {"commit":"rust", "url":"https://example.invalid/rust", "sha256":"rust-hash", "bytes":2}
            });
            let revision = "a".repeat(40);
            let original = json!({
                "SchemaVersion":3, "RunnerRevision":revision, "CollectorLanguage":"rust", "CollectorSourcePath":super::COLLECTOR,
                "BuildFeatures":["parallel-rows"], "CheckerFeatures":["parallel-rows"], "SupportedRowWorkers":[1,4],
                "RustToolchain":"nightly-2026-11-02-aarch64-pc-windows-msvc", "NativeSource":pins["nativeSource"], "RustSource":pins["rustSource"],
                "CollectorSourceSha256":sha256(source), "AbiControlSha256":sha256(abi), "AbiProvenanceSha256":sha256(provenance),
                "Files":binary_inventory(),
                "Compiler":{"RustcVerbose":super::COMPILER_VERSION,"Sha256":"b".repeat(64),"LinkerSha256":"c".repeat(64)}
            });
            let path = root.join("capture-tools.json");
            for field in [
                None,
                Some("RunnerRevision"),
                Some("CollectorSourceSha256"),
                Some("AbiControlSha256"),
                Some("AbiProvenanceSha256"),
                Some("Files"),
                Some("Compiler"),
                Some("BuildFeatures"),
            ] {
                let mut value = original.clone();
                if let Some(field) = field {
                    value[field] = "changed".into();
                }
                std::fs::write(
                    &path,
                    serde_json::to_vec(&value).map_err(|error| error.to_string())?,
                )
                .map_err(|error| error.to_string())?;
                assert_eq!(
                    receipt(&root, &root, &revision, &pins, 1).is_ok(),
                    field.is_none()
                );
            }
            std::fs::write(&path, vec![b' '; 65_537]).map_err(|error| error.to_string())?;
            assert!(read_bounded(&path, super::LIMIT).is_err());
            Ok(())
        };
        let result = run();
        if root.exists() {
            std::fs::remove_dir_all(&root).map_err(|error| error.to_string())?;
        }
        result
    }

    fn binary_inventory() -> Value {
        json!([
            {"Name":"swift-ptq1-reference.exe", "Bytes":1,"Sha256":"a".repeat(64)},
            {"Name":"swift_model_check.exe", "Bytes":super::BINARY_LIMIT,"Sha256":"b".repeat(64)}
        ])
    }

    #[test]
    fn collector_inventory_accepts_either_exact_order_without_sorting() -> crate::Result<()> {
        let mut receipt = json!({"Files":binary_inventory()});
        validate_binary_inventory(&receipt)?;
        receipt["Files"]
            .as_array_mut()
            .ok_or("Expected binary array.")?
            .reverse();
        validate_binary_inventory(&receipt)
    }

    #[test]
    fn collector_inventory_rejects_missing_duplicate_and_inexact_binary_names() {
        let files = binary_inventory();
        for changed in [
            Value::Null,
            json!({}),
            json!([]),
            json!([files[0]]),
            json!([files[1]]),
            json!([files[0], files[0]]),
            json!([files[1], files[1]]),
            json!([files[0], files[1], files[0]]),
        ] {
            assert!(validate_binary_inventory(&json!({"Files":changed})).is_err());
        }
        for name in [
            Value::Null,
            json!(true),
            json!("SWIFT-PTQ1-reference.exe"),
            json!("swift-ptq1-reference.EXE"),
            json!("swift-ptq1-reference.exe "),
            json!("../swift-ptq1-reference.exe"),
            json!("swift_model_check.exe"),
            json!("unexpected.exe"),
        ] {
            let mut receipt = json!({"Files":files});
            receipt["Files"][0]["Name"] = name;
            assert!(validate_binary_inventory(&receipt).is_err());
        }
    }

    #[test]
    fn collector_inventory_rejects_unbounded_lengths_and_invalid_digests() {
        for (field, changed) in [
            ("Bytes", Value::Null),
            ("Bytes", json!(0)),
            ("Bytes", json!(-1)),
            ("Bytes", json!("1")),
            ("Bytes", json!(1.5)),
            ("Bytes", json!(super::BINARY_LIMIT + 1)),
            ("Bytes", json!(u64::MAX)),
            ("Sha256", Value::Null),
            ("Sha256", json!("a".repeat(63))),
            ("Sha256", json!("a".repeat(65))),
            ("Sha256", json!("A".repeat(64))),
            ("Sha256", json!("g".repeat(64))),
        ] {
            let mut receipt = json!({"Files":binary_inventory()});
            receipt["Files"][0][field] = changed;
            assert!(validate_binary_inventory(&receipt).is_err(), "{field}");
        }
    }

    #[test]
    fn collector_compiler_requires_the_exact_pinned_version_and_hash_metadata() -> crate::Result<()>
    {
        let original = json!({"Compiler":{
            "RustcVerbose":super::COMPILER_VERSION,"Sha256":"a".repeat(64),"LinkerSha256":"b".repeat(64)
        }});
        validate_compiler(&original)?;
        let mut windows_lines = original.clone();
        windows_lines["Compiler"]["RustcVerbose"] = json!(format!(
            "{}\r\n",
            super::COMPILER_VERSION.replace('\n', "\r\n")
        ));
        validate_compiler(&windows_lines)?;
        for (field, changed) in [
            ("RustcVerbose", Value::Null),
            (
                "RustcVerbose",
                json!(super::COMPILER_VERSION.replace("aarch64", "x86_64")),
            ),
            (
                "RustcVerbose",
                json!(super::COMPILER_VERSION.replace("3e92e1a2d", "000000000")),
            ),
            (
                "RustcVerbose",
                json!(format!("{}\nextra", super::COMPILER_VERSION)),
            ),
            ("Sha256", Value::Null),
            ("Sha256", json!("A".repeat(64))),
            ("LinkerSha256", Value::Null),
            ("LinkerSha256", json!("a".repeat(63))),
        ] {
            let mut receipt = original.clone();
            receipt["Compiler"][field] = changed;
            assert!(validate_compiler(&receipt).is_err(), "{field}");
        }
        Ok(())
    }

    #[test]
    fn collector_receipt_rejects_unbound_parallel_capability() -> crate::Result<()> {
        let old = json!({"SchemaVersion": 2});
        validate_checker_capability(&old, 1)?;
        assert!(validate_checker_capability(&old, 4).is_err());
        let current = json!({
            "SchemaVersion": 3, "BuildFeatures": ["parallel-rows"], "CheckerFeatures": ["parallel-rows"],
            "SupportedRowWorkers": [1, 4]
        });
        for workers in [1, 4] {
            validate_checker_capability(&current, workers)?;
        }
        for workers in [0, 2, 16] {
            assert!(validate_checker_capability(&current, workers).is_err());
        }
        for (field, changed) in [
            ("SchemaVersion", json!(2)),
            ("BuildFeatures", Value::Null),
            ("BuildFeatures", json!([])),
            ("BuildFeatures", json!(["parallel-rows", "unreviewed"])),
            ("CheckerFeatures", Value::Null),
            ("CheckerFeatures", json!([])),
            ("CheckerFeatures", json!(["parallel-rows", "unreviewed"])),
            ("SupportedRowWorkers", json!([1])),
            ("SupportedRowWorkers", json!([1, 4, 4])),
            ("SupportedRowWorkers", json!(["1", "4"])),
        ] {
            let mut receipt = current.clone();
            receipt[field] = changed;
            assert!(validate_checker_capability(&receipt, 4).is_err());
        }
        Ok(())
    }

    #[test]
    fn abi_controls_require_complete_unique_unsigned_records() -> crate::Result<()> {
        let expected = b"model.size=80\nbatch.size=56\nsigned=18446744073709551615\n";
        let digest = sha256(expected);
        let actual = b"signed=18446744073709551615\r\nbatch.size=56\r\nmodel.size=80\r\n";
        assert_eq!(compare(actual, expected, &digest)?["ObservationCount"], 3);
        for invalid in [
            "",
            "\n",
            "a=1\n\n",
            "a=1\na=1\n",
            "a=01",
            "a=-1",
            "a=+1",
            "a=1.0",
            "a=18446744073709551616",
            "a=1\ntail",
            "1a=0",
            "a=0\r\r\n",
            "a= 0",
            "a=1\r",
        ] {
            assert!(observations(invalid.as_bytes()).is_err(), "{invalid:?}");
        }
        for changed in [
            b"model.size=80\nbatch.size=56\n".as_slice(),
            b"model.size=81\nbatch.size=56\nsigned=18446744073709551615\n".as_slice(),
            b"model.size=80\nbatch.size=56\nsigned=18446744073709551615\nextra=1\n".as_slice(),
        ] {
            assert!(compare(changed, expected, &digest).is_err());
        }
        assert!(compare(actual, actual, &digest).is_err());
        assert!(observations(b"a=1\n\xff=0").is_err());
        Ok(())
    }
}
