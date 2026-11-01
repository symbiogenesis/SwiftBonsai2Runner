//! Independent, bounded verification of the fixed current-weight PTQ1 export.

use std::fs::File;
use std::io::{Read as _, Seek as _, SeekFrom};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use bonsai_artifacts::{ArtifactError, hash_reader, open_locked_file};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::identity::sha256;

mod header;
#[cfg(test)]
mod tests;

const BUFFER_BYTES: usize = 65_536;
const BLOCKS_PER_BATCH: u64 = 1024;
const TIMEOUT: &str = "Independent PTQ1 packing verification exceeded its deadline.";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    source_path: PathBuf,
    output_path: PathBuf,
    timeout_seconds: u64,
}

struct Deadline {
    started: Instant,
    allowance: Duration,
}

impl Deadline {
    fn check(&self) -> crate::Result<()> {
        if self.started.elapsed() >= self.allowance {
            return Err(TIMEOUT.into());
        }
        Ok(())
    }
}

fn read_exact(file: &mut File, bytes: &mut [u8], deadline: &Deadline) -> crate::Result<()> {
    deadline.check()?;
    file.read_exact(bytes).map_err(|error| error.to_string())
}

fn seek(file: &mut File, position: u64) -> crate::Result<()> {
    file.seek(SeekFrom::Start(position))
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn verify_digest(
    file: &mut File,
    bytes: u64,
    hash: &str,
    deadline: &Deadline,
) -> crate::Result<()> {
    if file.metadata().map_err(|error| error.to_string())?.len() != bytes {
        return Err("PTQ1 packing artifact length differs from its compiled identity.".into());
    }
    let actual = hash_reader(file, bytes, &mut vec![0; BUFFER_BYTES], &mut || {
        deadline.check().map_err(|_| ArtifactError::Cancelled)
    })
    .map_err(|error| error.to_string())?;
    deadline.check()?;
    if actual != hash {
        return Err("PTQ1 packing artifact SHA-256 differs from its compiled identity.".into());
    }
    seek(file, 0)
}

#[derive(Clone, Copy)]
struct Digits {
    lanes: u64,
    canonical_five: bool,
    canonical_four: bool,
}

// Derive this independent table from fractional native decoding, rather than
// calling the exporter's codec. Five recovered trits occupy separate byte lanes;
// four table entries combine into five original PQ2 bytes without a digit loop.
const fn digit_table() -> [Digits; 256] {
    let mut table = [Digits {
        lanes: 0,
        canonical_five: false,
        canonical_four: false,
    }; 256];
    let mut index = 0;
    let mut byte = 0_u16;
    while index < 256 {
        let mut fraction = byte;
        let mut integer = 0;
        let mut digit = 0;
        while digit < 5 {
            fraction *= 3;
            let trit = fraction >> 8;
            fraction &= 255;
            integer = integer * 3 + trit;
            table[index].lanes |=
                u64::from_le_bytes([trit.to_le_bytes()[0], 0, 0, 0, 0, 0, 0, 0]) << (digit * 8);
            digit += 1;
        }
        table[index].canonical_five = (integer * 256).div_ceil(243) == byte;
        table[index].canonical_four = (integer / 3 * 3 * 256).div_ceil(243) == byte;
        index += 1;
        byte += 1;
    }
    table
}

const DIGITS: [Digits; 256] = digit_table();

const fn strided_five(source: &[u8], start: usize, stride: usize) -> u64 {
    u64::from_le_bytes([
        source[start],
        source[start + stride],
        source[start + stride * 2],
        source[start + stride * 3],
        source[start + stride * 4],
        0,
        0,
        0,
    ])
}

fn block_matches(source: &[u8], output: &[u8]) -> crate::Result<()> {
    if source.len() != 34 || output.len() != 28 || source[..2] != output[26..] {
        return Err("PTQ1 packing changed raw scale bytes or block extent.".into());
    }
    let scale = u16::from_le_bytes([source[0], source[1]]);
    if scale.trailing_zeros() >= 15 || scale & 0x7c00 == 0x7c00 {
        return Err("PTQ1 packing requires finite nonzero raw FP16 scales.".into());
    }
    for (first, start, stride, groups) in [(0, 2, 4, 4), (16, 22, 2, 2)] {
        for group in 0..groups {
            let offset = first + group * 4;
            let a = DIGITS[usize::from(output[offset])];
            let b = DIGITS[usize::from(output[offset + 1])];
            let c = DIGITS[usize::from(output[offset + 2])];
            let d = DIGITS[usize::from(output[offset + 3])];
            if !(a.canonical_five && b.canonical_five && c.canonical_five && d.canonical_five) {
                return Err("PTQ1 packing uses a noncanonical wire byte.".into());
            }
            let recovered = a.lanes | (b.lanes << 2) | (c.lanes << 4) | (d.lanes << 6);
            if recovered != strided_five(source, start + group, stride) {
                return Err("PTQ1 packing changed an original PQ2 symbol.".into());
            }
        }
    }
    let a = DIGITS[usize::from(output[24])];
    let b = DIGITS[usize::from(output[25])];
    if !(a.canonical_four && b.canonical_four) {
        return Err("PTQ1 packing uses a noncanonical tail wire byte.".into());
    }
    let tail = u32::from_le_bytes([
        source[32] & 15,
        source[32] >> 4,
        source[33] & 15,
        source[33] >> 4,
    ]);
    if a.lanes | (b.lanes << 2) != u64::from(tail) {
        return Err("PTQ1 packing changed an original PQ2 tail symbol.".into());
    }
    Ok(())
}

fn compare_payloads(
    source: &mut File,
    output: &mut File,
    original: &header::Header,
    packed: &header::Header,
    deadline: &Deadline,
) -> crate::Result<(u64, u64, u64)> {
    let mut blocks = 0;
    let mut tensors = 0;
    let mut copied = 0;
    let mut left = vec![0; BUFFER_BYTES];
    let mut right = vec![0; BUFFER_BYTES];
    for (a, b) in original.tensors.iter().zip(&packed.tensors) {
        seek(source, original.data_offset + a.offset)?;
        seek(output, packed.data_offset + b.offset)?;
        if a.kind == 142 {
            if b.kind != 143 {
                return Err("PTQ1 packing did not convert a PQ2 tensor.".into());
            }
            let mut remaining = a.bytes / 34;
            tensors += 1;
            blocks += remaining;
            while remaining != 0 {
                let count = usize::try_from(remaining.min(BLOCKS_PER_BATCH))
                    .map_err(|error| error.to_string())?;
                read_exact(source, &mut left[..count * 34], deadline)?;
                read_exact(output, &mut right[..count * 28], deadline)?;
                for (a, b) in left[..count * 34]
                    .as_chunks::<34>()
                    .0
                    .iter()
                    .zip(right[..count * 28].as_chunks::<28>().0)
                {
                    block_matches(a, b)?;
                }
                remaining -= u64::try_from(count).map_err(|error| error.to_string())?;
            }
        } else {
            if a.kind != b.kind || a.bytes != b.bytes {
                return Err("PTQ1 packing changed an unconverted tensor's encoding.".into());
            }
            let mut remaining = a.bytes;
            copied += remaining;
            while remaining != 0 {
                let count =
                    usize::try_from(remaining.min(65_536)).map_err(|error| error.to_string())?;
                read_exact(source, &mut left[..count], deadline)?;
                read_exact(output, &mut right[..count], deadline)?;
                if left[..count] != right[..count] {
                    return Err("PTQ1 packing changed an unconverted tensor payload.".into());
                }
                remaining -= u64::try_from(count).map_err(|error| error.to_string())?;
            }
        }
    }
    deadline.check()?;
    Ok((tensors, blocks, copied))
}

fn compare(
    source: &mut File,
    output: &mut File,
    deadline: &Deadline,
) -> crate::Result<(u64, u64, u64)> {
    let original = header::read(source, false, deadline)?;
    let packed = header::read(output, true, deadline)?;
    if original.normalized_digest != packed.normalized_digest
        || original.tensors.len() != packed.tensors.len()
        || original.data_offset != packed.data_offset
    {
        return Err("PTQ1 packing changed metadata, tensor names, shapes or header bytes.".into());
    }
    compare_payloads(source, output, &original, &packed, deadline)
}

/// Verifies every current-weight export block and all preserved bytes independently.
///
/// # Errors
/// Rejects unpinned artifacts, malformed GGUF, unsupported encodings, changed
/// metadata/payloads, noncanonical PTQ1 and an expired aggregate deadline.
pub fn run(value: Value) -> crate::Result<Value> {
    let request: Request = serde_json::from_value(value).map_err(|error| error.to_string())?;
    if !(1..=300).contains(&request.timeout_seconds) {
        return Err("PTQ1 packing requires an aggregate deadline of 1..300 seconds.".into());
    }
    let deadline = Deadline {
        started: Instant::now(),
        allowance: Duration::from_secs(request.timeout_seconds),
    };
    let receipt: Value =
        serde_json::from_str(include_str!("../../../config/swift.ptq1-export.json"))
            .map_err(|error| error.to_string())?;
    let mut source = open_locked_file(&request.source_path).map_err(|error| error.to_string())?;
    let mut output = open_locked_file(&request.output_path).map_err(|error| error.to_string())?;
    for (file, bytes, hash) in [
        (
            &mut source,
            &receipt["SourceBytes"],
            &receipt["SourceSha256"],
        ),
        (
            &mut output,
            &receipt["OutputBytes"],
            &receipt["OutputSha256"],
        ),
    ] {
        verify_digest(
            file,
            bytes
                .as_u64()
                .ok_or("Invalid compiled PTQ1 byte identity.")?,
            hash.as_str()
                .ok_or("Invalid compiled PTQ1 digest identity.")?,
            &deadline,
        )?;
    }
    let (tensors, blocks, copied) = compare(&mut source, &mut output, &deadline)?;
    if Some(tensors) != receipt["ConvertedTensorCount"].as_u64()
        || Some(blocks) != receipt["ConvertedBlocks"].as_u64()
    {
        return Err("PTQ1 packing inventory differs from the compiled complete export.".into());
    }
    Ok(json!({
        "schemaVersion": 1, "kind": "independent-current-weight-ptq1-packing",
        "status": "matched", "sourceSha256": receipt["SourceSha256"],
        "sourceBytes": receipt["SourceBytes"], "outputSha256": receipt["OutputSha256"],
        "outputBytes": receipt["OutputBytes"], "convertedTensors": tensors,
        "reconstructedBlocks": blocks, "unchangedOtherTensorBytes": copied,
        "exportReceiptSha256": sha256(include_bytes!("../../../config/swift.ptq1-export.json")),
        "headerBytesPreservedExceptDeclaredEdits": true,
        "workingPayloadBufferBytes": BUFFER_BYTES * 2,
        "elapsedSeconds": deadline.started.elapsed().as_secs_f64(),
        "scope": "Independent complete weight-symbol, raw-scale, metadata and non-PQ tensor-byte verification. No activation, logits, state, adapter-basis, quality or performance qualification; no default selection."
    }))
}
