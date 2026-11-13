#Requires -Version 7.6
[CmdletBinding()]
param([Parameter(ValueFromRemainingArguments)][string[]]$Name=@())
. (Join-Path $PSScriptRoot 'audit_public_reference.ps1')
. (Join-Path $PSScriptRoot 'public_pack_reference.ps1')
function Assert-Rejected([scriptblock]$Action){$rejected=$false;try{& $Action 2>$null|Out-Null}catch{$rejected=$true};Assert-Reference $rejected 'fixture expected rejection'}
function Write-FixtureText([IO.BinaryWriter]$Writer,[string]$Text){$bytes=[Text.Encoding]::UTF8.GetBytes($Text);$Writer.Write([uint64]$bytes.Length);$Writer.Write($bytes)}
function New-PublicReplayFixture([string]$Scratch) {
    $source=Join-Path $Scratch 'source';$evidence=Join-Path $Scratch 'evidence';[void][IO.Directory]::CreateDirectory($evidence)
    $direction=[byte[]]::new(20480);[Array]::Copy([BitConverter]::GetBytes([single]1),$direction,4);$directionPath=Join-Path $Scratch 'direction';[IO.File]::WriteAllBytes($directionPath,$direction)
    $negative=[byte[]]::new(20480);for($i=0;$i -lt 5120;$i++){$negative[$i*4+3]=0x80};[Array]::Copy([BitConverter]::GetBytes([single]-1),$negative,4)
    $factors=[ordered]@{'token_embd.weight.lora_a'=[byte[]]::new(248320*4);'token_embd.weight.lora_b'=$negative}
    for($block=0;$block -lt 64;$block++){$family=if(($block+1)%4 -eq 0){'attn_output'}else{'ssm_out'};foreach($site in @(@("blk.$block.ffn_down.weight",17408),@("blk.$block.$family.weight",6144))){$factors[$site[0]+'.lora_a']=[byte[]]::new($site[1]*4);$factors[$site[0]+'.lora_b']=$negative}}
    $adapterPath=Join-Path $Scratch 'adapter';$memory=[IO.MemoryStream]::new();$writer=[IO.BinaryWriter]::new($memory)
    $writer.Write([Text.Encoding]::ASCII.GetBytes('GGUF'));$writer.Write([uint32]3);$writer.Write([uint64]258);$writer.Write([uint64]4)
    foreach($pair in @(@('general.architecture','qwen35'),@('general.type','adapter'),@('adapter.type','lora'))){Write-FixtureText $writer $pair[0];$writer.Write([uint32]8);Write-FixtureText $writer $pair[1]}
    Write-FixtureText $writer 'adapter.lora_alpha';$writer.Write([uint32]6);$writer.Write([single]1)
    $offset=0L
    foreach($key in $factors.Keys){Write-FixtureText $writer $key;$writer.Write([uint32]2);$count=$factors[$key].Length/4;$shape=if($key.EndsWith('.lora_b')){@(1,5120)}elseif($key.StartsWith('token_embd.')){@(1,248320)}else{@($count,1)};$writer.Write([uint64]$shape[0]);$writer.Write([uint64]$shape[1]);$writer.Write([uint32]0);$writer.Write([uint64]$offset);$offset+=$factors[$key].Length}
    while($memory.Position%32){$writer.Write([byte]0)};foreach($key in $factors.Keys){$writer.Write([byte[]]$factors[$key])};$writer.Flush();[IO.File]::WriteAllBytes($adapterPath,$memory.ToArray());$writer.Dispose();$memory.Dispose()
    $config=[Text.Encoding]::UTF8.GetBytes('{"synthetic":true}');[IO.File]::WriteAllBytes((Join-Path $evidence 'config.json'),$config)
    $header=[ordered]@{};$cursor=0L
    foreach($block in $script:ReferenceBlocks){foreach($suffix in 'weight','scales','biases','signs'){[object[]]$shape=switch($suffix){weight{@(5120,384)}scales{@(5120,48)}biases{@(5120,48)}signs{@(6144)}};$dtype=switch($suffix){weight{'U32'}signs{'F32'}default{'F16'}};$length=1L;foreach($dimension in $shape){$length*=$dimension};$length*=if($dtype -ceq 'F16'){2}else{4};$header["language_model.model.layers.$block.linear_attn.out_proj.$suffix"]=@{dtype=$dtype;shape=$shape;data_offsets=@($cursor,($cursor+$length))};$cursor+=$length}}
    $headerRaw=[Text.Encoding]::UTF8.GetBytes((ConvertTo-Json -InputObject $header -Compress -Depth 8));[IO.File]::WriteAllBytes((Join-Path $evidence 'model-header.json'),$headerRaw)
    $pins=@{schemaVersion=2;arithmeticId=$script:ReferenceArithmetic;absoluteTolerance=0.00001;relativeTolerance=0.00001;normalization='scalar-f64-sum-sqrt-rounded-f32';reduction='scalar-sequential-f32-no-fma';model=@{filename='model.safetensors';bytes=$headerRaw.Length+8+$cursor;sha256=('1'*64);url='https://example.invalid/model'};config=@{filename='config.json';bytes=$config.Length;sha256=(Get-ReferenceDigest $config);url='https://example.invalid/config'};adapter=@{filename='adapter.gguf';bytes=(Get-Item -LiteralPath $adapterPath).Length;sha256=(Get-ReferenceFileDigest $adapterPath);url='https://example.invalid/adapter'};direction=@{filename='direction.bin';bytes=20480;sha256=(Get-ReferenceDigest $direction);url='https://example.invalid/direction'}}
    foreach($relative in $script:ReplaySourceFiles){$target=Join-Path $source $relative;[void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($target));$value=switch($relative){'tools/native-reference/public-authoring-pins.json'{$pins}'config/swift-lora.lock.json'{@{adapter=$pins.adapter}}'config/swift-lora.direction.json'{@{direction=$pins.direction}}default{@{syntheticSource=$relative}}};Write-ReferenceJson $target $value}
    $report=[ordered]@{schemaVersion=2;kind='independent-public-pack-A-reference';status='complete-public-reference-match';stage='complete';arithmeticId=$script:ReferenceArithmetic;referencePinsSha256=(Get-ReferenceFileDigest (Join-Path $source 'tools/native-reference/public-authoring-pins.json'));inputPins=$pins;repository='symbiogenesis/SwiftBonsai2Runner';workflowPath='.github/workflows/ptq1-public-authoring-reference.yml';sourceRevision=('a'*40);runId='123';runAttempt='1';runnerEnvironment='github-hosted';ownerDeviceTrials=0;modelInferenceCommands=0;directionF32NormBeforeNormalization=1.0;normalizedDirectionSha256=(Get-ReferenceDigest $direction);all129BComparisonsPassed=$true;all48PublicAComparisonPassed=$true;sites=@();modelHeaderFile='model-header.json';modelHeaderBytes=$headerRaw.Length;modelHeaderSha256=(Get-ReferenceDigest $headerRaw);modelTensorCount=$header.Count;authorOriginalProvenanceQualified=$false;nativeWriterBasisQualified=$false;logitsOrStateQualified=$false;defaultMayChange=$false;fullCheckpointWritten=$false}
    $descriptors=Get-AuditDescriptors $evidence $report $pins;$bResults=[Collections.Generic.List[object]]::new()
    foreach($key in $factors.Keys){if($key.EndsWith('.lora_b')){$bResults.Add(@{tensor=$key;sha256=(Get-ReferenceDigest $factors[$key]);comparedElements=5120;mismatchedElements=0;maximumAbsoluteDifference=0.0})}}
    Write-ReferenceJson (Join-Path $evidence 'direction-comparison.json') @{sites=$bResults.ToArray()}
    foreach($block in $script:ReferenceBlocks){$name='ssm-{0:D2}.f32le' -f $block;$raw=[byte[]]::new(24576);[IO.File]::WriteAllBytes((Join-Path $evidence $name),$raw);$prefix="language_model.model.layers.$block.linear_attn.out_proj";$wanted=[ordered]@{};$hashes=[ordered]@{};foreach($suffix in 'weight','scales','biases','signs'){$wanted[$suffix]=$descriptors[$prefix+'.'+$suffix];$hashes[$suffix]='b'*64};$report.sites+=@(@{block=$block;publicTensorPrefix=$prefix;candidateF16MatrixSha256=('c'*64);authoringAFile=$name;authoringASha256=(Get-ReferenceDigest $raw);authoringABytes=24576;shippedASha256=(Get-ReferenceDigest $raw);inputTensorDescriptors=$wanted;inputTensorSha256=$hashes;comparedElements=6144;mismatchedElements=0;maximumAbsoluteDifference=0.0})}
    Write-ReferenceJson (Join-Path $evidence 'toolchain.json') @{powershell='7.6.0';rust='rustc synthetic fixture';threads=1;reduction=$pins.reduction;normalization=$pins.normalization;numericExecutableSha256=('d'*64);referenceScriptSha256=(Get-ReferenceFileDigest (Join-Path $source 'tools/native-reference/public_pack_reference.ps1'));formatScriptSha256=(Get-ReferenceFileDigest (Join-Path $source 'tools/native-reference/public_pack_formats.ps1'));numericSourceSha256=(Get-ReferenceFileDigest (Join-Path $source 'tools/native-reference/reference_numerics.rs'))}
    return @{source=$source;evidence=$evidence;adapter=$adapterPath;direction=$directionPath;report=$report}
}
function Invoke-SyntheticReplay([scriptblock]$Mutate={},[string]$RunId='123'){
    $report=ConvertFrom-ReferenceJson ([Text.Encoding]::UTF8.GetBytes((ConvertTo-Json -InputObject $fixture.report -Depth 64)));& $Mutate $report
    Write-ReferenceJson (Join-Path $fixture.evidence 'report.json') $report;Write-ReferenceJson (Join-Path $fixture.evidence 'attempt.json') $report
    Invoke-PublicReferenceReplay $fixture.evidence $fixture.adapter $fixture.direction ('a'*40) $RunId '1' $fixture.source
}
function Add-SealedSource {Save-PublicReplaySource $fixture.source $fixture.evidence ('a'*40)}
$scratch=Join-Path ([IO.Path]::GetTempPath()) ('Bonsai-PublicReplay-Tests-'+[guid]::NewGuid().ToString('N'));[void][IO.Directory]::CreateDirectory($scratch)
$tests=[ordered]@{
    test_complete_scalar_replay_retains_qualification_boundaries={
        $result=Invoke-SyntheticReplay;Assert-Reference ($result.sites.Count -eq 48 -and $result.all48PublicAComparisonPassed -and -not $result.retainedReplaySourceVerified -and -not $result.producerAuthenticated -and -not $result.defaultMayChange) 'independent replay scope differs'
    }
    test_valid_losing_result_is_verified_without_relabelling_as_a_match={
        $path=Join-Path $fixture.evidence 'ssm-00.f32le';$original=[IO.File]::ReadAllBytes($path);$losing=$original.Clone();[Array]::Copy([BitConverter]::GetBytes([single]1),$losing,4);[IO.File]::WriteAllBytes($path,$losing)
        try{$result=Invoke-SyntheticReplay {$args[0].status='rejected-public-A-comparison';$args[0].all48PublicAComparisonPassed=$false;$args[0].sites[0].authoringASha256=Get-ReferenceDigest $losing;$args[0].sites[0].mismatchedElements=1;$args[0].sites[0].maximumAbsoluteDifference=1.0};Assert-Reference (-not $result.all48PublicAComparisonPassed -and $result.status -ceq 'retained-result-verified') 'losing result relabelled';Assert-Rejected {Invoke-SyntheticReplay}}finally{[IO.File]::WriteAllBytes($path,$original)}
    }
    test_fabricated_metrics_missing_duplicate_reordered_and_path_sites_reject={
        foreach($mode in 'metric','missing','duplicate','reordered','path','descriptor','promotion'){Assert-Rejected {Invoke-SyntheticReplay {$r=$args[0];switch($mode){metric{$r.sites[0].mismatchedElements=1}missing{$r.sites=$r.sites[0..46]}duplicate{$r.sites[1]=$r.sites[0]}reordered{[Array]::Reverse($r.sites)}path{$r.sites[0].authoringAFile='../ssm-00.f32le'}descriptor{$r.sites[0].inputTensorDescriptors.weight.absoluteByteBegin=0}promotion{$r.defaultMayChange=$true}}}}}
    }
    test_independent_producer_selection_and_source_bytes_reject_substitution={
        Assert-Rejected {Invoke-SyntheticReplay -RunId '124'};$path=Join-Path $fixture.source 'tools/native-reference/public_pack_formats.ps1';$raw=[IO.File]::ReadAllBytes($path);try{[IO.File]::WriteAllText($path,'substituted');Assert-Rejected {Invoke-SyntheticReplay}}finally{[IO.File]::WriteAllBytes($path,$raw)}
    }
    test_sealed_source_replays_with_different_checkout_bytes_without_executing_producer={
        $binding=Add-SealedSource;$path=Join-Path $fixture.source 'tools/native-reference/public_pack_formats.ps1';$raw=[IO.File]::ReadAllBytes($path)
        try{[IO.File]::WriteAllText($path,'changed reviewer checkout');$result=Invoke-SyntheticReplay {foreach($key in $binding.Keys){$args[0][$key]=$binding[$key]}};Assert-Reference ($result.retainedReplaySourceVerified -and -not $result.producerAuthenticated) 'sealed replay scope differs'}finally{[IO.File]::WriteAllBytes($path,$raw)}
    }
    test_sealed_source_rejects_missing_extra_changed_and_wrong_type_entries={
        $binding=Add-SealedSource;$bundle=Join-Path $fixture.evidence 'replay-source';$path=Join-Path $bundle 'tools/native-reference/public_pack_reference.ps1';$original=[IO.File]::ReadAllBytes($path)
        foreach($mode in 'missing','extra-root','extra-nested','changed','wrong-type','unbound'){
            $extra=$null
            try{
                switch($mode){missing{[IO.File]::Delete($path)}extra-root{$extra=Join-Path $bundle 'unexpected.txt';[IO.File]::WriteAllText($extra,'extra')}extra-nested{$extra=Join-Path $bundle 'tools/native-reference/unexpected.ps1';[IO.File]::WriteAllText($extra,'extra')}changed{[IO.File]::WriteAllText($path,'changed producer')}wrong-type{[IO.File]::Delete($path);[void][IO.Directory]::CreateDirectory($path)}}
                if($mode -ceq 'unbound'){Assert-Rejected {Invoke-SyntheticReplay}}else{Assert-Rejected {Invoke-SyntheticReplay {foreach($key in $binding.Keys){$args[0][$key]=$binding[$key]}}}}
            }finally{if($extra){[IO.File]::Delete($extra)};if([IO.Directory]::Exists($path)){[IO.Directory]::Delete($path)};[IO.File]::WriteAllBytes($path,$original)}
        }
    }
    test_sealed_source_rejects_manifest_tampering_paths_and_approved_lock_substitution={
        $binding=Add-SealedSource;$bundle=Join-Path $fixture.evidence 'replay-source';$path=Join-Path $bundle 'source-manifest.json';$original=[IO.File]::ReadAllBytes($path)
        foreach($mode in 'digest','revision','duplicate','path','outside','approved-lock'){
            $manifest=ConvertFrom-ReferenceJson $original;$restores=@{}
            try{
                switch($mode){revision{$manifest.sourceRevision='b'*40}duplicate{$manifest.files[1]=$manifest.files[0]}path{$manifest.files[0].path='../swift-lora.lock.json'}approved-lock{
                    $pinPath='tools/native-reference/public-authoring-pins.json';$pins=Read-ReferenceJson (Join-Path $bundle $pinPath);$pins.adapter.url='https://example.invalid/substitution'
                    foreach($relative in $pinPath,'config/swift-lora.lock.json'){$target=Join-Path $bundle $relative;$restores[$target]=[IO.File]::ReadAllBytes($target);Write-ReferenceJson $target $(if($relative -ceq $pinPath){$pins}else{@{adapter=$pins.adapter}});foreach($item in $manifest.files){if($item.path -ceq $relative){$item.bytes=(Get-Item -LiteralPath $target).Length;$item.sha256=Get-ReferenceFileDigest $target}}}
                }}
                Write-ReferenceJson $path $manifest
                Assert-Rejected {Invoke-SyntheticReplay {$args[0].replaySourceManifestFile=$(if($mode -ceq 'outside'){'../source-manifest.json'}else{$binding.replaySourceManifestFile});$args[0].replaySourceManifestSha256=$(if($mode -ceq 'digest'){'0'*64}else{Get-ReferenceFileDigest $path})}}
            }finally{[IO.File]::WriteAllBytes($path,$original);foreach($target in $restores.Keys){[IO.File]::WriteAllBytes($target,$restores[$target])}}
        }
    }
    test_sealed_source_preserves_a_verified_losing_result={
        $binding=Add-SealedSource;$path=Join-Path $fixture.evidence 'ssm-00.f32le';$losing=[byte[]]::new(24576);[Array]::Copy([BitConverter]::GetBytes([single]1),$losing,4);[IO.File]::WriteAllBytes($path,$losing)
        $result=Invoke-SyntheticReplay {foreach($key in $binding.Keys){$args[0][$key]=$binding[$key]};$args[0].status='rejected-public-A-comparison';$args[0].all48PublicAComparisonPassed=$false;$args[0].sites[0].authoringASha256=Get-ReferenceDigest $losing;$args[0].sites[0].mismatchedElements=1;$args[0].sites[0].maximumAbsoluteDifference=1.0};Assert-Reference ($result.retainedReplaySourceVerified -and -not $result.all48PublicAComparisonPassed) 'sealed losing replay differs'
    }
    test_sealed_source_rejects_recipe_substitution_despite_consistent_report_and_manifest={
        $binding=Add-SealedSource;$bundle=Join-Path $fixture.evidence 'replay-source';$relative='tools/native-reference/public-authoring-pins.json';$pin=Join-Path $bundle $relative;$manifestPath=Join-Path $bundle 'source-manifest.json';$pinOriginal=[IO.File]::ReadAllBytes($pin);$manifestOriginal=[IO.File]::ReadAllBytes($manifestPath);$configPath=Join-Path $fixture.evidence 'config.json';$configOriginal=[IO.File]::ReadAllBytes($configPath)
        foreach($mode in 'model','config'){
            try{
                $pins=ConvertFrom-ReferenceJson $pinOriginal;$manifest=ConvertFrom-ReferenceJson $manifestOriginal
                if($mode -ceq 'model'){$pins.model.sha256='0'*64}else{$raw=[Text.Encoding]::UTF8.GetBytes('{"substituted":true}');$pins.config.bytes=$raw.Length;$pins.config.sha256=Get-ReferenceDigest $raw;[IO.File]::WriteAllBytes($configPath,$raw)}
                Write-ReferenceJson $pin $pins;foreach($item in $manifest.files){if($item.path -ceq $relative){$item.bytes=(Get-Item -LiteralPath $pin).Length;$item.sha256=Get-ReferenceFileDigest $pin}};Write-ReferenceJson $manifestPath $manifest
                Assert-Rejected {Invoke-SyntheticReplay {$args[0].replaySourceManifestFile=$binding.replaySourceManifestFile;$args[0].replaySourceManifestSha256=Get-ReferenceFileDigest $manifestPath;$args[0].inputPins=$pins;$args[0].referencePinsSha256=Get-ReferenceFileDigest $pin}}
            }finally{[IO.File]::WriteAllBytes($pin,$pinOriginal);[IO.File]::WriteAllBytes($manifestPath,$manifestOriginal);[IO.File]::WriteAllBytes($configPath,$configOriginal)}
        }
    }
    test_missing_or_extra_vector_files_reject={
        $path=Join-Path $fixture.evidence 'ssm-00.f32le';$raw=[IO.File]::ReadAllBytes($path);[IO.File]::Delete($path);Assert-Rejected {Invoke-SyntheticReplay};[IO.File]::WriteAllBytes($path,$raw)
        [IO.File]::WriteAllBytes((Join-Path $fixture.evidence 'ssm-03.f32le'),[byte[]]::new(24576));Assert-Rejected {Invoke-SyntheticReplay}
    }
    test_duplicate_nonfinite_json_truncation_and_misaligned_adapter_reject={
        foreach($raw in '{"x":1,"x":2}','{"x":NaN}','{"x":1e999}'){Assert-Rejected {ConvertFrom-ReferenceJson ([Text.Encoding]::UTF8.GetBytes($raw))}}
        foreach($raw in @([byte[]]@(),([Text.Encoding]::ASCII.GetBytes('GGUF')+[BitConverter]::GetBytes([uint32]3)+[BitConverter]::GetBytes([uint64]258)+[BitConverter]::GetBytes([uint64]0)))){Assert-Rejected {Read-AuditAdapter $raw}}
    }
}
try{$selected=if($Name.Count){@($Name|ForEach-Object{($_ -split '\.')[-1]})}elseif($env:GITHUB_ACTIONS -ceq 'true'){@($tests.Keys)}else{throw 'Select a named focused regression locally; complete tests are CI-only.'};foreach($case in $selected){Assert-Reference ($tests.Contains($case)) 'unknown focused regression';$lane=Join-Path $scratch ([guid]::NewGuid().ToString('N'));[void][IO.Directory]::CreateDirectory($lane);$fixture=New-PublicReplayFixture $lane;& $tests[$case];Write-Output "PASS $case"}}finally{[IO.Directory]::Delete($scratch,$true)}
