//! Pure-Rust ECAPA-TDNN speaker encoder and Slaney log-mel spectrogram extractor.
//!
//! Mirrors `qwen_tts.core.models.modeling_qwen3_tts.Qwen3TTSSpeakerEncoder`
//! for 24 kHz mono reference waveforms:
//! 1. Reflect-pad by `(n_fft - hop) / 2 = 384` samples on each side.
//! 2. Periodic Hann window (`n_fft = 1024, hop = 256`) + real/imaginary DFT
//!    power spectrum (`mag = sqrt(re^2 + im^2 + 1e-9)`).
//! 3. Slaney-normalized 128-bin triangular mel filterbank (`0..12000` Hz)
//!    followed by `log(max(mel, 1e-5))`.
//! 4. Initial TDNN (`k = 5, 128 -> 512`, reflect-padded) + `ReLU`.
//! 5. Three SE-Res2Net blocks (`dilations = [2, 3, 4], scale = 8, se_channels = 128`).
//! 6. Multi-layer feature aggregation (`MFA`, `k = 1, 1536 -> 1536`) + `ReLU`.
//! 7. Attentive Statistical Pooling (`ASP`, `1536 -> 3072`) + final `k = 1` FC
//!    projection (`3072 -> 2048` x-vector speaker embedding).
#![forbid(unsafe_code)]
#![expect(
    clippy::missing_errors_doc,
    reason = "Internal spectrogram and TDNN shape checks return io::ErrorKind::InvalidInput."
)]

use std::io;

use fearless_simd::{Level, dispatch, f32x4, prelude::*};

use crate::numerics::{axpy_simd, f32_dot_simd, usize_to_f32};

#[expect(
    clippy::inline_always,
    reason = "fearless_simd dispatch! requires inline(always) to inherit target_feature attributes."
)]
#[inline(always)]
fn window_frame_simd<S: Simd>(simd: S, frame: &[f32], hann: &[f32], dst: &mut [f32]) {
    let (f_chunks, f_tail) = frame.as_chunks::<4>();
    let (h_chunks, h_tail) = hann.as_chunks::<4>();
    let (d_chunks, d_tail) = dst.as_chunks_mut::<4>();
    for ((fc, hc), dc) in f_chunks
        .iter()
        .zip(h_chunks.iter())
        .zip(d_chunks.iter_mut())
    {
        let fv = f32x4::from_slice(simd, fc);
        let hv = f32x4::from_slice(simd, hc);
        (fv * hv).store_slice(dc);
    }
    for ((&f, &h), d) in f_tail.iter().zip(h_tail.iter()).zip(d_tail.iter_mut()) {
        *d = f * h;
    }
}

#[expect(
    clippy::inline_always,
    reason = "fearless_simd dispatch! requires inline(always) to inherit target_feature attributes."
)]
#[inline(always)]
fn f32_dot_pair_simd<S: Simd>(simd: S, x: &[f32], y0: &[f32], y1: &[f32]) -> (f32, f32) {
    let (x_chunks, x_tail) = x.as_chunks::<4>();
    let (y0_chunks, y0_tail) = y0.as_chunks::<4>();
    let (y1_chunks, y1_tail) = y1.as_chunks::<4>();
    let mut acc0_a = f32x4::splat(simd, 0.0_f32);
    let mut acc0_b = f32x4::splat(simd, 0.0_f32);
    let mut acc1_a = f32x4::splat(simd, 0.0_f32);
    let mut acc1_b = f32x4::splat(simd, 0.0_f32);
    let (x_pairs, x_rem) = x_chunks.as_chunks::<2>();
    let (y0_pairs, y0_rem) = y0_chunks.as_chunks::<2>();
    let (y1_pairs, y1_rem) = y1_chunks.as_chunks::<2>();
    for ((xp, y0p), y1p) in x_pairs.iter().zip(y0_pairs.iter()).zip(y1_pairs.iter()) {
        let x_a = f32x4::from_slice(simd, &xp[0]);
        let x_b = f32x4::from_slice(simd, &xp[1]);
        acc0_a = x_a.mul_add(f32x4::from_slice(simd, &y0p[0]), acc0_a);
        acc0_b = x_b.mul_add(f32x4::from_slice(simd, &y0p[1]), acc0_b);
        acc1_a = x_a.mul_add(f32x4::from_slice(simd, &y1p[0]), acc1_a);
        acc1_b = x_b.mul_add(f32x4::from_slice(simd, &y1p[1]), acc1_b);
    }
    for ((xc, y0c), y1c) in x_rem.iter().zip(y0_rem.iter()).zip(y1_rem.iter()) {
        let xv = f32x4::from_slice(simd, xc);
        acc0_a = xv.mul_add(f32x4::from_slice(simd, y0c), acc0_a);
        acc1_a = xv.mul_add(f32x4::from_slice(simd, y1c), acc1_a);
    }
    let mut l0 = [0.0_f32; 4];
    let mut l1 = [0.0_f32; 4];
    (acc0_a + acc0_b).store_slice(&mut l0);
    (acc1_a + acc1_b).store_slice(&mut l1);
    let mut s0 = (l0[0] + l0[1]) + (l0[2] + l0[3]);
    let mut s1 = (l1[0] + l1[1]) + (l1[2] + l1[3]);
    for ((&xv, &v0), &v1) in x_tail.iter().zip(y0_tail.iter()).zip(y1_tail.iter()) {
        s0 = xv.mul_add(v0, s0);
        s1 = xv.mul_add(v1, s1);
    }
    (s0, s1)
}

#[expect(
    clippy::inline_always,
    reason = "fearless_simd dispatch! requires inline(always) to inherit target_feature attributes."
)]
#[inline(always)]
fn axpy4_simd<S: Simd>(
    simd: S,
    w: [f32; 4],
    x0: &[f32],
    x1: &[f32],
    x2: &[f32],
    x3: &[f32],
    dst: &mut [f32],
) {
    let w0 = f32x4::splat(simd, w[0]);
    let w1 = f32x4::splat(simd, w[1]);
    let w2 = f32x4::splat(simd, w[2]);
    let w3 = f32x4::splat(simd, w[3]);
    let (c0, t0) = x0.as_chunks::<4>();
    let (c1, t1) = x1.as_chunks::<4>();
    let (c2, t2) = x2.as_chunks::<4>();
    let (c3, t3) = x3.as_chunks::<4>();
    let (dc, dt) = dst.as_chunks_mut::<4>();
    for ((((v0, v1), v2), v3), d) in c0
        .iter()
        .zip(c1.iter())
        .zip(c2.iter())
        .zip(c3.iter())
        .zip(dc.iter_mut())
    {
        let mut acc = f32x4::from_slice(simd, d);
        acc = w0.mul_add(f32x4::from_slice(simd, v0), acc);
        acc = w1.mul_add(f32x4::from_slice(simd, v1), acc);
        acc = w2.mul_add(f32x4::from_slice(simd, v2), acc);
        acc = w3.mul_add(f32x4::from_slice(simd, v3), acc);
        acc.store_slice(d);
    }
    for ((((&s0, &s1), &s2), &s3), d) in t0
        .iter()
        .zip(t1.iter())
        .zip(t2.iter())
        .zip(t3.iter())
        .zip(dt.iter_mut())
    {
        let mut acc = *d;
        acc = w[0].mul_add(s0, acc);
        acc = w[1].mul_add(s1, acc);
        acc = w[2].mul_add(s2, acc);
        acc = w[3].mul_add(s3, acc);
        *d = acc;
    }
}

#[expect(
    clippy::inline_always,
    reason = "fearless_simd dispatch! requires inline(always) to inherit target_feature attributes."
)]
#[inline(always)]
fn relu_in_place_simd<S: Simd>(simd: S, dst: &mut [f32]) {
    let zero_v = f32x4::splat(simd, 0.0_f32);
    let (chunks, tail) = dst.as_chunks_mut::<4>();
    for c in chunks {
        f32x4::from_slice(simd, c).max(zero_v).store_slice(c);
    }
    for val in tail {
        if *val < 0.0_f32 {
            *val = 0.0_f32;
        }
    }
}

/// Expected input sample rate for the Qwen3-TTS speaker encoder (`24_000` Hz).
pub const SPEAKER_ENCODER_SAMPLE_RATE: u32 = 24_000;
/// Default STFT FFT size (`1024`).
pub const MEL_N_FFT: usize = 1024;
/// Default STFT hop size (`256`).
pub const MEL_HOP: usize = 256;
/// Default number of mel filterbank channels (`128`).
pub const MEL_CHANNELS: usize = 128;
/// Default speaker x-vector embedding dimension (`2048`).
pub const SPEAKER_EMBEDDING_DIM: usize = 2048;

/// Configuration for the Slaney log-mel spectrogram frontend.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AudioMelConfig {
    /// Audio sample rate in Hz (`24_000`).
    pub sample_rate: u32,
    /// FFT window length (`1024`).
    pub n_fft: usize,
    /// Frame hop length (`256`).
    pub hop: usize,
    /// Number of mel bins (`128`).
    pub n_mels: usize,
    /// Minimum filterbank frequency in Hz (`0.0`).
    pub fmin: f32,
    /// Maximum filterbank frequency in Hz (`12_000.0`).
    pub fmax: f32,
}

impl Default for AudioMelConfig {
    fn default() -> Self {
        Self {
            sample_rate: SPEAKER_ENCODER_SAMPLE_RATE,
            n_fft: MEL_N_FFT,
            hop: MEL_HOP,
            n_mels: MEL_CHANNELS,
            fmin: 0.0,
            fmax: 12_000.0,
        }
    }
}

/// Slaney mel-scale forward conversion (`librosa.filters.mel(htk=False)`).
#[must_use]
pub fn hz_to_mel_slaney(hz: f32) -> f32 {
    const F_MIN: f32 = 0.0;
    const F_SP: f32 = 200.0 / 3.0;
    const MIN_LOG_HZ: f32 = 1000.0;
    const MIN_LOG_MEL: f32 = (MIN_LOG_HZ - F_MIN) / F_SP;
    let logstep = 6.4_f32.ln() / 27.0_f32;
    if hz < MIN_LOG_HZ {
        (hz - F_MIN) / F_SP
    } else {
        MIN_LOG_MEL + (hz / MIN_LOG_HZ).ln() / logstep
    }
}

/// Slaney mel-scale inverse conversion (`librosa.filters.mel(htk=False)`).
#[must_use]
pub fn mel_to_hz_slaney(mel: f32) -> f32 {
    const F_MIN: f32 = 0.0;
    const F_SP: f32 = 200.0 / 3.0;
    const MIN_LOG_HZ: f32 = 1000.0;
    const MIN_LOG_MEL: f32 = (MIN_LOG_HZ - F_MIN) / F_SP;
    let logstep = 6.4_f32.ln() / 27.0_f32;
    if mel < MIN_LOG_MEL {
        F_SP.mul_add(mel, F_MIN)
    } else {
        MIN_LOG_HZ * (logstep * (mel - MIN_LOG_MEL)).exp()
    }
}

/// Precomputed periodic Hann window, DFT basis matrices, and Slaney mel filterbank.
#[derive(Clone, Debug)]
pub struct MelSpectrogramExtractor {
    cfg: AudioMelConfig,
    n_freq: usize,
    hann: Vec<f32>,
    dft_real: Vec<f32>,
    dft_imag: Vec<f32>,
    mel_basis: Vec<f32>,
}

impl MelSpectrogramExtractor {
    /// Bake the periodic Hann window, DFT basis, and Slaney triangular mel filterbank.
    ///
    /// # Errors
    /// Rejects zero `n_fft`, `hop`, `n_mels`, or `n_fft < hop`.
    pub fn new(cfg: AudioMelConfig) -> io::Result<Self> {
        if cfg.n_fft == 0 || cfg.hop == 0 || cfg.n_mels == 0 || cfg.n_fft < cfg.hop {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid mel spectrogram configuration.",
            ));
        }
        let n_freq = cfg.n_fft / 2 + 1;
        let n_fft_f = usize_to_f32(cfg.n_fft);
        let two_pi = 2.0_f32 * core::f32::consts::PI;

        let mut hann = Vec::with_capacity(cfg.n_fft);
        for i in 0..cfg.n_fft {
            let phase = two_pi * usize_to_f32(i) / n_fft_f;
            hann.push(0.5_f32 * (1.0_f32 - phase.cos()));
        }

        let mut dft_real = vec![0.0_f32; n_freq * cfg.n_fft];
        let mut dft_imag = vec![0.0_f32; n_freq * cfg.n_fft];
        for k in 0..n_freq {
            let kf = usize_to_f32(k);
            for n in 0..cfg.n_fft {
                let th = two_pi * kf * usize_to_f32(n) / n_fft_f;
                dft_real[k * cfg.n_fft + n] = th.cos();
                dft_imag[k * cfg.n_fft + n] = -th.sin();
            }
        }

        let sr_f = usize_to_f32(usize::try_from(cfg.sample_rate).unwrap_or(24_000));
        let fmin = cfg.fmin.max(0.0);
        let fmax = if cfg.fmax <= 0.0 {
            sr_f * 0.5
        } else {
            cfg.fmax
        };
        let mmin = hz_to_mel_slaney(fmin);
        let mmax = hz_to_mel_slaney(fmax);

        let mut hz_pts = Vec::with_capacity(cfg.n_mels + 2);
        let denom = usize_to_f32(cfg.n_mels + 1);
        for i in 0..(cfg.n_mels + 2) {
            let mel = (mmax - mmin).mul_add(usize_to_f32(i) / denom, mmin);
            hz_pts.push(mel_to_hz_slaney(mel));
        }

        let mut fft_freqs = Vec::with_capacity(n_freq);
        for k in 0..n_freq {
            fft_freqs.push(usize_to_f32(k) * sr_f / n_fft_f);
        }

        let mut mel_basis = vec![0.0_f32; cfg.n_mels * n_freq];
        for m in 0..cfg.n_mels {
            let lo = hz_pts[m];
            let md = hz_pts[m + 1];
            let hi = hz_pts[m + 2];
            let enorm = 2.0_f32 / (hi - lo).max(1e-9_f32);
            for (k, &f) in fft_freqs.iter().enumerate() {
                let up = (f - lo) / (md - lo).max(1e-9_f32);
                let down = (hi - f) / (hi - md).max(1e-9_f32);
                let w = up.min(down).max(0.0_f32);
                mel_basis[m * n_freq + k] = w * enorm;
            }
        }

        Ok(Self {
            cfg,
            n_freq,
            hann,
            dft_real,
            dft_imag,
            mel_basis,
        })
    }

    /// Extract a `[n_mels, n_frames]` channel-major (`m * n_frames + t`) log-mel spectrogram.
    ///
    /// # Errors
    /// Rejects waveforms shorter than `(n_fft - hop) / 2 + 1` or containing non-finite samples.
    pub fn extract(&self, audio: &[f32]) -> io::Result<(Vec<f32>, usize)> {
        let pad = (self.cfg.n_fft - self.cfg.hop) / 2;
        if audio.len() <= pad || !audio.iter().all(|s| s.is_finite()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Reference audio is too short or non-finite for mel extraction.",
            ));
        }
        let padded = reflect_pad_1d(audio, pad, pad)?;
        if padded.len() < self.cfg.n_fft {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Padded reference audio is shorter than n_fft.",
            ));
        }
        let n_frames = (padded.len() - self.cfg.n_fft) / self.cfg.hop + 1;
        let mut log_mel = vec![0.0_f32; self.cfg.n_mels * n_frames];
        let mut windowed = vec![0.0_f32; self.cfg.n_fft];
        let mut mag_frame = vec![0.0_f32; self.n_freq];
        let level = Level::new();

        dispatch!(level, simd => {
            for t in 0..n_frames {
                let start = t * self.cfg.hop;
                let frame = &padded[start..start + self.cfg.n_fft];
                window_frame_simd(simd, frame, &self.hann, &mut windowed);
                for (k, mag_slot) in mag_frame.iter_mut().enumerate() {
                    let re_row = &self.dft_real[k * self.cfg.n_fft..(k + 1) * self.cfg.n_fft];
                    let im_row = &self.dft_imag[k * self.cfg.n_fft..(k + 1) * self.cfg.n_fft];
                    let (re, im) = f32_dot_pair_simd(simd, &windowed, re_row, im_row);
                    *mag_slot = re.mul_add(re, im.mul_add(im, 1e-9_f32)).sqrt();
                }
                for m in 0..self.cfg.n_mels {
                    let basis_row = &self.mel_basis[m * self.n_freq..(m + 1) * self.n_freq];
                    let sum = f32_dot_simd(simd, basis_row, &mag_frame);
                    log_mel[m * n_frames + t] = sum.max(1e-5_f32).ln();
                }
            }
        });
        Ok((log_mel, n_frames))
    }
}

/// 1D reflect padding matching `torch.nn.functional.pad(mode="reflect")`.
pub fn reflect_pad_1d(input: &[f32], left: usize, right: usize) -> io::Result<Vec<f32>> {
    let len = input.len();
    if len == 0 || (left > 0 && len <= left) || (right > 0 && len <= right) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Reflect padding width must be strictly less than signal length.",
        ));
    }
    let mut out = Vec::with_capacity(left + len + right);
    for i in 0..left {
        out.push(input[left - i]);
    }
    out.extend_from_slice(input);
    for i in 0..right {
        out.push(input[len - 2 - i]);
    }
    Ok(out)
}

/// Reflect-padded `Conv1d(padding="same", mode="reflect")` weight and bias.
///
/// Weights are stored in `[out_c, in_c, kernel]` row-major order.
#[derive(Clone, Debug)]
pub struct TdnnConv1d {
    /// Output channels.
    pub out_c: usize,
    /// Input channels.
    pub in_c: usize,
    /// Odd kernel width (`1`, `3`, or `5`).
    pub kernel: usize,
    /// Dilation factor (`1`, `2`, `3`, or `4`).
    pub dilation: usize,
    /// Flattened weights `[out_c * in_c * kernel]`.
    pub weight: Vec<f32>,
    /// Bias vector `[out_c]`.
    pub bias: Vec<f32>,
}

impl TdnnConv1d {
    /// Execute reflect-padded same-length 1D convolution on `[in_c, frames]` channel-major input.
    pub fn forward(&self, input: &[f32], frames: usize, apply_relu: bool) -> io::Result<Vec<f32>> {
        if input.len() != self.in_c * frames
            || self.weight.len() != self.out_c * self.in_c * self.kernel
            || self.bias.len() != self.out_c
            || self.kernel.is_multiple_of(2)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid TDNN Conv1d dimensions.",
            ));
        }
        let pad = ((self.kernel - 1) * self.dilation) / 2;
        let padded_frames = frames + 2 * pad;
        let owned_padded;
        let padded: &[f32] = if pad == 0 {
            input
        } else {
            if frames == 0 || pad >= frames {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Reflect padding width must be strictly less than signal length.",
                ));
            }
            let mut buf = vec![0.0_f32; self.in_c * padded_frames];
            for c in 0..self.in_c {
                let row = &input[c * frames..(c + 1) * frames];
                let dst = &mut buf[c * padded_frames..(c + 1) * padded_frames];
                for i in 0..pad {
                    dst[i] = row[pad - i];
                }
                dst[pad..pad + frames].copy_from_slice(row);
                for i in 0..pad {
                    dst[pad + frames + i] = row[frames - 2 - i];
                }
            }
            owned_padded = buf;
            &owned_padded
        };

        let mut out = vec![0.0_f32; self.out_c * frames];
        let level = Level::new();
        dispatch!(level, simd => {
            for oc in 0..self.out_c {
                let b = self.bias[oc];
                let out_row = &mut out[oc * frames..(oc + 1) * frames];
                out_row.fill(b);
                if self.kernel == 1 {
                    let w_row = &self.weight[oc * self.in_c..(oc + 1) * self.in_c];
                    let mut ic = 0;
                    while ic + 4 <= self.in_c {
                        let r0 = &padded[ic * padded_frames..ic * padded_frames + frames];
                        let r1 = &padded[(ic + 1) * padded_frames..(ic + 1) * padded_frames + frames];
                        let r2 = &padded[(ic + 2) * padded_frames..(ic + 2) * padded_frames + frames];
                        let r3 = &padded[(ic + 3) * padded_frames..(ic + 3) * padded_frames + frames];
                        axpy4_simd(
                            simd,
                            [w_row[ic], w_row[ic + 1], w_row[ic + 2], w_row[ic + 3]],
                            r0,
                            r1,
                            r2,
                            r3,
                            out_row,
                        );
                        ic += 4;
                    }
                    while ic < self.in_c {
                        let in_row = &padded[ic * padded_frames..ic * padded_frames + frames];
                        axpy_simd(simd, w_row[ic], in_row, out_row);
                        ic += 1;
                    }
                } else {
                    for ic in 0..self.in_c {
                        let in_row = &padded[ic * padded_frames..(ic + 1) * padded_frames];
                        let w_base = (oc * self.in_c + ic) * self.kernel;
                        for k in 0..self.kernel {
                            let w = self.weight[w_base + k];
                            let offset = k * self.dilation;
                            axpy_simd(simd, w, &in_row[offset..offset + frames], out_row);
                        }
                    }
                }
                if apply_relu {
                    relu_in_place_simd(simd, out_row);
                }
            }
        });
        Ok(out)
    }
}

/// Squeeze-and-Excitation channel attention block (`conv1 + ReLU -> conv2 + Sigmoid`).
#[derive(Clone, Debug)]
pub struct SqueezeExcitation {
    /// Reduction 1x1 conv (`channels -> se_channels`).
    pub conv1: TdnnConv1d,
    /// Expansion 1x1 conv (`se_channels -> channels`).
    pub conv2: TdnnConv1d,
}

impl SqueezeExcitation {
    /// Apply channel-wise Squeeze-and-Excitation scaling to `[channels, frames]`.
    pub fn forward(&self, input: &[f32], frames: usize) -> io::Result<Vec<f32>> {
        let channels = self.conv1.in_c;
        if frames == 0 || input.len() != channels * frames || self.conv2.out_c != channels {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid Squeeze-and-Excitation input dimensions.",
            ));
        }
        let inv_t = 1.0_f32 / usize_to_f32(frames);
        let mut mean = vec![0.0_f32; channels];
        for c in 0..channels {
            let row = &input[c * frames..(c + 1) * frames];
            let sum: f32 = row.iter().copied().sum();
            mean[c] = sum * inv_t;
        }
        let h = self.conv1.forward(&mean, 1, true)?;
        let gate_logits = self.conv2.forward(&h, 1, false)?;
        let mut out = input.to_vec();
        for c in 0..channels {
            let gate = 1.0_f32 / (1.0_f32 + (-gate_logits[c]).exp());
            for val in &mut out[c * frames..(c + 1) * frames] {
                *val *= gate;
            }
        }
        Ok(out)
    }
}

/// SE-Res2Net block (`tdnn1 -> 8-branch dilated Res2Net -> tdnn2 -> SE + residual`).
#[derive(Clone, Debug)]
pub struct SeRes2NetBlock {
    /// Initial 1x1 TDNN.
    pub tdnn1: TdnnConv1d,
    /// `scale - 1` (`7`) dilated `k = 3` TDNN branches, each over `channels / scale` channels.
    pub branches: Vec<TdnnConv1d>,
    /// Output 1x1 TDNN.
    pub tdnn2: TdnnConv1d,
    /// Squeeze-and-Excitation gate.
    pub se: SqueezeExcitation,
    /// `Res2Net` scale factor (`8`).
    pub scale: usize,
}

impl SeRes2NetBlock {
    /// Run one SE-Res2Net block on `[channels, frames]`.
    pub fn forward(&self, input: &[f32], frames: usize) -> io::Result<Vec<f32>> {
        let channels = self.tdnn1.in_c;
        if self.scale < 2
            || !channels.is_multiple_of(self.scale)
            || self.branches.len() != self.scale - 1
            || input.len() != channels * frames
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid SE-Res2Net block configuration.",
            ));
        }
        let h = self.tdnn1.forward(input, frames, true)?;
        let chunk_c = channels / self.scale;
        let chunk_len = chunk_c * frames;
        let mut res2net_out = vec![0.0_f32; channels * frames];
        res2net_out[..chunk_len].copy_from_slice(&h[..chunk_len]);

        let mut prev = Vec::new();
        for i in 1..self.scale {
            let slice = &h[i * chunk_len..(i + 1) * chunk_len];
            let branch_in: &[f32] = if i == 1 {
                slice
            } else {
                for (dst, &src) in prev.iter_mut().zip(slice.iter()) {
                    *dst += src;
                }
                &prev
            };
            let y = self.branches[i - 1].forward(branch_in, frames, true)?;
            res2net_out[i * chunk_len..(i + 1) * chunk_len].copy_from_slice(&y);
            prev = y;
        }

        let h2 = self.tdnn2.forward(&res2net_out, frames, true)?;
        let mut gated = self.se.forward(&h2, frames)?;
        for (dst, &res) in gated.iter_mut().zip(input.iter()) {
            *dst += res;
        }
        Ok(gated)
    }
}

/// Attentive Statistical Pooling (`[C, T] -> [2*C, 1]`).
#[derive(Clone, Debug)]
pub struct AttentiveStatPool {
    /// Attention TDNN (`3*C -> attn_dim`) with `ReLU`.
    pub tdnn: TdnnConv1d,
    /// Attention 1x1 projection (`attn_dim -> C`).
    pub conv: TdnnConv1d,
}

impl AttentiveStatPool {
    /// Pool `[channels, frames]` into `[2 * channels]` weighted mean and standard deviation.
    pub fn forward(&self, input: &[f32], channels: usize, frames: usize) -> io::Result<Vec<f32>> {
        if frames == 0
            || input.len() != channels * frames
            || self.tdnn.in_c != channels * 3
            || self.conv.out_c != channels
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid AttentiveStatisticalPooling dimensions.",
            ));
        }
        let inv_t = 1.0_f32 / usize_to_f32(frames);
        let mut cat = vec![0.0_f32; channels * 3 * frames];
        cat[..channels * frames].copy_from_slice(input);

        for c in 0..channels {
            let row = &input[c * frames..(c + 1) * frames];
            let mean = row.iter().copied().sum::<f32>() * inv_t;
            let var = row
                .iter()
                .map(|&v| {
                    let d = v - mean;
                    d * d
                })
                .sum::<f32>()
                * inv_t;
            let std = (var + 1e-12_f32).sqrt();
            cat[(channels + c) * frames..(channels + c + 1) * frames].fill(mean);
            cat[(2 * channels + c) * frames..(2 * channels + c + 1) * frames].fill(std);
        }

        // Upstream applies TDNN (with ReLU), then tanh, then 1x1 conv.
        let mut attn = self.tdnn.forward(&cat, frames, true)?;
        for v in &mut attn {
            *v = v.tanh();
        }
        let scores = self.conv.forward(&attn, frames, false)?;

        let mut stats = vec![0.0_f32; channels * 2];
        for c in 0..channels {
            let s_row = &scores[c * frames..(c + 1) * frames];
            let x_row = &input[c * frames..(c + 1) * frames];
            let max_s = s_row.iter().copied().fold(f32::NEG_INFINITY, f32::max);
            let mut weights = Vec::with_capacity(frames);
            let mut w_sum = 0.0_f32;
            for &s in s_row {
                let w = (s - max_s).exp();
                weights.push(w);
                w_sum += w;
            }
            let inv_w = 1.0_f32 / w_sum.max(1e-12_f32);
            for w in &mut weights {
                *w *= inv_w;
            }
            let mut w_mean = 0.0_f32;
            for (&w, &x) in weights.iter().zip(x_row.iter()) {
                w_mean = w.mul_add(x, w_mean);
            }
            let mut w_var = 0.0_f32;
            for (&w, &x) in weights.iter().zip(x_row.iter()) {
                let d = x - w_mean;
                w_var = w.mul_add(d * d, w_var);
            }
            stats[c] = w_mean;
            stats[channels + c] = (w_var + 1e-12_f32).sqrt();
        }
        Ok(stats)
    }
}

/// Complete pure-Rust ECAPA-TDNN speaker encoder.
#[derive(Clone, Debug)]
pub struct EcapaSpeakerEncoder {
    /// Mel spectrogram frontend.
    pub mel: MelSpectrogramExtractor,
    /// Initial `k = 5` TDNN (`128 -> 512`).
    pub conv0: TdnnConv1d,
    /// Three SE-Res2Net blocks at dilations `2, 3, 4`.
    pub blocks: [SeRes2NetBlock; 3],
    /// Multi-layer feature aggregation `k = 1` TDNN (`1536 -> 1536`).
    pub mfa: TdnnConv1d,
    /// Attentive statistical pooling (`1536 -> 3072`).
    pub asp: AttentiveStatPool,
    /// Final 1x1 projection (`3072 -> 2048`).
    pub fc: TdnnConv1d,
}

impl EcapaSpeakerEncoder {
    /// Extract the 2048-dimensional speaker x-vector from a 24 kHz mono waveform.
    pub fn extract_xvector(&self, audio_24k: &[f32]) -> io::Result<Vec<f32>> {
        let (mel, frames) = self.mel.extract(audio_24k)?;
        let h0 = self.conv0.forward(&mel, frames, true)?;
        let b1 = self.blocks[0].forward(&h0, frames)?;
        let b2 = self.blocks[1].forward(&b1, frames)?;
        let b3 = self.blocks[2].forward(&b2, frames)?;

        let hidden = self.conv0.out_c;
        let mut cat = Vec::with_capacity(hidden * 3 * frames);
        cat.extend_from_slice(&b1);
        cat.extend_from_slice(&b2);
        cat.extend_from_slice(&b3);

        let mfa_out = self.mfa.forward(&cat, frames, true)?;
        let pooled = self.asp.forward(&mfa_out, hidden * 3, frames)?;
        self.fc.forward(&pooled, 1, false)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AttentiveStatPool, AudioMelConfig, EcapaSpeakerEncoder, MelSpectrogramExtractor,
        SeRes2NetBlock, SqueezeExcitation, TdnnConv1d, hz_to_mel_slaney, mel_to_hz_slaney,
        reflect_pad_1d,
    };

    #[test]
    fn slaney_mel_round_trips_across_linear_and_log_regions() {
        for hz in [0.0_f32, 250.0, 999.5, 1000.0, 2400.0, 8000.0, 12_000.0] {
            let mel = hz_to_mel_slaney(hz);
            let round = mel_to_hz_slaney(mel);
            assert!((round - hz).abs() < 1e-2, "hz={hz} round={round}");
        }
    }

    #[test]
    fn reflect_padding_and_mel_extractor_produce_finite_frames() -> std::io::Result<()> {
        let padded = reflect_pad_1d(&[1.0, 2.0, 3.0, 4.0], 2, 2)?;
        assert_eq!(padded, vec![3.0, 2.0, 1.0, 2.0, 3.0, 4.0, 3.0, 2.0]);

        let cfg = AudioMelConfig {
            sample_rate: 24_000,
            n_fft: 64,
            hop: 16,
            n_mels: 8,
            fmin: 0.0,
            fmax: 12_000.0,
        };
        let extractor = MelSpectrogramExtractor::new(cfg)?;
        let audio: Vec<f32> = (0..128)
            .map(|i| (f32::from(u8::try_from(i).unwrap_or(0)) * 0.1).sin())
            .collect();
        let (mel, frames) = extractor.extract(&audio)?;
        assert_eq!(frames, 8);
        assert_eq!(mel.len(), 8 * 8);
        assert!(mel.iter().all(|v| v.is_finite()));
        Ok(())
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "Constructs synthetic TDNN, SE-Res2Net, MFA, ASP, and FC layers in one end-to-end test."
    )]
    fn ecapa_tdnn_speaker_encoder_extracts_finite_xvector() -> std::io::Result<()> {
        let cfg = AudioMelConfig {
            sample_rate: 24_000,
            n_fft: 32,
            hop: 8,
            n_mels: 4,
            fmin: 0.0,
            fmax: 12_000.0,
        };
        let mel = MelSpectrogramExtractor::new(cfg)?;
        let hidden = 4_usize;
        let scale = 2_usize;
        let make_block = |dilation: usize| SeRes2NetBlock {
            tdnn1: TdnnConv1d {
                out_c: hidden,
                in_c: hidden,
                kernel: 1,
                dilation: 1,
                weight: vec![0.1; hidden * hidden],
                bias: vec![0.01; hidden],
            },
            branches: vec![TdnnConv1d {
                out_c: hidden / scale,
                in_c: hidden / scale,
                kernel: 3,
                dilation,
                weight: vec![0.05; (hidden / scale) * (hidden / scale) * 3],
                bias: vec![0.0; hidden / scale],
            }],
            tdnn2: TdnnConv1d {
                out_c: hidden,
                in_c: hidden,
                kernel: 1,
                dilation: 1,
                weight: vec![0.1; hidden * hidden],
                bias: vec![0.0; hidden],
            },
            se: SqueezeExcitation {
                conv1: TdnnConv1d {
                    out_c: 2,
                    in_c: hidden,
                    kernel: 1,
                    dilation: 1,
                    weight: vec![0.1; 2 * hidden],
                    bias: vec![0.0; 2],
                },
                conv2: TdnnConv1d {
                    out_c: hidden,
                    in_c: 2,
                    kernel: 1,
                    dilation: 1,
                    weight: vec![0.1; hidden * 2],
                    bias: vec![0.0; hidden],
                },
            },
            scale,
        };
        let mfa_c = hidden * 3;
        let encoder = EcapaSpeakerEncoder {
            mel,
            conv0: TdnnConv1d {
                out_c: hidden,
                in_c: 4,
                kernel: 5,
                dilation: 1,
                weight: vec![0.05; hidden * 4 * 5],
                bias: vec![0.01; hidden],
            },
            blocks: [make_block(2), make_block(3), make_block(4)],
            mfa: TdnnConv1d {
                out_c: mfa_c,
                in_c: mfa_c,
                kernel: 1,
                dilation: 1,
                weight: vec![0.05; mfa_c * mfa_c],
                bias: vec![0.0; mfa_c],
            },
            asp: AttentiveStatPool {
                tdnn: TdnnConv1d {
                    out_c: 4,
                    in_c: mfa_c * 3,
                    kernel: 1,
                    dilation: 1,
                    weight: vec![0.02; 4 * mfa_c * 3],
                    bias: vec![0.0; 4],
                },
                conv: TdnnConv1d {
                    out_c: mfa_c,
                    in_c: 4,
                    kernel: 1,
                    dilation: 1,
                    weight: vec![0.02; mfa_c * 4],
                    bias: vec![0.0; mfa_c],
                },
            },
            fc: TdnnConv1d {
                out_c: 8,
                in_c: mfa_c * 2,
                kernel: 1,
                dilation: 1,
                weight: vec![0.05; 8 * mfa_c * 2],
                bias: vec![0.01; 8],
            },
        };
        let audio: Vec<f32> = (0..64)
            .map(|i| (f32::from(u8::try_from(i).unwrap_or(0)) * 0.2).cos())
            .collect();
        let xvec = encoder.extract_xvector(&audio)?;
        assert_eq!(xvec.len(), 8);
        assert!(xvec.iter().all(|v| v.is_finite()));
        Ok(())
    }
}
