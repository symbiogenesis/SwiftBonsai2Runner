//! Independent GGUF v3 parser, Unicode, array and hostile-descriptor controls.

use super::{Harness, Wire, patch, require};
use serde_json::{Value, json};
use std::collections::BTreeMap;

struct Fixture {
    bytes: Vec<u8>,
    positions: BTreeMap<&'static str, usize>,
    data: usize,
    tokenizer: String,
    template: String,
}

fn fixture() -> crate::Result<Fixture> {
    let mut wire = Wire::header(3, 11);
    let mut positions = BTreeMap::new();
    wire.scalar_text("general.architecture", "qwen35")?;
    wire.key("general.alignment", 4)?;
    positions.insert("alignment", wire.pos());
    wire.u32(32);
    for (name, value) in [
        ("qwen35.block_count", 65),
        ("qwen35.nextn_predict_layers", 1),
        ("qwen35.context_length", 262_144),
        ("qwen35.attention.head_count", 24),
    ] {
        wire.scalar(name, value)?;
    }
    wire.key("qwen35.attention.recurrent_layers", 9)?;
    wire.u32(7);
    wire.u64(4);
    positions.insert("boolean", wire.pos());
    wire.0.extend_from_slice(&[1, 1, 1, 0]);
    let tokenizer_start = wire.pos();
    wire.scalar_text("tokenizer.ggml.model", "gpt2")?;
    wire.key("tokenizer.ggml.tokens", 9)?;
    wire.u32(8);
    positions.insert("array-count", wire.pos());
    wire.u64(5000);
    for index in 0..5000 {
        wire.text(&format!("token-{index}"))?;
    }
    wire.scalar("tokenizer.ggml.eos_token_id", 42)?;
    let template = "{{ messages }}\nUnicode: \u{03bb} \u{1f340}\n";
    wire.scalar_text("tokenizer.chat_template", template)?;
    let tokenizer = crate::identity::sha256(&wire.0[tokenizer_start..]);
    wire.text("blk.0.attn_q.weight")?;
    positions.insert("dimension-count", wire.pos());
    wire.u32(1);
    positions.insert("dimension", wire.pos());
    wire.u64(256);
    positions.insert("tensor-type", wire.pos());
    wire.u32(11);
    positions.insert("tensor-offset", wire.pos());
    wire.u64(0);
    for (name, offset) in [
        ("blk.64.ffn_norm.weight", 128),
        ("blk.64.nextn.eh_proj.weight", 160),
    ] {
        wire.text(name)?;
        wire.u32(1);
        wire.u64(2);
        wire.u32(0);
        wire.u64(offset);
    }
    wire.align();
    let data = wire.pos();
    wire.0.resize(data + 168, 0);
    Ok(Fixture {
        bytes: wire.0,
        positions,
        data,
        tokenizer,
        template: crate::identity::sha256(template.as_bytes()),
    })
}

fn parser_report(report: &Value, fixture: &Fixture) -> crate::Result<()> {
    require(
        report["Architecture"] == "qwen35" && report["TensorCount"] == 3,
        "Header/architecture inspection failed.",
    )?;
    require(
        report["Scalars"]["qwen35.context_length"] == 262_144,
        "Context scalar was lost.",
    )?;
    require(
        report["Arrays"]["qwen35.attention.recurrent_layers"]
            .as_array()
            .is_some_and(|values| values.len() == 4),
        "Small layout array was lost.",
    )?;
    require(
        report["Arrays"]["tokenizer.ggml.tokens"]["Count"] == 5000
            && !report["Arrays"]["tokenizer.ggml.tokens"].is_array(),
        "Vocabulary contents or descriptor changed.",
    )?;
    require(
        report["TokenizerMetadataSha256"] == fixture.tokenizer,
        "Tokenizer digest differs from raw record bytes.",
    )?;
    require(
        report["EmbeddedTemplates"]["tokenizer.chat_template"]["Sha256"] == fixture.template,
        "Template UTF-8 digest changed.",
    )?;
    require(
        report["SpecialTokenIds"]["tokenizer.ggml.eos_token_id"] == 42,
        "Special token ID was lost.",
    )?;
    require(
        report["TensorDataOffset"] == json!(fixture.data) && report["TensorPayloadRead"] == false,
        "Unexpected tensor payload inspection.",
    )?;
    require(
        report["TensorTypeHistogram"]
            .as_array()
            .is_some_and(|values| values.len() == 2)
            && report["TensorTypeHistogram"][1]["TypeId"] == 11
            && report["TensorTypeHistogram"][1]["Name"] == "Q3_K",
        "Mixed tensor type inventory changed.",
    )?;
    require(
        report["MtpTensorCount"] == 2,
        "MTP tensors were not classified using metadata.",
    )?;
    Ok(())
}

pub(super) fn parser(harness: &Harness<'_>) -> crate::Result<()> {
    let fixture = fixture()?;
    let report = harness.inspect("parser", &fixture.bytes, "Stock", false)?;
    parser_report(&report, &fixture)?;
    for (name, offset, replacement) in [
        ("magic", 0, 0_u32.to_le_bytes().to_vec()),
        ("version", 4, 2_u32.to_le_bytes().to_vec()),
        (
            "alignment",
            fixture.positions["alignment"],
            3_u32.to_le_bytes().to_vec(),
        ),
        ("boolean", fixture.positions["boolean"], vec![2]),
        (
            "array-count",
            fixture.positions["array-count"],
            u64::MAX.to_le_bytes().to_vec(),
        ),
        (
            "dimension-count",
            fixture.positions["dimension-count"],
            5_u32.to_le_bytes().to_vec(),
        ),
        (
            "dimension-zero",
            fixture.positions["dimension"],
            0_u64.to_le_bytes().to_vec(),
        ),
        (
            "dimension-overflow",
            fixture.positions["dimension"],
            u64::MAX.to_le_bytes().to_vec(),
        ),
        (
            "tensor-type",
            fixture.positions["tensor-type"],
            999_u32.to_le_bytes().to_vec(),
        ),
        (
            "tensor-offset",
            fixture.positions["tensor-offset"],
            32_u64.to_le_bytes().to_vec(),
        ),
    ] {
        let mut bytes = fixture.bytes.clone();
        patch(&mut bytes, offset, &replacement);
        harness.reject(name, &bytes, "Stock", false, None)?;
    }
    harness.reject(
        "truncated-header",
        &fixture.bytes[..21],
        "Stock",
        false,
        None,
    )?;
    harness.reject(
        "truncated-payload",
        &fixture.bytes[..fixture.bytes.len() - 1],
        "Stock",
        false,
        None,
    )
}

fn string_fixture(key: &str, value: &str) -> crate::Result<(Vec<u8>, usize, usize, String)> {
    let mut wire = Wire::header(0, 2);
    wire.scalar_text("general.architecture", "synthetic")?;
    let start = wire.pos();
    wire.key(key, 8)?;
    let value_start = wire.pos() + 8;
    wire.text(value)?;
    let digest = crate::identity::sha256(&wire.0[start..]);
    Ok((wire.0, start + 8, value_start, digest))
}

pub(super) fn strings(harness: &Harness<'_>) -> crate::Result<()> {
    for size in [0_usize, 1, 1024, 65_535, 65_536, 65_537] {
        let value = if size < 4 {
            "a".repeat(size)
        } else {
            "a".repeat(size - 4) + "\u{1f340}"
        };
        let (bytes, _, _, digest) = string_fixture("tokenizer.test_string", &value)?;
        let report = harness.inspect(&format!("string-{size}"), &bytes, "Stock", false)?;
        if size <= 65_536 {
            require(
                report["Scalars"]["tokenizer.test_string"] == value,
                "Retained UTF-8 string changed.",
            )?;
        } else {
            require(
                report["Scalars"].get("tokenizer.test_string").is_none(),
                "Over-limit string was materialized.",
            )?;
        }
        require(
            report["TokenizerMetadataSha256"] == digest,
            "String buffering changed tokenizer digest.",
        )?;
    }
    let key = "tokenizer.".to_owned() + &"a".repeat(1014);
    let (bytes, key_start, value_start, _) = string_fixture(&key, "\u{03bb}")?;
    require(
        harness.inspect("long-key", &bytes, "Stock", false)?["Scalars"][&key] == "\u{03bb}",
        "The 1024-byte metadata key changed.",
    )?;
    for (part, offset) in [("key", key_start), ("value", value_start)] {
        let mut changed = bytes.clone();
        changed[offset] = 255;
        harness.reject(
            &format!("invalid-utf8-{part}"),
            &changed,
            "Stock",
            false,
            None,
        )?;
    }
    harness.reject(
        "truncated-string",
        &bytes[..bytes.len() - 1],
        "Stock",
        false,
        None,
    )?;
    harness.reject(
        "over-limit-key",
        &string_fixture(&(key + "a"), "test")?.0,
        "Stock",
        false,
        None,
    )
}

fn array_fixture(kind: u32, count: u64, values: &[u8]) -> crate::Result<(Vec<u8>, usize, String)> {
    let mut wire = Wire::header(0, 2);
    wire.scalar_text("general.architecture", "synthetic")?;
    let start = wire.pos();
    wire.key("tokenizer.test_array", 9)?;
    wire.u32(kind);
    wire.u64(count);
    let position = wire.pos();
    wire.0.extend_from_slice(values);
    let digest = crate::identity::sha256(&wire.0[start..]);
    Ok((wire.0, position, digest))
}

pub(super) fn arrays(harness: &Harness<'_>) -> crate::Result<()> {
    let mut flags = vec![0; 131_077];
    for offset in [0, 65_536, 131_076] {
        flags[offset] = 1;
    }
    let (bytes, start, digest) = array_fixture(7, 131_077, &flags)?;
    let report = harness.inspect("boolean-array", &bytes, "Stock", false)?;
    require(
        report["Arrays"]["tokenizer.test_array"] == json!({"Count":131_077,"ElementTypeId":7}),
        "Large boolean array descriptor changed.",
    )?;
    require(
        report["TokenizerMetadataSha256"] == digest,
        "Boolean validation changed raw tokenizer digest.",
    )?;
    for offset in [0, 65_536, 131_076] {
        let mut changed = bytes.clone();
        changed[start + offset] = 2;
        harness.reject(
            &format!("boolean-invalid-{offset}"),
            &changed,
            "Stock",
            false,
            None,
        )?;
    }
    harness.reject(
        "boolean-truncated",
        &bytes[..bytes.len() - 1],
        "Stock",
        false,
        None,
    )?;
    let mut values = Wire::default();
    values.u32(4);
    values.u64(8192);
    values.0.extend(vec![0; 8192 * 4]);
    values.u32(7);
    values.u64(5000);
    values.0.extend(vec![0; 5000]);
    values.u32(9);
    values.u64(1);
    values.u32(7);
    values.u64(1);
    values.0.push(1);
    let (mut nested, _, digest) = array_fixture(9, 3, &values.0)?;
    let report = harness.inspect("nested-array", &nested, "Stock", false)?;
    require(
        report["Arrays"]["tokenizer.test_array"] == json!({"Count":3,"ElementTypeId":9}),
        "Nested array descriptor changed.",
    )?;
    require(
        report["TokenizerMetadataSha256"] == digest,
        "Nested array changed raw tokenizer digest.",
    )?;
    *nested.last_mut().ok_or("Missing nested boolean.")? = 2;
    harness.reject("nested-invalid-boolean", &nested, "Stock", false, None)?;
    let mut deep = Wire::default();
    for _ in 0..3 {
        deep.u32(9);
        deep.u64(1);
    }
    deep.u32(7);
    deep.u64(1);
    deep.0.push(1);
    harness.reject(
        "nested-depth",
        &array_fixture(9, 1, &deep.0)?.0,
        "Stock",
        false,
        None,
    )
}

pub(super) fn descriptors(harness: &Harness<'_>) -> crate::Result<()> {
    // Independently authored fixtures inspired by OxiBonsai (Apache-2.0),
    // copyright 2026 COOLJAPAN OU (Team Kitasan), revision
    // 1d10c48e14754ff6999158f8fa13c7c92b40b0dc: tests/fuzz_gguf.rs,
    // tests/gguf_edge_cases.rs and src/gguf/streaming.rs in oxibonsai-core.
    let mut wire = Wire::header(2, 3);
    wire.scalar_text("general.architecture", "synthetic")?;
    wire.scalar("test.first", 1)?;
    let second_key = wire.pos() + 8;
    wire.scalar("test.other", 2)?;
    wire.text("a.weight")?;
    wire.u32(1);
    wire.u64(16);
    wire.u32(0);
    let first_offset = wire.pos();
    wire.u64(0);
    let second_name = wire.pos() + 8;
    wire.text("b.weight")?;
    wire.u32(1);
    wire.u64(16);
    wire.u32(0);
    let second_offset = wire.pos();
    wire.u64(64);
    wire.align();
    wire.0.extend(vec![0; 128]);
    let report = harness.inspect("descriptors", &wire.0, "Stock", false)?;
    require(
        report["MetadataCount"] == 3
            && report["TensorCount"] == 2
            && report["Scalars"]["test.first"] == 1
            && report["Scalars"]["test.other"] == 2,
        "Descriptor positive control failed.",
    )?;
    let mut reversed = wire.0.clone();
    patch(&mut reversed, first_offset, &64_u64.to_le_bytes());
    patch(&mut reversed, second_offset, &0_u64.to_le_bytes());
    require(
        harness.inspect("reversed-descriptors", &reversed, "Stock", false)?["TensorCount"] == 2,
        "Out-of-order nonoverlapping descriptors were rejected.",
    )?;
    for (name, offset, replacement, diagnostic) in [
        (
            "tensor-count-limit",
            8,
            100_001_u64.to_le_bytes().to_vec(),
            "tensor/KV count exceeds inspection limit",
        ),
        (
            "metadata-count-limit",
            16,
            10_001_u64.to_le_bytes().to_vec(),
            "tensor/KV count exceeds inspection limit",
        ),
        (
            "tensor-count-overflow",
            8,
            u64::MAX.to_le_bytes().to_vec(),
            "tensor/KV count exceeds inspection limit",
        ),
        (
            "metadata-count-overflow",
            16,
            u64::MAX.to_le_bytes().to_vec(),
            "tensor/KV count exceeds inspection limit",
        ),
        (
            "duplicate-key",
            second_key,
            b"test.first".to_vec(),
            "duplicate metadata key",
        ),
        (
            "duplicate-name",
            second_name,
            b"a.weight".to_vec(),
            "duplicate tensor name",
        ),
        (
            "partial-overlap",
            second_offset,
            32_u64.to_le_bytes().to_vec(),
            "tensor payload extent is truncated or overlapping",
        ),
        (
            "identical-extent",
            second_offset,
            0_u64.to_le_bytes().to_vec(),
            "tensor payload extent is truncated or overlapping",
        ),
    ] {
        let mut changed = wire.0.clone();
        patch(&mut changed, offset, &replacement);
        harness.reject(name, &changed, "Stock", false, Some(diagnostic))?;
    }
    Ok(())
}

pub(super) fn artifact(harness: &Harness<'_>) -> crate::Result<()> {
    let saved = harness.bridge("artifact", &fixture()?.bytes, "Stock", false)?;
    require(saved["TensorCount"] == 3, "Saved tensor inventory changed.")
}
