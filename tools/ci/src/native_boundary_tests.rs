//! Miniature independent file-set, tamper and qualification-refusal controls.

use super::{Entry, Exclusion, Family, Manifest, digest, ordinary_path, verify};
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let path = fs::canonicalize(std::env::temp_dir())?.join(format!(
            "Bonsai-NativeBoundary-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }

    fn fixture(&self) -> Result<Manifest, Box<dyn std::error::Error>> {
        fs::write(self.0.join("adapter.cpp"), b"native source identity")?;
        fs::write(self.0.join("CMakeLists.txt"), b"operative compiler edge")?;
        let entry = |path: &str| -> Result<Entry, Box<dyn std::error::Error>> {
            let (bytes, sha256) = digest(&self.0.join(path), super::MAX_FILE)?;
            Ok(Entry {
                path: path.into(),
                bytes,
                sha256,
                family: "reference".into(),
            })
        };
        Ok(Manifest {
            schema_version: 1,
            kind: "checked-in-native-retirement-boundary".into(),
            repository: "runner".into(),
            source_revision: "a".repeat(40),
            exclusions: [".git", ".worktrees", "target", "third_party"]
                .into_iter()
                .map(|path| Exclusion {
                    path: path.into(),
                    reason: "explicit build/vendor boundary".into(),
                })
                .collect(),
            families: vec![Family {
                id: "reference".into(),
                obligations: vec!["Preserve original assertions and independent replay.".into()],
                retirement_qualified: false,
            }],
            files: vec![entry("adapter.cpp")?],
            edges: vec![entry("CMakeLists.txt")?],
        })
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn native_boundary_binds_exact_files_edges_and_never_grants_retirement() -> TestResult {
    let scratch = Scratch::new()?;
    let manifest = scratch.fixture()?;
    let report = verify(&scratch.0, &manifest)?;
    assert_eq!(report["nativeFiles"], 1);
    assert_eq!(report["operativeAndContractEdges"], 1);
    for field in [
        "sourceExecuted",
        "gitRevisionVerified",
        "externalSourceClosureQualified",
        "compiledClosureQualified",
        "independentParityQualified",
        "nativeRetirementQualified",
    ] {
        assert_eq!(report[field], false, "{field}");
    }
    Ok(())
}

#[test]
fn native_boundary_rejects_added_deleted_changed_files_and_changed_edges() -> TestResult {
    let scratch = Scratch::new()?;
    let manifest = scratch.fixture()?;
    fs::write(scratch.0.join("unreviewed.HPP"), b"extra native header")?;
    assert!(verify(&scratch.0, &manifest).is_err());
    fs::remove_file(scratch.0.join("unreviewed.HPP"))?;
    fs::remove_file(scratch.0.join("adapter.cpp"))?;
    assert!(verify(&scratch.0, &manifest).is_err());
    fs::write(scratch.0.join("adapter.cpp"), b"changed source identity")?;
    assert!(verify(&scratch.0, &manifest).is_err());
    fs::write(scratch.0.join("adapter.cpp"), b"native source identity")?;
    fs::write(scratch.0.join("CMakeLists.txt"), b"changed compiler edge")?;
    assert!(verify(&scratch.0, &manifest).is_err());
    Ok(())
}

#[test]
fn native_boundary_rejects_qualification_flips_empty_obligations_and_hidden_sources() -> TestResult
{
    let scratch = Scratch::new()?;
    let mut manifest = scratch.fixture()?;
    manifest.families[0].retirement_qualified = true;
    assert!(verify(&scratch.0, &manifest).is_err());
    manifest.families[0].retirement_qualified = false;
    manifest.families[0].obligations.clear();
    assert!(verify(&scratch.0, &manifest).is_err());
    manifest.families[0]
        .obligations
        .push("Required independent assertions".into());
    manifest.exclusions.push(Exclusion {
        path: "adapter.cpp".into(),
        reason: "attempted omission".into(),
    });
    assert!(verify(&scratch.0, &manifest).is_err());
    manifest.exclusions.pop();
    manifest.edges[0].path = "../outside".into();
    assert!(verify(&scratch.0, &manifest).is_err());
    for path in [
        "../outside",
        "a//b",
        "C:/outside",
        "a\\b",
        "NUL.cpp",
        "a/COM1.h",
        "a/trailing.",
    ] {
        assert!(!ordinary_path(path), "{path}");
    }
    Ok(())
}

#[test]
fn native_boundary_keeps_build_vendor_and_historical_data_exclusions_explicit() -> TestResult {
    let scratch = Scratch::new()?;
    let manifest = scratch.fixture()?;
    for directory in ["target", ".worktrees", "third_party"] {
        fs::create_dir(scratch.0.join(directory))?;
        fs::write(
            scratch.0.join(directory).join("external.cpp"),
            b"excluded build or vendor source",
        )?;
    }
    fs::create_dir(scratch.0.join("evidence"))?;
    fs::write(
        scratch.0.join("evidence/native.c.txt"),
        b"historical data, not an operative source file",
    )?;
    assert!(verify(&scratch.0, &manifest).is_ok());
    fs::write(
        scratch.0.join("evidence/operative.c"),
        b"new operative source cannot hide as evidence",
    )?;
    assert!(verify(&scratch.0, &manifest).is_err());
    Ok(())
}
