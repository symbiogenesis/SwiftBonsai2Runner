//! Tiny complete Rust graph wiring and literal driver-boundary controls.

use super::{Observation, RustRecognizer, argmax, decode_budget, repeating};
use crate::{
    decoder::{self, CachePrecision, Model, Projection},
    encoder::{self, Encoder, Linear, Norm, Weights},
    frontend::Frontend,
    protocol::PromptTokens,
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

#[expect(
    clippy::redundant_pub_crate,
    reason = "Shared model-free graph fixture; unreachable_pub forbids broader visibility."
)]
pub(crate) fn recognizer(eos: u32) -> io::Result<RustRecognizer> {
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
    let prompt_tokens = PromptTokens::new([1, 2, 3, 4, 5, 6], [8, 9, 10], 11)?;
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
    let block = |retain_values| -> io::Result<decoder::Block> {
        Ok(decoder::Block {
            attention_norm: vec![1.0; 2],
            ffn_norm: vec![1.0; 2],
            query: zero(2, 2)?,
            key: zero(2, 2)?,
            value: if retain_values {
                Projection::f32(2, 2, vec![1.0, 0.0, 0.0, 1.0])?
            } else {
                zero(2, 2)?
            },
            attention_out: zero(2, 2)?,
            query_norm: vec![1.0; 2],
            key_norm: vec![1.0; 2],
            gate: zero(2, 2)?,
            up: zero(2, 2)?,
            down: zero(2, 2)?,
        })
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
        vec![block(false)?, block(true)?],
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

#[expect(
    clippy::redundant_pub_crate,
    reason = "Shared model-free reset assertion; unreachable_pub forbids broader visibility."
)]
pub(crate) fn assert_cleared(recognizer: &RustRecognizer) {
    assert_eq!(recognizer.decoder.position(), 0);
    assert_eq!(recognizer.decoder.layer_count(), 2);
    for layer in 0..recognizer.decoder.layer_count() {
        assert_eq!(recognizer.decoder.cache(layer), Some((&[][..], &[][..])));
    }
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

#[test]
fn rust_asr_observer_complete_boundaries_logits_and_all_committed_cache_bits() -> io::Result<()> {
    let mut recognizer = recognizer(0)?;
    let mut events = Vec::new();
    let transcript = recognizer.transcribe_observed(&[0.0; 201], &mut || false, &mut |event| {
        match event {
            Observation::Frontend(mel) => {
                events.push("frontend");
                assert_eq!(mel.frames(), 1);
                assert_eq!(mel.values(), [-1.5; 128]);
            }
            Observation::Encoder(audio) => {
                events.push("encoder");
                assert_eq!((audio.rows(), audio.width()), (1, 2));
                assert_eq!(audio.values(), [2.0, -3.0]);
            }
            Observation::Prompt(prompt) => {
                events.push("prompt");
                assert_eq!(
                    prompt.ids(),
                    [1, 4, 3, 2, 3, 1, 5, 3, 8, 9, 10, 2, 3, 1, 6, 3]
                );
                assert_eq!(prompt.audio_rows(), 9..10);
            }
            Observation::Decoder {
                generation_index,
                logits,
                token,
                state,
            } => {
                events.push("decoder");
                assert_eq!(generation_index, 0);
                assert_eq!(token, 0);
                assert_eq!(logits, [1.0 / 1.5_f32.sqrt(); 11]);
                assert_eq!(state.position(), 16);
                assert_eq!(state.layer_count(), 2);
                for layer in 0..state.layer_count() {
                    let (keys, values) = state
                        .cache(layer)
                        .ok_or_else(|| io::Error::other("missing observed layer"))?;
                    // IEEE zero signs follow the independently known unit-
                    // radian RoPE quadrants: x=0*cos-0*sin, y=0*sin+0*cos.
                    let mut expected_keys = [0.0_f32; 32];
                    for index in [4, 6, 9, 16, 18, 21, 30] {
                        expected_keys[index] = -0.0;
                    }
                    assert_eq!(
                        keys.iter().map(|value| value.to_bits()).collect::<Vec<_>>(),
                        expected_keys
                            .iter()
                            .map(|value| value.to_bits())
                            .collect::<Vec<_>>()
                    );
                    let mut expected = [0.0_f32; 32];
                    if layer == 1 {
                        // Token rows normalize [1,0] with epsilon one,
                        // then round 1/sqrt(1.5) to binary16. Audio row
                        // nine is [2,-3]/sqrt(7.5), also rounded to half.
                        for row in expected.as_chunks_mut::<2>().0 {
                            row[0] = 0.816_406_25;
                        }
                        expected[18..20].copy_from_slice(&[0.730_468_75, -1.095_703_1]);
                    }
                    assert_eq!(
                        values
                            .iter()
                            .map(|value| value.to_bits())
                            .collect::<Vec<_>>(),
                        expected
                            .iter()
                            .map(|value| value.to_bits())
                            .collect::<Vec<_>>()
                    );
                }
                assert!(state.cache(state.layer_count()).is_none());
            }
        }
        Ok(())
    })?;
    assert_eq!(transcript, "");
    assert_eq!(events, ["frontend", "encoder", "prompt", "decoder"]);
    assert_cleared(&recognizer);
    Ok(())
}

#[test]
fn rust_asr_observer_every_generation_step_is_observed_before_failed_request_reset()
-> io::Result<()> {
    let mut recognizer = recognizer(1)?;
    let mut decoder_steps = 0;
    assert!(
        recognizer
            .transcribe_observed(&[0.0; 201], &mut || false, &mut |event| {
                if let Observation::Decoder {
                    generation_index,
                    logits,
                    token,
                    state,
                } = event
                {
                    assert_eq!(generation_index, decoder_steps);
                    assert_eq!(state.position(), 16 + decoder_steps);
                    assert_eq!(token, 0);
                    assert_eq!(logits.len(), 11);
                    for layer in 0..state.layer_count() {
                        let (keys, values) = state
                            .cache(layer)
                            .ok_or_else(|| io::Error::other("missing observed layer"))?;
                        assert_eq!(keys.len(), 2 * state.position());
                        assert_eq!(values.len(), keys.len());
                        assert!(keys.iter().all(|value| *value == 0.0));
                        assert_eq!(values.last().copied(), Some(0.0));
                        if layer == 0 {
                            assert!(values.iter().all(|value| value.to_bits() == 0));
                        } else {
                            assert_eq!(values[0], 0.816_406_25);
                            assert_eq!(values[18..20], [0.730_468_75, -1.095_703_1]);
                        }
                    }
                    decoder_steps += 1;
                }
                Ok(())
            })
            .is_err()
    );
    assert_eq!(decoder_steps, 64);
    assert_cleared(&recognizer);
    Ok(())
}

#[test]
fn rust_asr_observer_refusal_at_each_stage_preserves_error_and_fresh_replay() -> io::Result<()> {
    for rejected in 0..4 {
        let mut recognizer = recognizer(0)?;
        let mut observed = 0;
        let result = recognizer.transcribe_observed(&[0.0; 201], &mut || false, &mut |_| {
            let index = observed;
            observed += 1;
            if index == rejected {
                Err(io::Error::from(io::ErrorKind::PermissionDenied))
            } else {
                Ok(())
            }
        });
        assert_eq!(
            result.err().map(|error| error.kind()),
            Some(io::ErrorKind::PermissionDenied)
        );
        assert_eq!(observed, rejected + 1);
        assert_cleared(&recognizer);
        assert_eq!(recognizer.transcribe(&[0.0; 201])?, "");
    }
    Ok(())
}

#[test]
fn rust_asr_observer_unwind_and_post_observation_cancel_clear_complete_state() -> io::Result<()> {
    let mut recognizer = recognizer(0)?;
    let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = recognizer.transcribe_observed(&[0.0; 201], &mut || false, &mut |event| {
            assert!(
                !matches!(event, Observation::Decoder { .. }),
                "injected observer panic"
            );
            Ok(())
        });
    }));
    assert!(unwind.is_err());
    assert_cleared(&recognizer);
    let cancelled = std::cell::Cell::new(false);
    let result =
        recognizer.transcribe_observed(&[0.0; 201], &mut || cancelled.get(), &mut |event| {
            if matches!(event, Observation::Decoder { .. }) {
                cancelled.set(true);
            }
            Ok(())
        });
    assert_eq!(
        result.err().map(|error| error.kind()),
        Some(io::ErrorKind::Interrupted)
    );
    assert_cleared(&recognizer);
    assert_eq!(recognizer.transcribe(&[0.0; 201])?, "");
    Ok(())
}
