//! Independent original-layout controls for the conditional HMX bitplane bridge.

use super::*;
use crate::{Plan, prepare_q8k, tiled_projection};

#[test]
fn every_signed_i8_code_and_original_symbol_reconstructs_without_quantizer_changes()
-> Result<(), Error> {
    for packed in [0x00_u8, 0x55, 0xaa, 0xff, 0xe4] {
        let mut original = [packed; 34];
        original[..2].copy_from_slice(&0x3c00_u16.to_le_bytes());
        for code in i8::MIN..=i8::MAX {
            let codes = [code; 128];
            let inputs = HmxQ2Inputs::new(&original, &codes)?;
            for record in inputs.logical_q2_blocks().chunks_exact(18) {
                assert_eq!(&record[..2], &[0, 0x3c]);
                assert_eq!(&record[2..], &[packed; 16]);
            }
            let mut expected_dot = 0_i32;
            for lane in 0..128 {
                let symbol = (packed >> (lane % 4 * 2)) & 3;
                expected_dot += (i32::from(symbol) - 1) * i32::from(code);
                let mut decoded = 0_i32;
                for plane in 0..8 {
                    let bit = (code.cast_unsigned() >> plane) & 1;
                    assert_eq!(inputs.activation_planes()[plane][lane], f32::from(bit));
                    decoded += i32::from(bit) * if plane == 7 { -128 } else { 1 << plane };
                }
                assert_eq!(decoded, i32::from(code));
            }
            let planes = inputs.cpu_control_dots()?;
            for (plane, &dot) in planes.iter().enumerate() {
                let bit = i32::from((code.cast_unsigned() >> plane) & 1);
                let expected = (0..128)
                    .map(|lane| (i32::from((packed >> (lane % 4 * 2)) & 3) - 1) * bit)
                    .sum::<i32>();
                assert_eq!(dot, expected as f32);
                assert!((-128.0..=256.0).contains(&dot));
            }
            assert_eq!(
                reduce_hmx_original_block(&original, 1.0, 0.0, planes)?.to_bits(),
                (expected_dot as f32).to_bits()
            );
        }
    }
    Ok(())
}

#[test]
fn retained_fma_golden_distinguishes_block_order_from_nonfused_sum() -> Result<(), Error> {
    let mut first = [0xaa; 34];
    first[..2].copy_from_slice(&0x3c01_u16.to_le_bytes());
    let mut second = [0xaa; 34];
    second[..2].copy_from_slice(&0x3787_u16.to_le_bytes());
    let first_planes = HmxQ2Inputs::new(&first, &[3; 128])?.cpu_control_dots()?;
    let second_planes = HmxQ2Inputs::new(&second, &[-13; 128])?.cpu_control_dots()?;
    assert_eq!(first_planes, [128.0, 128.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    assert_eq!(
        second_planes,
        [128.0, 128.0, 0.0, 0.0, 128.0, 128.0, 128.0, 128.0]
    );
    let scale = f32::from_bits(0x3da0_0001);
    let prior = reduce_hmx_original_block(&first, scale, 0.0, first_planes)?;
    let result = reduce_hmx_original_block(&second, scale, prior, second_planes)?;
    // Retained original-PQ2 golden: separately rounded products yield
    // 0xc1f90b02, one ULP away from this required ordered FMA result.
    assert_eq!(result.to_bits(), 0xc1f9_0b01);
    assert_eq!(
        reduce_hmx_original_block(&first, 1.0, -0.0, [-0.0; 8])?.to_bits(),
        0.0_f32.to_bits()
    );
    for malformed in [&first[..0], &first[..33], &[0; 35][..]] {
        assert_eq!(
            HmxQ2Inputs::new(malformed, &[0; 128]).err(),
            Some(Error::Extent)
        );
        assert_eq!(
            reduce_hmx_original_block(malformed, 1.0, 0.0, [0.0; 8]),
            Err(Error::Extent)
        );
    }
    Ok(())
}

fn fixture(columns: usize, rows: usize) -> (Vec<u8>, Vec<f32>) {
    let mut weights = vec![0_u8; columns / 128 * rows * 34];
    for (block_index, block) in weights.chunks_exact_mut(34).enumerate() {
        block[..2].copy_from_slice(
            &[0x3c01_u16, 0x3787, 0x8000, 0x0001, 0xbc00][block_index % 5].to_le_bytes(),
        );
        for (index, byte) in block[2..].iter_mut().enumerate() {
            *byte = ((index * 37 + block_index * 53) % 256) as u8;
        }
    }
    let mut input: Vec<_> = (0..columns)
        .map(|lane| ((lane * 71 % 255) as f32 - 127.0) * 0.03125)
        .collect();
    // The selected positive maximum yields a negative Q8_K scale and exercises
    // sign-plane reconstruction independently of the raw PQ2 scale sign.
    for block in input.chunks_exact_mut(256) {
        block[0] = 127.0 * 0.03125;
        block[1] = -127.0 * 0.03125;
    }
    (weights, input)
}

// Decode the ORIGINAL adjacent two-bit PQ2 layout, never the bridge's tiled
// layout, and compute each plane dot directly from the original signed codes.
fn original_plane_dots(
    operation: &HmxQ2Operation,
    weights: &[u8],
    prepared: &[u8],
    columns: usize,
) -> Vec<f32> {
    let mut output = vec![0.0; operation.rows() * 8];
    let block_index = operation.block_index();
    let code_start = block_index / 2 * 292 + 4 + block_index % 2 * 128;
    for plane in 0..8 {
        for row in 0..operation.rows() {
            let start = ((operation.row_start() + row) * (columns / 128) + block_index) * 34;
            let mut dot = 0_i32;
            for lane in 0..128 {
                let symbol = (weights[start + 2 + lane / 4] >> (lane % 4 * 2)) & 3;
                let bit = (prepared[code_start + lane] >> plane) & 1;
                dot += (i32::from(symbol) - 1) * i32::from(bit);
                assert_eq!(operation.activation_planes()[plane][lane], f32::from(bit));
            }
            assert!((-128..=256).contains(&dot));
            output[plane * operation.rows() + row] = dot as f32;
        }
    }
    for lane in 0..128 {
        let reconstructed = (0..7)
            .map(|plane| operation.activation_planes()[plane][lane] as i32 * (1 << plane))
            .sum::<i32>()
            - operation.activation_planes()[7][lane] as i32 * 128;
        assert_eq!(
            reconstructed,
            i32::from(i8::from_ne_bytes([prepared[code_start + lane]]))
        );
    }
    output
}

#[test]
fn original_layout_oracle_preserves_signed_planes_fourth_symbol_and_ordered_fma()
-> Result<(), Error> {
    let columns = 768;
    let rows = 35;
    let (weights, input) = fixture(columns, rows);
    let prepared = prepare_q8k(&input)?;
    assert_eq!(prepared[4], (-127_i8).cast_unsigned());
    assert_eq!(prepared[5], 127);
    let mut expected = Vec::new();
    tiled_projection(
        Plan::new(columns, rows, 32, 2, usize::MAX)?,
        &weights,
        &input,
        &mut expected,
        || true,
    )?;
    for tile_rows in [1, 31, 32, 33, 35] {
        let plan = HmxQ2Plan::new(columns, rows, tile_rows, usize::MAX)?;
        let mut result = vec![99.0];
        let mut sequence = 0;
        hmx_q2_projection(
            plan,
            &weights,
            &input,
            &mut result,
            |operation, dots| {
                assert_eq!(operation.sequence(), sequence);
                let oracle = original_plane_dots(operation, &weights, &prepared, columns);
                let mut replay = vec![f32::NAN; dots.len()];
                operation.cpu_control_dots(&mut replay)?;
                assert_eq!(replay, oracle);
                // The independent original-layout oracle supplies the returned
                // values, so an incorrect bridge decoder cannot validate itself.
                dots.copy_from_slice(&oracle);
                sequence += 1;
                Ok(())
            },
            || true,
        )?;
        assert_eq!(sequence, columns / 128 * rows.div_ceil(tile_rows));
        assert_eq!(
            result
                .iter()
                .map(|value| value.to_bits())
                .collect::<Vec<_>>(),
            expected
                .iter()
                .map(|value| value.to_bits())
                .collect::<Vec<_>>()
        );
    }
    Ok(())
}

#[test]
fn host_tiles_have_literal_576_byte_stride_and_scale_one_without_guard_lanes() -> Result<(), Error>
{
    let columns = 256;
    let rows = 33;
    let mut weights = vec![0; rows * 2 * 34];
    for block in weights.chunks_exact_mut(34) {
        block[..2].copy_from_slice(&0x3c00_u16.to_le_bytes());
        for (index, byte) in block[2..].iter_mut().enumerate() {
            *byte = [0x00, 0x55, 0xaa, 0xff][index % 4];
        }
    }
    let input = vec![1.0; columns];
    let mut output = Vec::new();
    hmx_q2_projection(
        HmxQ2Plan::new(columns, rows, rows, usize::MAX)?,
        &weights,
        &input,
        &mut output,
        |operation, dots| {
            let wire = operation.repacked_weights();
            assert_eq!(wire.len(), 2304);
            for start in [0, 576] {
                assert_eq!(&wire[start..start + 512], &[0xe4; 512]);
                for scale in wire[start + 512..start + 576].chunks_exact(2) {
                    assert_eq!(scale, [0, 0x3c]);
                }
            }
            // One active row in the next row tile; neither host K-tile uses
            // the separate VTCM destination's 640-byte alignment stride.
            for start in [1152, 1728] {
                for offset in [0, 128, 256, 384] {
                    assert_eq!(&wire[start + offset..start + offset + 4], &[0xe4; 4]);
                    assert!(
                        wire[start + offset + 4..start + offset + 128]
                            .iter()
                            .all(|&byte| byte == 0)
                    );
                }
                assert_eq!(&wire[start + 512..start + 514], &[0, 0x3c]);
                assert!(wire[start + 514..start + 576].iter().all(|&byte| byte == 0));
            }
            operation.cpu_control_dots(dots)
        },
        || true,
    )?;
    assert_eq!(output.len(), rows);
    Ok(())
}

#[test]
fn invalid_or_partial_plane_results_never_publish_a_projection() -> Result<(), Error> {
    let (weights, input) = fixture(256, 2);
    let plan = HmxQ2Plan::new(256, 2, 2, usize::MAX)?;
    for invalid in [
        f32::NAN,
        f32::INFINITY,
        f32::NEG_INFINITY,
        0.5,
        -129.0,
        257.0,
    ] {
        let mut output = vec![99.0];
        assert_eq!(
            hmx_q2_projection(
                plan,
                &weights,
                &input,
                &mut output,
                |operation, dots| {
                    operation.cpu_control_dots(dots)?;
                    // The final plane/final row proves complete validation,
                    // rather than accepting the first valid returned element.
                    *dots.last_mut().ok_or(Error::Extent)? = invalid;
                    Ok(())
                },
                || true,
            ),
            Err(Error::Protocol)
        );
        assert!(output.is_empty());
    }
    for written in [0, 1, 15] {
        let mut output = vec![99.0];
        assert_eq!(
            hmx_q2_projection(
                plan,
                &weights,
                &input,
                &mut output,
                |_, dots| {
                    dots[..written].fill(0.0);
                    Ok(())
                },
                || true,
            ),
            Err(Error::Protocol)
        );
        assert!(output.is_empty());
    }
    Ok(())
}

#[test]
fn cancellation_after_native_completion_and_submission_error_discard_all_output()
-> Result<(), Error> {
    let (weights, input) = fixture(256, 2);
    let plan = HmxQ2Plan::new(256, 2, 2, usize::MAX)?;
    let mut checks = 0;
    let mut submitted = 0;
    let mut output = vec![99.0];
    assert_eq!(
        hmx_q2_projection(
            plan,
            &weights,
            &input,
            &mut output,
            |operation, dots| {
                submitted += 1;
                operation.cpu_control_dots(dots)
            },
            || {
                checks += 1;
                checks < 3
            },
        ),
        Err(Error::Cancelled)
    );
    assert_eq!(submitted, 1);
    assert!(output.is_empty());
    output.push(99.0);
    assert_eq!(
        hmx_q2_projection(
            plan,
            &weights,
            &input,
            &mut output,
            |_, _| Err(Error::Protocol),
            || true,
        ),
        Err(Error::Protocol)
    );
    assert!(output.is_empty());
    Ok(())
}

#[test]
fn malformed_final_scale_and_nonfinite_activation_reject_before_first_submission()
-> Result<(), Error> {
    let (mut weights, mut input) = fixture(256, 2);
    let plan = HmxQ2Plan::new(256, 2, 2, usize::MAX)?;
    let last = weights.len() - 34;
    for malformed_weights in [true, false] {
        if malformed_weights {
            weights[last..last + 2].copy_from_slice(&0x7c00_u16.to_le_bytes());
        } else {
            weights[last..last + 2].copy_from_slice(&0x3c00_u16.to_le_bytes());
            input[255] = f32::NAN;
        }
        let mut submitted = 0;
        let mut output = vec![99.0];
        assert_eq!(
            hmx_q2_projection(
                plan,
                &weights,
                &input,
                &mut output,
                |_, _| {
                    submitted += 1;
                    Ok(())
                },
                || true,
            ),
            Err(Error::Nonfinite)
        );
        assert_eq!(submitted, 0);
        assert!(output.is_empty());
    }
    Ok(())
}
