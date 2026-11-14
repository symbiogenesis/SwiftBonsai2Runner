//! Independently assembled GGUF wire/layout cases; no native engine or model.

use super::{Index, Reader, Tensor, Value, invalid, model, validate};
use std::io::{self, Cursor};

fn text(output: &mut Vec<u8>, value: &str) {
    output.extend(u64::try_from(value.len()).unwrap_or_default().to_le_bytes());
    output.extend(value.as_bytes());
}

fn string(value: &str) -> Vec<u8> {
    let mut bytes = 8_u32.to_le_bytes().to_vec();
    text(&mut bytes, value);
    bytes
}

fn uint(value: u32) -> Vec<u8> {
    [4_u32.to_le_bytes(), value.to_le_bytes()].concat()
}

fn float(value: f32) -> Vec<u8> {
    [6_u32.to_le_bytes(), value.to_le_bytes()].concat()
}

fn strings(values: &[&str]) -> Vec<u8> {
    let mut bytes = [9_u32.to_le_bytes(), 8_u32.to_le_bytes()].concat();
    bytes.extend(
        u64::try_from(values.len())
            .unwrap_or_default()
            .to_le_bytes(),
    );
    for value in values {
        text(&mut bytes, value);
    }
    bytes
}

// GGUF independently defines: 24-byte header, typed KVs, tensor descriptors,
// 32-byte padded data start and offsets relative to that start.
fn wire(
    metadata: &[(&str, Vec<u8>)],
    tensors: &[(&str, &[u64], u32, u64)],
    data: usize,
) -> Vec<u8> {
    let mut output = b"GGUF\x03\0\0\0".to_vec();
    output.extend(
        u64::try_from(tensors.len())
            .unwrap_or_default()
            .to_le_bytes(),
    );
    output.extend(
        u64::try_from(metadata.len())
            .unwrap_or_default()
            .to_le_bytes(),
    );
    for (key, value) in metadata {
        text(&mut output, key);
        output.extend(value);
    }
    for (name, shape, kind, offset) in tensors {
        text(&mut output, name);
        output.extend(u32::try_from(shape.len()).unwrap_or_default().to_le_bytes());
        for dimension in *shape {
            output.extend(dimension.to_le_bytes());
        }
        output.extend(kind.to_le_bytes());
        output.extend(offset.to_le_bytes());
    }
    output.resize(output.len().div_ceil(32) * 32 + data, 0);
    output
}

fn parse(bytes: Vec<u8>) -> io::Result<Index> {
    let length = u64::try_from(bytes.len()).map_err(|_| invalid())?;
    Reader {
        source: Cursor::new(bytes),
        length,
        remaining: super::MAX_METADATA,
    }
    .index()
}

#[test]
fn rust_asr_intake_reads_independent_f32_q8_layout_and_reordered_metadata() -> io::Result<()> {
    // Q8_0 has one f16 scale and 32 i8 quants: 34 bytes/row. Two rows
    // occupy [32,100); the F32 vector occupies [0,12), without overlap.
    let bytes = wire(
        &[
            ("unrelated", uint(7)),
            ("general.architecture", string("qwen3_asr")),
        ],
        &[("q8", &[32, 2], 8, 32), ("f32", &[3], 0, 0)],
        100,
    );
    let index = parse(bytes)?;
    assert_eq!(index.tensors["q8"].bytes, 68);
    assert_eq!(index.tensors["f32"].bytes, 12);
    assert_eq!(index.tensors["q8"].offset - index.tensors["f32"].offset, 32);
    Ok(())
}

#[test]
fn rust_asr_intake_rejects_overlap_alignment_truncation_and_unknown_dtype() {
    for (shape, kind, offset, data) in [
        (&[32][..], 8, 0, 100),  // overlaps the F32 vector
        (&[32][..], 8, 1, 100),  // unaligned offset
        (&[32][..], 8, 32, 65),  // 34 bytes require an end of 66
        (&[31][..], 8, 32, 100), // fractional quantization block
        (&[32][..], 999, 32, 100),
        (&[0][..], 0, 32, 100),
        (&[u64::MAX, 2][..], 0, 32, 100),
    ] {
        assert!(
            parse(wire(
                &[("key", uint(1))],
                &[("f32", &[2], 0, 0), ("bad", shape, kind, offset)],
                data
            ))
            .is_err()
        );
    }
}

#[test]
fn rust_asr_intake_rejects_duplicate_keys_names_and_unbounded_fields() {
    assert!(
        parse(wire(
            &[("key", uint(1)), ("key", uint(2))],
            &[("weight", &[1], 0, 0)],
            4
        ))
        .is_err()
    );
    assert!(
        parse(wire(
            &[("key", uint(1))],
            &[("weight", &[1], 0, 0), ("weight", &[1], 0, 32)],
            36
        ))
        .is_err()
    );
    let baseline = wire(&[("key", uint(1))], &[("weight", &[1], 0, 0)], 4);
    for (range, replacement) in [
        (8..16, u64::MAX.to_le_bytes()),
        (16..24, u64::MAX.to_le_bytes()),
        (24..32, u64::MAX.to_le_bytes()),
    ] {
        let mut bytes = baseline.clone();
        bytes[range].copy_from_slice(&replacement);
        assert!(parse(bytes).is_err());
    }
    let mut bad = baseline;
    bad[4] = 2;
    assert!(parse(bad).is_err());
}

#[test]
fn rust_asr_intake_rejects_invalid_utf8_nested_arrays_and_boolean_values() {
    for value in [
        [
            8_u32.to_le_bytes().as_slice(),
            1_u64.to_le_bytes().as_slice(),
            &[255],
        ]
        .concat(),
        [
            9_u32.to_le_bytes().as_slice(),
            9_u32.to_le_bytes().as_slice(),
            0_u64.to_le_bytes().as_slice(),
        ]
        .concat(),
        [7_u32.to_le_bytes().as_slice(), &[2]].concat(),
    ] {
        assert!(parse(wire(&[("key", value)], &[("weight", &[1], 0, 0)], 4)).is_err());
    }
}

fn tokenizer() -> io::Result<Index> {
    let tokens = [
        "<|im_start|>",
        "<|im_end|>",
        "Ċ",
        "system",
        "user",
        "assistant",
        "<asr_text>",
        "<|audio_start|>",
        "<|audio_pad|>",
        "<|audio_end|>",
        "a",
        "b",
        "ab",
    ];
    parse(wire(
        &[
            ("tokenizer.ggml.model", string("gpt2")),
            ("tokenizer.ggml.tokens", strings(&tokens)),
            ("tokenizer.ggml.merges", strings(&["a b"])),
            ("tokenizer.ggml.eos_token_id", uint(1)),
            ("stt.qwen3_asr.decoder.vocab_size", uint(13)),
            ("stt.qwen3_asr.audio_start_token_id", uint(7)),
            ("stt.qwen3_asr.audio_token_id", uint(8)),
            ("stt.qwen3_asr.audio_end_token_id", uint(9)),
        ],
        &[("weight", &[1], 0, 0)],
        4,
    ))
}

#[test]
fn rust_asr_intake_checks_tokenizer_ids_table_types_and_merge_boundaries() -> io::Result<()> {
    let mut index = tokenizer()?;
    model::validate_tokenizer(&index)?;
    for (key, value) in [
        (
            "tokenizer.ggml.eos_token_id",
            Value::Integer { kind: 4, value: 13 },
        ),
        (
            "tokenizer.ggml.eos_token_id",
            Value::Integer { kind: 10, value: 1 },
        ),
        ("tokenizer.ggml.scores", Value::Array { kind: 5, count: 13 }),
        (
            "tokenizer.ggml.token_type",
            Value::Array { kind: 5, count: 12 },
        ),
        (
            "tokenizer.ggml.merges",
            Value::Strings(vec!["nosplit".into()]),
        ),
        (
            "stt.qwen3_asr.audio_token_id",
            Value::Integer { kind: 4, value: 7 },
        ),
    ] {
        let before = index.metadata.insert(key.into(), value);
        assert!(model::validate_tokenizer(&index).is_err());
        index.metadata.remove(key);
        if let Some(value) = before {
            index.metadata.insert(key.into(), value);
        }
    }
    Ok(())
}

#[test]
fn rust_asr_intake_prompt_preserves_declared_audio_ids_with_duplicate_spellings() -> io::Result<()>
{
    let mut index = tokenizer()?;
    let Some(Value::Strings(tokens)) = index.metadata.get_mut("tokenizer.ggml.tokens") else {
        return Err(invalid());
    };
    tokens.extend(
        [
            "<|audio_start|>",
            "<|audio_pad|>",
            "<|audio_end|>",
            "<|im_start|>",
        ]
        .map(str::to_owned),
    );
    let tokenizer =
        crate::tokenizer::Tokenizer::from_tables(tokens.clone(), vec!["a b".into()], 1)?;
    for (key, value) in [
        ("stt.qwen3_asr.decoder.vocab_size", 17),
        ("stt.qwen3_asr.audio_start_token_id", 13),
        ("stt.qwen3_asr.audio_token_id", 14),
        ("stt.qwen3_asr.audio_end_token_id", 15),
    ] {
        index
            .metadata
            .insert(key.into(), Value::Integer { kind: 4, value });
    }
    model::validate_tokenizer(&index)?;
    // Native build_prompt_tokens uses metadata audio IDs, while chat-role
    // resolution uses the first vocabulary occurrence. These literal IDs bind
    // both choices independently of candidate lookup/assembly helpers.
    assert_eq!(tokenizer.find("<|audio_start|>"), Some(7));
    assert_eq!(tokenizer.find("<|audio_pad|>"), Some(8));
    assert_eq!(tokenizer.find("<|audio_end|>"), Some(9));
    let prompt = model::prompt_tokens(&index, &tokenizer)?.automatic_prompt(1, 8192)?;
    assert_eq!(
        prompt.ids(),
        [0, 3, 2, 1, 2, 0, 4, 2, 13, 14, 15, 1, 2, 0, 5, 2]
    );
    assert_eq!(prompt.audio_rows(), 9..10);
    Ok(())
}

#[test]
fn rust_asr_intake_charges_empty_string_arrays_and_skipped_scalars() {
    let mut reader = Reader {
        source: Cursor::new(strings(&["", ""])),
        length: 32,
        remaining: 63,
    };
    // Two empty strings still own two String objects and their length words.
    assert!(reader.value().is_err());
    let mut reader = Reader {
        source: Cursor::new(
            [
                9_u32.to_le_bytes().as_slice(),
                5_u32.to_le_bytes().as_slice(),
                100_u64.to_le_bytes().as_slice(),
            ]
            .concat(),
        ),
        length: 16,
        remaining: 64,
    };
    assert!(reader.value().is_err());
}

#[test]
fn rust_asr_intake_complete_validation_rejects_partial_and_non_qwen_models() -> io::Result<()> {
    let bytes = wire(
        &[
            ("general.architecture", string("qwen3_asr")),
            ("stt.variant", string("qwen3-asr-1.7b")),
        ],
        &[("weight", &[1], 0, 0)],
        4,
    );
    let length = u64::try_from(bytes.len()).map_err(|_| invalid())?;
    assert!(validate(&mut Cursor::new(bytes), length).is_err());
    let mut index = tokenizer()?;
    index
        .metadata
        .insert("general.architecture".into(), Value::Text("whisper".into()));
    index
        .metadata
        .insert("stt.variant".into(), Value::Text("qwen3-asr-1.7b".into()));
    assert!(model::validate(&index).is_err());
    Ok(())
}

#[test]
fn rust_asr_intake_accepts_signed_unset_optional_token_without_weakening_eos() -> io::Result<()> {
    let mut index = tokenizer()?;
    index
        .metadata
        .insert("tokenizer.ggml.unknown_token_id".into(), Value::UnsetToken);
    model::validate_tokenizer(&index)?;
    index
        .metadata
        .insert("tokenizer.ggml.eos_token_id".into(), Value::UnsetToken);
    assert!(model::validate_tokenizer(&index).is_err());
    Ok(())
}

#[test]
fn rust_asr_intake_declared_tensor_blocks_have_independent_byte_sizes() -> io::Result<()> {
    for (kind, columns, bytes) in [
        (0, 1, 4),
        (1, 1, 2),
        (30, 1, 2),
        (2, 32, 18),
        (3, 32, 20),
        (6, 32, 22),
        (7, 32, 24),
        (8, 32, 34),
        (12, 256, 144),
        (13, 256, 176),
        (14, 256, 210),
    ] {
        let parsed = parse(wire(
            &[("key", float(1.0))],
            &[("weight", &[columns], kind, 0)],
            bytes,
        ))?;
        assert_eq!(
            parsed.tensors["weight"].bytes,
            u64::try_from(bytes).map_err(|_| invalid())?
        );
    }
    Ok(())
}

fn catalog_metadata(index: &mut Index) {
    for (key, value) in [
        ("general.architecture", "qwen3_asr"),
        ("stt.variant", "qwen3-asr-1.7b"),
        ("stt.qwen3_asr.encoder.activation", "gelu"),
        ("stt.qwen3_asr.decoder.hidden_act", "silu"),
        ("stt.frontend.type", "mel"),
        ("stt.frontend.window", "hann_periodic"),
        ("stt.frontend.normalize", "per_utterance"),
    ] {
        index.metadata.insert(key.into(), Value::Text(value.into()));
    }
    for (key, value) in [
        ("encoder.n_layers", 1),
        ("encoder.d_model", 4),
        ("encoder.n_heads", 2),
        ("encoder.ffn_dim", 8),
        ("encoder.num_mel_bins", 8),
        ("encoder.downsample_hidden", 2),
        ("encoder.output_dim", 4),
        ("encoder.max_source_positions", 1500),
        ("encoder.n_window", 100),
        ("encoder.n_window_infer", 400),
        ("encoder.conv_chunksize", 500),
        ("decoder.n_layers", 1),
        ("decoder.hidden_size", 4),
        ("decoder.intermediate_size", 8),
        ("decoder.n_heads", 2),
        ("decoder.n_kv_heads", 1),
        ("decoder.head_dim", 2),
        ("decoder.max_position_embeddings", 8192),
        ("decoder.rope_mrope_section_t", 1),
        ("decoder.rope_mrope_section_h", 0),
        ("decoder.rope_mrope_section_w", 0),
    ] {
        index.metadata.insert(
            format!("stt.qwen3_asr.{key}"),
            Value::Integer { kind: 4, value },
        );
    }
    for (key, value) in [
        ("sample_rate", 16_000),
        ("num_mels", 8),
        ("n_fft", 400),
        ("win_length", 400),
        ("hop_length", 160),
    ] {
        index.metadata.insert(
            format!("stt.frontend.{key}"),
            Value::Integer { kind: 4, value },
        );
    }
    for (key, value) in [
        ("stt.frontend.dither", 0.0),
        ("stt.frontend.pre_emphasis", 0.0),
        ("stt.frontend.f_min", 0.0),
        ("stt.frontend.f_max", 8000.0),
        ("stt.qwen3_asr.decoder.rms_norm_eps", 1e-6),
        ("stt.qwen3_asr.decoder.rope_theta", 1e6),
    ] {
        index
            .metadata
            .insert(key.into(), Value::Float { kind: 6, value });
    }
}

fn complete_catalog() -> io::Result<Index> {
    let mut index = tokenizer()?;
    catalog_metadata(&mut index);
    // One miniature source-catalog case with hand-selected dimensions. It is
    // neither a converted 1.7B model nor a numerical inference observation.
    for (name, shape) in [
        ("enc.conv.0.weight", &[3, 3, 1, 2][..]),
        ("enc.conv.0.bias", &[2]),
        ("enc.conv.1.weight", &[3, 3, 2, 2]),
        ("enc.conv.1.bias", &[2]),
        ("enc.conv.2.weight", &[3, 3, 2, 2]),
        ("enc.conv.2.bias", &[2]),
        ("enc.conv_out.weight", &[2, 4]),
        ("enc.blocks.0.norm_attn.weight", &[4]),
        ("enc.blocks.0.norm_attn.bias", &[4]),
        ("enc.blocks.0.attn.q.weight", &[4, 4]),
        ("enc.blocks.0.attn.q.bias", &[4]),
        ("enc.blocks.0.attn.k.weight", &[4, 4]),
        ("enc.blocks.0.attn.k.bias", &[4]),
        ("enc.blocks.0.attn.v.weight", &[4, 4]),
        ("enc.blocks.0.attn.v.bias", &[4]),
        ("enc.blocks.0.attn.out.weight", &[4, 4]),
        ("enc.blocks.0.attn.out.bias", &[4]),
        ("enc.blocks.0.norm_ffn.weight", &[4]),
        ("enc.blocks.0.norm_ffn.bias", &[4]),
        ("enc.blocks.0.ffn.fc1.weight", &[4, 8]),
        ("enc.blocks.0.ffn.fc1.bias", &[8]),
        ("enc.blocks.0.ffn.fc2.weight", &[8, 4]),
        ("enc.blocks.0.ffn.fc2.bias", &[4]),
        ("enc.ln_post.weight", &[4]),
        ("enc.ln_post.bias", &[4]),
        ("enc.proj1.weight", &[4, 4]),
        ("enc.proj1.bias", &[4]),
        ("enc.proj2.weight", &[4, 4]),
        ("enc.proj2.bias", &[4]),
        ("dec.token_embd.weight", &[4, 13]),
        ("dec.output_norm.weight", &[4]),
        ("dec.blocks.0.norm_attn.weight", &[4]),
        ("dec.blocks.0.norm_ffn.weight", &[4]),
        ("dec.blocks.0.attn.q.weight", &[4, 4]),
        ("dec.blocks.0.attn.k.weight", &[4, 2]),
        ("dec.blocks.0.attn.v.weight", &[4, 2]),
        ("dec.blocks.0.attn.o.weight", &[4, 4]),
        ("dec.blocks.0.attn.q_norm.weight", &[2]),
        ("dec.blocks.0.attn.k_norm.weight", &[2]),
        ("dec.blocks.0.ffn.gate.weight", &[4, 8]),
        ("dec.blocks.0.ffn.up.weight", &[4, 8]),
        ("dec.blocks.0.ffn.down.weight", &[8, 4]),
    ] {
        index.tensors.insert(
            name.into(),
            Tensor {
                shape: shape.to_vec(),
                kind: 0,
                offset: 0,
                bytes: shape.iter().product::<u64>() * 4,
            },
        );
    }
    Ok(index)
}

#[test]
fn rust_asr_intake_catalog_checks_complete_shapes_types_and_cross_field_state() -> io::Result<()> {
    let mut index = complete_catalog()?;
    model::validate(&index)?;
    for key in [
        "enc.conv.2.weight",
        "enc.blocks.0.attn.k.weight",
        "dec.blocks.0.ffn.down.weight",
        "dec.output_norm.weight",
    ] {
        let tensor = index.tensors.get_mut(key).ok_or_else(invalid)?;
        let original = tensor.shape[0];
        tensor.shape[0] += 1;
        assert!(model::validate(&index).is_err());
        index.tensors.get_mut(key).ok_or_else(invalid)?.shape[0] = original;
    }
    index.metadata.insert(
        "stt.qwen3_asr.decoder.n_kv_heads".into(),
        Value::Integer { kind: 4, value: 3 },
    );
    assert!(model::validate(&index).is_err());
    index.metadata.insert(
        "stt.qwen3_asr.decoder.n_kv_heads".into(),
        Value::Integer { kind: 4, value: 1 },
    );
    index.metadata.insert(
        "stt.qwen3_asr.decoder.tie_word_embeddings".into(),
        Value::Boolean(false),
    );
    assert!(model::validate(&index).is_err());
    Ok(())
}

#[test]
#[ignore = "Explicit predeclared owner-device metadata intake only; no inference or authenticity verdict."]
fn rust_asr_intake_pinned_model_index() -> io::Result<()> {
    let path =
        std::path::PathBuf::from(std::env::var_os("BONSAI_ASR_INTAKE_MODEL").ok_or_else(invalid)?);
    if !path.is_absolute() {
        return Err(invalid());
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        options.share_mode(1).custom_flags(0x0020_0000);
    }
    let mut file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() != 2_185_030_624 {
        return Err(invalid());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(invalid());
        }
    }
    // The lock's size bounds this structural read; only the caller's retained
    // complete SHA-256/file guard can establish artifact authenticity.
    validate(&mut file, metadata.len())
}
