"""Model-free unit tests for the static v0.10.0 HTP K>128 tiling, DSP reduction, and 8-HVX/2MiB-VTCM solver audit."""

import importlib.util
import json
from pathlib import Path
import unittest


SPEC = importlib.util.spec_from_file_location(
    "verify_k_tiling_and_reduction_audit",
    Path(__file__).with_name("verify_k_tiling_and_reduction_audit.py"),
)
AUDIT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(AUDIT)


class KTilingAndReductionAuditTests(unittest.TestCase):
    def test_single_block_k128_plane_dots_are_exact_in_fp16_while_multi_block_overflows(self):
        # Every integer in -2048..=2048 is exact in IEEE-754 binary16 (11-bit significand).
        for integer in range(-2048, 2049):
            self.assertEqual(AUDIT.f16_round(integer), float(integer))
        # At K=128 with Q2 symbols in {-1, 0, +1, +2} and binary plane in {0, 1},
        # the dot product is bounded by [-128, +256], strictly within [-2048, +2048].
        report = AUDIT.verify_multi_block_k_tiling_counterexamples()
        self.assertTrue(report["singleBlockK128ExactRange"]["allPlaneDotsExactInFp16"])
        # Above 2048 (possible when accumulating >= 9 blocks of +2 symbols in HMX_STORE_AFTER_F16),
        # odd integers lose their least-significant bit in FP16.
        self.assertEqual(report["multiBlockFp16IntegerTruncation"]["fp16RoundedValueFor2049"], 2048)

    def test_two_block_k256_distinct_scales_cannot_preserve_pinned_swift_fma(self):
        report = AUDIT.verify_multi_block_k_tiling_counterexamples()
        counterexample = report["twoBlockK256DistinctScalesCounterexample"]
        self.assertEqual(counterexample["pinnedSwiftPer128BlockFmaF32Bits"], "c1f90b01")
        self.assertNotEqual(
            counterexample["hmxBakedScaleK256Fp16StoreF32Bits"],
            counterexample["pinnedSwiftPer128BlockFmaF32Bits"],
        )
        self.assertFalse(counterexample["matchesPinnedSwiftBitwise"])
        self.assertFalse(counterexample["unweightedScaleFactorable"])

    def test_dsp_plane_reduction_opcodes_are_not_supported_for_pinned_swift_bitplanes(self):
        reduction = AUDIT.verify_dsp_plane_reduction_audit()
        self.assertEqual(reduction["hmxMinimumRowPredicate"]["constant"], "HTP_MM_HMX_MIN_NROWS = 4")
        self.assertEqual(reduction["sumRowsAxisMismatch"]["opcode"], "HTP_OP_SUM_ROWS (27)")
        self.assertFalse(reduction["missingSignedWeightedPlaneFmaOpcode"]["supportedBySignedV0100Package"])

    def test_hmx_2d_solver_selects_4352_row_chunks_with_2mib_vtcm_and_8_hvx_workers(self):
        legacy = AUDIT.solve_hmx_2d_chunks(rows=17_408, vtcm_budget=65_536, n_threads=1)
        self.assertEqual(legacy["nChunk"], 128)
        self.assertEqual(legacy["vtcmSizeBytes"], 57_344)
        self.assertEqual(legacy["chunksPerOperation"], 136)
        self.assertEqual(legacy["chunksPer40OpProjection"], 5440)

        optimized = AUDIT.solve_hmx_2d_chunks(rows=17_408, vtcm_budget=2_097_152, n_threads=8)
        self.assertEqual(optimized["hvxThreads"], 8)
        self.assertEqual(optimized["actThreads"], 4)
        self.assertEqual(optimized["nChunk"], 4352)
        self.assertEqual(optimized["vtcmSizeBytes"], 1_576_960)
        self.assertEqual(optimized["chunksPerOperation"], 4)
        self.assertEqual(optimized["chunksPer40OpProjection"], 160)
        self.assertEqual(optimized["tailWasteRows"], 0)

        for rows, expected_vtcm in ((32, 22_528), (64, 34_816)):
            small = AUDIT.solve_hmx_2d_chunks(rows=rows, vtcm_budget=2_097_152, n_threads=8)
            self.assertEqual(small["nChunk"], rows)
            self.assertEqual(small["vtcmSizeBytes"], expected_vtcm)

    def test_solver_rejects_invalid_geometries_and_undersized_vtcm(self):
        for rows, vtcm, threads in ((0, 2_097_152, 8), (33, 2_097_152, 8), (32, 16_384, 8), (32, 2_097_152, 0), (32, 2_097_152, 9)):
            with self.assertRaises(ValueError):
                AUDIT.solve_hmx_2d_chunks(rows=rows, vtcm_budget=vtcm, n_threads=threads)

    def test_retained_static_audit_json_matches_derived_report(self):
        derived = AUDIT.derive()
        retained_path = AUDIT.ROOT / AUDIT.RETAINED
        self.assertTrue(retained_path.is_file())
        retained = json.loads(AUDIT.read(AUDIT.RETAINED))
        self.assertEqual(retained, derived)


if __name__ == "__main__":
    unittest.main()
