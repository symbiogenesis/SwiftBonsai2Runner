#Requires -Version 7.6
# Offline independent scalar replay. Opens no model and executes no producer source.
[CmdletBinding()]
param([Alias('evidence-directory')][string]$EvidenceDirectory='', [string]$Adapter='', [string]$Direction='',
    [Alias('expected-source-revision')][string]$ExpectedSourceRevision='',
    [Alias('expected-run-id')][string]$ExpectedRunId='',
    [Alias('expected-run-attempt')][string]$ExpectedRunAttempt='')
. (Join-Path $PSScriptRoot 'public_pack_formats.ps1')

function Read-AuditVector([byte[]]$Raw,[int]$Count) {
    Assert-Reference ($Raw.Length -eq $Count*4) 'retained F32 vector extent differs'
    $values=[single[]]::new($Count);[Buffer]::BlockCopy($Raw,0,$values,0,$Raw.Length)
    foreach($value in $values){Assert-Reference ([single]::IsFinite($value)) 'nonfinite retained F32 vector'}
    return ,$values
}
function Get-AuditMetrics([single[]]$Actual,[single[]]$Reference,[double]$Absolute=0.00001,[double]$Relative=0.00001) {
    Assert-Reference ($Actual.Length -eq $Reference.Length) 'comparison vector lengths differ'
    $failed=0;$maximum=0.0
    for($i=0;$i -lt $Reference.Length;$i++) {
        Assert-Reference ([single]::IsFinite($Actual[$i]) -and [single]::IsFinite($Reference[$i])) 'nonfinite comparison value'
        $difference=[Math]::Abs([double]$Actual[$i]-[double]$Reference[$i]);$maximum=[Math]::Max($maximum,$difference)
        if($difference -gt [Math]::FusedMultiplyAdd($Relative,[Math]::Abs([double]$Reference[$i]),$Absolute)){$failed++}
    }
    return @{comparedElements=$Reference.Length;mismatchedElements=$failed;maximumAbsoluteDifference=$maximum}
}
function Read-AuditText([IO.BinaryReader]$Reader) {
    $length=$Reader.ReadUInt64();Assert-Reference ($length -le 4096) 'retained adapter text exceeds bound'
    $bytes=$Reader.ReadBytes([int]$length);Assert-Reference ($bytes.Length -eq $length) 'truncated retained adapter text'
    return [Text.UTF8Encoding]::new($false,$true).GetString($bytes)
}
function Read-AuditMetadata([IO.BinaryReader]$Reader,[int]$Type,[int]$Depth=0) {
    Assert-Reference ($Depth -le 2) 'retained adapter nesting exceeds bound'
    switch($Type) {
        0{return $Reader.ReadByte()} 1{return $Reader.ReadSByte()} 2{return $Reader.ReadUInt16()} 3{return $Reader.ReadInt16()}
        4{return $Reader.ReadUInt32()} 5{return $Reader.ReadInt32()} 6{return $Reader.ReadSingle()}
        7{$value=$Reader.ReadByte();Assert-Reference ($value -in 0,1) 'invalid retained adapter Boolean';return [bool]$value}
        8{return Read-AuditText $Reader}
        9{$kind=$Reader.ReadUInt32();$count=$Reader.ReadUInt64();Assert-Reference ($count -le 65536) 'retained metadata count differs';$items=[Collections.Generic.List[object]]::new();for($i=0L;$i -lt $count;$i++){$items.Add((Read-AuditMetadata $Reader $kind ($Depth+1)))};return ,$items.ToArray()}
        10{return $Reader.ReadUInt64()} 11{return $Reader.ReadInt64()} 12{return $Reader.ReadDouble()}
        default{throw [IO.InvalidDataException]::new('unknown retained metadata type')}
    }
}
function Read-AuditAdapter([byte[]]$Raw) {
    # Deliberately independent of the producer's GGUF descriptor implementation.
    $stream=[IO.MemoryStream]::new($Raw,$false);$reader=[IO.BinaryReader]::new($stream)
    try {
        Assert-Reference ([Text.Encoding]::ASCII.GetString($reader.ReadBytes(4)) -ceq 'GGUF' -and $reader.ReadUInt32() -eq 3) 'retained adapter format differs'
        $count=$reader.ReadUInt64();$metadataCount=$reader.ReadUInt64()
        Assert-Reference ($count -eq 258 -and $metadataCount -le 128) 'retained adapter inventory differs'
        $metadata=[Collections.Generic.Dictionary[string,object]]::new([StringComparer]::Ordinal)
        for($i=0;$i -lt $metadataCount;$i++){$name=Read-AuditText $reader;Assert-Reference (-not $metadata.ContainsKey($name)) 'duplicate retained adapter metadata';$metadata.Add($name,(Read-AuditMetadata $reader ($reader.ReadUInt32())))}
        Assert-Reference ((-not $metadata.ContainsKey('general.alignment') -or $metadata['general.alignment'] -eq 32) -and $metadata['general.architecture'] -ceq 'qwen35' -and $metadata['general.type'] -ceq 'adapter' -and $metadata['adapter.type'] -ceq 'lora' -and $metadata['adapter.lora_alpha'] -eq 1.0) 'retained adapter metadata differs'
        $descriptors=[Collections.Generic.Dictionary[string,object]]::new([StringComparer]::Ordinal)
        for($i=0;$i -lt $count;$i++) {
            $name=Read-AuditText $reader;Assert-Reference (-not $descriptors.ContainsKey($name) -and $reader.ReadUInt32() -eq 2) 'retained adapter tensor name/rank differs'
            $shape=@($reader.ReadUInt64(),$reader.ReadUInt64());Assert-Reference ($shape[0] -gt 0 -and $shape[0] -le 248320 -and $shape[1] -gt 0 -and $shape[1] -le 248320 -and $reader.ReadUInt32() -eq 0) 'retained adapter tensor shape/type differs'
            $offset=$reader.ReadUInt64();Assert-Reference ($offset % 32 -eq 0) 'retained adapter tensor alignment differs'
            $descriptors.Add($name,@{shape=$shape;offset=$offset})
        }
        $expected=@{'token_embd.weight.lora_a'=@(1,248320);'token_embd.weight.lora_b'=@(1,5120)}
        for($block=0;$block -lt 64;$block++){
            $family=if(($block+1)%4 -eq 0){'attn_output'}else{'ssm_out'}
            foreach($site in @(@("blk.$block.ffn_down.weight",17408),@("blk.$block.$family.weight",6144))){$expected[$site[0]+'.lora_a']=@($site[1],1);$expected[$site[0]+'.lora_b']=@(1,5120)}
        }
        Assert-Reference ($descriptors.Count -eq $expected.Count) 'retained adapter writer set differs'
        $base=([long][Math]::Ceiling($stream.Position/32.0))*32;$factors=[Collections.Generic.Dictionary[string,object]]::new([StringComparer]::Ordinal);$ranges=[Collections.Generic.List[object]]::new()
        foreach($name in $descriptors.Keys){
            Assert-Reference ($expected.ContainsKey($name) -and (Test-ReferenceEqual $descriptors[$name].shape $expected[$name])) 'retained adapter writer shape differs'
            $shape=$expected[$name];$begin=$base+$descriptors[$name].offset;$end=$begin+4L*$shape[0]*$shape[1]
            Assert-Reference ($end -le $Raw.Length) 'retained adapter writer extent differs'
            $bytes=[byte[]]::new([int]($end-$begin));[Array]::Copy($Raw,[long]$begin,$bytes,0L,[long]$bytes.Length)
            [void](Read-AuditVector $bytes ($bytes.Length/4));$factors.Add($name,$bytes);$ranges.Add(@{begin=$begin;end=$end})
        }
        $cursor=$base;foreach($range in @($ranges|Sort-Object begin)){Assert-Reference ($range.begin -ge $cursor) 'retained adapter writers overlap';$cursor=$range.end}
        return ,$factors
    }finally{$reader.Dispose();$stream.Dispose()}
}
function Get-AuditDescriptors([string]$Evidence,$Report,$Pins) {
    [void](ConvertFrom-ReferenceJson (Read-ReferenceBounded (Join-Path $Evidence 'config.json') $Pins.config.bytes $Pins.config.bytes $Pins.config.sha256))
    Assert-Reference ($Report.modelHeaderFile -ceq 'model-header.json') 'retained header filename differs'
    $raw=Read-ReferenceBounded (Join-Path $Evidence 'model-header.json') 4194304 $Report.modelHeaderBytes $Report.modelHeaderSha256
    $header=ConvertFrom-ReferenceJson $raw
    if($header.Contains('__metadata__')){$header.Remove('__metadata__')}
    Assert-Reference ($header.Count -eq $Report.modelTensorCount -and $header.Count -gt 0 -and $header.Count -le 8192) 'retained model tensor inventory differs'
    $widths=@{BOOL=1;U8=1;I8=1;U16=2;I16=2;F16=2;BF16=2;U32=4;I32=4;F32=4;U64=8;I64=8;F64=8};$descriptors=@{};$ranges=[Collections.Generic.List[object]]::new();$base=$raw.Length+8L
    foreach($name in $header.Keys){
        $item=$header[$name];Assert-ReferenceKeys $item @('dtype','shape','data_offsets')
        Assert-Reference ($item.shape -is [array] -and $item.shape.Count -le 8 -and $item.data_offsets -is [array] -and $item.data_offsets.Count -eq 2 -and $widths.ContainsKey($item.dtype)) 'retained model descriptor types differ'
        $product=[Numerics.BigInteger]::One;foreach($dimension in $item.shape){Assert-Reference (Test-ReferenceInteger $dimension 1000000000) 'retained model shape differs';$product*=$dimension}
        $begin,$end=$item.data_offsets
        Assert-Reference ((Test-ReferenceInteger $begin $Pins.model.bytes) -and (Test-ReferenceInteger $end $Pins.model.bytes) -and $begin -le $end -and $end-$begin -eq $product*$widths[$item.dtype] -and $base+$end -le $Pins.model.bytes) 'retained model descriptor extent differs'
        $descriptors[$name]=@{dtype=$item.dtype;shape=$item.shape;absoluteByteBegin=$base+$begin;absoluteByteEnd=$base+$end}
        if($begin -ne $end){$ranges.Add(@{begin=$begin;end=$end})}
    }
    $cursor=0L;foreach($range in @($ranges|Sort-Object begin)){Assert-Reference ($range.begin -eq $cursor) 'retained model payload hole or overlap';$cursor=$range.end}
    Assert-Reference ($base+$cursor -eq $Pins.model.bytes) 'retained model payload tail differs'
    return $descriptors
}
function Read-AuditReplaySource([string]$Evidence,$Report,[string]$Revision) {
    $haveFile=$Report.Contains('replaySourceManifestFile');$haveHash=$Report.Contains('replaySourceManifestSha256')
    Assert-Reference ($haveFile -eq $haveHash) 'incomplete retained source manifest binding'
    $bundle=Join-Path $Evidence 'replay-source'
    if(-not $haveFile){Assert-Reference (-not [IO.Directory]::Exists($bundle)) 'retained source binding removed';return $null}
    Assert-Reference ($Report.replaySourceManifestFile -ceq 'replay-source/source-manifest.json' -and $Report.replaySourceManifestSha256 -cmatch '^[0-9a-f]{64}$') 'retained source manifest selection differs'
    $expected=[Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    foreach($name in $script:ReplaySourceFiles){[void]$expected.Add($name)};[void]$expected.Add('source-manifest.json')
    $actual=[Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    $root=Get-Item -LiteralPath $bundle;Assert-Reference ($root.PSIsContainer -and -not ($root.Attributes -band [IO.FileAttributes]::ReparsePoint)) 'retained source directory redirected'
    foreach($entry in Get-ChildItem -LiteralPath $bundle -Force -Recurse){
        Assert-Reference (-not ($entry.Attributes -band [IO.FileAttributes]::ReparsePoint)) 'retained source entry redirected'
        $relative=[IO.Path]::GetRelativePath($bundle,$entry.FullName).Replace('\','/')
        if($entry.PSIsContainer){Assert-Reference ($relative -cin 'config','tools','tools/native-reference') 'unexpected retained source directory'}else{Assert-Reference ($expected.Contains($relative) -and $actual.Add($relative)) 'unexpected retained source entry'}
    }
    Assert-Reference ($actual.SetEquals($expected)) 'retained source inventory incomplete'
    $manifest=ConvertFrom-ReferenceJson (Read-ReferenceBounded (Join-Path $bundle 'source-manifest.json') 65536 -Sha256 $Report.replaySourceManifestSha256)
    Assert-ReferenceKeys $manifest @('schemaVersion','kind','sourceRevision','files')
    Assert-Reference ($manifest.schemaVersion -eq 1 -and $manifest.kind -ceq 'public-reference-replay-source' -and $manifest.sourceRevision -ceq $Revision -and $manifest.files -is [array] -and $manifest.files.Count -eq $script:ReplaySourceFiles.Count) 'retained source manifest identity differs'
    $sources=[ordered]@{}
    foreach($item in $manifest.files){
        Assert-ReferenceKeys $item @('path','bytes','sha256')
        Assert-Reference ($item.path -cin $script:ReplaySourceFiles -and -not $sources.Contains($item.path) -and (Test-ReferenceInteger $item.bytes 1048576) -and $item.bytes -gt 0 -and $item.sha256 -cmatch '^[0-9a-f]{64}$') 'retained source declaration differs'
        $sources[$item.path]=Read-ReferenceBounded (Join-Path $bundle $item.path) 1048576 $item.bytes $item.sha256
    }
    return $sources
}
function Invoke-PublicReferenceReplay([string]$Evidence,[string]$Adapter,[string]$Direction,[string]$Revision,[string]$RunId,[string]$RunAttempt,[string]$RepositoryRoot='') {
    Assert-Reference ($Revision -cmatch '^[0-9a-f]{40}$' -and $RunId -cmatch '^[1-9][0-9]*$' -and $RunAttempt -cmatch '^[1-9][0-9]*$') 'independent producer selection invalid'
    $report=Read-ReferenceJson (Join-Path $Evidence 'report.json');$sources=Read-AuditReplaySource $Evidence $report $Revision
    $repository=if($RepositoryRoot){[IO.Path]::GetFullPath($RepositoryRoot)}else{[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))}
    function Get-ReplaySource([string]$Relative){if($null -ne $sources){return ,$sources[$Relative]};return ,(Read-ReferenceBounded (Join-Path $repository $Relative) 1048576)}
    $pinsRaw=Get-ReplaySource 'tools/native-reference/public-authoring-pins.json';$pins=ConvertFrom-ReferenceJson $pinsRaw
    Assert-Reference (Test-ReferenceEqual $pins (Read-ReferenceJson (Join-Path $repository 'tools/native-reference/public-authoring-pins.json') 65536)) 'reviewer pins differ from independently selected recipe'
    foreach($pair in @(@('swift-lora.lock.json','adapter'),@('swift-lora.direction.json','direction'))){
        $locked=(ConvertFrom-ReferenceJson (Get-ReplaySource ('config/'+$pair[0])))[$pair[1]];$approved=(Read-ReferenceJson (Join-Path $repository ('config/'+$pair[0])))[$pair[1]]
        foreach($key in 'filename','bytes','sha256','url'){Assert-Reference ($pins[$pair[1]][$key] -ceq $locked[$key] -and $locked[$key] -ceq $approved[$key]) 'reviewer pins differ from approved repository lock'}
    }
    Assert-Reference (Test-ReferenceEqual $report (Read-ReferenceJson (Join-Path $Evidence 'attempt.json'))) 'terminal attempt differs from report'
    Assert-Reference ($report.schemaVersion -eq 2 -and $report.kind -ceq 'independent-public-pack-A-reference' -and $report.arithmeticId -ceq $script:ReferenceArithmetic -and $report.stage -ceq 'complete' -and $report.referencePinsSha256 -ceq (Get-ReferenceDigest $pinsRaw) -and (Test-ReferenceEqual $report.inputPins $pins)) 'retained recipe/input identity differs'
    Assert-Reference ($report.repository -ceq 'symbiogenesis/SwiftBonsai2Runner' -and $report.workflowPath -ceq '.github/workflows/ptq1-public-authoring-reference.yml' -and $report.sourceRevision -ceq $Revision -and $report.runId -ceq $RunId -and $report.runAttempt -ceq $RunAttempt -and $report.runnerEnvironment -ceq 'github-hosted') 'retained producer differs from independent selection'
    foreach($flag in 'authorOriginalProvenanceQualified','nativeWriterBasisQualified','logitsOrStateQualified','defaultMayChange','fullCheckpointWritten'){Assert-Reference ($report[$flag] -is [bool] -and -not $report[$flag]) 'retained result overstates qualification'}
    Assert-Reference ($report.ownerDeviceTrials -eq 0 -and $report.modelInferenceCommands -eq 0) 'retained execution scope differs'
    $toolchain=Read-ReferenceJson (Join-Path $Evidence 'toolchain.json')
    foreach($pair in @(@('referenceScriptSha256','public_pack_reference.ps1'),@('formatScriptSha256','public_pack_formats.ps1'),@('numericSourceSha256','reference_numerics.rs'))){Assert-Reference ($toolchain[$pair[0]] -ceq (Get-ReferenceDigest (Get-ReplaySource ('tools/native-reference/'+$pair[1])))) 'retained producer source differs from reviewer source'}
    Assert-Reference ([version]$toolchain.powershell -ge [version]'7.6' -and $toolchain.threads -eq 1 -and $toolchain.reduction -ceq $pins.reduction -and $toolchain.normalization -ceq $pins.normalization -and $toolchain.numericExecutableSha256 -cmatch '^[0-9a-f]{64}$' -and $toolchain.rust.StartsWith('rustc ')) 'retained producer toolchain differs'
    $descriptors=Get-AuditDescriptors $Evidence $report $pins
    $factors=Read-AuditAdapter (Read-ReferenceBounded $Adapter $pins.adapter.bytes $pins.adapter.bytes $pins.adapter.sha256)
    $rawDirection=Read-ReferenceBounded $Direction 20480 20480 $pins.direction.sha256;$directionValues=Read-AuditVector $rawDirection 5120
    $sum=0.0;foreach($value in $directionValues){$sum += [double]$value*[double]$value};$expectedNorm=[single][Math]::Sqrt($sum);$norm=[single]$report.directionF32NormBeforeNormalization
    Assert-Reference ([single]::IsFinite($norm) -and $norm -gt 0 -and [Math]::Abs([double]$norm-1) -le 0.00001 -and [BitConverter]::SingleToInt32Bits($norm) -eq [BitConverter]::SingleToInt32Bits($expectedNorm)) 'retained direction norm inconsistent'
    $normalized=[single[]]::new(5120);$negative=[single[]]::new(5120)
    for($i=0;$i -lt 5120;$i++){$normalized[$i]=[single]($directionValues[$i]/$norm);$negative[$i]=-$normalized[$i]}
    $normalizedRaw=[byte[]]::new(20480);[Buffer]::BlockCopy($normalized,0,$normalizedRaw,0,20480)
    Assert-Reference ((Get-ReferenceDigest $normalizedRaw) -ceq $report.normalizedDirectionSha256) 'retained normalized direction digest differs'
    $bReport=(Read-ReferenceJson (Join-Path $Evidence 'direction-comparison.json')).sites
    $expectedB=[Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal);foreach($name in $factors.Keys){if($name.EndsWith('.lora_b',[StringComparison]::Ordinal)){[void]$expectedB.Add($name)}}
    $seenB=[Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    Assert-Reference ($bReport.Count -eq 129) 'retained B comparison inventory differs'
    foreach($item in $bReport){
        Assert-Reference ($expectedB.Contains($item.tensor) -and $seenB.Add($item.tensor)) 'retained B comparison inventory differs'
        $raw=$factors[$item.tensor];$metrics=Get-AuditMetrics (Read-AuditVector $raw 5120) $negative 0.000001 0.000001
        Assert-Reference ($item.sha256 -ceq (Get-ReferenceDigest $raw) -and $metrics.mismatchedElements -eq 0) 'retained B comparison replay differs'
        foreach($key in $metrics.Keys){Assert-Reference ($item[$key] -eq $metrics[$key]) 'retained B comparison metrics differ'}
    }
    Assert-Reference ($report.all129BComparisonsPassed -is [bool] -and $report.all129BComparisonsPassed) 'retained B verdict differs'
    $sites=$report.sites;Assert-Reference ($sites.Count -eq 48) 'retained A comparison inventory differs'
    $expectedFiles=@($script:ReferenceBlocks|ForEach-Object{'ssm-{0:D2}.f32le' -f $_}|Sort-Object);$actualFiles=@(Get-ChildItem -LiteralPath $Evidence -File -Filter 'ssm-*.f32le'|ForEach-Object{$_.Name}|Sort-Object)
    Assert-Reference (Test-ReferenceEqual $actualFiles $expectedFiles) 'retained A file inventory differs'
    $results=[Collections.Generic.List[object]]::new()
    for($i=0;$i -lt 48;$i++){
        $item=$sites[$i];$block=$script:ReferenceBlocks[$i];$name='ssm-{0:D2}.f32le' -f $block;$prefix="language_model.model.layers.$block.linear_attn.out_proj"
        Assert-Reference ($item.block -eq $block -and $item.publicTensorPrefix -ceq $prefix) 'retained A site order differs'
        Assert-ReferenceKeys $item.inputTensorDescriptors @('weight','scales','biases','signs');Assert-ReferenceKeys $item.inputTensorSha256 @('weight','scales','biases','signs')
        foreach($suffix in 'weight','scales','biases','signs'){
            $descriptor=$descriptors[$prefix+'.'+$suffix];[object[]]$shape=switch($suffix){weight{@(5120,384)}scales{@(5120,48)}biases{@(5120,48)}signs{@(6144)}};$types=switch($suffix){weight{@('U32')}signs{@('F16','F32')}default{@('F16')}}
            Assert-Reference ((Test-ReferenceEqual $descriptor $item.inputTensorDescriptors[$suffix]) -and (Test-ReferenceEqual $descriptor.shape $shape) -and $descriptor.dtype -cin $types -and $item.inputTensorSha256[$suffix] -cmatch '^[0-9a-f]{64}$') 'retained source tensor descriptor differs'
        }
        Assert-Reference ($item.candidateF16MatrixSha256 -cmatch '^[0-9a-f]{64}$' -and $item.authoringAFile -ceq $name -and $item.authoringABytes -eq 24576) 'retained candidate matrix/A extent differs'
        $reference=Read-ReferenceBounded (Join-Path $Evidence $name) 24576 24576 $item.authoringASha256;$shipped=$factors["blk.$block.ssm_out.weight.lora_a"]
        $metrics=Get-AuditMetrics (Read-AuditVector $shipped 6144) (Read-AuditVector $reference 6144)
        Assert-Reference ($item.shippedASha256 -ceq (Get-ReferenceDigest $shipped)) 'retained shipped A digest differs'
        foreach($key in $metrics.Keys){Assert-Reference ($item[$key] -eq $metrics[$key]) 'retained A comparison replay differs'}
        $metrics.block=$block;$metrics.authoringASha256=Get-ReferenceDigest $reference;$results.Add($metrics)
    }
    $passed=@($results|Where-Object{$_.mismatchedElements -ne 0}).Count -eq 0;$status=if($passed){'complete-public-reference-match'}else{'rejected-public-A-comparison'}
    Assert-Reference ($report.all48PublicAComparisonPassed -is [bool] -and $report.all48PublicAComparisonPassed -eq $passed -and $report.status -ceq $status) 'retained A verdict differs'
    return @{schemaVersion=2;kind='offline-retained-public-reference-replay';status='retained-result-verified';sourceRevision=$Revision;runId=$RunId;runAttempt=$RunAttempt;reportSha256=(Get-ReferenceFileDigest (Join-Path $Evidence 'report.json'));referencePinsSha256=(Get-ReferenceDigest $pinsRaw);replaySourceManifestSha256=$(if($report.Contains('replaySourceManifestSha256')){$report.replaySourceManifestSha256}else{$null});retainedReplaySourceVerified=($null -ne $sources);reviewScriptSha256=(Get-ReferenceFileDigest $PSCommandPath);adapterSha256=$pins.adapter.sha256;directionSha256=$pins.direction.sha256;all48PublicAComparisonPassed=$passed;all129BComparisonsPassed=$true;sites=$results.ToArray();producerAuthenticated=$false;matrixReconstructionReexecuted=$false;authorOriginalProvenanceQualified=$false;nativeWriterBasisQualified=$false;defaultMayChange=$false;modelInferenceCommands=0;scope='Independent retained-file and scalar A/B metric replay only. Authenticate the producer and artifact separately; no matrix reconstruction, provenance, writer-basis, logits/state, quality or promotion qualification.'}
}
if($MyInvocation.InvocationName -ne '.'){
    try{Invoke-PublicReferenceReplay $EvidenceDirectory $Adapter $Direction $ExpectedSourceRevision $ExpectedRunId $ExpectedRunAttempt | ConvertTo-Json -Depth 64}
    catch{[Console]::Error.WriteLine('Retained public reference rejected: '+$_.Exception.GetType().Name);exit 1}
}
