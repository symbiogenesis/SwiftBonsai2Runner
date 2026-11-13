//! Hand-calculated convolution, normalization, attention, sinusoid and tail controls.

use super::{
    Block, Config, Encoder, Linear, Norm, Projection, Weights, attention, convolve, numerics,
    position_add,
};
use std::io;

fn zero(input: usize, output: usize) -> io::Result<Linear> {
    Ok(Linear {
        weights: Projection::f32(input, output, vec![0.0; input * output])?,
        bias: vec![0.0; output],
    })
}
fn norm(width: usize) -> Norm {
    Norm {
        weight: vec![1.0; width],
        bias: vec![0.0; width],
    }
}

pub(super) fn fixture() -> io::Result<Encoder> {
    let c = Config {
        mel_bins: 1,
        channels: 1,
        hidden: 4,
        heads: 1,
        intermediate: 4,
        output: 2,
        window: 5,
        max_positions: 2,
    };
    let block = Block {
        attention_norm: norm(4),
        ffn_norm: norm(4),
        query: zero(4, 4)?,
        key: zero(4, 4)?,
        value: zero(4, 4)?,
        attention_out: zero(4, 4)?,
        ffn_in: zero(4, 4)?,
        ffn_out: zero(4, 4)?,
    };
    let mut output_out = zero(4, 2)?;
    output_out.bias = vec![2.0, -3.0];
    Encoder::new(
        c,
        Weights {
            convolutions: [zero(9, 1)?, zero(9, 1)?, zero(9, 1)?],
            convolution_out: Projection::f32(1, 4, vec![0.0; 4])?,
            blocks: vec![block],
            output_norm: norm(4),
            output_in: zero(4, 4)?,
            output_out,
        },
    )
}

#[test]
fn rust_asr_encoder_erf_gelu_known_answers_and_signed_zero() {
    for (input, expected) in [
        (-3.0, -0.004_049_694),
        (-1.0, -0.158_655_26),
        (0.0, 0.0),
        (1.0, 0.841_344_7),
        (3.0, 2.995_950_2),
        (20.0, 20.0),
    ] {
        assert!(
            (numerics::gelu(input) - expected).abs() < 0.000_000_3,
            "{input}"
        );
    }
    assert_eq!(numerics::gelu(-0.0).to_bits(), (-0.0_f32).to_bits());
}

#[test]
fn rust_asr_encoder_stride_padding_kernel_order_and_finite_output() -> io::Result<()> {
    // A 3x3 all-one kernel over [1..9], stride two and pad one,
    // sums four corners: 12,16,24,28. GELU rounds these back exactly.
    let kernel = Linear {
        weights: Projection::f32(9, 1, vec![1.0; 9])?,
        bias: vec![0.0],
    };
    assert_eq!(
        convolve(
            &kernel,
            &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0],
            3,
            3,
            1,
            &mut || false
        )?,
        [12.0, 16.0, 24.0, 28.0]
    );
    let kernel = Linear {
        weights: Projection::f32(9, 1, vec![0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0])?,
        bias: vec![0.0],
    };
    let output = convolve(
        &kernel,
        &[1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 3.0],
        3,
        3,
        1,
        &mut || false,
    )?;
    assert!((output[0] - 0.841_344_7).abs() < 0.000_000_1);
    assert!((output[3] - 2.995_950_2).abs() < 0.000_000_3);
    assert_eq!(&output[1..3], &[0.0; 2]);
    Ok(())
}

#[test]
fn rust_asr_encoder_layer_norm_bias_and_bidirectional_attention() -> io::Result<()> {
    let norm = Norm {
        weight: vec![2.0, 3.0],
        bias: vec![4.0, 5.0],
    };
    let output = norm.apply(&[1.0, 3.0])?;
    assert!((output[0] - 2.000_01).abs() < 0.000_001);
    assert!((output[1] - 7.999_985).abs() < 0.000_001);
    let keys = vec![vec![0.0; 4]; 2];
    let values = vec![vec![2.0, 4.0, 6.0, 8.0], vec![10.0, 12.0, 14.0, 16.0]];
    assert_eq!(
        attention(&[0.0; 4], &keys, &values, 2, &mut || false)?,
        [6.0, 8.0, 10.0, 12.0]
    );
    Ok(())
}

#[test]
fn rust_asr_encoder_sinusoid_chunk_positions() -> io::Result<()> {
    let mut zero = [0.0; 4];
    position_add(&mut zero, 0)?;
    assert_eq!(zero, [0.0, 0.0, 1.0, 1.0]);
    let mut one = [0.0; 4];
    position_add(&mut one, 1)?;
    for (actual, expected) in one.iter().zip([0.841_470_96, 0.000_1, 0.540_302_3, 1.0]) {
        assert!((*actual - expected).abs() <= 0.000_000_1);
    }
    Ok(())
}

#[test]
fn rust_asr_encoder_full_graph_independent_chunk_tail_rows() -> io::Result<()> {
    let encoder = fixture()?;
    // 10 mel rows per chunk -> 2 encoder rows, then one real tail row.
    // ceil(11/8)=2 would lose the independently padded chunk's third row.
    let output = encoder.encode_mel(&[0.0; 11], 11, &mut || false)?;
    assert_eq!(output.rows(), 3);
    assert_eq!(output.width(), 2);
    assert_eq!(output.values(), &[2.0, -3.0, 2.0, -3.0, 2.0, -3.0]);
    assert_eq!(encoder.encode_mel(&[0.0; 10], 10, &mut || false)?.rows(), 2);
    Ok(())
}

#[test]
fn rust_asr_encoder_failure_cancellation_and_replay() -> io::Result<()> {
    let encoder = fixture()?;
    assert!(encoder.encode_mel(&[f32::NAN], 1, &mut || false).is_err());
    assert!(encoder.encode_mel(&[], 0, &mut || false).is_err());
    assert!(encoder.encode_mel(&[0.0; 11], 10, &mut || false).is_err());
    let mut calls = 0;
    let result = encoder.encode_mel(&[0.0; 11], 11, &mut || {
        calls += 1;
        calls > 30
    });
    assert_eq!(
        result.err().map(|error| error.kind()),
        Some(io::ErrorKind::Interrupted)
    );
    assert_eq!(
        encoder.encode_mel(&[0.0], 1, &mut || false)?.values(),
        &[2.0, -3.0]
    );
    Ok(())
}
