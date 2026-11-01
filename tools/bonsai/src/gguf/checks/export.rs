//! Independent streamed-export wire oracle, including raw negative scale bits.

use super::{Harness, Wire, count, patch, require};
use serde_json::Value;
use std::{fs, path::Path};

struct Fixture {
    bytes: Vec<u8>,
    data: usize,
    last: usize,
    blocks: usize,
}

fn fixture(blocks: usize, file_type: u32) -> crate::Result<Fixture> {
    let mut wire = Wire::header(3, 4);
    wire.scalar_text("general.architecture", "synthetic")?;
    wire.scalar("general.file_type", file_type)?;
    wire.scalar_text("tokenizer.chat_template", "{{ unchanged }}")?;
    wire.scalar_text("prism.hadamard.transform", "unchanged-transform")?;
    let last = (64 + 34 * blocks).div_ceil(32) * 32 + 32;
    for (name, elements, kind, offset) in [
        ("last.pq", 128, 142, last),
        ("first.bf16", 2, 30, 0),
        ("large.pq", 128 * blocks, 142, 64),
    ] {
        wire.text(name)?;
        wire.u32(1);
        wire.u64(u64::try_from(elements).map_err(|error| error.to_string())?);
        wire.u32(kind);
        wire.u64(u64::try_from(offset).map_err(|error| error.to_string())?);
    }
    wire.align();
    let data = wire.pos();
    wire.0.resize(data + last + 64, 0xef);
    patch(&mut wire.0, data, &[1, 2, 3, 4]);
    let mut block = [0xaa; 34];
    block[0] = 0;
    block[1] = 0x3c;
    for index in 0..blocks {
        patch(&mut wire.0, data + 64 + 34 * index, &block);
    }
    block[1] = 0xbc;
    patch(&mut wire.0, data + last, &block);
    Ok(Fixture {
        bytes: wire.0,
        data,
        last,
        blocks,
    })
}

fn invoke(
    harness: &Harness<'_>,
    name: &str,
    bytes: &[u8],
    output: &Path,
) -> crate::Result<crate::startup_process::ProcessOutput> {
    let path = harness.write(name, bytes)?;
    super::super::invoke(
        harness.tool,
        &[
            "export-ptq1".into(),
            "--model".into(),
            path.into_os_string(),
            "--output".into(),
            output.as_os_str().to_owned(),
            "--expected-bytes".into(),
            bytes.len().to_string().into(),
            "--expected-sha256".into(),
            crate::identity::sha256(bytes).into(),
            "--timeout-seconds".into(),
            "30".into(),
        ],
        harness.directory,
        30,
    )
}

fn oracle(fixture: &Fixture) -> Vec<u8> {
    let last = (32 + 28 * fixture.blocks).div_ceil(32) * 32;
    let mut payload = vec![0; (last + 28).div_ceil(32) * 32];
    patch(&mut payload, 0, &[1, 2, 3, 4]);
    let mut block = [255; 28];
    block[24] = 253;
    block[25] = 253;
    block[26] = 0;
    block[27] = 0x3c;
    for index in 0..fixture.blocks {
        patch(&mut payload, 32 + 28 * index, &block);
    }
    block[27] = 0xbc;
    patch(&mut payload, last, &block);
    payload
}

fn checks(
    result: &Value,
    original: &Value,
    converted: &Value,
    fixture: &Fixture,
    bytes: &[u8],
) -> crate::Result<()> {
    require(
        result["ConvertedBlocks"] == serde_json::json!(fixture.blocks + 1)
            && result["ConvertedTensorCount"] == 2
            && result["ExactPq2ReconstructionVerified"] == true,
        "Not every PQ2 block was verified.",
    )?;
    require(
        converted["Scalars"]["general.file_type"] == 143
            && converted["TensorTypeHistogram"][1]["TypeId"] == 143,
        "PTQ1 file/tensor types differ.",
    )?;
    require(
        converted["TokenizerMetadataSha256"] == original["TokenizerMetadataSha256"]
            && converted["Scalars"]["prism.hadamard.transform"] == "unchanged-transform",
        "Tokenizer/template/transform metadata changed.",
    )?;
    require(
        result["OutputSha256"] == crate::identity::sha256(bytes)
            && result["OutputBytes"] == serde_json::json!(bytes.len()),
        "Export receipt differs from written bytes.",
    )?;
    require(
        count(result, "WorkingBufferBytes")? < 256 * 1024,
        "Exporter working buffers exceed their allowance.",
    )?;
    require(
        bytes.get(fixture.data..fixture.data + 4) == Some(&[1, 2, 3, 4]),
        "Non-PQ tensor bytes changed.",
    )?;
    require(
        bytes.get(fixture.data..) == Some(oracle(fixture).as_slice()),
        "Compacted payload differs from independent wire bytes, raw scales or zero padding.",
    )?;
    require(
        count(result, "OutputBytes")? < count(result, "SourceBytes")?,
        "Synthetic export did not compact tensor payloads/gaps.",
    )
}

pub(super) fn repack(harness: &Harness<'_>) -> crate::Result<()> {
    let fixture = fixture(1928, 141)?;
    let target = harness.directory.join("export.gguf");
    let result = super::super::decode(invoke(harness, "export-source", &fixture.bytes, &target)?)?;
    let bytes = fs::read(target).map_err(|error| error.to_string())?;
    let original = harness.inspect(
        "export-original-inspect",
        &fixture.bytes,
        super::super::PRISM,
        false,
    )?;
    let converted = harness.inspect(
        "export-converted-inspect",
        &bytes,
        super::super::PRISM,
        false,
    )?;
    checks(&result, &original, &converted, &fixture, &bytes)
}

fn rejection_result(output: crate::startup_process::ProcessOutput) -> crate::Result<()> {
    require(
        output.code == 1 && output.stdout.is_empty() && !output.stderr.is_empty(),
        "Expected exporter rejection rather than crash, timeout or success.",
    )
}

pub(super) fn rejection(harness: &Harness<'_>) -> crate::Result<()> {
    let mut fixture = fixture(1, 141)?;
    fixture.bytes[fixture.data + fixture.last + 2] = 255;
    let target = harness.directory.join("failed.partial");
    rejection_result(invoke(harness, "export-invalid", &fixture.bytes, &target)?)?;
    require(!target.exists(), "Failed export published a final output.")?;
    let partials = fs::read_dir(harness.directory)
        .map_err(|error| error.to_string())?
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(|error| error.to_string())?
        .iter()
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("failed.partial.partial-")
        })
        .count();
    require(
        partials == 1,
        "Export failure evidence was unexpectedly removed.",
    )?;
    fs::write(&target, [1, 2, 3, 4]).map_err(|error| error.to_string())?;
    rejection_result(invoke(harness, "export-existing", &fixture.bytes, &target)?)?;
    require(
        fs::read(&target).map_err(|error| error.to_string())? == [1, 2, 3, 4],
        "CreateNew rejection modified existing output.",
    )?;
    let wrong = self::fixture(1, 143)?;
    rejection_result(invoke(
        harness,
        "export-wrong-file-type",
        &wrong.bytes,
        &harness.directory.join("wrong-output.gguf"),
    )?)
}
