//! Safe-Rust replay of independently captured pinned masking, penalty and greedy observations.
//!
//! The immutable record formats and original source/compiler identities are in
//! `tests/fixtures/sampling-v1.provenance.json` and `greedy-v1.provenance.json`.
//! Expected outputs came from the native source before retirement, never Rust.
#![forbid(unsafe_code)]

use std::error::Error;

use sha2::{Digest, Sha256};

use crate::sampling::{
    SampleTopKPParams, greedy_argmax, repetition_penalty, sample_top_k_p, suppress,
};

const SAMPLING_FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/sampling-v1.bin");
const GREEDY_FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/greedy-v1.bin");
const CASE_COUNT: usize = 722;
const GREEDY_CASE_COUNT: usize = 714;
const SAMPLING_FIXTURE_SHA256: [u8; 32] = [
    0x0f, 0x75, 0x8e, 0xe6, 0xc9, 0x96, 0xb4, 0xc4, 0x4e, 0xcb, 0x75, 0x6b, 0x86, 0x64, 0xc6, 0xcc,
    0x0b, 0xd1, 0x7e, 0x60, 0xdc, 0x02, 0xda, 0x5f, 0x2d, 0xb2, 0x59, 0x70, 0xf6, 0xf4, 0xeb, 0x23,
];
const GREEDY_FIXTURE_SHA256: [u8; 32] = [
    0xff, 0xa6, 0xf1, 0x01, 0x3c, 0x35, 0xbc, 0x8a, 0xa4, 0x6a, 0x1b, 0xe6, 0x0e, 0xad, 0xc0, 0x2d,
    0x5b, 0xad, 0xcd, 0xc7, 0xc7, 0xf9, 0x7f, 0xb9, 0xc3, 0xaf, 0x84, 0xbb, 0x67, 0x61, 0x7e, 0xa7,
];

struct Case {
    logits: Vec<f32>,
    history: Vec<i32>,
    penalty: f32,
    low: i32,
    high: i32,
    keep: i32,
}

fn take<'a>(remaining: &mut &'a [u8], count: usize) -> Result<&'a [u8], Box<dyn Error>> {
    let (bytes, tail) = remaining
        .split_at_checked(count)
        .ok_or("Truncated sampling capture.")?;
    *remaining = tail;
    Ok(bytes)
}

fn word(remaining: &mut &[u8]) -> Result<u32, Box<dyn Error>> {
    Ok(u32::from_le_bytes(take(remaining, 4)?.try_into()?))
}

fn assert_bits(actual: &[f32], remaining: &mut &[u8], case: usize) -> Result<(), Box<dyn Error>> {
    for (index, value) in actual.iter().enumerate() {
        assert_eq!(
            value.to_bits(),
            word(remaining)?,
            "Sampler case {case}, logit {index}."
        );
    }
    Ok(())
}

fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    let edge = vec![
        0.0,
        -0.0,
        4.0,
        -4.0,
        f32::MIN_POSITIVE,
        f32::from_bits(1),
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::from_bits(0x7fc0_1234),
    ];
    for penalty in [
        0.0,
        -0.0,
        1.0,
        1.05,
        2.0,
        0.5,
        -2.0,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::from_bits(0x7fc0_2345),
    ] {
        for (low, high, keep) in [
            (i32::MIN, i32::MAX, 2),
            (0, 0, -1),
            (3, 1, 2),
            (0, -1, -1),
            (20, 30, -1),
            (0, 9, 8),
            (2, 7, 4),
        ] {
            for history in [
                vec![],
                vec![-1, i32::MIN, i32::MAX, 9],
                vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 2, 3, 0],
            ] {
                cases.push(Case {
                    logits: edge.clone(),
                    history,
                    penalty,
                    low,
                    high,
                    keep,
                });
            }
        }
    }
    // Bounded deterministic corpus independent of either policy implementation.
    let mut state = 0x1234_5678_u32;
    for index in 0..512 {
        let width = index % 67;
        let mut logits = Vec::new();
        let mut history = Vec::new();
        for _ in 0..width {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            logits.push(f32::from_bits(state));
            history.push(i32::from_ne_bytes((state % 80).to_ne_bytes()) - 8);
        }
        cases.push(Case {
            logits,
            history,
            penalty: 1.05,
            low: 7,
            high: 31,
            keep: 12,
        });
    }
    cases
}

#[test]
fn rust_sampler_helpers_match_captured_bits() -> Result<(), Box<dyn Error>> {
    let digest: [u8; 32] = Sha256::digest(SAMPLING_FIXTURE).into();
    assert_eq!(digest, SAMPLING_FIXTURE_SHA256);
    assert_eq!(SAMPLING_FIXTURE.len(), 306_584);
    let mut remaining = SAMPLING_FIXTURE;
    assert_eq!(take(&mut remaining, 8)?, b"BSMFv1\0\0");
    assert_eq!(word(&mut remaining)?, 1);
    assert_eq!(usize::try_from(word(&mut remaining)?)?, CASE_COUNT);
    let cases = cases();
    assert_eq!(cases.len(), CASE_COUNT);
    let mut output_words = 0;
    for (index, case) in cases.into_iter().enumerate() {
        let vocabulary = i32::try_from(case.logits.len())?;
        let history_count = i32::try_from(case.history.len())?;
        assert_eq!(word(&mut remaining)?, u32::try_from(vocabulary)?);
        assert_eq!(word(&mut remaining)?, u32::try_from(history_count)?);
        assert_eq!(word(&mut remaining)?, case.penalty.to_bits());
        for parameter in [case.low, case.high, case.keep] {
            assert_eq!(
                word(&mut remaining)?,
                u32::from_le_bytes(parameter.to_le_bytes())
            );
        }
        assert_bits(&case.logits, &mut remaining, index)?;
        for token in &case.history {
            assert_eq!(
                word(&mut remaining)?,
                u32::from_le_bytes(token.to_le_bytes())
            );
        }
        let mut logits = case.logits;
        let mut workspace = vec![255; logits.len()];
        suppress(&mut logits, case.low, case.high, case.keep);
        assert_bits(&logits, &mut remaining, index)?;
        assert!(repetition_penalty(
            &mut logits,
            &case.history,
            case.penalty,
            &mut workspace,
        ));
        assert_bits(&logits, &mut remaining, index)?;
        output_words += logits.len() * 2;
    }
    assert_eq!(output_words, 36_540);
    assert_eq!(remaining, [0_u8; 0]);
    Ok(())
}

#[test]
fn rust_greedy_argmax_matches_captured_arm64_bits() -> Result<(), Box<dyn Error>> {
    let digest: [u8; 32] = Sha256::digest(GREEDY_FIXTURE).into();
    assert_eq!(digest, GREEDY_FIXTURE_SHA256);
    assert_eq!(GREEDY_FIXTURE.len(), 81_664);
    let mut remaining = GREEDY_FIXTURE;
    assert_eq!(take(&mut remaining, 8)?, b"BSGAv1\0\0");
    assert_eq!(word(&mut remaining)?, 1);
    assert_eq!(usize::try_from(word(&mut remaining)?)?, GREEDY_CASE_COUNT);
    let cases: Vec<_> = cases()
        .into_iter()
        .filter(|case| !case.logits.is_empty())
        .collect();
    assert_eq!(cases.len(), GREEDY_CASE_COUNT);
    for (index, case) in cases.into_iter().enumerate() {
        let vocabulary = i32::try_from(case.logits.len())?;
        assert_eq!(word(&mut remaining)?, u32::try_from(vocabulary)?);
        assert_bits(&case.logits, &mut remaining, index)?;
        let expected = i32::from_le_bytes(word(&mut remaining)?.to_le_bytes());
        let expected_dump_bits = word(&mut remaining)?;
        assert_eq!(expected_dump_bits, (-1.0_f32).to_bits());
        assert_eq!(
            greedy_argmax(&case.logits),
            usize::try_from(expected)?,
            "Greedy case {index}."
        );
        let mut logits_copy = case.logits;
        let mut dump_u = 123.0_f32;
        let sampled = sample_top_k_p(
            &mut logits_copy,
            SampleTopKPParams {
                temperature: 0.0,
                top_k: 0,
                top_p: 1.0,
                repetition_penalty: 1.0,
                seed: 0,
                philox_subseq: 0,
            },
            &[],
            &mut [],
            Some(&mut dump_u),
        );
        assert_eq!(
            sampled,
            Some(usize::try_from(expected)?),
            "Sample greedy case {index}."
        );
        assert_eq!(dump_u.to_bits(), expected_dump_bits);
    }
    assert_eq!(remaining, [0_u8; 0]);
    // Empty storage is the explicit Rust boundary, omitted by the native capture.
    assert_eq!(greedy_argmax(&[]), 0);
    let mut empty_dump = 99.0_f32;
    assert_eq!(
        sample_top_k_p(
            &mut [],
            SampleTopKPParams {
                temperature: 0.0,
                top_k: 0,
                top_p: 1.0,
                repetition_penalty: 1.0,
                seed: 0,
                philox_subseq: 0,
            },
            &[],
            &mut [],
            Some(&mut empty_dump),
        ),
        Some(0)
    );
    assert_eq!(empty_dump.to_bits(), (-1.0_f32).to_bits());
    Ok(())
}

#[test]
fn empty_and_no_op_sampler_calls_need_no_workspace_and_rejection_is_atomic() {
    suppress(&mut [], i32::MIN, i32::MAX, -1);
    let mut original = [-20.0, 15.0, -20.0, -20.0];
    assert!(repetition_penalty(&mut original, &[], 2.0, &mut []));
    assert!(repetition_penalty(&mut original, &[0, 2], 1.0, &mut []));
    assert!(repetition_penalty(&mut [], &[0, 2], 2.0, &mut []));
    assert!(!repetition_penalty(&mut original, &[0, 2], 2.0, &mut []));
    assert_eq!(
        original.map(f32::to_bits),
        [-20.0_f32, 15.0, -20.0, -20.0].map(f32::to_bits)
    );
    let mut logits = [-20.0_f32, 15.0, -20.0, -20.0];
    let history = [0_i32, 2];
    let mut seen = [0_u8; 4];
    let mut dump_u = -1.0_f32;
    let choice = sample_top_k_p(
        &mut logits,
        SampleTopKPParams {
            temperature: 0.6,
            top_k: 2,
            top_p: 0.9,
            repetition_penalty: 1.05,
            seed: 42,
            philox_subseq: 1,
        },
        &history,
        &mut seen,
        Some(&mut dump_u),
    );
    assert_eq!(choice, Some(1));
    assert!(dump_u > 0.0 && dump_u <= 1.0);
}
