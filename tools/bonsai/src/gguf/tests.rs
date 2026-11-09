//! Entry policy checks require no external inference release or model.
use super::*;
use serde_json::json;

fn model() -> PathBuf {
    std::env::temp_dir().join("fixture.gguf")
}

#[test]
fn inspection_preserves_reference_pin_and_requires_complete_explicit_identity() -> crate::Result<()>
{
    let request: Inspect =
        serde_json::from_value(json!({"modelPath":model()})).map_err(|error| error.to_string())?;
    let arguments = inspect_arguments(&request)?;
    let reference = pin(REFERENCE)?;
    assert!(arguments.contains(&count(&reference["model"], "bytes")?.to_string().into()));
    assert!(arguments.contains(&text(&reference["model"], "sha256")?.into()));
    assert_eq!(request.timeout_seconds, 300);
    for value in [
        json!({"expectedBytes":1}),
        json!({"expectedSha256":"a".repeat(64)}),
        json!({"expectedBytes":0,"expectedSha256":"a".repeat(64)}),
        json!({"expectedBytes":1,"expectedSha256":"wrong"}),
        json!({"censusPq2":true}),
        json!({"dialect":"Prism-unpinned"}),
    ] {
        let mut value = value;
        value["modelPath"] = json!(model());
        let request: Inspect = serde_json::from_value(value).map_err(|error| error.to_string())?;
        assert!(inspect_arguments(&request).is_err());
    }
    let (bytes, digest) = artifact(Some(10), Some(&"A".repeat(64)), REFERENCE)?;
    assert_eq!(bytes, 10);
    assert_eq!(digest, "a".repeat(64));
    Ok(())
}

#[test]
fn export_uses_swift_pin_and_forwards_the_whole_process_limit() -> crate::Result<()> {
    let output = std::env::temp_dir().join(format!(
        "new-export-{}.gguf",
        crate::startup_process::unique()
    ));
    let request: Export = serde_json::from_value(
        json!({"modelPath":model(),"outputPath":output,"timeoutSeconds":19}),
    )
    .map_err(|error| error.to_string())?;
    let arguments = export_arguments(&request)?;
    let swift = pin(SWIFT)?;
    assert!(arguments.contains(&text(&swift["model"], "sha256")?.into()));
    assert!(
        arguments
            .array_windows()
            .any(|pair| *pair == [OsString::from("--timeout-seconds"), OsString::from("19")])
    );
    assert!(execute(None, None, &[], 0).is_err());
    assert!(execute(None, None, &[], 3601).is_err());
    assert!(
        run(json!({"operation":"inspect","modelPath":model(),"allowUnverified":true})).is_err()
    );
    Ok(())
}
