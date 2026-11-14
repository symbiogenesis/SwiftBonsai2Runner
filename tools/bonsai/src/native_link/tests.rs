//! Literal link/package fixtures; these are not captures of compiler execution.

use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> std::result::Result<Self, Box<dyn std::error::Error>> {
        // The real command intentionally accepts ordinary disk paths, not
        // canonical Windows namespace paths (\\?\C:\...).
        let path = std::env::temp_dir().join(format!(
            "Bonsai-NativeLink-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }

    fn file(
        &self,
        name: &str,
        bytes: &[u8],
    ) -> std::result::Result<Identity, Box<dyn std::error::Error>> {
        let path = self.0.join(name);
        fs::write(&path, bytes)?;
        Ok(Identity {
            path,
            bytes: u64::try_from(bytes.len())?,
            sha256: crate::identity::sha256(bytes),
        })
    }

    fn document(
        &self,
        name: &str,
        value: &Value,
    ) -> std::result::Result<Identity, Box<dyn std::error::Error>> {
        self.file(name, &serde_json::to_vec(value)?)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Ok(parent) = fs::canonicalize(std::env::temp_dir())
            && let Ok(target) = fs::canonicalize(&self.0)
            && target.parent() == Some(parent.as_path())
            && fs::symlink_metadata(&self.0)
                .is_ok_and(|metadata| !bonsai_artifacts::is_redirect(&metadata))
        {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}

fn identity(value: &Identity) -> Value {
    json!({"path":value.path,"bytes":value.bytes,"sha256":value.sha256})
}

fn put16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn image() -> Vec<u8> {
    let mut bytes = vec![0; 0x1400];
    bytes[..2].copy_from_slice(b"MZ");
    put32(&mut bytes, 60, 0x80);
    put32(&mut bytes, 0x80, 0x4550);
    put16(&mut bytes, 0x84, 0xaa64);
    put16(&mut bytes, 0x86, 1);
    put16(&mut bytes, 0x94, 240);
    put16(&mut bytes, 0x98, 0x20b);
    put32(&mut bytes, 0x98 + 108, 16);
    bytes[0x188..0x18e].copy_from_slice(b".rdata");
    for offset in [0x190, 0x194, 0x198] {
        put32(&mut bytes, offset, 0x1000);
    }
    put32(&mut bytes, 0x19c, 0x400);
    put32(&mut bytes, 0x98 + 120, 0x1000);
    put32(&mut bytes, 0x98 + 124, 40);
    put32(&mut bytes, 0x40c, 0x1100);
    bytes[0x500..0x50d].copy_from_slice(b"kernel32.dll\0");
    put32(&mut bytes, 0x98 + 112 + 13 * 8, 0x1200);
    put32(&mut bytes, 0x98 + 116 + 13 * 8, 64);
    put32(&mut bytes, 0x600, 1);
    put32(&mut bytes, 0x604, 0x1300);
    bytes[0x700..0x70b].copy_from_slice(b"bcrypt.dll\0");
    bytes
}

fn fixture(scratch: &Scratch) -> std::result::Result<Capture, Box<dyn std::error::Error>> {
    let object = scratch.file("source.obj", b"literal original object bytes")?;
    let linker = scratch.file("lld-link.exe", b"literal original linker bytes")?;
    let library = scratch.file("system.lib", b"literal original import-library bytes")?;
    let source = scratch.document(
        "source-manifest.json",
        &json!({
            "schemaVersion":3,"kind":"selected-clang-native-source-closure",
            "sourceRevision":"a".repeat(40),"target":"aarch64-pc-windows-msvc",
            "configuration":"Release","compiler":{"sha256":"b".repeat(64)},
            "compilationDatabase":{"sha256":"c".repeat(64)},
            "units":[{"object":identity(&object)}]
        }),
    )?;
    let receipt = scratch.document(
        "source-receipt.json",
        &json!({
            "schemaVersion":1,"kind":"selected-native-source-closure-verification","passed":true,
            "manifestSchemaVersion":3,"recursiveGenerationProvenanceChecked":true,
            "selectedBuildInventoryChecked":true,"manifestSha256":source.sha256,
            "sourceRevision":"a".repeat(40),"target":"aarch64-pc-windows-msvc",
            "configuration":"Release","compilerSha256":"b".repeat(64),
            "compilationDatabaseSha256":"c".repeat(64),"translationUnits":1
        }),
    )?;
    fs::create_dir(scratch.0.join("package"))?;
    let output = scratch.file("linked.exe", &image())?;
    let packaged = scratch.file("package/service.exe", &image())?;
    let capture = Capture {
        schema_version: 1,
        kind: "selected-lld-arm64-link-package".into(),
        source_manifest: source,
        source_receipt: receipt,
        linker: linker.clone(),
        links: vec![Link {
            working_directory: scratch.0.clone(),
            arguments: vec![
                linker.path.to_str().ok_or("NonUTF8 scratch")?.into(),
                "/nodefaultlib".into(),
                "/machine:arm64".into(),
                "/manifest:no".into(),
                format!("/out:{}", output.path.display()),
                object.path.to_str().ok_or("NonUTF8 scratch")?.into(),
                library.path.to_str().ok_or("NonUTF8 scratch")?.into(),
            ],
            exit_code: 0,
            inputs: vec![
                Input {
                    kind: InputKind::CompiledObject,
                    identity: object,
                },
                Input {
                    kind: InputKind::Library,
                    identity: library,
                },
            ],
            output,
            package_name: "service.exe".into(),
            normal_imports: vec!["kernel32.dll".into()],
            delay_imports: vec!["bcrypt.dll".into()],
        }],
        package_directory: scratch.0.join("package"),
        package_files: vec![PackageFile {
            name: "service.exe".into(),
            bytes: packaged.bytes,
            sha256: packaged.sha256,
        }],
        system_imports: vec!["kernel32.dll".into(), "bcrypt.dll".into()],
    };
    // Every mutation control starts from a successfully admitted selection.
    // Otherwise a path/setup failure could masquerade as policy rejection.
    verify(&capture, &mut Intake::default())?;
    Ok(capture)
}

fn rejected<T>(result: Result<T>, expected: &str) {
    assert_eq!(result.err().as_deref(), Some(expected));
}

#[test]
fn native_link_literal_package_binds_all_objects_imports_and_limits_claims() -> TestResult {
    let scratch = Scratch::new()?;
    let manifest = fixture(&scratch)?;
    let receipt = verify(&manifest, &mut Intake::default())?;
    assert_eq!(receipt["translationUnits"], 1);
    assert_eq!(receipt["normalAndDelayImportsChecked"], true);
    assert_eq!(receipt["exactFlatPackageChecked"], true);
    assert_eq!(receipt["sourceClosureReexecuted"], false);
    assert_eq!(receipt["independentSourceReceiptReviewRequired"], true);
    assert_eq!(receipt["cppRuntimeAbsenceQualified"], false);
    assert_eq!(receipt["nativeRetirementQualified"], false);
    Ok(())
}

#[test]
fn native_link_explicit_invocation_rejects_hidden_inputs_and_failed_links() -> TestResult {
    let scratch = Scratch::new()?;
    let mut manifest = fixture(&scratch)?;
    for option in [
        "@response.rsp",
        "/libpath:C:/hidden",
        "/defaultlib:hidden.lib",
        "relative.obj",
        "/incremental",
    ] {
        manifest.links[0].arguments.push(option.into());
        rejected(
            verify_arguments(&manifest.links[0], &manifest.linker),
            "Native link has an unsupported or repeated option; response/search/default-library inputs are excluded.",
        );
        manifest.links[0].arguments.pop();
    }
    let ordinary = manifest.links[0].arguments.clone();
    manifest.links[0]
        .arguments
        .retain(|value| value != "/nodefaultlib");
    rejected(
        verify_arguments(&manifest.links[0], &manifest.linker),
        "Native link requires explicit input, ARM64, output and disabled implicit-library/manifest paths.",
    );
    manifest.links[0].arguments = ordinary;
    manifest.links[0].exit_code = 1;
    rejected(
        verify_arguments(&manifest.links[0], &manifest.linker),
        "Native link lacks a successful explicit original invocation.",
    );
    Ok(())
}

#[test]
fn native_link_rejects_object_omission_relabeling_and_changed_receipt() -> TestResult {
    let scratch = Scratch::new()?;
    let mut manifest = fixture(&scratch)?;
    let removed = manifest.links[0].inputs.remove(0);
    rejected(
        verify(&manifest, &mut Intake::default()),
        "Native link inputs differ from its explicit invocation.",
    );
    manifest.links[0].inputs.insert(0, removed);
    manifest.links[0].inputs[0].kind = InputKind::ToolchainObject;
    rejected(
        verify(&manifest, &mut Intake::default()),
        "Native link cannot relabel a selected compiled object.",
    );
    manifest.links[0].inputs[0].kind = InputKind::CompiledObject;
    let mut receipt: Value = serde_json::from_slice(&fs::read(&manifest.source_receipt.path)?)?;
    for field in [
        "manifestSha256",
        "compilerSha256",
        "compilationDatabaseSha256",
    ] {
        let original = receipt[field].clone();
        receipt[field] = "d".repeat(64).into();
        manifest.source_receipt = scratch.document("source-receipt.json", &receipt)?;
        rejected(
            verify(&manifest, &mut Intake::default()),
            "Native link requires the matching independently reviewed schema-3 source receipt.",
        );
        receipt[field] = original;
    }
    Ok(())
}

#[test]
fn native_link_rejects_missing_extra_or_changed_package_bytes() -> TestResult {
    let scratch = Scratch::new()?;
    let mut manifest = fixture(&scratch)?;
    scratch.file("package/extra.txt", b"unexpected")?;
    rejected(
        verify(&manifest, &mut Intake::default()),
        "Native link package has missing or extra entries.",
    );
    fs::remove_file(scratch.0.join("package/extra.txt"))?;
    let original = manifest.package_files[0].sha256.clone();
    manifest.package_files[0].sha256 = "d".repeat(64);
    rejected(
        verify(&manifest, &mut Intake::default()),
        "Artifact length or SHA-256 differs from the compiled lock.",
    );
    manifest.package_files[0].sha256 = original;
    manifest.links[0].output.sha256 = "d".repeat(64);
    rejected(
        verify(&manifest, &mut Intake::default()),
        "Artifact length or SHA-256 differs from the compiled lock.",
    );
    manifest.links[0].output.sha256 = manifest.package_files[0].sha256.clone();
    fs::remove_file(scratch.0.join("package/service.exe"))?;
    rejected(
        verify(&manifest, &mut Intake::default()),
        "Artifact storage could not be opened safely.",
    );
    Ok(())
}

#[test]
fn native_link_rejects_unreviewed_delay_imports_and_unbound_boundaries() -> TestResult {
    let scratch = Scratch::new()?;
    let mut manifest = fixture(&scratch)?;
    manifest.links[0].delay_imports.clear();
    rejected(
        verify(&manifest, &mut Intake::default()),
        "Native link PE imports differ from the reviewed inventory.",
    );
    manifest.links[0].delay_imports.push("bcrypt.dll".into());
    manifest.system_imports.pop();
    rejected(
        verify(&manifest, &mut Intake::default()),
        "Native link PE import is neither packaged nor a declared system boundary.",
    );
    manifest.system_imports.push("bcrypt.dll".into());
    manifest.system_imports.push("unused.dll".into());
    rejected(
        verify(&manifest, &mut Intake::default()),
        "Native link omits source objects/package images or declares unused system imports.",
    );
    assert!(module_set(&["../unsafe.dll".into()]).is_err());
    assert!(module_set(&["kernel32.dll".into(), "kernel32.dll".into()]).is_err());
    Ok(())
}

#[test]
fn native_link_manifest_intake_rejects_digest_tamper_and_unknown_fields() -> TestResult {
    let scratch = Scratch::new()?;
    let mut manifest = scratch.document("link.json", &json!({"schemaVersion":1,"unknown":true}))?;
    assert!(
        run(json!({"manifest":identity(&manifest)}))
            .err()
            .is_some_and(|error| error.contains("unknown field"))
    );
    manifest.sha256 = "d".repeat(64);
    rejected(
        run(json!({"manifest":identity(&manifest)})),
        "Artifact length or SHA-256 differs from the compiled lock.",
    );
    assert!(
        run(json!({"manifest":identity(&manifest),"ignoreErrors":true}))
            .err()
            .is_some_and(|error| error.contains("unknown field"))
    );
    Ok(())
}

#[test]
fn native_link_source_binding_rejects_paired_missing_and_malformed_identities() -> TestResult {
    let scratch = Scratch::new()?;
    let mut manifest = fixture(&scratch)?;
    let original_source: Value =
        serde_json::from_slice(&fs::read(&manifest.source_manifest.path)?)?;
    let original_receipt: Value =
        serde_json::from_slice(&fs::read(&manifest.source_receipt.path)?)?;
    for (source_path, receipt_key) in [
        ("/sourceRevision", "sourceRevision"),
        ("/configuration", "configuration"),
        ("/compiler/sha256", "compilerSha256"),
        ("/compilationDatabase/sha256", "compilationDatabaseSha256"),
    ] {
        for malformed in [
            None,
            Some(Value::Null),
            Some(json!(17)),
            Some(json!("")),
            Some(json!("bad\nidentity")),
        ] {
            let mut source = original_source.clone();
            let mut receipt = original_receipt.clone();
            if let Some(malformed) = malformed {
                *source
                    .pointer_mut(source_path)
                    .ok_or("Missing fixture identity")? = malformed.clone();
                receipt[receipt_key] = malformed;
            } else {
                let (parent, key) = source_path.rsplit_once('/').ok_or("Missing fixture path")?;
                source
                    .pointer_mut(parent)
                    .and_then(Value::as_object_mut)
                    .ok_or("Missing fixture object")?
                    .remove(key);
                receipt
                    .as_object_mut()
                    .ok_or("Missing fixture receipt")?
                    .remove(receipt_key);
            }
            manifest.source_manifest = scratch.document("source-manifest.json", &source)?;
            receipt["manifestSha256"] = manifest.source_manifest.sha256.clone().into();
            manifest.source_receipt = scratch.document("source-receipt.json", &receipt)?;
            rejected(
                source_objects(&manifest, &mut Intake::default()),
                "Native link requires the matching independently reviewed schema-3 source receipt.",
            );
        }
    }
    Ok(())
}
