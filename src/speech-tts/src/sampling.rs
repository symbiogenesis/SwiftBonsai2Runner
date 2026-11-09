//! Exact masking, repetition, greedy and stochastic selection from the pinned Qwen sampler.
//!
//! Ported from `src/sampling.h` in qwentts.cpp revision
//! `5916bf7c79dbb4e66fc059cdc507a79a23a28bb4`, under the MIT license reproduced
//! in `LICENSE.qwen-core` (Copyright 2023-2026 The omnivoice.cpp authors).
//! Immutable independently captured observations retain the native control;
//! `build_overlay.rs` guards the downloaded source by normalized length and
//! SHA-256 and routes the complete sampler pipeline to Rust.
//! Original header SHA-256:
//! `9812a8b428efffcfe3d832409f3eae092cb3be90bfeefbf141f75df0320e2241`.
#![forbid(unsafe_code)]

/// Trailing history window for repetition penalty (~5.76 s at 12.5 Hz).
///
/// Bounding repetition penalty to the recent acoustic context prevents
/// progressive speaking-rate acceleration and clipped word endings over
/// multi-sentence read-aloud while preserving bit-exact replay on all 722
/// captured `sampling-v1.bin` controls (whose maximum history length is 66).
#[expect(
    clippy::redundant_pub_crate,
    reason = "Sampler policy is private to the speech crate and its native ABI module."
)]
pub(super) const REPETITION_PENALTY_WINDOW: usize = 72;

const ISORT_MAX: usize = 32;

/// Scalar parameters for one stochastic or greedy `sample_top_k_p` draw.
#[derive(Clone, Copy, Debug)]
#[expect(
    clippy::redundant_pub_crate,
    reason = "Sampler policy is private to the speech crate and its native ABI module."
)]
pub(super) struct SampleTopKPParams {
    pub(super) temperature: f32,
    pub(super) top_k: i32,
    pub(super) top_p: f32,
    pub(super) repetition_penalty: f32,
    pub(super) seed: i64,
    pub(super) philox_subseq: i64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct TokenProb {
    id: usize,
    prob: f32,
}

/// Select the captured Windows ARM64 MSVC 14.51 greedy maximum.
///
/// Ordinary maxima retain the first equal token. The native optimized float
/// algorithm propagates any NaN across its complete four-lane vector prefix,
/// discards that prefix and scans the remaining scalar tail from negative
/// infinity. Its short-input scalar path also starts from negative infinity
/// and ignores NaNs. Empty input returns zero without accessing storage.
///
/// This independently expressed scalar policy preserves the captured target
/// behavior, including its exceptional-value results. The mechanism and exact
/// local STL source identities are recorded in `greedy-v1.provenance.json`;
/// broader compiler/target and owning-crate qualification remain separate gates.
#[expect(
    clippy::redundant_pub_crate,
    reason = "Sampler policy is private to the speech crate and its native ABI module."
)]
pub(super) fn greedy_argmax(logits: &[f32]) -> usize {
    // The private C ABI limits vocabulary to positive c_int, so the native
    // 2^32-vector index-wrap portion boundary cannot be reached.
    let vector_end = logits.len() / 4 * 4;
    let mut best = 0;
    let mut maximum = f32::NEG_INFINITY;
    let mut prefix_nan = false;
    for (index, &candidate) in logits[..vector_end].iter().enumerate() {
        prefix_nan |= candidate.is_nan();
        if maximum < candidate {
            maximum = candidate;
            best = index;
        }
    }
    if prefix_nan {
        best = 0;
        maximum = f32::NEG_INFINITY;
    }
    for (offset, &candidate) in logits[vector_end..].iter().enumerate() {
        if maximum < candidate {
            maximum = candidate;
            best = vector_end + offset;
        }
    }
    best
}

/// Mask the clipped half-open interval, preserving the optional kept token.
#[expect(
    clippy::redundant_pub_crate,
    reason = "Sampler policy is private to the speech crate and its native ABI module."
)]
pub(super) fn suppress(logits: &mut [f32], low: i32, high: i32, keep: i32) {
    let low = usize::try_from(low.max(0)).unwrap_or(usize::MAX);
    let high = usize::try_from(high).unwrap_or(0).min(logits.len());
    let keep = usize::try_from(keep).ok();
    for (index, logit) in logits.iter_mut().enumerate().take(high).skip(low) {
        if Some(index) != keep {
            *logit = f32::NEG_INFINITY;
        }
    }
}

/// Apply the pinned sign-sensitive penalty once per valid token in the trailing
/// [`REPETITION_PENALTY_WINDOW`] history slice.
///
/// The caller supplies one workspace byte per logit. A smaller workspace is
/// rejected before mutation; the audited native wrapper always supplies the
/// complete vocabulary extent.
#[expect(
    clippy::redundant_pub_crate,
    clippy::float_cmp,
    reason = "Sampler policy is private to the speech crate and its native ABI module."
)]
pub(super) fn repetition_penalty(
    logits: &mut [f32],
    history: &[i32],
    penalty: f32,
    seen: &mut [u8],
) -> bool {
    if penalty == 1.0 || history.is_empty() {
        return true;
    }
    let Some(seen) = seen.get_mut(..logits.len()) else {
        return false;
    };
    seen.fill(0);
    let window_start = history.len().saturating_sub(REPETITION_PENALTY_WINDOW);
    for &token in &history[window_start..] {
        let Ok(token) = usize::try_from(token) else {
            continue;
        };
        let Some(logit) = logits.get_mut(token) else {
            continue;
        };
        if seen[token] == 0 {
            seen[token] = 1;
            // The explicit comparison preserves -0 and NaN behavior of the
            // pinned C++ ternary; signbit and abs-based rewrites differ.
            *logit = if *logit < 0.0 {
                *logit * penalty
            } else {
                *logit / penalty
            };
        }
    }
    true
}

fn msvc_insertion_sort<T: Copy, F: Fn(T, T) -> bool>(slice: &mut [T], pred: F) {
    for mid in 1..slice.len() {
        let val = slice[mid];
        if pred(val, slice[0]) {
            slice.copy_within(0..mid, 1);
            slice[0] = val;
        } else {
            let mut hole = mid;
            while hole > 0 && pred(val, slice[hole - 1]) {
                slice[hole] = slice[hole - 1];
                hole -= 1;
            }
            slice[hole] = val;
        }
    }
}

fn msvc_med3<T: Copy, F: Fn(T, T) -> bool>(
    slice: &mut [T],
    first: usize,
    mid: usize,
    last: usize,
    pred: &F,
) {
    if pred(slice[mid], slice[first]) {
        slice.swap(mid, first);
    }
    if pred(slice[last], slice[mid]) {
        slice.swap(last, mid);
        if pred(slice[mid], slice[first]) {
            slice.swap(mid, first);
        }
    }
}

fn msvc_guess_median<T: Copy, F: Fn(T, T) -> bool>(
    slice: &mut [T],
    first: usize,
    mid: usize,
    last: usize,
    pred: &F,
) {
    let count = last - first;
    if count > 40 {
        let step = (count + 1) >> 3;
        let two_step = step << 1;
        msvc_med3(slice, first, first + step, first + two_step, pred);
        msvc_med3(slice, mid - step, mid, mid + step, pred);
        msvc_med3(slice, last - two_step, last - step, last, pred);
        msvc_med3(slice, first + step, mid, last - step, pred);
    } else {
        msvc_med3(slice, first, mid, last, pred);
    }
}

fn msvc_partition_by_median_guess<T: Copy, F: Fn(T, T) -> bool>(
    slice: &mut [T],
    pred: &F,
) -> (usize, usize) {
    let len = slice.len();
    let mid = len >> 1;
    msvc_guess_median(slice, 0, mid, len - 1, pred);
    let mut pfirst = mid;
    let mut plast = pfirst + 1;
    while pfirst > 0
        && !pred(slice[pfirst - 1], slice[pfirst])
        && !pred(slice[pfirst], slice[pfirst - 1])
    {
        pfirst -= 1;
    }
    while plast < len && !pred(slice[plast], slice[pfirst]) && !pred(slice[pfirst], slice[plast]) {
        plast += 1;
    }
    let mut gfirst = plast;
    let mut glast = pfirst;
    loop {
        while gfirst < len {
            if pred(slice[pfirst], slice[gfirst]) {
                // Continue scanning right.
            } else if pred(slice[gfirst], slice[pfirst]) {
                break;
            } else if plast != gfirst {
                slice.swap(plast, gfirst);
                plast += 1;
            } else {
                plast += 1;
            }
            gfirst += 1;
        }
        while glast > 0 {
            let previous = glast - 1;
            if pred(slice[previous], slice[pfirst]) {
                // Continue scanning left.
            } else if pred(slice[pfirst], slice[previous]) {
                break;
            } else {
                pfirst -= 1;
                if pfirst != previous {
                    slice.swap(pfirst, previous);
                }
            }
            glast -= 1;
        }
        if glast == 0 && gfirst == len {
            return (pfirst, plast);
        }
        if glast == 0 {
            if plast != gfirst {
                slice.swap(pfirst, plast);
            }
            plast += 1;
            slice.swap(pfirst, gfirst);
            pfirst += 1;
            gfirst += 1;
        } else if gfirst == len {
            glast -= 1;
            pfirst -= 1;
            if glast != pfirst {
                slice.swap(glast, pfirst);
            }
            plast -= 1;
            slice.swap(pfirst, plast);
        } else {
            glast -= 1;
            slice.swap(gfirst, glast);
            gfirst += 1;
        }
    }
}

fn msvc_sift_down<T: Copy, F: Fn(T, T) -> bool>(
    slice: &mut [T],
    mut hole: usize,
    bottom: usize,
    pred: &F,
) {
    if bottom < 2 {
        return;
    }
    let top = hole;
    let val = slice[hole];
    let max_non_leaf = (bottom - 1) >> 1;
    while hole < max_non_leaf {
        let mut child = 2 * hole + 2;
        if pred(slice[child], slice[child - 1]) {
            child -= 1;
        }
        slice[hole] = slice[child];
        hole = child;
    }
    if hole == max_non_leaf && (bottom & 1) == 0 {
        slice[hole] = slice[bottom - 1];
        hole = bottom - 1;
    }
    while hole > top {
        let parent = (hole - 1) >> 1;
        if pred(slice[parent], val) {
            slice[hole] = slice[parent];
            hole = parent;
        } else {
            break;
        }
    }
    slice[hole] = val;
}

fn msvc_heap_sort<T: Copy, F: Fn(T, T) -> bool>(slice: &mut [T], pred: &F) {
    let len = slice.len();
    for hole in (0..(len >> 1)).rev() {
        msvc_sift_down(slice, hole, len, pred);
    }
    for end in (2..=len).rev() {
        slice.swap(0, end - 1);
        msvc_sift_down(slice, 0, end - 1, pred);
    }
}

fn msvc_sort_impl<T: Copy, F: Fn(T, T) -> bool>(mut slice: &mut [T], mut ideal: usize, pred: &F) {
    while slice.len() > ISORT_MAX {
        if ideal == 0 {
            msvc_heap_sort(slice, pred);
            return;
        }
        let (pfirst, plast) = msvc_partition_by_median_guess(slice, pred);
        ideal = (ideal >> 1) + (ideal >> 2);
        let right_len = slice.len() - plast;
        if pfirst < right_len {
            msvc_sort_impl(&mut slice[..pfirst], ideal, pred);
            slice = &mut slice[plast..];
        } else {
            msvc_sort_impl(&mut slice[plast..], ideal, pred);
            slice = &mut slice[..pfirst];
        }
    }
    msvc_insertion_sort(slice, pred);
}

fn msvc_sort<T: Copy, F: Fn(T, T) -> bool>(slice: &mut [T], pred: F) {
    let len = slice.len();
    msvc_sort_impl(slice, len, &pred);
}

fn msvc_nth_element<T: Copy, F: Fn(T, T) -> bool>(mut slice: &mut [T], mut nth: usize, pred: F) {
    if nth >= slice.len() {
        return;
    }
    while slice.len() > ISORT_MAX {
        let (pfirst, plast) = msvc_partition_by_median_guess(slice, &pred);
        if plast <= nth {
            nth -= plast;
            slice = &mut slice[plast..];
        } else if pfirst <= nth {
            return;
        } else {
            slice = &mut slice[..pfirst];
        }
    }
    msvc_insertion_sort(slice, &pred);
}

/// Apply the pinned top-k cutoff in place (`0 < top_k < logits.len()`).
#[expect(
    clippy::redundant_pub_crate,
    reason = "Sampler policy is private to the speech crate and its native ABI module."
)]
pub(super) fn top_k_mask(logits: &mut [f32], top_k: i32) {
    if let Ok(k) = usize::try_from(top_k)
        && k > 0
        && k < logits.len()
    {
        let mut tmp = logits.to_vec();
        msvc_nth_element(&mut tmp, k - 1, |a, b| a > b);
        let threshold = tmp[k - 1];
        for logit in logits.iter_mut() {
            if *logit < threshold {
                *logit = f32::NEG_INFINITY;
            }
        }
    }
}

/// Apply the pinned top-p nucleus mask in place (`0.0 < top_p < 1.0`).
#[expect(
    clippy::redundant_pub_crate,
    reason = "Sampler policy is private to the speech crate and its native ABI module."
)]
pub(super) fn top_p_mask(logits: &mut [f32], top_p: f32) {
    if !(top_p > 0.0 && top_p < 1.0) || logits.is_empty() {
        return;
    }
    let mut max_logit = f32::NEG_INFINITY;
    for &logit in logits.iter() {
        if logit > max_logit {
            max_logit = logit;
        }
    }
    let mut sum_exp = 0.0_f32;
    for &logit in logits.iter() {
        sum_exp += (logit - max_logit).exp();
    }
    let inv_sum = 1.0_f32 / sum_exp;
    let cutoff = max_logit - 16.0_f32;
    let mut sorted_buf = Vec::new();
    for (id, logit) in logits.iter_mut().enumerate() {
        if *logit >= cutoff {
            let prob = (*logit - max_logit).exp() * inv_sum;
            sorted_buf.push(TokenProb { id, prob });
        } else {
            *logit = f32::NEG_INFINITY;
        }
    }
    if !sorted_buf.is_empty() {
        msvc_sort(&mut sorted_buf, |a, b| a.prob > b.prob);
        let mut cum = 0.0_f32;
        for (index, candidate) in sorted_buf.iter().enumerate() {
            if index > 0
                && cum >= top_p
                && let Some(slot) = logits.get_mut(candidate.id)
            {
                *slot = f32::NEG_INFINITY;
            }
            cum += candidate.prob;
        }
    }
}

/// Execute the full pinned `sample_top_k_p` chain in safe Rust.
///
/// Mirrors `src/sampling.h`:
/// 1. Greedy branch (`temperature <= 0.0`): sets `*dump_u_out = -1.0` and returns [`greedy_argmax`].
/// 2. Windowed [`repetition_penalty`] over `history`.
/// 3. Temperature scaling via `1.0 / temperature`.
/// 4. [`top_k_mask`] when `0 < top_k < V`.
/// 5. [`top_p_mask`] when `0.0 < top_p < 1.0`.
/// 6. Sequential `expf` softmax and Philox4x32-10 uniform multinomial draw.
#[expect(
    clippy::redundant_pub_crate,
    reason = "Sampler policy is private to the speech crate and its native ABI module."
)]
pub(super) fn sample_top_k_p(
    logits: &mut [f32],
    params: SampleTopKPParams,
    history: &[i32],
    seen: &mut [u8],
    dump_u_out: Option<&mut f32>,
) -> Option<usize> {
    if params.temperature <= 0.0 {
        if let Some(dump) = dump_u_out {
            *dump = -1.0;
        }
        return Some(greedy_argmax(logits));
    }
    if logits.is_empty() {
        return Some(0);
    }
    if !repetition_penalty(logits, history, params.repetition_penalty, seen) {
        return None;
    }
    let inv_temp = 1.0_f32 / params.temperature;
    for logit in logits.iter_mut() {
        *logit *= inv_temp;
    }
    top_k_mask(logits, params.top_k);
    top_p_mask(logits, params.top_p);

    let mut max_val = f32::NEG_INFINITY;
    for &logit in logits.iter() {
        if logit > max_val {
            max_val = logit;
        }
    }
    let mut sum = 0.0_f32;
    for logit in logits.iter_mut() {
        *logit = (*logit - max_val).exp();
        sum += *logit;
    }
    let u = super::philox::uniform(params.seed, params.philox_subseq, 0);
    if let Some(dump) = dump_u_out {
        *dump = u;
    }
    let r = u * sum;
    let mut acc = 0.0_f32;
    for (index, &prob) in logits.iter().enumerate() {
        acc += prob;
        if acc >= r {
            return Some(index);
        }
    }
    Some(logits.len() - 1)
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::float_cmp,
        reason = "Exact zero, infinity and integral results describe the pinned sampler contract."
    )]
    use super::{
        REPETITION_PENALTY_WINDOW, SampleTopKPParams, TokenProb, greedy_argmax, msvc_sort,
        repetition_penalty, sample_top_k_p, suppress, top_k_mask, top_p_mask,
    };

    #[test]
    fn greedy_selection_retains_captured_arm64_ties_and_nan_boundaries() {
        let nan = f32::from_bits(0x7fc0_1234);
        for (logits, expected) in [
            (vec![], 0),
            (vec![nan], 0),
            (vec![nan, f32::INFINITY], 1),
            (vec![nan, 2.0, 1.0], 1),
            (vec![-0.0, 0.0], 0),
            (vec![0.0, -0.0], 0),
            (vec![1.0, nan, 2.0, 2.0], 0),
            (vec![1.0, 2.0, 2.0, nan], 0),
            (vec![1.0, 2.0, 2.0, 1.0, nan], 1),
            (vec![1.0, nan, 2.0, 2.0, -5.0], 4),
            (vec![1.0, nan, 2.0, 2.0, -5.0, -6.0], 4),
            (vec![1.0, nan, 2.0, 2.0, -5.0, -6.0, -4.0], 6),
            (vec![1.0, nan, 2.0, 2.0, -5.0, -6.0, -4.0, 7.0], 0),
            (vec![1.0, nan, 2.0, 2.0, -5.0, -6.0, -4.0, 7.0, -8.0], 8),
            (vec![nan; 3], 0),
            (vec![nan; 4], 0),
            (vec![nan; 7], 0),
            (vec![nan; 8], 0),
            (vec![nan; 9], 0),
            (vec![f32::NEG_INFINITY; 3], 0),
            (vec![f32::NEG_INFINITY; 4], 0),
            (vec![f32::NEG_INFINITY; 7], 0),
            (
                vec![
                    f32::NEG_INFINITY,
                    nan,
                    f32::NEG_INFINITY,
                    f32::NEG_INFINITY,
                    f32::NEG_INFINITY,
                ],
                0,
            ),
            (vec![-0.0, 0.0, -1.0, -2.0, 0.0], 0),
            (
                vec![f32::NEG_INFINITY, f32::INFINITY, nan, f32::INFINITY],
                0,
            ),
            (
                vec![
                    f32::NEG_INFINITY,
                    f32::INFINITY,
                    0.0,
                    f32::INFINITY,
                    f32::INFINITY,
                ],
                1,
            ),
        ] {
            let original: Vec<_> = logits.iter().map(|value| value.to_bits()).collect();
            assert_eq!(greedy_argmax(&logits), expected);
            assert_eq!(
                logits
                    .iter()
                    .map(|value| value.to_bits())
                    .collect::<Vec<_>>(),
                original
            );
        }
    }

    #[test]
    fn suppression_clips_ranges_and_preserves_kept_bits() {
        let original = [0.0, -0.0, f32::from_bits(0x7fc0_1234), f32::INFINITY];
        let mut logits = original;
        suppress(&mut logits, i32::MIN, i32::MAX, 2);
        assert_eq!(logits[2].to_bits(), original[2].to_bits());
        assert_eq!(logits[0], f32::NEG_INFINITY);
        assert_eq!(logits[1], f32::NEG_INFINITY);
        assert_eq!(logits[3], f32::NEG_INFINITY);
        for (low, high) in [(3, 1), (0, -1), (5, 8), (0, 0)] {
            let mut logits = original;
            suppress(&mut logits, low, high, -1);
            assert_eq!(logits.map(f32::to_bits), original.map(f32::to_bits));
        }
        suppress(&mut [], i32::MIN, i32::MAX, -1);
    }

    #[test]
    fn repetition_ignores_invalid_and_duplicate_tokens_and_reuses_workspace() {
        let mut logits = [-4.0, 4.0, -0.0, f32::NEG_INFINITY];
        let mut seen = [255; 4];
        assert!(repetition_penalty(
            &mut logits,
            &[-1, 0, 1, 1, 0, 2, 3, 4, i32::MAX],
            2.0,
            &mut seen,
        ));
        assert_eq!(
            logits.map(f32::to_bits),
            [-8.0, 2.0, -0.0, f32::NEG_INFINITY].map(f32::to_bits)
        );
        assert!(repetition_penalty(&mut logits, &[0, 0, 1], 0.5, &mut seen));
        assert_eq!(logits[..2], [-4.0, 4.0]);
    }

    #[test]
    fn repetition_penalty_windows_trailing_history_beyond_window() {
        let mut logits = [4.0, 4.0, 4.0];
        let mut seen = [0_u8; 3];
        // Token 0 is outside the trailing window; token 1 is inside the trailing window.
        let mut history = vec![2_i32; REPETITION_PENALTY_WINDOW + 8];
        history[0] = 0;
        history[8] = 1;
        assert!(repetition_penalty(&mut logits, &history, 2.0, &mut seen));
        assert_eq!(logits, [4.0, 2.0, 2.0]);
    }

    #[test]
    fn no_op_and_rejected_workspace_preserve_logits() {
        let original = [f32::from_bits(0x7fc0_1234), -0.0];
        let mut logits = original;
        assert!(repetition_penalty(&mut logits, &[0, 1], 1.0, &mut []));
        assert!(repetition_penalty(&mut logits, &[], 2.0, &mut []));
        assert!(!repetition_penalty(&mut logits, &[0, 1], 2.0, &mut [0]));
        assert_eq!(logits.map(f32::to_bits), original.map(f32::to_bits));
    }

    #[test]
    fn top_k_and_top_p_preserve_ties_and_nucleus_boundaries() {
        let mut logits = [1.0_f32, 3.0, 2.0, 3.0, -5.0];
        top_k_mask(&mut logits, 2);
        assert_eq!(
            logits,
            [
                f32::NEG_INFINITY,
                3.0,
                f32::NEG_INFINITY,
                3.0,
                f32::NEG_INFINITY
            ]
        );

        let mut tied = [0.0_f32, 0.0, 0.0, 0.0];
        top_p_mask(&mut tied, 0.45);
        // Each token has probability 0.25; cumulative probs before index i are
        // [0.0, 0.25, 0.50, 0.75], so indices 0 and 1 survive in ascending id order.
        assert_eq!(tied, [0.0, 0.0, f32::NEG_INFINITY, f32::NEG_INFINITY]);

        // Also test >32 elements through the median-of-3/9 partition path.
        let mut many: Vec<TokenProb> = (0..64)
            .map(|id| TokenProb {
                id,
                prob: if id.is_multiple_of(2) { 0.5 } else { 0.25 },
            })
            .collect();
        msvc_sort(&mut many, |a, b| a.prob > b.prob);
        assert!(many[..32].iter().all(|item| item.prob == 0.5));
        assert!(many[32..].iter().all(|item| item.prob == 0.25));
    }

    #[test]
    fn stochastic_sampler_executes_greedy_and_philox_cdf_walk() {
        let mut logits = [0.5_f32, 2.5, 1.0, -1.0];
        let mut seen = [0_u8; 4];
        let mut dump = 42.0_f32;
        let greedy = sample_top_k_p(
            &mut logits,
            SampleTopKPParams {
                temperature: 0.0,
                top_k: 50,
                top_p: 1.0,
                repetition_penalty: 1.05,
                seed: 7,
                philox_subseq: 0,
            },
            &[1],
            &mut seen,
            Some(&mut dump),
        );
        assert_eq!(greedy, Some(1));
        assert_eq!(dump, -1.0);

        let mut dominant = [-50.0_f32, -50.0, 20.0, -50.0];
        let mut draw_u = -1.0_f32;
        let sampled = sample_top_k_p(
            &mut dominant,
            SampleTopKPParams {
                temperature: 0.85,
                top_k: 2,
                top_p: 0.9,
                repetition_penalty: 1.05,
                seed: 1234,
                philox_subseq: 3,
            },
            &[0, 1],
            &mut seen,
            Some(&mut draw_u),
        );
        assert_eq!(sampled, Some(2));
        assert!(draw_u > 0.0 && draw_u <= 1.0);
    }
}
