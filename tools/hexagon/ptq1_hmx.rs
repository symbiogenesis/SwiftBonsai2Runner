//! PTQ1/Q8_0 lane-preserving controls over the signed K128/N8 Q2 primitive.
//!
//! Inputs are already prepared by the owning CPU graph. This module opens no
//! device, changes no activation quantizer and grants no native admission.
//! Four sparse weight rows preserve four native DOTPROD lane sums per Q8_0
//! group; four operations preserve all groups of one original PTQ1 block.

use super::{Error, MAX_COLUMNS, finite, half, hmx_projection::repack_row};

const K: usize = 128;
const LANES: usize = 4;
const PLANES: usize = 8;
const PTQ_BYTES: usize = 28;
const Q8_BYTES: usize = 34;
const STAGE_BYTES: usize = 1152;

fn half_pair(bytes: &[u8]) -> Result<f32, Error> {
    let mut block = [0; 34];
    block[..2].copy_from_slice(bytes.get(..2).ok_or(Error::Extent)?);
    half(&block)
}

// The native fractional decoder accepts every byte, including aliases. Strict
// canonicality belongs to lossless export and must not reject runtime blocks.
fn trit(block: &[u8; PTQ_BYTES], index: usize) -> u8 {
    let (byte, digit) = if index < 80 {
        (index % 16, index / 16)
    } else if index < 120 {
        (16 + (index - 80) % 8, (index - 80) / 8)
    } else {
        (24 + (index - 120) % 2, (index - 120) / 2)
    };
    let mut fraction = u16::from(block[byte]);
    for _ in 0..digit {
        fraction = (fraction * 3) & 255;
    }
    (fraction * 3 >> 8) as u8
}

/// One Q8_0 group's four sparse native-lane rows and shared binary activations.
pub struct Ptq1HmxInputs {
    weights: [u8; STAGE_BYTES],
    activation: [[f32; K]; PLANES],
}

impl Ptq1HmxInputs {
    /// Preserve one finite native PTQ1 block and exact already-prepared codes.
    ///
    /// Group 0..4 selects its 32 original columns. Row `lane` keeps positions
    /// `lane*4..lane*4+4` and `lane*4+16..lane*4+20` within that group;
    /// every other weight is zero. Padding rows retain zero symbols and unit
    /// scales, as required by the existing native submitter's input contract.
    pub fn new(original: &[u8], codes: &[i8], group: usize) -> Result<Self, Error> {
        let original: &[u8; PTQ_BYTES] = original.try_into().map_err(|_| Error::Extent)?;
        if codes.len() != K || group >= 4 {
            return Err(Error::Extent);
        }
        half_pair(&original[26..])?;
        let mut result = Self {
            weights: [0; STAGE_BYTES],
            activation: [[0.0; K]; PLANES],
        };
        for lane in 0..LANES {
            // Ordinary Q2 symbol 1 represents exact zero with unit scale.
            let mut sparse = [0x55; 34];
            sparse[..2].copy_from_slice(&0x3c00_u16.to_le_bytes());
            for half in [0, 16] {
                for offset in 0..4 {
                    let index = group * 32 + lane * 4 + half + offset;
                    let shift = index % 4 * 2;
                    let packed = &mut sparse[2 + index / 4];
                    *packed = (*packed & !(3 << shift)) | (trit(original, index) << shift);
                }
            }
            repack_row(&sparse, lane, &mut result.weights);
        }
        let mut neutral = [0x55; 34];
        neutral[..2].copy_from_slice(&0x3c00_u16.to_le_bytes());
        for padding_row in LANES..32 {
            repack_row(&neutral, padding_row, &mut result.weights);
        }
        for (bit, plane) in result.activation.iter_mut().enumerate() {
            for (value, &code) in plane.iter_mut().zip(codes) {
                *value = f32::from((code.cast_unsigned() >> bit) & 1);
            }
        }
        Ok(result)
    }

    /// Two source-native 576-byte Q2 host tiles containing four active rows.
    #[must_use]
    pub const fn repacked_weights(&self) -> &[u8; STAGE_BYTES] {
        &self.weights
    }

    /// Eight binary F32 K128 rows; these are bits of one token, not prefill.
    #[must_use]
    pub const fn activation_planes(&self) -> &[[f32; K]; PLANES] {
        &self.activation
    }

    /// Independent packed-layout integer control in `[bit][native lane]` order.
    ///
    /// This does not emulate the device solver, FP16 arithmetic or lifetime.
    pub fn cpu_control_dots(&self, output: &mut [f32; PLANES * LANES]) {
        for (bit, plane) in self.activation.iter().enumerate() {
            for row in 0..LANES {
                let mut dot = 0_i32;
                for (index, &value) in plane.iter().enumerate() {
                    let tile = index / 64 * 576;
                    let local = index % 64;
                    let packed = self.weights[tile + local / 16 * 128 + row * 4 + local % 4];
                    let symbol = (packed >> (local / 4 % 4 * 2)) & 3;
                    dot += (i32::from(symbol) - 1) * value as i32;
                }
                output[bit * LANES + row] = dot as f32;
            }
        }
    }
}

fn reconstruct(dots: &[f32; PLANES * LANES], lane: usize) -> Result<i32, Error> {
    let mut sum = 0;
    for bit in 0..PLANES {
        let value = dots[bit * LANES + lane];
        if !value.is_finite() || !(-8.0..=8.0).contains(&value) || value.fract() != 0.0 {
            return Err(Error::Protocol);
        }
        sum += value as i32 * if bit == 7 { -128 } else { 1_i32 << bit };
    }
    if !(-1024..=1024).contains(&sum) {
        return Err(Error::Protocol);
    }
    Ok(sum)
}

/// Control one pinned PTQ1 row from original packed blocks and prepared Q8_0.
///
/// Every group preserves four independent native integer lanes. Original half
/// weight and activation scales multiply on CPU, followed by ordered lane FMA
/// across the complete row and the final adjacent-pair reduction. Original
/// preparation, transforms, adapters and committed state remain CPU-owned.
///
/// The synchronous submitter must overwrite all poisoned results and finish all
/// access before returning. Any failure clears the caller's output. This does
/// not establish asynchronous ownership or permit CPU replay after submission.
pub fn hmx_ptq1_dot(
    weights: &[u8],
    prepared_q8: &[u8],
    output: &mut Option<f32>,
    mut submit: impl FnMut(&Ptq1HmxInputs, &mut [f32; PLANES * LANES]) -> Result<(), Error>,
    mut continue_work: impl FnMut() -> bool,
) -> Result<(), Error> {
    *output = None;
    if weights.is_empty()
        || !weights.len().is_multiple_of(PTQ_BYTES)
        || weights.len() / PTQ_BYTES > MAX_COLUMNS / K
        || prepared_q8.len() != weights.len() / PTQ_BYTES * 4 * Q8_BYTES
    {
        return Err(Error::Extent);
    }
    // Validate every scale before the first injected submission.
    for weight in weights.chunks_exact(PTQ_BYTES) {
        half_pair(&weight[26..])?;
    }
    for activation in prepared_q8.chunks_exact(Q8_BYTES) {
        half_pair(activation)?;
    }
    let mut lanes = [0.0_f32; LANES];
    for (block_index, original) in weights.chunks_exact(PTQ_BYTES).enumerate() {
        let q8 = &prepared_q8[block_index * 4 * Q8_BYTES..(block_index + 1) * 4 * Q8_BYTES];
        let codes: [i8; K] = std::array::from_fn(|index| {
            i8::from_ne_bytes([q8[index / 32 * Q8_BYTES + 2 + index % 32]])
        });
        let weight_scale = half_pair(&original[26..])?;
        for (group, activation) in q8.chunks_exact(Q8_BYTES).enumerate() {
            if !continue_work() {
                return Err(Error::Cancelled);
            }
            let operation = Ptq1HmxInputs::new(original, &codes, group)?;
            let mut dots = [f32::NAN; PLANES * LANES];
            submit(&operation, &mut dots)?;
            if !continue_work() {
                return Err(Error::Cancelled);
            }
            let scale = finite(weight_scale * half_pair(activation)?)?;
            for (lane, accumulator) in lanes.iter_mut().enumerate() {
                *accumulator =
                    finite((reconstruct(&dots, lane)? as f32).mul_add(scale, *accumulator))?;
            }
        }
    }
    if !continue_work() {
        return Err(Error::Cancelled);
    }
    *output = Some(finite((lanes[0] + lanes[1]) + (lanes[2] + lanes[3]))?);
    Ok(())
}

#[cfg(test)]
#[path = "ptq1_hmx_tests.rs"]
mod tests;
