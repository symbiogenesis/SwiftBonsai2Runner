#Requires -Version 7.6
[CmdletBinding()]
param([string]$Native='', [string]$Rust='', [string]$Backend='',
    [Alias('row-workers')][int]$RowWorkers=1,
    [Alias('adapter-condition')][string]$AdapterCondition='standard',
    [string]$Output='', [string]$ToolPath='')
. (Join-Path $PSScriptRoot 'public_pack_formats.ps1')
$script:NativeModel='e658f0f0736a03a4549535a439f9b07254ebf8ab88ae27cbe6e52d098ca5d1f5'
$script:NativeRevision='73bfe6cc485554822f80c473a3c0c4f25e80f41b'
$script:NativeAdapter='f1669534803d340a496015f5c45125f3437b4d13ec764f40e34488ce83967f42'

function Get-NativeReferenceComponents([string]$Path,[int]$Position,
    [int[]]$Attention=@(3..63|Where-Object{$_%4 -eq 3}),
    [int[]]$Recurrent=@(0..63|Where-Object{$_%4 -ne 3}),
    [int]$Layers=64,[int]$Width=1024,[int]$Convolution=30720,[int]$Matrix=786432) {
    Assert-Reference ($Position -in 0,1) 'decoder position outside bounded extent'
    $stream=[IO.File]::OpenRead($Path);$reader=[IO.BinaryReader]::new($stream)
    $components=[Collections.Generic.List[object]]::new()
    try {
        Assert-Reference ($stream.Length -gt 0 -and $stream.Length -le 536870912) 'native state exceeds byte bound'
        $count=$Position+1
        Assert-Reference ($reader.ReadUInt32() -eq [Convert]::ToUInt32('AF143CD8',16) -and $reader.ReadInt32() -eq 0 -and $reader.ReadUInt32() -eq 1 -and $reader.ReadUInt32() -eq $count) 'native sequence metadata changed'
        for($token=0;$token -lt $count;$token++) {
            Assert-Reference ($reader.ReadInt32() -eq $token -and $reader.ReadUInt32() -eq 1 -and $reader.ReadInt32() -eq $token -and $reader.ReadInt32() -eq $token -and $reader.ReadInt32() -eq 0) 'native attention position/sequence changed'
        }
        $transposed=$reader.ReadUInt32()
        Assert-Reference ($transposed -in 0,1 -and $reader.ReadUInt32() -eq $Attention.Count) 'native V layout/layer count changed'
        foreach($block in $Attention) {
            Assert-Reference ($reader.ReadInt32() -eq 1 -and $reader.ReadUInt64() -eq $Width*2L) 'native K row type/extent changed'
            for($token=0;$token -lt $count;$token++) {
                $components.Add(@{path="block-$block-key-$token.f32le";offset=$stream.Position;elements=$Width;code='f16';stride=2})
                $stream.Position+=$Width*2L
            }
        }
        foreach($block in $Attention) {
            Assert-Reference ($reader.ReadInt32() -eq 1) 'native V is not binary16'
            if($transposed) {
                Assert-Reference ($reader.ReadUInt32() -eq 2 -and $reader.ReadUInt32() -eq $Width) 'native transposed V geometry changed'
                for($token=0;$token -lt $count;$token++) { $components.Add(@{path="block-$block-value-$token.f32le";offset=$stream.Position+$token*2L;elements=$Width;code='f16';stride=$count*2}) }
                $stream.Position+=$Width*$count*2L
            } else {
                Assert-Reference ($reader.ReadUInt64() -eq $Width*2L) 'native V row size changed'
                for($token=0;$token -lt $count;$token++) { $components.Add(@{path="block-$block-value-$token.f32le";offset=$stream.Position;elements=$Width;code='f16';stride=2});$stream.Position+=$Width*2L }
            }
        }
        Assert-Reference ($reader.ReadUInt32() -eq 1 -and $reader.ReadInt32() -eq $Position -and $reader.ReadUInt32() -eq 0 -and $reader.ReadUInt32() -eq 0 -and $reader.ReadUInt32() -eq $Layers) 'native recurrent metadata/schema changed'
        foreach($component in @(@('convolution',$Convolution),@('recurrent',$Matrix))) {
            foreach($block in $Recurrent) {
                $elements=[int]$component[1]
                Assert-Reference ($reader.ReadInt32() -eq 0 -and $reader.ReadUInt64() -eq $elements*4L) 'native recurrent payload type/extent changed'
                $components.Add(@{path="block-$block-$($component[0]).f32le";offset=$stream.Position;elements=$elements;code='f32';stride=4})
                $stream.Position+=$elements*4L
            }
        }
        Assert-Reference ($stream.Position -eq $stream.Length) 'native sequence state truncated or has trailing bytes'
        return ,$components.ToArray()
    } finally { $reader.Dispose();$stream.Dispose() }
}
function Assert-NativeAdapterCondition($Capture,[string]$Condition) {
    Assert-Reference ($Condition -in 'standard','loaded-zero','no-adapter') 'unsupported adapter condition'
    $loaded=$Condition -cne 'no-adapter';$scale=if($Condition -ceq 'standard'){1.3}else{0.0}
    Assert-Reference ($Capture.adapterLoaded -is [bool] -and $Capture.adapterLoaded -eq $loaded) 'native adapter loading differs from selected condition'
    Assert-Reference (($Capture.adapterScale -is [double] -or $Capture.adapterScale -is [long] -or $Capture.adapterScale -is [int]) -and $Capture.adapterScale -eq $scale -and [BitConverter]::SingleToInt32Bits([single]$Capture.adapterScale) -eq [BitConverter]::SingleToInt32Bits([single]$scale)) 'native adapter scale differs from selected condition'
}
function Assert-NativeRustControls([string]$Rust,$Capture,[string]$Backend,[int]$RowWorkers) {
    Assert-Reference ($Backend -in 'auto','scalar','neon' -and $RowWorkers -in 1,4) 'unsupported Rust backend/row-worker policy'
    $records=@([Text.UTF8Encoding]::new($false,$true).GetString((Read-ReferenceBounded (Join-Path $Rust 'stdout.txt') 65536)).Split("`n") | Where-Object { $_.Trim().Length -gt 0 } | ForEach-Object { ConvertFrom-ReferenceJson ([Text.Encoding]::UTF8.GetBytes($_)) })
    $weights=@($records|Where-Object{$_.Contains('event') -and $_.event -ceq 'weights-loaded'})
    $steps=@($records|Where-Object{$_.Contains('event') -and $_.event -ceq 'forward-step'})
    $adapters=@($records|Where-Object{$_.Contains('event') -and $_.event -ceq 'rank-one-adapter-loaded'})
    Assert-Reference ($weights.Count -eq 1 -and $weights[0].model_sha256 -ceq $script:NativeModel -and $weights[0].weight_representation -ceq 'current-repacked-PTQ1_0') 'Rust capture used a different artifact'
    Assert-Reference ($steps.Count -eq 1 -and $steps[0].position -eq 0 -and $steps[0].input_token -eq 248045 -and $steps[0].backend_requested -ceq $Backend) 'Rust supplied tokens/backend differ'
    foreach($step in $steps) {
        Assert-Reference ((Test-ReferenceInteger $step.row_workers 4) -and $step.row_workers -eq $RowWorkers -and (Test-ReferenceInteger $step.parallel_row_operations ([long]::MaxValue)) -and (($RowWorkers -eq 4 -and $step.parallel_row_operations -gt 0) -or ($RowWorkers -eq 1 -and $step.parallel_row_operations -eq 0))) 'Rust row-worker execution differs'
    }
    Assert-Reference ($Capture.adapterLoaded -is [bool]) 'invalid native adapter control'
    if($Capture.adapterLoaded) {
        $scaleBytes=[BitConverter]::GetBytes([single]$Capture.adapterScale);[Array]::Reverse($scaleBytes)
        $bits=[Convert]::ToHexStringLower($scaleBytes)
        Assert-Reference ($adapters.Count -eq 1 -and $adapters[0].adapter_sha256 -ceq $script:NativeAdapter -and $adapters[0].target_pairs -eq 129 -and $adapters[0].scale_bits -ceq $bits -and $adapters[0].model_sha256 -ceq $script:NativeModel) 'Rust/native adapter identity/scale differs'
    } else { Assert-Reference ($adapters.Count -eq 0 -and $Capture.adapterScale -eq 0) 'native/Rust baseline adapter differs' }
    return ,$steps
}
function Invoke-NativeReferenceComparison([string]$Native,[string]$Rust,[string]$Backend,[int]$RowWorkers=1,[string]$AdapterCondition='standard',[string]$ToolPath='') {
    $tool=Get-ReferenceNumericTool $ToolPath
    $nativeInventory=@(Get-ChildItem -LiteralPath $Native -Recurse -File | Where-Object{$_.Extension -cin '.f32le','.native-seq'} | ForEach-Object{[IO.Path]::GetRelativePath($Native,$_.FullName).Replace('\','/')} | Sort-Object)
    Assert-Reference (Test-ReferenceEqual $nativeInventory @('logits-0.f32le','state-0.native-seq')) 'native payload inventory changed'
    $capture=Read-ReferenceJson (Join-Path $Native 'native-capture.json') 65536
    Assert-Reference ($capture.nativeCommit -ceq $script:NativeRevision -and $capture.modelSha256 -ceq $script:NativeModel -and $capture.contextTokens -eq 8192 -and $capture.threads -eq 4) 'native capture identity/controls changed'
    Assert-Reference ($capture.steps.Count -eq 1 -and $capture.steps[0].inputToken -eq 248045 -and $capture.steps[0].position -eq 0 -and $capture.steps[0].logitsCount -eq 248320) 'native trajectory/logits geometry changed'
    Assert-NativeAdapterCondition $capture $AdapterCondition
    $steps=Assert-NativeRustControls $Rust $capture $Backend $RowWorkers
    $files=[Collections.Generic.List[object]]::new();$expected=[Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    $logits=Join-Path $Native 'logits-0.f32le'
    Assert-Reference ((Get-Item -LiteralPath $logits).Length -eq 248320*4L) 'native logits extent changed'
    $actual=Join-Path $Rust 'logits-0.f32le'
    $result=Invoke-ReferenceNumeric $tool @('compare',$logits,$actual,'f32','0','248320','4','0.001','0.001','plain')
    $result.path='logits-0.f32le';$result.nativeSha256=Get-ReferenceFileDigest $logits;$result.rustSha256=Get-ReferenceFileDigest $actual
    $files.Add($result);[void]$expected.Add('logits-0.f32le')
    $state=Join-Path $Native 'state-0.native-seq'
    Assert-Reference ((Get-Item -LiteralPath $state).Length -eq $capture.steps[0].sequenceStateBytes) 'native serialized state receipt extent changed'
    foreach($component in (Get-NativeReferenceComponents $state 0)) {
        $relative='state-0/'+$component.path;$actual=Join-Path $Rust $relative
        $result=Invoke-ReferenceNumeric $tool @('compare',$state,$actual,$component.code,[string]$component.offset,[string]$component.elements,[string]$component.stride,'0.001','0.001','plain')
        $result.path=$relative;$result.rustSha256=Get-ReferenceFileDigest $actual
        $files.Add($result);[void]$expected.Add($relative)
    }
    $observed=[Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    foreach($file in Get-ChildItem -LiteralPath $Rust -Recurse -File -Filter '*.f32le') { [void]$observed.Add([IO.Path]::GetRelativePath($Rust,$file.FullName).Replace('\','/')) }
    Assert-Reference ($expected.SetEquals($observed)) 'Rust complete-state inventory differs from exact native extent'
    $failed=[long](($files|Measure-Object failedElements -Sum).Sum);$greedy=$steps[0].greedy_token -eq $capture.steps[0].greedyToken
    return [ordered]@{
        schemaVersion=1;status=$(if($failed -gt 0 -or -not $greedy){'failed'}else{'passed-bounded-one-token-smoke'})
        nativeCommit=$script:NativeRevision;modelSha256=$script:NativeModel;absoluteTolerance=0.001;relativeTolerance=0.001
        files=$files.Count;elements=[long](($files|Measure-Object elements -Sum).Sum);failedElements=$failed
        byteExactFiles=@($files|Where-Object{$_.byteExact}).Count;nativeCaptureSha256=(Get-ReferenceFileDigest (Join-Path $Native 'native-capture.json'))
        nativeStateSha256=@((Get-ReferenceFileDigest $state));maximumAbsoluteError=($files|Measure-Object maximumAbsoluteError -Maximum).Maximum
        comparisons=$files.ToArray();backendRequested=$Backend;rustRowWorkers=$RowWorkers;greedyTokensMatch=$greedy
        adapterCondition=$AdapterCondition;adapterLoaded=$capture.adapterLoaded;adapterScale=$capture.adapterScale
        adapterSha256=$(if($capture.adapterLoaded){$script:NativeAdapter}else{$null})
        parallelRowOperations=@($steps|ForEach-Object{$_.parallel_row_operations});rustStdoutSha256=(Get-ReferenceFileDigest (Join-Path $Rust 'stdout.txt'))
        writerBasisQualified=$false;qualityQualified=$false;defaultMayChange=$false;loadedZeroIdentityQualified=$false;nonzeroApplicationQualified=$false
        pendingControls=@('no-adapter','loaded-zero-identity','nonzero-change','scalar','explicit-neon','writer-authoring-basis','quality','performance','full-context')
    }
}
if($MyInvocation.InvocationName -ne '.') {
    Assert-Reference ($Native -and $Rust -and $Backend -and $Output -and -not [IO.File]::Exists($Output)) 'native comparison requires fresh output and explicit inputs'
    $report=Invoke-NativeReferenceComparison $Native $Rust $Backend $RowWorkers $AdapterCondition $ToolPath
    Write-ReferenceJson $Output $report
    if($report.status -ceq 'failed'){exit 1}
}
