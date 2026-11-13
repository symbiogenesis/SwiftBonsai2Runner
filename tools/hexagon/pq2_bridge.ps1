#Requires -Version 7.6
param([string]$Metadata, [string]$Census, [string]$Payload, [long]$Columns, [long]$Rows=1)
. "$PSScriptRoot/Hexagon.Common.ps1"
$script:NativeCommit = '73bfe6cc485554822f80c473a3c0c4f25e80f41b'
function Assert-Pq2Extent([byte[]]$Data, [long]$Columns, [long]$Rows) {
    Assert-Hexagon ($Columns -gt 0 -and $Columns % 128 -eq 0 -and $Rows -gt 0) 'positive rows and columns divisible by 128 required'
    Assert-Hexagon ($Data.Length -le 1048576) 'payload exceeds bounded audit size'
    Assert-Hexagon ($Data.Length -eq ([bigint]$Columns / 128 * $Rows * 34)) 'payload extent does not match complete PQ2 rows'
}
function Convert-Pq2ToQ2([byte[]]$Data, [long]$Columns, [long]$Rows=1) {
    Assert-Pq2Extent $Data $Columns $Rows
    $result = [byte[]]::new($Data.Length / 34 * 36)
    for ($offset=0; $offset -lt $Data.Length; $offset+=34) {
        $null = Get-HexagonHalf ([BitConverter]::ToUInt16($Data,$offset))
        $target = $offset / 34 * 36
        [Array]::Copy($Data,$offset,$result,$target,18)
        [Array]::Copy($Data,$offset,$result,$target+18,2)
        [Array]::Copy($Data,$offset+18,$result,$target+20,16)
    }
    return ,$result
}
function Convert-Q2ToPq2([byte[]]$Data, [long]$Columns, [long]$Rows=1) {
    Assert-Hexagon ($Columns -gt 0 -and $Columns % 128 -eq 0 -and $Rows -gt 0) 'invalid paired Q2_0 dimensions'
    Assert-Hexagon ($Data.Length -le 1110240 -and $Data.Length -eq ([bigint]$Columns / 128 * $Rows * 36)) 'payload extent does not match paired Q2_0 rows'
    $result = [byte[]]::new($Data.Length / 36 * 34)
    for ($offset=0; $offset -lt $Data.Length; $offset+=36) {
        $bits = [BitConverter]::ToUInt16($Data,$offset); $null = Get-HexagonHalf $bits
        Assert-Hexagon ($bits -eq [BitConverter]::ToUInt16($Data,$offset+18)) 'paired raw scale bits differ'
        $target = $offset / 36 * 34
        [Array]::Copy($Data,$offset,$result,$target,18)
        [Array]::Copy($Data,$offset+20,$result,$target+18,16)
    }
    return ,$result
}
function Get-PackedF32Bits([byte[]]$Data, [int]$BlockBytes) {
    Assert-Hexagon ($Data.Length % $BlockBytes -eq 0) 'incomplete packed block'
    $result = [byte[]]::new($Data.Length / $BlockBytes * ($BlockBytes-2) * 16)
    $cursor=0
    for ($offset=0; $offset -lt $Data.Length; $offset+=$BlockBytes) {
        $scale = Get-HexagonHalf ([BitConverter]::ToUInt16($Data,$offset))
        for ($i=2; $i -lt $BlockBytes; $i++) {
            foreach ($shift in @(0,2,4,6)) {
                $value = [float](((($Data[$offset+$i] -shr $shift) -band 3)-1)*$scale)
                [Array]::Copy([BitConverter]::GetBytes($value),0,$result,$cursor,4); $cursor+=4
            }
        }
    }
    return ,$result
}
function Test-Pq2Payload([byte[]]$Data,[long]$Columns,[long]$Rows=1) {
    $bridged = Convert-Pq2ToQ2 $Data $Columns $Rows
    Assert-Hexagon ([Convert]::ToHexString((Convert-Q2ToPq2 $bridged $Columns $Rows)) -ceq [Convert]::ToHexString($Data)) 'raw layout roundtrip failed'
    Assert-Hexagon ([Convert]::ToHexString((Get-PackedF32Bits $Data 34)) -ceq [Convert]::ToHexString((Get-PackedF32Bits $bridged 18))) 'decoded f32 bits differ'
    $codes=@(0,0,0,0)
    for ($offset=0;$offset -lt $Data.Length;$offset+=34) { for ($i=2;$i -lt 34;$i++) { foreach ($shift in @(0,2,4,6)) { $codes[($Data[$offset+$i] -shr $shift) -band 3]++ } } }
    return @{Columns=$Columns;Rows=$Rows;SourceBytes=$Data.Length;BridgedBytes=$bridged.Length;SourceSha256=(Get-HexagonHash $Data);BridgedSha256=(Get-HexagonHash $bridged);RawCodeCounts=$codes;RawRoundtrip='exact';DecodedF32Bits='exact'}
}
function Get-Pq2StoragePlan($Metadata,$Census) {
    Assert-Hexagon ($Metadata -is [Collections.IDictionary] -and $Census -is [Collections.IDictionary]) 'inventory objects required'
    Assert-Hexagon ((Get-HexagonInteger $Metadata SchemaVersion) -eq 1 -and (Get-HexagonInteger $Census SchemaVersion) -eq 1) 'unsupported inventory schema'
    Assert-Hexagon ($Metadata.Format -ceq 'GGUF' -and $Metadata.ByteOrder -ceq 'little') 'expected little-endian GGUF metadata'
    Assert-Hexagon ($Metadata.ArtifactSha256 -cmatch '^[0-9a-f]{64}$') 'invalid artifact digest'
    foreach ($record in @($Metadata,$Census)) { foreach ($name in @('ModelRepository','ModelRevision')) { Assert-Hexagon ($record[$name] -is [string] -and $record[$name].Length -in 1..512 -and $record[$name] -notmatch '[\x00-\x1f\x7f]') ('invalid string identity: '+$name) } }
    foreach ($name in @('ArtifactSha256','ModelRepository','ModelRevision')) { Assert-Hexagon ($Metadata[$name] -ceq $Census[$name]) ('inventory identity mismatch: '+$name) }
    Assert-Hexagon ($Census.CodecCommit -ceq $script:NativeCommit -and $Metadata.Dialect -ceq ('Prism-'+$script:NativeCommit)) 'inventory codec does not match the pinned native source'
    $fileBytes=Get-HexagonInteger $Metadata FileBytes; $blocks=Get-HexagonInteger $Census Blocks
    Assert-Hexagon ($fileBytes -eq (Get-HexagonInteger $Census ArtifactBytes)) 'artifact byte mismatch'
    $elements=Get-HexagonInteger $Census Elements; $sourceBytes=Get-HexagonInteger $Census SerializedBytesRead
    Assert-Hexagon ($blocks -gt 0 -and $elements -eq $blocks*128 -and $sourceBytes -eq $blocks*34) 'PQ2 inventory geometry mismatch'
    $counts=@(foreach ($name in @('CodeMinusOne','CodeZero','CodePlusOne','CodePlusTwo')) { Get-HexagonInteger $Census $name })
    Assert-Hexagon (($counts | Measure-Object -Sum).Sum -eq $elements) 'raw code census mismatch'
    Assert-Hexagon ((Get-HexagonInteger $Census NonfiniteScaleBlocks) -eq 0 -and (Get-HexagonInteger $Census ZeroScaleBlocks) -le $blocks) 'invalid scale inventory'
    Assert-Hexagon ($Metadata.TensorTypeHistogram -is [Array]) 'missing or malformed tensor histogram'
    foreach ($entry in $Metadata.TensorTypeHistogram) { $null=Get-HexagonInteger $entry TypeId }
    $matches=@($Metadata.TensorTypeHistogram | Where-Object TypeId -EQ 142)
    Assert-Hexagon ($matches.Count -eq 1) 'expected one PQ2 tensor histogram entry'
    $pq2=$matches[0]; $tensorCount=Get-HexagonInteger $Census TensorCount
    Assert-Hexagon ($tensorCount -gt 0 -and $tensorCount -le $blocks -and $pq2.Name -ceq 'PQ2_0' -and (Get-HexagonInteger $pq2 Bytes) -eq $sourceBytes -and (Get-HexagonInteger $pq2 Elements) -eq $elements -and (Get-HexagonInteger $pq2 Count) -eq $tensorCount) 'tensor histogram and census disagree'
    $transform=@{'prism.hadamard.version'=1;'prism.hadamard.block_size'=1024;'prism.hadamard.transform'='normalized-sylvester-walsh-hadamard';'prism.hadamard.axis'='input-last-dimension';'prism.hadamard.sign_mode'='explicit';'prism.hadamard.gdn_v_grouped'=$true}
    foreach ($key in $transform.Keys) { Assert-Hexagon (Test-HexagonEqual $Metadata.Scalars[$key] $transform[$key]) 'unsupported or missing Swift transform identity' }
    Assert-Hexagon ((Get-HexagonInteger $Metadata.Arrays['prism.hadamard.sign_values'] Count) -gt 0) 'missing explicit sign inventory'
    $bridgedBytes=$blocks*36; $estimated=$fileBytes+$bridgedBytes-$sourceBytes
    Assert-Hexagon ($fileBytes -ge $sourceBytes -and $blocks*2 -le [ulong]::MaxValue -and $bridgedBytes -le [ulong]::MaxValue -and $estimated -le [ulong]::MaxValue) 'bridged storage exceeds u64 extent'
    return @{ArtifactSha256=$Metadata.ArtifactSha256;InventoryOnly=$true;Pq2Blocks=[ulong]$blocks;Q2Blocks=[ulong]($blocks*2);Elements=[ulong]$elements;SourcePackedBytes=[ulong]$sourceBytes;BridgedPackedBytes=[ulong]$bridgedBytes;AdditionalPackedBytes=[ulong]($bridgedBytes-$sourceBytes);PackedExpansionNumerator=18;PackedExpansionDenominator=17;ArtifactBytesBeforeAlignmentChanges=[ulong]$estimated;RawCodeCounts=@($counts | ForEach-Object {[ulong]$_});FourthSymbolSupported=$true;TransformContract=$transform;SignIdentityCoverage='inventory summaries only; actual sign bytes remain to be bound'}
}
if ($MyInvocation.InvocationName -ne '.') {
    if (-not $Metadata) { $Metadata=Join-Path $script:HexagonRoot 'config/swift.metadata.json' }; if (-not $Census) { $Census=Join-Path $script:HexagonRoot 'config/swift.pq2-census.json' }
    Assert-Hexagon (([bool]$Payload -eq ($Columns -ne 0)) -and ($Rows -eq 1 -or $Payload)) 'payload and columns must be supplied together; rows requires payload'
    $report=@{SchemaVersion=1;Qualification='weight-layout-audit-only';SourceCommit=$script:NativeCommit;Sources=@("https://github.com/PrismML-Eng/llama.cpp/blob/$script:NativeCommit/ggml/src/ggml-common.h","https://github.com/PrismML-Eng/llama.cpp/blob/$script:NativeCommit/ggml/src/ggml-quants.c");MetadataSha256=(Get-HexagonHash (Read-HexagonBytes $Metadata));CensusSha256=(Get-HexagonHash (Read-HexagonBytes $Census));StoragePlan=(Get-Pq2StoragePlan (Read-HexagonJson $Metadata) (Read-HexagonJson $Census));Limitations=@('Weight layout only; no NPU dispatch, signed deployment or speed qualification.','Q2_0 Q8_0 activation arithmetic does not establish PQ2 Q8_K/odd-group parity.','No GGUF rewrite; transform/sign arrays, tensor names and grouped basis stay unchanged.','Model payload, logits, complete state, adapters and lifecycle remain unqualified.','Storage totals exclude alignment changes, simultaneous source copies and device scratch.')}
    if ($Payload) { $report.Fixture=Test-Pq2Payload (Read-HexagonBytes $Payload) $Columns $Rows }; $report | ConvertTo-Json -Depth 64
}
