//! Bounded, synchronous preparation/reduction for the conditional signed Q2 path.
//!
//! The layout follows the separately pinned Q2 package, rather than the older
//! access package. This module opens no device. A submitter must independently
//! qualify the signed operation, solver, quantizer and completion/ownership ABI.

use super::q2_integer::{PADDED_K, Q2_BLOCK_BYTES, guarded_weights};
use super::{
    Error, GuardedQ2Inputs, MAX_COLUMNS, MAX_ROWS, PQ2_BYTES, Q8K_BYTES, finite, half, prepare_q8k,
    reduce_original_block, zeroed,
};

const TILE_ROWS: usize = 32;
const TILE_BYTES: usize = 640;
const BLOCK_TILES: usize = PADDED_K / 64;

/// Derived host bounds for one complete original-PQ2 projection, not admission.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuardedQ2Ledger {
    /// Number of synchronous operations; each preserves one original 128-block.
    pub operation_count: usize,
    /// Caller-owned original weights, borrowed without a second complete copy.
    pub original_weight_bytes: usize,
    /// Peak Q2 tile staging, including all 640-byte tile alignment/padding.
    pub repacked_stage_bytes: usize,
    /// Original pinned Q8_K preparation retained across row tiles.
    pub prepared_activation_bytes: usize,
    /// Two guarded F32 rows shared by every weight row in one operation.
    pub guarded_activation_bytes: usize,
    /// Peak F32 limb output; never exposed as a partial projection.
    pub limb_output_bytes: usize,
    /// Complete provisional F32 projection, committed only on success.
    pub final_output_bytes: usize,
    /// Payload storage bound; vendor/VTCM/allocator/stack charges are additional.
    pub owned_payload_bytes: usize,
    /// Conservative total Q2 bytes if every operation uploads its own weights.
    pub repacked_weight_transfer_bytes: usize,
    /// Conservative activation traffic with no assumed device residency/reuse.
    pub activation_transfer_bytes: usize,
    /// Total intermediate limb result bytes, before CPU per-block reduction.
    pub limb_return_bytes: usize,
}

/// One first-projection-or-smaller plan. It confers no device/profile eligibility.
#[derive(Clone, Copy, Debug)]
pub struct GuardedQ2Plan {
    columns: usize,
    rows: usize,
    tile_rows: usize,
}

impl GuardedQ2Plan {
    /// Choose a row tile using an explicit owned-payload allowance.
    ///
    /// K remains the original projection width; every submitted operation has
    /// guarded K=256/N=2. Larger M amortizes submission without combining the
    /// original per-128 scales/reductions. M need not be a multiple of 32.
    pub fn new(
        columns: usize,
        rows: usize,
        tile_rows: usize,
        maximum_owned_payload_bytes: usize,
    ) -> Result<Self, Error> {
        if columns == 0
            || columns > MAX_COLUMNS
            || !columns.is_multiple_of(256)
            || rows == 0
            || rows > MAX_ROWS
            || tile_rows == 0
            || tile_rows > rows
        {
            return Err(Error::Extent);
        }
        let plan = Self {
            columns,
            rows,
            tile_rows,
        };
        if plan.ledger().owned_payload_bytes > maximum_owned_payload_bytes {
            return Err(Error::Capacity);
        }
        Ok(plan)
    }

    /// Exact checked-shape payload arithmetic; excludes vendor framing/resources.
    #[must_use]
    pub fn ledger(self) -> GuardedQ2Ledger {
        let column_blocks = self.columns / 128;
        let row_tiles = self.rows.div_ceil(self.tile_rows);
        let operation_count = column_blocks * row_tiles;
        let repacked_stage_bytes = self.tile_rows.div_ceil(TILE_ROWS) * BLOCK_TILES * TILE_BYTES;
        let prepared_activation_bytes = self.columns / 256 * Q8K_BYTES;
        let guarded_activation_bytes = 2 * PADDED_K * size_of::<f32>();
        let limb_output_bytes = 2 * self.tile_rows * size_of::<f32>();
        let final_output_bytes = self.rows * size_of::<f32>();
        let last_rows = self.rows - (row_tiles - 1) * self.tile_rows;
        let tile_weight_total = (row_tiles - 1) * repacked_stage_bytes
            + last_rows.div_ceil(TILE_ROWS) * BLOCK_TILES * TILE_BYTES;
        GuardedQ2Ledger {
            operation_count,
            original_weight_bytes: self.rows * column_blocks * PQ2_BYTES,
            repacked_stage_bytes,
            prepared_activation_bytes,
            guarded_activation_bytes,
            limb_output_bytes,
            final_output_bytes,
            owned_payload_bytes: repacked_stage_bytes
                + prepared_activation_bytes
                + guarded_activation_bytes
                + limb_output_bytes
                + final_output_bytes,
            repacked_weight_transfer_bytes: column_blocks * tile_weight_total,
            activation_transfer_bytes: operation_count * guarded_activation_bytes,
            limb_return_bytes: self.rows * column_blocks * 2 * size_of::<f32>(),
        }
    }
}

/// One ordinary-Q2 input pair in the newer signed source's tiled weight layout.
///
/// Slices remain borrowed throughout `submit`; returning from that call must
/// mean the submitter has completed/drained all asynchronous access. A device
/// timeout/error must retain its own in-use owners through verified teardown.
/// Merely borrowing these CPU slices cannot establish native completion.
pub struct GuardedQ2Operation {
    sequence: usize,
    row_start: usize,
    block_index: usize,
    rows: usize,
    weights: Vec<u8>,
    activation: [[f32; PADDED_K]; 2],
}

impl GuardedQ2Operation {
    /// Zero-based sequence for correlation; it is not a vendor packet identifier.
    #[must_use]
    pub const fn sequence(&self) -> usize {
        self.sequence
    }

    /// First output row of this operation in the original projection.
    #[must_use]
    pub const fn row_start(&self) -> usize {
        self.row_start
    }

    /// Original 128-column block index; CPU FMA order follows this index.
    #[must_use]
    pub const fn block_index(&self) -> usize {
        self.block_index
    }

    /// Active M rows. Padded weight rows have raw scale zero and symbol zero.
    #[must_use]
    pub const fn rows(&self) -> usize {
        self.rows
    }

    /// Four 640-byte tiles per 32 weight rows, with K-tile varying fastest.
    ///
    /// The pinned scalar layout stores 512 symbol bytes, 32 little-endian FP16
    /// scales at 512..576 and 64 zero padding bytes. Actual rows use scale one.
    #[must_use]
    pub fn repacked_weights(&self) -> &[u8] {
        &self.weights
    }

    /// Two F32 K=256 rows. Limb rows are not independent prefill tokens and N=2
    /// does not satisfy the source's HMX minimum-row condition.
    #[must_use]
    pub const fn activation_rows(&self) -> &[[f32; PADDED_K]; 2] {
        &self.activation
    }

    /// Replay the intended integer primitive, independently decoding tiled bytes.
    ///
    /// Outputs use contiguous [limb][active-M-row] order. This CPU decoder does
    /// not emulate vendor quantizer intrinsics or prove signed DSP parity.
    pub fn cpu_control_dots(&self, output: &mut [f32]) -> Result<(), Error> {
        if output.len() != 2 * self.rows {
            return Err(Error::Extent);
        }
        for limb in 0..2 {
            for row in 0..self.rows {
                let mut dot = 0_i32;
                for lane in 0..PADDED_K {
                    let tile_start = (row / TILE_ROWS * BLOCK_TILES + lane / 64) * TILE_BYTES;
                    let local = lane % 64;
                    let byte = self.weights
                        [tile_start + local / 16 * 128 + row % TILE_ROWS * 4 + local % 4];
                    let symbol = (byte >> (local / 4 % 4 * 2)) & 3;
                    let value = self.activation[limb][lane];
                    if !value.is_finite() || value.fract() != 0.0 || value.abs() > 127.0 {
                        return Err(Error::Protocol);
                    }
                    dot += (i32::from(symbol) - 1) * value as i32;
                }
                output[limb * self.rows + row] = dot as f32;
            }
        }
        Ok(())
    }
}

// Independent per-row logical records enter the source-native tiled layout.
// Mapping source: llama.cpp@19cce6919992fdffd92e0ac152caaf1e2b8f254e,
// ggml/src/ggml-hexagon/htp-repack.cpp::repack_q2_0_tiled (MIT).
fn repack_row(logical: &[u8; 72], row: usize, output: &mut [u8]) {
    for block in 0..BLOCK_TILES {
        let source = &logical[block * Q2_BLOCK_BYTES..(block + 1) * Q2_BLOCK_BYTES];
        let tile_start = (row / TILE_ROWS * BLOCK_TILES + block) * TILE_BYTES;
        for lane in 0..64 {
            let code = (source[2 + lane / 4] >> (lane % 4 * 2)) & 3;
            let index = tile_start + lane / 16 * 128 + 4 * (row % TILE_ROWS) + lane % 4;
            output[index] |= code << (lane / 4 % 4 * 2);
        }
        let scale = tile_start + 512 + 2 * (row % TILE_ROWS);
        output[scale..scale + 2].copy_from_slice(&source[..2]);
    }
}

/// Prepare once, submit bounded operations and commit the complete CPU projection.
///
/// The input is already in the graph's original transform basis. Exact original
/// Q8_K preparation and per-128 FMA remain CPU-owned. `submit` must overwrite
/// every poisoned result and return only after completion; outputs are checked
/// for finite, integral, bounded limb values before reduction. Passing these
/// checks cannot authenticate an integer-looking wrong device result.
///
/// Any extent, arithmetic, cancellation or submission failure clears `output`.
/// No original scales are delegated and no partial projection is published.
pub fn guarded_q2_projection(
    plan: GuardedQ2Plan,
    weights: &[u8],
    input: &[f32],
    output: &mut Vec<f32>,
    mut submit: impl FnMut(&GuardedQ2Operation, &mut [f32]) -> Result<(), Error>,
    mut continue_work: impl FnMut() -> bool,
) -> Result<(), Error> {
    output.clear();
    let ledger = plan.ledger();
    if weights.len() != ledger.original_weight_bytes || input.len() != plan.columns {
        return Err(Error::Extent);
    }
    if !continue_work() {
        return Err(Error::Cancelled);
    }
    // Validate all raw scales before the first submission. A later malformed
    // weight must not trigger device work with a partly valid projection.
    for block in weights.as_chunks::<PQ2_BYTES>().0 {
        half(block)?;
    }
    let activation = prepare_q8k(input)?;
    let mut pending = zeroed::<f32>(plan.rows)?;
    let mut dots = zeroed::<f32>(ledger.limb_output_bytes / size_of::<f32>())?;
    let mut operation = GuardedQ2Operation {
        sequence: 0,
        row_start: 0,
        block_index: 0,
        rows: plan.tile_rows,
        weights: zeroed(ledger.repacked_stage_bytes)?,
        activation: [[0.0; PADDED_K]; 2],
    };
    let blocks = plan.columns / 128;
    for row_start in (0..plan.rows).step_by(plan.tile_rows) {
        let rows = plan.tile_rows.min(plan.rows - row_start);
        let stage_bytes = rows.div_ceil(TILE_ROWS) * BLOCK_TILES * TILE_BYTES;
        operation.weights.truncate(stage_bytes);
        for block_index in 0..blocks {
            if !continue_work() {
                return Err(Error::Cancelled);
            }
            operation.row_start = row_start;
            operation.block_index = block_index;
            operation.rows = rows;
            operation.weights.fill(0);
            let q8_start = block_index / 2 * Q8K_BYTES;
            let code_start = q8_start + 4 + block_index % 2 * 128;
            let mut codes = [0_i8; 128];
            for (code, &byte) in codes
                .iter_mut()
                .zip(&activation[code_start..code_start + 128])
            {
                *code = byte.cast_signed();
            }
            let first = (row_start * blocks + block_index) * PQ2_BYTES;
            operation.activation =
                *GuardedQ2Inputs::new(&weights[first..first + PQ2_BYTES], &codes)?
                    .activation_rows();
            for row in 0..rows {
                let index = ((row_start + row) * blocks + block_index) * PQ2_BYTES;
                repack_row(
                    &guarded_weights(&weights[index..index + PQ2_BYTES])?,
                    row,
                    &mut operation.weights,
                );
            }
            let active_dots = &mut dots[..2 * rows];
            active_dots.fill(f32::NAN);
            submit(&operation, active_dots)?;
            if !continue_work() {
                return Err(Error::Cancelled);
            }
            let scale = finite(f32::from_le_bytes(
                activation[q8_start..q8_start + 4]
                    .try_into()
                    .map_err(|_| Error::Extent)?,
            ))?;
            for row in 0..rows {
                let index = ((row_start + row) * blocks + block_index) * PQ2_BYTES;
                pending[row_start + row] = reduce_original_block(
                    &weights[index..index + PQ2_BYTES],
                    scale,
                    pending[row_start + row],
                    [active_dots[row], active_dots[rows + row]],
                )?;
            }
            operation.sequence += 1;
        }
    }
    if !continue_work() {
        return Err(Error::Cancelled);
    }
    *output = pending;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Plan, tiled_projection};

    #[test]
    fn repack_matches_independent_transposed_bit_plane_fixture() {
        // Within each sixteen-lane quartet: 0000,1111,2222,3333.
        // The four destination lane bytes are therefore 0b11100100.
        let mut logical = [0_u8; 72];
        for block in logical.chunks_exact_mut(18) {
            block[..2].copy_from_slice(&0x3c00_u16.to_le_bytes());
            for (index, byte) in block[2..].iter_mut().enumerate() {
                *byte = [0x00, 0x55, 0xaa, 0xff][index % 4];
            }
        }
        let mut tile = vec![0; 4 * TILE_BYTES];
        for row in 0..32 {
            repack_row(&logical, row, &mut tile);
        }
        for block in tile.chunks_exact(TILE_BYTES) {
            assert_eq!(&block[..512], &[0xe4; 512]);
            for scale in block[512..576].chunks_exact(2) {
                assert_eq!(scale, [0x00, 0x3c]);
            }
            assert_eq!(&block[576..], &[0; 64]);
        }

        // Asymmetric row/K-tile fixture: only row 5 is active in the first row
        // tile and only row 0 in the second. Literal offsets distinguish row,
        // block, bit-plane and matrix-row-tile ordering independently of decode.
        let mut isolated = vec![0; 8 * TILE_BYTES];
        repack_row(&logical, 5, &mut isolated);
        logical[2..18].fill(0xff);
        repack_row(&logical, 32, &mut isolated);
        assert_eq!(&isolated[20..24], &[0xe4; 4]);
        assert_eq!(&isolated[148..152], &[0xe4; 4]);
        assert_eq!(&isolated[276..280], &[0xe4; 4]);
        assert_eq!(&isolated[404..408], &[0xe4; 4]);
        assert_eq!(&isolated[522..524], &[0x00, 0x3c]);
        assert_eq!(&isolated[660..664], &[0xe4; 4]);
        assert_eq!(&isolated[2560..2564], &[0xff; 4]);
        assert_eq!(&isolated[3072..3074], &[0x00, 0x3c]);
        assert_eq!(&isolated[3200..3204], &[0xe4; 4]);
        assert_eq!(&isolated[3712..3714], &[0x00, 0x3c]);
        assert!(isolated[..20].iter().all(|&byte| byte == 0));
        assert!(isolated[576..640].iter().all(|&byte| byte == 0));
    }

    #[test]
    fn full_m_amortizes_submission_without_merging_original_blocks() -> Result<(), Error> {
        let plan = GuardedQ2Plan::new(5120, 17408, 17408, usize::MAX)?;
        let ledger = plan.ledger();
        assert_eq!(ledger.operation_count, 40);
        assert_eq!(ledger.original_weight_bytes, 23_674_880);
        assert_eq!(ledger.repacked_stage_bytes, 1_392_640);
        assert_eq!(ledger.prepared_activation_bytes, 5840);
        assert_eq!(ledger.guarded_activation_bytes, 2048);
        assert_eq!(ledger.limb_output_bytes, 139_264);
        assert_eq!(ledger.final_output_bytes, 69_632);
        assert_eq!(ledger.owned_payload_bytes, 1_609_424);
        assert_eq!(ledger.repacked_weight_transfer_bytes, 55_705_600);
        assert_eq!(ledger.activation_transfer_bytes, 81_920);
        assert_eq!(ledger.limb_return_bytes, 5_570_560);
        assert_eq!(
            GuardedQ2Plan::new(5120, 17408, 17408, ledger.owned_payload_bytes - 1).err(),
            Some(Error::Capacity)
        );
        assert_eq!(
            GuardedQ2Plan::new(5120, 17408, 32, usize::MAX)?
                .ledger()
                .operation_count,
            21_760
        );
        Ok(())
    }

    fn fixture(columns: usize, rows: usize) -> (Vec<u8>, Vec<f32>) {
        let mut weights = vec![0; columns / 128 * rows * PQ2_BYTES];
        for (block_index, block) in weights.chunks_exact_mut(PQ2_BYTES).enumerate() {
            block[..2].copy_from_slice(
                &[0x3c01_u16, 0x3787, 0x8000, 0x0001][block_index % 4].to_le_bytes(),
            );
            for (index, byte) in block[2..].iter_mut().enumerate() {
                *byte = ((index * 37 + block_index * 53) % 256) as u8;
            }
        }
        let input = (0..columns)
            .map(|lane| (lane as f32 - 137.0) * 0.03125)
            .collect();
        (weights, input)
    }

    #[test]
    fn tiled_guarded_replay_preserves_original_q8k_and_per_block_fma() -> Result<(), Error> {
        let columns = 768;
        let rows = 35;
        let (weights, input) = fixture(columns, rows);
        let control = Plan::new(columns, rows, 32, 2, usize::MAX)?;
        let mut expected = Vec::new();
        tiled_projection(control, &weights, &input, &mut expected, || true)?;
        for tile_rows in [1, 31, 32, 33, 35] {
            let plan = GuardedQ2Plan::new(columns, rows, tile_rows, usize::MAX)?;
            let mut result = Vec::new();
            let mut observed = 0;
            guarded_q2_projection(
                plan,
                &weights,
                &input,
                &mut result,
                |operation, dots| {
                    assert_eq!(operation.sequence(), observed);
                    assert_eq!(operation.activation_rows().len(), 2);
                    assert_eq!(operation.repacked_weights().len() % (4 * TILE_BYTES), 0);
                    operation.cpu_control_dots(dots)?;
                    observed += 1;
                    Ok(())
                },
                || true,
            )?;
            assert_eq!(observed, plan.ledger().operation_count);
            assert_eq!(
                result
                    .iter()
                    .map(|value| value.to_bits())
                    .collect::<Vec<_>>(),
                expected
                    .iter()
                    .map(|value| value.to_bits())
                    .collect::<Vec<_>>()
            );
        }
        Ok(())
    }

    #[test]
    fn poisoned_partial_failed_and_cancelled_submissions_publish_no_projection() -> Result<(), Error>
    {
        let (weights, input) = fixture(256, 2);
        let plan = GuardedQ2Plan::new(256, 2, 2, usize::MAX)?;
        for mode in 0..4 {
            let mut result = vec![99.0];
            let rejected = guarded_q2_projection(
                plan,
                &weights,
                &input,
                &mut result,
                |operation, dots| {
                    match mode {
                        0 => (),            // All poison must reject.
                        1 => dots[0] = 0.0, // Partial writes must reject.
                        2 => {
                            operation.cpu_control_dots(dots)?;
                            dots[0] = 0.5;
                        }
                        _ => return Err(Error::Protocol),
                    }
                    Ok(())
                },
                || true,
            );
            assert_eq!(rejected, Err(Error::Protocol));
            assert!(result.is_empty());
        }
        let mut checks = 0;
        let mut result = vec![99.0];
        assert_eq!(
            guarded_q2_projection(
                plan,
                &weights,
                &input,
                &mut result,
                |operation, dots| operation.cpu_control_dots(dots),
                || {
                    checks += 1;
                    checks < 3 // Cancellation after a completed primitive.
                },
            ),
            Err(Error::Cancelled)
        );
        assert!(result.is_empty());
        Ok(())
    }

    #[test]
    fn all_scales_and_extents_validate_before_submission() -> Result<(), Error> {
        let (mut weights, input) = fixture(256, 2);
        let plan = GuardedQ2Plan::new(256, 2, 2, usize::MAX)?;
        let last = weights.len() - PQ2_BYTES;
        weights[last..last + 2].copy_from_slice(&0x7c00_u16.to_le_bytes());
        let mut result = vec![99.0];
        let mut calls = 0;
        assert_eq!(
            guarded_q2_projection(
                plan,
                &weights,
                &input,
                &mut result,
                |_, _| {
                    calls += 1;
                    Ok(())
                },
                || true
            ),
            Err(Error::Nonfinite)
        );
        assert_eq!(calls, 0);
        assert!(result.is_empty());
        for (columns, rows, tile_rows) in
            [(128, 1, 1), (256, 0, 1), (256, 2, 3), (usize::MAX, 1, 1)]
        {
            assert_eq!(
                GuardedQ2Plan::new(columns, rows, tile_rows, usize::MAX).err(),
                Some(Error::Extent)
            );
        }
        Ok(())
    }
}
