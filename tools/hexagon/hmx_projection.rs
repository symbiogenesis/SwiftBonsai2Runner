//! Exact integer-bitplane preparation for the conditional signed Q2 HMX path.
//!
//! This module opens no device. The pinned source admits K=128/N=8 Q2 HMX
//! operations, but signed execution, placement, solver fields and ownership
//! require their own native implementation and qualification. Binary planes
//! avoid the HVX activation quantizer; original Q8_K and ordered FMA stay on CPU.

use super::{
    Error, MAX_COLUMNS, MAX_ROWS, PQ2_BYTES, Q8K_BYTES, finite, half, prepare_q8k, zeroed,
};

const K: usize = 128;
const PLANES: usize = 8;
const TILE_ROWS: usize = 32;
const TILE_BYTES: usize = 576;
const BLOCK_TILES: usize = 2;

/// One original PQ2 block and eight binary planes of existing signed i8 codes.
pub struct HmxQ2Inputs {
    weights: [u8; 36],
    activation: [[f32; K]; PLANES],
}

impl HmxQ2Inputs {
    /// Preserve all four symbols, duplicate scale-one Q2 headers and split codes.
    ///
    /// The supplied codes are already quantized by the owning CPU graph. Their
    /// two's-complement high bit has coefficient -128 during reconstruction.
    /// No guard lanes, activation requantization or original-scale conversion
    /// is performed. All plane operands are exactly representable as FP16.
    pub fn new(original_pq2: &[u8], prepared_codes: &[i8]) -> Result<Self, Error> {
        let original: &[u8; PQ2_BYTES] = original_pq2.try_into().map_err(|_| Error::Extent)?;
        if prepared_codes.len() != K {
            return Err(Error::Extent);
        }
        half(original)?;
        let mut result = Self {
            weights: [0; 36],
            activation: [[0.0; K]; PLANES],
        };
        for (block, symbols) in result
            .weights
            .chunks_exact_mut(18)
            .zip(original[2..].chunks_exact(16))
        {
            block[..2].copy_from_slice(&0x3c00_u16.to_le_bytes());
            block[2..].copy_from_slice(symbols);
        }
        for (lane, &code) in prepared_codes.iter().enumerate() {
            for (bit, plane) in result.activation.iter_mut().enumerate() {
                plane[lane] = f32::from((code.cast_unsigned() >> bit) & 1);
            }
        }
        Ok(result)
    }

    /// Two logical ordinary-Q2 records with the original symbols and unit scales.
    #[must_use]
    pub const fn logical_q2_blocks(&self) -> &[u8; 36] {
        &self.weights
    }

    /// Eight F32 K=128 rows in increasing bit order, including the signed bit.
    #[must_use]
    pub const fn activation_planes(&self) -> &[[f32; K]; PLANES] {
        &self.activation
    }

    /// Independent logical integer-dot control; this does not emulate HMX.
    pub fn cpu_control_dots(&self) -> Result<[f32; PLANES], Error> {
        let mut result = [0.0; PLANES];
        for (plane, output) in self.activation.iter().zip(&mut result) {
            let mut dot = 0_i32;
            for (lane, &value) in plane.iter().enumerate() {
                if value != 0.0 && value != 1.0 {
                    return Err(Error::Protocol);
                }
                let block = lane / 64 * 18;
                let symbol = (self.weights[block + 2 + lane % 64 / 4] >> (lane % 4 * 2)) & 3;
                dot += (i32::from(symbol) - 1) * value as i32;
            }
            *output = dot as f32;
        }
        Ok(result)
    }
}

/// Validate eight exact plane dots and preserve the original per-128 CPU FMA.
///
/// Plane dots are integers in -128..=256. Their signed reconstruction is bounded
/// by the original 128-symbol/i8 dot product before applying the original PQ2
/// scale, original Q8_K scale and previous accumulator. Signed zero is normalized
/// as in the original integer-to-F32 conversion. Plausible wrong integer results
/// still require an independent signed-device comparison.
pub fn reduce_hmx_original_block(
    original_pq2: &[u8],
    original_q8k_scale: f32,
    previous: f32,
    plane_dots: [f32; PLANES],
) -> Result<f32, Error> {
    let original: &[u8; PQ2_BYTES] = original_pq2.try_into().map_err(|_| Error::Extent)?;
    finite(original_q8k_scale)?;
    finite(previous)?;
    let mut dot = 0_i32;
    for (bit, value) in plane_dots.into_iter().enumerate() {
        if !value.is_finite() || !(-128.0..=256.0).contains(&value) || value.fract() != 0.0 {
            return Err(Error::Protocol);
        }
        let coefficient = if bit == 7 { -128 } else { 1_i32 << bit };
        dot += coefficient * value as i32;
    }
    if !(-32_768..=32_768).contains(&dot) {
        return Err(Error::Protocol);
    }
    finite((half(original)? * original_q8k_scale).mul_add(dot as f32, previous))
}

/// Host payload reservations and conservative traffic, excluding native resources.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HmxQ2Ledger {
    /// Original 128-block operations across row tiles.
    pub operation_count: usize,
    /// Borrowed packed weights; no complete second weight representation is kept.
    pub original_weight_bytes: usize,
    /// Peak tightly packed host weight staging, padded to 32 rows.
    pub repacked_stage_bytes: usize,
    /// Original Q8_K activation preparation retained across row tiles.
    pub prepared_activation_bytes: usize,
    /// Eight shared binary F32 K=128 activation planes.
    pub plane_activation_bytes: usize,
    /// Peak eight-plane active-row result payload, excluding native row padding.
    pub plane_output_bytes: usize,
    /// Complete provisional projection, published only after all operations.
    pub final_output_bytes: usize,
    /// Sum of owned payloads; mappings, VTCM, allocator and stack are additional.
    pub owned_payload_bytes: usize,
    /// Repacked weight traffic with no assumed residency or reuse.
    pub repacked_weight_transfer_bytes: usize,
    /// Activation traffic with no assumed residency or reuse.
    pub activation_transfer_bytes: usize,
    /// All returned active-row plane payloads before CPU reconstruction.
    pub plane_return_bytes: usize,
}

/// First-projection-or-smaller bounded HMX packing plan; no device admission.
#[derive(Clone, Copy, Debug)]
pub struct HmxQ2Plan {
    columns: usize,
    rows: usize,
    tile_rows: usize,
}

impl HmxQ2Plan {
    /// Bound allocation before staging; unsupported geometry or capacity rejects.
    ///
    /// Original columns are divisible by 256 and at most 5120; rows are at most
    /// 17408. Each operation keeps one original 128-block. Partial 32-row tiles
    /// are zero-filled with zero scales; native output padding is an additional
    /// submitter-owned resource. A payload allowance is not a VTCM guarantee.
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

    /// Derived storage/traffic; no mapped, DMA or effective placement observation.
    #[must_use]
    pub fn ledger(self) -> HmxQ2Ledger {
        let blocks = self.columns / K;
        let row_tiles = self.rows.div_ceil(self.tile_rows);
        let operation_count = blocks * row_tiles;
        let repacked_stage_bytes = self.tile_rows.div_ceil(TILE_ROWS) * BLOCK_TILES * TILE_BYTES;
        let prepared_activation_bytes = self.columns / 256 * Q8K_BYTES;
        let plane_activation_bytes = PLANES * K * size_of::<f32>();
        let plane_output_bytes = PLANES * self.tile_rows * size_of::<f32>();
        let final_output_bytes = self.rows * size_of::<f32>();
        let last_rows = self.rows - (row_tiles - 1) * self.tile_rows;
        let total_stage_bytes = (row_tiles - 1) * repacked_stage_bytes
            + last_rows.div_ceil(TILE_ROWS) * BLOCK_TILES * TILE_BYTES;
        HmxQ2Ledger {
            operation_count,
            original_weight_bytes: self.rows * blocks * PQ2_BYTES,
            repacked_stage_bytes,
            prepared_activation_bytes,
            plane_activation_bytes,
            plane_output_bytes,
            final_output_bytes,
            owned_payload_bytes: repacked_stage_bytes
                + prepared_activation_bytes
                + plane_activation_bytes
                + plane_output_bytes
                + final_output_bytes,
            repacked_weight_transfer_bytes: blocks * total_stage_bytes,
            activation_transfer_bytes: operation_count * plane_activation_bytes,
            plane_return_bytes: self.rows * blocks * PLANES * size_of::<f32>(),
        }
    }
}

/// Borrowed operation payload; successful submission must mean completed access.
pub struct HmxQ2Operation {
    sequence: usize,
    row_start: usize,
    block_index: usize,
    rows: usize,
    weights: Vec<u8>,
    activation: [[f32; K]; PLANES],
}

impl HmxQ2Operation {
    /// Zero-based caller sequence, separate from any native packet identifier.
    #[must_use]
    pub const fn sequence(&self) -> usize {
        self.sequence
    }
    /// First active output row in the original matrix.
    #[must_use]
    pub const fn row_start(&self) -> usize {
        self.row_start
    }
    /// Original 128-column block; original FMA order follows this index.
    #[must_use]
    pub const fn block_index(&self) -> usize {
        self.block_index
    }
    /// Active output rows; the packed weight extent rounds this up to 32.
    #[must_use]
    pub const fn rows(&self) -> usize {
        self.rows
    }
    /// Two tightly packed 576-byte tiles per 32 rows, K tile varying fastest.
    #[must_use]
    pub fn repacked_weights(&self) -> &[u8] {
        &self.weights
    }
    /// Eight binary F32 rows in increasing two's-complement bit order.
    #[must_use]
    pub const fn activation_planes(&self) -> &[[f32; K]; PLANES] {
        &self.activation
    }

    /// Decode packed host tiles independently of their original logical blocks.
    ///
    /// The result has [plane][active row] order. This CPU control does not
    /// emulate or qualify native HMX FP16 conversion or accumulation.
    pub fn cpu_control_dots(&self, output: &mut [f32]) -> Result<(), Error> {
        if output.len() != PLANES * self.rows {
            return Err(Error::Extent);
        }
        for (bit, plane) in self.activation.iter().enumerate() {
            for row in 0..self.rows {
                let mut dot = 0_i32;
                for (lane, &value) in plane.iter().enumerate() {
                    if value != 0.0 && value != 1.0 {
                        return Err(Error::Protocol);
                    }
                    let tile = (row / TILE_ROWS * BLOCK_TILES + lane / 64) * TILE_BYTES;
                    let local = lane % 64;
                    let byte =
                        self.weights[tile + local / 16 * 128 + row % TILE_ROWS * 4 + local % 4];
                    let symbol = (byte >> (local / 4 % 4 * 2)) & 3;
                    dot += (i32::from(symbol) - 1) * value as i32;
                }
                output[bit * self.rows + row] = dot as f32;
            }
        }
        Ok(())
    }
}

// Source-native Q2 host layout, llama.cpp@19cce6919992fdffd92e0ac152caaf1e2b8f254e
// ggml/src/ggml-hexagon/htp-repack.cpp::repack_q2_0_tiled (MIT).
pub(super) fn repack_row(original: &[u8], row: usize, output: &mut [u8]) {
    for lane in 0..K {
        let symbol = (original[2 + lane / 4] >> (lane % 4 * 2)) & 3;
        let tile = (row / TILE_ROWS * BLOCK_TILES + lane / 64) * TILE_BYTES;
        let local = lane % 64;
        let destination = tile + local / 16 * 128 + row % TILE_ROWS * 4 + local % 4;
        output[destination] |= symbol << (local / 4 % 4 * 2);
    }
    for block in 0..BLOCK_TILES {
        let scale =
            (row / TILE_ROWS * BLOCK_TILES + block) * TILE_BYTES + 512 + row % TILE_ROWS * 2;
        output[scale..scale + 2].copy_from_slice(&0x3c00_u16.to_le_bytes());
    }
}

/// Prepare Q8_K once, submit eight-plane dots and commit the complete projection.
///
/// The input is already transformed by the CPU graph. A submitter must overwrite
/// all poisoned plane results and drain all asynchronous access before returning;
/// uncertain native owners must remain alive through independently verified
/// teardown. Any malformed input/result, cancellation or submission error leaves
/// `output` empty. No partial projection or CPU replay is published.
pub fn hmx_q2_projection(
    plan: HmxQ2Plan,
    weights: &[u8],
    input: &[f32],
    output: &mut Vec<f32>,
    mut submit: impl FnMut(&HmxQ2Operation, &mut [f32]) -> Result<(), Error>,
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
    for block in weights.as_chunks::<PQ2_BYTES>().0 {
        half(block)?;
    }
    let activation = prepare_q8k(input)?;
    let mut pending = zeroed::<f32>(plan.rows)?;
    let mut dots = zeroed::<f32>(ledger.plane_output_bytes / size_of::<f32>())?;
    let mut operation = HmxQ2Operation {
        sequence: 0,
        row_start: 0,
        block_index: 0,
        rows: plan.tile_rows,
        weights: zeroed(ledger.repacked_stage_bytes)?,
        activation: [[0.0; K]; PLANES],
    };
    let blocks = plan.columns / K;
    for row_start in (0..plan.rows).step_by(plan.tile_rows) {
        let rows = plan.tile_rows.min(plan.rows - row_start);
        operation
            .weights
            .truncate(rows.div_ceil(TILE_ROWS) * BLOCK_TILES * TILE_BYTES);
        for block_index in 0..blocks {
            if !continue_work() {
                return Err(Error::Cancelled);
            }
            operation.row_start = row_start;
            operation.block_index = block_index;
            operation.rows = rows;
            operation.weights.fill(0);
            let q8_start = block_index / 2 * Q8K_BYTES;
            let code_start = q8_start + 4 + block_index % 2 * K;
            for (bit, plane) in operation.activation.iter_mut().enumerate() {
                for (destination, &code) in plane
                    .iter_mut()
                    .zip(&activation[code_start..code_start + K])
                {
                    *destination = f32::from((code >> bit) & 1);
                }
            }
            for row in 0..rows {
                let index = ((row_start + row) * blocks + block_index) * PQ2_BYTES;
                repack_row(
                    &weights[index..index + PQ2_BYTES],
                    row,
                    &mut operation.weights,
                );
            }
            let active_dots = &mut dots[..PLANES * rows];
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
                let planes = std::array::from_fn(|bit| active_dots[bit * rows + row]);
                pending[row_start + row] = reduce_hmx_original_block(
                    &weights[index..index + PQ2_BYTES],
                    scale,
                    pending[row_start + row],
                    planes,
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
#[path = "hmx_projection_tests.rs"]
mod tests;
