//! Literal miniature ZIP controls, never observations of native source execution.

use std::io::{Cursor, Write as _};

use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

use super::{Pin, inventory, portable_path};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn pin() -> Pin {
    Pin {
        schema_version: 1,
        repository: "https://example.invalid/source".into(),
        revision: "a".repeat(40),
        archive_root: "source-revision".into(),
        archive_bytes: 123,
        archive_sha256: "b".repeat(64),
    }
}

fn fixture(entries: &[(&str, &[u8])]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        writer.start_file(
            *name,
            SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
        )?;
        writer.write_all(bytes)?;
    }
    Ok(writer.finish()?.into_inner())
}

#[test]
fn native_archive_inventory_covers_all_files_including_notices_and_build_inputs() -> TestResult {
    let bytes = fixture(&[
        ("source-revision/z.cpp", b"abc"),
        ("source-revision/LICENSE", b"license"),
        ("source-revision/CMakeLists.txt", b"build"),
        ("source-revision/generated/empty.h", b""),
    ])?;
    let result = inventory(Cursor::new(bytes), &pin())?;
    assert_eq!(result["fileCount"], 4);
    assert_eq!(result["sourceBytes"], 15);
    assert_eq!(result["files"][0]["path"], "CMakeLists.txt");
    assert_eq!(
        result["files"][3]["sha256"],
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    for gate in [
        "sourceExecuted",
        "compiledTranslationUnitClosureQualified",
        "inferenceParityQualified",
        "nativeRetirementQualified",
    ] {
        assert_eq!(result[gate], false, "{gate}");
    }
    Ok(())
}

#[test]
fn native_archive_inventory_rejects_unsafe_names_roots_and_implicit_collisions() -> TestResult {
    for path in [
        "../source",
        "/absolute",
        "a//b",
        "a\\b",
        "a:b",
        "a/./b",
        "CON.cpp",
        "a/LPT1.txt",
        "a/trailing.",
        "a/trailing ",
        "a\n.cpp",
    ] {
        assert!(!portable_path(path), "{path}");
    }
    for names in [
        vec!["outside/a.cpp"],
        vec!["source-revision/a.cpp", "source-revision/A.cpp"],
        vec!["source-revision/a", "source-revision/a/b.cpp"],
        vec!["source-revision/a/b.cpp", "source-revision/A"],
    ] {
        let entries = names
            .iter()
            .map(|name| (*name, b"abc".as_slice()))
            .collect::<Vec<_>>();
        assert!(
            inventory(Cursor::new(fixture(&entries)?), &pin()).is_err(),
            "{names:?}"
        );
    }
    Ok(())
}

#[test]
fn native_archive_inventory_rejects_symlinks_corruption_and_invalid_pins() -> TestResult {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer.add_symlink(
        "source-revision/link",
        "source.cpp",
        SimpleFileOptions::default(),
    )?;
    assert!(inventory(writer.finish()?, &pin()).is_err());
    assert!(inventory(Cursor::new(b"not a ZIP"), &pin()).is_err());
    let bytes = fixture(&[("source-revision/source.cpp", b"abc")])?;
    for change in 0..5 {
        let mut value = pin();
        match change {
            0 => value.schema_version = 2,
            1 => value.archive_root = "../source".into(),
            2 => value.archive_bytes = super::MAX_ARCHIVE + 1,
            3 => value.archive_sha256 = "B".repeat(64),
            _ => value.revision = "not-a-commit".into(),
        }
        assert!(inventory(Cursor::new(&bytes), &value).is_err());
    }
    Ok(())
}

#[test]
fn native_archive_inventory_binds_reviewed_pin_and_original_archive() -> TestResult {
    use sha2::{Digest as _, Sha256};
    use std::{
        ffi::OsString,
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    // Cleanup happens on both a successful assertion and unwinding. Never follow
    // a substituted scratch root or recursively remove outside the owned parent.
    struct Scratch(std::path::PathBuf, std::path::PathBuf);
    impl Drop for Scratch {
        fn drop(&mut self) {
            if self.0.parent() == Some(self.1.as_path()) && super::plain_ancestors(&self.0).is_ok()
            {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
    }
    let temporary_root = fs::canonicalize(std::env::temp_dir())?;
    let root = temporary_root.join(format!(
        "Bonsai-ArchiveInventory-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    ));
    fs::create_dir(&root)?;
    let scratch = Scratch(root, temporary_root);
    let bytes = fixture(&[("source-revision/source.cpp", b"abc")])?;
    let archive_path = scratch.0.join("source.zip");
    let pin_path = scratch.0.join("pin.json");
    let pin = serde_json::json!({"schemaVersion":1,"repository":"https://example.invalid/source","revision":"a".repeat(40),"archiveRoot":"source-revision","archiveBytes":bytes.len(),"archiveSha256":super::hex(&Sha256::digest(&bytes))});
    let pin_bytes = serde_json::to_vec(&pin)?;
    fs::write(&archive_path, &bytes)?;
    fs::write(&pin_path, &pin_bytes)?;
    let arguments = [
        OsString::from("native-archive-inventory"),
        "--archive".into(),
        archive_path.into_os_string(),
        "--pin".into(),
        pin_path.clone().into_os_string(),
        "--expected-pin-sha256".into(),
        super::hex(&Sha256::digest(&pin_bytes)).into(),
    ];
    let result: serde_json::Value = serde_json::from_str(&super::run(&arguments)?)?;
    assert_eq!(result["fileCount"], 1);
    fs::write(&pin_path, b"{}")?;
    assert!(super::run(&arguments).is_err());
    fs::write(&pin_path, &pin_bytes)?;
    let mut changed = bytes;
    changed[0] ^= 1;
    fs::write(&arguments[2], &changed)?;
    assert!(super::run(&arguments).is_err());
    Ok(())
}
