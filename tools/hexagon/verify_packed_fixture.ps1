#Requires -Version 7.6
. "$PSScriptRoot/Hexagon.Common.ps1"
function Test-PackedFixture {
    $manifest=Read-HexagonJson (Join-Path $PSScriptRoot 'packed-sources.json')
    $hmx=$manifest.hmxIntegerBitplaneProjectionControl
    $expected=@{source='hmx_projection.rs';independentControls='hmx_projection_tests.rs';primitiveK=128;primitiveN=8;activationPlaneCoefficients=@(1,2,4,8,16,32,64,-128);activationPlaneValues=@(0,1);minimumPlaneDot=-128;maximumPlaneDot=256;weightRowsPerHostTile=32;weightColumnsPerHostTile=64;hostTileBytes=576;hostTilesPer32WeightRows=2;maximumColumns=5120;maximumRows=17408;fullMFirstProjectionOperations=40;fullMFirstProjectionPeakRepackedStageBytes=626688;fullMFirstProjectionPreparedQ8KBytes=5840;fullMFirstProjectionPlaneActivationBytes=4096;fullMFirstProjectionPeakPlaneOutputBytes=557056;fullMFirstProjectionPrivateFinalOutputBytes=69632;fullMFirstProjectionOwnedPayloadBytes=1263312;fullMFirstProjectionRepackedWeightTransferBytes=25067520;fullMFirstProjectionActivationTransferBytes=163840;fullMFirstProjectionPlaneReturnBytes=22282240}
    foreach ($name in $expected.Keys) {Assert-Hexagon (Test-HexagonEqual $hmx[$name] $expected[$name]) ('HMX projection arithmetic or source inventory changed: '+$name)}
    foreach ($name in @('source','independentControls')) {Assert-Hexagon (Test-Path -LiteralPath (Join-Path $PSScriptRoot $hmx[$name]) -PathType Leaf) 'HMX projection or independent controls missing'}
    foreach ($name in @('bitplaneRowsArePrefillTokens','nativeHmxSessionImplemented','effectiveHmxPlacementQualified','PTQ1ArithmeticQualified','profileEligible')) {Assert-Hexagon ($hmx[$name] -is [bool] -and -not $hmx[$name]) 'HMX CPU controls cannot qualify device or model admission'}
    $control=$manifest.ptq1HmxLaneControl
    $expected=@{source='ptq1_hmx.rs';independentControls='ptq1_hmx_tests.rs';originalBlockWeights=128;originalBlockBytes=28;primitiveK=128;primitiveN=8;q8GroupsPerOriginalBlock=4;sparseWeightRowsPerOriginalRow=4;primitiveOperationsPerOriginalBlockAndRowTile=4;integerProductsPerNativeLane=8;planeDotMinimum=-8;planeDotMaximum=8;nativeLaneMinimum=-1024;nativeLaneMaximum=1024;laneAccumulatorCount=4;paddedHostStageBytes=1152}
    foreach ($name in $expected.Keys) {Assert-Hexagon (Test-HexagonEqual $control[$name] $expected[$name]) ('PTQ1 native-lane arithmetic contract changed: '+$name)}
    foreach ($name in @('source','independentControls')) {Assert-Hexagon (Test-Path -LiteralPath (Join-Path $PSScriptRoot $control[$name]) -PathType Leaf) 'PTQ1 lane control source missing'}
    foreach ($name in @('deviceParityQualified','actualModelParityQualified','profileEligible')) {Assert-Hexagon ($control[$name] -is [bool] -and -not $control[$name]) 'PTQ1 CPU controls cannot qualify native/model admission'}
    foreach ($entry in @(@{fixture=$control.nativeDotFixture;bytes=508;digest='781fa202c4d23e2a78cf362e3d84e008dd647c31606ba92b7869f5c2168d439c';provenance='21885745d1603934a59ba3ba2b491911f07eb07362003e76a69545140f5b09ba'},@{fixture=$manifest.independentNativeCapture.preparationFixture;bytes=584;digest='86d985b8a60fde3241daeeb1712dac45a0cc8e4684ac962dcb488ef17bb1fba5';provenance='131155bb668a4ca3c4897ff5b270a6f9560e83da173fd6d14cf1dd3228b0c7cd'})) {
        $fixture=$entry.fixture;$data=Read-HexagonBytes (Join-Path $PSScriptRoot $fixture.path)
        Assert-Hexagon ($data.Length -eq $entry.bytes -and $fixture.bytes -eq $entry.bytes -and (Get-HexagonHash $data) -ceq $entry.digest -and $fixture.sha256 -ceq $entry.digest) 'independent native fixture bytes changed'
        $raw=[Text.Encoding]::UTF8.GetBytes([Text.Encoding]::UTF8.GetString((Read-HexagonBytes (Join-Path $PSScriptRoot $fixture.provenancePath))).Replace("`r`n","`n"))
        Assert-Hexagon ((Get-HexagonHash $raw) -ceq $entry.provenance -and $fixture.provenanceSha256 -ceq $entry.provenance -and $fixture.provenanceDigestEncoding -ceq 'UTF8-with-LF-newlines') 'native provenance changed'
        $observation=ConvertFrom-HexagonJson $raw
        Assert-Hexagon ($observation.fixture_sha256 -ceq $entry.digest -and $observation.fixture_bytes -eq $entry.bytes -and $observation.runtime_commit -ceq '73bfe6cc485554822f80c473a3c0c4f25e80f41b' -and $observation.ggml_cpu_sha256 -ceq '701b354a38b25e5c6dc57a1559f71cd26b2d0f8d69c1576a0e77a59a4273d2ab') 'native capture identities disagree'
        if ($entry.bytes -eq 584) {
            Assert-Hexagon ($observation.cases.Count -eq 2) 'native case inventory changed'
            for ($i=0;$i -lt 2;$i++) {Assert-Hexagon ((Get-HexagonHash $data[($i*292)..($i*292+291)]) -ceq $observation.cases[$i].native_block_sha256) 'native case digest changed'}
        }
    }
}
if ($MyInvocation.InvocationName -ne '.') {Test-PackedFixture;Write-Output 'Retained native Q8_K/PTQ1 fixture identities verified; device/Swift gates remain pending.'}
