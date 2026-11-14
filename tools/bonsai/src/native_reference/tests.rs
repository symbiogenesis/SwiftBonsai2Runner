//! Focused malformed-format and original-tolerance regression controls.

use super::{compare_bytes, decode, formats, native};

#[test]
fn native_source_manifest_extends_selection_without_relabelling_legacy() -> crate::Result<()> {
    let root = std::env::temp_dir().join(format!(
        "reference-source-{}",
        crate::startup_process::unique()
    ));
    std::fs::create_dir(&root).map_err(|error| error.to_string())?;
    let result = (|| -> crate::Result<()> {
        let repository = root.join("source");
        let evidence = root.join("evidence");
        std::fs::create_dir(&evidence).map_err(|error| error.to_string())?;
        for relative in super::public::ORIGINAL_SOURCES
            .iter()
            .chain(super::public::NATIVE_SOURCES)
        {
            let path = repository.join(relative);
            std::fs::create_dir_all(path.parent().ok_or("missing fixture parent")?)
                .map_err(|error| error.to_string())?;
            std::fs::write(path, format!("{relative}\r\n")).map_err(|error| error.to_string())?;
        }
        let revision = "a".repeat(40);
        let binding = super::public::save_sources(&repository, &evidence, &revision, false)?;
        let manifest =
            super::read_json(&evidence.join("replay-source/source-manifest.json"), 65_536)?;
        assert_eq!(manifest["schemaVersion"], 2);
        let selected = super::audit::sources(&evidence, &binding, &revision)?
            .ok_or("native source selection missing")?;
        assert_eq!(
            selected.len(),
            super::public::ORIGINAL_SOURCES.len() + super::public::NATIVE_SOURCES.len()
        );
        let native = evidence.join("replay-source/tools/bonsai/src/native_reference/native.rs");
        std::fs::write(native, b"changed").map_err(|error| error.to_string())?;
        assert!(super::audit::sources(&evidence, &binding, &revision).is_err());
        Ok(())
    })();
    std::fs::remove_dir_all(&root).map_err(|error| error.to_string())?;
    result
}
use serde_json::json;

#[test]
fn rejects_ambiguous_json_and_every_tensor_extent() -> crate::Result<()> {
    for raw in [r#"{"x":1,"x":2}"#, r#"{"x":1e999}"#, "{}{}"] {
        assert!(decode(raw.as_bytes()).is_err());
    }
    assert!(decode(&[255]).is_err());
    let valid = json!({"tensor":{"dtype":"F32","shape":[1],"data_offsets":[0,4]}});
    let raw = serde_json::to_vec(&valid).map_err(|error| error.to_string())?;
    let bytes = u64::try_from(raw.len()).map_err(|error| error.to_string())? + 12;
    assert_eq!(
        formats::tensor_header(&raw, bytes)?["tensor"]["absoluteByteEnd"],
        bytes
    );
    for (field, value) in [
        ("dtype", json!("UNKNOWN")),
        ("shape", json!([-1])),
        ("shape", json!([true])),
        ("shape", json!([1_000_000_001])),
        ("data_offsets", json!([0, 5])),
    ] {
        let mut changed = valid.clone();
        changed["tensor"][field] = value;
        assert!(
            formats::tensor_header(
                &serde_json::to_vec(&changed).map_err(|error| error.to_string())?,
                bytes
            )
            .is_err()
        );
    }
    for raw in [
        json!({"a":valid["tensor"],"b":valid["tensor"]}),
        json!({"tensor":{"dtype":"F32","shape":[1],"data_offsets":[4,8]}}),
    ] {
        let bytes = serde_json::to_vec(&raw).map_err(|error| error.to_string())?;
        assert!(
            formats::tensor_header(
                &bytes,
                u64::try_from(bytes.len()).map_err(|error| error.to_string())? + 16
            )
            .is_err()
        );
    }
    Ok(())
}

#[test]
fn keeps_original_tolerance_signed_zero_and_finite_controls() -> crate::Result<()> {
    let component = json!({"elements":2,"offset":0,"stride":2,"code":"f16"});
    let expected = [0, 60, 0, 128];
    let actual = [1.001_f32.to_le_bytes(), (-0.0_f32).to_le_bytes()].concat();
    assert_eq!(
        compare_bytes(&expected, &actual, &component, 0.001, 0.001)?["failedElements"],
        0
    );
    let actual = [1.003_f32.to_le_bytes(), 0.0_f32.to_le_bytes()].concat();
    let result = compare_bytes(&expected, &actual, &component, 0.001, 0.001)?;
    assert_eq!(result["failedElements"], 1);
    assert_eq!(result["byteExact"], false);
    let actual = [f32::NAN.to_le_bytes(), 0.0_f32.to_le_bytes()].concat();
    assert!(compare_bytes(&expected, &actual, &component, 0.001, 0.001).is_err());
    assert!(compare_bytes(&expected, &[0; 4], &component, 0.001, 0.001).is_err());
    for scale in [json!(true), json!(null), json!("0"), json!(-0.0)] {
        assert!(
            native::adapter_condition(
                &json!({"adapterLoaded":true,"adapterScale":scale}),
                "loaded-zero"
            )
            .is_err()
        );
    }
    Ok(())
}
