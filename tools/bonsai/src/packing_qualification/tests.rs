//! Independent receipt mutations and hosted-admission controls.
use super::*;

fn baseline() -> BTreeMap<String, String> {
    [
        ("GITHUB_ACTIONS", "true"),
        ("RUNNER_ENVIRONMENT", "github-hosted"),
        ("GITHUB_REPOSITORY", "symbiogenesis/SwiftBonsai2Runner"),
        ("GITHUB_REF", "refs/heads/main"),
        ("RUNNER_ARCH", "ARM64"),
        ("GITHUB_RUN_ID", "123"),
        ("GITHUB_RUN_ATTEMPT", "1"),
    ]
    .into_iter()
    .map(|(name, value)| (name.into(), value.into()))
    .chain([
        ("GITHUB_SHA".into(), "a".repeat(40)),
        ("BONSAI_TOOLS_SOURCE_ID".into(), "b".repeat(64)),
    ])
    .collect()
}

#[test]
fn hosted_context_rejects_every_unbound_identity_and_owner_device() -> crate::Result<()> {
    let valid = baseline();
    context(true, &valid)?;
    assert!(context(false, &valid).is_err());
    for name in valid.keys() {
        let mut changed = valid.clone();
        changed.insert(name.clone(), "unbound".into());
        assert!(context(true, &changed).is_err(), "{name}");
        changed.remove(name);
        assert!(context(true, &changed).is_err(), "{name}");
    }
    for (name, value) in [
        ("RUNNER_ENVIRONMENT", "self-hosted"),
        ("GITHUB_REF", "refs/pull/1/merge"),
        ("GITHUB_RUN_ATTEMPT", "0"),
        ("GITHUB_RUN_ATTEMPT", "01"),
        ("GITHUB_RUN_ID", "18446744073709551616"),
    ] {
        let mut changed = valid.clone();
        changed.insert(name.into(), value.into());
        assert!(context(true, &changed).is_err());
    }
    Ok(())
}

#[test]
fn complete_packing_and_export_fields_are_strictly_bound() -> crate::Result<()> {
    let expected: Value = serde_json::from_str(EXPECTED).map_err(|error| error.to_string())?;
    let digest = crate::identity::sha256(EXPECTED.as_bytes());
    let fields = [
        "SchemaVersion",
        "CodecCommit",
        "SourceBytes",
        "OutputBytes",
        "SourceSha256",
        "OutputSha256",
        "SourceRevision",
        "ConvertedTensorCount",
        "ConvertedBlocks",
        "ExactPq2ReconstructionVerified",
        "NonPq2TensorBytesCopiedVerbatim",
        "TokenizerMetadataSha256",
    ];
    // Match the released CLI receipt shape; the curated pin's filenames and
    // repository annotations are deliberately absent from the exporter result.
    let export: Value = fields
        .iter()
        .map(|name| ((*name).into(), expected[*name].clone()))
        .collect::<serde_json::Map<_, _>>()
        .into();
    let packing = json!({"schemaVersion":1, "kind":"independent-current-weight-ptq1-packing", "status":"matched",
        "sourceBytes":expected["SourceBytes"], "sourceSha256":expected["SourceSha256"],
        "outputBytes":expected["OutputBytes"], "outputSha256":expected["OutputSha256"],
        "convertedTensors":expected["ConvertedTensorCount"], "reconstructedBlocks":expected["ConvertedBlocks"],
        "exportReceiptSha256":digest, "headerBytesPreservedExceptDeclaredEdits":true,
        "unchangedOtherTensorBytes":57_767_936, "workingPayloadBufferBytes":131_072});
    matches(&expected, &export, &packing, &digest)?;
    for field in fields {
        let mut changed = export.clone();
        changed[field] = "unbound".into();
        assert!(
            matches(&expected, &changed, &packing, &digest).is_err(),
            "{field}"
        );
        changed
            .as_object_mut()
            .ok_or("Missing object.")?
            .remove(field);
        assert!(
            matches(&expected, &changed, &packing, &digest).is_err(),
            "{field}"
        );
    }
    for field in packing.as_object().ok_or("Missing packing.")?.keys() {
        let mut changed = packing.clone();
        changed[field] = "unbound".into();
        assert!(
            matches(&expected, &export, &changed, &digest).is_err(),
            "{field}"
        );
        changed
            .as_object_mut()
            .ok_or("Missing object.")?
            .remove(field);
        assert!(
            matches(&expected, &export, &changed, &digest).is_err(),
            "{field}"
        );
    }
    let mut changed = packing.clone();
    changed["headerBytesPreservedExceptDeclaredEdits"] = "true".into();
    assert!(matches(&expected, &export, &changed, &digest).is_err());
    assert!(matches(&expected, &export, &packing, "missing").is_err());
    Ok(())
}

#[test]
fn retained_bounds_fit_the_owning_workflow_without_retries() -> crate::Result<()> {
    let limits = run(json!({"operation":"limits"}))?;
    assert_eq!(limits["retries"], 0);
    assert_eq!(limits["originalDownloadSeconds"], 1800);
    assert_eq!(limits["runtimeDownloadSeconds"], 120);
    assert_eq!(limits["exporterProcessSeconds"], 600);
    assert_eq!(limits["independentVerificationSeconds"], 300);
    assert_eq!(limits["independentProcessSeconds"], 305);
    let maximum = SOURCE_SECONDS + RUNTIME_SECONDS + EXPORT_SECONDS + SCAN_PROCESS_SECONDS;
    assert!(maximum + 120 <= count(&limits, "workflowStepMinutes")? * 60);
    assert!(count(&limits, "workflowStepMinutes")? + 10 <= count(&limits, "workflowJobMinutes")?);
    let workflow = include_str!("../../../../.github/workflows/ptq1-packing-qualification.yml");
    assert!(
        workflow
            .lines()
            .any(|line| line == "    timeout-minutes: 60")
    );
    assert!(
        workflow
            .lines()
            .any(|line| line == "        timeout-minutes: 50")
    );
    assert!(run(json!({"operation":"execute", "dataDirectory":"relative", "evidenceDirectory":"relative", "unexpected":true})).is_err());
    Ok(())
}

#[test]
fn data_and_uploaded_receipts_cannot_alias_through_windows_path_case() {
    assert!(overlapping(Path::new("C:/Data"), Path::new("c:\\data")));
    assert!(overlapping(
        Path::new("C:/Data"),
        Path::new("c:/DATA/evidence")
    ));
    assert!(overlapping(Path::new("C:/Data/work"), Path::new("c:/data")));
    assert!(!overlapping(
        Path::new("C:/Data"),
        Path::new("c:/data-evidence")
    ));
}

#[test]
fn atomic_receipts_preserve_prior_evidence_and_bounded_partial_observations() -> crate::Result<()> {
    let root = std::env::temp_dir().join(format!(
        "Bonsai-PackingReceipt-{}",
        crate::startup_process::unique()
    ));
    fs::create_dir(&root).map_err(|error| error.to_string())?;
    let path = root.join("qualification.json");
    save(&path, &json!({"status":"preparing"}))?;
    save(&path, &json!({"status":"failed"}))?;
    let previous = fs::read(&path).map_err(|error| error.to_string())?;
    assert!(save(&path, &json!({"oversized":"x".repeat(MAX_RECEIPT + 1)})).is_err());
    assert_eq!(
        fs::read(&path).map_err(|error| error.to_string())?,
        previous
    );
    assert!(save(&root, &json!({"status":"failed"})).is_err());
    let own_prefix = format!("{}.", root.file_name().ok_or("Missing fixture name.")?.to_string_lossy());
    assert!(
        !fs::read_dir(root.parent().ok_or("Missing parent.")?)
            .map_err(|error| error.to_string())?
            .any(|entry| entry.is_ok_and(|entry| entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "tmp")
                && entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(&own_prefix)))
    );
    let payload = root.join("transfer.partial");
    assert_eq!(partial(&payload, 3)["status"], "absent");
    fs::write(&payload, [1, 2, 3]).map_err(|error| error.to_string())?;
    let observation = partial(&payload, 3);
    assert_eq!(observation["partialBytes"], 3);
    assert!(
        !observation
            .to_string()
            .contains(&payload.to_string_lossy().to_string())
    );
    assert_eq!(partial(&payload, 2)["status"], "not-regular-or-oversized");
    assert_eq!(partial(&root, 3)["status"], "not-regular-or-oversized");
    fs::remove_dir_all(root).map_err(|error| error.to_string())
}
