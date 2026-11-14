//! Source-bound Qwen3-ASR metadata, tokenizer and tensor catalog checks.

use super::{Index, Value, invalid};
use crate::{protocol::PromptTokens, tokenizer::Tokenizer};
use std::io;

impl Index {
    fn uint(&self, key: &str) -> io::Result<u64> {
        match self.metadata.get(key) {
            Some(Value::Integer { kind: 4, value })
                if *value <= u64::from(i32::MAX.unsigned_abs()) =>
            {
                Ok(*value)
            }
            _ => Err(invalid()),
        }
    }

    fn positive(&self, key: &str) -> io::Result<u64> {
        let value = self.uint(key)?;
        if value == 0 {
            Err(invalid())
        } else {
            Ok(value)
        }
    }

    fn text(&self, key: &str) -> io::Result<&str> {
        match self.metadata.get(key) {
            Some(Value::Text(value)) => Ok(value),
            _ => Err(invalid()),
        }
    }

    fn float(&self, key: &str) -> io::Result<f64> {
        match self.metadata.get(key) {
            Some(Value::Float { kind: 6, value }) if value.is_finite() => Ok(*value),
            _ => Err(invalid()),
        }
    }

    fn boolean(&self, key: &str, default: bool) -> io::Result<bool> {
        match self.metadata.get(key) {
            None => Ok(default),
            Some(Value::Boolean(value)) => Ok(*value),
            _ => Err(invalid()),
        }
    }

    fn strings(&self, key: &str) -> io::Result<&[String]> {
        match self.metadata.get(key) {
            Some(Value::Strings(values)) => Ok(values),
            _ => Err(invalid()),
        }
    }

    fn tensor(&self, name: &str, expected: &[u64], kinds: &[u32]) -> io::Result<()> {
        let tensor = self.tensors.get(name).ok_or_else(invalid)?;
        // GGML treats omitted trailing unit axes as present. Retain that
        // convention rather than requiring a converter-specific rank.
        if !kinds.contains(&tensor.kind)
            || (0..4).any(|axis| {
                tensor.shape.get(axis).copied().unwrap_or(1)
                    != expected.get(axis).copied().unwrap_or(1)
            })
        {
            return Err(invalid());
        }
        Ok(())
    }
}

pub(super) fn validate(index: &Index) -> io::Result<()> {
    if index.text("general.architecture")? != "qwen3_asr"
        || index.text("stt.variant")? != "qwen3-asr-1.7b"
    {
        return Err(invalid());
    }
    validate_tokenizer(index)?;
    validate_frontend(index)?;
    validate_catalog(index)
}

pub(super) fn validate_tokenizer(index: &Index) -> io::Result<()> {
    if index.text("tokenizer.ggml.model")? != "gpt2" {
        return Err(invalid());
    }
    if index.metadata.contains_key("tokenizer.ggml.pre") {
        // The native family defaults this field to qwen2 when it is absent.
        let _ = index.text("tokenizer.ggml.pre")?;
    }
    let tokens = index.strings("tokenizer.ggml.tokens")?;
    let vocabulary = usize::try_from(index.positive("stt.qwen3_asr.decoder.vocab_size")?)
        .map_err(|_| invalid())?;
    if tokens.len() != vocabulary {
        return Err(invalid());
    }
    for required in [
        "<|im_start|>",
        "<|im_end|>",
        "Ċ",
        "system",
        "user",
        "assistant",
        "<asr_text>",
    ] {
        if !tokens.iter().any(|piece| piece == required) {
            return Err(invalid());
        }
    }
    for (key, kind) in [
        ("tokenizer.ggml.scores", 6),
        ("tokenizer.ggml.token_type", 5),
    ] {
        if let Some(value) = index.metadata.get(key)
            && !matches!(value, Value::Array {kind: actual, count} if *actual == kind && *count == vocabulary)
        {
            return Err(invalid());
        }
    }
    // Qwen's optional language prefix requires an encoder, so this selected
    // tokenizer must retain an actual nonempty merge table, not decode only.
    let merges = index.strings("tokenizer.ggml.merges")?;
    if merges.is_empty() {
        return Err(invalid());
    }
    for merge in merges {
        if !merge.as_bytes().iter().skip(1).any(|byte| *byte == b' ') {
            return Err(invalid());
        }
    }
    for key in [
        "tokenizer.ggml.eos_token_id",
        "tokenizer.ggml.bos_token_id",
        "tokenizer.ggml.unknown_token_id",
        "tokenizer.ggml.blank_token_id",
    ] {
        match index.metadata.get(key) {
            None | Some(Value::UnsetToken) if key != "tokenizer.ggml.eos_token_id" => {}
            Some(Value::Integer { kind: 4, value })
                if *value == u64::from(u32::MAX) && key != "tokenizer.ggml.eos_token_id" => {}
            Some(Value::Integer { kind: 4 | 5, value })
                if *value < u64::try_from(vocabulary).map_err(|_| invalid())? => {}
            _ => return Err(invalid()),
        }
    }
    for (key, piece) in [
        ("stt.qwen3_asr.audio_start_token_id", "<|audio_start|>"),
        ("stt.qwen3_asr.audio_token_id", "<|audio_pad|>"),
        ("stt.qwen3_asr.audio_end_token_id", "<|audio_end|>"),
    ] {
        let id = usize::try_from(index.uint(key)?).map_err(|_| invalid())?;
        if tokens.get(id).map(String::as_str) != Some(piece) {
            return Err(invalid());
        }
    }
    Ok(())
}

pub(super) fn prompt_tokens(index: &Index, tokenizer: &Tokenizer) -> io::Result<PromptTokens> {
    let id = |piece| tokenizer.find(piece).ok_or_else(invalid);
    let audio = |key| u32::try_from(index.uint(key)?).map_err(|_| invalid());
    // Chat pieces retain the native tokenizer's first occurrence, while the
    // audio IDs come from the admitted model metadata. A later duplicate of an
    // audio spelling may intentionally be the selected native prompt token.
    PromptTokens::new(
        [
            id("<|im_start|>")?,
            id("<|im_end|>")?,
            id("Ċ")?,
            id("system")?,
            id("user")?,
            id("assistant")?,
        ],
        [
            audio("stt.qwen3_asr.audio_start_token_id")?,
            audio("stt.qwen3_asr.audio_token_id")?,
            audio("stt.qwen3_asr.audio_end_token_id")?,
        ],
        u32::try_from(tokenizer.vocabulary_size()).map_err(|_| invalid())?,
    )
}

fn validate_frontend(index: &Index) -> io::Result<()> {
    if index.text("stt.frontend.type")? != "mel"
        || index.positive("stt.frontend.sample_rate")? != 16_000
        || index.positive("stt.frontend.num_mels")?
            != index.positive("stt.qwen3_asr.encoder.num_mel_bins")?
    {
        return Err(invalid());
    }
    for key in ["n_fft", "win_length", "hop_length"] {
        let _ = index.positive(&format!("stt.frontend.{key}"))?;
    }
    for key in ["window", "normalize"] {
        let _ = index.text(&format!("stt.frontend.{key}"))?;
    }
    for key in ["dither", "pre_emphasis", "f_min", "f_max"] {
        let _ = index.float(&format!("stt.frontend.{key}"))?;
    }
    for key in ["pad_mode", "mel_norm"] {
        let name = format!("stt.frontend.{key}");
        if index.metadata.contains_key(&name) {
            let _ = index.text(&name)?;
        }
    }
    let _ = index.boolean("stt.frontend.center", true)?;
    for key in ["chunk_length", "n_samples", "nb_max_frames"] {
        let name = format!("stt.frontend.{key}");
        if index.metadata.contains_key(&name) {
            let _ = index.uint(&name)?;
        }
    }
    Ok(())
}

fn validate_catalog(index: &Index) -> io::Result<()> {
    validate_decoder(index, validate_encoder(index)?)
}

const LINEAR_TYPES: &[u32] = &[0, 1, 2, 3, 6, 7, 8, 12, 13, 14, 30];

fn validate_encoder(index: &Index) -> io::Result<u64> {
    let encoder = |key: &str| index.positive(&format!("stt.qwen3_asr.encoder.{key}"));
    let enc_layers = encoder("n_layers")?;
    let d = encoder("d_model")?;
    let heads = encoder("n_heads")?;
    let ffn = encoder("ffn_dim")?;
    let ds = encoder("downsample_hidden")?;
    let output = encoder("output_dim")?;
    let mels = encoder("num_mel_bins")?;
    if !d.is_multiple_of(heads)
        || enc_layers > 256
        || index.text("stt.qwen3_asr.encoder.activation")? != "gelu"
    {
        return Err(invalid());
    }
    for key in [
        "max_source_positions",
        "n_window",
        "n_window_infer",
        "conv_chunksize",
    ] {
        let _ = encoder(key)?;
    }
    let linear = LINEAR_TYPES;
    for layer in 0..3 {
        index.tensor(
            &format!("enc.conv.{layer}.weight"),
            &[3, 3, if layer == 0 { 1 } else { ds }, ds],
            &[0, 1],
        )?;
        index.tensor(&format!("enc.conv.{layer}.bias"), &[ds], &[0])?;
    }
    let mel_rows = mels.div_ceil(2).div_ceil(2).div_ceil(2);
    index.tensor(
        "enc.conv_out.weight",
        &[ds.checked_mul(mel_rows).ok_or_else(invalid)?, d],
        linear,
    )?;
    for layer in 0..enc_layers {
        let prefix = format!("enc.blocks.{layer}.");
        for name in [
            "norm_attn.weight",
            "norm_attn.bias",
            "norm_ffn.weight",
            "norm_ffn.bias",
        ] {
            index.tensor(&(prefix.clone() + name), &[d], &[0])?;
        }
        for projection in ["q", "k", "v", "out"] {
            index.tensor(
                &format!("{prefix}attn.{projection}.weight"),
                &[d, d],
                linear,
            )?;
            index.tensor(&format!("{prefix}attn.{projection}.bias"), &[d], &[0])?;
        }
        index.tensor(&(prefix.clone() + "ffn.fc1.weight"), &[d, ffn], linear)?;
        index.tensor(&(prefix.clone() + "ffn.fc1.bias"), &[ffn], &[0])?;
        index.tensor(&(prefix.clone() + "ffn.fc2.weight"), &[ffn, d], linear)?;
        index.tensor(&(prefix + "ffn.fc2.bias"), &[d], &[0])?;
    }
    for name in ["enc.ln_post.weight", "enc.ln_post.bias", "enc.proj1.bias"] {
        index.tensor(name, &[d], &[0])?;
    }
    index.tensor("enc.proj1.weight", &[d, d], linear)?;
    index.tensor("enc.proj2.weight", &[d, output], linear)?;
    index.tensor("enc.proj2.bias", &[output], &[0])?;
    Ok(output)
}

fn validate_decoder(index: &Index, encoder_output: u64) -> io::Result<()> {
    let decoder = |key: &str| index.positive(&format!("stt.qwen3_asr.decoder.{key}"));
    let layers = decoder("n_layers")?;
    let hidden = decoder("hidden_size")?;
    let intermediate = decoder("intermediate_size")?;
    let n_heads = decoder("n_heads")?;
    let n_kv = decoder("n_kv_heads")?;
    let head_dim = decoder("head_dim")?;
    let vocabulary = decoder("vocab_size")?;
    let _ = decoder("max_position_embeddings")?;
    if layers > 256
        || !n_heads.is_multiple_of(n_kv)
        || encoder_output != hidden
        || !head_dim.is_multiple_of(2)
        || !matches!(
            index.text("stt.qwen3_asr.decoder.hidden_act")?,
            "silu" | "swish"
        )
        || !index.boolean("stt.qwen3_asr.decoder.tie_word_embeddings", true)?
        || !index.boolean("stt.qwen3_asr.decoder.rope_mrope_interleaved", true)?
    {
        return Err(invalid());
    }
    let mut sections = 0_u64;
    for suffix in ["t", "h", "w"] {
        sections = sections
            .checked_add(index.uint(&format!(
                "stt.qwen3_asr.decoder.rope_mrope_section_{suffix}"
            ))?)
            .ok_or_else(invalid)?;
    }
    if sections != head_dim / 2 {
        return Err(invalid());
    }
    let _ = index.float("stt.qwen3_asr.decoder.rms_norm_eps")?;
    let _ = index.float("stt.qwen3_asr.decoder.rope_theta")?;
    let linear = LINEAR_TYPES;
    index.tensor("dec.token_embd.weight", &[hidden, vocabulary], linear)?;
    let query = n_heads.checked_mul(head_dim).ok_or_else(invalid)?;
    let kv = n_kv.checked_mul(head_dim).ok_or_else(invalid)?;
    for layer in 0..layers {
        let prefix = format!("dec.blocks.{layer}.");
        for name in ["norm_attn.weight", "norm_ffn.weight"] {
            index.tensor(&(prefix.clone() + name), &[hidden], &[0])?;
        }
        for (name, input, output) in [
            ("attn.q.weight", hidden, query),
            ("attn.k.weight", hidden, kv),
            ("attn.v.weight", hidden, kv),
            ("attn.o.weight", query, hidden),
            ("ffn.gate.weight", hidden, intermediate),
            ("ffn.up.weight", hidden, intermediate),
            ("ffn.down.weight", intermediate, hidden),
        ] {
            index.tensor(&(prefix.clone() + name), &[input, output], linear)?;
        }
        for name in ["attn.q_norm.weight", "attn.k_norm.weight"] {
            index.tensor(&(prefix.clone() + name), &[head_dim], &[0])?;
        }
        if index.tensors[&(prefix.clone() + "ffn.gate.weight")].kind
            != index.tensors[&(prefix + "ffn.up.weight")].kind
        {
            return Err(invalid());
        }
    }
    index.tensor("dec.output_norm.weight", &[hidden], &[0])
}
