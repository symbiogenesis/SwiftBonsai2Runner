use std::io;

use super::super::{
    LinearProjection, WeightQuantization, apply_hadamard_sign_fold, repack_pq2_to_ptq1,
};

#[test]
fn projection_fold_reuses_borrowed_storage_and_owns_only_the_fallback() -> io::Result<()> {
    use std::borrow::Cow;

    let input = [1.0, 2.0, 3.0, 4.0];
    let signs = [1, -1, 1, -1];
    let cached = [-1.0_f32, 5.0, -0.0, -2.0];
    let reused = super::folded(&input, Some(&cached), &signs, 4)?;
    assert!(matches!(reused, Cow::Borrowed(_)));
    assert_eq!(reused.as_ptr(), cached.as_ptr());
    assert_eq!(reused[2].to_bits(), (-0.0_f32).to_bits());

    // Missing or wrong-width cache rows must compute a new complete fold,
    // rather than borrow input storage or reuse only part of a cached row.
    for cached in [None, Some(&cached[..3]), Some(&[][..])] {
        let computed = super::folded(&input, cached, &signs, 4)?;
        assert!(matches!(computed, Cow::Owned(_)));
        assert_eq!(computed.as_ref(), [-1.0, 5.0, 0.0, -2.0]);
        assert_ne!(computed.as_ptr(), input.as_ptr());
    }
    assert_eq!(input, [1.0, 2.0, 3.0, 4.0]);
    Ok(())
}

fn fixture() -> WeightQuantization {
    WeightQuantization::PQ2_0 {
        group_size: 8,
        scales: vec![0.5, 0.25],
        packed_trits: vec![0xa4, 0x61, 0x61, 0xa4],
        hadamard_block: 8,
        signs: vec![1, -1, 1, 1, -1, 1, -1, 1],
    }
}

fn reject(weight: WeightQuantization) {
    assert!(
        LinearProjection::new("talker.blk.0.attn_q.weight", 8, 2, weight.clone(), None).is_err()
    );
    // Public fields can bypass construction or change after admission.
    let projection = LinearProjection {
        name: "talker.blk.0.attn_q.weight".into(),
        in_dim: 8,
        out_dim: 2,
        weight,
        bias: None,
    };
    assert!(projection.forward_vec(&[0.0; 8], None).is_err());
    assert!(
        projection
            .forward_vec_with_pre_folded(&[0.0; 8], Some(&[0.0; 8]), None, 1)
            .is_err()
    );
}

#[test]
fn projection_layout_rejects_malformed_ternary_before_division_or_fold_reuse() -> io::Result<()> {
    for weight in [fixture(), repack_pq2_to_ptq1(&fixture())?] {
        for fault in 0..8 {
            let mut malformed = weight.clone();
            match &mut malformed {
                WeightQuantization::PQ2_0 {
                    group_size,
                    scales,
                    packed_trits,
                    hadamard_block,
                    signs,
                } => match fault {
                    0 => *group_size = 0,
                    1 => *group_size = 3,
                    2 => *group_size = 16,
                    3 => scales.truncate(1),
                    4 => packed_trits.truncate(3),
                    5 => *hadamard_block = 0,
                    6 => *hadamard_block = 3,
                    _ => signs.truncate(7),
                },
                WeightQuantization::PTQ1_0 {
                    group_size,
                    scales,
                    nz_mask,
                    sign_mask,
                    hadamard_block,
                    signs,
                } => match fault {
                    0 => *group_size = 0,
                    1 => *group_size = 3,
                    2 => nz_mask.truncate(1),
                    3 => scales.truncate(1),
                    4 => sign_mask.truncate(1),
                    5 => *hadamard_block = 0,
                    6 => *hadamard_block = 3,
                    _ => signs.truncate(7),
                },
                _ => return Err(io::Error::other("Expected a ternary fixture.")),
            }
            reject(malformed);
        }
    }
    Ok(())
}

#[test]
fn projection_layout_rejects_overflow_and_empty_dimensions_before_allocation() -> io::Result<()> {
    for weight in [
        fixture(),
        repack_pq2_to_ptq1(&fixture())?,
        WeightQuantization::F32(vec![0.0; 16]),
        WeightQuantization::Packed {
            ggml_type: 0,
            bytes: vec![0; 64],
        },
        WeightQuantization::Q8_0 { blocks: vec![] },
    ] {
        for (input, output) in [(64, usize::MAX), (0, 2), (8, 0)] {
            assert!(
                LinearProjection::new(
                    "talker.blk.0.attn_q.weight",
                    input,
                    output,
                    weight.clone(),
                    None
                )
                .is_err()
            );
            let projection = LinearProjection {
                name: "talker.blk.0.attn_q.weight".into(),
                in_dim: input,
                out_dim: output,
                weight: weight.clone(),
                bias: None,
            };
            assert!(projection.forward_vec(&[0.0; 64][..input], None).is_err());
        }
    }
    Ok(())
}

#[test]
fn projection_layout_preserves_ternary_fold_reuse_fallback_and_repack_bits() -> io::Result<()> {
    let input = [0.5_f32, -1.25, 2.0, 0.75, -0.5, 1.5, -2.25, 0.125];
    let folded = apply_hadamard_sign_fold(&input, &[1, -1, 1, 1, -1, 1, -1, 1], 8)?;
    let mut outputs = Vec::new();
    for weight in [fixture(), repack_pq2_to_ptq1(&fixture())?] {
        let projection = LinearProjection::new(
            "talker.blk.0.attn_q.weight",
            8,
            2,
            weight,
            Some(vec![0.1, -0.1]),
        )?;
        let output = projection.forward_vec(&input, None)?;
        for reuse in [Some(folded.as_slice()), Some(&folded[..4])] {
            for workers in [1, 4] {
                assert_eq!(
                    output
                        .iter()
                        .map(|value| value.to_bits())
                        .collect::<Vec<_>>(),
                    projection
                        .forward_vec_with_pre_folded(&input, reuse, None, workers)?
                        .iter()
                        .map(|value| value.to_bits())
                        .collect::<Vec<_>>()
                );
            }
        }
        outputs.push(
            output
                .iter()
                .map(|value| value.to_bits())
                .collect::<Vec<_>>(),
        );
    }
    assert_eq!(outputs[0], outputs[1]);
    Ok(())
}
