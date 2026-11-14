//! Independent integer VTCM solver and explicit binary16 counterexamples.
use super::{ROOT, add, align, contract, contracts, integer, mul, pq2, read, require, sources};
use crate::Result;
use serde_json::{Value, json};
use std::path::Path;

pub(super) fn round_half(value: f32) -> Result<f32> {
    require(value.is_finite(), "Finite half-rounding operand required.")?;
    let bits = value.to_bits();
    let sign = (bits >> 16) & 0x8000;
    let exponent = (bits >> 23) & 255;
    if exponent < 102 {
        return pq2::half(u16::try_from(sign).map_err(|error| error.to_string())?);
    }
    let (significand, shift, base) = if exponent < 113 {
        ((bits & 0x7f_ffff) | 0x80_0000, 126 - exponent, 0)
    } else {
        (bits & 0x7f_ffff, 13, (exponent - 112) << 10)
    };
    require(
        shift <= 24 && exponent <= 142,
        "Half-rounding extent unsupported.",
    )?;
    let truncated = significand >> shift;
    let remaining = significand & ((1_u32 << shift) - 1);
    let midpoint = 1_u32 << (shift - 1);
    let rounded = truncated
        + u32::from(remaining > midpoint || (remaining == midpoint && truncated & 1 == 1));
    pq2::half(
        u16::try_from(sign | base.checked_add(rounded).ok_or("Half rounding overflow.")?)
            .map_err(|error| error.to_string())?,
    )
}

pub(super) fn chunks(rows: u64, budget: u64, threads: u64) -> Result<Value> {
    require(
        rows > 0 && rows.is_multiple_of(32) && (1..=8).contains(&threads),
        "Positive 32-row geometry and one to eight threads required.",
    )?;
    let usable = budget
        .checked_sub(5 * 2048 + 256)
        .ok_or("VTCM budget too small.")?;
    require(usable >= 32 * 296, "VTCM budget too small for one tile.")?;
    let mut best: Option<(u64, u64, u64)> = None;
    for columns in (32..=rows.min(usable / 296)).step_by(32) {
        let remaining = usable - mul(columns, 296)?;
        let mc = (remaining / add(256, mul(columns, 2)?)? / 32 * 32).min(32);
        if mc == 0 {
            continue;
        }
        let blocks = rows.div_ceil(columns);
        let waste = if rows.is_multiple_of(columns) {
            0
        } else {
            columns - rows % columns
        };
        if best.is_none_or(|(b, w, c)| {
            blocks < b || (blocks == b && (waste < w || (waste == w && columns > c)))
        }) {
            best = Some((blocks, waste, columns));
        }
    }
    let (_, _, columns) = best.ok_or("No valid HMX chunk configuration.")?;
    let weight = align(mul(columns / 32, 2 * 640)?, 2048)?;
    let output = align(mul(32, mul(columns, 2)?)?, 2048)?;
    let scratch = align(mul(columns, 256)?, 2048)?;
    let group = add(add(weight, output)?, scratch)?;
    let act = threads.min(4);
    let activation = align(group.clamp(act * 1024, act * 32768), 128)?;
    let size = add(2048 + 8192, group.max(activation))?;
    require(size <= budget, "Solved VTCM layout exceeds budget.")?;
    let chunks = rows.div_ceil(columns);
    Ok(
        json!({"rows":rows,"vtcmBudgetBytes":budget,"hvxThreads":threads,"actThreads":act,"nChunk":columns,"vtcmSizeBytes":size,"weightVtcmBytes":weight,"outputVtcmBytes":output,"scratchVtcmBytes":scratch,"activationTempVtcmBytes":activation,"chunksPerOperation":chunks,"chunksPer40OpProjection":mul(40,chunks)?,"tailWasteRows":mul(columns,chunks)?-rows}),
    )
}

#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::suboptimal_flops,
    reason = "The independent historical counterexample explicitly rounds its binary64 reconstruction to binary32 after each plane."
)]
pub(super) fn counterexamples() -> Result<Value> {
    let d0 = pq2::half(0x3c01)?;
    let d1 = pq2::half(0x3787)?;
    let q8 = f32::from_bits(0x3da0_0001);
    let factor0 = d0 * q8;
    let factor1 = d1 * q8;
    let pinned = factor1.mul_add(-1664.0, factor0.mul_add(384.0, 0.0));
    require(
        pinned.to_bits() == 0xc1f9_0b01,
        "PinnedSwift golden mismatch.",
    )?;
    let mut reconstructed = 0.0_f32;
    for (bit, coefficient) in [1, 2, 4, 8, 16, 32, 64, -128].iter().enumerate() {
        let first = f64::from((3_u16 >> bit) & 1) * f64::from(d0);
        let second = f64::from((243_u16 >> bit) & 1) * f64::from(d1);
        let plane = round_half((128.0 * first + 128.0 * second) as f32)?;
        reconstructed = (f64::from(reconstructed)
            + f64::from(*coefficient) * f64::from(plane) * f64::from(q8))
            as f32;
    }
    let rounded = round_half(2049.0)?;
    Ok(
        json!({"singleBlockK128ExactRange":{"k":128,"minPlaneDot":-128,"maxPlaneDot":256,"fp16ExactIntegerCeiling":2048,"allPlaneDotsExactInFp16":true},"twoBlockK256DistinctScalesCounterexample":{"k":256,"block0ScaleHalfBits":"3c01","block1ScaleHalfBits":"3787","q8kSuperblockScaleF32Bits":"3da00001","block0IntegerDot":384,"block1IntegerDot":-1664,"unweightedCombinedIntegerDot":-1280,"unweightedScaleFactorable":false,"pinnedSwiftPer128BlockFmaF32Bits":format!("{:08x}",pinned.to_bits()),"hmxBakedScaleK256Fp16StoreF32Bits":format!("{:08x}",reconstructed.to_bits()),"matchesPinnedSwiftBitwise":pinned.to_bits()==reconstructed.to_bits()},"multiBlockFp16IntegerTruncation":{"hmxOutputStoreMacro":"HMX_STORE_AFTER_F16 (__fp16 *vtcm_output at hmx-mm-kernels-tiled.h:987,1034)","fp16SignificandBits":11,"firstInexactPositiveIntegerInFp16":2049,"fp16RoundedValueFor2049":rounded as i32,"maxUnweightedPlaneDotAtK5120":10240}}),
    )
}
pub(super) fn report(root: &Path) -> Result<Value> {
    let candidate = "config/hexagon-signed-q2-candidate.lock.json";
    let packed = "tools/hexagon/packed-sources.json";
    require(
        read(root, candidate)?["candidate"]["gitlink"]["commit"]
            == "19cce6919992fdffd92e0ac152caaf1e2b8f254e",
        "Candidate ggml revision changed.",
    )?;
    require(
        read(root, packed)?["independentNativeCapture"]["pq2Dot"]["expectedF32Bits"] == "c1f90b01",
        "Packed-source golden changed.",
    )?;
    let mut legacy = chunks(17408, 65536, 1)?;
    let mut optimized = chunks(17408, 2_097_152, 8)?;
    let small32 = chunks(32, 2_097_152, 8)?;
    let small64 = chunks(64, 2_097_152, 8)?;
    require(
        legacy["nChunk"] == 128
            && legacy["vtcmSizeBytes"] == 57344
            && legacy["chunksPerOperation"] == 136
            && optimized["nChunk"] == 4352
            && optimized["vtcmSizeBytes"] == 1_576_960
            && optimized["chunksPerOperation"] == 4
            && small32["nChunk"] == 32
            && small32["vtcmSizeBytes"] == 22528
            && small64["nChunk"] == 64
            && small64["vtcmSizeBytes"] == 34816,
        "Independent HMX solver derivation changed.",
    )?;
    let counterexamples = counterexamples()?;
    require(
        counterexamples["twoBlockK256DistinctScalesCounterexample"]["matchesPinnedSwiftBitwise"]
            == false,
        "K256 counterexample unexpectedly matched.",
    )?;
    legacy["powerMode"] = json!(4);
    legacy["powerModeName"] = json!("HTP_POWER_MODE_BALANCED");
    legacy["divNActThreads"] = json!({"mul":1,"add":0,"shift":0});
    optimized["powerMode"] = json!(7);
    optimized["powerModeName"] = json!("HTP_POWER_MODE_BURST");
    optimized["divNActThreads"] = json!({"mul":1,"add":0,"shift":2});
    for (field, original) in [
        ("chunkBarrierReductionFactor", "chunksPerOperation"),
        ("hvxWorkerParallelismFactor", "hvxThreads"),
        ("actWorkerParallelismFactor", "actThreads"),
    ] {
        let (numerator, denominator) = if field == "chunkBarrierReductionFactor" {
            (integer(&legacy, original)?, integer(&optimized, original)?)
        } else {
            (integer(&optimized, original)?, integer(&legacy, original)?)
        };
        require(
            numerator.is_multiple_of(denominator),
            "Worker ratio must be exact.",
        )?;
        optimized[field] = json!(numerator / denominator);
    }
    optimized["smallRowControls"] = json!({"rows32":{"nChunk":small32["nChunk"],"vtcmSizeBytes":small32["vtcmSizeBytes"]},"rows64":{"nChunk":small64["nChunk"],"vtcmSizeBytes":small64["vtcmSizeBytes"]}});
    let mut report = contract(contracts::TILING)?;
    report["sources"] = sources(
        root,
        &[
            candidate,
            packed,
            "tools/hexagon/hmx_projection.rs",
            "tools/hexagon/q2_integer.rs",
            &format!("{ROOT}tiling.rs"),
            &format!("{ROOT}pq2.rs"),
            &format!("{ROOT}contracts.rs"),
            &format!("{ROOT}mod.rs"),
        ],
    )?;
    report["multiBlockKTilingAudit"] = json!({"supportedWithPinnedSwiftBitwiseParity":false,"hmx2dKernel":counterexamples,"hvxQuantRowKernel":contract(contracts::HVX)?});
    report["signedBinaryExecutionOptimizations"] = json!({"hardwareCapabilitiesOnSnapdragonX2V81":{"hvx128Contexts":8,"vtcmPageBytes":8_388_608,"hmxUnits":1},"previousSessionAndSolverConfig":legacy,"optimizedSessionAndSolverConfig":optimized});
    Ok(report)
}
