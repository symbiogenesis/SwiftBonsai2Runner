//! Tiny complete Rust graph wiring and literal driver-boundary controls.

use super::{RustRecognizer, argmax, decode_budget, prompt_tokens, repeating};
use crate::{
    decoder::{self, CachePrecision, Model, Projection},
    encoder::{self, Encoder, Linear, Norm, Weights},
    frontend::Frontend,
    tokenizer::Tokenizer,
};
use std::{io, path::Path, sync::Arc};

fn zero(input: usize, output: usize) -> io::Result<Projection> {
    Projection::f32(input, output, vec![0.0; input * output])
}
fn linear(input: usize, output: usize) -> io::Result<Linear> {
    Ok(Linear {
        weights: zero(input, output)?,
        bias: vec![0.0; output],
    })
}
fn norm(width: usize) -> Norm {
    Norm {
        weight: vec![1.0; width],
        bias: vec![0.0; width],
    }
}

fn recognizer(eos: u32) -> io::Result<RustRecognizer> {
    let tokens = [
        "<|endoftext|>",
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
    ];
    let tokenizer = Tokenizer::from_tables(
        tokens.iter().map(|token| (*token).to_owned()).collect(),
        vec!["a b".into()],
        eos,
    )?;
    let prompt_tokens = prompt_tokens(&tokenizer)?;
    let block = encoder::Block {
        attention_norm: norm(4),
        ffn_norm: norm(4),
        query: linear(4, 4)?,
        key: linear(4, 4)?,
        value: linear(4, 4)?,
        attention_out: linear(4, 4)?,
        ffn_in: linear(4, 4)?,
        ffn_out: linear(4, 4)?,
    };
    let mut output_out = linear(4, 2)?;
    output_out.bias = vec![2.0, -3.0];
    let encoder = Encoder::new(
        encoder::Config {
            mel_bins: 128,
            channels: 1,
            hidden: 4,
            heads: 1,
            intermediate: 4,
            output: 2,
            window: 5,
            max_positions: 2,
        },
        Weights {
            convolutions: [linear(9, 1)?, linear(9, 1)?, linear(9, 1)?],
            convolution_out: zero(16, 4)?,
            blocks: vec![block],
            output_norm: norm(4),
            output_in: linear(4, 4)?,
            output_out,
        },
    )?;
    let block = decoder::Block {
        attention_norm: vec![1.0; 2],
        ffn_norm: vec![1.0; 2],
        query: zero(2, 2)?,
        key: zero(2, 2)?,
        value: zero(2, 2)?,
        attention_out: zero(2, 2)?,
        query_norm: vec![1.0; 2],
        key_norm: vec![1.0; 2],
        gate: zero(2, 2)?,
        up: zero(2, 2)?,
        down: zero(2, 2)?,
    };
    // All vocabulary embedding rows [1,0] produce equal logits; the literal
    // native lowest-ID tie rule therefore chooses 0, including immediate EOS.
    let embedding = Projection::f32(
        2,
        tokens.len(),
        (0..tokens.len()).flat_map(|_| [1.0, 0.0]).collect(),
    )?;
    let model = Model::new(
        decoder::Config {
            hidden: 2,
            heads: 1,
            kv_heads: 1,
            head_dim: 2,
            intermediate: 2,
            context: 512,
            epsilon: 1.0,
            rope_theta: 1_000_000.0,
            cache_precision: CachePrecision::F16,
        },
        embedding,
        vec![block],
        vec![1.0; 2],
    )?;
    Ok(RustRecognizer {
        frontend: Frontend::from_checkpoint(None, None)?,
        encoder,
        tokenizer,
        decoder: Arc::new(model).session(),
        prompt_tokens,
        context: 512,
    })
}

#[test]
fn rust_asr_engine_greedy_ties_and_nonfinite_rejection() -> io::Result<()> {
    assert_eq!(argmax(&[-1.0, 3.0, 3.0, 2.0])?, 1);
    assert_eq!(argmax(&[-0.0, 0.0])?, 0);
    assert!(argmax(&[]).is_err());
    assert!(argmax(&[f32::NAN, 1.0]).is_err());
    assert!(argmax(&[1.0, f32::INFINITY]).is_err());
    Ok(())
}

#[test]
fn rust_asr_engine_fixed_output_budget_and_context_reserve() -> io::Result<()> {
    assert_eq!(decode_budget(1, 16, 8192)?, 256);
    assert_eq!(decode_budget(390, 405, 8192)?, 374);
    assert_eq!(decode_budget(390, 405, 665)?, 260);
    assert!(decode_budget(390, 405, 660).is_err());
    assert!(decode_budget(usize::MAX, 16, 8192).is_err());
    Ok(())
}

#[test]
fn rust_asr_engine_repetition_bars_preserve_short_emphasis() {
    assert!(!repeating(&[7; 63]));
    assert!(repeating(&[7; 64]));
    let paragraph: Vec<u32> = (0..48).cycle().take(192).collect();
    assert!(repeating(&paragraph));
    assert!(!repeating(&paragraph[..191]));
    assert!(!repeating(&(0..256).collect::<Vec<_>>()));
}

#[test]
fn rust_asr_engine_complete_rust_pipeline_eos_and_fresh_replay() -> io::Result<()> {
    let mut recognizer = recognizer(0)?;
    // A mathematically defined silent clip runs the actual Rust frontend,
    // CNN, transformer, audio injection, causal stack and tied EOS head.
    assert_eq!(recognizer.transcribe(&[0.0; 201])?, "");
    assert_eq!(recognizer.decoder.position(), 0);
    assert_eq!(recognizer.decoder.cache(0), Some((&[][..], &[][..])));
    assert_eq!(recognizer.transcribe(&[0.0; 201])?, "");
    assert_eq!(recognizer.decoder.position(), 0);
    Ok(())
}

#[test]
fn rust_asr_engine_missing_eos_and_cancel_invalid_pcm_unwind_reset() -> io::Result<()> {
    let mut recognizer = recognizer(1)?;
    assert!(recognizer.transcribe(&[0.0; 201]).is_err());
    assert_eq!(recognizer.decoder.position(), 0);
    let _ = recognizer.decoder.step(0, &mut || false)?;
    assert!(recognizer.transcribe(&[f32::NAN; 201]).is_err());
    assert_eq!(recognizer.decoder.position(), 0);
    assert!(recognizer.transcribe(&[0.0; 200]).is_err());
    assert!(recognizer.transcribe(&vec![0.0; 480_001]).is_err());
    let result = recognizer.transcribe_cancellable(&[0.0; 201], &mut || true);
    assert_eq!(
        result.err().map(|error| error.kind()),
        Some(io::ErrorKind::Interrupted)
    );
    let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut calls = 0;
        let _ = recognizer.transcribe_cancellable(&[0.0; 201], &mut || {
            calls += 1;
            assert!(calls < 6, "injected Rust ASR callback panic");
            false
        });
    }));
    assert!(unwind.is_err());
    assert_eq!(recognizer.decoder.position(), 0);
    assert_eq!(recognizer.decoder.cache(0), Some((&[][..], &[][..])));
    Ok(())
}

#[test]
fn rust_asr_engine_load_rejects_relative_paths_and_pre_cancelled_work() {
    assert!(RustRecognizer::load(Path::new("model.gguf")).is_err());
    let result = RustRecognizer::load_cancellable(Path::new("model.gguf"), &mut || true);
    assert_eq!(
        result.err().map(|error| error.kind()),
        Some(io::ErrorKind::Interrupted)
    );
}
