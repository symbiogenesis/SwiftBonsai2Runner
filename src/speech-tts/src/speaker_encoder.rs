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
#![allow(
    clippy::missing_errors_doc,
    reason = "Internal spectrogram and TDNN shape checks return io::ErrorKind::InvalidInput."
)]

use std::io;

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
        let mut mag = vec![0.0_f32; self.n_freq * n_frames];

        for t in 0..n_frames {
            let start = t * self.cfg.hop;
            let frame = &padded[start..start + self.cfg.n_fft];
            for k in 0..self.n_freq {
                let re_row = &self.dft_real[k * self.cfg.n_fft..(k + 1) * self.cfg.n_fft];
                let im_row = &self.dft_imag[k * self.cfg.n_fft..(k + 1) * self.cfg.n_fft];
                let mut re = 0.0_f32;
                let mut im = 0.0_f32;
                for n in 0..self.cfg.n_fft {
                    let w_x = frame[n] * self.hann[n];
                    re = w_x.mul_add(re_row[n], re);
                    im = w_x.mul_add(im_row[n], im);
                }
                mag[k * n_frames + t] = re.mul_add(re, im.mul_add(im, 1e-9_f32)).sqrt();
            }
        }

        let mut log_mel = vec![0.0_f32; self.cfg.n_mels * n_frames];
        for m in 0..self.cfg.n_mels {
            let basis_row = &self.mel_basis[m * self.n_freq..(m + 1) * self.n_freq];
            for t in 0..n_frames {
                let mut sum = 0.0_f32;
                for k in 0..self.n_freq {
                    sum = basis_row[k].mul_add(mag[k * n_frames + t], sum);
                }
                log_mel[m * n_frames + t] = sum.max(1e-5_f32).ln();
            }
        }
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
            || self.kernel % 2 == 0
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid TDNN Conv1d dimensions.",
            ));
        }
        let pad = ((self.kernel - 1) * self.dilation) / 2;
        let padded_frames = frames + 2 * pad;
        let mut padded = vec![0.0_f32; self.in_c * padded_frames];
        for c in 0..self.in_c {
            let row = &input[c * frames..(c + 1) * frames];
            let p_row = if pad > 0 {
                reflect_pad_1d(row, pad, pad)?
            } else {
                row.to_vec()
            };
            padded[c * padded_frames..(c + 1) * padded_frames].copy_from_slice(&p_row);
        }

        let mut out = vec![0.0_f32; self.out_c * frames];
        for oc in 0..self.out_c {
            let b = self.bias[oc];
            let out_row = &mut out[oc * frames..(oc + 1) * frames];
            out_row.fill(b);
            for ic in 0..self.in_c {
                let in_row = &padded[ic * padded_frames..(ic + 1) * padded_frames];
                let w_base = (oc * self.in_c + ic) * self.kernel;
                for k in 0..self.kernel {
                    let w = self.weight[w_base + k];
                    let offset = k * self.dilation;
                    for t in 0..frames {
                        out_row[t] = w.mul_add(in_row[t + offset], out_row[t]);
                    }
                }
            }
            if apply_relu {
                for val in out_row.iter_mut() {
                    if *val < 0.0_f32 {
                        *val = 0.0_f32;
                    }
                }
            }
        }
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

        let mut prev = vec![0.0_f32; chunk_len];
        for i in 1..self.scale {
            let slice = &h[i * chunk_len..(i + 1) * chunk_len];
            let branch_in = if i == 1 {
                slice.to_vec()
            } else {
                slice
                    .iter()
                    .zip(prev.iter())
                    .map(|(&a, &b)| a + b)
                    .collect()
            };
            let y = self.branches[i - 1].forward(&branch_in, frames, true)?;
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
    #[allow(
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
