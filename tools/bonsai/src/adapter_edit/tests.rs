//! Weight-edit selection, numerical reporting and immutable publication checks.
#![expect(clippy::float_cmp, reason = "Exact synthetic coefficients and energies are regression identities.")]

use super::*;

fn bytes(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|value| value.to_le_bytes()).collect()
}

fn fixture() -> Result<Vec<u8>> {
    fn text(bytes: &mut Vec<u8>, value: &str) -> Result<()> {
        bytes.extend_from_slice(&u64::try_from(value.len()).map_err(|error| error.to_string())?.to_le_bytes());
        bytes.extend_from_slice(value.as_bytes());
        Ok(())
    }
    let mut targets = vec![("token_embd.weight".to_owned(), [1_u64, 248_320])];
    for block in 0_u32..64 {
        targets.push((format!("blk.{block}.ffn_down.weight"), [17_408, 1]));
        let family = if (block + 1).is_multiple_of(4) { "attn_output" } else { "ssm_out" };
        targets.push((format!("blk.{block}.{family}.weight"), [6144, 1]));
    }
    let mut bytes = b"GGUF".to_vec();
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(&258_u64.to_le_bytes());
    bytes.extend_from_slice(&0_u64.to_le_bytes());
    let mut offset = 0_u64;
    for (target, shape_a) in targets {
        for (suffix, shape) in [("lora_a", shape_a), ("lora_b", [1, 5120])] {
            text(&mut bytes, &format!("{target}.{suffix}"))?;
            bytes.extend_from_slice(&2_u32.to_le_bytes());
            for dimension in shape { bytes.extend_from_slice(&dimension.to_le_bytes()); }
            bytes.extend_from_slice(&0_u32.to_le_bytes());
            bytes.extend_from_slice(&offset.to_le_bytes());
            offset += shape[0] * shape[1] * 4;
        }
    }
    let data = bytes.len().div_ceil(32) * 32;
    bytes.resize(data + usize::try_from(offset).map_err(|error| error.to_string())?, 0);
    for (_, range) in crate::qualification::linear_a_ranges(&bytes)? {
        let values = &mut bytes[range];
        for (index, value) in values.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let coefficient = u16::try_from(index % 4 + 1).map_err(|error| error.to_string())?;
            *value = f32::from(coefficient).to_le_bytes();
        }
    }
    Ok(bytes)
}

#[test]
fn edit_preserves_all_non_linear_a_bytes_and_reports_complete_inventory() -> Result<()> {
    let source = fixture()?;
    let (candidate, summary) = edited(&source, 1, 2)?;
    assert_eq!(summary["linearSites"], 128);
    assert_eq!(summary["families"].as_array().ok_or("Missing families.")?.len(), 3);
    assert_eq!(summary["retainedSquaredL2Fraction"], 25.0 / 30.0);
    assert_eq!(candidate.len(), source.len());
    let mut ranges = crate::qualification::linear_a_ranges(&source)?;
    ranges.sort_unstable_by_key(|(_, range)| range.start);
    let mut previous = 0;
    for (_, range) in ranges {
        assert_eq!(candidate[previous..range.start], source[previous..range.start]);
        let values = candidate[range.clone()].as_chunks::<4>().0;
        assert_eq!(values.iter().filter(|value| f32::from_le_bytes(**value) != 0.0).count(), values.len()/2);
        previous = range.end;
    }
    assert_eq!(candidate[previous..], source[previous..]);
    let mut malformed = source;
    malformed[0] = 0;
    assert!(edited(&malformed, 1, 2).is_err());
    Ok(())
}

#[test]
fn magnitude_pruning_keeps_largest_values_and_stable_ties() -> Result<()> {
    let source = bytes(&[1.0, -4.0, 2.0, -4.0, 0.0, -0.0]);
    let result = prune(&source, 1, 3)?;
    assert_eq!(result.bytes, bytes(&[0.0, -4.0, 0.0, -4.0, 0.0, -0.0]));
    assert_eq!(result.original_nonzero, 4);
    assert_eq!(result.retained_nonzero, 2);
    assert_eq!(result.changed, 2);
    assert_eq!(result.original_energy, 37.0);
    assert_eq!(result.removed_energy, 5.0);
    assert_eq!(prune(&bytes(&[-4.0, 4.0]), 1, 2)?.bytes, bytes(&[-4.0, 0.0]));
    Ok(())
}

#[test]
fn pruning_uses_ceiling_and_preserves_original_bits_for_retained_values() -> Result<()> {
    let source = bytes(&[f32::MIN_POSITIVE, -2.0, 1.0]);
    assert_eq!(prune(&source, 1, 2)?.bytes, bytes(&[0.0, -2.0, 1.0]));
    assert_eq!(prune(&source, 1, 1)?.bytes, source);
    assert_eq!(retained_energy(0.0, 0.0), 1.0);
    for invalid in [vec![], vec![0], bytes(&[f32::NAN]), bytes(&[f32::INFINITY])] {
        assert!(prune(&invalid, 1, 2).is_err());
    }
    Ok(())
}

#[test]
fn edited_weights_reject_unknown_fields_invalid_fractions_and_untrusted_sources() -> Result<()> {
    let scratch = std::env::temp_dir().join(format!("bonsai-adapter-intake-{}", std::process::id()));
    fs::write(&scratch, b"untrusted").map_err(|error| error.to_string())?;
    let base = json!({"schemaVersion":1,"operation":"inspect","adapterPath":scratch,
        "keepNumerator":1,"keepDenominator":2});
    assert!(run(base.clone()).is_err());
    for (key, value) in [
        ("unexpected", json!(true)), ("keepNumerator", json!(0)),
        ("keepDenominator", json!(0)), ("keepDenominator", json!(1025)),
        ("keepNumerator", json!(3)), ("schemaVersion", json!(2)),
        ("outputPath", json!(scratch)),
    ] {
        let mut request = base.clone();
        request[key] = value;
        assert!(run(request).is_err());
    }
    fs::remove_file(scratch).map_err(|error| error.to_string())
}

#[test]
fn edited_adapter_publication_never_overwrites_existing_output() -> Result<()> {
    let directory = std::env::temp_dir().join(format!("bonsai-adapter-publish-{}", std::process::id()));
    fs::create_dir(&directory).map_err(|error| error.to_string())?;
    let path = directory.join("candidate.gguf");
    let result: Result<()> = (|| {
        publish(&path, b"first")?;
        assert!(publish(&path, b"second").is_err());
        assert_eq!(fs::read(&path).map_err(|error| error.to_string())?, b"first");
        Ok(())
    })();
    let cleanup = fs::remove_file(path).and_then(|()| fs::remove_dir(directory));
    result?;
    cleanup.map_err(|error| error.to_string())
}
