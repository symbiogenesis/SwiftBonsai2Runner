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
    if !metadata.is_file() || metadata.len() > maximum {
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
        || receipt["Files"].as_array().map(Vec::len) != Some(2)
    {
        return Err(PIN_MISMATCH.into());
    }
    validate_checker_capability(&receipt, row_workers)?;
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
    use super::{compare, observations, read_bounded, receipt, validate_checker_capability};
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
                "SchemaVersion":2, "RunnerRevision":revision, "CollectorLanguage":"rust", "CollectorSourcePath":super::COLLECTOR,
                "RustToolchain":"nightly-2026-11-02-aarch64-pc-windows-msvc", "NativeSource":pins["nativeSource"], "RustSource":pins["rustSource"],
                "CollectorSourceSha256":sha256(source), "AbiControlSha256":sha256(abi), "AbiProvenanceSha256":sha256(provenance), "Files":[{},{}]
            });
            let path = root.join("capture-tools.json");
            for field in [
                None,
                Some("RunnerRevision"),
                Some("CollectorSourceSha256"),
                Some("AbiControlSha256"),
                Some("AbiProvenanceSha256"),
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

    #[test]
    fn collector_receipt_rejects_unbound_parallel_capability() -> crate::Result<()> {
        let old = json!({"SchemaVersion": 2});
        validate_checker_capability(&old, 1)?;
        assert!(validate_checker_capability(&old, 4).is_err());
        let current = json!({
            "SchemaVersion": 3, "CheckerFeatures": ["parallel-rows"],
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
