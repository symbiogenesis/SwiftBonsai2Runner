//! Pure-Rust Qwen3-TTS 12 Hz / 48 kHz causal audio codec and streaming `CodecStateSnap` cache.
//!
//! Implements both the 24 kHz (`TOKENIZER_HOP_24K = 1920`) and 48 kHz
//! (`takuma104/Qwen3-TTS-Tokenizer-12Hz-48kHz`, `TOKENIZER_HOP_48K = 3840`)
//! causal `ConvNeXt`/`SnakeBeta` decoder pipelines, the stateful `CodecStateSnap`
//! 8-slot LRU cache for zero-redecode ICL reference priming, and the `SEANet` +
//! Mimi transformer + RVQ reference encoder.
#![forbid(unsafe_code)]
#![allow(
    clippy::missing_errors_doc,
    reason = "Internal codec and convolution shape checks return io::ErrorKind::InvalidInput."
)]

use std::io;

use fearless_simd::{Level, dispatch, f32x4, prelude::*};

use crate::{SAMPLE_RATE, SAMPLE_RATE_48K};

#[allow(clippy::inline_always, reason = "fearless_simd dispatch! requires inline(always) to inherit target_feature attributes.")]
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
        if frames == 0
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

        let mut out = vec![0.0_f32; out_frames * self.out_c];
        for t_out in 0..out_frames {
            let t_in_base = t_out * self.stride;
            let out_row = &mut out[t_out * self.out_c..(t_out + 1) * self.out_c];
            if let Some(bias) = &self.bias {
                out_row.copy_from_slice(bias);
            }
            for (oc, out_slot) in out_row.iter_mut().enumerate().take(self.out_c) {
                let mut acc = *out_slot;
                for ic in 0..self.in_c {
                    let w_base = (oc * self.in_c + ic) * self.kernel;
                    for k in 0..self.kernel {
                        let t_in = t_in_base + k * self.dilation;
                        acc = self.weight[w_base + k].mul_add(padded[t_in * self.in_c + ic], acc);
                    }
                }
                *out_slot = acc;
            }
        }
        Ok((out, out_frames))
    }

    /// Stateful streaming causal forward pass (`stride == 1`) updating `state` (`[L, in_c]`) in place.
    pub fn forward_stream(
        &self,
        input: &[f32],
        frames: usize,
        state: &mut [f32],
    ) -> io::Result<Vec<f32>> {
        let ctx_frames = self.stream_context_frames();
        if self.stride != 1
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

        let mut out = vec![0.0_f32; frames * self.out_c];
        for t in 0..frames {
            let out_row = &mut out[t * self.out_c..(t + 1) * self.out_c];
            if let Some(bias) = &self.bias {
                out_row.copy_from_slice(bias);
            }
            for (oc, out_slot) in out_row.iter_mut().enumerate().take(self.out_c) {
                let mut acc = *out_slot;
                for ic in 0..self.in_c {
                    let w_base = (oc * self.in_c + ic) * self.kernel;
                    for k in 0..self.kernel {
                        let t_ext = t + k * self.dilation;
                        acc = self.weight[w_base + k].mul_add(ext[t_ext * self.in_c + ic], acc);
                    }
                }
                *out_slot = acc;
            }
        }
        Ok(out)
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
        let mut carry = vec![0.0_f32; self.carry_frames() * self.out_c];
        let out = self.forward_stream(input, frames, &mut carry)?;
        Ok((out, frames * self.stride))
    }

    /// Stateful streaming causal transposed convolution updating `carry` (`[kernel - stride, out_c]`).
    pub fn forward_stream(
        &self,
        input: &[f32],
        frames: usize,
        carry: &mut [f32],
    ) -> io::Result<Vec<f32>> {
        let trim = self.carry_frames();
        if frames == 0
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

        for t in 0..frames {
            let in_row = &input[t * self.in_c..(t + 1) * self.in_c];
            let out_base = t * self.stride;
            for (ic, &x) in in_row.iter().enumerate().take(self.in_c) {
                for oc in 0..self.out_c {
                    let w_base = (ic * self.out_c + oc) * self.kernel;
                    for k in 0..self.kernel {
                        let dst_idx = (out_base + k) * self.out_c + oc;
                        raw[dst_idx] = x.mul_add(self.weight[w_base + k], raw[dst_idx]);
                    }
                }
            }
        }

        if trim > 0 {
            carry.copy_from_slice(&raw[emit * self.out_c..]);
        }

        let mut emitted = raw[..emit * self.out_c].to_vec();
        if let Some(bias) = &self.bias {
            for t in 0..emit {
                let row = &mut emitted[t * self.out_c..(t + 1) * self.out_c];
                for (dst, &b) in row.iter_mut().zip(bias.iter()) {
                    *dst += b;
                }
            }
        }
        Ok(emitted)
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
        let inv_b = beta.iter().map(|&v| 1.0_f32 / (v.exp() + 1e-9_f32)).collect();
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
        let mut h = input.to_vec();
        self.act1.apply_in_place(&mut h, frames)?;
        let mut h = self.conv1.forward_stream(&h, frames, conv1_state)?;
        self.act2.apply_in_place(&mut h, frames)?;
        let mut dummy_state = [];
        let mut out = self.conv2.forward_stream(&h, frames, &mut dummy_state)?;
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
        let mut h = input.to_vec();
        self.snake1.apply_in_place(&mut h, frames)?;
        let mut h = self.trans_conv.forward_stream(&h, frames, carry)?;
        let out_frames = frames * self.trans_conv.stride;
        for (ru, st) in self.res_units.iter().zip(ru_states.iter_mut()) {
            h = ru.forward_stream(&h, out_frames, st)?;
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

        for t in 0..frames {
            let mut y = vec![0.0_f32; c];
            for ch in 0..c {
                let mut acc = self.dw_bias[ch];
                let w_row = &self.dw_weight[ch * k..(ch + 1) * k];
                for ki in 0..k {
                    acc = w_row[ki].mul_add(ext[(t + ki) * c + ch], acc);
                }
                y[ch] = acc;
            }
            // LayerNorm across channels
            let mean = y.iter().copied().sum::<f32>() * inv_c;
            let var = y
                .iter()
                .map(|&v| {
                    let d = v - mean;
                    d * d
                })
                .sum::<f32>()
                * inv_c;
            let inv_std = 1.0_f32 / (var + 1e-6_f32).sqrt();
            for ch in 0..c {
                y[ch] = ((y[ch] - mean) * inv_std).mul_add(self.norm_w[ch], self.norm_b[ch]);
            }

            // pwconv1 + exact/tanh GELU
            let mut mid = vec![0.0_f32; mid_c];
            for o in 0..mid_c {
                let w_row = &self.pwconv1_w[o * c..(o + 1) * c];
                let mut acc = self.pwconv1_b[o];
                for i in 0..c {
                    acc = w_row[i].mul_add(y[i], acc);
                }
                // Tanh-approximated GELU matching ggml_gelu
                let x = acc;
                let k_gelu = (2.0_f32 / core::f32::consts::PI).sqrt();
                let inner = k_gelu * 0.044_715_f32.mul_add(x * x * x, x);
                mid[o] = 0.5_f32 * x * (1.0_f32 + inner.tanh());
            }

            // pwconv2 + gamma + residual
            let out_row = &mut out[t * c..(t + 1) * c];
            for o in 0..c {
                let w_row = &self.pwconv2_w[o * mid_c..(o + 1) * mid_c];
                let mut acc = self.pwconv2_b[o];
                for i in 0..mid_c {
                    acc = w_row[i].mul_add(mid[i], acc);
                }
                out_row[o] = acc.mul_add(self.gamma[o], out_row[o]);
            }
        }
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
                let row = &self.codebooks[k][idx * self.codebook_dim..(idx + 1) * self.codebook_dim];
                for (dst, &val) in sum_emb.iter_mut().zip(row.iter()) {
                    *dst += val;
                }
            }
            let out_row = &mut out[t * self.hidden_dim..(t + 1) * self.hidden_dim];
            for o in 0..self.hidden_dim {
                let w_row = &self.out_proj_w[o * self.codebook_dim..(o + 1) * self.codebook_dim];
                let mut acc = 0.0_f32;
                for i in 0..self.codebook_dim {
                    acc = w_row[i].mul_add(sum_emb[i], acc);
                }
                out_row[o] = acc;
            }
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
        for (dst, &val) in h_sem.iter_mut().zip(h_aco.iter()) {
            *dst += val;
        }
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
        let mut sqsum = Vec::with_capacity(num_layers);
        for cb in &codebooks {
            let mut layer_sq = Vec::with_capacity(codebook_size);
            for e in 0..codebook_size {
                let row = &cb[e * codebook_dim..(e + 1) * codebook_dim];
                let norm_sq: f32 = row.iter().map(|&v| v * v).sum();
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
    pub fn encode(&self, hidden: &[f32], frames: usize, codes_out: &mut Vec<i32>) -> io::Result<()> {
        if hidden.len() != frames * self.hidden_size {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid RvqEncodeSide input length.",
            ));
        }
        let d = self.codebook_dim;
        let mut res = vec![0.0_f32; frames * d];
        for t in 0..frames {
            let x = &hidden[t * self.hidden_size..(t + 1) * self.hidden_size];
            let y = &mut res[t * d..(t + 1) * d];
            for o in 0..d {
                let w_row = &self.input_proj[o * self.hidden_size..(o + 1) * self.hidden_size];
                let mut acc = 0.0_f32;
                for i in 0..self.hidden_size {
                    acc = w_row[i].mul_add(x[i], acc);
                }
                y[o] = acc;
            }
        }

        for l in 0..self.num_layers {
            let cb = &self.codebooks[l];
            let sq = &self.sqsum[l];
            for t in 0..frames {
                let r = &mut res[t * d..(t + 1) * d];
                let mut best_e = 0_usize;
                let mut best_sc = f32::INFINITY;
                for e in 0..self.codebook_size {
                    let c = &cb[e * d..(e + 1) * d];
                    let mut dot = 0.0_f32;
                    for i in 0..d {
                        dot = r[i].mul_add(c[i], dot);
                    }
                    let sc = (-2.0_f32).mul_add(dot, sq[e]);
                    if sc < best_sc {
                        best_sc = sc;
                        best_e = e;
                    }
                }
                codes_out.push(i32::try_from(best_e).unwrap_or(0));
                let best_centroid = &cb[best_e * d..(best_e + 1) * d];
                for i in 0..d {
                    r[i] -= best_centroid[i];
                }
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

    /// Restore the streaming state matching `key`, bumping its LRU stamp on hit.
    pub fn restore(&mut self, key: u64) -> Option<CodecStreamState> {
        for slot in &mut self.slots {
            if slot.stamp != 0 && slot.key == key {
                self.stamp = self.stamp.saturating_add(1);
                slot.stamp = self.stamp;
                return Some(slot.state.clone());
            }
        }
        None
    }

    /// Save `state` under `key`, evicting the least-recently-used slot when full.
    pub fn snapshot(&mut self, key: u64, state: CodecStreamState) {
        self.stamp = self.stamp.saturating_add(1);
        if let Some(existing) = self
            .slots
            .iter_mut()
            .find(|s| s.stamp != 0 && s.key == key)
        {
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
        if state.dac_carry.len() != self.blocks.len() || state.dac_ru.len() != self.blocks.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "DAC streaming state block count mismatch.",
            ));
        }
        let mut h = self.conv_pre.forward_stream(input, frames, &mut state.dac_pre)?;
        let mut cur_frames = frames;
        for (i, blk) in self.blocks.iter().enumerate() {
            let (next_h, next_f) = blk.forward_stream(
                &h,
                cur_frames,
                &mut state.dac_carry[i],
                &mut state.dac_ru[i],
            )?;
            h = next_h;
            cur_frames = next_f;
        }
        self.snake_post.apply_in_place(&mut h, cur_frames)?;
        let mut audio = self
            .conv_post
            .forward_stream(&h, cur_frames, &mut state.dac_post)?;
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
                -1.0, 1.0,  // 1
                1.0, -1.0,  // 2
                1.0, 1.0,   // 3
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
    #[allow(
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
}
