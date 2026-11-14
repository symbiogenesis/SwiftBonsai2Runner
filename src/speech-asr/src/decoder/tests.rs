//! Hand-calculated matrix, GQA, cache-rounding and state-publication controls.

use super::{Block, CachePrecision, Config, LayerState, Model, Projection, attend, cache_value};
use crate::protocol::PromptTokens;
use std::{io, sync::Arc};

fn projection(values: &[f32]) -> io::Result<Projection> {
    Projection::f32(2, 2, values.to_vec())
}

fn model() -> io::Result<Arc<Model>> {
    let identity = [1.0, 0.0, 0.0, 1.0];
    let zero = [0.0; 4];
    Ok(Arc::new(Model::new(
        config(),
        projection(&identity)?,
        vec![Block {
            attention_norm: vec![1.0; 2],
            ffn_norm: vec![1.0; 2],
            query: projection(&zero)?,
            key: projection(&zero)?,
            value: projection(&identity)?,
            attention_out: projection(&identity)?,
            query_norm: vec![1.0; 2],
            key_norm: vec![1.0; 2],
            gate: projection(&zero)?,
            up: projection(&identity)?,
            down: projection(&identity)?,
        }],
        vec![1.0; 2],
    )?))
}

const fn config() -> Config {
    Config {
        hidden: 2,
        heads: 1,
        kv_heads: 1,
        head_dim: 2,
        intermediate: 2,
        context: 64,
        epsilon: 3.0,
        rope_theta: 1_000_000.0,
        cache_precision: CachePrecision::F16,
    }
}

#[test]
fn rust_asr_decoder_f32_q8_signed_projection_answers() -> io::Result<()> {
    assert_eq!(
        projection(&[1.0, -2.0, 3.0, 4.0])?.multiply(&[5.0, 6.0], &mut || false)?,
        [-7.0, 39.0]
    );
    // Half scale 0x3800=0.5. Signed quants sum to -1.
    let mut block = vec![0; 34];
    block[..2].copy_from_slice(&0x3800_u16.to_le_bytes());
    block[2..6].copy_from_slice(&[1, 255, 127, 128]);
    let projection = Projection::q8(32, 1, block)?;
    assert_eq!(projection.multiply(&[1.0; 32], &mut || false)?, [-0.5]);
    assert!(Projection::q8(32, 1, vec![0; 33]).is_err());
    let mut bad_scale = vec![0; 34];
    bad_scale[..2].copy_from_slice(&0x7c00_u16.to_le_bytes());
    assert!(Projection::q8(32, 1, bad_scale).is_err());
    Ok(())
}

#[test]
fn rust_asr_decoder_binary16_ties_subnormals_and_overflow() -> io::Result<()> {
    let precision = CachePrecision::F16;
    assert_eq!(
        cache_value(1.0 + 1.0 / 2048.0, precision)?.to_bits(),
        1.0_f32.to_bits()
    );
    assert_eq!(
        cache_value(1.0 + 3.0 / 2048.0, precision)?.to_bits(),
        (1.0_f32 + 1.0 / 512.0).to_bits()
    );
    assert_eq!(
        cache_value(1.0 / 33_554_432.0, precision)?.to_bits(),
        0.0_f32.to_bits()
    );
    assert_eq!(
        cache_value(3.0 / 33_554_432.0, precision)?.to_bits(),
        (1.0_f32 / 8_388_608.0).to_bits()
    );
    assert_eq!(
        cache_value(-0.0, precision)?.to_bits(),
        (-0.0_f32).to_bits()
    );
    assert!(cache_value(65_520.0, precision).is_err());
    assert!(cache_value(-65_520.0, precision).is_err());
    assert_eq!(cache_value(65_504.0, precision)?, 65_504.0);
    assert_eq!(cache_value(-65_504.0, precision)?, -65_504.0);
    assert!(cache_value(f32::NAN, precision).is_err());
    assert_eq!(
        cache_value(1.0 + 1.0 / 2048.0, CachePrecision::F32)?,
        1.0 + 1.0 / 2048.0
    );
    Ok(())
}

#[test]
fn rust_asr_decoder_uniform_attention_and_grouped_head_mapping() -> io::Result<()> {
    let c = Config {
        heads: 4,
        kv_heads: 2,
        ..config()
    };
    // Zero Q produces uniform probabilities. Heads 0/1 share KV0; 2/3
    // share KV1, while each has a distinct output range.
    let old = LayerState {
        keys: vec![0.0; 4],
        values: vec![1.0, 3.0, 10.0, 30.0],
    };
    let output = attend(
        &[0.0; 8],
        &[0.0; 4],
        &[3.0, 5.0, 30.0, 50.0],
        &old,
        c,
        1,
        &mut || false,
    )?;
    assert_eq!(output, [2.0, 4.0, 2.0, 4.0, 20.0, 40.0, 20.0, 40.0]);
    Ok(())
}

#[test]
fn rust_asr_decoder_full_block_logits_complete_cache_and_reset() -> io::Result<()> {
    let model = model()?;
    let mut session = model.session();
    let logits = session.step_audio(&[1.0, 1.0], &mut || false)?;
    let expected = 1.5 / 5.25_f32.sqrt();
    assert_eq!(logits, [expected, expected]);
    assert_eq!(session.position(), 1);
    assert_eq!(session.cache(0), Some((&[0.0, 0.0][..], &[0.5, 0.5][..])));
    assert_eq!(
        session.step_audio(&[-1.0, -1.0], &mut || false)?,
        [-0.5, -0.5]
    );
    assert_eq!(
        session.cache(0),
        Some((&[0.0; 4][..], &[0.5, 0.5, -0.5, -0.5][..]))
    );
    session.reset();
    assert_eq!(session.position(), 0);
    assert_eq!(session.cache(0), Some((&[][..], &[][..])));
    assert_eq!(session.step_audio(&[1.0, 1.0], &mut || false)?, logits);
    // A separate session cannot inherit the first session's positions or KV.
    assert_eq!(model.session().position(), 0);
    Ok(())
}

#[test]
fn rust_asr_decoder_failures_cancellation_and_unwind_preserve_committed_bits() -> io::Result<()> {
    let mut session = model()?.session();
    let _ = session.step_audio(&[1.0, 1.0], &mut || false)?;
    let (keys, values) = session
        .cache(0)
        .ok_or_else(|| io::Error::other("missing test layer"))?;
    let original = (keys.to_vec(), values.to_vec());
    assert!(
        session
            .step_audio(&[f32::MAX, f32::MAX], &mut || false)
            .is_err()
    );
    assert!(session.step_audio(&[f32::NAN, 0.0], &mut || false).is_err());
    let mut calls = 0;
    let result = session.step_audio(&[1.0, 1.0], &mut || {
        calls += 1;
        calls == 7
    });
    assert_eq!(
        result.err().map(|error| error.kind()),
        Some(io::ErrorKind::Interrupted)
    );
    let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut calls = 0;
        let _ = session.step_audio(&[1.0, 1.0], &mut || {
            calls += 1;
            assert!(calls < 7, "injected decoder cancellation callback panic");
            false
        });
    }));
    assert!(unwind.is_err());
    assert_eq!(session.position(), 1);
    assert_eq!(
        session.cache(0),
        Some((original.0.as_slice(), original.1.as_slice()))
    );
    Ok(())
}

#[test]
fn rust_asr_decoder_prompt_audio_injection_and_failed_prefill_reset() -> io::Result<()> {
    let model = model()?;
    let prompt = PromptTokens::new([0; 6], [0; 3], 2)?.automatic_prompt(1, 512)?;
    let audio = [2.0, -2.0];
    let mut prefilled = model.session();
    let logits = prefilled.prefill(&prompt, &audio, &mut || false)?;
    let mut independent = model.session();
    let mut expected = Vec::new();
    for position in 0..prompt.ids().len() {
        expected = if prompt.audio_rows().contains(&position) {
            independent.step_audio(&audio, &mut || false)?
        } else {
            independent.step(0, &mut || false)?
        };
    }
    assert_eq!(logits, expected);
    assert_eq!(prefilled.cache(0), independent.cache(0));
    let mut calls = 0;
    assert!(
        prefilled
            .prefill(&prompt, &audio, &mut || {
                calls += 1;
                calls > 80
            })
            .is_err()
    );
    assert_eq!(prefilled.position(), 0);
    assert_eq!(prefilled.cache(0), Some((&[][..], &[][..])));
    Ok(())
}

#[test]
fn rust_asr_decoder_prefill_omits_unused_heads_and_keeps_final_failure_atomic() -> io::Result<()> {
    let mut model = Arc::try_unwrap(model()?).map_err(|_| io::Error::other("shared test model"))?;
    // Prompt token 0 is zero. An unselected vocabulary row [2,2], together
    // with the large final norm, overflows ONLY if an audio row's unused head
    // is evaluated. The final zero prompt row has literal all-zero logits.
    model.embedding = Projection::f32(2, 2, vec![0.0, 0.0, 2.0, 2.0])?;
    model.output_norm = vec![f32::MAX; 2];
    model.blocks[0].attention_out = projection(&[0.0; 4])?;
    let model = Arc::new(model);
    let prompt = PromptTokens::new([0; 6], [0; 3], 2)?.automatic_prompt(1, 512)?;
    let mut session = model.session();
    assert_eq!(
        session.prefill(&prompt, &[1.0, 1.0], &mut || false)?,
        [0.0; 2]
    );
    assert_eq!(session.position(), 16);
    let (_, values) = session
        .cache(0)
        .ok_or_else(|| io::Error::other("missing test layer"))?;
    let mut expected_values = [0.0; 32];
    expected_values[18..20].copy_from_slice(&[0.5; 2]);
    assert_eq!(values, expected_values);

    // A requested step still evaluates that head, fails before append and
    // leaves every previously committed K/V bit unchanged.
    let (keys, values) = session
        .cache(0)
        .ok_or_else(|| io::Error::other("missing test layer"))?;
    let original = (keys.to_vec(), values.to_vec());
    assert!(session.step_audio(&[1.0, 1.0], &mut || false).is_err());
    assert_eq!(session.position(), 16);
    assert_eq!(
        session.cache(0),
        Some((original.0.as_slice(), original.1.as_slice()))
    );

    // Selecting that nonzero token at the final prompt position must still
    // evaluate the failing head and reset all earlier successful prompt rows.
    let failing = PromptTokens::new([0, 0, 1, 0, 0, 0], [0; 3], 2)?.automatic_prompt(1, 512)?;
    assert!(
        session
            .prefill(&failing, &[1.0, 1.0], &mut || false)
            .is_err()
    );
    assert_eq!(session.position(), 0);
    assert_eq!(session.cache(0), Some((&[][..], &[][..])));
    assert_eq!(
        session.prefill(&prompt, &[1.0, 1.0], &mut || false)?,
        [0.0; 2]
    );
    Ok(())
}
