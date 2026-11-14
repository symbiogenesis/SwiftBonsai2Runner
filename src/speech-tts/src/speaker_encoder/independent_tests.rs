//! Mathematical speaker controls, independent of native captures and candidate outputs.
//!
//! These small geometries test the serving primitives. They establish neither
//! pinned-weight x-vector parity nor voice identity or listening acceptance.

use super::{
    AttentiveStatPool, AudioMelConfig, MelSpectrogramExtractor, SeRes2NetBlock, SqueezeExcitation,
    TdnnConv1d,
};

fn close(actual: &[f32], expected: &[f64]) {
    assert_eq!(actual.len(), expected.len());
    for (index, (&actual, &expected)) in actual.iter().zip(expected).enumerate() {
        assert!(
            (f64::from(actual) - expected).abs() <= 3e-6,
            "speaker value {index}: {actual} differs from mathematical answer {expected}"
        );
    }
}

fn pointwise(out_c: usize, in_c: usize, weight: Vec<f32>, bias: Vec<f32>) -> TdnnConv1d {
    TdnnConv1d {
        out_c,
        in_c,
        kernel: 1,
        dilation: 1,
        weight,
        bias,
    }
}

#[test]
fn speaker_reference_slaney_has_independent_linear_and_logarithmic_anchors() {
    // Slaney's linear scale is 3/200 mel/Hz. Its logarithmic interval from
    // 1000 to 6400 Hz spans exactly 27 mel, independently of the implementation.
    close(
        &[
            super::hz_to_mel_slaney(0.0),
            super::hz_to_mel_slaney(200.0),
            super::hz_to_mel_slaney(1000.0),
            super::hz_to_mel_slaney(6400.0),
        ],
        &[0.0, 3.0, 15.0, 42.0],
    );
    for (mel, hz) in [(0.0, 0.0), (3.0, 200.0), (15.0, 1000.0), (42.0, 6400.0)] {
        assert!((super::mel_to_hz_slaney(mel) - hz).abs() <= 0.002);
    }
}

#[test]
fn speaker_reference_mel_matches_eight_point_dft_and_channel_major_layout() -> std::io::Result<()> {
    // Below 1 kHz Slaney is linear. At sr=8, FFT=8, the three normalized
    // triangles have unit weights at exactly bins 1, 2, 3 and zero elsewhere.
    // Reflection places the two impulses at Hann positions 4 and 5 in frame
    // zero, then 0 and 1 in frame one. The reflected impulse at zero has zero
    // Hann weight. Thus the first spectrum is |1 + a*exp(-i*pi*k/4)| and the
    // second is the constant b, without calling any candidate math helper.
    let extractor = MelSpectrogramExtractor::new(AudioMelConfig {
        sample_rate: 8,
        n_fft: 8,
        hop: 4,
        n_mels: 3,
        fmin: 0.0,
        fmax: 4.0,
    })?;
    let (actual, frames) = extractor.extract(&[0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0])?;
    assert_eq!(frames, 2);
    let root_two = 2.0_f64.sqrt();
    let a = (2.0 + root_two) / 4.0;
    let b = (2.0 - root_two) / 4.0;
    let tail = b.mul_add(b, 1e-9).ln() / 2.0;
    let base = a.mul_add(a, 1.0);
    close(
        &actual,
        &[
            (root_two.mul_add(a, base) + 1e-9).ln() / 2.0,
            tail,
            (base + 1e-9).ln() / 2.0,
            tail,
            (root_two.mul_add(-a, base) + 1e-9).ln() / 2.0,
            tail,
        ],
    );
    let (silence, _) = extractor.extract(&[0.0; 8])?;
    close(&silence, &[1e-9_f64.ln() / 2.0; 6]);
    assert!(extractor.extract(&[0.0, f32::NAN, 0.0]).is_err());
    assert!(extractor.extract(&[0.0; 2]).is_err());
    Ok(())
}

#[test]
fn speaker_reference_tdnn_preserves_signed_channels_dilation_and_reflection() -> std::io::Result<()>
{
    // Five inputs exercise both the four-channel SIMD group and its remainder.
    let point = pointwise(
        2,
        5,
        vec![1.0, -0.5, 2.0, 4.0, -1.0, -1.0, 0.0, 0.0, 2.0, 0.5],
        vec![0.5, -1.0],
    );
    let input = [
        1.0, 2.0, 3.0, 4.0, 5.0, 10.0, 20.0, 30.0, 40.0, 50.0, -1.0, -2.0, -3.0, -4.0, -5.0, 0.5,
        1.0, 1.5, 2.0, 2.5, 2.0, 1.0, 0.0, -1.0, -2.0,
    ];
    assert_eq!(
        point.forward(&input, 5, false)?,
        [-5.5, -8.5, -11.5, -14.5, -17.5, 0.0, -0.5, -1.0, -1.5, -2.0]
    );
    assert_eq!(point.forward(&input, 5, true)?, [0.0; 10]);
    let dilated = TdnnConv1d {
        out_c: 1,
        in_c: 2,
        kernel: 3,
        dilation: 2,
        weight: vec![1.0, 2.0, -1.0, 0.5, -1.0, 2.0],
        bias: vec![0.25],
    };
    // Reflected channel rows are [4,2,1,2,4,8,16,8,4] and
    // [4,8,16,8,4,2,1,2,4]; take offsets 0,2,4 at each position.
    assert_eq!(
        dilated.forward(
            &[1.0, 2.0, 4.0, 8.0, 16.0, 16.0, 8.0, 4.0, 2.0, 1.0],
            5,
            false
        )?,
        [-3.75, -1.75, -0.75, 16.25, 41.25]
    );
    assert!(dilated.forward(&[1.0; 4], 2, false).is_err());
    Ok(())
}

#[test]
fn speaker_reference_squeeze_excitation_uses_means_relu_and_distinct_gates() -> std::io::Result<()>
{
    let se = SqueezeExcitation {
        conv1: pointwise(2, 2, vec![1.0, -0.5, -1.0, 0.25], vec![0.0; 2]),
        conv2: pointwise(2, 2, vec![1.0, 0.75, -1.0, 2.0], vec![0.0; 2]),
    };
    // Means [2,2] project to [1,-1.5], then ReLU [1,0], then logits [1,-1].
    let positive = 1.0 / (1.0 + (-1.0_f64).exp());
    let negative = 1.0 / (1.0 + 1.0_f64.exp());
    close(
        &se.forward(&[1.0, 2.0, 3.0, 0.0, 2.0, 4.0], 3)?,
        &[
            positive,
            2.0 * positive,
            3.0 * positive,
            0.0,
            2.0 * negative,
            4.0 * negative,
        ],
    );
    assert!(se.forward(&[], 0).is_err());
    Ok(())
}

#[test]
fn speaker_reference_res2net_keeps_branch_carry_and_ungated_residual() -> std::io::Result<()> {
    let identity = vec![1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
    let block = SeRes2NetBlock {
        tdnn1: pointwise(3, 3, identity.clone(), vec![0.0; 3]),
        branches: vec![
            pointwise(1, 1, vec![2.0], vec![0.0]),
            pointwise(1, 1, vec![3.0], vec![0.0]),
        ],
        tdnn2: pointwise(3, 3, identity, vec![0.0; 3]),
        se: SqueezeExcitation {
            conv1: pointwise(1, 3, vec![0.0; 3], vec![0.0]),
            conv2: pointwise(3, 1, vec![0.0; 3], vec![0.0; 3]),
        },
        scale: 3,
    };
    // Branches are [1], [2*2=4], [(4+4)*3=24]. Zero SE logits halve
    // each channel before adding the original input, including negative input.
    assert_eq!(block.forward(&[1.0, 2.0, 4.0], 1)?, [1.5, 4.0, 16.0]);
    assert_eq!(block.forward(&[-1.0, 2.0, 4.0], 1)?, [-1.0, 4.0, 16.0]);
    assert!(block.forward(&[1.0, 2.0], 1).is_err());
    Ok(())
}

#[test]
fn speaker_reference_pooling_matches_closed_form_channel_specific_moments() -> std::io::Result<()> {
    let pool = AttentiveStatPool {
        tdnn: pointwise(1, 6, vec![0.5, -0.25, 1.0, -1.0, 0.5, 0.5], vec![0.0]),
        conv: pointwise(2, 1, vec![2.0, -1.0], vec![0.0; 2]),
    };
    // Channel rows [0,2], [1,3] have means [1,2] and population std [1,1].
    // Concatenated [input,mean,std] projects to [-.25,.25]; after ReLU/tanh
    // the attention rows are [0,2*tanh(.25)] and [0,-tanh(.25)]. For a
    // two-point distribution with distance 2, variance is 4*p*(1-p).
    let t = 0.25_f64.tanh();
    let p0 = 1.0 / (1.0 + (-2.0 * t).exp());
    let p1 = 1.0 / (1.0 + t.exp());
    close(
        &pool.forward(&[0.0, 2.0, 1.0, 3.0], 2, 2)?,
        &[
            2.0 * p0,
            2.0_f64.mul_add(p1, 1.0),
            (4.0 * p0).mul_add(1.0 - p0, 1e-12).sqrt(),
            (4.0 * p1).mul_add(1.0 - p1, 1e-12).sqrt(),
        ],
    );
    assert!(pool.forward(&[], 2, 0).is_err());
    Ok(())
}
