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

// This verifier deliberately does not call the exporter's pack/unpack routines.
// Recover each base-three digit directly from the fractional native encoding,
// then demand the unique wire byte and compare the original two-bit symbol.
fn block_matches(source: &[u8], output: &[u8]) -> crate::Result<()> {
    if source.len() != 34 || output.len() != 28 || source[..2] != output[26..] {
        return Err("PTQ1 packing changed raw scale bytes or block extent.".into());
    }
    let scale = u16::from_le_bytes([source[0], source[1]]);
    if scale & 0x7fff == 0 || scale & 0x7c00 == 0x7c00 {
        return Err("PTQ1 packing requires finite nonzero raw FP16 scales.".into());
    }
    for (lane, packed) in output[..26].iter().copied().enumerate() {
        let (start, width, digits) = if lane < 16 {
            (lane, 16, 5)
        } else if lane < 24 {
            (80 + lane - 16, 8, 5)
        } else {
            (120 + lane - 24, 2, 4)
        };
        let mut remainder = u16::from(packed);
        let mut integer = 0_u16;
        for digit in 0..digits {
            remainder *= 3;
            let recovered = remainder >> 8;
            remainder &= 255;
            integer = integer * 3 + recovered;
            let index = start + digit * width;
            let original = (source[2 + index / 4] >> (2 * (index % 4))) & 3;
            if u16::from(original) != recovered {
                return Err("PTQ1 packing changed an original PQ2 symbol.".into());
            }
        }
        if digits == 4 {
            integer *= 3;
        }
        if (integer * 256).div_ceil(243) != u16::from(packed) {
            return Err("PTQ1 packing uses a noncanonical wire byte.".into());
        }
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
                    .chunks_exact(34)
                    .zip(right[..count * 28].chunks_exact(28))
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
