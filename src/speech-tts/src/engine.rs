//! Pure-Rust Qwen3-TTS engine (`Talker`, `CodePredictor`, `PromptBuilder`, `LoRA` overlays,
//! and Bonsai-style ternary `PQ2_0`/`PTQ1_0` with Hadamard folding).
//!
//! Implements the pure-Rust execution stack for `Qwen3-TTS-12Hz-1.7B-VoiceDesign` and `Base`:
//! - 28-layer `Talker` transformer (`RMSNorm`, GQA with QK-norm, 3D `MRoPE` / 1D NEOX `RoPE`, `SwiGLU`, KV cache).
//! - Weight formats: `F32`, `Q8_0`, and Bonsai-style group-128 ternary `PQ2_0` / lossless `PTQ1_0`
//!   dual-bitplane repack with block-1024 Sylvester Walsh–Hadamard + sign folding (`prism.hadamard.*`).
//! - Optional low-rank / rank-one `LoRA` adapter overlays (`TalkerLoraAdapter`) with bit-exact
//!   loaded-at-zero (`scale == 0.0`) identity.
//! - 5-layer `CodePredictor` (`mtp_proj`, 15 residual codebooks `1..=15`) with decoupled
//!   `subtalker_*` sampling parameters.
//! - `PromptBuilder` and `PromptCache` with cached `icl_codec_stream` and `icl_ref_text_stream`.
//!
//! Execution is CPU-only. Signed Hexagon dispatch, placement and shared-cache
//! ownership require a separately qualified implementation; no queue protocol
//! or device execution is supplied by this module.
#![forbid(unsafe_code)]
#![expect(
    clippy::missing_errors_doc,
    reason = "Internal tensor, quantization, and layer shape checks return io::ErrorKind::InvalidInput or InvalidData."
)]

use std::{collections::HashMap, io};

use fearless_simd::{Level, dispatch, f32x4, prelude::*};

use crate::{
    codec::codec_ref_key,
    numerics::{axpy_simd, f32_dot_simd, usize_to_f32},
    sampling,
};

mod packed;
pub(crate) mod row_workers;

/// Number of residual acoustic codebooks predicted by `CodePredictor` (`15`).
pub const NUM_ACOUSTIC_CODEBOOKS: usize = 15;
/// Default low-latency streaming chunk size in 12.5 Hz codec frames (`4` frames = `320` ms).
pub const DEFAULT_STREAM_CHUNK_FRAMES: usize = 4;
/// Minimum supported streaming chunk size in 12.5 Hz codec frames (`1` frame = `80` ms).
pub const MIN_STREAM_CHUNK_FRAMES: usize = 1;
/// Maximum supported streaming chunk size in 12.5 Hz codec frames (`32` frames = `2.56` s).
pub const MAX_STREAM_CHUNK_FRAMES: usize = 32;

/// Configurable streaming codec emission schedule (`streaming-qwen3-tts` / `qwen3-tts-rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamChunkConfig {
    /// Number of 12.5 Hz frames in the first emitted audio chunk (`1..=32`, default `4`).
    pub first_chunk_frames: usize,
    /// Number of 12.5 Hz frames in subsequent steady-state chunks (`1..=32`, default `4`).
    pub steady_chunk_frames: usize,
}

impl StreamChunkConfig {
    /// Low-latency 4-frame (`320` ms) streaming schedule halving first-chunk TTFA vs. 8-frame chunking.
    #[must_use]
    pub const fn low_latency_4_frame() -> Self {
        Self {
            first_chunk_frames: DEFAULT_STREAM_CHUNK_FRAMES,
            steady_chunk_frames: DEFAULT_STREAM_CHUNK_FRAMES,
        }
    }

    /// Standard 8-frame (`640` ms) streaming schedule.
    #[must_use]
    pub const fn standard_8_frame() -> Self {
        Self {
            first_chunk_frames: 8,
            steady_chunk_frames: 8,
        }
    }

    /// Validate that both first and steady chunk sizes lie within `MIN_STREAM_CHUNK_FRAMES..=MAX_STREAM_CHUNK_FRAMES`.
    pub fn validate(self) -> io::Result<Self> {
        if self.first_chunk_frames < MIN_STREAM_CHUNK_FRAMES
            || self.first_chunk_frames > MAX_STREAM_CHUNK_FRAMES
            || self.steady_chunk_frames < MIN_STREAM_CHUNK_FRAMES
            || self.steady_chunk_frames > MAX_STREAM_CHUNK_FRAMES
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "StreamChunkConfig frame counts must be within 1..=32.",
            ));
        }
        Ok(self)
    }

    /// Target frame count for emission `chunk_index` (`0` for first chunk, `> 0` for steady state).
    #[must_use]
    pub const fn frames_for_chunk(self, chunk_index: usize) -> usize {
        if chunk_index == 0 {
            self.first_chunk_frames
        } else {
            self.steady_chunk_frames
        }
    }
}
/// Total RVQ codebooks per 12.5 Hz frame (`1` semantic + `15` acoustic = `16`).
pub const TOTAL_CODE_GROUPS: usize = 16;
/// Default Hadamard block size for the 1.7B `Talker` (`1024`, dividing `hidden_size = 2048`).
pub const TALKER_HADAMARD_BLOCK_SIZE: usize = 1024;
/// Default ternary quantization group size (`128`).
pub const TERNARY_GROUP_SIZE: usize = 128;

/// Decoupled sampling parameters for `Talker` (codebook 0) vs. `CodePredictor` (codebooks `1..=15`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DecoupledSamplingParams {
    /// `Talker` semantic temperature (`0.9` for `VoiceDesign`, `0.85` for `Base` synthesis).
    pub temperature: f32,
    /// `Talker` semantic top-k (`50`).
    pub top_k: i32,
    /// `Talker` semantic top-p (`1.0` for `VoiceDesign`, `0.9` for `Base` synthesis).
    pub top_p: f32,
    /// Windowed (`72`-token) repetition penalty (`1.05`).
    pub repetition_penalty: f32,
    /// `CodePredictor` acoustic temperature (`0.9` for `VoiceDesign`, `0.6` for `Base` synthesis).
    pub subtalker_temperature: f32,
    /// `CodePredictor` acoustic top-k (`50`).
    pub subtalker_top_k: i32,
    /// `CodePredictor` acoustic top-p (`1.0` for `VoiceDesign`, `0.9` for `Base` synthesis).
    pub subtalker_top_p: f32,
}

impl DecoupledSamplingParams {
    /// Expressive `VoiceDesign` reference bootstrap sampling profile.
    #[must_use]
    pub const fn voice_design() -> Self {
        Self {
            temperature: 0.9,
            top_k: 50,
            top_p: 1.0,
            repetition_penalty: 1.05,
            subtalker_temperature: 0.9,
            subtalker_top_k: 50,
            subtalker_top_p: 1.0,
        }
    }

    /// High-stability `Base` voice-clone synthesis sampling profile with cooled `CodePredictor`.
    #[must_use]
    pub const fn base_clone() -> Self {
        Self {
            temperature: 0.85,
            top_k: 50,
            top_p: 0.9,
            repetition_penalty: 1.05,
            subtalker_temperature: 0.6,
            subtalker_top_k: 50,
            subtalker_top_p: 0.9,
        }
    }
}

/// One 32-element `Q8_0` quantization block.
#[derive(Clone, Debug, PartialEq)]
pub struct Q8Block {
    /// Per-block floating-point scale.
    pub scale: f32,
    /// 32 signed 8-bit quantized values.
    pub quants: [i8; 32],
}

/// Supported weight representations for `Talker` and `CodePredictor` linear projections.
#[derive(Clone, Debug, PartialEq)]
pub enum WeightQuantization {
    /// Unquantized row-major `F32` weights `[out_dim * in_dim]`.
    F32(Vec<f32>),
    /// Original GGML `F32/F16/Q8_0/Q4_K/Q6_K` blocks, decoded one row at a time.
    Packed {
        /// GGML tensor type (0, 1, 8, 12 or 14).
        ggml_type: u32,
        /// Original little-endian tensor bytes without whole-model expansion.
        bytes: Vec<u8>,
    },
    /// 32-element block-quantized `Q8_0` weights.
    Q8_0 {
        /// Quantized 32-weight blocks.
        blocks: Vec<Q8Block>,
    },
    /// Bonsai-style 2-bit packed ternary (`PQ2_0`, 4 trits per byte: `0 -> -1, 1 -> 0, 2 -> +1`)
    /// with group-128 scales and input-side block Walsh–Hadamard + sign folding.
    PQ2_0 {
        /// Quantization group size (`128`).
        group_size: usize,
        /// Per-group scales `[out_dim * (in_dim / group_size)]`.
        scales: Vec<f32>,
        /// Packed 2-bit trits `[out_dim * (in_dim / 4)]`.
        packed_trits: Vec<u8>,
        /// Walsh–Hadamard transform block size (`1024`, or power-of-two dividing `in_dim`).
        hadamard_block: usize,
        /// Input-side Rademacher signs (`+1` or `-1`) `[in_dim]`.
        signs: Vec<i8>,
    },
    /// Lossless dual-bitplane (`nz_mask`, `sign_mask`) ternary repack (`PTQ1_0`).
    /// This CPU layout is not the signed DSP's tiled `HTP_TYPE_Q2_0` format.
    PTQ1_0 {
        /// Quantization group size (`128`).
        group_size: usize,
        /// Per-group scales `[out_dim * (in_dim / group_size)]`.
        scales: Vec<f32>,
        /// Non-zero bitplane (8 weights per byte; bit `b` is `1` when weight is `!= 0`).
        nz_mask: Vec<u8>,
        /// Sign bitplane (8 weights per byte; bit `b` is `1` when weight is `+1`, `0` when `-1`).
        sign_mask: Vec<u8>,
        /// Walsh–Hadamard transform block size (`1024`, or power-of-two dividing `in_dim`).
        hadamard_block: usize,
        /// Input-side Rademacher signs (`+1` or `-1`) `[in_dim]`.
        signs: Vec<i8>,
    },
}

/// Returns `true` if `tensor_name` is an eligible 28-layer `Talker` projection for
/// `PQ2_0` / `PTQ1_0` ternary compression (`talker.blk.{l}.attn_{q,k,v,output}.weight`
/// or `talker.blk.{l}.ffn_{gate,up,down}.weight`).
#[must_use]
pub fn is_talker_ternary_eligible(tensor_name: &str) -> bool {
    let Some(rest) = tensor_name.strip_prefix("talker.blk.") else {
        return false;
    };
    let Some((layer_str, suffix)) = rest.split_once('.') else {
        return false;
    };
    if layer_str.is_empty() || !layer_str.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    matches!(
        suffix,
        "attn_q.weight"
            | "attn_k.weight"
            | "attn_v.weight"
            | "attn_output.weight"
            | "ffn_gate.weight"
            | "ffn_up.weight"
            | "ffn_down.weight"
    )
}

/// Enforce the Qwen3-TTS mixed-precision ternary policy: never ternarize `CodePredictor`,
/// `codec_head`, `text_embd`, `codec_embd`, `text_proj`, norms, codec, or speaker encoder.
pub fn validate_talker_ternary_policy(
    tensor_name: &str,
    quant: &WeightQuantization,
) -> io::Result<()> {
    if matches!(
        quant,
        WeightQuantization::PQ2_0 { .. } | WeightQuantization::PTQ1_0 { .. }
    ) && !is_talker_ternary_eligible(tensor_name)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Tensor is excluded from PQ2_0/PTQ1_0 ternary quantization by speech policy.",
        ));
    }
    Ok(())
}

/// Losslessly repack `PQ2_0` 2-bit trits into `PTQ1_0` dual bitplanes (`nz_mask`, `sign_mask`).
pub fn repack_pq2_to_ptq1(quant: &WeightQuantization) -> io::Result<WeightQuantization> {
    let WeightQuantization::PQ2_0 {
        group_size,
        scales,
        packed_trits,
        hadamard_block,
        signs,
    } = quant
    else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Expected PQ2_0 weight tensor for PTQ1_0 repack.",
        ));
    };
    if !packed_trits.len().is_multiple_of(2) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "PQ2_0 packed_trits length must be even (8 trits per bitplane byte).",
        ));
    }
    let out_bytes = packed_trits.len() / 2;
    let mut nz_mask = Vec::with_capacity(out_bytes);
    let mut sign_mask = Vec::with_capacity(out_bytes);

    let (pairs, _) = packed_trits.as_chunks::<2>();
    for pair in pairs {
        let mut nz = 0_u8;
        let mut sg = 0_u8;
        for (byte_idx, &b) in pair.iter().enumerate() {
            for sub in 0..4 {
                let code = (b >> (sub * 2)) & 0b11;
                let bit_pos = byte_idx * 4 + sub;
                match code {
                    0 => {
                        // -1
                        nz |= 1_u8 << bit_pos;
                    }
                    1 => {
                        // 0
                    }
                    2 => {
                        // +1
                        nz |= 1_u8 << bit_pos;
                        sg |= 1_u8 << bit_pos;
                    }
                    _ => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "Invalid trit code 3 in PQ2_0 tensor.",
                        ));
                    }
                }
            }
        }
        nz_mask.push(nz);
        sign_mask.push(sg);
    }

    Ok(WeightQuantization::PTQ1_0 {
        group_size: *group_size,
        scales: scales.clone(),
        nz_mask,
        sign_mask,
        hadamard_block: *hadamard_block,
        signs: signs.clone(),
    })
}

#[expect(
    clippy::inline_always,
    reason = "fearless_simd dispatch! requires inline(always) to inherit target_feature attributes."
)]
#[inline(always)]
fn fwht_simd_in_place<S: Simd>(simd: S, data: &mut [f32], norm: f32) {
    let n = data.len();
    let mut len = 1_usize;
    while len < n {
        let step = len * 2;
        if len >= 4 {
            for base in (0..n).step_by(step) {
                let mut i = 0_usize;
                while i + 4 <= len {
                    let a = f32x4::from_slice(simd, &data[base + i..base + i + 4]);
                    let b = f32x4::from_slice(simd, &data[base + i + len..base + i + len + 4]);
                    (a + b).store_slice(&mut data[base + i..base + i + 4]);
                    (a - b).store_slice(&mut data[base + i + len..base + i + len + 4]);
                    i += 4;
                }
            }
        } else {
            for base in (0..n).step_by(step) {
                for i in 0..len {
                    let a = data[base + i];
                    let b = data[base + i + len];
                    data[base + i] = a + b;
                    data[base + i + len] = a - b;
                }
            }
        }
        len = step;
    }
    let norm_v = f32x4::splat(simd, norm);
    let (chunks, tail) = data.as_chunks_mut::<4>();
    for chunk in chunks {
        let v = f32x4::from_slice(simd, chunk);
        (v * norm_v).store_slice(chunk);
    }
    for val in tail {
        *val *= norm;
    }
}

/// In-place normalized Sylvester Walsh–Hadamard transform over a power-of-two slice.
pub fn sylvester_walsh_hadamard_in_place(data: &mut [f32]) -> io::Result<()> {
    let n = data.len();
    if n == 0 || !n.is_power_of_two() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Hadamard block size must be a positive power of two.",
        ));
    }
    let norm = 1.0_f32 / usize_to_f32(n).sqrt();
    let level = Level::new();
    dispatch!(level, simd => fwht_simd_in_place(simd, data, norm));
    Ok(())
}

#[expect(
    clippy::inline_always,
    reason = "fearless_simd dispatch! requires inline(always) to inherit target_feature attributes."
)]
#[inline(always)]
fn sign_flip_simd<S: Simd>(simd: S, input: &[f32], signs: &[i8], out: &mut [f32]) {
    let (in_chunks, in_tail) = input.as_chunks::<4>();
    let (s_chunks, s_tail) = signs.as_chunks::<4>();
    let (out_chunks, out_tail) = out.as_chunks_mut::<4>();
    for ((x_c, s_c), dst_c) in in_chunks
        .iter()
        .zip(s_chunks.iter())
        .zip(out_chunks.iter_mut())
    {
        let xv = f32x4::from_slice(simd, x_c);
        let sv = f32x4::from_slice(
            simd,
            &[
                if s_c[0] < 0 { -1.0_f32 } else { 1.0_f32 },
                if s_c[1] < 0 { -1.0_f32 } else { 1.0_f32 },
                if s_c[2] < 0 { -1.0_f32 } else { 1.0_f32 },
                if s_c[3] < 0 { -1.0_f32 } else { 1.0_f32 },
            ],
        );
        (xv * sv).store_slice(dst_c);
    }
    for ((&x, &s), dst) in in_tail.iter().zip(s_tail.iter()).zip(out_tail.iter_mut()) {
        *dst = if s < 0 { -x } else { x };
    }
}

#[expect(
    clippy::inline_always,
    reason = "fearless_simd dispatch! requires inline(always) to inherit target_feature attributes."
)]
#[inline(always)]
fn q8_block_dot_simd<S: Simd>(simd: S, quants: &[i8; 32], x: &[f32]) -> f32 {
    let (q_chunks, _) = quants.as_chunks::<4>();
    let (x_chunks, _) = x.as_chunks::<4>();
    let mut acc0 = f32x4::splat(simd, 0.0_f32);
    let mut acc1 = f32x4::splat(simd, 0.0_f32);
    for pair in 0..4 {
        let q0 = &q_chunks[pair * 2];
        let q1 = &q_chunks[pair * 2 + 1];
        let x0 = f32x4::from_slice(simd, &x_chunks[pair * 2]);
        let x1 = f32x4::from_slice(simd, &x_chunks[pair * 2 + 1]);
        let qv0 = f32x4::from_slice(
            simd,
            &[
                f32::from(q0[0]),
                f32::from(q0[1]),
                f32::from(q0[2]),
                f32::from(q0[3]),
            ],
        );
        let qv1 = f32x4::from_slice(
            simd,
            &[
                f32::from(q1[0]),
                f32::from(q1[1]),
                f32::from(q1[2]),
                f32::from(q1[3]),
            ],
        );
        acc0 = qv0.mul_add(x0, acc0);
        acc1 = qv1.mul_add(x1, acc1);
    }
    let sum_v = acc0 + acc1;
    let mut lanes = [0.0_f32; 4];
    sum_v.store_slice(&mut lanes);
    (lanes[0] + lanes[1]) + (lanes[2] + lanes[3])
}

const PQ2_BYTE_TRITS_F32: [[f32; 4]; 256] = {
    let mut table = [[0.0_f32; 4]; 256];
    let mut b = 0_usize;
    while b < 256 {
        let mut sub = 0_usize;
        while sub < 4 {
            let code = (b >> (sub * 2)) & 0b11;
            table[b][sub] = match code {
                0 => -1.0_f32,
                2 => 1.0_f32,
                _ => 0.0_f32,
            };
            sub += 1;
        }
        b += 1;
    }
    table
};

const PTQ1_NIBBLE_TRITS_F32: [[f32; 4]; 256] = {
    let mut table = [[0.0_f32; 4]; 256];
    let mut idx = 0_usize;
    while idx < 256 {
        let nz = idx & 0x0F;
        let sg = (idx >> 4) & 0x0F;
        let mut bit_i = 0_usize;
        while bit_i < 4 {
            if (nz >> bit_i) & 1 != 0 {
                table[idx][bit_i] = if (sg >> bit_i) & 1 != 0 {
                    1.0_f32
                } else {
                    -1.0_f32
                };
            }
            bit_i += 1;
        }
        idx += 1;
    }
    table
};

#[expect(
    clippy::inline_always,
    reason = "fearless_simd dispatch! requires inline(always) to inherit target_feature attributes."
)]
#[inline(always)]
fn pq2_group_dot_simd<S: Simd>(simd: S, byte_slice: &[u8], x_group: &[f32]) -> f32 {
    let (x_quads, _) = x_group.as_chunks::<4>();
    let (b_pairs, b_rem) = byte_slice.as_chunks::<2>();
    let (x_pairs, x_rem) = x_quads.as_chunks::<2>();
    let mut acc0 = f32x4::splat(simd, 0.0_f32);
    let mut acc1 = f32x4::splat(simd, 0.0_f32);
    for (bp, xp) in b_pairs.iter().zip(x_pairs.iter()) {
        let w0 = f32x4::from_slice(simd, &PQ2_BYTE_TRITS_F32[usize::from(bp[0])]);
        let w1 = f32x4::from_slice(simd, &PQ2_BYTE_TRITS_F32[usize::from(bp[1])]);
        let x0 = f32x4::from_slice(simd, &xp[0]);
        let x1 = f32x4::from_slice(simd, &xp[1]);
        acc0 = w0.mul_add(x0, acc0);
        acc1 = w1.mul_add(x1, acc1);
    }
    for (&b, xq) in b_rem.iter().zip(x_rem.iter()) {
        let w = f32x4::from_slice(simd, &PQ2_BYTE_TRITS_F32[usize::from(b)]);
        let x = f32x4::from_slice(simd, xq);
        acc0 = w.mul_add(x, acc0);
    }
    let sum_v = acc0 + acc1;
    let mut lanes = [0.0_f32; 4];
    sum_v.store_slice(&mut lanes);
    (lanes[0] + lanes[1]) + (lanes[2] + lanes[3])
}

#[expect(
    clippy::inline_always,
    reason = "fearless_simd dispatch! requires inline(always) to inherit target_feature attributes."
)]
#[inline(always)]
fn ptq1_group_dot_simd<S: Simd>(simd: S, nz_slice: &[u8], sg_slice: &[u8], x_group: &[f32]) -> f32 {
    let (x_octs, _) = x_group.as_chunks::<8>();
    let mut acc0 = f32x4::splat(simd, 0.0_f32);
    let mut acc1 = f32x4::splat(simd, 0.0_f32);
    for ((&nz_b, &sg_b), xo) in nz_slice.iter().zip(sg_slice.iter()).zip(x_octs.iter()) {
        if nz_b == 0 {
            continue;
        }
        let lo_idx = usize::from((nz_b & 0x0F) | ((sg_b & 0x0F) << 4));
        let hi_idx = usize::from((nz_b >> 4) | (sg_b & 0xF0));
        let w0 = f32x4::from_slice(simd, &PTQ1_NIBBLE_TRITS_F32[lo_idx]);
        let w1 = f32x4::from_slice(simd, &PTQ1_NIBBLE_TRITS_F32[hi_idx]);
        let x0 = f32x4::from_slice(simd, &xo[0..4]);
        let x1 = f32x4::from_slice(simd, &xo[4..8]);
        acc0 = w0.mul_add(x0, acc0);
        acc1 = w1.mul_add(x1, acc1);
    }
    let sum_v = acc0 + acc1;
    let mut lanes = [0.0_f32; 4];
    sum_v.store_slice(&mut lanes);
    (lanes[0] + lanes[1]) + (lanes[2] + lanes[3])
}

/// Apply input-side Rademacher sign folding (`x[i] *= signs[i]`) followed by block-wise
/// normalized Sylvester Walsh–Hadamard transform (`prism.hadamard.*`).
pub fn apply_hadamard_sign_fold(
    input: &[f32],
    signs: &[i8],
    block_size: usize,
) -> io::Result<Vec<f32>> {
    if input.is_empty()
        || input.len() != signs.len()
        || block_size == 0
        || !block_size.is_power_of_two()
        || !input.len().is_multiple_of(block_size)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid Hadamard sign-fold dimensions.",
        ));
    }
    let mut folded = vec![0.0_f32; input.len()];
    let norm = 1.0_f32 / usize_to_f32(block_size).sqrt();
    let level = Level::new();
    dispatch!(level, simd => {
        sign_flip_simd(simd, input, signs, &mut folded);
        for block in folded.chunks_exact_mut(block_size) {
            fwht_simd_in_place(simd, block, norm);
        }
    });
    Ok(folded)
}

/// Optional rank-one or low-rank `LoRA` adapter patch for a single linear projection.
#[derive(Clone, Debug)]
pub struct LoraProjectionPatch {
    /// Input feature dimension.
    pub in_dim: usize,
    /// Output feature dimension.
    pub out_dim: usize,
    /// Adapter rank (`>= 1`).
    pub rank: usize,
    /// `LoRA` scaling `alpha / rank`.
    pub alpha_over_rank: f32,
    /// Runtime adapter strength (`0.0` guarantees bit-exact base identity).
    pub scale: f32,
    /// Down-projection `[rank * in_dim]`.
    pub lora_a: Vec<f32>,
    /// Up-projection `[out_dim * rank]`.
    pub lora_b: Vec<f32>,
}

impl LoraProjectionPatch {
    /// Add `scale * alpha_over_rank * (lora_b @ (lora_a @ x))` onto `output` (`[out_dim]`).
    /// When `scale == 0.0`, returns immediately without touching `output` (0-ULP identity).
    pub fn apply(&self, input: &[f32], output: &mut [f32]) -> io::Result<()> {
        if input.len() != self.in_dim
            || output.len() != self.out_dim
            || self.lora_a.len() != self.rank * self.in_dim
            || self.lora_b.len() != self.out_dim * self.rank
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid LoRA projection patch dimensions.",
            ));
        }
        if self.scale == 0.0_f32 {
            return Ok(());
        }
        let eff_scale = self.scale * self.alpha_over_rank;
        let mut mid = vec![0.0_f32; self.rank];
        let level = Level::new();
        dispatch!(level, simd => {
            for (r, mid_slot) in mid.iter_mut().enumerate() {
                let a_row = &self.lora_a[r * self.in_dim..(r + 1) * self.in_dim];
                *mid_slot = f32_dot_simd(simd, a_row, input) * eff_scale;
            }
            for (o, out_slot) in output.iter_mut().enumerate() {
                let b_row = &self.lora_b[o * self.rank..(o + 1) * self.rank];
                *out_slot += f32_dot_simd(simd, b_row, &mid);
            }
        });
        Ok(())
    }
}

/// Named collection of `LoRA` projection overlays attached to a shared `Base` `Talker`
/// (e.g., regional-accent or language adapters such as `aguken-ai/qwen3-tts-indic-finetuned`).
#[derive(Clone, Debug, Default)]
pub struct TalkerLoraAdapter {
    /// Adapter profile identifier.
    pub name: String,
    /// Map from GGUF tensor name (e.g. `"talker.blk.0.attn_q.weight"`) to its `LoRA` patch.
    pub patches: HashMap<String, LoraProjectionPatch>,
}

/// Matrix-vector / matrix-matrix linear projection supporting `F32`, `Q8_0`, `PQ2_0`, `PTQ1_0`,
/// and optional `LoraProjectionPatch` overlays.
#[derive(Clone, Debug)]
pub struct LinearProjection {
    /// Tensor name in the GGUF artifact.
    pub name: String,
    /// Input feature dimension.
    pub in_dim: usize,
    /// Output feature dimension.
    pub out_dim: usize,
    /// Quantized or full-precision weight storage.
    pub weight: WeightQuantization,
    /// Optional bias `[out_dim]`.
    pub bias: Option<Vec<f32>>,
}

impl LinearProjection {
    /// Construct a validated linear projection, checking the speech ternary policy.
    pub fn new(
        name: impl Into<String>,
        in_dim: usize,
        out_dim: usize,
        weight: WeightQuantization,
        bias: Option<Vec<f32>>,
    ) -> io::Result<Self> {
        let name = name.into();
        validate_talker_ternary_policy(&name, &weight)?;
        if in_dim == 0 || out_dim == 0 || bias.as_ref().is_some_and(|b| b.len() != out_dim) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid LinearProjection dimensions.",
            ));
        }
        if let WeightQuantization::Packed { ggml_type, bytes } = &weight {
            let (block, size) = crate::weights::packed_layout(*ggml_type)?;
            if !in_dim.is_multiple_of(block)
                || (in_dim / block)
                    .checked_mul(out_dim)
                    .and_then(|count| count.checked_mul(size))
                    != Some(bytes.len())
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Invalid packed projection size.",
                ));
            }
            crate::weights::validate_packed(*ggml_type, bytes)?;
        }
        Ok(Self {
            name,
            in_dim,
            out_dim,
            weight,
            bias,
        })
    }

    /// Decode one embedding row without expanding the complete matrix.
    ///
    /// # Errors
    /// Rejects an out-of-range row, malformed weights or a transformed ternary
    /// projection, which cannot represent an ordinary embedding lookup.
    pub fn row(&self, index: usize) -> io::Result<Vec<f32>> {
        if index >= self.out_dim {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Embedding row is out of range.",
            ));
        }
        let invalid = || io::Error::new(io::ErrorKind::InvalidData, "Invalid embedding weights.");
        let offset = index.checked_mul(self.in_dim).ok_or_else(invalid)?;
        let end = offset.checked_add(self.in_dim).ok_or_else(invalid)?;
        match &self.weight {
            WeightQuantization::F32(values) => values
                .get(offset..end)
                .map(<[f32]>::to_vec)
                .ok_or_else(invalid),
            WeightQuantization::Packed { ggml_type, bytes } => {
                let (block, size) = crate::weights::packed_layout(*ggml_type)?;
                if !self.in_dim.is_multiple_of(block) {
                    return Err(invalid());
                }
                let start = (offset / block).checked_mul(size).ok_or_else(invalid)?;
                let stop = (end / block).checked_mul(size).ok_or_else(invalid)?;
                let row = bytes.get(start..stop).ok_or_else(invalid)?;
                let mut output = vec![0.0; self.in_dim];
                for (source, destination) in
                    row.chunks_exact(size).zip(output.chunks_exact_mut(block))
                {
                    crate::weights::decode_block(*ggml_type, source, destination);
                }
                Ok(output)
            }
            WeightQuantization::Q8_0 { blocks } => {
                if !self.in_dim.is_multiple_of(32) {
                    return Err(invalid());
                }
                let row = blocks.get(offset / 32..end / 32).ok_or_else(invalid)?;
                Ok(row
                    .iter()
                    .flat_map(|block| {
                        block
                            .quants
                            .iter()
                            .map(move |value| block.scale * f32::from(*value))
                    })
                    .collect())
            }
            WeightQuantization::PQ2_0 { .. } | WeightQuantization::PTQ1_0 { .. } => Err(invalid()),
        }
    }

    /// Return the input-side `(signs, hadamard_block)` signature when this projection uses
    /// `PQ2_0` or `PTQ1_0` block Walsh–Hadamard folding.
    #[must_use]
    pub const fn hadamard_signature(&self) -> Option<(&[i8], usize)> {
        match &self.weight {
            WeightQuantization::PQ2_0 {
                hadamard_block,
                signs,
                ..
            }
            | WeightQuantization::PTQ1_0 {
                hadamard_block,
                signs,
                ..
            } => Some((signs.as_slice(), *hadamard_block)),
            WeightQuantization::F32(_)
            | WeightQuantization::Q8_0 { .. }
            | WeightQuantization::Packed { .. } => None,
        }
    }

    /// Project one vector `input` (`[in_dim]`) into `[out_dim]`, optionally applying `lora`.
    pub fn forward_vec(
        &self,
        input: &[f32],
        lora: Option<&LoraProjectionPatch>,
    ) -> io::Result<Vec<f32>> {
        self.forward_vec_parallel(input, lora, 1)
    }

    /// Project one vector `input` (`[in_dim]`) into `[out_dim]` using `fearless_simd` NEON
    /// and up to `threads` reusable row workers.
    pub fn forward_vec_parallel(
        &self,
        input: &[f32],
        lora: Option<&LoraProjectionPatch>,
        threads: usize,
    ) -> io::Result<Vec<f32>> {
        self.forward_vec_with_pre_folded(input, None, lora, threads)
    }

    /// Project one vector `input` (`[in_dim]`) into `[out_dim]`, optionally reusing a pre-folded
    /// Hadamard activation slice (`pre_folded`) shared across sibling projections (`q/k/v` or `gate/up`).
    #[expect(
        clippy::too_many_lines,
        reason = "Dispatches across F32, Q8_0, PQ2_0, and PTQ1_0 weight representations with optional reusable row parallelism."
    )]
    pub fn forward_vec_with_pre_folded(
        &self,
        input: &[f32],
        pre_folded: Option<&[f32]>,
        lora: Option<&LoraProjectionPatch>,
        threads: usize,
    ) -> io::Result<Vec<f32>> {
        if input.len() != self.in_dim {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "LinearProjection input length mismatch.",
            ));
        }
        let mut out = self
            .bias
            .clone()
            .unwrap_or_else(|| vec![0.0_f32; self.out_dim]);
        let worker_count = threads.clamp(1, 18).min(self.out_dim.max(1));
        let level = Level::new();

        match &self.weight {
            WeightQuantization::Packed { ggml_type, bytes } => {
                let (block, size) = crate::weights::packed_layout(*ggml_type)?;
                let row_bytes = (self.in_dim / block).checked_mul(size).ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Invalid packed projection size.",
                    )
                })?;
                if !self.in_dim.is_multiple_of(block)
                    || row_bytes.checked_mul(self.out_dim) != Some(bytes.len())
                {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Invalid packed projection size.",
                    ));
                }
                let compute_slice = |row_offset: usize, destination: &mut [f32]| {
                    dispatch!(level, simd => {
                        let offset = row_offset * row_bytes;
                        packed::rows(simd, *ggml_type, &bytes[offset..], input, destination);
                    });
                };
                run_row_chunks(&mut out, worker_count, compute_slice);
            }
            WeightQuantization::F32(w) => {
                if w.len() != self.out_dim * self.in_dim {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "F32 weight buffer size mismatch.",
                    ));
                }
                let in_dim = self.in_dim;
                let compute_slice = |row_offset: usize, dst_slice: &mut [f32]| {
                    dispatch!(level, simd => {
                        for (local_o, dst) in dst_slice.iter_mut().enumerate() {
                            let o = row_offset + local_o;
                            let row = &w[o * in_dim..(o + 1) * in_dim];
                            *dst += f32_dot_simd(simd, row, input);
                        }
                    });
                };
                run_row_chunks(&mut out, worker_count, compute_slice);
            }
            WeightQuantization::Q8_0 { blocks } => {
                let blocks_per_row = self.in_dim / 32;
                if !self.in_dim.is_multiple_of(32) || blocks.len() != self.out_dim * blocks_per_row
                {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Q8_0 block count mismatch.",
                    ));
                }
                let compute_slice = |row_offset: usize, dst_slice: &mut [f32]| {
                    dispatch!(level, simd => {
                        for (local_o, dst) in dst_slice.iter_mut().enumerate() {
                            let o = row_offset + local_o;
                            let row_blocks = &blocks[o * blocks_per_row..(o + 1) * blocks_per_row];
                            let mut acc = *dst;
                            for (bi, blk) in row_blocks.iter().enumerate() {
                                let x_slice = &input[bi * 32..(bi + 1) * 32];
                                let b_sum = q8_block_dot_simd(simd, &blk.quants, x_slice);
                                acc = blk.scale.mul_add(b_sum, acc);
                            }
                            *dst = acc;
                        }
                    });
                };
                run_row_chunks(&mut out, worker_count, compute_slice);
            }
            WeightQuantization::PQ2_0 {
                group_size,
                scales,
                packed_trits,
                hadamard_block,
                signs,
            } => {
                let owned_folded;
                let x_folded: &[f32] = if let Some(pf) = pre_folded
                    && pf.len() == self.in_dim
                {
                    pf
                } else {
                    owned_folded = apply_hadamard_sign_fold(input, signs, *hadamard_block)?;
                    &owned_folded
                };
                let groups_per_row = self.in_dim / *group_size;
                let bytes_per_row = self.in_dim / 4;
                if *group_size == 0
                    || !group_size.is_multiple_of(4)
                    || !self.in_dim.is_multiple_of(*group_size)
                    || !self.in_dim.is_multiple_of(4)
                    || scales.len() != self.out_dim * groups_per_row
                    || packed_trits.len() != self.out_dim * bytes_per_row
                {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "PQ2_0 buffer dimensions mismatch.",
                    ));
                }
                let gsize = *group_size;
                let gbytes = gsize / 4;
                let compute_slice = |row_offset: usize, dst_slice: &mut [f32]| {
                    dispatch!(level, simd => {
                        for (local_o, dst) in dst_slice.iter_mut().enumerate() {
                            let o = row_offset + local_o;
                            let row_scales = &scales[o * groups_per_row..(o + 1) * groups_per_row];
                            let row_bytes = &packed_trits[o * bytes_per_row..(o + 1) * bytes_per_row];
                            let mut acc = *dst;
                            for (gi, &scale) in row_scales.iter().enumerate() {
                                let start_col = gi * gsize;
                                let byte_slice = &row_bytes[gi * gbytes..(gi + 1) * gbytes];
                                let x_group = &x_folded[start_col..start_col + gsize];
                                let g_sum = pq2_group_dot_simd(simd, byte_slice, x_group);
                                acc = scale.mul_add(g_sum, acc);
                            }
                            *dst = acc;
                        }
                    });
                };
                run_row_chunks(&mut out, worker_count, compute_slice);
            }
            WeightQuantization::PTQ1_0 {
                group_size,
                scales,
                nz_mask,
                sign_mask,
                hadamard_block,
                signs,
            } => {
                let owned_folded;
                let x_folded: &[f32] = if let Some(pf) = pre_folded
                    && pf.len() == self.in_dim
                {
                    pf
                } else {
                    owned_folded = apply_hadamard_sign_fold(input, signs, *hadamard_block)?;
                    &owned_folded
                };
                let groups_per_row = self.in_dim / *group_size;
                let bytes_per_row = self.in_dim / 8;
                if *group_size == 0
                    || !group_size.is_multiple_of(8)
                    || !self.in_dim.is_multiple_of(*group_size)
                    || !self.in_dim.is_multiple_of(8)
                    || scales.len() != self.out_dim * groups_per_row
                    || nz_mask.len() != self.out_dim * bytes_per_row
                    || sign_mask.len() != self.out_dim * bytes_per_row
                {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "PTQ1_0 buffer dimensions mismatch.",
                    ));
                }
                let gsize = *group_size;
                let gbytes = gsize / 8;
                let compute_slice = |row_offset: usize, dst_slice: &mut [f32]| {
                    dispatch!(level, simd => {
                        for (local_o, dst) in dst_slice.iter_mut().enumerate() {
                            let o = row_offset + local_o;
                            let row_scales = &scales[o * groups_per_row..(o + 1) * groups_per_row];
                            let row_nz = &nz_mask[o * bytes_per_row..(o + 1) * bytes_per_row];
                            let row_sg = &sign_mask[o * bytes_per_row..(o + 1) * bytes_per_row];
                            let mut acc = *dst;
                            for (gi, &scale) in row_scales.iter().enumerate() {
                                let start_col = gi * gsize;
                                let nz_slice = &row_nz[gi * gbytes..(gi + 1) * gbytes];
                                let sg_slice = &row_sg[gi * gbytes..(gi + 1) * gbytes];
                                let x_group = &x_folded[start_col..start_col + gsize];
                                let g_sum = ptq1_group_dot_simd(simd, nz_slice, sg_slice, x_group);
                                acc = scale.mul_add(g_sum, acc);
                            }
                            *dst = acc;
                        }
                    });
                };
                run_row_chunks(&mut out, worker_count, compute_slice);
            }
        }

        if let Some(patch) = lora {
            patch.apply(input, &mut out)?;
        }
        Ok(out)
    }
}

fn run_row_chunks<F>(out: &mut [f32], worker_count: usize, compute_slice: F)
where
    F: Fn(usize, &mut [f32]) + Sync,
{
    row_workers::run(out, worker_count, &compute_slice);
}

#[expect(
    clippy::inline_always,
    reason = "fearless_simd dispatch! requires inline(always) to inherit target_feature attributes."
)]
#[inline(always)]
fn rms_norm_scale_simd<S: Simd>(
    simd: S,
    input: &[f32],
    weight: &[f32],
    scale: f32,
    out: &mut [f32],
) {
    let scale_v = f32x4::splat(simd, scale);
    let (in_chunks, in_tail) = input.as_chunks::<4>();
    let (w_chunks, w_tail) = weight.as_chunks::<4>();
    let (out_chunks, out_tail) = out.as_chunks_mut::<4>();
    for ((x_c, w_c), dst_c) in in_chunks
        .iter()
        .zip(w_chunks.iter())
        .zip(out_chunks.iter_mut())
    {
        let xv = f32x4::from_slice(simd, x_c);
        let wv = f32x4::from_slice(simd, w_c);
        ((xv * scale_v) * wv).store_slice(dst_c);
    }
    for ((&x, &w), dst) in in_tail.iter().zip(w_tail.iter()).zip(out_tail.iter_mut()) {
        *dst = (x * scale) * w;
    }
}

#[expect(
    clippy::inline_always,
    reason = "fearless_simd dispatch! requires inline(always) to inherit target_feature attributes."
)]
#[inline(always)]
fn rms_norm_in_place_simd<S: Simd>(simd: S, data: &mut [f32], weight: &[f32], scale: f32) {
    let scale_v = f32x4::splat(simd, scale);
    let (d_chunks, d_tail) = data.as_chunks_mut::<4>();
    let (w_chunks, w_tail) = weight.as_chunks::<4>();
    for (d_c, w_c) in d_chunks.iter_mut().zip(w_chunks.iter()) {
        let xv = f32x4::from_slice(simd, d_c);
        let wv = f32x4::from_slice(simd, w_c);
        ((xv * scale_v) * wv).store_slice(d_c);
    }
    for (x, &w) in d_tail.iter_mut().zip(w_tail.iter()) {
        *x = (*x * scale) * w;
    }
}

/// RMS normalization (`x * weight / sqrt(mean(x^2) + eps)`).
pub fn rms_norm(input: &[f32], weight: &[f32], eps: f32) -> io::Result<Vec<f32>> {
    if input.is_empty() || input.len() != weight.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "RMSNorm dimension mismatch.",
        ));
    }
    let inv_n = 1.0_f32 / usize_to_f32(input.len());
    let mean_sq = input.iter().map(|&v| v * v).sum::<f32>() * inv_n;
    let scale = 1.0_f32 / (mean_sq + eps).sqrt();
    let mut out = vec![0.0_f32; input.len()];
    let level = Level::new();
    dispatch!(level, simd => rms_norm_scale_simd(simd, input, weight, scale, &mut out));
    Ok(out)
}

/// In-place RMS normalization (`data[i] = (data[i] * scale) * weight[i]`), avoiding per-head heap allocations.
pub fn rms_norm_in_place(data: &mut [f32], weight: &[f32], eps: f32) -> io::Result<()> {
    if data.is_empty() || data.len() != weight.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "RMSNorm dimension mismatch.",
        ));
    }
    let inv_n = 1.0_f32 / usize_to_f32(data.len());
    let mean_sq = data.iter().map(|&v| v * v).sum::<f32>() * inv_n;
    let scale = 1.0_f32 / (mean_sq + eps).sqrt();
    let level = Level::new();
    dispatch!(level, simd => rms_norm_in_place_simd(simd, data, weight, scale));
    Ok(())
}

#[expect(
    clippy::inline_always,
    reason = "fearless_simd dispatch! requires inline(always) to inherit target_feature attributes."
)]
#[inline(always)]
fn rope_head_simd<S: Simd>(simd: S, lo: &mut [f32], hi: &mut [f32], cos: &[f32], sin: &[f32]) {
    let (lo_chunks, lo_tail) = lo.as_chunks_mut::<4>();
    let (hi_chunks, hi_tail) = hi.as_chunks_mut::<4>();
    let (cos_chunks, cos_tail) = cos.as_chunks::<4>();
    let (sin_chunks, sin_tail) = sin.as_chunks::<4>();
    for (((lo_c, hi_c), cos_c), sin_c) in lo_chunks
        .iter_mut()
        .zip(hi_chunks.iter_mut())
        .zip(cos_chunks.iter())
        .zip(sin_chunks.iter())
    {
        let x0 = f32x4::from_slice(simd, lo_c);
        let x1 = f32x4::from_slice(simd, hi_c);
        let cv = f32x4::from_slice(simd, cos_c);
        let sv = f32x4::from_slice(simd, sin_c);
        x0.mul_add(cv, -(x1 * sv)).store_slice(lo_c);
        x0.mul_add(sv, x1 * cv).store_slice(hi_c);
    }
    for (((x0_ref, x1_ref), &cos_v), &sin_v) in lo_tail
        .iter_mut()
        .zip(hi_tail.iter_mut())
        .zip(cos_tail.iter())
        .zip(sin_tail.iter())
    {
        let x0 = *x0_ref;
        let x1 = *x1_ref;
        *x0_ref = x0.mul_add(cos_v, -x1 * sin_v);
        *x1_ref = x0.mul_add(sin_v, x1 * cos_v);
    }
}

/// Apply 1D NEOX-style half-split `RoPE` (or collapsed 3D `MRoPE` when all 3 axes share `pos`)
/// in place over `[num_heads, head_dim]`, precomputing `(sin, cos)` once across all heads.
pub fn apply_rope_neox_in_place(
    heads: &mut [f32],
    num_heads: usize,
    head_dim: usize,
    pos: usize,
    theta: f32,
) -> io::Result<()> {
    if head_dim == 0 || !head_dim.is_multiple_of(2) || heads.len() != num_heads * head_dim {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid RoPE head dimensions.",
        ));
    }
    let half = head_dim / 2;
    let hd_f = usize_to_f32(head_dim);
    let pos_f = usize_to_f32(pos);
    let mut cos_table = Vec::with_capacity(half);
    let mut sin_table = Vec::with_capacity(half);
    for i in 0..half {
        let exponent = usize_to_f32(2 * i) / hd_f;
        let freq = pos_f / theta.powf(exponent);
        let (sin_v, cos_v) = freq.sin_cos();
        cos_table.push(cos_v);
        sin_table.push(sin_v);
    }

    let level = Level::new();
    dispatch!(level, simd => {
        for h in 0..num_heads {
            let slice = &mut heads[h * head_dim..(h + 1) * head_dim];
            let (lo, hi) = slice.split_at_mut(half);
            rope_head_simd(simd, lo, hi, &cos_table, &sin_table);
        }
    });
    Ok(())
}

/// One decoder layer shared by `Talker` (28 layers) and `CodePredictor` (5 layers).
#[derive(Clone, Debug)]
pub struct QwenDecoderLayer {
    /// Pre-attention `RMSNorm` scale `[hidden_size]`.
    pub attn_norm_w: Vec<f32>,
    /// Query projection `[num_q_heads * head_dim, hidden_size]`.
    pub q_proj: LinearProjection,
    /// Key projection `[num_kv_heads * head_dim, hidden_size]`.
    pub k_proj: LinearProjection,
    /// Value projection `[num_kv_heads * head_dim, hidden_size]`.
    pub v_proj: LinearProjection,
    /// Output projection `[hidden_size, num_q_heads * head_dim]`.
    pub o_proj: LinearProjection,
    /// Per-head query `RMSNorm` `[head_dim]`.
    pub q_norm_w: Vec<f32>,
    /// Per-head key `RMSNorm` `[head_dim]`.
    pub k_norm_w: Vec<f32>,
    /// Post-attention `RMSNorm` scale `[hidden_size]`.
    pub ffn_norm_w: Vec<f32>,
    /// `SwiGLU` gate projection `[intermediate_size, hidden_size]`.
    pub gate_proj: LinearProjection,
    /// `SwiGLU` up projection `[intermediate_size, hidden_size]`.
    pub up_proj: LinearProjection,
    /// `SwiGLU` down projection `[hidden_size, intermediate_size]`.
    pub down_proj: LinearProjection,
}

/// Per-layer KV cache storing past key/value vectors (`[seq_len, num_kv_heads * head_dim]`).
#[derive(Clone, Debug, Default)]
pub struct LayerKvCache {
    /// Cached keys `[seq_len * num_kv_heads * head_dim]`.
    pub k: Vec<f32>,
    /// Cached values `[seq_len * num_kv_heads * head_dim]`.
    pub v: Vec<f32>,
    /// Number of cached token positions.
    pub seq_len: usize,
}

impl QwenDecoderLayer {
    /// Run a single-token step (`pos = kv.seq_len`), appending K/V into `kv` and applying optional `LoRA` patches.
    #[expect(
        clippy::too_many_arguments,
        reason = "Passes explicit GQA geometry, RoPE/RMSNorm scalars, KV cache, and optional LoRA adapter."
    )]
    pub fn forward_step(
        &self,
        input: &[f32],
        num_q_heads: usize,
        num_kv_heads: usize,
        head_dim: usize,
        rope_theta: f32,
        rms_eps: f32,
        kv: &mut LayerKvCache,
        adapter: Option<&TalkerLoraAdapter>,
    ) -> io::Result<Vec<f32>> {
        self.forward_step_parallel(
            input,
            num_q_heads,
            num_kv_heads,
            head_dim,
            rope_theta,
            rms_eps,
            kv,
            adapter,
            1,
        )
    }

    /// Run one decoder step with an explicit bounded CPU worker count.
    #[expect(
        clippy::too_many_arguments,
        clippy::too_many_lines,
        reason = "Keep GQA geometry, cache ownership and compute budget explicit."
    )]
    pub fn forward_step_parallel(
        &self,
        input: &[f32],
        num_q_heads: usize,
        num_kv_heads: usize,
        head_dim: usize,
        rope_theta: f32,
        rms_eps: f32,
        kv: &mut LayerKvCache,
        adapter: Option<&TalkerLoraAdapter>,
        workers: usize,
    ) -> io::Result<Vec<f32>> {
        if !(1..=256).contains(&workers) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid CPU worker count.",
            ));
        }
        if num_q_heads == 0 || num_kv_heads == 0 || !num_q_heads.is_multiple_of(num_kv_heads) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid GQA head configuration.",
            ));
        }
        let h_norm = rms_norm(input, &self.attn_norm_w, rms_eps)?;
        let patch = |proj: &LinearProjection| adapter.and_then(|a| a.patches.get(&proj.name));

        let q_sig = self.q_proj.hadamard_signature();
        let k_sig = self.k_proj.hadamard_signature();
        let v_sig = self.v_proj.hadamard_signature();
        let shared_qkv_folded = if let Some((signs, block)) = q_sig
            && (k_sig == q_sig || v_sig == q_sig)
        {
            Some((
                signs,
                block,
                apply_hadamard_sign_fold(&h_norm, signs, block)?,
            ))
        } else {
            None
        };
        let qkv_fold_for = |sig: Option<(&[i8], usize)>| -> Option<&[f32]> {
            shared_qkv_folded.as_ref().and_then(|(s, b, buf)| {
                if sig == Some((*s, *b)) {
                    Some(buf.as_slice())
                } else {
                    None
                }
            })
        };

        let mut q = self.q_proj.forward_vec_with_pre_folded(
            &h_norm,
            qkv_fold_for(q_sig),
            patch(&self.q_proj),
            workers,
        )?;
        let mut k = self.k_proj.forward_vec_with_pre_folded(
            &h_norm,
            qkv_fold_for(k_sig),
            patch(&self.k_proj),
            workers,
        )?;
        let v = self.v_proj.forward_vec_with_pre_folded(
            &h_norm,
            qkv_fold_for(v_sig),
            patch(&self.v_proj),
            workers,
        )?;

        for h in 0..num_q_heads {
            rms_norm_in_place(
                &mut q[h * head_dim..(h + 1) * head_dim],
                &self.q_norm_w,
                rms_eps,
            )?;
        }
        for h in 0..num_kv_heads {
            rms_norm_in_place(
                &mut k[h * head_dim..(h + 1) * head_dim],
                &self.k_norm_w,
                rms_eps,
            )?;
        }

        let pos = kv.seq_len;
        apply_rope_neox_in_place(&mut q, num_q_heads, head_dim, pos, rope_theta)?;
        apply_rope_neox_in_place(&mut k, num_kv_heads, head_dim, pos, rope_theta)?;

        kv.k.extend_from_slice(&k);
        kv.v.extend_from_slice(&v);
        kv.seq_len += 1;

        let total_seq = kv.seq_len;
        let kv_stride = num_kv_heads * head_dim;
        let gqa_ratio = num_q_heads / num_kv_heads;
        let scale = 1.0_f32 / usize_to_f32(head_dim).sqrt();

        let mut attn_out = vec![0.0_f32; num_q_heads * head_dim];
        let mut scores = vec![0.0_f32; total_seq];
        let level = Level::new();

        dispatch!(level, simd => {
            for qh in 0..num_q_heads {
                let kvh = qh / gqa_ratio;
                let q_vec = &q[qh * head_dim..(qh + 1) * head_dim];
                let mut max_score = f32::NEG_INFINITY;
                for (t, score_slot) in scores.iter_mut().enumerate() {
                    let k_vec =
                        &kv.k[t * kv_stride + kvh * head_dim..t * kv_stride + (kvh + 1) * head_dim];
                    let s = f32_dot_simd(simd, q_vec, k_vec) * scale;
                    *score_slot = s;
                    if s > max_score {
                        max_score = s;
                    }
                }
                let mut sum_exp = 0.0_f32;
                for s in &mut scores {
                    *s = (*s - max_score).exp();
                    sum_exp += *s;
                }
                let inv_sum = 1.0_f32 / sum_exp.max(1e-12_f32);
                let out_head = &mut attn_out[qh * head_dim..(qh + 1) * head_dim];
                for (t, &prob_unnorm) in scores.iter().enumerate() {
                    let w = prob_unnorm * inv_sum;
                    let v_vec =
                        &kv.v[t * kv_stride + kvh * head_dim..t * kv_stride + (kvh + 1) * head_dim];
                    axpy_simd(simd, w, v_vec, out_head);
                }
            }
        });

        let o_vec = self
            .o_proj
            .forward_vec_parallel(&attn_out, patch(&self.o_proj), workers)?;
        let mut x_res: Vec<f32> = input
            .iter()
            .zip(o_vec.iter())
            .map(|(&a, &b)| a + b)
            .collect();

        let h2 = rms_norm(&x_res, &self.ffn_norm_w, rms_eps)?;
        let gate_sig = self.gate_proj.hadamard_signature();
        let up_sig = self.up_proj.hadamard_signature();
        let shared_ffn_folded = if let Some((signs, block)) = gate_sig
            && up_sig == gate_sig
        {
            Some(apply_hadamard_sign_fold(&h2, signs, block)?)
        } else {
            None
        };
        let gate = self.gate_proj.forward_vec_with_pre_folded(
            &h2,
            shared_ffn_folded.as_deref(),
            patch(&self.gate_proj),
            workers,
        )?;
        let up = self.up_proj.forward_vec_with_pre_folded(
            &h2,
            shared_ffn_folded.as_deref(),
            patch(&self.up_proj),
            workers,
        )?;
        let swiglu: Vec<f32> = gate
            .iter()
            .zip(up.iter())
            .map(|(&g, &u)| {
                let silu = g / (1.0_f32 + (-g).exp());
                silu * u
            })
            .collect();
        let down = self
            .down_proj
            .forward_vec_parallel(&swiglu, patch(&self.down_proj), workers)?;
        dispatch!(level, simd => axpy_simd(simd, 1.0_f32, &down, &mut x_res));
        Ok(x_res)
    }
}

/// Pure-Rust 5-layer `CodePredictor` predicting acoustic codebooks `1..=15` per frame.
#[derive(Clone, Debug)]
pub struct CodePredictorModel {
    /// Predictor hidden size (`1024`).
    pub hidden_size: usize,
    /// Attention query heads (`16`).
    pub num_q_heads: usize,
    /// Attention KV heads (`8`).
    pub num_kv_heads: usize,
    /// Per-head dimension (`128`).
    pub head_dim: usize,
    /// Codebook vocabulary size (`2048`).
    pub vocab_size: usize,
    /// Number of acoustic codebooks (`15`).
    pub num_acoustic_codebooks: usize,
    /// `RoPE` base frequency (`1_000_000.0`).
    pub rope_theta: f32,
    /// `RMSNorm` epsilon (`1e-6`).
    pub rms_norm_eps: f32,
    /// Optional `mtp_proj` (`talker_hidden -> predictor_hidden` when `2048 != 1024`).
    pub mtp_proj: Option<LinearProjection>,
    /// 5 decoder layers.
    pub layers: Vec<QwenDecoderLayer>,
    /// Final `RMSNorm` weight `[hidden_size]`.
    pub norm_w: Vec<f32>,
    /// 15 private acoustic embedding tables (`[vocab_size * talker_hidden]`).
    pub codec_embeddings: Vec<Vec<f32>>,
    /// 15 private acoustic LM heads (`[vocab_size, hidden_size]`).
    pub lm_heads: Vec<LinearProjection>,
}

impl CodePredictorModel {
    fn project_in_parallel<'a>(
        &self,
        raw: &'a [f32],
        workers: usize,
    ) -> io::Result<std::borrow::Cow<'a, [f32]>> {
        self.mtp_proj
            .as_ref()
            .map_or(Ok(std::borrow::Cow::Borrowed(raw)), |proj| {
                proj.forward_vec_parallel(raw, None, workers)
                    .map(std::borrow::Cow::Owned)
            })
    }
    /// Predict all 15 acoustic codes (`c1..=c15`) for one frame given `talker_hidden`,
    /// semantic `c0`, `talker_codec_embd`, and decoupled `subtalker_*` sampling parameters.
    /// Returns `[c0, c1, ..., c15]`.
    pub fn predict_frame(
        &self,
        talker_hidden: &[f32],
        c0: i32,
        talker_codec_embd: &[f32],
        params: DecoupledSamplingParams,
        seed: i64,
        subseq_base: i64,
    ) -> io::Result<[i32; TOTAL_CODE_GROUPS]> {
        self.predict_frame_with_control(
            talker_hidden,
            c0,
            talker_codec_embd,
            params,
            seed,
            subseq_base,
            1,
            &mut || Ok(()),
        )
    }

    /// Predict one frame with explicit CPU workers and cancellation between layers/codebooks.
    #[expect(
        clippy::too_many_arguments,
        clippy::too_many_lines,
        reason = "Retain the independently selected sampler, seed, worker and cancellation controls."
    )]
    pub fn predict_frame_with_control(
        &self,
        talker_hidden: &[f32],
        c0: i32,
        talker_codec_embd: &[f32],
        params: DecoupledSamplingParams,
        seed: i64,
        subseq_base: i64,
        workers: usize,
        cancel: &mut dyn FnMut() -> io::Result<()>,
    ) -> io::Result<[i32; TOTAL_CODE_GROUPS]> {
        if !(1..=256).contains(&workers) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid CPU worker count.",
            ));
        }
        if self.num_acoustic_codebooks != NUM_ACOUSTIC_CODEBOOKS
            || self.codec_embeddings.len() != NUM_ACOUSTIC_CODEBOOKS
            || self.lm_heads.len() != NUM_ACOUSTIC_CODEBOOKS
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid CodePredictor codebook table count.",
            ));
        }
        let talker_dim = talker_hidden.len();
        let c0_usize = usize::try_from(c0).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "Negative semantic code c0.")
        })?;
        let c0_emb_start = c0_usize.saturating_mul(talker_dim);
        if c0_emb_start + talker_dim > talker_codec_embd.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Semantic code c0 out of range for talker.codec_embd.",
            ));
        }
        let c0_emb = &talker_codec_embd[c0_emb_start..c0_emb_start + talker_dim];

        let mut kv_caches = vec![LayerKvCache::default(); self.layers.len()];
        let mut codes = [0_i32; TOTAL_CODE_GROUPS];
        codes[0] = c0;
        let mut seen_scratch = Vec::new();

        // Step 0 prefill: position 0 = talker_hidden, position 1 = embed(c0)
        cancel()?;
        let mut h = self.project_in_parallel(talker_hidden, workers)?;
        for (layer, kv) in self.layers.iter().zip(kv_caches.iter_mut()) {
            cancel()?;
            h = std::borrow::Cow::Owned(layer.forward_step_parallel(
                &h,
                self.num_q_heads,
                self.num_kv_heads,
                self.head_dim,
                self.rope_theta,
                self.rms_norm_eps,
                kv,
                None,
                workers,
            )?);
        }

        let mut next_raw = c0_emb;
        for g in 0..NUM_ACOUSTIC_CODEBOOKS {
            cancel()?;
            let mut h_step = self.project_in_parallel(next_raw, workers)?;
            for (layer, kv) in self.layers.iter().zip(kv_caches.iter_mut()) {
                cancel()?;
                h_step = std::borrow::Cow::Owned(layer.forward_step_parallel(
                    &h_step,
                    self.num_q_heads,
                    self.num_kv_heads,
                    self.head_dim,
                    self.rope_theta,
                    self.rms_norm_eps,
                    kv,
                    None,
                    workers,
                )?);
            }
            let normed = rms_norm(&h_step, &self.norm_w, self.rms_norm_eps)?;
            let mut logits = self.lm_heads[g].forward_vec_parallel(&normed, None, workers)?;
            let sampled_idx = sampling::sample_top_k_p(
                &mut logits,
                sampling::SampleTopKPParams {
                    temperature: params.subtalker_temperature,
                    top_k: params.subtalker_top_k,
                    top_p: params.subtalker_top_p,
                    repetition_penalty: 1.0,
                    seed,
                    philox_subseq: subseq_base
                        .wrapping_add(1)
                        .wrapping_add(i64::try_from(g).unwrap_or(0)),
                },
                &[],
                &mut seen_scratch,
                None,
            )
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "Acoustic code sampling failed.")
            })?;
            codes[g + 1] = i32::try_from(sampled_idx).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "Invalid sampled acoustic code.")
            })?;

            if g + 1 < NUM_ACOUSTIC_CODEBOOKS {
                let table = &self.codec_embeddings[g];
                let start = sampled_idx.saturating_mul(talker_dim);
                if start + talker_dim > table.len() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Acoustic code index exceeds embedding table.",
                    ));
                }
                next_raw = &table[start..start + talker_dim];
            }
        }
        Ok(codes)
    }
}

/// Cached ICL reference streams (`icl_codec_stream` and `icl_ref_text_stream`) in `PromptCache`.
#[derive(Clone, Debug)]
pub struct CachedIclReference {
    /// FNV-1a 64-bit content hash of the `[16, ref_codes_T]` reference codes.
    pub ref_key: u64,
    /// Pre-summed codec stream `[ (1 + ref_codes_T) * hidden_size ]` (`codec_bos` + summed 16 codebooks).
    pub icl_codec_stream: Vec<f32>,
    /// Pre-projected reference text stream `[ N_ref_text * hidden_size ]`.
    pub icl_ref_text_stream: Vec<f32>,
    /// Number of inner reference text tokens (`N_ref_text`).
    pub n_ref_text: usize,
    /// Number of reference audio frames (`ref_codes_t`).
    pub ref_codes_t: usize,
}

/// Pure-Rust `PromptCache` retaining special token projections and cached ICL reference streams.
#[derive(Clone, Debug, Default)]
pub struct PromptCache {
    /// Projected `tts_bos` `[hidden_size]`.
    pub tts_bos_emb: Vec<f32>,
    /// Projected `tts_eos` `[hidden_size]`.
    pub tts_eos_emb: Vec<f32>,
    /// Projected `tts_pad` `[hidden_size]`.
    pub tts_pad_emb: Vec<f32>,
    /// `codec_pad` embedding `[hidden_size]`.
    pub codec_pad_emb: Vec<f32>,
    /// `codec_bos` embedding `[hidden_size]`.
    pub codec_bos_emb: Vec<f32>,
    /// Most-recent cached ICL reference streams so repeated `Base` synthesis skips re-projecting `REFERENCE_TEXT` and `ref_codes`.
    pub cached_icl: Option<CachedIclReference>,
}

impl PromptCache {
    /// Return or compute and cache the ICL reference codec and text streams for `ref_codes`.
    pub fn get_or_build_icl_reference(
        &mut self,
        ref_codes: &[i32],
        ref_codes_t: usize,
        compute_codec_stream: impl FnOnce() -> io::Result<Vec<f32>>,
        compute_ref_text_stream: impl FnOnce() -> io::Result<(Vec<f32>, usize)>,
    ) -> io::Result<&CachedIclReference> {
        let key = codec_ref_key(ref_codes, TOTAL_CODE_GROUPS, ref_codes_t);
        if let Some(cached) = self.cached_icl.as_ref()
            && cached.ref_key == key
            && cached.ref_codes_t == ref_codes_t
        {
            return Ok(cached);
        }
        let icl_codec_stream = compute_codec_stream()?;
        let (icl_ref_text_stream, n_ref_text) = compute_ref_text_stream()?;
        Ok(self.cached_icl.insert(CachedIclReference {
            ref_key: key,
            icl_codec_stream,
            icl_ref_text_stream,
            n_ref_text,
            ref_codes_t,
        }))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{
        CodePredictorModel, DecoupledSamplingParams, LinearProjection, LoraProjectionPatch,
        NUM_ACOUSTIC_CODEBOOKS, PromptCache, QwenDecoderLayer, TalkerLoraAdapter,
        WeightQuantization, is_talker_ternary_eligible, repack_pq2_to_ptq1,
        sylvester_walsh_hadamard_in_place, validate_talker_ternary_policy,
    };

    #[test]
    fn walsh_hadamard_is_involutory_and_pq2_ptq1_repack_is_bit_exact() -> std::io::Result<()> {
        let original = vec![0.5_f32, -1.25, 2.0, 0.75, -0.5, 1.5, -2.25, 0.125];
        let mut buf = original.clone();
        sylvester_walsh_hadamard_in_place(&mut buf)?;
        sylvester_walsh_hadamard_in_place(&mut buf)?;
        for (a, b) in original.iter().zip(buf.iter()) {
            assert!((a - b).abs() < 1e-6, "a={a} b={b}");
        }

        // 8 weights, group_size = 8, 2 output rows
        // Row 0 trits: [-1, 0, +1, +1, 0, -1, +1, 0] -> codes [0, 1, 2, 2], [1, 0, 2, 1]
        let b0 = (1_u8 << 2) | (2_u8 << 4) | (2_u8 << 6);
        let b1 = 1_u8 | (2_u8 << 4) | (1_u8 << 6);
        let pq2 = WeightQuantization::PQ2_0 {
            group_size: 8,
            scales: vec![0.5, 0.25],
            packed_trits: vec![b0, b1, b1, b0],
            hadamard_block: 8,
            signs: vec![1, -1, 1, 1, -1, 1, -1, 1],
        };
        let ptq1 = repack_pq2_to_ptq1(&pq2)?;

        let proj_pq2 = LinearProjection::new(
            "talker.blk.0.attn_q.weight",
            8,
            2,
            pq2,
            Some(vec![0.1, -0.1]),
        )?;
        let proj_ptq1 = LinearProjection::new(
            "talker.blk.0.attn_q.weight",
            8,
            2,
            ptq1,
            Some(vec![0.1, -0.1]),
        )?;
        let out_pq2 = proj_pq2.forward_vec(&original, None)?;
        let out_ptq1 = proj_ptq1.forward_vec(&original, None)?;
        assert_eq!(
            out_pq2, out_ptq1,
            "PTQ1_0 repack must be bit-exact with PQ2_0"
        );

        // Exercise 16-element SIMD group loops (group_size = 32, in_dim = 64, 2 output rows).
        let input64: Vec<f32> = (0_u8..64).map(|i| (f32::from(i) - 31.5) * 0.0625).collect();
        let packed64: Vec<u8> = (0_u8..32)
            .map(|i| {
                let c0 = i % 3;
                let c1 = (i + 1) % 3;
                let c2 = (i + 2) % 3;
                let c3 = (i * 2) % 3;
                c0 | (c1 << 2) | (c2 << 4) | (c3 << 6)
            })
            .collect();
        let signs64: Vec<i8> = (0_u8..64)
            .map(|i| if i.is_multiple_of(2) { 1 } else { -1 })
            .collect();
        let pq2_64 = WeightQuantization::PQ2_0 {
            group_size: 32,
            scales: vec![0.375, -0.625, 0.5, 0.25],
            packed_trits: packed64,
            hadamard_block: 16,
            signs: signs64,
        };
        let ptq1_64 = repack_pq2_to_ptq1(&pq2_64)?;
        let proj_pq2_64 = LinearProjection::new(
            "talker.blk.0.attn_k.weight",
            64,
            2,
            pq2_64,
            Some(vec![0.125, -0.25]),
        )?;
        let proj_ptq1_64 = LinearProjection::new(
            "talker.blk.0.attn_k.weight",
            64,
            2,
            ptq1_64,
            Some(vec![0.125, -0.25]),
        )?;
        assert_eq!(
            proj_pq2_64.forward_vec(&input64, None)?,
            proj_ptq1_64.forward_vec(&input64, None)?,
            "16-wide SIMD PTQ1_0 and PQ2_0 kernels must be bit-identical"
        );
        Ok(())
    }

    #[test]
    fn ternary_policy_admits_only_talker_block_projections() {
        let dummy_pq2 = WeightQuantization::PQ2_0 {
            group_size: 8,
            scales: vec![1.0],
            packed_trits: vec![0, 0],
            hadamard_block: 8,
            signs: vec![1; 8],
        };
        assert!(is_talker_ternary_eligible("talker.blk.0.attn_q.weight"));
        assert!(is_talker_ternary_eligible("talker.blk.27.ffn_down.weight"));
        assert!(
            validate_talker_ternary_policy("talker.blk.12.attn_output.weight", &dummy_pq2).is_ok()
        );

        for forbidden in [
            "talker.text_embd.weight",
            "talker.codec_embd.weight",
            "talker.text_proj.fc1.weight",
            "talker.codec_head.weight",
            "code_pred.blk.0.attn_q.weight",
            "code_pred.lm_head.0.weight",
            "tok_dec.pre_conv.weight",
            "spk_enc.conv0.weight",
        ] {
            assert!(!is_talker_ternary_eligible(forbidden));
            assert!(validate_talker_ternary_policy(forbidden, &dummy_pq2).is_err());
        }
    }

    #[test]
    fn lora_overlay_preserves_zero_scale_bit_identity_and_applies_nonzero_delta()
    -> std::io::Result<()> {
        let proj = LinearProjection::new(
            "talker.blk.0.attn_q.weight",
            4,
            2,
            WeightQuantization::F32(vec![
                0.5, -0.25, 0.1, 0.2, // row 0
                -0.1, 0.4, 0.3, -0.5, // row 1
            ]),
            Some(vec![0.05, -0.05]),
        )?;
        let x = [1.0_f32, -0.5, 0.25, 0.75];
        let base_out = proj.forward_vec(&x, None)?;

        let mut patch = LoraProjectionPatch {
            in_dim: 4,
            out_dim: 2,
            rank: 2,
            alpha_over_rank: 2.0,
            scale: 0.0,
            lora_a: vec![0.3, -0.2, 0.4, 0.1, -0.5, 0.2, 0.1, 0.3],
            lora_b: vec![0.2, -0.4, 0.5, 0.1],
        };
        let zero_out = proj.forward_vec(&x, Some(&patch))?;
        assert_eq!(
            base_out, zero_out,
            "Loaded-at-zero LoRA must be 0-ULP identical"
        );

        patch.scale = 1.0;
        let active_out = proj.forward_vec(&x, Some(&patch))?;
        assert_ne!(base_out, active_out);
        Ok(())
    }

    fn code_predictor_fixture() -> std::io::Result<CodePredictorModel> {
        let hidden = 4_usize;
        let vocab = 8_usize;
        let make_proj =
            |name: &str, in_d: usize, out_d: usize| -> std::io::Result<LinearProjection> {
                LinearProjection::new(
                    name,
                    in_d,
                    out_d,
                    WeightQuantization::F32(vec![0.05; out_d * in_d]),
                    None,
                )
            };
        let layer = QwenDecoderLayer {
            attn_norm_w: vec![1.0; hidden],
            q_proj: make_proj("code_pred.blk.0.attn_q.weight", hidden, hidden)?,
            k_proj: make_proj("code_pred.blk.0.attn_k.weight", hidden, hidden)?,
            v_proj: make_proj("code_pred.blk.0.attn_v.weight", hidden, hidden)?,
            o_proj: make_proj("code_pred.blk.0.attn_output.weight", hidden, hidden)?,
            q_norm_w: vec![1.0; 2],
            k_norm_w: vec![1.0; 2],
            ffn_norm_w: vec![1.0; hidden],
            gate_proj: make_proj("code_pred.blk.0.ffn_gate.weight", hidden, hidden * 2)?,
            up_proj: make_proj("code_pred.blk.0.ffn_up.weight", hidden, hidden * 2)?,
            down_proj: make_proj("code_pred.blk.0.ffn_down.weight", hidden * 2, hidden)?,
        };
        let mut lm_heads = Vec::with_capacity(NUM_ACOUSTIC_CODEBOOKS);
        for g in 0..NUM_ACOUSTIC_CODEBOOKS {
            lm_heads.push(make_proj(
                &format!("code_pred.lm_head.{g}.weight"),
                hidden,
                vocab,
            )?);
        }
        Ok(CodePredictorModel {
            hidden_size: hidden,
            num_q_heads: 2,
            num_kv_heads: 2,
            head_dim: 2,
            vocab_size: vocab,
            num_acoustic_codebooks: NUM_ACOUSTIC_CODEBOOKS,
            rope_theta: 1_000_000.0,
            rms_norm_eps: 1e-6,
            mtp_proj: None,
            layers: vec![layer],
            norm_w: vec![1.0; hidden],
            codec_embeddings: vec![vec![0.1; vocab * hidden]; NUM_ACOUSTIC_CODEBOOKS],
            lm_heads,
        })
    }

    #[test]
    fn code_predictor_and_prompt_cache_execute_cleanly() -> std::io::Result<()> {
        let hidden = 4;
        let vocab = 8;
        let cp = code_predictor_fixture()?;
        let talker_embd = vec![0.2_f32; vocab * hidden];
        let codes = cp.predict_frame(
            &[0.1, -0.2, 0.3, 0.4],
            3,
            &talker_embd,
            DecoupledSamplingParams::base_clone(),
            42,
            100,
        )?;
        assert_eq!(codes[0], 3);
        assert!(codes.iter().all(|&c| (0..8).contains(&c)));
        let mut pcache = PromptCache::default();
        let ref_codes = [1_i32; 32];
        let mut codec_builds = 0_usize;
        let _ = pcache.get_or_build_icl_reference(
            &ref_codes,
            2,
            || {
                codec_builds += 1;
                Ok(vec![1.0; 12])
            },
            || Ok((vec![2.0; 8], 2)),
        )?;
        let _ = pcache.get_or_build_icl_reference(
            &ref_codes,
            2,
            || {
                codec_builds += 1;
                Ok(vec![1.0; 12])
            },
            || Ok((vec![2.0; 8], 2)),
        )?;
        assert_eq!(
            codec_builds, 1,
            "PromptCache must reuse cached ICL streams on hit"
        );

        let _ = TalkerLoraAdapter {
            name: "indic-en".to_owned(),
            patches: HashMap::new(),
        };
        Ok(())
    }

    #[test]
    fn predictor_workers_preserve_codes_and_cancellation_reaches_inner_layers()
    -> std::io::Result<()> {
        let cp = code_predictor_fixture()?;
        let talker_embd = vec![0.2_f32; 8 * 4];
        let codes = cp.predict_frame(
            &[0.1, -0.2, 0.3, 0.4],
            3,
            &talker_embd,
            DecoupledSamplingParams::base_clone(),
            42,
            100,
        )?;
        let parallel = cp.predict_frame_with_control(
            &[0.1, -0.2, 0.3, 0.4],
            3,
            &talker_embd,
            DecoupledSamplingParams::base_clone(),
            42,
            100,
            4,
            &mut || Ok(()),
        )?;
        assert_eq!(codes, parallel);
        let mut polls = 0;
        let cancelled = cp.predict_frame_with_control(
            &[0.1, -0.2, 0.3, 0.4],
            3,
            &talker_embd,
            DecoupledSamplingParams::base_clone(),
            42,
            100,
            1,
            &mut || {
                polls += 1;
                if polls == 4 {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::Interrupted,
                        "cancelled",
                    ))
                } else {
                    Ok(())
                }
            },
        );
        assert_eq!(polls, 4);
        assert!(cancelled.is_err_and(|error| error.kind() == std::io::ErrorKind::Interrupted));

        Ok(())
    }

    #[test]
    fn stream_chunk_config_and_parallel_projection_match_sequential() -> std::io::Result<()> {
        let cfg = super::StreamChunkConfig::low_latency_4_frame().validate()?;
        assert_eq!(cfg.frames_for_chunk(0), 4);
        assert_eq!(cfg.frames_for_chunk(1), 4);
        assert_eq!(
            super::StreamChunkConfig::standard_8_frame().frames_for_chunk(0),
            8
        );

        let proj_par = LinearProjection::new(
            "talker.blk.0.attn_q.weight",
            4,
            8,
            WeightQuantization::F32(
                (0..32)
                    .map(|i| f32::from(u8::try_from(i).unwrap_or(0)) * 0.1)
                    .collect(),
            ),
            Some(vec![0.05; 8]),
        )?;
        let x4 = [0.25_f32, -0.5, 0.75, 1.0];
        assert_eq!(
            proj_par.forward_vec(&x4, None)?,
            proj_par.forward_vec_parallel(&x4, None, 4)?
        );

        let q8_proj = LinearProjection::new(
            "code_pred.blk.0.attn_q.weight",
            32,
            2,
            WeightQuantization::Q8_0 {
                blocks: vec![
                    super::Q8Block {
                        scale: 0.25,
                        quants: [2_i8; 32],
                    },
                    super::Q8Block {
                        scale: -0.5,
                        quants: [-1_i8; 32],
                    },
                ],
            },
            None,
        )?;
        let q8_out = q8_proj.forward_vec(&[0.5_f32; 32], None)?;
        assert!((q8_out[0] - 8.0_f32).abs() < 1e-6);
        assert!((q8_out[1] - 8.0_f32).abs() < 1e-6);

        let mut in_place = x4;
        let out_of_place = super::rms_norm(&x4, &[1.0, 0.5, 1.5, 2.0], 1e-6)?;
        super::rms_norm_in_place(&mut in_place, &[1.0, 0.5, 1.5, 2.0], 1e-6)?;
        assert_eq!(out_of_place.as_slice(), in_place.as_slice());
        Ok(())
    }
}
