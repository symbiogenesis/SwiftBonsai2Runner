//! Independent signed-dot control for a deliberately exact Q8 diagnostic subset.
//!
//! The pinned `quantize_block_f32_q8_0_tiled` converts each 32-value group to
//! half, multiplies its absolute maximum by half 0x2008, and uses the half
//! inverse before rounding to signed codes. Integer inputs with magnitude at
//! most 127 and a magnitude-127 anchor in every group have exactly unit scale
//! and unchanged codes. This control rejects all other activation inputs.
//! The signed integer dots below read original row-major weights, independently
//! of the native tiled layout. Half power-of-two weight scales and an exact
//! representation check on every running sum avoid emulating approximate HVX
//! arithmetic or substituting a Rust TTS F32/FMA oracle for the DSP algorithm.

use crate::{Error, Q8Matmul, Q8Submitter};

/// Independent numeric review of unchanged inputs in an exact diagnostic subset.
///
/// This is suitable for a separately admitted Q8 transport arithmetic control,
/// not arbitrary model activations, a native capture or speech qualification.
/// Construction opens no driver and authenticates no model/package identity.
pub struct Q8ExactOracle<'a> {
    weights: &'a [u8],
    activation: &'a [f32],
    expected: Box<[f32]>,
}

impl<'a> Q8ExactOracle<'a> {
    /// Independently evaluate every signed dot from original GGUF Q8 blocks.
    ///
    /// Each 32-value activation group must contain only integers in -127..127
    /// and have absolute maximum 127. Every weight scale must be signed zero or
    /// a signed power of two from 2^-8 through 2^8. Every prefix of the ordered
    /// 32-code block sum must be exactly representable in F32. These constraints
    /// make the selected DSP half quantization and separate multiply/add exact;
    /// they deliberately exclude general dynamic quantization and rounded sums.
    /// Original borrowed input bytes remain unchanged through submission.
    ///
    /// # Errors
    /// Rejects unsupported inputs, malformed extents, allocation failure or a
    /// running sum requiring rounding before any native submission is possible.
    pub fn new(
        projection: Q8Matmul,
        weights: &'a [u8],
        activation: &'a [f32],
    ) -> Result<Self, Error> {
        if weights.len() != projection.weight_bytes()
            || activation.len() != projection.columns() * projection.batch()
        {
            return Err(Error::Contract);
        }
        for block in activation.as_chunks::<32>().0 {
            if !block.iter().all(|&value| {
                value.is_finite() && (-127.0..=127.0).contains(&value) && value.fract() == 0.0
            }) || !block.iter().any(|value| value.abs() == 127.0)
            {
                return Err(Error::Contract);
            }
        }
        for block in weights.as_chunks::<34>().0 {
            scale(u16::from_le_bytes([block[0], block[1]]))?;
        }
        let count = projection.rows() * projection.batch();
        let mut expected = Vec::new();
        expected
            .try_reserve_exact(count)
            .map_err(|_| Error::Contract)?;
        let blocks = projection.columns() / 32;
        for vector in activation.chunks_exact(projection.columns()) {
            for row in weights.chunks_exact(blocks * 34) {
                // Units are 2^-8. Even K8192, i8 extremes and the maximum
                // admitted scale keep the absolute sum below 2^44, within i64.
                let mut units = 0_i64;
                let mut value = 0.0;
                for (weight, input) in row
                    .as_chunks::<34>()
                    .0
                    .iter()
                    .zip(vector.as_chunks::<32>().0)
                {
                    let mut dot = 0_i32;
                    for (&code, &input) in weight[2..].iter().zip(input) {
                        dot += i32::from(i8::from_ne_bytes([code])) * exact_input(input);
                    }
                    let multiplier = scale(u16::from_le_bytes([weight[0], weight[1]]))?;
                    units += i64::from(dot) * multiplier;
                    value = exact_sum(units)?;
                }
                expected.push(value);
            }
        }
        Ok(Self {
            weights,
            activation,
            expected: expected.into_boxed_slice(),
        })
    }

    /// Require every returned bit, including zero sign, to match the exact dots.
    ///
    /// # Errors
    /// Rejects changed extents or any mismatching/nonfinite returned value.
    pub fn review(&self, output: &[f32]) -> Result<(), Error> {
        if output.len() != self.expected.len()
            || !output
                .iter()
                .zip(&self.expected)
                .all(|(actual, expected)| actual.to_bits() == expected.to_bits())
        {
            return Err(Error::Contract);
        }
        Ok(())
    }

    /// Submit the same immutable inputs with mandatory independent numeric review.
    ///
    /// This supplies no native authority, retry, fallback, deadline or teardown
    /// policy; the consuming transport retains all those original obligations.
    ///
    /// # Errors
    /// Propagates transport, cancellation, bound or independent numeric failure.
    pub fn submit(&self, transport: &mut dyn Q8Submitter, output: &mut [f32]) -> Result<(), Error> {
        transport.submit(self.weights, self.activation, output, &mut |actual| {
            self.review(actual)
        })
    }
}

fn scale(bits: u16) -> Result<i64, Error> {
    let magnitude = bits & 0x7fff;
    if magnitude == 0 {
        return Ok(0);
    }
    let exponent = magnitude >> 10;
    if magnitude & 0x3ff != 0 || !(7..=23).contains(&exponent) {
        return Err(Error::Contract);
    }
    let units = 1_i64 << (exponent - 7);
    Ok(if bits & 0x8000 == 0 { units } else { -units })
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "Caller already admitted finite integral activation values in -127..127"
)]
fn exact_input(value: f32) -> i32 {
    value as i32
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    reason = "Every bounded i64-to-F32 conversion is checked by an exact round trip before admission"
)]
fn exact_sum(units: i64) -> Result<f32, Error> {
    let rounded = units as f32;
    if rounded as i64 != units {
        return Err(Error::Contract);
    }
    // Exact normal power-of-two scaling; zero remains positive as in the
    // DSP accumulator initialized to positive zero before its first addition.
    Ok(rounded * (1.0 / 256.0))
}

#[cfg(test)]
#[path = "q8_oracle_tests.rs"]
mod tests;
