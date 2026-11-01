//! Independent raw PQ2 symbol and scale census fixtures.

use super::{Harness, Wire, count, patch, require};
use serde_json::Value;

const PRISM: &str = super::super::PRISM;
const BLOCKS: usize = 1928;

struct Tensor<'a> {
    name: &'a str,
    kind: u32,
    elements: u64,
    payload: Vec<u8>,
}
struct Position {
    dimension: usize,
    offset: usize,
    data: usize,
}
struct Fixture {
    bytes: Vec<u8>,
    data: usize,
    positions: Vec<Position>,
}

fn fixture(tensors: &[Tensor<'_>]) -> crate::Result<Fixture> {
    let mut wire = Wire::header(
        u64::try_from(tensors.len()).map_err(|error| error.to_string())?,
        1,
    );
    wire.scalar_text("general.architecture", "synthetic")?;
    let mut offset = 0_usize;
    let mut positions = Vec::new();
    for tensor in tensors {
        wire.text(tensor.name)?;
        wire.u32(1);
        let dimension = wire.pos();
        wire.u64(tensor.elements);
        wire.u32(tensor.kind);
        let position = wire.pos();
        wire.u64(u64::try_from(offset).map_err(|error| error.to_string())?);
        positions.push(Position {
            dimension,
            offset: position,
            data: offset,
        });
        offset = (offset + tensor.payload.len()).div_ceil(32) * 32;
    }
    wire.align();
    let data = wire.pos();
    wire.0.resize(data + offset, 255);
    for (tensor, position) in tensors.iter().zip(&positions) {
        patch(&mut wire.0, data + position.data, &tensor.payload);
    }
    Ok(Fixture {
        bytes: wire.0,
        data,
        positions,
    })
}

fn mixed() -> crate::Result<Fixture> {
    // FP16 scale then low-first 2-bit raw codes: 0=-1, 1=0, 2=+1, 3=+2.
    let mut small = vec![0x55; 68];
    small[0] = 0;
    small[34] = 0;
    small[1] = 0x3c;
    small[35] = 0xbc;
    small[2] = 0x58;
    small[67] = 0x95;
    let mut large = vec![0x64; 34 * BLOCKS];
    for block in large.chunks_exact_mut(34) {
        block[0] = 0;
        block[1] = 0x3c;
    }
    fixture(&[
        Tensor {
            name: "fixture.small",
            kind: 142,
            elements: 256,
            payload: small,
        },
        Tensor {
            name: "fixture.large",
            kind: 142,
            elements: 128 * 1928,
            payload: large,
        },
        Tensor {
            name: "fixture.float",
            kind: 0,
            elements: 1,
            payload: vec![255; 4],
        },
        Tensor {
            name: "fixture.ptq",
            kind: 143,
            elements: 128,
            payload: vec![0; 28],
        },
    ])
}

fn mixed_census(harness: &Harness<'_>, fixture: &Fixture) -> crate::Result<Value> {
    harness.reject("stock-private-ids", &fixture.bytes, "Stock", false, None)?;
    harness.reject(
        "unpinned-dialect",
        &fixture.bytes,
        "Prism-unpinned",
        true,
        None,
    )?;
    let metadata = harness.inspect("census-metadata", &fixture.bytes, PRISM, false)?;
    require(
        metadata["TensorPayloadRead"] == false && metadata.get("Pq2Census").is_none(),
        "Metadata-only inspection read tensor payloads.",
    )?;
    require(
        metadata["TensorTypeHistogram"][1]["Name"] == "PQ2_0"
            && metadata["TensorTypeHistogram"][2]["Name"] == "PTQ1_0",
        "Pinned private type table differs.",
    )?;
    let report = harness.inspect("mixed-ternary", &fixture.bytes, PRISM, true)?;
    let census = &report["Pq2Census"];
    let first = &census["Tensors"][0];
    let second = &census["Tensors"][1];
    require(
        report["TensorPayloadRead"] == true && census["TensorCount"] == 2,
        "Actual payload census was not reported.",
    )?;
    require(
        first["CodeMinusOne"] == 1
            && first["CodeZero"] == 253
            && first["CodePlusOne"] == 2
            && first["CodePlusTwo"] == 0,
        "Low-first two-bit code census differs.",
    )?;
    require(
        first["LosslessThreeSymbolEligible"] == true
            && first["ZeroScaleBlocks"] == 0
            && first["NonfiniteScaleBlocks"] == 0,
        "Finite negative scale was incorrectly rejected.",
    )?;
    require(
        second["CodeMinusOne"] == 32 * 1928
            && second["CodeZero"] == 64 * 1928
            && second["CodePlusOne"] == 32 * 1928,
        "Census lost symbols across a read boundary.",
    )?;
    require(
        census["SerializedBytesRead"] == 68 + 34 * 1928
            && census["UnchangedOtherTensorBytes"] == 32,
        "Census scanned gaps or unrelated payloads.",
    )?;
    require(
        census["PayloadBufferBytes"] == 65_536 && census["MaximumBlocksPerRead"] == 1927,
        "Bounded payload buffer contract changed.",
    )?;
    require(
        census["Blocks"] == 2 + 1928 && census["Scales"] == census["Blocks"],
        "Block/scale counts differ.",
    )?;
    let total = count(census, "CodeMinusOne")?
        + count(census, "CodeZero")?
        + count(census, "CodePlusOne")?
        + count(census, "CodePlusTwo")?;
    require(
        total == 128 * count(census, "Blocks")?,
        "Raw code counts do not sum to validated tensor elements.",
    )?;
    Ok(report)
}

fn all_codes(harness: &Harness<'_>) -> crate::Result<()> {
    let mut payload = vec![0; 34 * 8];
    let mut expected = [0_u64; 4];
    for value in 0_u8..=255 {
        let index = usize::from(value);
        let block = index / 32;
        payload[block * 34 + 1] = 0x3c;
        payload[block * 34 + 2 + index % 32] = value;
        for shift in [0, 2, 4, 6] {
            expected[usize::from((value >> shift) & 3)] += 1;
        }
    }
    let fixture = fixture(&[Tensor {
        name: "all.codes",
        kind: 142,
        elements: 1024,
        payload,
    }])?;
    let report = harness.inspect("all-packed-bytes", &fixture.bytes, PRISM, true)?;
    let census = &report["Pq2Census"];
    for (field, value) in ["CodeMinusOne", "CodeZero", "CodePlusOne", "CodePlusTwo"]
        .into_iter()
        .zip(expected)
    {
        require(
            count(census, field)? == value,
            "Lookup census differs for a packed byte pattern.",
        )?;
    }
    require(
        census["LosslessThreeSymbolEligible"] == false,
        "Fourth symbols received three-symbol eligibility.",
    )
}

fn scales(harness: &Harness<'_>, fixture: &Fixture) -> crate::Result<()> {
    for scale in [0_u16, 0x8000, 0x7c00, 0xfc00, 0x7e00] {
        let mut bytes = fixture.bytes.clone();
        patch(&mut bytes, fixture.data, &scale.to_le_bytes());
        let report = harness.inspect(&format!("scale-{scale}"), &bytes, PRISM, true)?;
        let tensor = &report["Pq2Census"]["Tensors"][0];
        require(
            tensor["StrictTernaryCodes"] == true
                && tensor["LosslessThreeSymbolEligible"] == false
                && tensor["CodeZero"] == 253,
            "Invalid scale changed raw symbols or eligibility.",
        )?;
        let zero = scale == 0 || scale == 0x8000;
        require(
            count(tensor, "ZeroScaleBlocks")? == u64::from(zero)
                && count(tensor, "NonfiniteScaleBlocks")? == u64::from(!zero),
            "Zero/nonfinite scale classification differs.",
        )?;
    }
    Ok(())
}

fn mutations(harness: &Harness<'_>, fixture: &Fixture) -> crate::Result<()> {
    harness.reject(
        "census-truncation",
        &fixture.bytes[..fixture.data + 34],
        PRISM,
        true,
        None,
    )?;
    for (name, offset, value) in [
        ("census-overlap", fixture.positions[1].offset, 32),
        ("census-row-size", fixture.positions[0].dimension, 129),
        (
            "census-dimension-overflow",
            fixture.positions[0].dimension,
            u64::MAX,
        ),
    ] {
        let mut bytes = fixture.bytes.clone();
        patch(&mut bytes, offset, &value.to_le_bytes());
        harness.reject(name, &bytes, PRISM, true, None)?;
    }
    let stock = self::fixture(&[Tensor {
        name: "stock.q2",
        kind: 42,
        elements: 64,
        payload: vec![0; 18],
    }])?;
    let report = harness.inspect("stock-q2", &stock.bytes, "Stock", false)?;
    require(
        report["TensorTypeHistogram"][0]["Name"] == "Q2_0"
            && report["TensorTypeHistogram"][0]["Bytes"] == 18,
        "Stock type 42 changed.",
    )?;
    harness.reject("empty-census", &stock.bytes, PRISM, true, None)?;
    harness.reject("stock-census", &stock.bytes, "Stock", true, None)
}

pub(super) fn run(harness: &Harness<'_>) -> crate::Result<()> {
    let fixture = mixed()?;
    mixed_census(harness, &fixture)?;
    all_codes(harness)?;
    scales(harness, &fixture)?;
    mutations(harness, &fixture)?;
    harness.bridge("census-artifact", &fixture.bytes, PRISM, true)?;
    let path = harness.write("census-implicit-dialect", &fixture.bytes)?;
    require(super::super::run(serde_json::json!({"operation":"inspect","modelPath":path,"expectedBytes":fixture.bytes.len(),
        "expectedSha256":crate::identity::sha256(&fixture.bytes),"censusPq2":true,"ggufToolPath":harness.tool})).is_err(),
        "Entrypoint permitted an implicit Prism dialect.")
}
