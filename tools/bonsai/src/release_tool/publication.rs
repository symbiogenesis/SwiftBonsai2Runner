//! Offline checks for authoritative GitHub release responses; no remote writes.
use super::{Result, text, version};

mod driver;

pub(super) fn publish(value: &Value) -> Result<Value> {
    driver::publish(value)
}
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub(super) fn response(text: &str) -> Result<Value> {
    if text.trim().is_empty() || text.len() > 1024 * 1024 {
        return Err("Release API response empty or oversized.".into());
    }
    let value: Value = serde_json::from_str(text).map_err(|error| error.to_string())?;
    if !value.is_object() {
        return Err("Release API response must be one object.".into());
    }
    Ok(value)
}
fn id(value: &Value) -> Result<u64> {
    value
        .as_u64()
        .filter(|id| *id > 0 && *id <= i64::MAX.unsigned_abs())
        .ok_or_else(|| "Release API object ID must be a positive integer.".into())
}

pub(super) fn checked_response(value: &Value) -> Result<Value> {
    let result = response(text(value, "text")?)?;
    if let Some(expected) = value["expectedId"].as_u64()
        && id(&result["id"])? != expected
    {
        return Err("Release identity differs from catalog entry.".into());
    }
    Ok(result)
}

pub(super) fn tag_step(value: &Value) -> Result<Value> {
    if value["depth"]
        .as_u64()
        .is_none_or(|depth| depth == 0 || depth > 8)
    {
        return Err("Release tag nesting exceeds bound.".into());
    }
    let object = &value["object"];
    match text(object, "type")? {
        "tag"
            if value["depth"].as_u64() != Some(8)
                && crate::identity::is_lower_hex(text(object, "sha")?, 40) =>
        {
            Ok(object.clone())
        }
        "commit" if object["sha"] == value["sourceRevision"] => Ok(Value::Null),
        _ => Err("Remote release tag points to different source.".into()),
    }
}

pub(super) fn catalog(value: &Value) -> Result<Value> {
    let raw = text(value, "text")?;
    let tag = text(value, "tag")?;
    if raw.len() > 1024 * 1024 {
        return Err("Release catalog exceeds size bound.".into());
    }
    let mut matched = None;
    for (index, line) in raw.lines().filter(|line| !line.is_empty()).enumerate() {
        if index >= 1024 {
            return Err("Release catalog exceeds inventory bound.".into());
        }
        let item = response(line)?;
        let item_id = id(&item["id"])?;
        if text(&item, "tag_name")? == tag && matched.replace(item_id).is_some() {
            return Err("Duplicate releases exist for tag.".into());
        }
    }
    Ok(json!(matched))
}

pub(super) fn tag(value: &Value) -> Result<Value> {
    let raw = text(value, "text")?;
    let tag = text(value, "tag")?;
    let revision = text(value, "sourceRevision")?;
    version(tag, revision)?;
    if raw.len() > 1024 * 1024 {
        return Err("Release tag response oversized.".into());
    }
    let refs: Vec<Value> = serde_json::from_str(raw).map_err(|error| error.to_string())?;
    if refs.len() > 1024 {
        return Err("Release tag inventory invalid.".into());
    }
    let exact: Vec<_> = refs
        .iter()
        .filter(|item| item["ref"] == format!("refs/tags/{tag}"))
        .collect();
    match exact.as_slice() {
        [] if value["requireExisting"] != true => Ok(Value::Null),
        [reference] => {
            let object = &reference["object"];
            match text(object, "type")? {
                "tag" if crate::identity::is_lower_hex(text(object, "sha")?, 40) => {
                    Ok(object.clone())
                }
                "commit" if object["sha"] == revision => Ok(Value::Null),
                _ => Err("Remote release tag points to different source.".into()),
            }
        }
        _ => Err("Published release tag missing or duplicated.".into()),
    }
}

pub(super) fn validate(value: &Value) -> Result<()> {
    let release = &value["release"];
    let expected = id(&release["id"])?;
    let tag = text(value, "tag")?;
    let revision = text(value, "sourceRevision")?;
    let repository = text(value, "repository")?;
    version(tag, revision)?;
    let expected_draft = value["expectedDraft"]
        .as_bool()
        .ok_or("Missing draft policy.")?;
    let prerelease = value["prerelease"]
        .as_bool()
        .ok_or("Missing prerelease policy.")?;
    if value["expectedId"]
        .as_u64()
        .is_some_and(|id| id > 0 && id != expected)
        || release["tag_name"] != tag
        || release["target_commitish"] != revision
        || release["name"] != format!("Swift Bonsai 2 Runner {tag}")
        || release["html_url"] != format!("https://github.com/{repository}/releases/tag/{tag}")
        || text(release, "body")?.replace("\r\n", "\n")
            != text(value, "notes")?.replace("\r\n", "\n")
        || release["draft"].as_bool() != Some(expected_draft)
        || release["prerelease"].as_bool() != Some(prerelease)
    {
        return Err("Remote release source, notes or metadata differs; existing releases are never replaced.".into());
    }
    let expected = value["assets"]
        .as_object()
        .ok_or("Missing release asset policy.")?;
    let assets = release["assets"]
        .as_array()
        .filter(|assets| {
            assets.len() <= expected.len()
                && (value["complete"] != true || assets.len() == expected.len())
        })
        .ok_or("Remote release asset inventory invalid or incomplete.")?;
    let mut seen = BTreeSet::new();
    let mut ids = BTreeSet::new();
    for asset in assets {
        let name = text(asset, "name")?;
        let policy = expected.get(name).ok_or("Unknown release asset.")?;
        if !seen.insert(name)
            || !ids.insert(id(&asset["id"])?)
            || asset["state"] != "uploaded"
            || asset["size"].as_u64().is_none()
            || asset["size"] != policy["Bytes"]
            || asset["digest"] != format!("sha256:{}", text(policy, "Sha256")?)
        {
            return Err(
                "Remote release has duplicate, incomplete or changed asset; no asset overwritten."
                    .into(),
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn release_catalog_rejects_duplicates_noninteger_ids_and_oversize() -> Result<()> {
        assert_eq!(
            catalog(&json!({"text":"{\"id\":1,\"tag_name\":\"v1.0.0\"}","tag":"v1.0.0"}))?,
            json!(1)
        );
        for raw in [
            "{\"id\":1.0,\"tag_name\":\"v1.0.0\"}".into(),
            "{\"id\":1,\"tag_name\":\"v1.0.0\"}\n{\"id\":2,\"tag_name\":\"v1.0.0\"}".into(),
            " ".repeat(1_048_577),
        ] {
            assert!(catalog(&json!({"text":raw,"tag":"v1.0.0"})).is_err());
        }
        Ok(())
    }

    #[test]
    fn release_publication_rejects_changed_source_notes_metadata_and_duplicate_assets() -> Result<()>
    {
        let asset = json!({"id":2,"name":"application.exe","state":"uploaded","size":5,"digest":format!("sha256:{}","b".repeat(64))});
        let request = json!({"repository":"example/repo","tag":"v1.0.0","sourceRevision":"a".repeat(40),"notes":"notes","prerelease":true,"expectedDraft":true,"complete":true,"expectedId":1,
            "assets":{"application.exe":{"Bytes":5,"Sha256":"b".repeat(64)}},"release":{"id":1,"tag_name":"v1.0.0","target_commitish":"a".repeat(40),"name":"Swift Bonsai 2 Runner v1.0.0","html_url":"https://github.com/example/repo/releases/tag/v1.0.0","body":"notes","draft":true,"prerelease":true,"assets":[asset]}});
        validate(&request)?;
        for (field, changed) in [
            ("id", json!(2)),
            ("tag_name", json!("v1.1.0")),
            ("target_commitish", json!("c".repeat(40))),
            ("body", json!("changed")),
            ("draft", json!(false)),
            ("prerelease", json!(false)),
            ("assets", json!([asset, asset])),
        ] {
            let mut modified = request.clone();
            modified["release"][field] = changed;
            assert!(validate(&modified).is_err(), "Accepted changed {field}");
        }
        Ok(())
    }
}
