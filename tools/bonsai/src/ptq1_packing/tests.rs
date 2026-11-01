use std::fs::{self, File};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;

use super::{Deadline, Duration, Instant, block_matches, compare, run, verify_digest};

fn deadline() -> Deadline {
    Deadline {
        started: Instant::now(),
        allowance: Duration::from_secs(5),
    }
}

fn text(bytes: &mut Vec<u8>, value: &str) -> crate::Result<()> {
    bytes.extend(
        u64::try_from(value.len())
            .map_err(|error| error.to_string())?
            .to_le_bytes(),
    );
    bytes.extend(value.as_bytes());
    Ok(())
}

fn fixture(packed: bool) -> crate::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    bytes.extend(0x4655_4747_u32.to_le_bytes());
    bytes.extend(3_u32.to_le_bytes());
    bytes.extend(2_u64.to_le_bytes());
    bytes.extend(3_u64.to_le_bytes());
    text(&mut bytes, "general.file_type")?;
    bytes.extend(4_u32.to_le_bytes());
    bytes.extend((if packed { 143_u32 } else { 141_u32 }).to_le_bytes());
    text(&mut bytes, "tokenizer.chat_template")?;
    bytes.extend(8_u32.to_le_bytes());
    text(&mut bytes, "retained-template")?;
    text(&mut bytes, "prism.hadamard.signs")?;
    bytes.extend(9_u32.to_le_bytes());
    bytes.extend(1_u32.to_le_bytes());
    bytes.extend(3_u64.to_le_bytes());
    bytes.extend([255, 1, 255]);
    // Descriptor order differs from payload order, and source tensor gaps compact.
    for (name, extent, kind, offset) in [
        (
            "quant",
            128_u64,
            if packed { 143_u32 } else { 142_u32 },
            32_u64,
        ),
        ("bf16", 2, 30, 0),
    ] {
        text(&mut bytes, name)?;
        bytes.extend(1_u32.to_le_bytes());
        bytes.extend(extent.to_le_bytes());
        bytes.extend(kind.to_le_bytes());
        bytes.extend(offset.to_le_bytes());
    }
    bytes.resize(bytes.len().next_multiple_of(32), 0);
    bytes.extend([1, 2, 3, 4]);
    bytes.resize(bytes.len().next_multiple_of(32), 0);
    if packed {
        bytes.extend([255; 24]);
        bytes.extend([253, 253, 0, 0xbc]);
    } else {
        bytes.extend([0, 0xbc]);
        bytes.extend([0xaa; 32]);
    }
    bytes.resize(bytes.len().next_multiple_of(32), 0);
    Ok(bytes)
}

fn with_files<T>(
    source: &[u8],
    output: &[u8],
    operation: impl FnOnce(&mut File, &mut File) -> crate::Result<T>,
) -> crate::Result<T> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let path =
        std::env::temp_dir().join(format!("Bonsai-PtqPacking-{}-{nonce}", std::process::id()));
    fs::create_dir(&path).map_err(|error| error.to_string())?;
    let source_path = path.join("source.gguf");
    let output_path = path.join("output.gguf");
    fs::write(&source_path, source).map_err(|error| error.to_string())?;
    fs::write(&output_path, output).map_err(|error| error.to_string())?;
    let result = {
        let mut a = File::open(&source_path).map_err(|error| error.to_string())?;
        let mut b = File::open(&output_path).map_err(|error| error.to_string())?;
        operation(&mut a, &mut b)
    };
    fs::remove_file(source_path).map_err(|error| error.to_string())?;
    fs::remove_file(output_path).map_err(|error| error.to_string())?;
    fs::remove_dir(path).map_err(|error| error.to_string())?;
    result
}

#[test]
fn independent_packing_checks_every_wire_byte_and_scale_bit() -> crate::Result<()> {
    for (lane, start, stride, digits) in (0..16)
        .map(|lane| (lane, lane, 16, 5))
        .chain((16..24).map(|lane| (lane, 80 + lane - 16, 8, 5)))
        .chain((24..26).map(|lane| (lane, 120 + lane - 24, 2, 4)))
    {
        for byte in 0_u16..=255 {
            // Native repeated-times-three decoder oracle, independently transposed
            // into the original PQ2 bit layout for every possible first wire byte.
            let mut source = [0_u8; 34];
            source[1] = 0xbc;
            let mut output = [0_u8; 28];
            output[27] = 0xbc;
            output[lane] = u8::try_from(byte).map_err(|error| error.to_string())?;
            let mut fraction = byte;
            let mut value = 0;
            for digit in 0..digits {
                fraction *= 3;
                let trit = fraction >> 8;
                fraction &= 255;
                value = value * 3 + trit;
                let index = start + digit * stride;
                source[2 + index / 4] |=
                    u8::try_from(trit).map_err(|error| error.to_string())? << (2 * (index % 4));
            }
            if digits == 4 {
                value *= 3;
            }
            assert_eq!(
                block_matches(&source, &output).is_ok(),
                (value * 256).div_ceil(243) == byte
            );
        }
    }
    let mut source = [0xaa; 34];
    source[..2].copy_from_slice(&[0, 0xbc]);
    let mut output = [255; 28];
    output[24..].copy_from_slice(&[253, 253, 0, 0xbc]);
    block_matches(&source, &output)?;
    for index in 0..28 {
        let mut changed = output;
        changed[index] ^= 1;
        assert!(block_matches(&source, &changed).is_err(), "byte {index}");
    }
    for scale in [[0, 0], [0, 128], [0, 124], [1, 252]] {
        source[..2].copy_from_slice(&scale);
        output[26..].copy_from_slice(&scale);
        assert!(block_matches(&source, &output).is_err());
    }
    Ok(())
}

#[test]
fn independent_packing_rejects_invalid_tensor_geometry_and_array_tags() -> crate::Result<()> {
    let source = fixture(false)?;
    let output = fixture(true)?;
    let name = source
        .windows(5)
        .position(|bytes| bytes == b"quant")
        .ok_or("Missing fixture descriptor.")?;
    let signs = source
        .windows(20)
        .position(|bytes| bytes == b"prism.hadamard.signs")
        .ok_or("Missing fixture signs.")?;
    let mut cases = Vec::new();
    for extent in [0_u64, 127, u64::MAX] {
        let mut changed = source.clone();
        changed[name + 9..name + 17].copy_from_slice(&extent.to_le_bytes());
        cases.push(changed);
    }
    for offset in [1_u64, 0, u64::MAX - 31] {
        let mut changed = source.clone();
        changed[name + 21..name + 29].copy_from_slice(&offset.to_le_bytes());
        cases.push(changed);
    }
    let mut changed = source;
    changed[signs + 24..signs + 28].copy_from_slice(&999_u32.to_le_bytes());
    changed[signs + 28..signs + 36].copy_from_slice(&0_u64.to_le_bytes());
    cases.push(changed);
    for changed in cases {
        assert!(with_files(&changed, &output, |a, b| compare(a, b, &deadline())).is_err());
    }
    Ok(())
}

#[test]
fn independent_packing_rejects_metadata_payload_and_late_corruption() -> crate::Result<()> {
    let source = fixture(false)?;
    let output = fixture(true)?;
    assert_eq!(
        with_files(&source, &output, |a, b| compare(a, b, &deadline()))?,
        (1, 1, 4)
    );
    let template = output
        .windows(17)
        .position(|bytes| bytes == b"retained-template")
        .ok_or("Missing fixture template.")?;
    for index in [template, output.len() - 64, output.len() - 6] {
        let mut changed = output.clone();
        changed[index] ^= 1;
        assert!(with_files(&source, &changed, |a, b| compare(a, b, &deadline())).is_err());
    }
    let truncated = &output[..output.len() - 5];
    assert!(with_files(&source, truncated, |a, b| compare(a, b, &deadline())).is_err());
    with_files(&source, &output, |a, _| {
        verify_digest(
            a,
            u64::try_from(source.len()).map_err(|error| error.to_string())?,
            &crate::identity::sha256(&source),
            &deadline(),
        )?;
        assert!(verify_digest(a, 1, &"0".repeat(64), &deadline()).is_err());
        Ok(())
    })
}

#[test]
fn independent_packing_rejects_expired_work_before_payload_reads() -> crate::Result<()> {
    let source = fixture(false)?;
    let output = fixture(true)?;
    let expired = Deadline {
        started: Instant::now(),
        allowance: Duration::ZERO,
    };
    assert!(with_files(&source, &output, |a, b| compare(a, b, &expired)).is_err());
    for seconds in [0, 301] {
        assert!(
            run(json!({"sourcePath": "x", "outputPath": "y", "timeoutSeconds": seconds})).is_err()
        );
    }
    Ok(())
}
