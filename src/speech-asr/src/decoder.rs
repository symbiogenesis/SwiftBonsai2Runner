//! Independent scalar Qwen3-ASR causal decoder and transactional session state.
//!
//! The reviewed graph is described in `rust-decoder.json`. This implementation
//! exposes token/audio prefill, GQA, per-head Q/K `RMSNorm`, `NeoX` `RoPE`, `SwiGLU`,
//! tied output projection and complete cache reset. Its explicitly separate
//! scalar arithmetic is not a claim of native quantized/flash-attention parity.
//! Production continues to select the native engine until encoder, weight
//! intake, independent numerical and actual-model acceptance are complete.
#![forbid(unsafe_code)]
#![allow(
    clippy::suboptimal_flops,
    reason = "Separate scalar operations define this independent reference's accumulation order."
)]

use crate::protocol::Prompt;
use std::{
    io::{self, Read, Seek},
    sync::Arc,
};

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Invalid Rust Qwen3-ASR decoder input, weights or state.",
    )
}

fn check(cancel: &mut impl FnMut() -> bool) -> io::Result<()> {
    if cancel() {
        Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Rust Qwen3-ASR decoder cancelled.",
        ))
    } else {
        Ok(())
    }
}

fn zeros(count: usize) -> io::Result<Vec<f32>> {
    let mut result = Vec::new();
    result.try_reserve_exact(count).map_err(io::Error::other)?;
    result.resize(count, 0.0);
    Ok(result)
}

fn finite(values: &[f32]) -> io::Result<()> {
    if values.iter().all(|value| value.is_finite()) {
        Ok(())
    } else {
        Err(invalid())
    }
}

/// GGUF row-major F32 or `Q8_0` projection, retaining compressed Q8 weights.
pub struct Projection {
    input: usize,
    output: usize,
    data: Storage,
}

enum Storage {
    Float(Vec<f32>),
    Q8(Vec<u8>),
}

impl Projection {
    /// Return `(input width, output rows)` for checked graph construction.
    #[must_use]
    pub const fn dimensions(&self) -> (usize, usize) {
        (self.input, self.output)
    }
    /// Admit little-endian IEEE binary16 rows, expanding only this projection.
    ///
    /// # Errors
    /// Rejects invalid shape, incomplete bytes, allocation failure or nonfinite weights.
    pub fn f16(input: usize, output: usize, bytes: &[u8]) -> io::Result<Self> {
        let count = extent(input, output)?;
        if bytes.len() != count * 2 {
            return Err(invalid());
        }
        let mut values = zeros(count)?;
        for (value, bytes) in values.iter_mut().zip(bytes.as_chunks::<2>().0) {
            *value = half(u16::from_le_bytes([bytes[0], bytes[1]]))?;
        }
        Self::f32(input, output, values)
    }
    /// Admit finite F32 values in `[output, input]` row order.
    ///
    /// # Errors
    /// Rejects invalid shape, nonfinite data or more than 8 GiB of values.
    pub fn f32(input: usize, output: usize, data: Vec<f32>) -> io::Result<Self> {
        let count = extent(input, output)?;
        if count != data.len() {
            return Err(invalid());
        }
        finite(&data)?;
        Ok(Self {
            input,
            output,
            data: Storage::Float(data),
        })
    }

    /// Admit `Q8_0` blocks: little-endian half scale followed by 32 signed bytes.
    ///
    /// Activations stay F32; this is a separately identified scalar path rather
    /// than GGML's activation-requantizing optimized matrix multiplication.
    ///
    /// # Errors
    /// Rejects incomplete blocks, wrong byte length or nonfinite half scales.
    pub fn q8(input: usize, output: usize, data: Vec<u8>) -> io::Result<Self> {
        let count = extent(input, output)?;
        if !input.is_multiple_of(32) || data.len() != count / 32 * 34 {
            return Err(invalid());
        }
        for block in data.as_chunks::<34>().0 {
            let _ = half(u16::from_le_bytes([block[0], block[1]]))?;
        }
        Ok(Self {
            input,
            output,
            data: Storage::Q8(data),
        })
    }

    fn row(&self, row: usize) -> io::Result<Vec<f32>> {
        if row >= self.output {
            return Err(invalid());
        }
        let mut output = zeros(self.input)?;
        match &self.data {
            Storage::Float(values) => {
                output.copy_from_slice(&values[row * self.input..(row + 1) * self.input]);
            }
            Storage::Q8(bytes) => {
                let row_bytes = self.input / 32 * 34;
                for (block, target) in bytes[row * row_bytes..(row + 1) * row_bytes]
                    .as_chunks::<34>()
                    .0
                    .iter()
                    .zip(output.as_chunks_mut::<32>().0)
                {
                    let scale = half(u16::from_le_bytes([block[0], block[1]]))?;
                    for (value, byte) in target.iter_mut().zip(&block[2..]) {
                        *value = scale * f32::from(i8::from_le_bytes([*byte]));
                    }
                }
            }
        }
        finite(&output)?;
        Ok(output)
    }

    /// Compute every output row with a cancellation check before each row.
    ///
    /// # Errors
    /// Rejects bad/nonfinite input, nonfinite output, allocation failure or cancellation.
    pub fn multiply(
        &self,
        input: &[f32],
        cancel: &mut impl FnMut() -> bool,
    ) -> io::Result<Vec<f32>> {
        if input.len() != self.input {
            return Err(invalid());
        }
        finite(input)?;
        let mut output = zeros(self.output)?;
        for (row, target) in output.iter_mut().enumerate() {
            check(cancel)?;
            match &self.data {
                Storage::Float(data) => {
                    for (weight, activation) in data[row * self.input..(row + 1) * self.input]
                        .iter()
                        .zip(input)
                    {
                        *target += *weight * *activation;
                    }
                }
                Storage::Q8(data) => {
                    let row_bytes = self.input / 32 * 34;
                    for (block, activations) in data[row * row_bytes..(row + 1) * row_bytes]
                        .as_chunks::<34>()
                        .0
                        .iter()
                        .zip(input.as_chunks::<32>().0)
                    {
                        let scale = half(u16::from_le_bytes([block[0], block[1]]))?;
                        for (byte, activation) in block[2..].iter().zip(activations) {
                            *target +=
                                (scale * f32::from(i8::from_le_bytes([*byte]))) * *activation;
                        }
                    }
                }
            }
        }
        finite(&output)?;
        Ok(output)
    }

    const fn shape(&self, input: usize, output: usize) -> bool {
        self.input == input && self.output == output
    }
}

fn extent(input: usize, output: usize) -> io::Result<usize> {
    input
        .checked_mul(output)
        .filter(|count| input > 0 && output > 0 && *count <= 2 * 1024 * 1024 * 1024)
        .ok_or_else(invalid)
}

fn half(bits: u16) -> io::Result<f32> {
    let sign = u32::from(bits & 0x8000) << 16;
    let exponent = (bits >> 10) & 31;
    let mantissa = bits & 1023;
    if exponent == 31 {
        return Err(invalid());
    }
    Ok(if exponent == 0 {
        let value = f32::from(mantissa) * 0.000_000_059_604_645;
        if sign == 0 { value } else { -value }
    } else {
        f32::from_bits(sign | ((u32::from(exponent) + 112) << 23) | (u32::from(mantissa) << 13))
    })
}

/// Cache scalar policy, selected explicitly when creating a decoder model.
#[derive(Clone, Copy, Debug)]
pub enum CachePrecision {
    /// Preserve F32 K/V bits.
    F32,
    /// Round K/V to IEEE binary16 with nearest-even ties before attention.
    F16,
}

fn cache_value(value: f32, precision: CachePrecision) -> io::Result<f32> {
    if !value.is_finite() {
        return Err(invalid());
    }
    if matches!(precision, CachePrecision::F32) {
        return Ok(value);
    }
    let bits = value.to_bits();
    let sign = bits & 0x8000_0000;
    let exponent = (bits >> 23) & 255;
    if exponent < 102 {
        return Ok(f32::from_bits(sign));
    }
    if exponent < 113 {
        let shift = 126 - exponent;
        let significand = (bits & 0x007f_ffff) | 0x0080_0000;
        let truncated = significand >> shift;
        let remainder = significand & ((1 << shift) - 1);
        let midpoint = 1 << (shift - 1);
        let rounded = truncated
            + u32::from(remainder > midpoint || (remainder == midpoint && truncated & 1 != 0));
        let rounded =
            f32::from(u16::try_from(rounded).map_err(|_| invalid())?) * 0.000_000_059_604_645;
        return Ok(if sign == 0 { rounded } else { -rounded });
    }
    let rounded = bits.wrapping_add(0x0fff + ((bits >> 13) & 1)) & 0xffff_e000;
    let result = f32::from_bits(rounded);
    if exponent > 142 || result.abs() > 65_504.0 || !result.is_finite() {
        Err(invalid())
    } else {
        Ok(result)
    }
}

/// Reviewed single-stream geometry for a Qwen3 causal decoder.
#[derive(Clone, Copy)]
pub struct Config {
    /// Residual-stream and audio-row width.
    pub hidden: usize,
    /// Number of query heads.
    pub heads: usize,
    /// Number of shared key/value heads.
    pub kv_heads: usize,
    /// Even number of values per attention head.
    pub head_dim: usize,
    /// `SwiGLU` intermediate width.
    pub intermediate: usize,
    /// Bounded session context, at most 8,192 positions.
    pub context: usize,
    /// Positive finite RMS normalization epsilon.
    pub epsilon: f32,
    /// Positive finite `NeoX` rotation base.
    pub rope_theta: f32,
    /// Explicit K/V storage rounding policy.
    pub cache_precision: CachePrecision,
}

/// One complete bias-free Qwen3 block with per-head Q/K normalization.
pub struct Block {
    /// Input `RMSNorm` weights, `[hidden]`.
    pub attention_norm: Vec<f32>,
    /// Post-attention `RMSNorm` weights, `[hidden]`.
    pub ffn_norm: Vec<f32>,
    /// Q projection, `[heads * head_dim, hidden]`.
    pub query: Projection,
    /// K projection, `[kv_heads * head_dim, hidden]`.
    pub key: Projection,
    /// V projection, `[kv_heads * head_dim, hidden]`.
    pub value: Projection,
    /// Attention output projection, `[hidden, heads * head_dim]`.
    pub attention_out: Projection,
    /// Q `RMSNorm` weights, shared across heads, `[head_dim]`.
    pub query_norm: Vec<f32>,
    /// K `RMSNorm` weights, shared across heads, `[head_dim]`.
    pub key_norm: Vec<f32>,
    /// `SwiGLU` gate projection, `[intermediate, hidden]`.
    pub gate: Projection,
    /// `SwiGLU` up projection, `[intermediate, hidden]`.
    pub up: Projection,
    /// `SwiGLU` down projection, `[hidden, intermediate]`.
    pub down: Projection,
}

/// Immutable admitted decoder weights, sharing the embedding and output head.
pub struct Model {
    config: Config,
    embedding: Projection,
    blocks: Vec<Block>,
    output_norm: Vec<f32>,
}

impl Model {
    /// Return the immutable residual width, vocabulary and bounded context.
    #[must_use]
    pub const fn dimensions(&self) -> (usize, usize, usize) {
        (
            self.config.hidden,
            self.embedding.output,
            self.config.context,
        )
    }
    /// Load the complete decoder from an admitted Qwen3-ASR GGUF index.
    ///
    /// The caller owns digest verification and immutable artifact guards. This
    /// reads decoder weights only, accepting `F32/F16/Q8_0` projections and F32
    /// normalization vectors. It does not execute inference or an encoder.
    ///
    /// # Errors
    /// Rejects malformed/unsupported tensors, scalar geometry, nonfinite weights,
    /// cancellation and I/O/allocation failures.
    pub fn load(
        source: &mut (impl Read + Seek),
        length: u64,
        precision: CachePrecision,
        cancel: &mut impl FnMut() -> bool,
    ) -> io::Result<Self> {
        crate::intake::decoder(source, length, precision, cancel)
    }
    /// Validate all layer dimensions before admitting a decoder session.
    ///
    /// # Errors
    /// Rejects unsupported/overflowing geometry, nonfinite normalization weights,
    /// invalid scalar parameters, untied/incorrect embedding or incomplete layers.
    pub fn new(
        config: Config,
        embedding: Projection,
        blocks: Vec<Block>,
        output_norm: Vec<f32>,
    ) -> io::Result<Self> {
        let c = config;
        if !(1..=8192).contains(&c.hidden)
            || !(1..=128).contains(&c.heads)
            || c.kv_heads == 0
            || !c.heads.is_multiple_of(c.kv_heads)
            || c.head_dim == 0
            || c.head_dim > 256
            || !c.head_dim.is_multiple_of(2)
            || !(1..=65_536).contains(&c.intermediate)
            || !(1..=8192).contains(&c.context)
            || !(1..=128).contains(&blocks.len())
            || embedding.input != c.hidden
            || embedding.output > 262_144
            || !c.epsilon.is_finite()
            || c.epsilon <= 0.0
            || !c.rope_theta.is_finite()
            || c.rope_theta <= 0.0
        {
            return Err(invalid());
        }
        norm_weights(&output_norm, c.hidden)?;
        for block in &blocks {
            norm_weights(&block.attention_norm, c.hidden)?;
            norm_weights(&block.ffn_norm, c.hidden)?;
            norm_weights(&block.query_norm, c.head_dim)?;
            norm_weights(&block.key_norm, c.head_dim)?;
            if !block.query.shape(c.hidden, c.heads * c.head_dim)
                || !block.key.shape(c.hidden, c.kv_heads * c.head_dim)
                || !block.value.shape(c.hidden, c.kv_heads * c.head_dim)
                || !block.attention_out.shape(c.heads * c.head_dim, c.hidden)
                || !block.gate.shape(c.hidden, c.intermediate)
                || !block.up.shape(c.hidden, c.intermediate)
                || !block.down.shape(c.intermediate, c.hidden)
            {
                return Err(invalid());
            }
        }
        Ok(Self {
            config,
            embedding,
            blocks,
            output_norm,
        })
    }

    /// Create an empty session bound to this exact immutable model owner.
    #[must_use]
    pub fn session(self: &Arc<Self>) -> Session {
        Session {
            model: Arc::clone(self),
            position: 0,
            layers: (0..self.blocks.len())
                .map(|_| LayerState {
                    keys: Vec::new(),
                    values: Vec::new(),
                })
                .collect(),
        }
    }
}

struct LayerState {
    keys: Vec<f32>,
    values: Vec<f32>,
}

/// One isolated mutable decoder session; no cache is shared between sessions.
pub struct Session {
    model: Arc<Model>,
    position: usize,
    layers: Vec<LayerState>,
}

impl Session {
    /// Number of successfully published token/audio positions.
    #[must_use]
    pub const fn position(&self) -> usize {
        self.position
    }

    /// Borrow complete layer K/V in `[position, kv_head, head_dim]` order.
    #[must_use]
    pub fn cache(&self, layer: usize) -> Option<(&[f32], &[f32])> {
        self.layers
            .get(layer)
            .map(|state| (state.keys.as_slice(), state.values.as_slice()))
    }

    /// Discard every layer's K/V and the position, preserving reusable capacity.
    pub fn reset(&mut self) {
        for layer in &mut self.layers {
            layer.keys.clear();
            layer.values.clear();
        }
        self.position = 0;
    }

    /// Decode a vocabulary embedding and publish one complete position.
    ///
    /// # Errors
    /// Rejects invalid token IDs or the conditions from [`Self::step_audio`].
    pub fn step(&mut self, id: u32, cancel: &mut impl FnMut() -> bool) -> io::Result<Vec<f32>> {
        check(cancel)?;
        let row = self
            .model
            .embedding
            .row(usize::try_from(id).map_err(|_| invalid())?)?;
        self.step_audio(&row, cancel)
    }

    /// Decode one encoder row or token embedding, then return tied-head logits.
    ///
    /// Every layer's K/V remains provisional until all layers and logits are
    /// finite and cancellation is checked. Errors and unwinding preserve the
    /// exact previously committed cache and position; no partial append escapes.
    ///
    /// # Errors
    /// Rejects wrong/nonfinite rows, full context, allocation faults, arithmetic
    /// overflow/nonfinite output and cancellation.
    pub fn step_audio(
        &mut self,
        row: &[f32],
        cancel: &mut impl FnMut() -> bool,
    ) -> io::Result<Vec<f32>> {
        let c = self.model.config;
        if self.position >= c.context || row.len() != c.hidden {
            return Err(invalid());
        }
        finite(row)?;
        check(cancel)?;
        let mut hidden = row.to_vec();
        let mut provisional = Vec::new();
        provisional
            .try_reserve_exact(self.layers.len())
            .map_err(io::Error::other)?;
        for (block, state) in self.model.blocks.iter().zip(&self.layers) {
            check(cancel)?;
            let (output, keys, values) = forward(block, state, &hidden, c, self.position, cancel)?;
            hidden = output;
            provisional.push((keys, values));
        }
        let normalized = rms(&hidden, &self.model.output_norm, c.epsilon)?;
        let logits = self.model.embedding.multiply(&normalized, cancel)?;
        check(cancel)?;
        for (state, (keys, values)) in self.layers.iter_mut().zip(&provisional) {
            state
                .keys
                .try_reserve_exact(keys.len())
                .map_err(io::Error::other)?;
            state
                .values
                .try_reserve_exact(values.len())
                .map_err(io::Error::other)?;
        }
        for (state, (keys, values)) in self.layers.iter_mut().zip(provisional) {
            state.keys.extend(keys);
            state.values.extend(values);
        }
        self.position += 1;
        Ok(logits)
    }

    /// Start a fresh prompt, replacing only its contiguous audio placeholder rows.
    ///
    /// On failure or unwind the entire attempted prompt is reset, including
    /// successful earlier rows. On success the complete prompt remains cached.
    ///
    /// # Errors
    /// Rejects audio geometry, context or any token/step failure.
    pub fn prefill(
        &mut self,
        prompt: &Prompt,
        audio: &[f32],
        cancel: &mut impl FnMut() -> bool,
    ) -> io::Result<Vec<f32>> {
        self.reset();
        let mut guard = ResetGuard {
            session: self,
            success: false,
        };
        let range = prompt.audio_rows();
        let hidden = guard.session.model.config.hidden;
        if audio.len() != range.len().checked_mul(hidden).ok_or_else(invalid)?
            || prompt.ids().len() > guard.session.model.config.context
        {
            return Err(invalid());
        }
        finite(audio)?;
        let mut logits = Vec::new();
        for (position, id) in prompt.ids().iter().enumerate() {
            logits = if range.contains(&position) {
                let start = (position - range.start) * hidden;
                guard
                    .session
                    .step_audio(&audio[start..start + hidden], cancel)?
            } else {
                guard.session.step(*id, cancel)?
            };
        }
        guard.success = true;
        Ok(logits)
    }
}

struct ResetGuard<'a> {
    session: &'a mut Session,
    success: bool,
}
impl Drop for ResetGuard<'_> {
    fn drop(&mut self) {
        if !self.success {
            self.session.reset();
        }
    }
}

fn norm_weights(weights: &[f32], width: usize) -> io::Result<()> {
    if weights.len() != width {
        return Err(invalid());
    }
    finite(weights)
}

#[allow(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "Admitted decoder dimensions and positions are at most 65,536 and exactly representable in f32."
)]
const fn scalar(value: usize) -> f32 {
    value as f32
}

fn rms(input: &[f32], weights: &[f32], epsilon: f32) -> io::Result<Vec<f32>> {
    let mut squares = 0.0_f32;
    for value in input {
        squares += *value * *value;
    }
    if !squares.is_finite() || !(squares / scalar(input.len()) + epsilon).is_finite() {
        return Err(invalid());
    }
    let scale = 1.0 / (squares / scalar(input.len()) + epsilon).sqrt();
    let output: Vec<_> = input
        .iter()
        .zip(weights)
        .map(|(value, weight)| (*value * scale) * *weight)
        .collect();
    finite(&output)?;
    Ok(output)
}

fn normalize_heads(values: &mut [f32], weights: &[f32], c: Config) -> io::Result<()> {
    for head in values.chunks_exact_mut(c.head_dim) {
        head.copy_from_slice(&rms(head, weights, c.epsilon)?);
    }
    Ok(())
}

fn rope(values: &mut [f32], c: Config, position: usize) -> io::Result<()> {
    for head in values.chunks_exact_mut(c.head_dim) {
        for index in 0..c.head_dim / 2 {
            let angle =
                scalar(position) * c.rope_theta.powf(-scalar(2 * index) / scalar(c.head_dim));
            let (sin, cos) = angle.sin_cos();
            let (a, b) = (head[index], head[index + c.head_dim / 2]);
            head[index] = a * cos - b * sin;
            head[index + c.head_dim / 2] = a * sin + b * cos;
        }
    }
    finite(values)
}

fn forward(
    block: &Block,
    state: &LayerState,
    input: &[f32],
    c: Config,
    position: usize,
    cancel: &mut impl FnMut() -> bool,
) -> io::Result<(Vec<f32>, Vec<f32>, Vec<f32>)> {
    let norm = rms(input, &block.attention_norm, c.epsilon)?;
    let mut query = block.query.multiply(&norm, cancel)?;
    let mut keys = block.key.multiply(&norm, cancel)?;
    let mut values = block.value.multiply(&norm, cancel)?;
    normalize_heads(&mut query, &block.query_norm, c)?;
    normalize_heads(&mut keys, &block.key_norm, c)?;
    rope(&mut query, c, position)?;
    rope(&mut keys, c, position)?;
    for value in keys.iter_mut().chain(&mut values) {
        *value = cache_value(*value, c.cache_precision)?;
    }
    let attention = attend(&query, &keys, &values, state, c, position, cancel)?;
    let projected = block.attention_out.multiply(&attention, cancel)?;
    let mut output: Vec<_> = input.iter().zip(&projected).map(|(a, b)| *a + *b).collect();
    finite(&output)?;
    let norm = rms(&output, &block.ffn_norm, c.epsilon)?;
    let gate = block.gate.multiply(&norm, cancel)?;
    let up = block.up.multiply(&norm, cancel)?;
    let activated: Vec<_> = gate
        .iter()
        .zip(up)
        .map(|(gate, up)| (*gate / (1.0 + (-*gate).exp())) * up)
        .collect();
    finite(&activated)?;
    let feed_forward = block.down.multiply(&activated, cancel)?;
    for (out, value) in output.iter_mut().zip(feed_forward) {
        *out += value;
    }
    finite(&output)?;
    Ok((output, keys, values))
}

fn attend(
    query: &[f32],
    keys: &[f32],
    values: &[f32],
    state: &LayerState,
    c: Config,
    position: usize,
    cancel: &mut impl FnMut() -> bool,
) -> io::Result<Vec<f32>> {
    let mut output = zeros(c.heads * c.head_dim)?;
    let width = c.kv_heads * c.head_dim;
    let mut scores = zeros(position + 1)?;
    for head in 0..c.heads {
        check(cancel)?;
        let kv_head = head / (c.heads / c.kv_heads);
        let offset = kv_head * c.head_dim;
        let q = &query[head * c.head_dim..(head + 1) * c.head_dim];
        for (row, score) in scores.iter_mut().enumerate() {
            let k = if row == position {
                &keys[offset..offset + c.head_dim]
            } else {
                &state.keys[row * width + offset..row * width + offset + c.head_dim]
            };
            *score = 0.0;
            for (q, k) in q.iter().zip(k) {
                *score += *q * *k;
            }
            *score /= scalar(c.head_dim).sqrt();
        }
        finite(&scores)?;
        let maximum = scores.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let mut sum = 0.0;
        for score in &mut scores {
            *score = (*score - maximum).exp();
            sum += *score;
        }
        if !sum.is_finite() || sum <= 0.0 {
            return Err(invalid());
        }
        for (row, score) in scores.iter().enumerate() {
            let v = if row == position {
                &values[offset..offset + c.head_dim]
            } else {
                &state.values[row * width + offset..row * width + offset + c.head_dim]
            };
            for (out, value) in output[head * c.head_dim..(head + 1) * c.head_dim]
                .iter_mut()
                .zip(v)
            {
                *out += (*score / sum) * *value;
            }
        }
    }
    finite(&output)?;
    Ok(output)
}

#[cfg(test)]
#[path = "decoder/tests.rs"]
mod tests;
