//! Model-free coverage of warm synthesis admission and observer cleanup.

use std::{io, panic::AssertUnwindSafe, path::PathBuf};

use super::{Codec, DecodeState, Encoder, KvState, Transformer, TransformerState};
use crate::{
    MAX_TEXT, MAX_VOICE, SynthesisGuard, Synthesizer, Voice, VoiceCache,
    codec::{
        CausalConv1d, CodecStreamState, DacDecoder, PadMode, RvqDecoder, RvqEncodeSide, RvqGroup,
        SnakeBeta,
    },
    observation::Observation,
    rust_talker::{Reference, tests::prompt_fixture},
};

fn transformer() -> Transformer {
    Transformer {
        threads: 1,
        hidden: 1,
        heads: 1,
        kv_heads: 1,
        head_dim: 1,
        window: None,
        theta: 10_000.0,
        eps: 1e-6,
        input: None,
        output: None,
        final_norm: None,
        layers: Vec::new(),
    }
}

fn warm_synthesizer() -> io::Result<Synthesizer> {
    let convolution = CausalConv1d {
        out_c: 1,
        in_c: 1,
        kernel: 2,
        dilation: 1,
        stride: 1,
        pad_mode: PadMode::Constant,
        weight: vec![0.0; 2],
        bias: None,
    };
    let group = RvqGroup {
        num_codebooks: 1,
        codebook_size: 1,
        codebook_dim: 1,
        hidden_dim: 1,
        codebooks: vec![vec![0.0]],
        out_proj_w: vec![1.0],
    };
    let side = RvqEncodeSide::new(1, 1, 1, vec![1.0], vec![vec![0.0]])?;
    let state = DecodeState {
        audio: CodecStreamState {
            pos: 7,
            pre_conv: vec![2.0],
            ..CodecStreamState::default()
        },
        transformer: TransformerState {
            position: 1,
            layers: vec![KvState {
                keys: vec![-0.0],
                values: vec![3.0],
            }],
        },
    };
    // These tiny graphs are not an acoustic oracle. Every public-method case
    // stops during conditioning, before codec inference or model loading.
    let codec = Codec {
        threads: 1,
        quantizer: RvqDecoder {
            semantic: group.clone(),
            acoustic: group,
        },
        pre: convolution.clone(),
        transformer: transformer(),
        upsample: Vec::new(),
        dac: DacDecoder {
            conv_pre: convolution.clone(),
            blocks: Vec::new(),
            snake_post: SnakeBeta {
                a: vec![1.0],
                inv_b: vec![1.0],
            },
            conv_post: convolution.clone(),
        },
        encoder: Encoder {
            threads: 1,
            first: convolution.clone(),
            stages: Vec::new(),
            last: convolution.clone(),
            transformer: transformer(),
            down: convolution,
            semantic: side.clone(),
            acoustic: side,
        },
        primed: Some((vec![[0; 16]], state.clone())),
        state,
    };
    Ok(Synthesizer {
        voices: VoiceCache(Some((
            c"warm".to_owned(),
            Voice {
                talker: prompt_fixture(true)?,
                reference: Reference {
                    speaker: vec![0.25],
                    codes: vec![[0; 16]],
                },
            },
        ))),
        design: None,
        model_path: PathBuf::new(),
        voice_model_path: PathBuf::new(),
        codec,
        threads: 1,
    })
}

fn assert_stream_reset(codec: &Codec) {
    assert_eq!(codec.state.audio.pos, 0);
    assert_eq!(codec.state.audio.pre_conv, [0.0]);
    assert_eq!(codec.state.transformer.position, 0);
    assert!(codec.state.transformer.layers.is_empty());
}

#[test]
fn synthesis_observer_conditioning_refusal_and_unwind_discard_warm_reuse() -> io::Result<()> {
    for stop in [1, 2] {
        for unwind in [false, true] {
            let mut synthesizer = warm_synthesizer()?;
            let mut observed = 0;
            let mut delivered = 0;
            let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
                synthesizer.synthesize_stream_observed(
                    "hello",
                    "warm",
                    &mut |_| {
                        delivered += 1;
                        Ok(())
                    },
                    &mut |event| {
                        observed += 1;
                        match (observed, event) {
                            (1, Observation::Speaker(speaker)) => assert_eq!(speaker, [0.25]),
                            (2, Observation::ReferenceCodes(codes)) => assert_eq!(codes, [[0; 16]]),
                            _ => return Err(io::Error::other("Unexpected graph execution.")),
                        }
                        if observed == stop {
                            if unwind {
                                std::panic::resume_unwind(Box::new("observer refused"));
                            }
                            return Err(io::Error::from(io::ErrorKind::Interrupted));
                        }
                        Ok(())
                    },
                )
            }));
            if unwind {
                assert!(result.is_err());
            } else {
                assert!(result.is_ok_and(|result| {
                    result.is_err_and(|error| error.kind() == io::ErrorKind::Interrupted)
                }));
            }
            assert_eq!(observed, stop);
            assert_eq!(delivered, 0);
            assert!(synthesizer.voices.0.is_none());
            assert!(synthesizer.codec.primed.is_none());
            assert_stream_reset(&synthesizer.codec);
        }
    }
    Ok(())
}

#[test]
fn synthesis_observer_rejected_input_preserves_warm_voice_reference_and_stream_state()
-> io::Result<()> {
    for (text, voice) in [
        (String::new(), "warm".to_owned()),
        ("x".repeat(MAX_TEXT + 1), "warm".to_owned()),
        ("hello\0private".to_owned(), "warm".to_owned()),
        ("hello".to_owned(), " \n\t".to_owned()),
        ("hello".to_owned(), "x".repeat(MAX_VOICE + 1)),
        ("hello".to_owned(), "warm\0private".to_owned()),
    ] {
        let mut synthesizer = warm_synthesizer()?;
        let mut observed = 0;
        let mut delivered = 0;
        let result = synthesizer.synthesize_stream_observed(
            &text,
            &voice,
            &mut |_| {
                delivered += 1;
                Ok(())
            },
            &mut |_| {
                observed += 1;
                Ok(())
            },
        );
        assert!(result.is_err_and(|error| error.kind() == io::ErrorKind::InvalidInput));
        assert_eq!((observed, delivered), (0, 0));
        assert!(
            synthesizer
                .voices
                .0
                .as_ref()
                .is_some_and(|(key, _)| key.as_c_str() == c"warm")
        );
        assert!(synthesizer.codec.primed.is_some());
        assert_eq!(synthesizer.codec.state.audio.pos, 7);
        assert_eq!(synthesizer.codec.state.audio.pre_conv, [2.0]);
        assert_eq!(synthesizer.codec.state.transformer.position, 1);
        assert_eq!(
            synthesizer.codec.state.transformer.layers[0].keys[0].to_bits(),
            (-0.0_f32).to_bits()
        );
    }
    Ok(())
}

#[test]
fn synthesis_observer_completed_guard_resets_stream_but_retains_warm_reuse() -> io::Result<()> {
    let mut synthesizer = warm_synthesizer()?;
    drop(SynthesisGuard {
        synthesizer: &mut synthesizer,
        complete: true,
    });
    assert!(synthesizer.voices.0.is_some());
    assert!(synthesizer.codec.primed.is_some());
    assert_stream_reset(&synthesizer.codec);
    Ok(())
}
