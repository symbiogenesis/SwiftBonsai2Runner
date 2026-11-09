use serde_json::{Value, json};

use super::{CHANNELS, Intake, Mapping, artifact, compare, header, validate, vector};
use crate::Result;
use crate::identity::sha256;

pub(super) fn manifest() -> Result<Value> {
    let runtime: Value = serde_json::from_str(include_str!(
        "../../../../../config/prism-inference.lock.json"
    ))
    .map_err(|error| error.to_string())?;
    let reference = json!({"file":"input.json", "bytes":2, "sha256":sha256(b"{}")});
    Ok(
        json!({"schemaVersion":2,"modelSha256":super::MODEL_SHA256,"runtimeCommit":runtime["commit"],
        "adapter":{"file":"adapter.gguf","bytes":super::ADAPTER_BYTES,"sha256":super::ADAPTER_SHA256},
        "checkpointManifest":reference,"conversionManifest":reference,"toolchainManifest":reference,
        "direction":reference,"normalization":"F32 L2 normalization before reproduction",
        "mapping":{"nativeToAuthoring":(0..CHANNELS).collect::<Vec<_>>(),"signs":vec![1;CHANNELS]},
        "absoluteTolerance":0.000_01,"relativeTolerance":0.000_01,
        "ssmSites":(0..64).filter(|block|(block+1)%4 != 0).map(|block|json!({"block":block,"authoringA":reference})).collect::<Vec<_>>()}),
    )
}

#[test]
fn intake_requires_all_sites_exact_identities_mapping_and_tight_bounds() -> Result<()> {
    let original = manifest()?;
    validate(
        &serde_json::from_value::<Intake>(original.clone()).map_err(|error| error.to_string())?,
    )?;
    for path in [
        "/schemaVersion",
        "/ssmSites/1/block",
        "/mapping/nativeToAuthoring/1",
        "/mapping/signs/0",
        "/absoluteTolerance",
        "/runtimeCommit",
        "/adapter/sha256",
    ] {
        let mut bad = original.clone();
        *bad.pointer_mut(path).ok_or("Missing basis fixture path.")? = match path {
            "/schemaVersion" => json!(1),
            "/absoluteTolerance" => json!(0.1),
            "/runtimeCommit" | "/adapter/sha256" => json!("0".repeat(64)),
            _ => json!(0),
        };
        let decoded: Intake = serde_json::from_value(bad).map_err(|error| error.to_string())?;
        assert!(
            validate(&decoded).is_err(),
            "Accepted malformed field {path}"
        );
    }
    Ok(())
}

#[test]
fn comparator_checks_every_signed_permuted_channel_and_rejects_nonfinite() -> Result<()> {
    let mut authoring = vec![0; CHANNELS * 4];
    authoring[4..8].copy_from_slice(&2.0_f32.to_le_bytes());
    let mut shipped = vec![0; CHANNELS * 4];
    shipped[..4].copy_from_slice(&(-2.0_f32).to_le_bytes());
    let mut mapping = Mapping {
        native_to_authoring: (0..CHANNELS).collect(),
        signs: vec![1; CHANNELS],
    };
    mapping.native_to_authoring.swap(0, 1);
    mapping.signs[0] = -1;
    assert_eq!(compare(&shipped, &authoring, &mapping, 0.0, 0.0)?.0, 0);
    shipped[(CHANNELS - 1) * 4..].copy_from_slice(&1.0_f32.to_le_bytes());
    assert_eq!(compare(&shipped, &authoring, &mapping, 0.0, 0.0)?.0, 1);
    shipped[..4].copy_from_slice(&f32::NAN.to_le_bytes());
    assert!(vector(&shipped).is_err());
    assert!(vector(&authoring[..authoring.len() - 1]).is_err());
    Ok(())
}

#[test]
fn artifact_intake_hashes_actual_bounded_bytes_and_rejects_escape() -> Result<()> {
    let directory =
        std::env::temp_dir().join(format!("Bonsai-Basis-Intake-{}", std::process::id()));
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let path = directory.join("input.json");
    std::fs::write(&path, b"{}").map_err(|error| error.to_string())?;
    let reference = |file: &str, digest: &str| super::Artifact {
        file: file.into(),
        bytes: 2,
        sha256: digest.into(),
    };
    assert_eq!(
        artifact(&directory, &reference("input.json", &sha256(b"{}")), 2)?,
        b"{}"
    );
    assert!(artifact(&directory, &reference("input.json", &sha256(b"[]")), 2).is_err());
    assert!(artifact(&directory, &reference("input.json", &sha256(b"{}")), 1).is_err());
    assert!(artifact(&directory, &reference("../input.json", &sha256(b"{}")), 2).is_err());
    std::fs::remove_file(path).map_err(|error| error.to_string())?;
    std::fs::remove_dir(directory).map_err(|error| error.to_string())?;
    Ok(())
}

fn text(bytes: &mut Vec<u8>, value: &str) -> Result<()> {
    bytes.extend_from_slice(
        &u64::try_from(value.len())
            .map_err(|error| error.to_string())?
            .to_le_bytes(),
    );
    bytes.extend_from_slice(value.as_bytes());
    Ok(())
}

#[test]
fn independent_gguf_reader_checks_named_a_payloads_and_overlap() -> Result<()> {
    let ssm = (0_u32..64)
        .filter(|block| !(block + 1).is_multiple_of(4))
        .map(|block| {
            (
                format!("blk.{block}.ssm_out.weight.lora_a"),
                6144_u64,
                24_576_u64,
            )
        });
    let other = (48..258).map(|index| (format!("test.{index}"), 1_u64, 32_u64));
    let mut bytes = b"GGUF".to_vec();
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(&258_u64.to_le_bytes());
    bytes.extend_from_slice(&0_u64.to_le_bytes());
    let mut offset = 0_u64;
    let mut offset_positions = Vec::new();
    for (name, dimension, extent) in ssm.chain(other) {
        text(&mut bytes, &name)?;
        bytes.extend_from_slice(&2_u32.to_le_bytes());
        bytes.extend_from_slice(&dimension.to_le_bytes());
        bytes.extend_from_slice(&1_u64.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        offset_positions.push(bytes.len());
        bytes.extend_from_slice(&offset.to_le_bytes());
        offset += extent;
    }
    let data = bytes.len().div_ceil(32) * 32;
    bytes.resize(
        data + usize::try_from(offset).map_err(|error| error.to_string())?,
        0,
    );
    bytes[data..data + 4].copy_from_slice(&3.5_f32.to_le_bytes());
    let parsed = header::ssm_vectors(&bytes)?;
    assert_eq!(parsed.len(), 48);
    assert_eq!(
        parsed.get(&0).ok_or("Missing block zero.")?[..4],
        3.5_f32.to_le_bytes()
    );
    assert!(header::ssm_vectors(&bytes[..=data]).is_err());
    let mut overlap = bytes.clone();
    let second_offset = offset_positions[1];
    overlap[second_offset..second_offset + 8].copy_from_slice(&0_u64.to_le_bytes());
    assert!(header::ssm_vectors(&overlap).is_err());
    let mut wrong = bytes;
    wrong[4..8].copy_from_slice(&2_u32.to_le_bytes());
    assert!(header::ssm_vectors(&wrong).is_err());
    Ok(())
}
