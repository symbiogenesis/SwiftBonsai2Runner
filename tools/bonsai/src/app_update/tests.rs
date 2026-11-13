use super::*;

fn installer(version: &str) -> crate::Result<policy::Installer> {
    serde_json::from_value(json!({
        "schemaVersion": 1, "version": version, "sourceRevision": "a".repeat(40),
        "target": TARGET,
        "asset": {
            "name": format!("SwiftBonsai2Runner-{version}-windows-arm64.msi"),
            "url": format!("https://github.com/{REPOSITORY}/releases/download/v{version}/SwiftBonsai2Runner-{version}-windows-arm64.msi"),
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
    let version = Version::parse(env!("CARGO_PKG_VERSION"))?.next_for_test()?;
    let other_version = Version::parse(&version)?.next_for_test()?;
    let published = installer(&version)?;
    let path = root.join(&published.asset.name);
    let receipt = root.join("verified-release.json");
    let encoded = serde_json::to_vec(&published).map_err(|error| error.to_string())?;
    transfer::write_exclusive(&path, b"original")?;
    transfer::write_exclusive(&receipt, &encoded)?;
    let guard = verify_stage(&root, &version, &published)?;
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
    let error = verify_stage(&root, &version, &published)
        .err()
        .ok_or("Accepted a rewritten installer receipt.")?;
    assert!(error.contains("differs from the published release"));

    fs::write(&receipt, &encoded).map_err(|error| error.to_string())?;
    assert!(verify_stage(&root, &version, &published).is_err());
    fs::write(&path, b"original").map_err(|error| error.to_string())?;
    assert!(verify_stage(&root, &other_version, &published).is_err());
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

#[test]
fn release_catalog_requires_complete_pages_and_retains_later_newest_release() -> crate::Result<()> {
    let mut calls = Vec::new();
    let bytes = collect_releases(|page| {
        calls.push(page);
        serde_json::to_vec(&if page == 1 {
            vec![json!({"tag_name": "v0.1.0"}); 100]
        } else {
            vec![json!({"tag_name": "v0.9.0"})]
        })
        .map_err(|error| error.to_string())
    })?;
    let entries: Vec<Value> = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    assert_eq!(calls, vec![1, 2]);
    assert_eq!(entries.len(), 101);
    assert_eq!(entries[100]["tag_name"], "v0.9.0");
    assert!(collect_releases(|_| Ok(b"{}".to_vec())).is_err());
    assert!(
        collect_releases(
            |_| serde_json::to_vec(&vec![json!({}); 101]).map_err(|error| error.to_string())
        )
        .is_err()
    );
    let mut calls = 0;
    assert!(
        collect_releases(|_| {
            calls += 1;
            serde_json::to_vec(&vec![json!({}); 100]).map_err(|error| error.to_string())
        })
        .is_err()
    );
    assert_eq!(calls, 3);
    assert!(
        collect_releases(|page| if page == 1 {
            serde_json::to_vec(&vec![json!({}); 100]).map_err(|error| error.to_string())
        } else {
            Err("offline".into())
        })
        .is_err()
    );
    Ok(())
}
