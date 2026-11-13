//! Pure-Rust Qwen3-TTS 12 Hz / 48 kHz causal audio codec and streaming `CodecStateSnap` cache.
//!
//! Implements both the 24 kHz (`TOKENIZER_HOP_24K = 1920`) and 48 kHz
//! (`takuma104/Qwen3-TTS-Tokenizer-12Hz-48kHz`, `TOKENIZER_HOP_48K = 3840`)
//! causal `ConvNeXt`/`SnakeBeta` decoder pipelines, the stateful `CodecStateSnap`
//! 8-slot LRU cache for zero-redecode ICL reference priming, and the `SEANet` +
//! Mimi transformer + RVQ reference encoder.
#![forbid(unsafe_code)]
#![expect(
    clippy::missing_errors_doc,
    reason = "Internal codec and convolution shape checks return io::ErrorKind::InvalidInput."
)]

use std::io;

use fearless_simd::{Level, dispatch, f32x4, prelude::*};

use crate::{SAMPLE_RATE, SAMPLE_RATE_48K, engine::row_workers};

fn frame_workers(threads: usize, frames: usize, work: usize) -> usize {
    if frames <= 1 || work < 65_536 {
        1
    } else {
        // Preserve the shared Talker/codec team size through 2/4/8-frame
        // stages; fewer jobs may use that team without recreating its workers.
        threads.clamp(1, 18)
    }
}

#[expect(
    clippy::inline_always,
    reason = "fearless_simd dispatch! requires inline(always) to inherit target_feature attributes."
)]
#[inline(always)]
fn add_slice_in_place_simd<S: Simd>(simd: S, dst: &mut [f32], src: &[f32]) {
    let (dst_chunks, dst_tail) = dst.as_chunks_mut::<4>();
    let (src_chunks, src_tail) = src.as_chunks::<4>();
    for (d, s) in dst_chunks.iter_mut().zip(src_chunks.iter()) {
        let dv = f32x4::from_slice(simd, d);
        let sv = f32x4::from_slice(simd, s);
        (dv + sv).store_slice(d);
    }
    for (d, &s) in dst_tail.iter_mut().zip(src_tail.iter()) {
        *d += s;
    }
}

#[expect(
    clippy::inline_always,
    reason = "fearless_simd dispatch! requires inline(always) to inherit target_feature attributes."
)]
#[inline(always)]
fn sub_slice_in_place_simd<S: Simd>(simd: S, dst: &mut [f32], src: &[f32]) {
    let (dst_chunks, dst_tail) = dst.as_chunks_mut::<4>();
    let (src_chunks, src_tail) = src.as_chunks::<4>();
    for (d, s) in dst_chunks.iter_mut().zip(src_chunks.iter()) {
        let dv = f32x4::from_slice(simd, d);
        let sv = f32x4::from_slice(simd, s);
        (dv - sv).store_slice(d);
    }
    for (d, &s) in dst_tail.iter_mut().zip(src_tail.iter()) {
        *d -= s;
    }
}

#[inline]
fn reduce_f32x4<S: Simd>(v: f32x4<S>) -> f32 {
    let mut a = [0.0_f32; 4];
    v.store_slice(&mut a);
    (a[0] + a[2]) + (a[1] + a[3])
}

#[expect(
    clippy::inline_always,
    reason = "fearless_simd dispatch! requires inline(always) to inherit target_feature attributes."
)]
#[inline(always)]
fn dot_f32_simd<S: Simd>(simd: S, a: &[f32], b: &[f32], init: f32) -> f32 {
    let zero = f32x4::splat(simd, 0.0);
    let mut acc0 = zero;
    let mut acc1 = zero;
    let (a8, a_rem8) = a.as_chunks::<8>();
    let (b8, b_rem8) = b.as_chunks::<8>();
    for (ac, bc) in a8.iter().zip(b8.iter()) {
        let av0 = f32x4::from_slice(simd, &ac[..4]);
        let bv0 = f32x4::from_slice(simd, &bc[..4]);
        let av1 = f32x4::from_slice(simd, &ac[4..]);
        let bv1 = f32x4::from_slice(simd, &bc[4..]);
        acc0 = av0.mul_add(bv0, acc0);
        acc1 = av1.mul_add(bv1, acc1);
    }
    let (a4, a_tail) = a_rem8.as_chunks::<4>();
    let (b4, b_tail) = b_rem8.as_chunks::<4>();
    for (ac, bc) in a4.iter().zip(b4.iter()) {
        let av = f32x4::from_slice(simd, ac);
        let bv = f32x4::from_slice(simd, bc);
        acc0 = av.mul_add(bv, acc0);
    }
    let mut sum = init + reduce_f32x4(acc0 + acc1);
    for (&av, &bv) in a_tail.iter().zip(b_tail.iter()) {
        sum = av.mul_add(bv, sum);
    }
    sum
}

#[expect(
    clippy::inline_always,
    reason = "fearless_simd dispatch! requires inline(always) to inherit target_feature attributes."
)]
#[inline(always)]
fn dot_four_rows_f32_simd<S: Simd>(
    simd: S,
    w0: &[f32],
    w1: &[f32],
    w2: &[f32],
    w3: &[f32],
    x: &[f32],
    inits: [f32; 4],
) -> [f32; 4] {
    let zero = f32x4::splat(simd, 0.0);
    let mut a0_0 = zero;
    let mut a0_1 = zero;
    let mut a1_0 = zero;
    let mut a1_1 = zero;
    let mut a2_0 = zero;
    let mut a2_1 = zero;
    let mut a3_0 = zero;
    let mut a3_1 = zero;

    let (x8, x_rem8) = x.as_chunks::<8>();
    let (w0_8, w0_rem8) = w0.as_chunks::<8>();
    let (w1_8, w1_rem8) = w1.as_chunks::<8>();
    let (w2_8, w2_rem8) = w2.as_chunks::<8>();
    let (w3_8, w3_rem8) = w3.as_chunks::<8>();

    for (idx, xc) in x8.iter().enumerate() {
        let xv0 = f32x4::from_slice(simd, &xc[..4]);
        let xv1 = f32x4::from_slice(simd, &xc[4..]);

        let r0 = &w0_8[idx];
        a0_0 = f32x4::from_slice(simd, &r0[..4]).mul_add(xv0, a0_0);
        a0_1 = f32x4::from_slice(simd, &r0[4..]).mul_add(xv1, a0_1);

        let r1 = &w1_8[idx];
        a1_0 = f32x4::from_slice(simd, &r1[..4]).mul_add(xv0, a1_0);
        a1_1 = f32x4::from_slice(simd, &r1[4..]).mul_add(xv1, a1_1);

        let r2 = &w2_8[idx];
        a2_0 = f32x4::from_slice(simd, &r2[..4]).mul_add(xv0, a2_0);
        a2_1 = f32x4::from_slice(simd, &r2[4..]).mul_add(xv1, a2_1);

        let r3 = &w3_8[idx];
        a3_0 = f32x4::from_slice(simd, &r3[..4]).mul_add(xv0, a3_0);
        a3_1 = f32x4::from_slice(simd, &r3[4..]).mul_add(xv1, a3_1);
    }

    let (x4, x_tail) = x_rem8.as_chunks::<4>();
    let (w0_4, w0_tail) = w0_rem8.as_chunks::<4>();
    let (w1_4, w1_tail) = w1_rem8.as_chunks::<4>();
    let (w2_4, w2_tail) = w2_rem8.as_chunks::<4>();
    let (w3_4, w3_tail) = w3_rem8.as_chunks::<4>();

    for (idx, xc) in x4.iter().enumerate() {
        let xv = f32x4::from_slice(simd, xc);
        a0_0 = f32x4::from_slice(simd, &w0_4[idx]).mul_add(xv, a0_0);
        a1_0 = f32x4::from_slice(simd, &w1_4[idx]).mul_add(xv, a1_0);
        a2_0 = f32x4::from_slice(simd, &w2_4[idx]).mul_add(xv, a2_0);
        a3_0 = f32x4::from_slice(simd, &w3_4[idx]).mul_add(xv, a3_0);
    }

    let mut s0 = inits[0] + reduce_f32x4(a0_0 + a0_1);
    let mut s1 = inits[1] + reduce_f32x4(a1_0 + a1_1);
    let mut s2 = inits[2] + reduce_f32x4(a2_0 + a2_1);
    let mut s3 = inits[3] + reduce_f32x4(a3_0 + a3_1);

    for (idx, &xv) in x_tail.iter().enumerate() {
        s0 = w0_tail[idx].mul_add(xv, s0);
        s1 = w1_tail[idx].mul_add(xv, s1);
        s2 = w2_tail[idx].mul_add(xv, s2);
        s3 = w3_tail[idx].mul_add(xv, s3);
    }
    [s0, s1, s2, s3]
}

#[expect(
    clippy::inline_always,
    reason = "fearless_simd dispatch! requires inline(always) to inherit target_feature attributes."
)]
#[inline(always)]
fn matvec_4rows_simd<S: Simd>(
    simd: S,
    weight: &[f32],
    input: &[f32],
    bias: Option<&[f32]>,
    out: &mut [f32],
) {
    let in_c = input.len();
    let (out_quads, out_rem) = out.as_chunks_mut::<4>();
    for (q_idx, quad) in out_quads.iter_mut().enumerate() {
        let o_base = q_idx * 4;
        let w_base = o_base * in_c;
        let inits = bias.map_or([0.0_f32; 4], |b| {
            [b[o_base], b[o_base + 1], b[o_base + 2], b[o_base + 3]]
        });
        *quad = dot_four_rows_f32_simd(
            simd,
            &weight[w_base..w_base + in_c],
            &weight[w_base + in_c..w_base + 2 * in_c],
            &weight[w_base + 2 * in_c..w_base + 3 * in_c],
            &weight[w_base + 3 * in_c..w_base + 4 * in_c],
            input,
            inits,
        );
    }
    let rem_base = out_quads.len() * 4;
    for (r_idx, slot) in out_rem.iter_mut().enumerate() {
        let o = rem_base + r_idx;
        let init = bias.map_or(0.0_f32, |b| b[o]);
        *slot = dot_f32_simd(simd, &weight[o * in_c..(o + 1) * in_c], input, init);
    }
}

#[expect(
    clippy::inline_always,
    reason = "fearless_simd dispatch! requires inline(always) to inherit target_feature attributes."
)]
#[inline(always)]
#[expect(
    clippy::too_many_arguments,
    reason = "Clip the original scatter kernel to one disjoint output-time window without changing its weight layout."
)]
fn trans_conv1d_scatter_frame_simd<S: Simd>(
    simd: S,
    in_row: &[f32],
    weight: &[f32],
    out_c: usize,
    kernel: usize,
    kernel_range: std::ops::Range<usize>,
    raw_window: &mut [f32],
) {
    let oc_quads = out_c / 4;
    for (ic, &x) in in_row.iter().enumerate() {
        let xv = f32x4::splat(simd, x);
        let ic_w_base = ic * out_c * kernel;
        for q_idx in 0..oc_quads {
            let oc_base = q_idx * 4;
            let w0 = &weight[ic_w_base + oc_base * kernel..ic_w_base + (oc_base + 1) * kernel];
            let w1 =
                &weight[ic_w_base + (oc_base + 1) * kernel..ic_w_base + (oc_base + 2) * kernel];
            let w2 =
                &weight[ic_w_base + (oc_base + 2) * kernel..ic_w_base + (oc_base + 3) * kernel];
            let w3 =
                &weight[ic_w_base + (oc_base + 3) * kernel..ic_w_base + (oc_base + 4) * kernel];
            for k in kernel_range.clone() {
                let dst_base = (k - kernel_range.start) * out_c + oc_base;
                let dst = &mut raw_window[dst_base..dst_base + 4];
                let wv = f32x4::from_slice(simd, &[w0[k], w1[k], w2[k], w3[k]]);
                let rv = f32x4::from_slice(simd, dst);
                xv.mul_add(wv, rv).store_slice(dst);
            }
        }
        for oc in (oc_quads * 4)..out_c {
            let w_base = ic_w_base + oc * kernel;
            for k in kernel_range.clone() {
                let dst_idx = (k - kernel_range.start) * out_c + oc;
                raw_window[dst_idx] = x.mul_add(weight[w_base + k], raw_window[dst_idx]);
            }
        }
    }
}

#[expect(
    clippy::inline_always,
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "fearless_simd dispatch! requires inline(always) to inherit target_feature attributes."
)]
#[inline(always)]
fn convnext_frame_simd<S: Simd>(
    simd: S,
    ext: &[f32],
    frame: usize,
    channels: usize,
    kernel: usize,
    inv_c: f32,
    dw_weight: &[f32],
    dw_bias: &[f32],
    norm_w: &[f32],
    norm_b: &[f32],
    pwconv1_w: &[f32],
    pwconv1_b: &[f32],
    pwconv2_w: &[f32],
    pwconv2_b: &[f32],
    gamma: &[f32],
    norm_buf: &mut [f32],
    mid: &mut [f32],
    out_row: &mut [f32],
) {
    // 1. Depthwise causal conv across `channels` channels (4 channels per f32x4 lane)
    let (y_quads, y_rem) = norm_buf.as_chunks_mut::<4>();
    let (b_quads, b_rem) = dw_bias.as_chunks::<4>();
    for (q_idx, (yq, bq)) in y_quads.iter_mut().zip(b_quads.iter()).enumerate() {
        let ch_base = q_idx * 4;
        let mut acc = f32x4::from_slice(simd, bq);
        let w0 = &dw_weight[ch_base * kernel..(ch_base + 1) * kernel];
        let w1 = &dw_weight[(ch_base + 1) * kernel..(ch_base + 2) * kernel];
        let w2 = &dw_weight[(ch_base + 2) * kernel..(ch_base + 3) * kernel];
        let w3 = &dw_weight[(ch_base + 3) * kernel..(ch_base + 4) * kernel];
        for ki in 0..kernel {
            let ext_off = (frame + ki) * channels + ch_base;
            let xv = f32x4::from_slice(simd, &ext[ext_off..ext_off + 4]);
            let wv = f32x4::from_slice(simd, &[w0[ki], w1[ki], w2[ki], w3[ki]]);
            acc = wv.mul_add(xv, acc);
        }
        acc.store_slice(yq);
    }
    let rem_base = y_quads.len() * 4;
    for (r_idx, (ys, &bias)) in y_rem.iter_mut().zip(b_rem.iter()).enumerate() {
        let ch = rem_base + r_idx;
        let mut acc = bias;
        let w_row = &dw_weight[ch * kernel..(ch + 1) * kernel];
        for ki in 0..kernel {
            acc = w_row[ki].mul_add(ext[(frame + ki) * channels + ch], acc);
        }
        *ys = acc;
    }

    // 2. LayerNorm across `channels` channels
    let mean = norm_buf.iter().copied().sum::<f32>() * inv_c;
    let mean_v = f32x4::splat(simd, mean);
    let mut var_acc = f32x4::splat(simd, 0.0);
    let (y_chunks, y_tail) = norm_buf.as_chunks::<4>();
    for yc in y_chunks {
        let diff = f32x4::from_slice(simd, yc) - mean_v;
        var_acc = diff.mul_add(diff, var_acc);
    }
    let mut var_sum = reduce_f32x4(var_acc);
    for &val in y_tail {
        let diff = val - mean;
        var_sum = diff.mul_add(diff, var_sum);
    }
    let inv_std = 1.0_f32 / var_sum.mul_add(inv_c, 1e-6_f32).sqrt();
    let inv_std_v = f32x4::splat(simd, inv_std);
    let (y_quads, y_rem) = norm_buf.as_chunks_mut::<4>();
    let (weight_quads, weight_rem) = norm_w.as_chunks::<4>();
    let (bias_quads, bias_rem) = norm_b.as_chunks::<4>();
    for ((yq, wq), bq) in y_quads
        .iter_mut()
        .zip(weight_quads.iter())
        .zip(bias_quads.iter())
    {
        let yv = (f32x4::from_slice(simd, yq) - mean_v) * inv_std_v;
        yv.mul_add(f32x4::from_slice(simd, wq), f32x4::from_slice(simd, bq))
            .store_slice(yq);
    }
    for ((ys, &nw), &nb) in y_rem.iter_mut().zip(weight_rem.iter()).zip(bias_rem.iter()) {
        *ys = ((*ys - mean) * inv_std).mul_add(nw, nb);
    }

    // 3. pwconv1 + tanh-approximated GELU
    matvec_4rows_simd(simd, pwconv1_w, norm_buf, Some(pwconv1_b), mid);
    let k_gelu = (2.0_f32 / core::f32::consts::PI).sqrt();
    for x_slot in mid.iter_mut() {
        let val = *x_slot;
        let inner = k_gelu * 0.044_715_f32.mul_add(val * val * val, val);
        *x_slot = 0.5_f32 * val * (1.0_f32 + inner.tanh());
    }

    // 4. pwconv2 + gamma scale + residual add into `out_row`
    let mid_c = mid.len();
    let (out_quads, out_rem) = out_row.as_chunks_mut::<4>();
    let (g_quads, g_rem) = gamma.as_chunks::<4>();
    for (q_idx, (oq, gq)) in out_quads.iter_mut().zip(g_quads.iter()).enumerate() {
        let o_base = q_idx * 4;
        let w_base = o_base * mid_c;
        let inits = [
            pwconv2_b[o_base],
            pwconv2_b[o_base + 1],
            pwconv2_b[o_base + 2],
            pwconv2_b[o_base + 3],
        ];
        let dots = dot_four_rows_f32_simd(
            simd,
            &pwconv2_w[w_base..w_base + mid_c],
            &pwconv2_w[w_base + mid_c..w_base + 2 * mid_c],
            &pwconv2_w[w_base + 2 * mid_c..w_base + 3 * mid_c],
            &pwconv2_w[w_base + 3 * mid_c..w_base + 4 * mid_c],
            mid,
            inits,
        );
        let dv = f32x4::from_slice(simd, &dots);
        let gv = f32x4::from_slice(simd, gq);
        let rv = f32x4::from_slice(simd, oq);
        dv.mul_add(gv, rv).store_slice(oq);
    }
    let rem_base = out_quads.len() * 4;
    for (r_idx, (os, &g_val)) in out_rem.iter_mut().zip(g_rem.iter()).enumerate() {
        let out_idx = rem_base + r_idx;
        let acc = dot_f32_simd(
            simd,
            &pwconv2_w[out_idx * mid_c..(out_idx + 1) * mid_c],
            mid,
            pwconv2_b[out_idx],
        );
        *os = acc.mul_add(g_val, *os);
    }
}

#[expect(
    clippy::inline_always,
    reason = "fearless_simd dispatch! requires inline(always) to inherit target_feature attributes."
)]
#[inline(always)]
fn rvq_argmin_simd<S: Simd>(
    simd: S,
    cb: &[f32],
    sq: &[f32],
    r: &[f32],
    codebook_size: usize,
    d: usize,
) -> usize {
    let mut best_e = 0_usize;
    let mut best_sc = f32::INFINITY;
    let quads = codebook_size / 4;
    for q_idx in 0..quads {
        let e_base = q_idx * 4;
        let w_base = e_base * d;
        let dots = dot_four_rows_f32_simd(
            simd,
            &cb[w_base..w_base + d],
            &cb[w_base + d..w_base + 2 * d],
            &cb[w_base + 2 * d..w_base + 3 * d],
            &cb[w_base + 3 * d..w_base + 4 * d],
            r,
            [0.0_f32; 4],
        );
        for (k, &dot) in dots.iter().enumerate() {
            let e = e_base + k;
            let sc = (-2.0_f32).mul_add(dot, sq[e]);
            if sc < best_sc {
                best_sc = sc;
                best_e = e;
            }
        }
    }
    for e in (quads * 4)..codebook_size {
        let c = &cb[e * d..(e + 1) * d];
        let dot = dot_f32_simd(simd, r, c, 0.0_f32);
        let sc = (-2.0_f32).mul_add(dot, sq[e]);
        if sc < best_sc {
            best_sc = sc;
            best_e = e;
        }
    }
    best_e
}

/// Number of RVQ codebooks (`1` semantic + `15` acoustic).
pub const TOKENIZER_NUM_CODEBOOKS: usize = 16;
/// Samples per 12.5 Hz codec frame at 24 kHz (`1920`).
pub const TOKENIZER_HOP_24K: usize = 1920;
/// Samples per 12.5 Hz codec frame at 48 kHz (`3840`).
pub const TOKENIZER_HOP_48K: usize = 3840;
/// Number of LRU slots for primed ICL reference streaming states (`8`).
pub const CODEC_SNAP_SLOTS: usize = 8;
/// Default sliding window for the 8-layer codec pre-transformer (`72` frames).
pub const CODEC_SLIDING_WINDOW: usize = 72;

#[inline]
fn usize_to_f32(n: usize) -> f32 {
    u16::try_from(n).map_or_else(
        |_| {
            let hi = u16::try_from(n >> 16).unwrap_or(u16::MAX);
            let lo = u16::try_from(n & 0xFFFF).unwrap_or(0);
            f32::from(hi).mul_add(65_536.0_f32, f32::from(lo))
        },
        f32::from,
    )
}

/// FNV-1a 64-bit content hash over `[K, T]` reference codes with `T` folded in,
/// matching `pipeline_codec_ref_key` in `qwentts.cpp`.
#[must_use]
pub fn codec_ref_key(codes: &[i32], num_codebooks: usize, frames: usize) -> u64 {
    const OFFSET_BASIS: u64 = 1_469_598_103_934_665_603;
    const PRIME: u64 = 1_099_511_628_211;
    let count = num_codebooks.saturating_mul(frames).min(codes.len());
    let mut hash = OFFSET_BASIS;
    for &code in &codes[..count] {
        for byte in code.to_le_bytes() {
            hash = (hash ^ u64::from(byte)).wrapping_mul(PRIME);
        }
    }
    let t_u64 = u64::try_from(frames).unwrap_or(0);
    (hash ^ t_u64).wrapping_mul(PRIME)
}

/// Decimate a 48 kHz mono waveform 2:1 to 24 kHz (`ref_audio_24k`) for
/// `VoiceDesign` reference extraction when a 48 kHz codec is active.
#[must_use]
pub fn decimate_48k_to_24k(samples_48k: &[f32]) -> Vec<f32> {
    let pairs = samples_48k.len() / 2;
    let mut out = Vec::with_capacity(pairs);
    for i in 0..pairs {
        out.push(f32::midpoint(samples_48k[2 * i], samples_48k[2 * i + 1]));
    }
    out
}

/// Padding mode for causal 1D convolutions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PadMode {
    /// Zero padding on left/right boundaries (`SEANet` and DAC decoder).
    Constant,
    /// Edge replication padding (Mimi encoder `k = 4, s = 2` downsample).
    Replicate,
}

/// Causal 1D convolution layer (`[out_c, in_c, kernel]` row-major weights).
#[derive(Clone, Debug)]
pub struct CausalConv1d {
    /// Output channels.
    pub out_c: usize,
    /// Input channels.
    pub in_c: usize,
    /// Kernel width.
    pub kernel: usize,
    /// Dilation factor (`>= 1`).
    pub dilation: usize,
    /// Stride (`>= 1`).
    pub stride: usize,
    /// Padding mode.
    pub pad_mode: PadMode,
    /// Weights `[out_c * in_c * kernel]`.
    pub weight: Vec<f32>,
    /// Optional bias `[out_c]`.
    pub bias: Option<Vec<f32>>,
}

impl CausalConv1d {
    /// Effective dilated kernel span `(kernel - 1) * dilation + 1`.
    #[must_use]
    pub const fn kernel_eff(&self) -> usize {
        (self.kernel - 1) * self.dilation + 1
    }

    /// Left-context frame count `(kernel - 1) * dilation` required for `stride == 1` streaming.
    #[must_use]
    pub const fn stream_context_frames(&self) -> usize {
        (self.kernel - 1) * self.dilation
    }

    /// Offline causal forward pass on `[frames, in_c]` time-first input.
    pub fn forward(&self, input: &[f32], frames: usize) -> io::Result<(Vec<f32>, usize)> {
        self.forward_parallel(input, frames, 1)
    }

    /// Offline forward pass using bounded workers for independent output frames.
    pub fn forward_parallel(
        &self,
        input: &[f32],
        frames: usize,
        threads: usize,
    ) -> io::Result<(Vec<f32>, usize)> {
        if frames == 0
            || self.in_c == 0
            || self.out_c == 0
            || self.stride == 0
            || self.dilation == 0
            || self.kernel == 0
            || input.len() != frames * self.in_c
            || self.weight.len() != self.out_c * self.in_c * self.kernel
            || self.bias.as_ref().is_some_and(|b| b.len() != self.out_c)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid CausalConv1d dimensions.",
            ));
        }
        let k_eff = self.kernel_eff();
        let pad_left = k_eff.saturating_sub(self.stride);
        let out_frames = (frames + pad_left - k_eff).div_ceil(self.stride) + 1;
        let ideal_len = (out_frames - 1) * self.stride + k_eff - pad_left;
        let extra_pad = ideal_len.saturating_sub(frames);
        let padded_len = pad_left + frames + extra_pad;

        let mut padded = vec![0.0_f32; padded_len * self.in_c];
        if self.pad_mode == PadMode::Replicate {
            for t in 0..pad_left {
                padded[t * self.in_c..(t + 1) * self.in_c].copy_from_slice(&input[..self.in_c]);
            }
        }
        padded[pad_left * self.in_c..(pad_left + frames) * self.in_c].copy_from_slice(input);
        if self.pad_mode == PadMode::Replicate && extra_pad > 0 {
            let last = &input[(frames - 1) * self.in_c..frames * self.in_c];
            for t in 0..extra_pad {
                let dst = (pad_left + frames + t) * self.in_c;
                padded[dst..dst + self.in_c].copy_from_slice(last);
            }
        }

        let out = self.compute_frames(&padded, out_frames, self.stride, threads);
        Ok((out, out_frames))
    }

    /// Stateful streaming causal forward pass (`stride == 1`) updating `state` (`[L, in_c]`) in place.
    pub fn forward_stream(
        &self,
        input: &[f32],
        frames: usize,
        state: &mut [f32],
    ) -> io::Result<Vec<f32>> {
        self.forward_stream_parallel(input, frames, state, 1)
    }

    /// Stream independent output frames on bounded workers, preserving context order.
    pub fn forward_stream_parallel(
        &self,
        input: &[f32],
        frames: usize,
        state: &mut [f32],
        threads: usize,
    ) -> io::Result<Vec<f32>> {
        let ctx_frames = self.stream_context_frames();
        if self.stride != 1
            || self.in_c == 0
            || self.out_c == 0
            || frames == 0
            || input.len() != frames * self.in_c
            || state.len() != ctx_frames * self.in_c
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid streaming CausalConv1d state or input.",
            ));
        }
        let ext_frames = ctx_frames + frames;
        let mut ext = Vec::with_capacity(ext_frames * self.in_c);
        ext.extend_from_slice(state);
        ext.extend_from_slice(input);

        if ctx_frames > 0 {
            let tail_start = frames * self.in_c;
            state.copy_from_slice(&ext[tail_start..]);
        }

        Ok(self.compute_frames(&ext, frames, 1, threads))
    }

    fn compute_frames(
        &self,
        ext: &[f32],
        out_frames: usize,
        stride: usize,
        threads: usize,
    ) -> Vec<f32> {
        let mut out = vec![0.0_f32; out_frames * self.out_c];
        let level = Level::new();
        let bias = self.bias.as_deref();
        let work = out_frames.saturating_mul(self.weight.len());
        let workers = frame_workers(threads, out_frames, work);
        row_workers::run_aligned(&mut out, workers, self.out_c, &|offset, output| {
            let first_frame = offset / self.out_c;
            if self.kernel == 1 {
                for (frame, out_row) in output.chunks_exact_mut(self.out_c).enumerate() {
                    let t_in = (first_frame + frame) * stride;
                    let in_row = &ext[t_in * self.in_c..(t_in + 1) * self.in_c];
                    dispatch!(
                        level,
                        simd => matvec_4rows_simd(simd, &self.weight, in_row, bias, out_row)
                    );
                }
                return;
            }
            let mut patch = vec![0.0_f32; self.in_c * self.kernel];
            for (frame, out_row) in output.chunks_exact_mut(self.out_c).enumerate() {
                let t_in_base = (first_frame + frame) * stride;
                for ic in 0..self.in_c {
                    let p_base = ic * self.kernel;
                    for k in 0..self.kernel {
                        let t_in = t_in_base + k * self.dilation;
                        patch[p_base + k] = ext[t_in * self.in_c + ic];
                    }
                }
                dispatch!(
                    level,
                    simd => matvec_4rows_simd(simd, &self.weight, &patch, bias, out_row)
                );
            }
        });
        out
    }
}

/// Causal 1D transposed convolution (`[in_c, out_c, kernel]` `PyTorch` layout).
#[derive(Clone, Debug)]
pub struct CausalTransConv1d {
    /// Input channels.
    pub in_c: usize,
    /// Output channels.
    pub out_c: usize,
    /// Kernel width (`>= stride`).
    pub kernel: usize,
    /// Upsampling stride (`>= 1`).
    pub stride: usize,
    /// Weights `[in_c * out_c * kernel]` in `[ic, oc, k]` order.
    pub weight: Vec<f32>,
    /// Optional bias `[out_c]`.
    pub bias: Option<Vec<f32>>,
}

impl CausalTransConv1d {
    /// Overlap carry frames `kernel - stride` retained across streaming chunks.
    #[must_use]
    pub const fn carry_frames(&self) -> usize {
        self.kernel.saturating_sub(self.stride)
    }

    /// Offline causal transposed convolution on `[frames, in_c]` time-first input,
    /// returning `[frames * stride, out_c]`.
    pub fn forward(&self, input: &[f32], frames: usize) -> io::Result<(Vec<f32>, usize)> {
        self.forward_parallel(input, frames, 1)
    }

    /// Offline transposed convolution using bounded workers over output windows.
    pub fn forward_parallel(
        &self,
        input: &[f32],
        frames: usize,
        threads: usize,
    ) -> io::Result<(Vec<f32>, usize)> {
        let mut carry = vec![0.0_f32; self.carry_frames() * self.out_c];
        let out = self.forward_stream_parallel(input, frames, &mut carry, threads)?;
        Ok((out, frames * self.stride))
    }

    /// Stateful streaming causal transposed convolution updating `carry` (`[kernel - stride, out_c]`).
    pub fn forward_stream(
        &self,
        input: &[f32],
        frames: usize,
        carry: &mut [f32],
    ) -> io::Result<Vec<f32>> {
        self.forward_stream_parallel(input, frames, carry, 1)
    }

    /// Stream disjoint output windows while retaining source-frame and FMA order.
    pub fn forward_stream_parallel(
        &self,
        input: &[f32],
        frames: usize,
        carry: &mut [f32],
        threads: usize,
    ) -> io::Result<Vec<f32>> {
        let trim = self.carry_frames();
        if frames == 0
            || self.in_c == 0
            || self.out_c == 0
            || self.stride == 0
            || self.kernel < self.stride
            || input.len() != frames * self.in_c
            || carry.len() != trim * self.out_c
            || self.weight.len() != self.in_c * self.out_c * self.kernel
            || self.bias.as_ref().is_some_and(|b| b.len() != self.out_c)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid CausalTransConv1d dimensions.",
            ));
        }
        let emit = frames * self.stride;
        let raw_frames = emit + trim;
        let mut raw = vec![0.0_f32; raw_frames * self.out_c];
        if trim > 0 {
            raw[..trim * self.out_c].copy_from_slice(carry);
        }

        let level = Level::new();
        let work = frames.saturating_mul(self.weight.len());
        let workers = frame_workers(threads, raw_frames, work);
        row_workers::run_aligned(&mut raw, workers, self.out_c, &|offset, output| {
            let first_frame = offset / self.out_c;
            let end_frame = first_frame + output.len() / self.out_c;
            let first_source = first_frame
                .saturating_sub(self.kernel - 1)
                .div_ceil(self.stride);
            let end_source = end_frame.div_ceil(self.stride).min(frames);
            // Adjacent jobs can read the same input frame, but write disjoint
            // windows. Each sample keeps the original t -> ic -> FMA sequence.
            for t in first_source..end_source {
                let base = t * self.stride;
                let start = base.max(first_frame);
                let end = (base + self.kernel).min(end_frame);
                let in_row = &input[t * self.in_c..(t + 1) * self.in_c];
                let raw_window = &mut output
                    [(start - first_frame) * self.out_c..(end - first_frame) * self.out_c];
                dispatch!(
                    level,
                    simd => trans_conv1d_scatter_frame_simd(
                        simd,
                        in_row,
                        &self.weight,
                        self.out_c,
                        self.kernel,
                        start - base..end - base,
                        raw_window,
                    )
                );
            }
        });

        if trim > 0 {
            carry.copy_from_slice(&raw[emit * self.out_c..]);
        }

        raw.truncate(emit * self.out_c);
        if let Some(bias) = &self.bias {
            for t in 0..emit {
                let row = &mut raw[t * self.out_c..(t + 1) * self.out_c];
                dispatch!(level, simd => add_slice_in_place_simd(simd, row, bias));
            }
        }
        Ok(raw)
    }
}

/// Precomputed `SnakeBeta` activation parameters (`a = exp(alpha)`, `inv_b = 1 / (exp(beta) + 1e-9)`).
#[derive(Clone, Debug)]
pub struct SnakeBeta {
    /// Pre-folded `exp(alpha)` per channel `[channels]`.
    pub a: Vec<f32>,
    /// Pre-folded `1.0 / (exp(beta) + 1e-9)` per channel `[channels]`.
    pub inv_b: Vec<f32>,
}

impl SnakeBeta {
    /// Construct `SnakeBeta` from raw log-space `alpha` and `beta` tensors.
    pub fn from_log_params(alpha: &[f32], beta: &[f32]) -> io::Result<Self> {
        if alpha.is_empty() || alpha.len() != beta.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "SnakeBeta alpha/beta length mismatch.",
            ));
        }
        let a = alpha.iter().map(|&v| v.exp()).collect();
        let inv_b = beta
            .iter()
            .map(|&v| 1.0_f32 / (v.exp() + 1e-9_f32))
            .collect();
        Ok(Self { a, inv_b })
    }

    /// Apply `y = x + sin^2(a * x) * inv_b` in place over `[frames, channels]` time-first data.
    pub fn apply_in_place(&self, data: &mut [f32], frames: usize) -> io::Result<()> {
        let channels = self.a.len();
        if data.len() != frames * channels {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "SnakeBeta input shape mismatch.",
            ));
        }
        for t in 0..frames {
            let row = &mut data[t * channels..(t + 1) * channels];
            for c in 0..channels {
                let x = row[c];
                let s = (self.a[c] * x).sin();
                row[c] = (s * s).mul_add(self.inv_b[c], x);
            }
        }
        Ok(())
    }
}

/// Dilated DAC residual unit (`act1 -> conv1(k=7, dilation) -> act2 -> conv2(k=1) + skip`).
#[derive(Clone, Debug)]
pub struct DacResUnit {
    /// First `SnakeBeta` activation.
    pub act1: SnakeBeta,
    /// Dilated `k = 7` causal convolution.
    pub conv1: CausalConv1d,
    /// Second `SnakeBeta` activation.
    pub act2: SnakeBeta,
    /// Pointwise `k = 1` convolution.
    pub conv2: CausalConv1d,
}

impl DacResUnit {
    /// Execute one residual unit with streaming `conv1_state`.
    pub fn forward_stream(
        &self,
        input: &[f32],
        frames: usize,
        conv1_state: &mut [f32],
    ) -> io::Result<Vec<f32>> {
        self.forward_stream_parallel(input, frames, conv1_state, 1)
    }

    /// Execute a residual unit using bounded convolution workers.
    pub fn forward_stream_parallel(
        &self,
        input: &[f32],
        frames: usize,
        conv1_state: &mut [f32],
        threads: usize,
    ) -> io::Result<Vec<f32>> {
        let mut h = input.to_vec();
        self.act1.apply_in_place(&mut h, frames)?;
        let mut h = self
            .conv1
            .forward_stream_parallel(&h, frames, conv1_state, threads)?;
        self.act2.apply_in_place(&mut h, frames)?;
        let mut dummy_state = [];
        let mut out = self
            .conv2
            .forward_stream_parallel(&h, frames, &mut dummy_state, threads)?;
        let level = Level::new();
        dispatch!(level, simd => add_slice_in_place_simd(simd, &mut out, input));
        Ok(out)
    }
}

/// One DAC upsampling block (`snake1 -> CausalTransConv1d -> 3 DacResUnits`).
#[derive(Clone, Debug)]
pub struct DacBlock {
    /// Pre-upsample `SnakeBeta` activation.
    pub snake1: SnakeBeta,
    /// Causal transposed convolution (`kernel = 2 * stride`).
    pub trans_conv: CausalTransConv1d,
    /// Three dilated residual units (`dilations = [1, 3, 9]`).
    pub res_units: [DacResUnit; 3],
}

impl DacBlock {
    /// Execute one DAC block, upsampling `frames` to `frames * stride`.
    pub fn forward_stream(
        &self,
        input: &[f32],
        frames: usize,
        carry: &mut [f32],
        ru_states: &mut [Vec<f32>; 3],
    ) -> io::Result<(Vec<f32>, usize)> {
        self.forward_stream_parallel(input, frames, carry, ru_states, 1)
    }

    /// Execute a DAC block with bounded independent-frame convolution workers.
    pub fn forward_stream_parallel(
        &self,
        input: &[f32],
        frames: usize,
        carry: &mut [f32],
        ru_states: &mut [Vec<f32>; 3],
        threads: usize,
    ) -> io::Result<(Vec<f32>, usize)> {
        let mut h = input.to_vec();
        self.snake1.apply_in_place(&mut h, frames)?;
        let mut h = self
            .trans_conv
            .forward_stream_parallel(&h, frames, carry, threads)?;
        let out_frames = frames * self.trans_conv.stride;
        for (ru, st) in self.res_units.iter().zip(ru_states.iter_mut()) {
            h = ru.forward_stream_parallel(&h, out_frames, st, threads)?;
        }
        Ok((h, out_frames))
    }
}

/// `ConvNeXt` block used in the 12 Hz codec upsample stage.
#[derive(Clone, Debug)]
pub struct ConvNeXtBlock {
    /// Number of channels (`1024`).
    pub channels: usize,
    /// Depthwise causal kernel width (`7`).
    pub kernel: usize,
    /// Depthwise weights `[channels * kernel]`.
    pub dw_weight: Vec<f32>,
    /// Depthwise bias `[channels]`.
    pub dw_bias: Vec<f32>,
    /// `LayerNorm` scale `[channels]`.
    pub norm_w: Vec<f32>,
    /// `LayerNorm` bias `[channels]`.
    pub norm_b: Vec<f32>,
    /// Pointwise expansion `[4 * channels, channels]` (`out_c, in_c`).
    pub pwconv1_w: Vec<f32>,
    /// Pointwise expansion bias `[4 * channels]`.
    pub pwconv1_b: Vec<f32>,
    /// Pointwise projection `[channels, 4 * channels]` (`out_c, in_c`).
    pub pwconv2_w: Vec<f32>,
    /// Pointwise projection bias `[channels]`.
    pub pwconv2_b: Vec<f32>,
    /// `LayerScale` `[channels]`.
    pub gamma: Vec<f32>,
}

impl ConvNeXtBlock {
    /// Run the `ConvNeXt` block on `[frames, channels]` time-first input with `dw_state` (`[kernel - 1, channels]`).
    pub fn forward_stream(
        &self,
        input: &[f32],
        frames: usize,
        dw_state: &mut [f32],
    ) -> io::Result<Vec<f32>> {
        self.forward_stream_parallel(input, frames, dw_state, 1)
    }

    /// Execute independent frames on bounded workers with unchanged normalization.
    pub fn forward_stream_parallel(
        &self,
        input: &[f32],
        frames: usize,
        dw_state: &mut [f32],
        threads: usize,
    ) -> io::Result<Vec<f32>> {
        let c = self.channels;
        let k = self.kernel;
        let ctx_len = k.saturating_sub(1);
        if frames == 0
            || c == 0
            || k == 0
            || input.len() != frames * c
            || dw_state.len() != ctx_len * c
            || self.dw_weight.len() != c * k
            || self.dw_bias.len() != c
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid ConvNeXtBlock dimensions.",
            ));
        }
        let mut ext = Vec::with_capacity((ctx_len + frames) * c);
        ext.extend_from_slice(dw_state);
        ext.extend_from_slice(input);
        if ctx_len > 0 {
            dw_state.copy_from_slice(&ext[frames * c..]);
        }

        let inv_c = 1.0_f32 / usize_to_f32(c);
        let mid_c = self.pwconv1_b.len();
        let mut out = input.to_vec();
        let level = Level::new();
        let work = frames.saturating_mul(self.pwconv1_w.len() + self.pwconv2_w.len());
        let workers = frame_workers(threads, frames, work);
        row_workers::run_aligned(&mut out, workers, c, &|offset, output| {
            let mut y = vec![0.0_f32; c];
            let mut mid = vec![0.0_f32; mid_c];
            for (frame, out_row) in output.chunks_exact_mut(c).enumerate() {
                let t = offset / c + frame;
                dispatch!(
                level,
                simd => convnext_frame_simd(
                    simd,
                    &ext,
                    t,
                    c,
                    k,
                    inv_c,
                    &self.dw_weight,
                    &self.dw_bias,
                    &self.norm_w,
                    &self.norm_b,
                    &self.pwconv1_w,
                    &self.pwconv1_b,
                    &self.pwconv2_w,
                    &self.pwconv2_b,
                    &self.gamma,
                    &mut y,
                    &mut mid,
                    out_row,
                )
                );
            }
        });
        Ok(out)
    }
}

/// Split RVQ group (semantic or acoustic) for the 12 Hz codec decoder.
#[derive(Clone, Debug)]
pub struct RvqGroup {
    /// Number of codebooks in this split (`1` for semantic, `15` for acoustic).
    pub num_codebooks: usize,
    /// Number of entries per codebook (`2048`).
    pub codebook_size: usize,
    /// Internal embedding dimension (`256`).
    pub codebook_dim: usize,
    /// Output hidden dimension (`512`).
    pub hidden_dim: usize,
    /// Codebook tables: `num_codebooks` tables of `[codebook_size * codebook_dim]`.
    pub codebooks: Vec<Vec<f32>>,
    /// 1x1 output projection `[hidden_dim * codebook_dim]` (`[out_c, in_c]`).
    pub out_proj_w: Vec<f32>,
}

impl RvqGroup {
    /// Decode `[frames, num_codebooks]` time-first codes into `[frames, hidden_dim]`.
    pub fn decode(&self, codes: &[i32], frames: usize) -> io::Result<Vec<f32>> {
        if codes.len() != frames * self.num_codebooks
            || self.codebooks.len() != self.num_codebooks
            || self.out_proj_w.len() != self.hidden_dim * self.codebook_dim
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid RVQ group decode dimensions.",
            ));
        }
        let mut out = vec![0.0_f32; frames * self.hidden_dim];
        let mut sum_emb = vec![0.0_f32; self.codebook_dim];
        let level = Level::new();
        for t in 0..frames {
            sum_emb.fill(0.0_f32);
            for k in 0..self.num_codebooks {
                let idx = usize::try_from(codes[t * self.num_codebooks + k]).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidInput, "Negative RVQ code index.")
                })?;
                if idx >= self.codebook_size {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "RVQ code index exceeds codebook_size.",
                    ));
                }
                let row =
                    &self.codebooks[k][idx * self.codebook_dim..(idx + 1) * self.codebook_dim];
                dispatch!(level, simd => add_slice_in_place_simd(simd, &mut sum_emb, row));
            }
            let out_row = &mut out[t * self.hidden_dim..(t + 1) * self.hidden_dim];
            dispatch!(
                level,
                simd => matvec_4rows_simd(simd, &self.out_proj_w, &sum_emb, None, out_row)
            );
        }
        Ok(out)
    }
}

/// Split RVQ decoder (`1` semantic + `15` acoustic codebooks -> `512` channels).
#[derive(Clone, Debug)]
pub struct RvqDecoder {
    /// Semantic RVQ split (`1` codebook).
    pub semantic: RvqGroup,
    /// Acoustic RVQ split (`15` codebooks).
    pub acoustic: RvqGroup,
}

impl RvqDecoder {
    /// Decode `[frames, 16]` time-first codes into `[frames, 512]`.
    pub fn decode(&self, codes: &[i32], frames: usize) -> io::Result<Vec<f32>> {
        let k_sem = self.semantic.num_codebooks;
        let k_aco = self.acoustic.num_codebooks;
        let total_k = k_sem + k_aco;
        if codes.len() != frames * total_k || self.semantic.hidden_dim != self.acoustic.hidden_dim {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid RvqDecoder input codes.",
            ));
        }
        let mut sem_codes = Vec::with_capacity(frames * k_sem);
        let mut aco_codes = Vec::with_capacity(frames * k_aco);
        for t in 0..frames {
            let row = &codes[t * total_k..(t + 1) * total_k];
            sem_codes.extend_from_slice(&row[..k_sem]);
            aco_codes.extend_from_slice(&row[k_sem..]);
        }
        let mut h_sem = self.semantic.decode(&sem_codes, frames)?;
        let h_aco = self.acoustic.decode(&aco_codes, frames)?;
        let level = Level::new();
        dispatch!(level, simd => add_slice_in_place_simd(simd, &mut h_sem, &h_aco));
        Ok(h_sem)
    }
}

/// Host-side L2 nearest-centroid RVQ encoder side (`input_proj` + residual codebooks).
#[derive(Clone, Debug)]
pub struct RvqEncodeSide {
    /// Number of residual layers (`1` for semantic, `15` for acoustic).
    pub num_layers: usize,
    /// Codebook vocabulary size (`2048`).
    pub codebook_size: usize,
    /// Internal codebook dimension (`256`).
    pub codebook_dim: usize,
    /// Input hidden dimension (`512`).
    pub hidden_size: usize,
    /// Input 1x1 projection `[codebook_dim, hidden_size]` (`out_c, in_c`).
    pub input_proj: Vec<f32>,
    /// Codebook centroids `[num_layers][codebook_size * codebook_dim]`.
    pub codebooks: Vec<Vec<f32>>,
    /// Precomputed `||c_e||^2` per entry `[num_layers][codebook_size]`.
    pub sqsum: Vec<Vec<f32>>,
}

impl RvqEncodeSide {
    /// Construct an RVQ encode side and precompute centroid squared norms `||c_e||^2`.
    pub fn new(
        codebook_size: usize,
        codebook_dim: usize,
        hidden_size: usize,
        input_proj: Vec<f32>,
        codebooks: Vec<Vec<f32>>,
    ) -> io::Result<Self> {
        let num_layers = codebooks.len();
        if num_layers == 0
            || codebook_size == 0
            || codebook_dim == 0
            || hidden_size == 0
            || input_proj.len() != codebook_dim * hidden_size
            || codebooks
                .iter()
                .any(|cb| cb.len() != codebook_size * codebook_dim)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid RvqEncodeSide dimensions.",
            ));
        }
        let level = Level::new();
        let mut sqsum = Vec::with_capacity(num_layers);
        for cb in &codebooks {
            let mut layer_sq = Vec::with_capacity(codebook_size);
            for e in 0..codebook_size {
                let row = &cb[e * codebook_dim..(e + 1) * codebook_dim];
                let norm_sq = dispatch!(level, simd => dot_f32_simd(simd, row, row, 0.0_f32));
                layer_sq.push(norm_sq);
            }
            sqsum.push(layer_sq);
        }
        Ok(Self {
            num_layers,
            codebook_size,
            codebook_dim,
            hidden_size,
            input_proj,
            codebooks,
            sqsum,
        })
    }

    /// Quantize `[frames, hidden_size]` into `[num_layers, frames]` codes.
    pub fn encode(
        &self,
        hidden: &[f32],
        frames: usize,
        codes_out: &mut Vec<i32>,
    ) -> io::Result<()> {
        if hidden.len() != frames * self.hidden_size {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid RvqEncodeSide input length.",
            ));
        }
        let d = self.codebook_dim;
        let mut res = vec![0.0_f32; frames * d];
        let level = Level::new();
        for t in 0..frames {
            let x = &hidden[t * self.hidden_size..(t + 1) * self.hidden_size];
            let y = &mut res[t * d..(t + 1) * d];
            dispatch!(
                level,
                simd => matvec_4rows_simd(simd, &self.input_proj, x, None, y)
            );
        }

        for l in 0..self.num_layers {
            let cb = &self.codebooks[l];
            let sq = &self.sqsum[l];
            for t in 0..frames {
                let r = &mut res[t * d..(t + 1) * d];
                let best_e = dispatch!(
                    level,
                    simd => rvq_argmin_simd(simd, cb, sq, r, self.codebook_size, d)
                );
                codes_out.push(i32::try_from(best_e).unwrap_or(0));
                let best_centroid = &cb[best_e * d..(best_e + 1) * d];
                dispatch!(level, simd => sub_slice_in_place_simd(simd, r, best_centroid));
            }
        }
        Ok(())
    }
}

/// Complete streaming state for the 12 Hz / 48 kHz causal audio decoder.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CodecStreamState {
    /// Absolute frame cursor.
    pub pos: usize,
    /// `pre_conv` (`k = 3`) left-context `[2, 512]`.
    pub pre_conv: Vec<f32>,
    /// Upsample `ConvNeXt` depthwise left-contexts (`[dwconv_kernel - 1, channels]` per block).
    pub upsample_dw: Vec<Vec<f32>>,
    /// DAC `conv_pre` (`k = 7`) left-context `[6, 1024]`.
    pub dac_pre: Vec<f32>,
    /// DAC transposed-conv overlap carries (`[stride, out_ch]` per block, 4 blocks @ 24 kHz or 5 @ 48 kHz).
    pub dac_carry: Vec<Vec<f32>>,
    /// DAC residual-unit `conv1` (`k = 7` dilated) left-contexts (`[3][6 * dilation, out_ch]` per block).
    pub dac_ru: Vec<[Vec<f32>; 3]>,
    /// DAC `conv_post` (`k = 7`) left-context.
    pub dac_post: Vec<f32>,
    /// Sliding-window pre-transformer K ring cache.
    pub kv_k: Vec<f32>,
    /// Sliding-window pre-transformer V ring cache.
    pub kv_v: Vec<f32>,
}

/// One slot in the 8-entry `CodecStateSnap` LRU cache.
#[derive(Clone, Debug, Default)]
pub struct CodecStateSnap {
    /// FNV-1a 64-bit reference-code content key.
    pub key: u64,
    /// Monotonic LRU access stamp (`0` means empty).
    pub stamp: u64,
    /// Saved streaming decoder state after ICL reference priming.
    pub state: CodecStreamState,
}

/// 8-slot LRU cache (`CODEC_SNAP_SLOTS = 8`) of primed ICL reference codec states.
#[derive(Clone, Debug)]
pub struct CodecStateCache {
    slots: [CodecStateSnap; CODEC_SNAP_SLOTS],
    stamp: u64,
}

impl Default for CodecStateCache {
    fn default() -> Self {
        Self {
            slots: core::array::from_fn(|_| CodecStateSnap::default()),
            stamp: 0,
        }
    }
}

impl CodecStateCache {
    /// Create an empty 8-slot `CodecStateSnap` LRU cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Borrow the streaming state matching `key`, bumping its LRU stamp on hit.
    pub fn get(&mut self, key: u64) -> Option<&CodecStreamState> {
        for slot in &mut self.slots {
            if slot.stamp != 0 && slot.key == key {
                self.stamp = self.stamp.saturating_add(1);
                slot.stamp = self.stamp;
                return Some(&slot.state);
            }
        }
        None
    }

    /// Borrow the cached streaming state for `key`, or compute and snapshot it on a miss.
    pub fn get_or_prime(
        &mut self,
        key: u64,
        prime: impl FnOnce() -> io::Result<CodecStreamState>,
    ) -> io::Result<&CodecStreamState> {
        if let Some(state) = self.get(key) {
            return Ok(state);
        }
        let state = prime()?;
        self.stamp = self.stamp.saturating_add(1);
        let mut lru_idx = 0_usize;
        for i in 1..CODEC_SNAP_SLOTS {
            if self.slots[i].stamp < self.slots[lru_idx].stamp {
                lru_idx = i;
            }
        }
        self.slots[lru_idx] = CodecStateSnap {
            key,
            stamp: self.stamp,
            state,
        };
        Ok(&self.slots[lru_idx].state)
    }

    /// Restore the streaming state matching `key`, bumping its LRU stamp on hit.
    pub fn restore(&mut self, key: u64) -> Option<CodecStreamState> {
        self.get(key).cloned()
    }

    /// Save `state` under `key`, evicting the least-recently-used slot when full.
    pub fn snapshot(&mut self, key: u64, state: CodecStreamState) {
        self.stamp = self.stamp.saturating_add(1);
        if let Some(existing) = self.slots.iter_mut().find(|s| s.stamp != 0 && s.key == key) {
            existing.state = state;
            existing.stamp = self.stamp;
            return;
        }
        let mut lru_idx = 0_usize;
        for i in 1..CODEC_SNAP_SLOTS {
            if self.slots[i].stamp < self.slots[lru_idx].stamp {
                lru_idx = i;
            }
        }
        self.slots[lru_idx] = CodecStateSnap {
            key,
            stamp: self.stamp,
            state,
        };
    }
}

/// Complete DAC decoder supporting both 24 kHz (`4` blocks: `8/5/4/3`) and
/// drop-in 48 kHz (`takuma104/Qwen3-TTS-Tokenizer-12Hz-48kHz`, `5` blocks: `8/5/4/3/2`).
#[derive(Clone, Debug)]
pub struct DacDecoder {
    /// Entry causal conv (`k = 7, 1024 -> 1536`).
    pub conv_pre: CausalConv1d,
    /// Upsampling blocks (`4` blocks for 24 kHz, `5` blocks for 48 kHz).
    pub blocks: Vec<DacBlock>,
    /// Post-block `SnakeBeta` activation (`96` channels @ 24 kHz, `48` channels @ 48 kHz).
    pub snake_post: SnakeBeta,
    /// Final causal conv (`k = 7 -> 1` mono channel).
    pub conv_post: CausalConv1d,
}

impl DacDecoder {
    /// Output sample rate (`24_000` Hz for 4 blocks, `48_000` Hz when the 5th 2x block is present).
    #[must_use]
    pub fn sample_rate(&self) -> u32 {
        let total_stride: usize = self.blocks.iter().map(|b| b.trans_conv.stride).product();
        if total_stride == 960 {
            SAMPLE_RATE_48K
        } else {
            SAMPLE_RATE
        }
    }

    /// Execute streaming DAC decode over `[frames, in_c]` post-upsample latents,
    /// clamping output samples to `[-1.0, 1.0]`.
    pub fn forward_stream(
        &self,
        input: &[f32],
        frames: usize,
        state: &mut CodecStreamState,
    ) -> io::Result<Vec<f32>> {
        self.forward_stream_parallel(input, frames, state, 1)
    }

    /// Decode using bounded workers for each independent convolution stage.
    pub fn forward_stream_parallel(
        &self,
        input: &[f32],
        frames: usize,
        state: &mut CodecStreamState,
        threads: usize,
    ) -> io::Result<Vec<f32>> {
        if state.dac_carry.len() != self.blocks.len() || state.dac_ru.len() != self.blocks.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "DAC streaming state block count mismatch.",
            ));
        }
        let mut h =
            self.conv_pre
                .forward_stream_parallel(input, frames, &mut state.dac_pre, threads)?;
        let mut cur_frames = frames;
        for (i, blk) in self.blocks.iter().enumerate() {
            let (next_h, next_f) = blk.forward_stream_parallel(
                &h,
                cur_frames,
                &mut state.dac_carry[i],
                &mut state.dac_ru[i],
                threads,
            )?;
            h = next_h;
            cur_frames = next_f;
        }
        self.snake_post.apply_in_place(&mut h, cur_frames)?;
        let mut audio =
            self.conv_post
                .forward_stream_parallel(&h, cur_frames, &mut state.dac_post, threads)?;
        for sample in &mut audio {
            *sample = sample.clamp(-1.0_f32, 1.0_f32);
        }
        Ok(audio)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CODEC_SNAP_SLOTS, CausalConv1d, CausalTransConv1d, CodecStateCache, CodecStreamState,
        ConvNeXtBlock, DacBlock, DacDecoder, DacResUnit, PadMode, RvqDecoder, RvqEncodeSide,
        RvqGroup, SnakeBeta, codec_ref_key, decimate_48k_to_24k,
    };
    use crate::{SAMPLE_RATE, SAMPLE_RATE_48K};

    fn values(count: usize) -> Vec<f32> {
        (0..count)
            .map(|i| f32::from(i16::try_from(i % 37).unwrap_or(0) - 18) * 0.013)
            .collect()
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "Keep independent scatter replay and the paired convolution/context controls in one fixed codec workload."
    )]
    fn parallel_codec_frames_preserve_bits_context_and_scatter_order() -> std::io::Result<()> {
        // Odd widths exercise SIMD tails; enough fixed work selects workers.
        let conv = CausalConv1d {
            in_c: 19,
            out_c: 17,
            kernel: 7,
            dilation: 3,
            stride: 1,
            pad_mode: PadMode::Constant,
            weight: values(19 * 17 * 7),
            bias: Some(values(17)),
        };
        let input = values(41 * 19);
        let mut serial_state = values(conv.stream_context_frames() * 19);
        let mut parallel_state = serial_state.clone();
        let serial = conv.forward_stream(&input, 41, &mut serial_state)?;
        let parallel = conv.forward_stream_parallel(&input, 41, &mut parallel_state, 4)?;
        assert_eq!(
            serial.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
            parallel.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
        );
        assert_eq!(serial_state, parallel_state);
        assert_eq!(
            conv.forward(&input, 41)?,
            conv.forward_parallel(&input, 41, 4)?
        );

        for (kernel, stride) in [(16, 8), (9, 2), (4, 4)] {
            let trans = CausalTransConv1d {
                in_c: 41,
                out_c: 17,
                kernel,
                stride,
                weight: values(41 * 17 * kernel),
                bias: Some(values(17)),
            };
            let frames = 41;
            let input = values(frames * trans.in_c);
            let initial = values(trans.carry_frames() * trans.out_c);
            let mut expected =
                vec![0.0_f32; (frames * stride + trans.carry_frames()) * trans.out_c];
            expected[..initial.len()].copy_from_slice(&initial);
            // Independent scalar replay of the original whole-window scatter.
            for t in 0..frames {
                for ic in 0..trans.in_c {
                    for oc in 0..trans.out_c {
                        for k in 0..kernel {
                            let dest = (t * stride + k) * trans.out_c + oc;
                            expected[dest] = input[t * trans.in_c + ic].mul_add(
                                trans.weight[(ic * trans.out_c + oc) * kernel + k],
                                expected[dest],
                            );
                        }
                    }
                }
            }
            let expected_carry = expected[frames * stride * trans.out_c..].to_vec();
            expected.truncate(frames * stride * trans.out_c);
            for row in expected.chunks_exact_mut(trans.out_c) {
                for (value, bias) in row.iter_mut().zip(trans.bias.as_deref().unwrap_or(&[])) {
                    *value += bias;
                }
            }
            for threads in [1, 4, 18] {
                let mut carry = initial.clone();
                let output = trans.forward_stream_parallel(&input, frames, &mut carry, threads)?;
                assert_eq!(
                    output.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                    expected.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
                );
                assert_eq!(carry, expected_carry);
            }
        }

        let channels = 17;
        let mid = channels * 4;
        let block = ConvNeXtBlock {
            channels,
            kernel: 7,
            dw_weight: values(channels * 7),
            dw_bias: values(channels),
            norm_w: vec![1.0; channels],
            norm_b: values(channels),
            pwconv1_w: values(channels * mid),
            pwconv1_b: values(mid),
            pwconv2_w: values(channels * mid),
            pwconv2_b: values(channels),
            gamma: values(channels),
        };
        let input = values(41 * channels);
        let mut serial_state = values(6 * channels);
        let mut parallel_state = serial_state.clone();
        let serial = block.forward_stream(&input, 41, &mut serial_state)?;
        let parallel = block.forward_stream_parallel(&input, 41, &mut parallel_state, 4)?;
        assert_eq!(
            serial.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
            parallel.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
        );
        assert_eq!(serial_state, parallel_state);
        Ok(())
    }

    #[test]
    fn codec_stage_sizes_preserve_selected_worker_team() {
        for frames in [2, 4, 8, 32, 1920] {
            assert_eq!(super::frame_workers(18, frames, 65_536), 18);
        }
        assert_eq!(super::frame_workers(18, 1, 1_000_000), 1);
        assert_eq!(super::frame_workers(18, 1920, 65_535), 1);
    }

    #[test]
    fn causal_conv1d_and_trans_conv1d_streaming_match_offline_bit_for_bit() -> std::io::Result<()> {
        let conv = CausalConv1d {
            out_c: 2,
            in_c: 2,
            kernel: 3,
            dilation: 2,
            stride: 1,
            pad_mode: PadMode::Constant,
            weight: (0..12)
                .map(|i| f32::from(u8::try_from(i + 1).unwrap_or(1)) * 0.05)
                .collect(),
            bias: Some(vec![0.1, -0.2]),
        };
        let input: Vec<f32> = (0..16)
            .map(|i| f32::from(u8::try_from(i).unwrap_or(0)).mul_add(0.1, -0.4))
            .collect();
        let (offline, out_frames) = conv.forward(&input, 8)?;
        assert_eq!(out_frames, 8);

        let mut state = vec![0.0_f32; conv.stream_context_frames() * conv.in_c];
        let mut streamed = Vec::new();
        for chunk in input.chunks(4) {
            streamed.extend(conv.forward_stream(chunk, 2, &mut state)?);
        }
        assert_eq!(offline, streamed);

        let tconv = CausalTransConv1d {
            in_c: 2,
            out_c: 2,
            kernel: 4,
            stride: 2,
            weight: (0..16)
                .map(|i| f32::from(u8::try_from(i + 1).unwrap_or(1)) * 0.03)
                .collect(),
            bias: Some(vec![0.05, -0.05]),
        };
        let (t_offline, t_frames) = tconv.forward(&input, 8)?;
        assert_eq!(t_frames, 16);

        let mut carry = vec![0.0_f32; tconv.carry_frames() * tconv.out_c];
        let mut t_streamed = Vec::new();
        for chunk in input.chunks(4) {
            t_streamed.extend(tconv.forward_stream(chunk, 2, &mut carry)?);
        }
        assert_eq!(t_offline, t_streamed);
        Ok(())
    }

    #[test]
    fn rvq_encode_and_decode_round_trip_nearest_centroids() -> std::io::Result<()> {
        let side = RvqEncodeSide::new(
            4,
            2,
            2,
            vec![1.0, 0.0, 0.0, 1.0],
            vec![vec![
                -1.0, -1.0, // 0
                -1.0, 1.0, // 1
                1.0, -1.0, // 2
                1.0, 1.0, // 3
            ]],
        )?;
        let hidden = vec![0.9, -0.8, -0.95, 0.85];
        let mut codes = Vec::new();
        side.encode(&hidden, 2, &mut codes)?;
        assert_eq!(codes, vec![2, 1]);

        let dec = RvqDecoder {
            semantic: RvqGroup {
                num_codebooks: 1,
                codebook_size: 4,
                codebook_dim: 2,
                hidden_dim: 2,
                codebooks: side.codebooks.clone(),
                out_proj_w: vec![1.0, 0.0, 0.0, 1.0],
            },
            acoustic: RvqGroup {
                num_codebooks: 1,
                codebook_size: 4,
                codebook_dim: 2,
                hidden_dim: 2,
                codebooks: side.codebooks,
                out_proj_w: vec![0.5, 0.0, 0.0, 0.5],
            },
        };
        let decoded = dec.decode(&[2, 0, 1, 3], 2)?;
        assert_eq!(decoded.len(), 4);
        assert!(decoded.iter().all(|v| v.is_finite()));
        Ok(())
    }

    #[test]
    fn codec_state_cache_restores_and_evicts_lru_slot() {
        let mut cache = CodecStateCache::new();
        for i in 0..CODEC_SNAP_SLOTS {
            let codes = [i32::try_from(i).unwrap_or(0); 16];
            let key = codec_ref_key(&codes, 16, 1);
            cache.snapshot(
                key,
                CodecStreamState {
                    pos: i + 1,
                    ..CodecStreamState::default()
                },
            );
        }
        // Touch slot 0 so slot 1 becomes the LRU victim.
        let key0 = codec_ref_key(&[0_i32; 16], 16, 1);
        let key1 = codec_ref_key(&[1_i32; 16], 16, 1);
        assert_eq!(cache.restore(key0).map(|s| s.pos), Some(1));

        let key_new = codec_ref_key(&[99_i32; 16], 16, 1);
        cache.snapshot(
            key_new,
            CodecStreamState {
                pos: 99,
                ..CodecStreamState::default()
            },
        );
        assert_eq!(cache.restore(key0).map(|s| s.pos), Some(1));
        assert!(cache.restore(key1).is_none());
        assert_eq!(cache.restore(key_new).map(|s| s.pos), Some(99));
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "Verifies both 4-block 24 kHz and 5-block 48 kHz DAC decode and 2:1 decimation in one test."
    )]
    fn dac_decoder_supports_24k_and_48k_upsample_blocks_and_decimation() -> std::io::Result<()> {
        let make_res = |ch: usize, dil: usize| -> std::io::Result<DacResUnit> {
            Ok(DacResUnit {
                act1: SnakeBeta::from_log_params(&vec![0.0; ch], &vec![0.0; ch])?,
                conv1: CausalConv1d {
                    out_c: ch,
                    in_c: ch,
                    kernel: 7,
                    dilation: dil,
                    stride: 1,
                    pad_mode: PadMode::Constant,
                    weight: vec![0.01; ch * ch * 7],
                    bias: Some(vec![0.0; ch]),
                },
                act2: SnakeBeta::from_log_params(&vec![0.0; ch], &vec![0.0; ch])?,
                conv2: CausalConv1d {
                    out_c: ch,
                    in_c: ch,
                    kernel: 1,
                    dilation: 1,
                    stride: 1,
                    pad_mode: PadMode::Constant,
                    weight: vec![0.01; ch * ch],
                    bias: Some(vec![0.0; ch]),
                },
            })
        };
        let make_block = |in_c: usize, out_c: usize, stride: usize| -> std::io::Result<DacBlock> {
            Ok(DacBlock {
                snake1: SnakeBeta::from_log_params(&vec![0.0; in_c], &vec![0.0; in_c])?,
                trans_conv: CausalTransConv1d {
                    in_c,
                    out_c,
                    kernel: 2 * stride,
                    stride,
                    weight: vec![0.02; in_c * out_c * 2 * stride],
                    bias: Some(vec![0.0; out_c]),
                },
                res_units: [
                    make_res(out_c, 1)?,
                    make_res(out_c, 3)?,
                    make_res(out_c, 9)?,
                ],
            })
        };
        let blocks_24k = vec![
            make_block(4, 4, 8)?,
            make_block(4, 2, 5)?,
            make_block(2, 2, 4)?,
            make_block(2, 2, 3)?,
        ];
        let mut blocks_48k = blocks_24k.clone();
        blocks_48k.push(make_block(2, 2, 2)?);

        let dec_24k = DacDecoder {
            conv_pre: CausalConv1d {
                out_c: 4,
                in_c: 2,
                kernel: 7,
                dilation: 1,
                stride: 1,
                pad_mode: PadMode::Constant,
                weight: vec![0.02; 4 * 2 * 7],
                bias: Some(vec![0.0; 4]),
            },
            blocks: blocks_24k,
            snake_post: SnakeBeta::from_log_params(&[0.0; 2], &[0.0; 2])?,
            conv_post: CausalConv1d {
                out_c: 1,
                in_c: 2,
                kernel: 7,
                dilation: 1,
                stride: 1,
                pad_mode: PadMode::Constant,
                weight: vec![0.05; 2 * 7],
                bias: Some(vec![0.0]),
            },
        };
        assert_eq!(dec_24k.sample_rate(), SAMPLE_RATE);

        let dec_48k = DacDecoder {
            blocks: blocks_48k,
            ..dec_24k
        };
        assert_eq!(dec_48k.sample_rate(), SAMPLE_RATE_48K);

        let cn = ConvNeXtBlock {
            channels: 2,
            kernel: 3,
            dw_weight: vec![0.1; 6],
            dw_bias: vec![0.0; 2],
            norm_w: vec![1.0; 2],
            norm_b: vec![0.0; 2],
            pwconv1_w: vec![0.05; 16],
            pwconv1_b: vec![0.0; 8],
            pwconv2_w: vec![0.05; 16],
            pwconv2_b: vec![0.0; 2],
            gamma: vec![0.5; 2],
        };
        let mut dw_state = vec![0.0_f32; 4];
        let up_out = cn.forward_stream(&[0.2, -0.1, 0.3, 0.4], 2, &mut dw_state)?;
        assert_eq!(up_out.len(), 4);

        let decimated = decimate_48k_to_24k(&[0.2, 0.6, -0.4, 0.0]);
        assert_eq!(decimated, vec![0.4, -0.2]);
        Ok(())
    }

    #[test]
    fn simd_wide_conv_convnext_and_rvq_preserve_streaming_and_centroid_parity()
    -> std::io::Result<()> {
        let conv = CausalConv1d {
            out_c: 10,
            in_c: 9,
            kernel: 3,
            dilation: 2,
            stride: 1,
            pad_mode: PadMode::Constant,
            weight: (0..(10 * 9 * 3))
                .map(|i| f32::from(u16::try_from((i % 29) + 1).unwrap_or(1)) * 0.01)
                .collect(),
            bias: Some(
                (0..10)
                    .map(|i| f32::from(u8::try_from(i).unwrap_or(0)) * 0.02)
                    .collect(),
            ),
        };
        let input: Vec<f32> = (0..(6 * 9))
            .map(|i| f32::from(u8::try_from(i % 13).unwrap_or(0)).mul_add(0.05, -0.3))
            .collect();
        let (offline, _) = conv.forward(&input, 6)?;
        let mut state = vec![0.0_f32; conv.stream_context_frames() * conv.in_c];
        let mut streamed = Vec::new();
        for chunk in input.chunks(2 * 9) {
            streamed.extend(conv.forward_stream(chunk, 2, &mut state)?);
        }
        assert_eq!(offline, streamed);

        let tconv = CausalTransConv1d {
            in_c: 6,
            out_c: 10,
            kernel: 4,
            stride: 2,
            weight: (0..(6 * 10 * 4))
                .map(|i| f32::from(u16::try_from((i % 19) + 1).unwrap_or(1)) * 0.015)
                .collect(),
            bias: Some(
                (0..10)
                    .map(|i| f32::from(u8::try_from(i).unwrap_or(0)) * -0.01)
                    .collect(),
            ),
        };
        let t_input: Vec<f32> = (0..(4 * 6))
            .map(|i| f32::from(u8::try_from(i % 11).unwrap_or(0)).mul_add(0.07, -0.2))
            .collect();
        let (t_offline, _) = tconv.forward(&t_input, 4)?;
        let mut carry = vec![0.0_f32; tconv.carry_frames() * tconv.out_c];
        let mut t_streamed = Vec::new();
        for chunk in t_input.chunks(2 * 6) {
            t_streamed.extend(tconv.forward_stream(chunk, 2, &mut carry)?);
        }
        assert_eq!(t_offline, t_streamed);

        let c = 10;
        let mid_c = 18;
        let cn = ConvNeXtBlock {
            channels: c,
            kernel: 3,
            dw_weight: vec![0.05; c * 3],
            dw_bias: vec![0.01; c],
            norm_w: vec![1.0; c],
            norm_b: vec![0.0; c],
            pwconv1_w: (0..(mid_c * c))
                .map(|i| f32::from(u16::try_from((i % 17) + 1).unwrap_or(1)) * 0.01)
                .collect(),
            pwconv1_b: vec![0.02; mid_c],
            pwconv2_w: (0..(c * mid_c))
                .map(|i| f32::from(u16::try_from((i % 23) + 1).unwrap_or(1)) * 0.01)
                .collect(),
            pwconv2_b: vec![-0.01; c],
            gamma: vec![0.5; c],
        };
        let cn_in: Vec<f32> = (0..(4 * c))
            .map(|i| f32::from(u8::try_from(i % 9).unwrap_or(0)).mul_add(0.1, -0.4))
            .collect();
        let mut dw_all = vec![0.0_f32; 2 * c];
        let cn_batch = cn.forward_stream(&cn_in, 4, &mut dw_all)?;
        let mut dw_step = vec![0.0_f32; 2 * c];
        let mut cn_stream = Vec::new();
        for chunk in cn_in.chunks(2 * c) {
            cn_stream.extend(cn.forward_stream(chunk, 2, &mut dw_step)?);
        }
        assert_eq!(cn_batch, cn_stream);
        assert_eq!(dw_all, dw_step);
        Ok(())
    }

    #[test]
    fn codec_state_cache_lends_and_primes_lru_slots() -> std::io::Result<()> {
        let mut cache = CodecStateCache::new();
        let make_state = |marker: f32| CodecStreamState {
            pre_conv: vec![marker],
            ..CodecStreamState::default()
        };
        assert_eq!(
            cache.get_or_prime(10, || Ok(make_state(1.0)))?.pre_conv,
            vec![1.0]
        );
        assert_eq!(
            cache
                .get_or_prime(10, || {
                    Err(std::io::Error::other("cached hit must not recompute"))
                })?
                .pre_conv,
            vec![1.0]
        );
        assert_eq!(cache.restore(10).map(|s| s.pre_conv), Some(vec![1.0]));
        Ok(())
    }
}
