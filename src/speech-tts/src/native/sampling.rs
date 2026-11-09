//! Allocation-free native adapters for the safe Qwen sampler policy.
#![expect(
    clippy::float_cmp,
    reason = "Exact comparison with one is the pinned sampler's required no-op branch."
)]

use std::ffi::c_int;

use super::super::sampling::SampleTopKPParams;

// SAFETY: The pinned greedy sampler supplies vocabulary initialized f32s,
// borrowed read-only for this synchronous call. No reference or pointer escapes.
#[unsafe(no_mangle)]
pub(super) extern "C" fn bonsai_qt_greedy_argmax(logits: *const f32, vocabulary: c_int) -> c_int {
    let vocabulary = usize::try_from(vocabulary).unwrap_or_else(|_| std::process::abort());
    if vocabulary == 0 {
        return 0;
    }
    if logits.is_null() || !logits.is_aligned() {
        std::process::abort();
    }
    // SAFETY: The native caller retains the aligned initialized vocabulary
    // extent throughout this call, with no concurrent mutation or deallocation.
    let logits = unsafe { std::slice::from_raw_parts(logits, vocabulary) };
    c_int::try_from(super::super::sampling::greedy_argmax(logits))
        .unwrap_or_else(|_| std::process::abort())
}

// SAFETY: This private static-link ABI is called only by the pinned sampler
// wrapper. For positive vocabulary, logits points to that many exclusively
// borrowed initialized f32s. The wrapper retains storage for this synchronous
// call; scalar interval arguments impose no further lifetime obligations.
#[unsafe(no_mangle)]
pub(super) extern "C" fn bonsai_qt_apply_suppress(
    logits: *mut f32,
    vocabulary: c_int,
    low: c_int,
    high: c_int,
    keep: c_int,
) {
    let vocabulary = usize::try_from(vocabulary).unwrap_or_else(|_| std::process::abort());
    if vocabulary == 0 {
        return;
    }
    if logits.is_null() || !logits.is_aligned() {
        std::process::abort();
    }
    // SAFETY: The reviewed native wrapper supplies the live, aligned and
    // exclusively borrowed vocabulary extent described above. Windows ARM64
    // c_int-sized f32 extents fit within isize::MAX.
    let logits = unsafe { std::slice::from_raw_parts_mut(logits, vocabulary) };
    super::super::sampling::suppress(logits, low, high, keep);
}

// SAFETY: The pinned wrapper supplies disjoint initialized storage: vocabulary
// writable f32 logits, history_count readable i32 history, and vocabulary
// writable workspace bytes. All borrows end before this synchronous call
// returns. Empty/no-op calls may supply null pointers and never dereference them.
#[unsafe(no_mangle)]
pub(super) extern "C" fn bonsai_qt_apply_repetition_penalty(
    logits: *mut f32,
    vocabulary: c_int,
    history: *const i32,
    history_count: c_int,
    penalty: f32,
    seen: *mut u8,
) {
    if penalty == 1.0 || history_count <= 0 {
        return;
    }
    let vocabulary = usize::try_from(vocabulary).unwrap_or_else(|_| std::process::abort());
    if vocabulary == 0 {
        return;
    }
    let history_count = usize::try_from(history_count).unwrap_or_else(|_| std::process::abort());
    if logits.is_null()
        || !logits.is_aligned()
        || history.is_null()
        || !history.is_aligned()
        || seen.is_null()
    {
        std::process::abort();
    }
    // SAFETY: The wrapper retains exclusive logits storage, disjoint from
    // history and workspace, for exactly the declared vocabulary extent.
    let logits = unsafe { std::slice::from_raw_parts_mut(logits, vocabulary) };
    // SAFETY: The wrapper's live i32 history covers history_count entries and
    // is disjoint from both writable regions for this synchronous call.
    let history = unsafe { std::slice::from_raw_parts(history, history_count) };
    // SAFETY: The native thread-local vector supplies vocabulary initialized
    // bytes, exclusively borrowed and disjoint from logits and history.
    let seen = unsafe { std::slice::from_raw_parts_mut(seen, vocabulary) };
    if !super::super::sampling::repetition_penalty(logits, history, penalty, seen) {
        std::process::abort();
    }
}

// SAFETY: The pinned wrapper supplies disjoint initialized storage: vocabulary
// writable f32 logits, optional dump_u_out writable f32, and (when temperature > 0,
// penalty != 1.0, and history_count > 0) history_count readable i32 history and
// vocabulary writable workspace bytes. All borrows end before this synchronous
// call returns.
#[unsafe(no_mangle)]
pub(super) extern "C" fn bonsai_qt_sample_top_k_p(
    logits: *mut f32,
    vocabulary: c_int,
    temperature: f32,
    top_k: c_int,
    top_p: f32,
    penalty: f32,
    history: *const i32,
    history_count: c_int,
    seed: i64,
    philox_subseq: i64,
    dump_u_out: *mut f32,
    seen_ptr: *mut u8,
) -> c_int {
    let vocabulary = usize::try_from(vocabulary).unwrap_or_else(|_| std::process::abort());
    let dump_ref = if dump_u_out.is_null() {
        None
    } else if !dump_u_out.is_aligned() {
        std::process::abort();
    } else {
        // SAFETY: Non-null aligned dump_u_out is exclusively borrowed for this call.
        Some(unsafe { &mut *dump_u_out })
    };
    if vocabulary == 0 {
        if temperature <= 0.0
            && let Some(dump) = dump_ref
        {
            *dump = -1.0;
        }
        return 0;
    }
    if logits.is_null() || !logits.is_aligned() {
        std::process::abort();
    }
    // SAFETY: The caller provides vocabulary initialized f32 logits exclusively for this call.
    let logits = unsafe { std::slice::from_raw_parts_mut(logits, vocabulary) };
    let mut empty_seen = [];
    let (history_slice, seen_slice): (&[i32], &mut [u8]) =
        if temperature <= 0.0 || penalty == 1.0 || history_count <= 0 {
            (&[], &mut empty_seen)
        } else {
            let count = usize::try_from(history_count).unwrap_or_else(|_| std::process::abort());
            if history.is_null() || !history.is_aligned() || seen_ptr.is_null() {
                std::process::abort();
            }
            // SAFETY: Non-empty active repetition penalty borrows disjoint history and seen buffers.
            let hist = unsafe { std::slice::from_raw_parts(history, count) };
            // SAFETY: The native caller resizes the thread-local seen buffer to vocabulary bytes.
            let seen_buf = unsafe { std::slice::from_raw_parts_mut(seen_ptr, vocabulary) };
            (hist, seen_buf)
        };
    let Some(selected) = super::super::sampling::sample_top_k_p(
        logits,
        SampleTopKPParams {
            temperature,
            top_k,
            top_p,
            repetition_penalty: penalty,
            seed,
            philox_subseq,
        },
        history_slice,
        seen_slice,
        dump_ref,
    ) else {
        std::process::abort();
    };
    c_int::try_from(selected).unwrap_or_else(|_| std::process::abort())
}
