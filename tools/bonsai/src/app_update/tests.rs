use super::*;

fn installer() -> crate::Result<policy::Installer> {
    serde_json::from_value(json!({
        "schemaVersion": 1, "version": "0.1.1", "sourceRevision": "a".repeat(40),
        "target": TARGET,
        "asset": {
            "name": "SwiftBonsai2Runner-0.1.1-windows-arm64.msi",
            "url": format!("https://github.com/{REPOSITORY}/releases/download/v0.1.1/SwiftBonsai2Runner-0.1.1-windows-arm64.msi"),
            "bytes": 8, "sha256": crate::identity::sha256(b"original"),
        }
    }))
    .map_err(|error| error.to_string())
}

#[test]
fn final_stage_requires_published_identity_and_retains_immutable_ancestry() -> crate::Result<()> {
    let root = std::env::temp_dir().join(format!(
        "Bonsai-Update-Stage-{}",
        crate::startup_process::unique()
    ));
    external(&root)?;
    fs::create_dir(&root).map_err(|error| error.to_string())?;
    let published = installer()?;
    let path = root.join(&published.asset.name);
    let receipt = root.join("verified-release.json");
    let encoded = serde_json::to_vec(&published).map_err(|error| error.to_string())?;
    transfer::write_exclusive(&path, b"original")?;
    transfer::write_exclusive(&receipt, &encoded)?;
    let guard = verify_stage(&root, "0.1.1", &published)?;
    assert_eq!(guard.path(), path);
    assert!(fs::write(&path, b"tampered").is_err());
    assert!(fs::rename(&root, root.with_extension("moved")).is_err());
    drop(guard);

    // An attacker can change both the file and its cached hash. The online
    // published identity still rejects this internally consistent replacement.
    fs::write(&path, b"tampered").map_err(|error| error.to_string())?;
    let mut rewritten = serde_json::to_value(&published).map_err(|error| error.to_string())?;
    rewritten["asset"]["sha256"] = crate::identity::sha256(b"tampered").into();
    fs::write(
        &receipt,
        serde_json::to_vec(&rewritten).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let error = verify_stage(&root, "0.1.1", &published)
        .err()
        .ok_or("Accepted a rewritten installer receipt.")?;
    assert!(error.contains("differs from the published release"));

    fs::write(&receipt, &encoded).map_err(|error| error.to_string())?;
    assert!(verify_stage(&root, "0.1.1", &published).is_err());
    fs::write(&path, b"original").map_err(|error| error.to_string())?;
    assert!(verify_stage(&root, "0.1.2", &published).is_err());
    fs::remove_file(&path)
        .and_then(|()| fs::remove_file(&receipt))
        .and_then(|()| fs::remove_dir(&root))
        .map_err(|error| error.to_string())
}

#[test]
fn missing_update_handoff_is_an_error() {
    let path = std::env::temp_dir().join(format!(
        "Bonsai-Missing-Update-{}.json",
        crate::startup_process::unique()
    ));
    assert!(apply_after_exit(&path).is_err());
}
