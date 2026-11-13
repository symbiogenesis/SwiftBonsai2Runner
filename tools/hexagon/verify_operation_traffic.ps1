#Requires -Version 7.6
param([switch]$Emit,[switch]$Write)
. "$PSScriptRoot/Hexagon.Common.ps1"
$script:OperationLedger='docs/evidence/hexagon-swift-operation-ledger.json'
$script:TrafficLedger='docs/evidence/hexagon-swift-traffic-ledger.json'
function Get-OperationTraffic {
    $sources=@($script:OperationLedger,'tools/hexagon/packed_projection.rs','tools/hexagon/q2_integer.rs','tools/hexagon/q2_projection.rs','tools/hexagon/hmx_projection.rs','tools/hexagon/packed-sources.json','tools/hexagon/verify_operation_traffic.ps1','tools/hexagon/Hexagon.Common.ps1')
    $ledger=ConvertFrom-HexagonJson (Read-HexagonSource $script:OperationLedger)
    Assert-Hexagon ($ledger.artifact.tensorKind -eq 142 -and $ledger.assumptions.originalBlockColumns -eq 128 -and $ledger.assumptions.maximumWeightRowsPerOperation -eq 17408) 'original PQ2 scheduling contract changed'
    $families=@(foreach ($family in $ledger.projectionFamilies) {
        $layers=Get-HexagonInteger $family layers;$columns=Get-HexagonInteger $family columns;$rows=Get-HexagonInteger $family weightRows
        Assert-Hexagon ($layers -gt 0 -and $columns -gt 0 -and $rows -gt 0 -and $columns % 256 -eq 0 -and $rows % 32 -eq 0) 'positive integer complete Q8_K and 32-row extents required'
        $rowTiles=[bigint]::Divide($rows+17407,17408);$operations=$layers*([bigint]::Divide($columns,128))*$rowTiles;$blocks=$layers*$rows*([bigint]::Divide($columns,128))
        Assert-Hexagon ($operations -eq $family.operationsPerToken -and $rowTiles -eq $family.rowTiles) 'family operation geometry disagrees with source ledger'
        @{tensor=$family.tensor;operations=[long]$operations;original128Blocks=[long]$blocks;originalWeightBytes=[long]($blocks*34);hmxRepackedWeightBytes=[long]($blocks*36);hmxActivationBytes=[long]($operations*4096);hmxReturnedPlaneBytes=[long]($blocks*32);guardedRepackedWeightBytes=[long]($blocks*72);guardedActivationBytes=[long]($operations*2048);guardedReturnedLimbBytes=[long]($blocks*8)}
    })
    $totals=@{};foreach ($key in $families[0].Keys) {if ($key -cne 'tensor') {$sum=0L;foreach ($family in $families) {$sum+=$family[$key]};$totals[$key]=$sum}}
    Assert-Hexagon ($totals.operations -eq $ledger.totals.evaluateOperationsPerToken) 'complete evaluate operation total changed'
    $totals.hmxSubmissionAndResultPayloadBytes=$totals.hmxRepackedWeightBytes+$totals.hmxActivationBytes+$totals.hmxReturnedPlaneBytes
    $totals.guardedSubmissionAndResultPayloadBytes=$totals.guardedRepackedWeightBytes+$totals.guardedActivationBytes+$totals.guardedReturnedLimbBytes
    $packed=ConvertFrom-HexagonJson (Read-HexagonSource 'tools/hexagon/packed-sources.json');$control=$packed.hmxIntegerBitplaneProjectionControl
    foreach ($pair in @(@('hmxRepackedWeightBytes','fullMFirstProjectionRepackedWeightTransferBytes'),@('hmxActivationBytes','fullMFirstProjectionActivationTransferBytes'),@('hmxReturnedPlaneBytes','fullMFirstProjectionPlaneReturnBytes'))) {Assert-Hexagon ($families[0][$pair[0]] / 64 -eq $control[$pair[1]]) 'whole-model derivation disagrees with first-projection control'}
    return @{schemaVersion=1;kind='static-original-pq2-bridge-logical-payload-ledger';scope=$ledger.scope;sourceIdentityEncoding='UTF8-with-LF-newlines';sources=@($sources | ForEach-Object {@{path=$_;sha256=(Get-HexagonHash (Read-HexagonSource $_))}});factors=@{original128BlockBytes=34;hostTileRows=32;hostTileBytes=576;hmxHostTilesPer32Rows=2;guardedHostTilesPer32Rows=4;hmxPlanesPerOriginalBlock=8;guardedLimbsPerOriginalBlock=2;f32Bytes=4;hmxActivationBytesPerOperation=4096;guardedActivationBytesPerOperation=2048};projectionFamilies=$families;totals=$totals;limitations=@('All listed original-PQ2 projections selected once, including final logits; PTQ1 is outside this derivation.','Payloads follow existing source packing and result layouts, with no persistent device weight reuse.','Every listed weight-row extent is a multiple of 32, so mapped row padding adds no payload here.','Device/DRAM/bus traffic, bandwidth, latency, energy and speedup are unmeasured.','Excludes source reads, extra host copies, scale scans, preparation, reduction, queue descriptors, cache traffic, mappings and native/DSP resources.','Aggregate per-token payload is not simultaneous memory residency or a capacity admission amount.','Smaller row tiles may increase activation payload and operation count; partial placement changes all totals.');newOwnerDeviceExecutionMilliseconds=0;deviceOpened=$false;modelLoaded=$false;profileEligible=$false;measuredInferenceSpeedup=$null}
}
if ($MyInvocation.InvocationName -ne '.') {
    Assert-Hexagon (-not ($Emit -and $Write)) 'emit and write are mutually exclusive';$expected=Get-OperationTraffic
    if ($Write) {Write-HexagonJson (Join-Path $script:HexagonRoot $script:TrafficLedger) $expected} elseif ($Emit) {$expected | ConvertTo-Json -Depth 64} else {Assert-Hexagon (Test-HexagonEqual (Read-HexagonJson (Join-Path $script:HexagonRoot $script:TrafficLedger)) $expected) 'retained traffic ledger differs from source-derived payloads/identities';Write-Output 'Original-PQ2 logical payload ledger verified; no device, model or performance measurement.'}
}
