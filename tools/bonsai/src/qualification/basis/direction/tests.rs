use super::{CHANNELS, compare, report, unit_direction};
use crate::Result;

fn unit_vector() -> Vec<u8> {
    let mut bytes = vec![0; CHANNELS * 4];
    bytes[..4].copy_from_slice(&1.0_f32.to_le_bytes());
    bytes
}

#[test]
fn direction_requires_exact_finite_already_normalized_vector() -> Result<()> {
    let mut direction = unit_vector();
    let (values, norm) = unit_direction(&direction)?;
    assert_eq!(norm, 1.0);
    assert_eq!(values.len(), CHANNELS);
    assert!(unit_direction(&direction[..direction.len() - 1]).is_err());
    for value in [0.0, 0.99, 2.0, f32::NAN, f32::INFINITY] {
        direction[..4].copy_from_slice(&value.to_le_bytes());
        assert!(unit_direction(&direction).is_err());
    }
    Ok(())
}

#[test]
fn direction_comparison_requires_negative_direction_and_every_channel() -> Result<()> {
    let (direction, _) = unit_direction(&unit_vector())?;
    let mut b = unit_vector();
    assert_eq!(compare(&b, &direction)?.0, 1);
    b[..4].copy_from_slice(&(-1.0_f32).to_le_bytes());
    assert_eq!(compare(&b, &direction)?.0, 0);
    b[(CHANNELS - 1) * 4..].copy_from_slice(&1.0_f32.to_le_bytes());
    assert_eq!(compare(&b, &direction)?.0, 1);
    b[..4].copy_from_slice(&f32::NAN.to_le_bytes());
    assert!(compare(&b, &direction).is_err());
    Ok(())
}

fn text(bytes: &mut Vec<u8>, value: &str) -> Result<()> {
    bytes.extend_from_slice(
        &u64::try_from(value.len())
            .map_err(|e| e.to_string())?
            .to_le_bytes(),
    );
    bytes.extend_from_slice(value.as_bytes());
    Ok(())
}

fn adapter() -> Result<(Vec<u8>, Vec<usize>)> {
    let mut targets = vec![("token_embd.weight".to_owned(), [1_u64, 248_320])];
    for block in 0..64 {
        targets.push((format!("blk.{block}.ffn_down.weight"), [17_408, 1]));
        let family = if (block + 1) % 4 == 0 {
            "attn_output"
        } else {
            "ssm_out"
        };
        targets.push((format!("blk.{block}.{family}.weight"), [6144, 1]));
    }
    let mut bytes = b"GGUF".to_vec();
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(&258_u64.to_le_bytes());
    bytes.extend_from_slice(&0_u64.to_le_bytes());
    let mut offset = 0_u64;
    let mut b_offsets = Vec::new();
    for (target, shape_a) in targets {
        for (suffix, shape) in [("lora_a", shape_a), ("lora_b", [1, 5120])] {
            text(&mut bytes, &format!("{target}.{suffix}"))?;
            bytes.extend_from_slice(&2_u32.to_le_bytes());
            for dimension in shape {
                bytes.extend_from_slice(&dimension.to_le_bytes());
            }
            bytes.extend_from_slice(&0_u32.to_le_bytes());
            bytes.extend_from_slice(&offset.to_le_bytes());
            if suffix == "lora_b" {
                b_offsets.push(usize::try_from(offset).map_err(|e| e.to_string())?);
            }
            offset += shape[0] * shape[1] * 4;
        }
    }
    let data = bytes.len().div_ceil(32) * 32;
    bytes.resize(
        data + usize::try_from(offset).map_err(|e| e.to_string())?,
        0,
    );
    for offset in &mut b_offsets {
        *offset += data;
        bytes[*offset..*offset + 4].copy_from_slice(&(-1.0_f32).to_le_bytes());
    }
    Ok((bytes, b_offsets))
}

#[test]
fn all_129_direction_factors_are_checked_and_never_qualify_provenance() -> Result<()> {
    let (mut bytes, offsets) = adapter()?;
    let valid = report(&bytes, &unit_vector())?;
    assert_eq!(valid["status"], "direction-consistent");
    let sites = valid["sites"].as_array().ok_or("Missing site reports.")?;
    assert_eq!(sites.len(), 129);
    assert!(
        sites
            .iter()
            .all(|site| site["comparedElements"] == CHANNELS)
    );
    for key in [
        "authoringProvenanceQualified",
        "writerBasisQualified",
        "defaultMayChange",
    ] {
        assert_eq!(valid[key], false);
    }
    for offset in offsets {
        let last = offset + (CHANNELS - 1) * 4;
        bytes[last..last + 4].copy_from_slice(&1.0_f32.to_le_bytes());
    }
    let rejected = report(&bytes, &unit_vector())?;
    assert_eq!(rejected["status"], "rejected-direction-comparison");
    assert_eq!(rejected["directionComparisonPassed"], false);
    assert!(
        rejected["sites"]
            .as_array()
            .ok_or("Missing rejected sites.")?
            .iter()
            .all(|site| site["mismatchedElements"] == 1)
    );
    Ok(())
}

#[test]
fn direction_reader_rejects_incomplete_or_misnamed_writer_inventory() -> Result<()> {
    let (mut bytes, _) = adapter()?;
    let name = b"blk.0.ssm_out.weight.lora_b";
    let position = bytes
        .windows(name.len())
        .position(|window| window == name)
        .ok_or("Missing fixture factor.")?;
    bytes[position + 4] = b'3';
    assert!(report(&bytes, &unit_vector()).is_err());
    Ok(())
}
