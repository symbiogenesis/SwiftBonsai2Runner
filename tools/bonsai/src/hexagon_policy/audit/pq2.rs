//! Raw PQ2/Q2 layout control, independent of device packing implementations.
use super::{NATIVE, Request, add, bytes, integer, mul, require};
use crate::{Result, identity};
use serde_json::{Value, json};
use std::path::Path;

pub(super) fn half(bits: u16) -> Result<f32> {
    let sign = u32::from(bits & 0x8000) << 16;
    let exponent = u32::from((bits >> 10) & 31);
    let fraction = bits & 1023;
    match exponent {
        31 => Err("Nonfinite FP16 scale.".into()),
        0 => Ok(f32::from_bits(
            (f32::from(fraction) * (1.0 / 16_777_216.0)).to_bits() | sign,
        )),
        _ => Ok(f32::from_bits(
            sign | ((exponent + 112) << 23) | (u32::from(fraction) << 13),
        )),
    }
}
fn extent(data: &[u8], columns: u64, rows: u64, block: u64, bound: usize) -> Result<()> {
    require(
        columns > 0 && columns.is_multiple_of(128) && rows > 0,
        "Positive rows and columns divisible by 128 required.",
    )?;
    require(
        data.len() <= bound
            && u64::try_from(data.len()).map_err(|error| error.to_string())?
                == mul(mul(columns / 128, rows)?, block)?,
        "Payload extent does not match complete packed rows.",
    )
}
pub(super) fn bridge(data: &[u8], columns: u64, rows: u64) -> Result<Vec<u8>> {
    extent(data, columns, rows, 34, 1_048_576)?;
    let mut result = Vec::with_capacity(data.len() / 34 * 36);
    for block in data.as_chunks::<34>().0 {
        half(u16::from_le_bytes([block[0], block[1]]))?;
        result.extend_from_slice(&block[..18]);
        result.extend_from_slice(&block[..2]);
        result.extend_from_slice(&block[18..]);
    }
    Ok(result)
}
pub(super) fn inverse(data: &[u8], columns: u64, rows: u64) -> Result<Vec<u8>> {
    extent(data, columns, rows, 36, 1_110_240)?;
    let mut result = Vec::with_capacity(data.len() / 36 * 34);
    for block in data.as_chunks::<36>().0 {
        half(u16::from_le_bytes([block[0], block[1]]))?;
        require(block[..2] == block[18..20], "Paired raw scale bits differ.")?;
        result.extend_from_slice(&block[..18]);
        result.extend_from_slice(&block[20..]);
    }
    Ok(result)
}
pub(super) fn decoded(data: &[u8], block_bytes: usize) -> Result<Vec<u32>> {
    require(
        [18, 34].contains(&block_bytes) && data.len().is_multiple_of(block_bytes),
        "Incomplete packed block.",
    )?;
    let mut result = Vec::new();
    for block in data.chunks_exact(block_bytes) {
        let scale = half(u16::from_le_bytes([block[0], block[1]]))?;
        for byte in &block[2..] {
            for shift in [0, 2, 4, 6] {
                result.push((f32::from(i16::from((byte >> shift) & 3) - 1) * scale).to_bits());
            }
        }
    }
    Ok(result)
}
pub(super) fn payload(data: &[u8], columns: u64, rows: u64) -> Result<Value> {
    let bridged = bridge(data, columns, rows)?;
    require(
        inverse(&bridged, columns, rows)? == data,
        "Raw layout roundtrip failed.",
    )?;
    require(
        decoded(data, 34)? == decoded(&bridged, 18)?,
        "Decoded f32 bits differ.",
    )?;
    let mut codes = [0_u64; 4];
    for block in data.as_chunks::<34>().0 {
        for byte in &block[2..] {
            for shift in [0, 2, 4, 6] {
                codes[usize::from((byte >> shift) & 3)] += 1;
            }
        }
    }
    Ok(
        json!({"Columns":columns,"Rows":rows,"SourceBytes":data.len(),"BridgedBytes":bridged.len(),"SourceSha256":identity::sha256(data),"BridgedSha256":identity::sha256(&bridged),"RawCodeCounts":codes,"RawRoundtrip":"exact","DecodedF32Bits":"exact"}),
    )
}
pub(super) fn storage(metadata: &Value, census: &Value) -> Result<Value> {
    require(
        integer(metadata, "SchemaVersion")? == 1 && integer(census, "SchemaVersion")? == 1,
        "Unsupported inventory schema.",
    )?;
    require(
        metadata["Format"] == "GGUF" && metadata["ByteOrder"] == "little",
        "Expected little-endian GGUF metadata.",
    )?;
    require(
        metadata["ArtifactSha256"]
            .as_str()
            .is_some_and(|digest| identity::is_lower_hex(digest, 64)),
        "Invalid artifact digest.",
    )?;
    for record in [metadata, census] {
        for name in ["ModelRepository", "ModelRevision"] {
            require(
                record[name].as_str().is_some_and(|text| {
                    !text.is_empty()
                        && text.encode_utf16().count() <= 512
                        && !text.chars().any(|ch| ch < ' ' || ch == '\u{7f}')
                }),
                "Invalid string identity.",
            )?;
        }
    }
    for name in ["ArtifactSha256", "ModelRepository", "ModelRevision"] {
        require(
            metadata[name] == census[name],
            "Inventory identity mismatch.",
        )?;
    }
    require(
        census["CodecCommit"] == NATIVE && metadata["Dialect"] == format!("Prism-{NATIVE}"),
        "Inventory codec differs from pinned native source.",
    )?;
    let file = integer(metadata, "FileBytes")?;
    let blocks = integer(census, "Blocks")?;
    let elements = integer(census, "Elements")?;
    let source = integer(census, "SerializedBytesRead")?;
    require(
        file == integer(census, "ArtifactBytes")?
            && blocks > 0
            && elements == mul(blocks, 128)?
            && source == mul(blocks, 34)?,
        "PQ2 inventory geometry mismatch.",
    )?;
    let counts = ["CodeMinusOne", "CodeZero", "CodePlusOne", "CodePlusTwo"]
        .map(|name| integer(census, name))
        .into_iter()
        .collect::<Result<Vec<_>>>()?;
    require(
        counts.iter().try_fold(0, |sum, value| add(sum, *value))? == elements
            && integer(census, "NonfiniteScaleBlocks")? == 0
            && integer(census, "ZeroScaleBlocks")? <= blocks,
        "Invalid raw-code or scale census.",
    )?;
    let histogram = metadata["TensorTypeHistogram"]
        .as_array()
        .ok_or("Missing tensor histogram.")?;
    let mut matches = Vec::new();
    for entry in histogram {
        if integer(entry, "TypeId")? == 142 {
            matches.push(entry);
        }
    }
    require(
        matches.len() == 1,
        "Expected one PQ2 tensor histogram entry.",
    )?;
    let pq2 = matches[0];
    let tensors = integer(census, "TensorCount")?;
    require(
        tensors > 0
            && tensors <= blocks
            && pq2["Name"] == "PQ2_0"
            && integer(pq2, "Bytes")? == source
            && integer(pq2, "Elements")? == elements
            && integer(pq2, "Count")? == tensors,
        "Tensor histogram and census disagree.",
    )?;
    let transform = json!({"prism.hadamard.version":1,"prism.hadamard.block_size":1024,"prism.hadamard.transform":"normalized-sylvester-walsh-hadamard","prism.hadamard.axis":"input-last-dimension","prism.hadamard.sign_mode":"explicit","prism.hadamard.gdn_v_grouped":true});
    for (name, value) in transform.as_object().ok_or("Invalid transform contract.")? {
        require(
            metadata["Scalars"][name] == *value,
            "Unsupported or missing Swift transform identity.",
        )?;
    }
    require(
        integer(&metadata["Arrays"]["prism.hadamard.sign_values"], "Count")? > 0 && file >= source,
        "Missing sign inventory or invalid artifact extent.",
    )?;
    let bridged = mul(blocks, 36)?;
    let estimated = add(file - source, bridged)?;
    Ok(
        json!({"ArtifactSha256":metadata["ArtifactSha256"],"InventoryOnly":true,"Pq2Blocks":blocks,"Q2Blocks":mul(blocks,2)?,"Elements":elements,"SourcePackedBytes":source,"BridgedPackedBytes":bridged,"AdditionalPackedBytes":bridged-source,"PackedExpansionNumerator":18,"PackedExpansionDenominator":17,"ArtifactBytesBeforeAlignmentChanges":estimated,"RawCodeCounts":counts,"FourthSymbolSupported":true,"TransformContract":transform,"SignIdentityCoverage":"inventory summaries only; actual sign bytes remain to be bound"}),
    )
}
pub(super) fn report(root: &Path, request: &Request) -> Result<Value> {
    require(
        request.payload.is_some() == (request.columns != 0)
            && (request.rows == 1 || request.payload.is_some()),
        "Payload and columns must be supplied together; rows requires payload.",
    )?;
    let metadata = request
        .metadata
        .clone()
        .unwrap_or_else(|| root.join("config/swift.metadata.json"));
    let census = request
        .census
        .clone()
        .unwrap_or_else(|| root.join("config/swift.pq2-census.json"));
    let parse = |path: &Path| {
        super::super::json::parse_bounded(
            std::str::from_utf8(&bytes(path, 1_048_576)?).map_err(|error| error.to_string())?,
            1_048_576,
            64,
        )
    };
    let mut report = json!({"SchemaVersion":1,"Qualification":"weight-layout-audit-only","SourceCommit":NATIVE,"Sources":[format!("https://github.com/PrismML-Eng/llama.cpp/blob/{NATIVE}/ggml/src/ggml-common.h"),format!("https://github.com/PrismML-Eng/llama.cpp/blob/{NATIVE}/ggml/src/ggml-quants.c")],"MetadataSha256":identity::sha256(&bytes(&metadata,1_048_576)?),"CensusSha256":identity::sha256(&bytes(&census,1_048_576)?),"StoragePlan":storage(&parse(&metadata)?,&parse(&census)?)?,"Limitations":["Weight layout only; no NPU dispatch, signed deployment or speed qualification.","Q2_0 Q8_0 activation arithmetic does not establish PQ2 Q8_K/odd-group parity.","No GGUF rewrite; transform/sign arrays, tensor names and grouped basis stay unchanged.","Model payload, logits, complete state, adapters and lifecycle remain unqualified.","Storage totals exclude alignment changes, simultaneous source copies and device scratch."]});
    if let Some(path) = &request.payload {
        report["Fixture"] = payload(&bytes(path, 1_048_576)?, request.columns, request.rows)?;
    }
    Ok(report)
}
