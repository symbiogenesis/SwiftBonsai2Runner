//! Direct packed projection kernels. Keep dequantization rounding and each
//! block's two-accumulator dot order identical to the original decoded path.
#![expect(
    clippy::inline_always,
    reason = "SIMD helpers inherit the dispatch site's target features through inlining."
)]
#![allow(
    clippy::suboptimal_flops,
    reason = "Separate dequantization and accumulation rounding must match the original packed projection."
)]

use fearless_simd::{f32x4, prelude::*, u8x16};

use crate::weights::half;

#[inline(always)]
fn unsigned_lanes<S: Simd>(simd: S, bytes: u8x16<S>) -> [f32x4<S>; 4] {
    let (lo, hi) = simd.widen_u8x16(bytes);
    let (a, b) = simd.widen_u16x8(lo);
    let (c, d) = simd.widen_u16x8(hi);
    [a, b, c, d].map(|value| simd.cvt_f32_u32x4(value))
}

#[inline(always)]
fn signed_lanes<S: Simd>(simd: S, bytes: u8x16<S>) -> [f32x4<S>; 4] {
    let (lo, hi) = simd.widen_i8x16(bytes.bitcast());
    let (a, b) = simd.widen_i16x8(lo);
    let (c, d) = simd.widen_i16x8(hi);
    [a, b, c, d].map(|value| simd.cvt_f32_i32x4(value))
}

struct Dot<S: Simd> {
    even: f32x4<S>,
    odd: f32x4<S>,
}

impl<S: Simd> Dot<S> {
    #[inline(always)]
    fn new(simd: S) -> Self {
        Self {
            even: f32x4::splat(simd, 0.0),
            odd: f32x4::splat(simd, 0.0),
        }
    }

    #[inline(always)]
    fn push(&mut self, simd: S, weights: [f32x4<S>; 4], input: &[f32]) {
        for (pair, weights) in weights.as_chunks::<2>().0.iter().enumerate() {
            let offset = pair * 8;
            self.even = weights[0].mul_add(
                f32x4::from_slice(simd, &input[offset..offset + 4]),
                self.even,
            );
            self.odd = weights[1].mul_add(
                f32x4::from_slice(simd, &input[offset + 4..offset + 8]),
                self.odd,
            );
        }
    }

    #[inline(always)]
    fn finish(self) -> f32 {
        let lanes: [f32; 4] = (self.even + self.odd).into();
        (lanes[0] + lanes[1]) + (lanes[2] + lanes[3])
    }
}

#[inline(always)]
fn q8<S: Simd>(simd: S, source: &[u8], input: &[f32]) -> f32 {
    let scale = f32x4::splat(simd, half(source));
    let mut dot = Dot::new(simd);
    for part in 0..2 {
        let offset = part * 16;
        let quants = u8x16::from_slice(simd, &source[2 + offset..2 + offset + 16]);
        let weights = signed_lanes(simd, quants).map(|quant| scale * quant);
        dot.push(simd, weights, &input[offset..offset + 16]);
    }
    dot.finish()
}

#[inline(always)]
fn q4_k<S: Simd>(simd: S, source: &[u8], input: &[f32]) -> f32 {
    let scale = half(source);
    let minimum = half(&source[2..]);
    let scales = &source[4..16];
    let mut dot = Dot::new(simd);
    for group in 0..8 {
        let (s, m) = if group < 4 {
            (scales[group] & 63, scales[group + 4] & 63)
        } else {
            (
                (scales[group + 4] & 15) | ((scales[group - 4] >> 6) << 4),
                (scales[group + 4] >> 4) | ((scales[group] >> 6) << 4),
            )
        };
        let multiplier = f32x4::splat(simd, scale * f32::from(s));
        let offset = f32x4::splat(simd, minimum * f32::from(m));
        for part in 0..2 {
            let start = 16 + (group / 2) * 32 + part * 16;
            let packed = u8x16::from_slice(simd, &source[start..start + 16]);
            let quants = if group % 2 == 0 {
                packed & u8x16::splat(simd, 15)
            } else {
                packed >> 4
            };
            // Separate multiply/subtract preserves decode_block's rounding.
            let weights = unsigned_lanes(simd, quants).map(|quant| multiplier * quant - offset);
            let start = group * 32 + part * 16;
            dot.push(simd, weights, &input[start..start + 16]);
        }
    }
    dot.finish()
}

#[inline(always)]
fn q6_k<S: Simd>(simd: S, source: &[u8], input: &[f32]) -> f32 {
    let scale = half(&source[208..]);
    let mut dot = Dot::new(simd);
    for half_index in 0..2 {
        for quarter in 0..4 {
            for part in 0..2 {
                let start = half_index * 64 + (quarter % 2) * 32 + part * 16;
                let packed = u8x16::from_slice(simd, &source[start..start + 16]);
                let low = if quarter < 2 {
                    packed & u8x16::splat(simd, 15)
                } else {
                    packed >> 4
                };
                let start = 128 + half_index * 32 + part * 16;
                let high = u8x16::from_slice(simd, &source[start..start + 16]);
                let high = (high >> [0, 2, 4, 6][quarter]) & u8x16::splat(simd, 3);
                let quants = low | (high << 4);
                let subscale =
                    i8::from_le_bytes([source[192 + half_index * 8 + quarter * 2 + part]]);
                let multiplier = f32x4::splat(simd, scale * f32::from(subscale));
                let weights = unsigned_lanes(simd, quants)
                    .map(|quant| multiplier * (quant - f32x4::splat(simd, 32.0)));
                let start = half_index * 128 + quarter * 32 + part * 16;
                dot.push(simd, weights, &input[start..start + 16]);
            }
        }
    }
    dot.finish()
}

// Validated callers supply complete rows of a supported type. F32/F16 preserve
// their original separately rounded multiplication and left-to-right summation.
#[inline(always)]
pub(super) fn row<S: Simd>(simd: S, kind: u32, source: &[u8], input: &[f32], mut sum: f32) -> f32 {
    match kind {
        0 | 1 => {
            let size = if kind == 0 { 4 } else { 2 };
            for (sources, inputs) in source.chunks_exact(size * 4).zip(input.as_chunks::<4>().0) {
                let values = f32x4::from_fn(simd, |index| {
                    let offset = index * size;
                    if kind == 0 {
                        f32::from_le_bytes([
                            sources[offset],
                            sources[offset + 1],
                            sources[offset + 2],
                            sources[offset + 3],
                        ])
                    } else {
                        half(&sources[offset..])
                    }
                });
                let products: [f32; 4] = (values * f32x4::from_slice(simd, inputs)).into();
                for product in products {
                    sum += product;
                }
            }
            let tail = input.len() / 4 * 4;
            for (bytes, value) in source[tail * size..].chunks_exact(size).zip(&input[tail..]) {
                let weight = if kind == 0 {
                    f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
                } else {
                    half(bytes)
                };
                sum += weight * value;
            }
        }
        8 => {
            for (bytes, values) in source
                .as_chunks::<34>()
                .0
                .iter()
                .zip(input.as_chunks::<32>().0)
            {
                sum += q8(simd, bytes, values);
            }
        }
        12 => {
            for (bytes, values) in source
                .as_chunks::<144>()
                .0
                .iter()
                .zip(input.as_chunks::<256>().0)
            {
                sum += q4_k(simd, bytes, values);
            }
        }
        14 => {
            for (bytes, values) in source
                .as_chunks::<210>()
                .0
                .iter()
                .zip(input.as_chunks::<256>().0)
            {
                sum += q6_k(simd, bytes, values);
            }
        }
        _ => return f32::NAN,
    }
    sum
}

#[cfg(test)]
mod tests {
    use std::io;

    use fearless_simd::{Level, dispatch};

    use crate::{
        engine::{LinearProjection, WeightQuantization, f32_dot_simd},
        weights::{decode_block, packed_layout},
    };

    #[test]
    fn packed_direct_kernels_match_decoded_block_rounding_and_workers() -> io::Result<()> {
        for kind in [0, 1, 8, 12, 14] {
            let (block, size) = packed_layout(kind)?;
            let width = if block == 1 { 19 } else { block * 3 };
            let rows = 7;
            let mut bytes: Vec<u8> = (0..width / block * size * rows)
                .map(|index| u8::try_from((index * 73 + 31) % 256).unwrap_or(0))
                .collect();
            for (index, source) in bytes.chunks_exact_mut(size).enumerate() {
                let scale = [0x3555_u16, 0xb955, 1, 0x8000][index % 4];
                match kind {
                    0 => source.copy_from_slice(
                        &(f32::from(i16::try_from(index % 97).unwrap_or(0)) * 0.09375 - 4.0)
                            .to_le_bytes(),
                    ),
                    1 | 8 => source[..2].copy_from_slice(&scale.to_le_bytes()),
                    12 => {
                        source[..2].copy_from_slice(&scale.to_le_bytes());
                        source[2..4].copy_from_slice(&0x2c01_u16.to_le_bytes());
                    }
                    14 => source[208..].copy_from_slice(&scale.to_le_bytes()),
                    _ => return Err(io::Error::other("Unsupported fixture type.")),
                }
            }
            let input: Vec<f32> = (0..width)
                .map(|index| (f32::from(i16::try_from(index % 37).unwrap_or(0)) - 18.0) * 0.071)
                .collect();
            let bias = vec![-0.75; rows];
            let projection = LinearProjection::new(
                "direct-packed-fixture",
                width,
                rows,
                WeightQuantization::Packed {
                    ggml_type: kind,
                    bytes: bytes.clone(),
                },
                Some(bias.clone()),
            )?;
            let level = Level::new();
            let mut expected = bias;
            let mut decoded = [0.0; 256];
            for (source, output) in bytes.chunks_exact(width / block * size).zip(&mut expected) {
                for (source, values) in source.chunks_exact(size).zip(input.chunks_exact(block)) {
                    decode_block(kind, source, &mut decoded[..block]);
                    *output += if block == 1 {
                        decoded[0] * values[0]
                    } else {
                        dispatch!(level, simd => f32_dot_simd(simd, &decoded[..block], values))
                    };
                }
            }
            for workers in [1, 3, 18] {
                let actual = projection.forward_vec_parallel(&input, None, workers)?;
                assert_eq!(
                    actual
                        .iter()
                        .map(|value| value.to_bits())
                        .collect::<Vec<_>>(),
                    expected
                        .iter()
                        .map(|value| value.to_bits())
                        .collect::<Vec<_>>(),
                    "GGUF type {kind}, workers {workers}",
                );
            }
        }
        Ok(())
    }

    #[test]
    fn packed_direct_float_and_half_keep_signed_zero_subnormals_and_sequential_sums()
    -> io::Result<()> {
        let half_bits = [0x3c00_u16, 0x8000, 1, 0xbc00, 0x0400, 0x3555, 0xb955];
        for kind in [0, 1] {
            let mut bytes = Vec::new();
            let mut weights = Vec::new();
            for bits in half_bits {
                let value = crate::weights::half(&bits.to_le_bytes());
                weights.push(value);
                if kind == 0 {
                    bytes.extend_from_slice(&value.to_le_bytes());
                } else {
                    bytes.extend_from_slice(&bits.to_le_bytes());
                }
            }
            for input in [
                [16_777_216.0, 1.0, -16_777_216.0, 1.0, -1.0, 0.0, -0.0],
                [-0.0; 7],
                [0.0; 7],
                [1.0, -1.0, 0.5, -0.25, 0.125, -0.0625, 0.03125],
            ] {
                for bias in [-0.0, 0.0, 0.75] {
                    let projection = LinearProjection::new(
                        "packed-float-rounding-fixture",
                        weights.len(),
                        1,
                        WeightQuantization::Packed {
                            ggml_type: kind,
                            bytes: bytes.clone(),
                        },
                        Some(vec![bias]),
                    )?;
                    let expected = weights
                        .iter()
                        .zip(input)
                        .fold(bias, |sum, (weight, value)| sum + weight * value);
                    assert_eq!(
                        projection.forward_vec(&input, None)?[0].to_bits(),
                        expected.to_bits(),
                        "GGUF type {kind}",
                    );
                }
            }
        }
        Ok(())
    }
}
