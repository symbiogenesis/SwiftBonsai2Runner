#Requires -Version 7.6
param([string]$Test)
. "$PSScriptRoot/pq2_bridge.ps1"
. "$PSScriptRoot/Hexagon.Tests.ps1"
$tests=[ordered]@{}
$tests.test_known_byte_fixture_splits_at_element_64_and_keeps_fourth_symbol={
    $source=[byte[]](@(0,60)+(@(228)*16)+(@(27)*16));$expected=[byte[]](@(0,60)+(@(228)*16)+@(0,60)+(@(27)*16))
    Assert-Hexagon (Test-HexagonEqual (Convert-Pq2ToQ2 $source 128) $expected) 'known bridge bytes differ'
    $values=@((-1.0,0.0,1.0,2.0)*16+(2.0,1.0,0.0,-1.0)*16);$bits=[byte[]]@($values | ForEach-Object {[BitConverter]::GetBytes([float]$_)})
    Assert-Hexagon (Test-HexagonEqual (Get-PackedF32Bits $source 34) $bits) 'PQ2 known f32 bits differ'
    Assert-Hexagon (Test-HexagonEqual (Get-PackedF32Bits $expected 18) $bits) 'Q2 known f32 bits differ'
    Assert-Hexagon (Test-HexagonEqual (Test-Pq2Payload $source 128).RawCodeCounts @(32,32,32,32)) 'fourth symbol lost'
}
$tests.test_every_packed_byte_and_lane_uses_native_little_endian_order={
    foreach ($packed in 0..255) {
        $source=[byte[]](@(0,56)+(@($packed)*32));$actual=Get-PackedF32Bits (Convert-Pq2ToQ2 $source 128) 18
        $values=@(@(foreach ($shift in @(0,2,4,6)) {((($packed -shr $shift)-band 3)-1)*0.5})*32)
        $expected=[byte[]]@($values | ForEach-Object {[BitConverter]::GetBytes([float]$_)})
        Assert-Hexagon (Test-HexagonEqual $actual $expected) ('native lane mismatch: '+$packed)
    }
}
$tests.test_extreme_signed_subnormal_and_zero_scales_keep_raw_bits={
    foreach ($bits in @(0,32768,1,32769,1023,33791,1024,33792,15360,48128,31743,64511)) {$source=[byte[]]([BitConverter]::GetBytes([ushort]$bits)+[byte[]](@(228)*32));$report=Test-Pq2Payload $source 128;Assert-Hexagon ($report.DecodedF32Bits -ceq 'exact') 'extreme scale mismatch'}
}
$tests.test_unaligned_multirow_and_odd_group_payload_is_exact={
    $random=[Random]::new(20261030)
    foreach ($geometry in @(@(128,1),@(256,2),@(384,3),@(640,1),@(5120,2))) {
        $blocks=$geometry[0]/128*$geometry[1];$source=[byte[]]::new($blocks*34)
        for ($block=0;$block -lt $blocks;$block++) {$blockBytes=[byte[]]::new(34);$random.NextBytes($blockBytes);[Array]::Copy($blockBytes,0,$source,$block*34,34);[Array]::Copy([BitConverter]::GetBytes([ushort](0x3c00+$block)),0,$source,$block*34,2)}
        $report=Test-Pq2Payload $source $geometry[0] $geometry[1];Assert-Hexagon ($report.BridgedBytes -eq $blocks*36 -and $report.DecodedF32Bits -ceq 'exact') 'multirow fixture mismatch'
    }
}
$tests.test_nonfinite_scale_is_rejected_even_for_all_zero_codes={foreach ($bits in @(31744,64512,31745,32767,64513,65535)) {$source=[byte[]]([BitConverter]::GetBytes([ushort]$bits)+[byte[]](@(85)*32));Assert-Rejected {Convert-Pq2ToQ2 $source 128} 'nonfinite'}}
$tests.test_partial_rows_tail_and_oversized_payload_rejected={foreach ($geometry in @(@(0,1),@(127,1),@(129,1),@(128,0),@(128,-1))) {Assert-Rejected {Convert-Pq2ToQ2 ([byte[]]::new(34)) $geometry[0] $geometry[1]}};foreach ($length in @(0,33,35)) {Assert-Rejected {Convert-Pq2ToQ2 ([byte[]]::new($length)) 128}};Assert-Rejected {Convert-Pq2ToQ2 ([byte[]]::new(1048577)) 128}}
$tests.test_inverse_rejects_unequal_scale_bits_including_signed_zero={$bridged=Convert-Pq2ToQ2 ([byte[]]::new(34)) 128;$bridged[19]=128;Assert-Rejected {Convert-Q2ToPq2 $bridged 128} 'raw scale bits';Assert-Rejected {Convert-Q2ToPq2 $bridged[0..34] 128}}
$tests.test_retained_inventory_storage_cost_is_exact={
    $report=Get-Pq2StoragePlan (Read-HexagonJson (Join-Path $script:HexagonRoot 'config/swift.metadata.json')) (Read-HexagonJson (Join-Path $script:HexagonRoot 'config/swift.pq2-census.json'))
    Assert-Hexagon ($report.SourcePackedBytes -eq 7137280000 -and $report.BridgedPackedBytes -eq 7557120000 -and $report.AdditionalPackedBytes -eq 419840000 -and $report.ArtifactBytesBeforeAlignmentChanges -eq 7626008928 -and $report.InventoryOnly) 'storage census mismatch'
}
$tests.test_changed_identity_geometry_and_transform_fail_closed={
    $metadata=Read-HexagonJson (Join-Path $script:HexagonRoot 'config/swift.metadata.json');$census=Read-HexagonJson (Join-Path $script:HexagonRoot 'config/swift.pq2-census.json')
    foreach ($pair in @(@('ArtifactSha256',('0'*64)),@('Blocks',1),@('CodePlusTwo',1),@('NonfiniteScaleBlocks',1),@('TensorCount',$false),@('SerializedBytesRead',-1),@('CodecCommit',('0'*40)))) {$changed=$census.Clone();$changed[$pair[0]]=$pair[1];Assert-Rejected {Get-Pq2StoragePlan $metadata $changed}}
    $metadata.Scalars.Remove('prism.hadamard.gdn_v_grouped');Assert-Rejected {Get-Pq2StoragePlan $metadata $census} 'transform'
}
$tests.test_duplicate_identity_fields_are_rejected={Assert-Rejected {ConvertFrom-HexagonJson ([Text.Encoding]::UTF8.GetBytes('{"ArtifactSha256":"a","ArtifactSha256":"b"}'))} 'duplicate JSON'}
$tests.test_inventory_validation_rejects_ambiguous_types_and_unbounded_extents={
    $metadata=Read-HexagonJson (Join-Path $script:HexagonRoot 'config/swift.metadata.json');$census=Read-HexagonJson (Join-Path $script:HexagonRoot 'config/swift.pq2-census.json')
    foreach ($pair in @(@('ModelRepository',@('ukisai','model')),@('ModelRevision',@('revision')),@('ModelRepository',''))) {$left=$metadata.Clone();$right=$census.Clone();$left[$pair[0]]=$pair[1];$right[$pair[0]]=$pair[1];Assert-Rejected {Get-Pq2StoragePlan $left $right} 'string identity'}
    foreach ($name in @('Blocks','Elements','TensorCount','SerializedBytesRead')) {$changed=$census.Clone();$changed[$name]=[bigint]::Pow(2,64);Assert-Rejected {Get-Pq2StoragePlan $metadata $changed} 'integer field'}
    foreach ($constant in @('NaN','Infinity','-Infinity')) {Assert-Rejected {ConvertFrom-HexagonJson ([Text.Encoding]::UTF8.GetBytes('{"unused":'+$constant+'}'))}}
    Assert-Rejected {ConvertFrom-HexagonJson ([Text.Encoding]::UTF8.GetBytes('{"nested":'+('['*5000)+'0'+(']'*5000)+'}'))}
}
Invoke-HexagonTests $tests $Test
