//! Source-bound `Q8_0` packing and the deliberately small one-HVX solver subset.
//!
//! This constructs data only. It opens no session and does not admit serving.
//! DSP activation quantization is a different arithmetic path from Rust TTS.

use crate::{Error, compute};

/// Maximum complete host mapping admitted by the Q8 data-only planner.
pub const Q8_MAX_MAPPED_BYTES: usize = 64 * 1024 * 1024;
const TILE_BYTES: usize = 1088;
const VTCM_TILE_BYTES: usize = 1152;
const WEIGHTS: usize = 1024;

/// Immutable contiguous `Q8_0/F32/F32` matrix plan for the pinned HTP protocol.
///
/// Only K multiples of 128, M multiples of 32 and N=1/2/4 are represented.
/// One HVX thread, no HMX, no bias/fusion/batching and no activation chunking
/// preserve a separately reviewable subset of the pinned solver. This plan
/// neither verifies a signed package nor grants device execution authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Q8Matmul {
    columns: usize,
    rows: usize,
    batch: usize,
    weights: usize,
    activation: usize,
    output: usize,
    allocation: usize,
    prefetch: u8,
    vtcm: usize,
}

impl Q8Matmul {
    /// Solve the fixed one-HVX row kernel against an explicitly observed budget.
    ///
    /// Source: `htp_kparams_solve_hvx_mm` and `htp_mm_hvx_vtcm_layout_build`
    /// at [`crate::COMPUTE_GGML_SOURCE`]. An unknown VTCM budget must not be
    /// replaced by a guessed available capacity. No tensor tail is padded here.
    ///
    /// # Errors
    /// Rejects unsupported geometry, mappings above 64 MiB, and VTCM budgets
    /// that cannot hold a complete activation matrix with prefetch at least two.
    pub fn new(
        columns: usize,
        rows: usize,
        batch: usize,
        vtcm_budget: usize,
    ) -> Result<Self, Error> {
        if !(128..=8192).contains(&columns)
            || !columns.is_multiple_of(128)
            || !(32..=17_408).contains(&rows)
            || !rows.is_multiple_of(32)
            || ![1, 2, 4].contains(&batch)
        {
            return Err(Error::Contract);
        }
        // Validated maxima bound every intermediate below usize/u32 on the
        // supported ARM64 target; the complete mapping is independently capped.
        let weights = columns / 32 * rows * 34;
        let activation = (WEIGHTS + weights).next_multiple_of(128);
        let output = (activation + columns * batch * 4).next_multiple_of(128);
        let allocation = (output + rows * batch * 4).next_multiple_of(4096);
        if allocation > Q8_MAX_MAPPED_BYTES {
            return Err(Error::Contract);
        }
        let act = (columns / 32 * VTCM_TILE_BYTES * batch).next_multiple_of(256);
        let raw = (columns * batch * 4).next_multiple_of(128);
        let destination = if batch == 1 {
            (rows * 4).next_multiple_of(128)
        } else {
            0
        };
        let (prefetch, vtcm) = [16_u8, 8, 4, 2]
            .into_iter()
            .find_map(|prefetch| {
                let weight =
                    (usize::from(prefetch) * columns / 32 * VTCM_TILE_BYTES).next_multiple_of(256);
                let vtcm = act + (weight + destination).max(raw);
                (vtcm <= vtcm_budget).then_some((prefetch, vtcm))
            })
            .ok_or(Error::Contract)?;
        Ok(Self {
            columns,
            rows,
            batch,
            weights,
            activation,
            output,
            allocation,
            prefetch,
            vtcm,
        })
    }

    /// Inner dimension K, without padding or conversion.
    #[must_use]
    pub const fn columns(self) -> usize {
        self.columns
    }

    /// Output feature dimension M, without row padding.
    #[must_use]
    pub const fn rows(self) -> usize {
        self.rows
    }

    /// Number N of contiguous activation vectors.
    #[must_use]
    pub const fn batch(self) -> usize {
        self.batch
    }

    /// Original and tiled weight bytes; transposition preserves every byte.
    #[must_use]
    pub const fn weight_bytes(self) -> usize {
        self.weights
    }

    /// Complete command/input/output mapping charged before allocation.
    #[must_use]
    pub const fn allocation_bytes(self) -> usize {
        self.allocation
    }

    pub(crate) const fn output_offset(self) -> usize {
        self.output
    }

    /// Source-derived total VTCM scratch requirement, including raw conversion.
    #[must_use]
    pub const fn vtcm_bytes(self) -> usize {
        self.vtcm
    }

    /// Source-derived lookahead chosen from 16/8/4/2, with no hidden retry.
    #[must_use]
    pub const fn prefetch(self) -> u8 {
        self.prefetch
    }

    /// Convert ordinary row-major GGUF `Q8_0` blocks into the pinned tiled layout.
    ///
    /// Scale bytes and signed codes, including -128 and signed zero, are copied
    /// exactly. Nonfinite half scales reject before any destination mutation.
    ///
    /// # Errors
    /// Requires complete exact source/destination extents and finite scales.
    pub fn pack_weights(self, source: &[u8], destination: &mut [u8]) -> Result<(), Error> {
        self.validate_weights(source)?;
        if destination.len() != self.weights {
            return Err(Error::Contract);
        }
        self.pack_validated(source, destination);
        Ok(())
    }

    fn validate_weights(self, source: &[u8]) -> Result<(), Error> {
        if source.len() != self.weights
            || source
                .as_chunks::<34>()
                .0
                .iter()
                .any(|block| u16::from_le_bytes([block[0], block[1]]) & 0x7c00 == 0x7c00)
        {
            return Err(Error::Contract);
        }
        Ok(())
    }

    fn pack_validated(self, source: &[u8], destination: &mut [u8]) {
        let blocks = self.columns / 32;
        for row in 0..self.rows {
            for block in 0..blocks {
                let source = &source[(row * blocks + block) * 34..][..34];
                let tile = (row / 32 * blocks + block) * TILE_BYTES;
                let local_row = row % 32;
                destination[tile + 1024 + local_row * 2..][..2].copy_from_slice(&source[..2]);
                for lane in 0..32 {
                    destination[tile + lane / 2 * 64 + local_row * 2 + lane % 2] = source[2 + lane];
                }
            }
        }
    }

    /// Construct one complete command and poisoned output in an owned mapping.
    ///
    /// This is serialization, never a completion fence. The caller must keep
    /// every mapped byte immutable until correlated completion and cache reply,
    /// and retain uncertain consumers through verified supervised teardown.
    ///
    /// # Errors
    /// Rejects unaligned/null/overflowing addresses, invalid FDs, wrong extents,
    /// nonfinite activations or scales before touching the mapping.
    pub fn write_image(
        self,
        image: &mut [u8],
        base: u64,
        fd: u32,
        source: &[u8],
        activation: &[f32],
    ) -> Result<(), Error> {
        const KP: usize = 264;
        self.validate_weights(source)?;
        if image.len() != self.allocation
            || base == 0
            || !base.is_multiple_of(4096)
            || base
                .checked_add(u64::try_from(self.allocation).map_err(|_| Error::Contract)?)
                .is_none()
            || fd > i32::MAX.unsigned_abs()
            || activation.len() != self.columns * self.batch
            || activation.iter().any(|value| !value.is_finite())
        {
            return Err(Error::Contract);
        }
        image.fill(0);
        let fixed = compute::fixture_image(base, fd);
        image[..1024].copy_from_slice(&fixed[..1024]);
        put64(image, 8, self.allocation);
        tensor(
            image,
            24,
            WEIGHTS,
            self.weights,
            3,
            8,
            0,
            [self.columns, self.rows, 1, 1],
            [34, self.columns / 32 * 34, self.weights, self.weights],
        );
        tensor(
            image,
            80,
            self.activation,
            self.columns * self.batch * 4,
            0,
            0,
            1,
            [self.columns, self.batch, 1, 1],
            [
                4,
                self.columns * 4,
                self.columns * self.batch * 4,
                self.columns * self.batch * 4,
            ],
        );
        tensor(
            image,
            136,
            self.output,
            self.rows * self.batch * 4,
            0,
            0,
            2,
            [self.rows, self.batch, 1, 1],
            [
                4,
                self.rows * 4,
                self.rows * self.batch * 4,
                self.rows * self.batch * 4,
            ],
        );
        image[KP..KP + 128].fill(0);
        image[KP] = 6; // HTP_MM_KERNEL_HVX_QUANT_ROW, N >= one HVX thread.
        image[KP + 4] = 1;
        image[KP + 6] = self.prefetch;
        put32(image, KP + 16, TILE_BYTES);
        put32(image, KP + 20, VTCM_TILE_BYTES);
        put32(image, KP + 24, self.columns / 32 * VTCM_TILE_BYTES);
        put32(image, KP + 28, self.vtcm);
        for (index, shift) in [
            self.batch.trailing_zeros(),
            self.batch.trailing_zeros(),
            0,
            0,
            0,
        ]
        .into_iter()
        .enumerate()
        {
            put32(image, KP + 36 + index * 8, 1);
            image[KP + 40 + index * 8..][..4].copy_from_slice(&shift.to_le_bytes());
        }
        self.pack_validated(source, &mut image[WEIGHTS..WEIGHTS + self.weights]);
        for (index, value) in activation.iter().enumerate() {
            image[self.activation + index * 4..][..4].copy_from_slice(&value.to_le_bytes());
        }
        for slot in image[self.output..self.output + self.rows * self.batch * 4]
            .as_chunks_mut::<4>()
            .0
        {
            *slot = 0x7fc0_ae51_u32.to_le_bytes();
        }
        Ok(())
    }
}

fn put32(bytes: &mut [u8], offset: usize, value: usize) {
    // All callers use geometry bounded by Q8Matmul::new to well below u32::MAX.
    bytes[offset..offset + 4]
        .copy_from_slice(&u32::try_from(value).unwrap_or_default().to_le_bytes());
}
fn put64(bytes: &mut [u8], offset: usize, value: usize) {
    bytes[offset..offset + 8]
        .copy_from_slice(&u64::try_from(value).unwrap_or_default().to_le_bytes());
}
#[expect(
    clippy::too_many_arguments,
    reason = "Keep independently reviewed HTP tensor wire fields explicit at all three encoding sites"
)]
fn tensor(
    bytes: &mut [u8],
    offset: usize,
    data: usize,
    size: usize,
    flags: usize,
    kind: usize,
    index: u16,
    shape: [usize; 4],
    strides: [usize; 4],
) {
    put64(bytes, offset, data);
    put32(bytes, offset + 8, size);
    put32(bytes, offset + 12, flags);
    put32(bytes, offset + 16, kind);
    bytes[offset + 22..offset + 24].copy_from_slice(&index.to_le_bytes());
    for (index, value) in shape.into_iter().enumerate() {
        put32(bytes, offset + 24 + index * 4, value);
    }
    for (index, value) in strides.into_iter().enumerate() {
        put32(bytes, offset + 40 + index * 4, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn q8_solver_hand_calculated_scratch_and_exact_budget() -> Result<(), Error> {
        // K128 => 4*1152 = 4608 activation/tile row bytes.
        // N2/pref16 => 9216 + max(73728,1024) = 82944 bytes.
        let plan = Q8Matmul::new(128, 32, 2, 82_944)?;
        assert_eq!((plan.prefetch(), plan.vtcm_bytes()), (16, 82_944));
        let smaller = Q8Matmul::new(128, 32, 2, 82_943)?;
        assert_eq!((smaller.prefetch(), smaller.vtcm_bytes()), (8, 46_080));
        assert_eq!(Q8Matmul::new(128, 32, 1, 14_000)?.vtcm_bytes(), 13_952);
        assert!(Q8Matmul::new(128, 32, 1, 13_951).is_err());
        for (k, m, n) in [
            (0, 32, 2),
            (32, 32, 2),
            (129, 32, 2),
            (128, 33, 2),
            (128, 32, 3),
            (8192, 17408, 2),
        ] {
            assert!(Q8Matmul::new(k, m, n, usize::MAX).is_err());
        }
        Ok(())
    }

    #[test]
    fn q8_packing_preserves_literal_lane_and_signed_scale_observations() -> Result<(), Error> {
        let plan = Q8Matmul::new(128, 64, 2, 100_000)?;
        let mut source = vec![0; plan.weight_bytes()];
        // Source row33/block2: negative-zero scale and extreme signed codes.
        let start = (33 * 4 + 2) * 34;
        source[start..start + 6].copy_from_slice(&[0, 0x80, 0x80, 0x7f, 0xff, 1]);
        source[start + 33] = 0xfe;
        let mut packed = vec![0xaa; plan.weight_bytes()];
        plan.pack_weights(&source, &mut packed)?;
        let tile = 6 * 1088;
        // Independent literal grid positions: cp*64 + 2*row + lane%2.
        assert_eq!(&packed[tile + 2..tile + 4], &[0x80, 0x7f]);
        assert_eq!(&packed[tile + 66..tile + 68], &[0xff, 1]);
        assert_eq!(packed[tile + 963], 0xfe);
        assert_eq!(&packed[tile + 1026..tile + 1028], &[0, 0x80]);
        assert_eq!(packed.iter().filter(|&&byte| byte != 0).count(), 6);
        source[start..start + 2].copy_from_slice(&0x7c00_u16.to_le_bytes());
        let previous = packed.clone();
        assert!(plan.pack_weights(&source, &mut packed).is_err());
        assert_eq!(packed, previous);
        Ok(())
    }

    #[test]
    fn q8_packet_has_exact_wire_fields_and_transactional_rejection() -> Result<(), Error> {
        let plan = Q8Matmul::new(128, 32, 2, 100_000)?;
        let source = vec![0; plan.weight_bytes()];
        let mut activation = vec![0.0; 256];
        let mut image = vec![0xaa; plan.allocation_bytes()];
        activation[255] = f32::NAN;
        assert!(
            plan.write_image(&mut image, 4096, 7, &source, &activation)
                .is_err()
        );
        assert!(image.iter().all(|&byte| byte == 0xaa));
        activation[255] = -3.0;
        plan.write_image(&mut image, 4096, 7, &source, &activation)?;
        let read32 =
            |offset| u32::from_le_bytes(image[offset..offset + 4].try_into().unwrap_or_default());
        assert_eq!((read32(40), read32(48), read32(52)), (8, 128, 32));
        assert_eq!((read32(264), read32(280), read32(284)), (6, 1088, 1152));
        assert_eq!((read32(288), read32(292)), (4608, 82944));
        assert_eq!(read32(plan.activation + 255 * 4), (-3.0_f32).to_bits());
        assert_eq!(read32(plan.output), 0x7fc0_ae51);
        assert!(
            plan.write_image(&mut image, u64::MAX - 4095, 7, &source, &activation)
                .is_err()
        );
        Ok(())
    }
}
