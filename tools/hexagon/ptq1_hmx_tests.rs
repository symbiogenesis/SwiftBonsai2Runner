//! Retained native arithmetic and sparse-lane layout controls, with no device.

use super::*;

fn submit(inputs: &Ptq1HmxInputs, dots: &mut [f32; PLANES * LANES]) -> Result<(), Error> {
    inputs.cpu_control_dots(dots);
    Ok(())
}

#[test]
fn retained_native_ptq1_capture_preserves_complete_lane_fma_order() -> Result<(), Error> {
    let mut data = include_bytes!("fixtures/swift-ptq1-dot-parity.bin").as_slice();
    for expected_width in [128, 256] {
        let width = u32::from_le_bytes(data[..4].try_into().map_err(|_| Error::Extent)?) as usize;
        let native_bits = u32::from_le_bytes(data[4..8].try_into().map_err(|_| Error::Extent)?);
        assert_eq!(width, expected_width);
        let (weights, remaining) = data[8..].split_at(width / 128 * PTQ_BYTES);
        let (activations, remaining) = remaining.split_at(width / 32 * Q8_BYTES);
        data = remaining;
        let mut output = None;
        let mut operations = 0;
        hmx_ptq1_dot(
            weights,
            activations,
            &mut output,
            |inputs, dots| {
                operations += 1;
                submit(inputs, dots)
            },
            || true,
        )?;
        assert_eq!(operations, width / 32);
        assert_eq!(output.ok_or(Error::Protocol)?.to_bits(), native_bits);
    }
    assert!(data.is_empty());
    Ok(())
}

#[test]
fn every_native_fractional_alias_and_signed_code_keeps_sparse_integer_lanes() -> Result<(), Error> {
    for byte in u8::MIN..=u8::MAX {
        let mut original = [byte; PTQ_BYTES];
        original[26..].copy_from_slice(&0x3c00_u16.to_le_bytes());
        // Different phases exercise every code over the full byte inventory.
        let codes: [i8; K] =
            std::array::from_fn(|index| (byte.wrapping_add(index as u8)).cast_signed());
        // Independent fractional expansion proceeds packed byte by packed byte.
        let mut weights = [0_i32; K];
        for (base, width, count) in [(0, 16, 5), (80, 8, 5), (120, 2, 4)] {
            for position in 0..width {
                let mut fraction = u32::from(byte);
                for digit in 0..count {
                    fraction *= 3;
                    weights[base + digit * width + position] = (fraction / 256) as i32 - 1;
                    fraction %= 256;
                }
            }
        }
        for group in 0..4 {
            let inputs = Ptq1HmxInputs::new(&original, &codes, group)?;
            let mut dots = [f32::NAN; PLANES * LANES];
            inputs.cpu_control_dots(&mut dots);
            for lane in 0..LANES {
                let expected: i32 = [0, 16]
                    .into_iter()
                    .flat_map(|half| {
                        (0..4).map(move |offset| group * 32 + lane * 4 + half + offset)
                    })
                    .map(|index| weights[index] * i32::from(codes[index]))
                    .sum();
                assert_eq!(reconstruct(&dots, lane)?, expected);
            }
            // Every native row has unit scale; padding symbols decode to zero.
            for tile in inputs.repacked_weights().chunks_exact(576) {
                assert!(tile[512..].chunks_exact(2).all(|scale| scale == [0, 0x3c]));
                for lane_chunk in tile[..512].chunks_exact(128) {
                    assert!(lane_chunk[LANES * 4..].iter().all(|&byte| byte == 0x55));
                }
            }
        }
    }
    Ok(())
}

#[test]
fn faults_and_every_cancellation_boundary_discard_previous_output() -> Result<(), Error> {
    let mut original = [0; PTQ_BYTES];
    original[26..].copy_from_slice(&0x3c00_u16.to_le_bytes());
    let mut q8 = [0; 4 * Q8_BYTES];
    for activation in q8.chunks_exact_mut(Q8_BYTES) {
        activation[..2].copy_from_slice(&0x3c00_u16.to_le_bytes());
        activation[2..].fill(127);
    }
    for boundary in 0..9 {
        let mut output = Some(42.0);
        let mut checks = 0;
        assert_eq!(
            hmx_ptq1_dot(&original, &q8, &mut output, submit, || {
                let permitted = checks != boundary;
                checks += 1;
                permitted
            }),
            Err(Error::Cancelled)
        );
        assert_eq!(output, None);
    }
    for bad in [f32::NAN, f32::INFINITY, 0.5, 9.0, -9.0] {
        let mut output = Some(42.0);
        assert_eq!(
            hmx_ptq1_dot(
                &original,
                &q8,
                &mut output,
                |_, dots| {
                    dots.fill(0.0);
                    dots[0] = bad;
                    Ok(())
                },
                || true
            ),
            Err(Error::Protocol)
        );
        assert_eq!(output, None);
    }
    let mut output = Some(42.0);
    assert_eq!(
        hmx_ptq1_dot(
            &original,
            &q8,
            &mut output,
            |_, _| Err(Error::Protocol),
            || true
        ),
        Err(Error::Protocol)
    );
    assert_eq!(output, None);
    original[26..].copy_from_slice(&0x7c00_u16.to_le_bytes());
    let mut submitted = false;
    assert_eq!(
        hmx_ptq1_dot(
            &original,
            &q8,
            &mut output,
            |_, _| {
                submitted = true;
                Ok(())
            },
            || true
        ),
        Err(Error::Nonfinite)
    );
    assert!(!submitted);
    assert_eq!(output, None);
    Ok(())
}

#[test]
fn malformed_extents_groups_and_unwritten_results_reject() -> Result<(), Error> {
    assert!(matches!(
        Ptq1HmxInputs::new(&[0; 28], &[0; 127], 0),
        Err(Error::Extent)
    ));
    assert!(matches!(
        Ptq1HmxInputs::new(&[0; 28], &[0; 128], 4),
        Err(Error::Extent)
    ));
    let mut output = Some(42.0);
    for (weights, activation) in [
        (&[0; 27][..], &[0; 136][..]),
        (&[0; 28], &[0; 135]),
        (&[], &[]),
    ] {
        assert_eq!(
            hmx_ptq1_dot(weights, activation, &mut output, submit, || true),
            Err(Error::Extent)
        );
        assert_eq!(output, None);
    }
    assert_eq!(
        hmx_ptq1_dot(&[0; 28], &[0; 136], &mut output, |_, _| Ok(()), || true),
        Err(Error::Protocol)
    );
    assert_eq!(output, None);
    Ok(())
}
