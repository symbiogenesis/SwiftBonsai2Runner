use super::*;
use serde_json::json;

fn release(version: &str) -> Value {
    let asset = |name: &str, bytes| {
        json!({"name": name, "size": bytes,
        "digest": format!("sha256:{}", "a".repeat(64)), "state": "uploaded",
        "browser_download_url": canonical_asset(version, name)})
    };
    json!({"tag_name": format!("v{version}"), "draft": false, "prerelease": true,
        "published_at": "2026-11-12T00:00:00Z",
        "html_url": format!("https://github.com/{REPOSITORY}/releases/tag/v{version}"),
        "assets": [asset("release-manifest.json", 512), asset("SHA256SUMS.txt", 256),
            asset(&format!("SwiftBonsai2Runner-{version}-windows-arm64.msi"), 4096)]})
}

fn selected(releases: Value) -> crate::Result<(bool, Option<Candidate>)> {
    select(
        &serde_json::to_vec(&releases).map_err(|error| error.to_string())?,
        "0.1.0",
    )
}

fn fixture() -> crate::Result<Candidate> {
    selected(json!([release("0.1.1")]))?
        .1
        .ok_or_else(|| "Expected newer fixture release.".into())
}

fn manifest(candidate: &Candidate) -> Value {
    json!({"schemaVersion": 1, "version": candidate.version,
        "tag": format!("v{}", candidate.version), "target": TARGET,
        "sourceRevision": "b".repeat(40), "installerSmoke": "passed-install-inspect-uninstall-without-model",
        "assets": [{"name": candidate.msi.name, "bytes": candidate.msi.bytes, "sha256": candidate.msi.sha256}]})
}

#[test]
fn versions_compare_numerically_and_reject_noncanonical_or_out_of_msi_bounds() -> crate::Result<()>
{
    assert!(Version::parse("0.10.0")? > Version::parse("0.9.99")?);
    assert_eq!(Version::parse("255.255.65535")?, Version(255, 255, 65535));
    for invalid in [
        "",
        "v0.1.1",
        "0.01.1",
        "00.1.1",
        "0.1",
        "0.1.1.0",
        "0.1.2-beta",
        "256.0.0",
        "0.256.0",
        "0.0.65536",
        "0.1.-1",
        "../0.1.1",
    ] {
        assert!(Version::parse(invalid).is_err(), "accepted {invalid}");
    }
    assert!(
        Version::parse(env!("CARGO_PKG_VERSION"))?
            .newer_than_current()
            .is_err()
    );
    Ok(())
}

#[test]
fn discovery_handles_empty_current_draft_and_numeric_prerelease_ordering() -> crate::Result<()> {
    let (published, candidate) = selected(json!([]))?;
    assert!(!published);
    assert!(candidate.is_none());
    let mut draft = release("0.1.99");
    draft["draft"] = true.into();
    let (_, candidate) = selected(json!([
        release("0.1.2"),
        draft,
        release("0.1.10"),
        release("0.1.0")
    ]))?;
    assert_eq!(
        candidate.ok_or("Missing update fixture.")?.version,
        "0.1.10"
    );
    let (published, candidate) = selected(json!([release("0.1.0")]))?;
    assert!(published);
    assert!(candidate.is_none());
    assert!(select(b"not-json", "0.1.0").is_err());
    assert!(selected(json!({"releases": []})).is_err());
    Ok(())
}

#[test]
fn discovery_rejects_foreign_urls_wrong_arch_missing_digests_and_duplicate_assets()
-> crate::Result<()> {
    for mutation in 0..5 {
        let mut value = release("0.1.1");
        match mutation {
            0 => value["html_url"] = "https://github.com/other/repo/releases/tag/v0.1.1".into(),
            1 => value["assets"][2]["name"] = "SwiftBonsai2Runner-0.1.1-windows-x64.msi".into(),
            2 => value["assets"][2]["digest"] = Value::Null,
            3 => {
                value["assets"][2]["browser_download_url"] = "https://example.com/update.msi".into()
            }
            _ => {
                let repeated = value["assets"][2].clone();
                value["assets"]
                    .as_array_mut()
                    .ok_or("Missing fixture assets.")?
                    .push(repeated);
            }
        }
        assert!(selected(json!([value])).is_err());
    }
    Ok(())
}

#[test]
fn discovery_selects_before_asset_validation_and_never_falls_back() -> crate::Result<()> {
    let mut incomplete = release("0.1.1");
    incomplete["assets"] = json!([]);
    for values in [
        json!([incomplete, release("0.1.2")]),
        json!([release("0.1.2"), incomplete]),
    ] {
        assert_eq!(
            selected(values)?.1.ok_or("Missing newest fixture.")?.version,
            "0.1.2"
        );
    }
    let mut invalid_newest = release("0.1.3");
    invalid_newest["assets"][2]["digest"] = Value::Null;
    for values in [
        json!([invalid_newest, release("0.1.2")]),
        json!([release("0.1.2"), invalid_newest]),
    ] {
        assert!(selected(values).is_err());
    }
    Ok(())
}

#[test]
fn discovery_rejects_ambiguous_repeated_published_versions() {
    let mut changed = release("0.1.1");
    changed["assets"][2]["digest"] = format!("sha256:{}", "b".repeat(64)).into();
    assert!(selected(json!([release("0.1.1"), changed])).is_err());
    assert!(selected(json!([changed, release("0.1.1")])).is_err());
    assert!(selected(json!([release("0.1.1"), release("0.1.1")])).is_err());
}

#[test]
fn manifest_requires_same_version_source_target_size_and_three_way_checksum() -> crate::Result<()> {
    let candidate = fixture()?;
    let checksum = format!("{}  {}\n", candidate.msi.sha256, candidate.msi.name);
    let valid = manifest(&candidate);
    let encoded = serde_json::to_vec(&valid).map_err(|error| error.to_string())?;
    let verified = installer(&candidate, &encoded, checksum.as_bytes())?;
    verified.verify_source(
        &serde_json::to_vec(&json!({"sha": "b".repeat(40)})).map_err(|error| error.to_string())?,
    )?;
    assert!(
        verified
            .verify_source(
                &serde_json::to_vec(&json!({"sha": "c".repeat(40)}))
                    .map_err(|error| error.to_string())?
            )
            .is_err()
    );
    for (key, value) in [
        ("target", json!("x86_64-pc-windows-msvc")),
        ("version", json!("0.1.2")),
        ("sourceRevision", json!("main")),
        ("installerSmoke", json!("pending")),
    ] {
        let mut invalid = valid.clone();
        invalid[key] = value;
        assert!(
            installer(
                &candidate,
                &serde_json::to_vec(&invalid).map_err(|error| error.to_string())?,
                checksum.as_bytes()
            )
            .is_err()
        );
    }
    let mut invalid = valid.clone();
    invalid["assets"][0]["bytes"] = 4097.into();
    assert!(
        installer(
            &candidate,
            &serde_json::to_vec(&invalid).map_err(|error| error.to_string())?,
            checksum.as_bytes()
        )
        .is_err()
    );
    let wrong_checksum = format!("{}  {}\n", "c".repeat(64), candidate.msi.name);
    assert!(installer(&candidate, &encoded, wrong_checksum.as_bytes()).is_err());
    assert!(
        installer(
            &candidate,
            &encoded,
            format!("{checksum}{checksum}").as_bytes()
        )
        .is_err()
    );
    assert!(
        installer(
            &candidate,
            &encoded,
            format!("{}  ../update.msi\n", "a".repeat(64)).as_bytes()
        )
        .is_err()
    );
    Ok(())
}
