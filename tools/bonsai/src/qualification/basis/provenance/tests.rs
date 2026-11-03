use serde_json::{Value, json};

use super::{Checkpoint, Conversion, Index, Toolchain, checkpoint, conversion};
use crate::Result;
use crate::identity::sha256;
use crate::qualification::basis::{Artifact, Intake};

fn documents() -> (Value, Value, Value) {
    let source = json!({"repository":"author/original", "revision":"a".repeat(40)});
    let checkpoint = json!({
        "schemaVersion":1, "source":source,
        "index":{"file":"model.safetensors.index.json", "bytes":2,"sha256":sha256(b"{}")},
        "shards":[{"file":"model-00001-of-00001.safetensors","bytes":3_100_000_000_u64,"sha256":"b".repeat(64)}],
        "ssmTensors":(0..64_u32).filter(|block|(block+1)%4 != 0).map(|block|json!({
            "block":block, "name":format!("model.language_model.layers.{block}.linear_attn.out_proj.weight"),
            "shard":"model-00001-of-00001.safetensors", "dtype":"F16", "shape":[5120,6144],
            "bytes":62_914_560, "sha256":format!("{block:064x}")
        })).collect::<Vec<_>>()
    });
    let index = json!({"metadata":{"total_size":3_100_000_000_u64},
        "weight_map": (0..64).filter(|block|(block+1)%4 != 0).map(|block|(
            format!("model.language_model.layers.{block}.linear_attn.out_proj.weight"),
            "model-00001-of-00001.safetensors"
        )).collect::<std::collections::BTreeMap<_,_>>()});
    let toolchain = json!({"schemaVersion":1,"python":"exact Python build identity",
        "numpy":"exact NumPy build identity", "safetensors":"exact safetensors build identity",
        "ggufWriter":"exact GGUF writer source identity", "matrixMultiplication":"exact F32 BLAS/build identity"});
    (checkpoint, index, toolchain)
}

fn intake() -> Result<Intake> {
    serde_json::from_value(super::super::tests::manifest()?).map_err(|error| error.to_string())
}

fn conversion_record(intake: &Intake) -> Value {
    json!({"schemaVersion":1,
        "exporter":{"source":{"repository":"author/exporter","revision":"c".repeat(40)},
            "path":"scripts/export_gguf_lora.py","bytes":123,"sha256":"d".repeat(64)},
        "invocation":["python","scripts/export_gguf_lora.py","--original-checkpoint","original"],
        "checkpointManifestSha256":intake.checkpoint_manifest.sha256,
        "toolchainManifestSha256":intake.toolchain_manifest.sha256,
        "directionSha256":intake.direction.sha256,"outputAdapterSha256":intake.adapter.sha256,
        "baseModel":{"file":"original-base.gguf","bytes":123,"sha256":"e".repeat(64)},
        "normalization":intake.normalization,"preprocessing":["Explicit original preprocessing declaration"]})
}

fn check_checkpoint(checkpoint_value: Value, index_value: Value) -> Result<()> {
    let manifest: Checkpoint =
        serde_json::from_value(checkpoint_value).map_err(|error| error.to_string())?;
    let index: Index = serde_json::from_value(index_value).map_err(|error| error.to_string())?;
    checkpoint(&manifest, &index)
}

#[test]
fn original_checkpoint_records_require_complete_canonical_index_bound_ssm_tensors() -> Result<()> {
    let (original, index, _) = documents();
    check_checkpoint(original.clone(), index.clone())?;
    for (path, value) in [
        ("/schemaVersion", json!(2)),
        ("/source/revision", json!("main")),
        ("/ssmTensors/1/block", json!(0)),
        ("/ssmTensors/0/name", json!("blk.0.ssm_out.weight")),
        ("/ssmTensors/0/shape/0", json!(6144)),
        ("/ssmTensors/0/dtype", json!("I8")),
        ("/ssmTensors/0/bytes", json!(12)),
        ("/ssmTensors/0/sha256", json!("wrong")),
        ("/ssmTensors/0/shard", json!("unlisted.safetensors")),
        ("/shards/0/file", json!("../model.safetensors")),
        ("/shards/0/bytes", json!(10)),
        ("/index/file", json!("C:/index.json")),
    ] {
        let mut changed = original.clone();
        *changed
            .pointer_mut(path)
            .ok_or("Missing checkpoint fixture field.")? = value;
        assert!(
            check_checkpoint(changed, index.clone()).is_err(),
            "Accepted {path}"
        );
    }
    let mut changed = original.clone();
    let _ = changed["ssmTensors"]
        .as_array_mut()
        .ok_or("Missing SSM array.")?
        .pop();
    assert!(check_checkpoint(changed, index.clone()).is_err());
    let mut changed = index;
    changed["weight_map"]["model.language_model.layers.0.linear_attn.out_proj.weight"] =
        json!("other.safetensors");
    assert!(check_checkpoint(original, changed).is_err());
    Ok(())
}

#[test]
fn original_conversion_records_bind_selected_inputs_output_and_exact_toolchain() -> Result<()> {
    let intake = intake()?;
    let (_, _, toolchain) = documents();
    let toolchain: Toolchain =
        serde_json::from_value(toolchain).map_err(|error| error.to_string())?;
    let original = conversion_record(&intake);
    let check = |value: Value| -> Result<()> {
        let record: Conversion =
            serde_json::from_value(value).map_err(|error| error.to_string())?;
        conversion(&intake, &record, &toolchain)
    };
    check(original.clone())?;
    for path in [
        "/checkpointManifestSha256",
        "/toolchainManifestSha256",
        "/directionSha256",
        "/outputAdapterSha256",
        "/normalization",
        "/exporter/source/revision",
        "/exporter/path",
        "/exporter/sha256",
        "/baseModel/sha256",
    ] {
        let mut changed = original.clone();
        *changed
            .pointer_mut(path)
            .ok_or("Missing conversion fixture field.")? = json!("unbound");
        // A path spelling is accepted when relative; traversal is the failure condition.
        if path == "/exporter/path" {
            changed["exporter"]["path"] = json!("../export.py");
        }
        assert!(check(changed).is_err(), "Accepted {path}");
    }
    for field in ["invocation", "preprocessing"] {
        let mut changed = original.clone();
        changed[field] = json!([]);
        assert!(check(changed).is_err());
    }
    let mut incomplete = documents().2;
    let _ = incomplete
        .as_object_mut()
        .ok_or("Missing toolchain object.")?
        .remove("matrixMultiplication");
    assert!(serde_json::from_value::<Toolchain>(incomplete).is_err());
    assert!(serde_json::from_value::<Checkpoint>(json!({})).is_err());
    Ok(())
}

fn write(directory: &std::path::Path, name: &str, value: &Value) -> Result<Artifact> {
    let bytes = serde_json::to_vec(value).map_err(|error| error.to_string())?;
    std::fs::write(directory.join(name), &bytes).map_err(|error| error.to_string())?;
    Ok(Artifact {
        file: name.into(),
        bytes: u64::try_from(bytes.len()).map_err(|error| error.to_string())?,
        sha256: sha256(&bytes),
    })
}

#[test]
fn manifest_intake_checks_raw_index_bytes_without_authenticating_declared_payloads() -> Result<()> {
    let directory =
        std::env::temp_dir().join(format!("Bonsai-Authoring-Manifests-{}", std::process::id()));
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let mut intake = intake()?;
    let (mut checkpoint, index, toolchain) = documents();
    let index_ref = write(&directory, "model.safetensors.index.json", &index)?;
    checkpoint["index"] =
        json!({"file":index_ref.file,"bytes":index_ref.bytes,"sha256":index_ref.sha256});
    intake.checkpoint_manifest = write(&directory, "checkpoint.json", &checkpoint)?;
    intake.toolchain_manifest = write(&directory, "toolchain.json", &toolchain)?;
    intake.conversion_manifest = write(&directory, "conversion.json", &conversion_record(&intake))?;
    let report = super::validate(&directory, &intake)?;
    assert_eq!(report["status"], "declared-records-consistent");
    for field in [
        "originalCheckpointPayloadsVerified",
        "authoringProvenanceQualified",
        "aFactorReproductionQualified",
    ] {
        assert_eq!(report[field], false);
    }
    std::fs::write(directory.join("model.safetensors.index.json"), b"{}")
        .map_err(|error| error.to_string())?;
    assert!(super::validate(&directory, &intake).is_err());
    for file in [
        "model.safetensors.index.json",
        "checkpoint.json",
        "toolchain.json",
        "conversion.json",
    ] {
        std::fs::remove_file(directory.join(file)).map_err(|error| error.to_string())?;
    }
    std::fs::remove_dir(directory).map_err(|error| error.to_string())?;
    Ok(())
}
