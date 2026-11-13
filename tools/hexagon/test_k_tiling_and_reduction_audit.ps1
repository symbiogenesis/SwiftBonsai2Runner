#Requires -Version 7.6
param([string]$Test)
. "$PSScriptRoot/verify_k_tiling_and_reduction_audit.ps1"
. "$PSScriptRoot/Hexagon.Tests.ps1"
$tests=[ordered]@{}
$tests.test_single_block_k128_plane_dots_are_exact_in_fp16_while_multi_block_overflows={foreach ($integer in -2048..2048) {Assert-Hexagon ((Round-HexagonHalf $integer) -eq $integer) 'representable integer changed'};$report=Get-KTilingCounterexamples;Assert-Hexagon ($report.singleBlockK128ExactRange.allPlaneDotsExactInFp16 -and $report.multiBlockFp16IntegerTruncation.fp16RoundedValueFor2049 -eq 2048) 'FP16 integer boundary changed'}
$tests.test_two_block_k256_distinct_scales_cannot_preserve_pinned_swift_fma={$counter=(Get-KTilingCounterexamples).twoBlockK256DistinctScalesCounterexample;Assert-Hexagon ($counter.pinnedSwiftPer128BlockFmaF32Bits -ceq 'c1f90b01' -and $counter.hmxBakedScaleK256Fp16StoreF32Bits -cne $counter.pinnedSwiftPer128BlockFmaF32Bits -and -not $counter.matchesPinnedSwiftBitwise -and -not $counter.unweightedScaleFactorable) 'K256 counterexample lost'}
$tests.test_dsp_plane_reduction_opcodes_are_not_supported_for_pinned_swift_bitplanes={$reduction=Get-DspReductionAudit;Assert-Hexagon ($reduction.hmxMinimumRowPredicate.constant -ceq 'HTP_MM_HMX_MIN_NROWS = 4' -and $reduction.sumRowsAxisMismatch.opcode -ceq 'HTP_OP_SUM_ROWS (27)' -and -not $reduction.missingSignedWeightedPlaneFmaOpcode.supportedBySignedV0100Package) 'reduction qualification changed'}
$tests.test_hmx_2d_solver_selects_4352_row_chunks_with_2mib_vtcm_and_8_hvx_workers={
    $legacy=Get-HmxChunks 17408 65536 1;Assert-Hexagon ($legacy.nChunk -eq 128 -and $legacy.vtcmSizeBytes -eq 57344 -and $legacy.chunksPerOperation -eq 136 -and $legacy.chunksPer40OpProjection -eq 5440) 'legacy solver changed'
    $optimized=Get-HmxChunks 17408 2097152 8;Assert-Hexagon ($optimized.hvxThreads -eq 8 -and $optimized.actThreads -eq 4 -and $optimized.nChunk -eq 4352 -and $optimized.vtcmSizeBytes -eq 1576960 -and $optimized.chunksPerOperation -eq 4 -and $optimized.chunksPer40OpProjection -eq 160 -and $optimized.tailWasteRows -eq 0) 'optimized solver changed'
    foreach ($pair in @(@(32,22528),@(64,34816))) {$small=Get-HmxChunks $pair[0] 2097152 8;Assert-Hexagon ($small.nChunk -eq $pair[0] -and $small.vtcmSizeBytes -eq $pair[1]) 'small solver changed'}
}
$tests.test_solver_rejects_invalid_geometries_and_undersized_vtcm={foreach ($case in @(@(0,2097152,8),@(33,2097152,8),@(32,16384,8),@(32,2097152,0),@(32,2097152,9))) {Assert-Rejected {Get-HmxChunks $case[0] $case[1] $case[2]}}}
$tests.test_retained_static_audit_json_matches_derived_report={Assert-Hexagon (Test-HexagonEqual (Read-HexagonJson (Join-Path $script:HexagonRoot 'docs/evidence/hexagon-k-tiling-and-reduction-audit.json')) (Get-KTilingAudit)) 'retained audit changed'}
Invoke-HexagonTests $tests $Test
