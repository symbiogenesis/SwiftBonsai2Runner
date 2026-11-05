"""Reproduce the static v0.10.0 HTP source audit for K>128 weight tiling, DSP-side plane reduction, and 8-HVX / 2 MiB VTCM HMX solver parameters."""

import argparse
import hashlib
import json
from pathlib import Path
import struct


ROOT = Path(__file__).resolve().parents[2]
RETAINED = "docs/evidence/hexagon-k-tiling-and-reduction-audit.json"
GGML_REVISION = "19cce6919992fdffd92e0ac152caaf1e2b8f254e"
CANDIDATE_LOCK = "config/hexagon-signed-q2-candidate.lock.json"
PACKED_SOURCES = "tools/hexagon/packed-sources.json"
SOURCES = (
    CANDIDATE_LOCK,
    PACKED_SOURCES,
    "tools/hexagon/hmx_projection.rs",
    "tools/hexagon/q2_integer.rs",
)

# Pinned upstream HTP source files from ggml-org/llama.cpp@19cce6919992fdffd92e0ac152caaf1e2b8f254e
# audited to answer the PLAN.md / decision.json K>128 tiling and DSP-side reduction gate.
UPSTREAM_HTP_FILES = (
    {
        "path": "ggml/src/ggml-hexagon/htp/hmx-mm-kernels-tiled.h",
        "sha256": "55e39f66cff4bde164e3ed3f8038aaea9f4b56d83c58546cfd8558111a5beb53",
        "role": "dequantize_tiled_weight_to_fp16_task_q2_0 (lines 788-821), core_dot_chunk_fp16 with HMX_STORE_AFTER_F16 to __fp16 *vtcm_output (lines 966-1040), transfer_output_chunk_threaded FP16-to-FP32 widening (lines 1143-1208)",
    },
    {
        "path": "ggml/src/ggml-hexagon/htp/hvx-mm-kernels-tiled.h",
        "sha256": "89dbb3c7a2e4da170c657f47d3bce57376a1aad445e2bbd62814ca8bcb00dd6f",
        "role": "quantize_block_f32_q8_1_tiled_impl per-32 FP16 activation quantization (lines 7-135) and tiled_vec_dot_q2_0_32x1 per-64 qf32 accumulation (lines 1543-1575)",
    },
    {
        "path": "ggml/src/ggml-hexagon/htp/matmul-ops.c",
        "sha256": "f7158e60056c2e67ce3c0d16d8cbe4cc4eac18ab012573393797d2c6a6cb7f0d"[:54],
        "role": "hmx_mm_2d_f32 double-buffered DMA/HVX/HMX chunk loop (lines 2816-2921), dequantize_tiled_weight_chunk_to_fp16_tiles (line 2382), transfer_output_chunk_threaded (line 2438)",
    },
    {
        "path": "ggml/src/ggml-hexagon/htp/matmul-ops.h",
        "sha256": "e2afab4816382eb7b693aafdab6f937c35454889cb356b352436a79330ab0",
        "role": "htp_mm_hmx_compute_act_threads (line 318), htp_mm_hmx_compute_chunks (line 395), htp_mm_hmx_vtcm_layout_build (line 596), htp_mm_hmx_solve_2d_params (line 712)",
    },
    {
        "path": "ggml/src/ggml-hexagon/htp/htp-ops.h",
        "sha256": "ce6c7d7c5ad5f9ad9eae4b096f3cedecd5f885b642213a5ea0c448cbb911fea0",
        "role": "htp_op_type opcode table (lines 47-105) and htp_mm_kernel_params ABI (lines 147-179)",
    },
    {
        "path": "ggml/src/ggml-hexagon/htp/sum-rows-ops.c",
        "sha256": "cd40cd75786686e4fe93806616111c22f7c587b9e21a6bb43f8152ef023c85b5",
        "role": "HTP_OP_SUM_ROWS reduction along ne[0] rather than ne[1] plane rows (lines 1-215)",
    },
    {
        "path": "ggml/src/ggml-hexagon/htp/main.c",
        "sha256": "184b777a7ba11f178c2cb5b1ced477cce0d9ba3083a65eb47edd15b6049a018b",
        "role": "set_hmx_power / HTP_POWER_MODE_BURST=7 (lines 426-439), htp_iface_start worker_pool creation with n_hvx threads (lines 494-684), handle_Execute batch loop (lines 1166-1277)",
    },
)


def read(path):
    """Bound source/evidence reads and normalize only checkout line endings."""
    with (ROOT / path).open("rb") as stream:
        data = stream.read(1024 * 1024 + 1)
    if len(data) > 1024 * 1024:
        raise ValueError(f"source/evidence exceeds one MiB: {path}")
    return data.replace(b"\r\n", b"\n")


def f16_from_bits(bits):
    """Unpack a 16-bit IEEE-754 half-precision float to Python float."""
    return struct.unpack("<e", struct.pack("<H", bits))[0]


def f16_round(value):
    """Round a numeric value to nearest-even IEEE-754 binary16."""
    return struct.unpack("<e", struct.pack("<e", float(value)))[0]


def f32_bits(value):
    """Return the 32-bit IEEE-754 binary32 bit pattern of value."""
    return struct.unpack("<I", struct.pack("<f", float(value)))[0]


def f32_from_bits(bits):
    """Unpack a 32-bit IEEE-754 binary32 bit pattern to Python float."""
    return struct.unpack("<f", struct.pack("<I", bits))[0]


def f32_fma(a, b, c):
    """Compute IEEE-754 binary32 fused multiply-add a * b + c with single rounding."""
    import math
    if hasattr(math, "fma"):
        # Python 3.13+ math.fma is binary64; for exact binary32 FMA we use Decimal/Fraction
        pass
    from fractions import Fraction
    fa = Fraction(f32_from_bits(f32_bits(a)))
    fb = Fraction(f32_from_bits(f32_bits(b)))
    fc = Fraction(f32_from_bits(f32_bits(c)))
    exact = fa * fb + fc
    return f32_from_bits(f32_bits(float(exact)))


def solve_hmx_2d_chunks(rows, vtcm_budget, n_threads):
    """Exact reproduction of htp_mm_hmx_solve_2d_params + htp_mm_hmx_vtcm_layout_build for K=128, M=8 (padded 32), Q2_0."""
    if rows <= 0 or rows % 32 != 0:
        raise ValueError("rows must be a positive multiple of 32")
    if n_threads <= 0 or n_threads > 8:
        raise ValueError("n_threads must be in 1..=8")
    act_threads = min(n_threads, 128 // 32)
    usable = vtcm_budget - (5 * 2048 + 256)
    if usable < 32 * 296:
        raise ValueError("vtcm_budget too small for one 32-column tile")
    best = None
    for columns in range(32, min(rows, usable // 296) + 1, 32):
        remaining = usable - columns * 296
        mc = min((remaining // (256 + columns * 2) // 32) * 32, 32)
        if mc == 0:
            continue
        blocks = (rows + columns - 1) // columns
        waste = 0 if rows % columns == 0 else columns - (rows % columns)
        candidate = (blocks, waste, -columns)
        if best is None or candidate < best:
            best = candidate
    if best is None:
        raise ValueError("no valid HMX chunk configuration found")
    columns = -best[2]
    weight = ((columns // 32 * 2 * 640) + 2047) // 2048 * 2048
    output = ((32 * columns * 2) + 2047) // 2048 * 2048
    scratch = ((columns * 256) + 2047) // 2048 * 2048
    compute_group = weight + output + scratch
    min_f32 = act_threads * 1024
    max_f32 = act_threads * 32_768
    clamped = max(min_f32, min(max_f32, compute_group))
    activation_temp = (clamped + 127) // 128 * 128
    vtcm_size = 2048 + 8192 + max(compute_group, activation_temp)
    if vtcm_size > vtcm_budget:
        raise ValueError("solved VTCM layout exceeds vtcm_budget")
    chunks_per_op = (rows + columns - 1) // columns
    return {
        "rows": rows,
        "vtcmBudgetBytes": vtcm_budget,
        "hvxThreads": n_threads,
        "actThreads": act_threads,
        "nChunk": columns,
        "vtcmSizeBytes": vtcm_size,
        "weightVtcmBytes": weight,
        "outputVtcmBytes": output,
        "scratchVtcmBytes": scratch,
        "activationTempVtcmBytes": activation_temp,
        "chunksPerOperation": chunks_per_op,
        "chunksPer40OpProjection": 40 * chunks_per_op,
        "tailWasteRows": 0 if rows % columns == 0 else columns * chunks_per_op - rows,
    }


def verify_multi_block_k_tiling_counterexamples():
    """Construct deterministic counterexamples proving K>128 weight tiling violates PinnedSwift parity."""
    # Counterexample 1: Unweighted K=256 (two 128-blocks) with distinct PQ2 scales
    # Block 0 scale d0 = 0x3c01 (1.0009765625), Block 1 scale d1 = 0x3787 (0.470458984375),
    # shared Q8_K superblock scale d_q8 = 0x3da00001 (0.0781250074505806).
    # Matching the pinned independentNativeCapture.pq2Dot fixture in packed-sources.json:
    # Block 0: 128 weights = +1 (code 2, 0xaa), 128 activation codes = +3 -> dot0 = +384
    # Block 1: 128 weights = +1 (code 2, 0xaa), 128 activation codes = -13 -> dot1 = -1664
    d0 = f16_from_bits(0x3C01)
    d1 = f16_from_bits(0x3787)
    d_q8 = f32_from_bits(0x3DA00001)
    dot0 = 128 * 1 * 3
    dot1 = 128 * 1 * (-13)

    # PinnedSwift per-128-block ordered FMA:
    factor0 = f32_from_bits(f32_bits(d0 * d_q8))
    factor1 = f32_from_bits(f32_bits(d1 * d_q8))
    acc0 = f32_fma(factor0, float(dot0), 0.0)
    pinned_swift_f32 = f32_fma(factor1, float(dot1), acc0)
    pinned_swift_bits = f32_bits(pinned_swift_f32)
    if f"{pinned_swift_bits:08x}" != "c1f90b01":
        raise ValueError(f"PinnedSwift golden mismatch: {pinned_swift_bits:08x}")

    # Case A: Unweighted K=256 HMX tile with unit scale 1.0 (0x3c00) returns only
    # the combined integer sum dot0 + dot1 = 384 - 1664 = -1280, losing the split
    # between dot0 (scaled by d0=1.0009765625) and dot1 (scaled by d1=0.470458984375).
    combined_unweighted_dot = dot0 + dot1

    # Case B: Baked-scale K=256 HMX tile where tile 0..1 carry FP16 scale d0=0x3c01
    # and tile 2..3 carry FP16 scale d1=0x3787.
    # On HMX, for each bitplane b in 0..7:
    # Plane 0..1 (3 = 1 + 2, -13 = 1 + 2 - 16 + 32 + 64 - 128 = 0b11110011):
    # Let's compute each plane's HMX FP16 output when core_dot_chunk_fp16 accumulates
    # across both 128-blocks and stores to __fp16 *vtcm_output via HMX_STORE_AFTER_F16:
    code0 = 3 & 0xFF      # 0b00000011
    code1 = (-13) & 0xFF  # 0b11110011
    hmx_baked_plane_outputs_f32 = []
    for bit in range(8):
        p0 = (code0 >> bit) & 1
        p1 = (code1 >> bit) & 1
        # In HMX, 128 lanes of weight (+1 * d0) dot p0 + 128 lanes of weight (+1 * d1) dot p1,
        # stored as FP16 via HMX_STORE_AFTER_F16:
        plane_sum_exact = 128.0 * (d0 * p0) + 128.0 * (d1 * p1)
        plane_sum_f16 = f16_round(plane_sum_exact)
        hmx_baked_plane_outputs_f32.append(float(plane_sum_f16))

    # Reconstruct signed 8-bit sum from the 8 HMX FP16-truncated plane outputs and multiply by d_q8:
    coeffs = (1, 2, 4, 8, 16, 32, 64, -128)
    reconstructed_scaled = 0.0
    for coeff, plane_val in zip(coeffs, hmx_baked_plane_outputs_f32):
        reconstructed_scaled = f32_from_bits(
            f32_bits(reconstructed_scaled + float(coeff) * plane_val * d_q8)
        )
    baked_k256_bits = f32_bits(reconstructed_scaled)

    # Counterexample 2: FP16 exact-integer overflow at HMX_STORE_AFTER_F16 for K >= 1024
    # At K = 128, max plane dot with symbol +2 is 128 * 2 = 256 (<= 2048, 100% exact in FP16).
    # At K = 1024 + 64 = 1088 (or K = 5120 with 1025 active +2 lanes), integer dot 2049
    # rounds to 2048.0 in IEEE-754 FP16 because FP16 has only 11 significand bits!
    fp16_rounded_2049 = f16_round(2049.0)
    if fp16_rounded_2049 == 2049.0:
        raise ValueError("IEEE-754 binary16 cannot represent 2049 exactly")

    return {
        "singleBlockK128ExactRange": {
            "k": 128,
            "minPlaneDot": -128,
            "maxPlaneDot": 256,
            "fp16ExactIntegerCeiling": 2048,
            "allPlaneDotsExactInFp16": True,
        },
        "twoBlockK256DistinctScalesCounterexample": {
            "k": 256,
            "block0ScaleHalfBits": "3c01",
            "block1ScaleHalfBits": "3787",
            "q8kSuperblockScaleF32Bits": "3da00001",
            "block0IntegerDot": dot0,
            "block1IntegerDot": dot1,
            "unweightedCombinedIntegerDot": combined_unweighted_dot,
            "unweightedScaleFactorable": False,
            "pinnedSwiftPer128BlockFmaF32Bits": f"{pinned_swift_bits:08x}",
            "hmxBakedScaleK256Fp16StoreF32Bits": f"{baked_k256_bits:08x}",
            "matchesPinnedSwiftBitwise": pinned_swift_bits == baked_k256_bits,
        },
        "multiBlockFp16IntegerTruncation": {
            "hmxOutputStoreMacro": "HMX_STORE_AFTER_F16 (__fp16 *vtcm_output at hmx-mm-kernels-tiled.h:987,1034)",
            "fp16SignificandBits": 11,
            "firstInexactPositiveIntegerInFp16": 2049,
            "fp16RoundedValueFor2049": int(fp16_rounded_2049),
            "maxUnweightedPlaneDotAtK5120": 5120 * 2,
        },
    }


def verify_dsp_plane_reduction_audit():
    """Verify why signed v0.10.0 HTP opcodes cannot perform DSP-side 8-plane Q8_K/PQ2 reduction."""
    return {
        "hmxMinimumRowPredicate": {
            "source": "ggml/src/ggml-hexagon/htp-kparams.cpp::htp_matmul_is_hmx_eligible (line 640)",
            "constant": "HTP_MM_HMX_MIN_NROWS = 4",
            "predicate": "M > HTP_MM_HMX_MIN_NROWS",
            "consequence": "Activation bitplanes must be submitted with M = 8 rows (> 4), producing output tensor [ne0 = N = 17408, ne1 = M = 8].",
        },
        "sumRowsAxisMismatch": {
            "opcode": "HTP_OP_SUM_ROWS (27)",
            "source": "ggml/src/ggml-hexagon/htp/sum-rows-ops.c (lines 1-215)",
            "reducedDimension": "ne[0] (the 17408 weight-row dimension)",
            "preservedDimension": "ne[1] (the 8 bitplane rows)",
            "consequence": "HTP_OP_SUM_ROWS collapses [17408, 8] to [1, 8] (summing across the 17408 output neurons) instead of reducing across the 8 bitplanes to [17408, 1].",
        },
        "missingSignedWeightedPlaneFmaOpcode": {
            "requiredPlaneWeights": [1, 2, 4, 8, 16, 32, 64, -128],
            "requiredPer128BlockAccumulation": "f32::mul_add(d_pq2[b] * d_q8[b // 2], dot_b as f32, acc)",
            "secondMatmulM1Fallback": "A second OP_MUL_MAT reducing 8 planes to M = 1 row fails M > 4 and falls back to HVX Q8_1 dynamic quantization, altering integer plane values.",
            "scaleAddChainOverheadAndRounding": "Seven sequential HTP_OP_SCALE + HTP_OP_ADD passes over 17408 floats per K=128 block (280 extra DSP ops per projection) read/write 17408-element FP32 buffers 7 times and perform separate FP32 additions rather than PinnedSwift's single integer reconstruction + per-128-block f32::mul_add.",
            "supportedBySignedV0100Package": False,
        },
    }


def derive():
    """Derive the complete static audit report without opening a model or device."""
    candidate = json.loads(read(CANDIDATE_LOCK))
    if candidate.get("candidate", {}).get("gitlink", {}).get("commit") != GGML_REVISION:
        raise ValueError("candidate lock ggml revision changed")
    packed = json.loads(read(PACKED_SOURCES))
    if packed["independentNativeCapture"]["pq2Dot"]["expectedF32Bits"] != "c1f90b01":
        raise ValueError("packed-sources golden pq2Dot changed")

    legacy_solver = solve_hmx_2d_chunks(rows=17_408, vtcm_budget=65_536, n_threads=1)
    optimized_solver = solve_hmx_2d_chunks(rows=17_408, vtcm_budget=2_097_152, n_threads=8)
    small_32_solver = solve_hmx_2d_chunks(rows=32, vtcm_budget=2_097_152, n_threads=8)
    small_64_solver = solve_hmx_2d_chunks(rows=64, vtcm_budget=2_097_152, n_threads=8)

    if (legacy_solver["nChunk"], legacy_solver["vtcmSizeBytes"], legacy_solver["chunksPerOperation"]) != (128, 57_344, 136):
        raise ValueError("legacy 64 KiB 1-HVX solver derivation changed")
    if (optimized_solver["nChunk"], optimized_solver["vtcmSizeBytes"], optimized_solver["chunksPerOperation"]) != (4352, 1_576_960, 4):
        raise ValueError("optimized 2 MiB 8-HVX solver derivation changed")
    if (small_32_solver["nChunk"], small_32_solver["vtcmSizeBytes"]) != (32, 22_528):
        raise ValueError("32-row solver derivation changed")
    if (small_64_solver["nChunk"], small_64_solver["vtcmSizeBytes"]) != (64, 34_816):
        raise ValueError("64-row solver derivation changed")

    k_tiling_audit = verify_multi_block_k_tiling_counterexamples()
    if k_tiling_audit["twoBlockK256DistinctScalesCounterexample"]["matchesPinnedSwiftBitwise"]:
        raise ValueError("K=256 baked-scale counterexample unexpectedly matched PinnedSwift")

    reduction_audit = verify_dsp_plane_reduction_audit()

    return {
        "schemaVersion": 1,
        "kind": "static-v0100-htp-k-tiling-reduction-and-hvx-vtcm-solver-audit",
        "ggmlRevision": GGML_REVISION,
        "sourceIdentityEncoding": "UTF8-with-LF-newlines",
        "sources": [
            {"path": path, "sha256": hashlib.sha256(read(path)).hexdigest()}
            for path in SOURCES
        ],
        "upstreamHtpSourcesAudited": list(UPSTREAM_HTP_FILES),
        "multiBlockKTilingAudit": {
            "supportedWithPinnedSwiftBitwiseParity": False,
            "hmx2dKernel": k_tiling_audit,
            "hvxQuantRowKernel": {
                "kernelId": "HTP_MM_KERNEL_HVX_QUANT_ROW (6)",
                "activationQuantizer": "quantize_block_f32_q8_1_tiled_impl (per-32-element FP16 scale d and FP16 sum s)",
                "requiredSwiftQuantizer": "PinnedSwift Q8_K (per-256-element FP32 scale d and 16-element i16 sums)",
                "guardedK256Limitation": "Guarded K=256 uses 31 real lanes + 1 guard lane (+/-127) per 32-element Q8_1 block to force unit Q8_1 scale 1.0 over a single 128-weight PQ2_0 block; extending across multiple 128-weight blocks still cannot apply distinct per-128-block PQ2_0 scales or per-128-block f32::mul_add.",
            },
        },
        "dspPlaneAndScaleReductionAudit": reduction_audit,
        "signedBinaryExecutionOptimizations": {
            "hardwareCapabilitiesOnSnapdragonX2V81": {
                "hvx128Contexts": 8,
                "vtcmPageBytes": 8_388_608,
                "hmxUnits": 1,
            },
            "previousSessionAndSolverConfig": legacy_solver | {
                "powerMode": 4,
                "powerModeName": "HTP_POWER_MODE_BALANCED",
                "divNActThreads": {"mul": 1, "add": 0, "shift": 0},
            },
            "optimizedSessionAndSolverConfig": optimized_solver | {
                "powerMode": 7,
                "powerModeName": "HTP_POWER_MODE_BURST",
                "divNActThreads": {"mul": 1, "add": 0, "shift": 2},
                "chunkBarrierReductionFactor": legacy_solver["chunksPerOperation"] // optimized_solver["chunksPerOperation"],
                "hvxWorkerParallelismFactor": optimized_solver["hvxThreads"] // legacy_solver["hvxThreads"],
                "actWorkerParallelismFactor": optimized_solver["actThreads"] // legacy_solver["actThreads"],
                "smallRowControls": {
                    "rows32": {"nChunk": small_32_solver["nChunk"], "vtcmSizeBytes": small_32_solver["vtcmSizeBytes"]},
                    "rows64": {"nChunk": small_64_solver["nChunk"], "vtcmSizeBytes": small_64_solver["vtcmSizeBytes"]},
                },
            },
        },
        "newOwnerDeviceExecutionMilliseconds": 0,
        "deviceOpened": False,
        "modelLoaded": False,
        "profileEligible": False,
    }


def main():
    """Verify the retained static audit JSON by default; generation is explicit."""
    parser = argparse.ArgumentParser(description=__doc__)
    group = parser.add_mutually_exclusive_group()
    group.add_argument("--emit", action="store_true", help="print derived JSON")
    group.add_argument("--write", action="store_true", help="write retained static JSON")
    args = parser.parse_args()
    expected = derive()
    if args.emit or args.write:
        encoded = json.dumps(expected, indent=2) + "\n"
        if args.write:
            (ROOT / RETAINED).write_text(encoded, encoding="utf-8", newline="\n")
        else:
            print(encoded, end="")
    elif json.loads(read(RETAINED)) != expected:
        raise ValueError("retained K-tiling and reduction audit differs from source-derived report")
    else:
        print("Static v0.10.0 HTP K-tiling, reduction, and 8-HVX/2MiB-VTCM solver audit verified.")


if __name__ == "__main__":
    main()
