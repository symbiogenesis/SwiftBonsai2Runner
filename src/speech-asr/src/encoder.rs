//! Independent scalar Qwen3-ASR CNN and bidirectional transformer encoder.
//!
//! Three stride-two convolutions pad each chunk separately. Positional sinusoids
//! restart per chunk, padded tail rows are removed before full self-attention,
//! and the post-normalization/two-projection head emits decoder audio rows.
//! `rust-encoder.json` identifies this arithmetic and its outstanding parity gates.
#![forbid(unsafe_code)]
#![allow(
    clippy::suboptimal_flops,
    reason = "Separate scalar operations define this independent encoder reference."
)]

use crate::{
    decoder::Projection,
    frontend::{Frontend, MelSpectrogram},
    protocol::EncoderLayout,
};
use std::io::{self, Read, Seek};

#[path = "encoder/numerics.rs"]
mod numerics;

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Invalid Rust Qwen3-ASR encoder input, weights or geometry.",
    )
}
fn finite(values: &[f32]) -> io::Result<()> {
    if values.iter().all(|value| value.is_finite()) {
        Ok(())
    } else {
        Err(invalid())
    }
}
fn check(cancel: &mut impl FnMut() -> bool) -> io::Result<()> {
    if cancel() {
        Err(io::Error::from(io::ErrorKind::Interrupted))
    } else {
        Ok(())
    }
}
fn zeros(count: usize) -> io::Result<Vec<f32>> {
    if count > 128 * 1024 * 1024 {
        return Err(invalid());
    }
    let mut values = Vec::new();
    values.try_reserve_exact(count).map_err(io::Error::other)?;
    values.resize(count, 0.0);
    Ok(values)
}
#[allow(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "All admitted encoder dimensions and positions are bounded by 65,536 and exactly representable as f32."
)]
const fn scalar(value: usize) -> f32 {
    value as f32
}

/// A biased row-major linear projection.
pub struct Linear {
    /// Weights in `[output, input]` row order.
    pub weights: Projection,
    /// Per-output bias.
    pub bias: Vec<f32>,
}
impl Linear {
    fn validate(&self, input: usize, output: usize) -> io::Result<()> {
        if self.weights.dimensions() != (input, output) || self.bias.len() != output {
            return Err(invalid());
        }
        finite(&self.bias)
    }
    fn apply(&self, input: &[f32], cancel: &mut impl FnMut() -> bool) -> io::Result<Vec<f32>> {
        let mut output = self.weights.multiply(input, cancel)?;
        for (value, bias) in output.iter_mut().zip(&self.bias) {
            *value += *bias;
        }
        finite(&output)?;
        Ok(output)
    }
}

/// Layer normalization's finite per-channel scale and bias.
pub struct Norm {
    /// Channel scale.
    pub weight: Vec<f32>,
    /// Channel bias.
    pub bias: Vec<f32>,
}
impl Norm {
    fn validate(&self, width: usize) -> io::Result<()> {
        if self.weight.len() != width || self.bias.len() != width {
            return Err(invalid());
        }
        finite(&self.weight)?;
        finite(&self.bias)
    }
    fn apply(&self, row: &[f32]) -> io::Result<Vec<f32>> {
        let mean = row.iter().sum::<f32>() / scalar(row.len());
        let variance = row
            .iter()
            .map(|value| {
                let delta = *value - mean;
                delta * delta
            })
            .sum::<f32>()
            / scalar(row.len());
        if !mean.is_finite() || !variance.is_finite() {
            return Err(invalid());
        }
        let scale = 1.0 / (variance + 0.000_01).sqrt();
        let output: Vec<_> = row
            .iter()
            .zip(&self.weight)
            .zip(&self.bias)
            .map(|((value, weight), bias)| (((*value - mean) * scale) * *weight) + *bias)
            .collect();
        finite(&output)?;
        Ok(output)
    }
}

/// One pre-normalized, biased, bidirectional encoder transformer block.
pub struct Block {
    /// Pre-attention layer normalization.
    pub attention_norm: Norm,
    /// Pre-FFN layer normalization.
    pub ffn_norm: Norm,
    /// Query projection.
    pub query: Linear,
    /// Key projection.
    pub key: Linear,
    /// Value projection.
    pub value: Linear,
    /// Attention output projection.
    pub attention_out: Linear,
    /// FFN input projection before erf-based GELU.
    pub ffn_in: Linear,
    /// FFN output projection.
    pub ffn_out: Linear,
}

/// Fixed geometry from the verified encoder metadata.
#[derive(Clone, Copy)]
pub struct Config {
    /// Number of mel-frequency bins.
    pub mel_bins: usize,
    /// Output channels of each of the three convolutions.
    pub channels: usize,
    /// Transformer width, even and at least four.
    pub hidden: usize,
    /// Number of attention heads, dividing `hidden`.
    pub heads: usize,
    /// FFN intermediate width.
    pub intermediate: usize,
    /// Decoder audio-row width.
    pub output: usize,
    /// Half of the independently padded convolution chunk width.
    pub window: usize,
    /// Maximum supported sinusoidal positions per chunk.
    pub max_positions: usize,
}

/// Complete immutable encoder graph weights.
pub struct Weights {
    /// Three convolution projections, rows `[output_channel, input_channel, ky, kx]`.
    pub convolutions: [Linear; 3],
    /// Projection after channel/frequency flattening, without a bias.
    pub convolution_out: Projection,
    /// Ordered bidirectional transformer blocks.
    pub blocks: Vec<Block>,
    /// Post-transformer layer normalization.
    pub output_norm: Norm,
    /// First output projection before erf-based GELU.
    pub output_in: Linear,
    /// Final decoder-width output projection.
    pub output_out: Linear,
}

/// One admitted immutable CNN/transformer encoder.
pub struct Encoder {
    config: Config,
    weights: Weights,
}

impl Encoder {
    /// Read encoder weights and matching frontend tables from a verified GGUF.
    ///
    /// The caller owns SHA-256 verification and immutable artifact guards.
    ///
    /// # Errors
    /// Rejects bad/unsupported metadata, tensor storage, nonfinite values,
    /// cancellation or I/O/allocation failures.
    pub fn load(
        source: &mut (impl Read + Seek),
        length: u64,
        cancel: &mut impl FnMut() -> bool,
    ) -> io::Result<(Self, Frontend)> {
        crate::intake::encoder(source, length, cancel)
    }

    /// Admit every convolution, transformer and output-head shape before execution.
    ///
    /// # Errors
    /// Rejects missing/incompatible weights, unsupported geometry or nonfinite biases.
    pub fn new(config: Config, weights: Weights) -> io::Result<Self> {
        let c = config;
        if !(1..=128).contains(&c.mel_bins)
            || !(1..=1024).contains(&c.channels)
            || !(4..=8192).contains(&c.hidden)
            || !c.hidden.is_multiple_of(2)
            || c.heads == 0
            || c.heads > 128
            || !c.hidden.is_multiple_of(c.heads)
            || !(1..=65_536).contains(&c.intermediate)
            || !(1..=8192).contains(&c.output)
            || !(1..=256).contains(&weights.blocks.len())
        {
            return Err(invalid());
        }
        let layout = EncoderLayout::new(1, c.window)?;
        if layout.rows_per_chunk > c.max_positions {
            return Err(invalid());
        }
        for (layer, convolution) in weights.convolutions.iter().enumerate() {
            convolution.validate(9 * if layer == 0 { 1 } else { c.channels }, c.channels)?;
        }
        if weights.convolution_out.dimensions() != (c.channels * c.mel_bins.div_ceil(8), c.hidden) {
            return Err(invalid());
        }
        for block in &weights.blocks {
            block.attention_norm.validate(c.hidden)?;
            block.ffn_norm.validate(c.hidden)?;
            for linear in [&block.query, &block.key, &block.value, &block.attention_out] {
                linear.validate(c.hidden, c.hidden)?;
            }
            block.ffn_in.validate(c.hidden, c.intermediate)?;
            block.ffn_out.validate(c.intermediate, c.hidden)?;
        }
        weights.output_norm.validate(c.hidden)?;
        weights.output_in.validate(c.hidden, c.hidden)?;
        weights.output_out.validate(c.hidden, c.output)?;
        Ok(Self { config, weights })
    }

    /// Compute frontend features and every encoder row for one bounded clip.
    ///
    /// # Errors
    /// Returns frontend validation/cancellation errors or [`Self::encode`] errors.
    pub fn encode_pcm(
        &self,
        frontend: &Frontend,
        pcm: &[f32],
        cancel: &mut impl FnMut() -> bool,
    ) -> io::Result<Encoded> {
        let mel = frontend.compute(pcm, &mut *cancel)?;
        self.encode(&mel, cancel)
    }

    /// Encode a complete bin-major normalized mel matrix from the Rust frontend.
    ///
    /// # Errors
    /// Rejects wrong/nonfinite input, unsupported geometry, cancellation,
    /// allocation failure or nonfinite intermediate output. No partial rows escape.
    pub fn encode(
        &self,
        mel: &MelSpectrogram,
        cancel: &mut impl FnMut() -> bool,
    ) -> io::Result<Encoded> {
        if self.config.mel_bins != crate::frontend::MEL_BINS {
            return Err(invalid());
        }
        self.encode_mel(mel.values(), mel.frames(), cancel)
    }

    /// Encode bin-major features under the already admitted model geometry.
    ///
    /// This low-level entry point also admits miniature mathematical controls;
    /// production callers use [`Self::encode_pcm`] with the matching frontend.
    ///
    /// # Errors
    /// Has the same bounded transactional output contract as [`Self::encode`].
    pub fn encode_mel(
        &self,
        mel: &[f32],
        frames: usize,
        cancel: &mut impl FnMut() -> bool,
    ) -> io::Result<Encoded> {
        let c = self.config;
        let layout = EncoderLayout::new(frames, c.window)?;
        if mel.len() != c.mel_bins * frames {
            return Err(invalid());
        }
        finite(mel)?;
        check(cancel)?;
        let mut rows = Vec::new();
        rows.try_reserve_exact(layout.audio_rows)
            .map_err(io::Error::other)?;
        for chunk in 0..layout.chunks {
            check(cancel)?;
            let real = if chunk + 1 == layout.chunks {
                layout.last_chunk_mel
            } else {
                layout.mel_per_chunk
            };
            let mut image = zeros(c.mel_bins * layout.mel_per_chunk)?;
            for frequency in 0..c.mel_bins {
                let start = frequency * frames + chunk * layout.mel_per_chunk;
                image[frequency * layout.mel_per_chunk..frequency * layout.mel_per_chunk + real]
                    .copy_from_slice(&mel[start..start + real]);
            }
            let (mut width, mut height, mut channels) = (layout.mel_per_chunk, c.mel_bins, 1);
            for convolution in &self.weights.convolutions {
                image = convolve(convolution, &image, width, height, channels, cancel)?;
                width = width.div_ceil(2);
                height = height.div_ceil(2);
                channels = c.channels;
            }
            let retained = if chunk + 1 == layout.chunks {
                layout.last_chunk_rows
            } else {
                layout.rows_per_chunk
            };
            for time in 0..retained {
                check(cancel)?;
                let mut flattened = zeros(channels * height)?;
                for channel in 0..channels {
                    for frequency in 0..height {
                        flattened[channel * height + frequency] =
                            image[(channel * height + frequency) * width + time];
                    }
                }
                let mut row = self.weights.convolution_out.multiply(&flattened, cancel)?;
                position_add(&mut row, time)?;
                rows.push(row);
            }
        }
        for block in &self.weights.blocks {
            rows = transform(block, &rows, c, cancel)?;
        }
        let mut values = Vec::new();
        values
            .try_reserve_exact(layout.audio_rows * c.output)
            .map_err(io::Error::other)?;
        for row in &rows {
            check(cancel)?;
            let normalized = self.weights.output_norm.apply(row)?;
            let mut projected = self.weights.output_in.apply(&normalized, cancel)?;
            gelu(&mut projected)?;
            values.extend(self.weights.output_out.apply(&projected, cancel)?);
        }
        Ok(Encoded {
            rows: layout.audio_rows,
            width: c.output,
            values,
        })
    }
}

/// Complete time-major audio embeddings for decoder placeholder replacement.
pub struct Encoded {
    rows: usize,
    width: usize,
    values: Vec<f32>,
}
impl Encoded {
    /// Number of real encoder time rows, after independent chunk-tail trimming.
    #[must_use]
    pub const fn rows(&self) -> usize {
        self.rows
    }
    /// Number of features in each decoder audio row.
    #[must_use]
    pub const fn width(&self) -> usize {
        self.width
    }
    /// Borrow `[time, feature]` values without transferring mutable ownership.
    #[must_use]
    pub fn values(&self) -> &[f32] {
        &self.values
    }
}

fn convolve(
    linear: &Linear,
    input: &[f32],
    width: usize,
    height: usize,
    channels: usize,
    cancel: &mut impl FnMut() -> bool,
) -> io::Result<Vec<f32>> {
    let (out_width, out_height) = (width.div_ceil(2), height.div_ceil(2));
    let out_channels = linear.bias.len();
    let mut output = zeros(out_width * out_height * out_channels)?;
    let mut patch = zeros(9 * channels)?;
    for y in 0..out_height {
        for x in 0..out_width {
            check(cancel)?;
            patch.fill(0.0);
            for channel in 0..channels {
                for ky in 0..3 {
                    for kx in 0..3 {
                        let px = (2 * x + kx).checked_sub(1);
                        let py = (2 * y + ky).checked_sub(1);
                        if let (Some(px), Some(py)) = (px, py)
                            && px < width
                            && py < height
                        {
                            patch[(channel * 3 + ky) * 3 + kx] =
                                input[(channel * height + py) * width + px];
                        }
                    }
                }
            }
            let mut values = linear.apply(&patch, cancel)?;
            gelu(&mut values)?;
            for (channel, value) in values.into_iter().enumerate() {
                output[(channel * out_height + y) * out_width + x] = value;
            }
        }
    }
    Ok(output)
}

fn gelu(values: &mut [f32]) -> io::Result<()> {
    for value in &mut *values {
        *value = numerics::gelu(*value);
    }
    finite(values)
}

#[allow(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "The pinned sinusoid constructs f64 angles and explicitly rounds sin/cos to f32; bounded dimensions convert exactly."
)]
fn position_add(row: &mut [f32], position: usize) -> io::Result<()> {
    let half = row.len() / 2;
    let logarithm = 10_000.0_f64.ln() / (half - 1) as f64;
    for index in 0..half {
        let angle = position as f64 * (-logarithm * index as f64).exp();
        row[index] += angle.sin() as f32;
        row[index + half] += angle.cos() as f32;
    }
    finite(row)
}

fn transform(
    block: &Block,
    rows: &[Vec<f32>],
    c: Config,
    cancel: &mut impl FnMut() -> bool,
) -> io::Result<Vec<Vec<f32>>> {
    let mut queries = Vec::new();
    let mut keys = Vec::new();
    let mut values = Vec::new();
    for row in rows {
        check(cancel)?;
        let norm = block.attention_norm.apply(row)?;
        queries.push(block.query.apply(&norm, cancel)?);
        keys.push(block.key.apply(&norm, cancel)?);
        values.push(block.value.apply(&norm, cancel)?);
    }
    let mut output = Vec::new();
    output
        .try_reserve_exact(rows.len())
        .map_err(io::Error::other)?;
    for (index, row) in rows.iter().enumerate() {
        let context = attention(&queries[index], &keys, &values, c.heads, cancel)?;
        let projected = block.attention_out.apply(&context, cancel)?;
        let mut residual: Vec<_> = row.iter().zip(projected).map(|(a, b)| *a + b).collect();
        finite(&residual)?;
        let norm = block.ffn_norm.apply(&residual)?;
        let mut intermediate = block.ffn_in.apply(&norm, cancel)?;
        gelu(&mut intermediate)?;
        let projected = block.ffn_out.apply(&intermediate, cancel)?;
        for (value, add) in residual.iter_mut().zip(projected) {
            *value += add;
        }
        finite(&residual)?;
        output.push(residual);
    }
    Ok(output)
}

fn attention(
    query: &[f32],
    keys: &[Vec<f32>],
    values: &[Vec<f32>],
    heads: usize,
    cancel: &mut impl FnMut() -> bool,
) -> io::Result<Vec<f32>> {
    let width = query.len() / heads;
    let mut output = zeros(query.len())?;
    let mut scores = zeros(keys.len())?;
    for head in 0..heads {
        check(cancel)?;
        let range = head * width..(head + 1) * width;
        for (score, key) in scores.iter_mut().zip(keys) {
            *score = query[range.clone()]
                .iter()
                .zip(&key[range.clone()])
                .map(|(q, k)| *q * *k)
                .sum::<f32>()
                / scalar(width).sqrt();
        }
        finite(&scores)?;
        let max = scores.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let mut denominator = 0.0;
        for score in &mut scores {
            *score = (*score - max).exp();
            denominator += *score;
        }
        if denominator <= 0.0 || !denominator.is_finite() {
            return Err(invalid());
        }
        for (score, value) in scores.iter().zip(values) {
            for (target, v) in output[range.clone()].iter_mut().zip(&value[range.clone()]) {
                *target += (*score / denominator) * *v;
            }
        }
    }
    finite(&output)?;
    Ok(output)
}

#[cfg(test)]
#[path = "encoder/tests.rs"]
mod tests;
