#Requires -Version 7.6
[CmdletBinding()]
param([Parameter(ValueFromRemainingArguments)][string[]]$Name=@(),[string]$ToolPath='')
. (Join-Path $PSScriptRoot 'public_pack_reference.ps1') -ToolPath $ToolPath
function Assert-Rejected([scriptblock]$Action){$rejected=$false;try{& $Action 2>$null|Out-Null}catch{$rejected=$true};Assert-Reference $rejected 'fixture expected rejection'}
function Encode-Fixture($Value){return ,([Text.Encoding]::UTF8.GetBytes((ConvertTo-Json -InputObject $Value -Depth 64 -Compress)))}
function New-FixedConfig {
    return @{schema_version=2;model_type='prism_hadamard_qwen35';base_model_type='qwen3_5';tensor_namespace='mlx-vlm-qwen3_5';gdn_activation_layout='grouped';quantization=@{bits=2;group_size=128;mode='affine'};modules=@($script:ReferenceBlocks|ForEach-Object{@{path="model.layers.$_.linear_attn.out_proj";block=1024;dtype='float16';embedding=$false}})}
}
function New-ReconstructionFixture([string]$Directory,[int]$Rows=2,[int]$Width=2048){
    [void][IO.Directory]::CreateDirectory($Directory);$path=Join-Path $Directory 'affine';$stream=[IO.File]::Create($path);$writer=[IO.BinaryWriter]::new($stream)
    $weight=[byte[]]::new($Rows*$Width/4);[Array]::Fill($weight,[byte]0x55);$weight[0]=$weight[0] -band 0x3f -bor 0x80
    if($Rows -eq 2){$position=$Width+1024+5;$weight[[int][Math]::Floor($position/4)]=$weight[[int][Math]::Floor($position/4)] -band (0xff -bxor (3 -shl (($position%4)*2))) -bor (2 -shl (($position%4)*2))}
    $writer.Write($weight);$scales=$stream.Position;for($i=0;$i -lt $Rows*$Width/128;$i++){$writer.Write([uint16]0x3c00)};$biases=$stream.Position;for($i=0;$i -lt $Rows*$Width/128;$i++){$writer.Write([uint16]0xbc00)};$signs=$stream.Position
    for($i=0;$i -lt $Width;$i++){$writer.Write([single]$(if(($i -band 8) -eq 0){1}else{-1}))}
    $writer.Dispose();$stream.Dispose();$direction=Join-Path $Directory 'direction';[single[]]$values=if($Rows -eq 2){@(0.25,-0.75)}else{@(1)};$raw=[byte[]]::new($values.Length*4);[Buffer]::BlockCopy($values,0,$raw,0,$raw.Length);[IO.File]::WriteAllBytes($direction,$raw)
    return @{path=$path;scales=$scales;biases=$biases;signs=$signs;rows=$Rows;width=$Width;direction=$direction;output=(Join-Path $Directory 'vector');matrix=(Join-Path $Directory 'matrix')}
}
function Invoke-FixtureReconstruction($Fixture){Invoke-ReferenceNumeric (Get-ReferenceNumericTool $ToolPath) @('reconstruct',$Fixture.path,'0',[string]$Fixture.scales,[string]$Fixture.biases,[string]$Fixture.signs,[string]$Fixture.rows,[string]$Fixture.width,$Fixture.direction,$Fixture.output,$Fixture.matrix,'F32')}
$scratch=Join-Path ([IO.Path]::GetTempPath()) ('Bonsai-PublicReference-Tests-'+[guid]::NewGuid().ToString('N'));[void][IO.Directory]::CreateDirectory($scratch)
$tests=[ordered]@{
    test_safetensors_rejects_ambiguous_json_and_pre_read_header_bounds={
        foreach($raw in '{"x":1,"x":2}','{"x":NaN}','{"x":1e999}','{}{}'){Assert-Rejected {ConvertFrom-ReferenceJson ([Text.Encoding]::UTF8.GetBytes($raw))}}
        Assert-Rejected {ConvertFrom-ReferenceJson ([byte[]]@(255))}
        foreach($raw in @([byte[]]@(),[BitConverter]::GetBytes([uint64]4194305),([BitConverter]::GetBytes([uint64]8)+[Text.Encoding]::UTF8.GetBytes('{}')))){$stream=[IO.MemoryStream]::new($raw,$false);try{Assert-Rejected {Read-ReferenceSafetensorsHeader $stream}}finally{$stream.Dispose()}}
    }
    test_safetensors_checks_every_extent_dtype_and_payload_tail={
        $valid=@{tensor=@{dtype='F32';shape=@(1);data_offsets=@(0,4)}};$raw=Encode-Fixture $valid;$descriptors=Read-ReferenceTensorHeader $raw ($raw.Length+12);Assert-Reference ($descriptors.tensor.absoluteByteEnd-$descriptors.tensor.absoluteByteBegin -eq 4) 'valid tensor extent differs'
        foreach($pair in @(@('dtype','UNKNOWN'),@('shape',@(-1)),@('shape',@($true)),@('shape',@(1000000001)),@('data_offsets',@(0,5)))){$value=@{tensor=@{dtype='F32';shape=@(1);data_offsets=@(0,4)}};$value.tensor[$pair[0]]=$pair[1];$raw=Encode-Fixture $value;Assert-Rejected {Read-ReferenceTensorHeader $raw ($raw.Length+12)}}
        foreach($value in @(@{a=$valid.tensor;b=$valid.tensor},@{a=@{dtype='F32';shape=@(1);data_offsets=@(4,8)}},@{__metadata__=@{bad=1};tensor=$valid.tensor})){$raw=Encode-Fixture $value;Assert-Rejected {Read-ReferenceTensorHeader $raw ($raw.Length+12)}}
    }
    test_adapter_reader_rejects_version_inventory_ambiguous_metadata_and_truncation={
        foreach($raw in @([byte[]]@(),([Text.Encoding]::ASCII.GetBytes('GGUF')+[BitConverter]::GetBytes([uint32]2)),([Text.Encoding]::ASCII.GetBytes('GGUF')+[BitConverter]::GetBytes([uint32]3)+[BitConverter]::GetBytes([uint64]257)+[BitConverter]::GetBytes([uint64]0)))){Assert-Rejected {Read-ReferenceAdapter $raw}}
        foreach($mode in 'metadata-count','duplicate','oversized-text'){
            $memory=[IO.MemoryStream]::new();$writer=[IO.BinaryWriter]::new($memory);$writer.Write([Text.Encoding]::ASCII.GetBytes('GGUF'));$writer.Write([uint32]3);$writer.Write([uint64]258);$writer.Write([uint64]$(if($mode -ceq 'metadata-count'){129}elseif($mode -ceq 'duplicate'){2}else{1}))
            if($mode -cne 'metadata-count'){foreach($entry in 1..$(if($mode -ceq 'duplicate'){2}else{1})){$key=[Text.Encoding]::UTF8.GetBytes('general.type');$writer.Write([uint64]$key.Length);$writer.Write($key);$writer.Write([uint32]8);if($mode -ceq 'oversized-text'){$writer.Write([uint64]1048577)}else{$text=[Text.Encoding]::UTF8.GetBytes('adapter');$writer.Write([uint64]$text.Length);$writer.Write($text)}}}
            $writer.Flush();$raw=$memory.ToArray();$writer.Dispose();$memory.Dispose();Assert-Rejected {Read-ReferenceAdapter $raw}
        }
    }
    test_affine_f16_matches_independent_fma_over_complete_accepted_f16_domain={
        $result=Invoke-ReferenceNumeric (Get-ReferenceNumericTool $ToolPath) @('self-check');Assert-Reference ($result.passed -and $result.binary16Roundtrips -eq 63488 -and $result.affineCases -eq 184320) 'binary16 codec/affine oracle self-check differs'
    }
    test_bit_order_block_normalization_and_inverse_signs_match_closed_form={
        $fixture=New-ReconstructionFixture (Join-Path $scratch 'closed-form');[void](Invoke-FixtureReconstruction $fixture);$raw=[IO.File]::ReadAllBytes($fixture.output)
        for($i=0;$i -lt 2048;$i++){$position=if($i -lt 1024){3}else{5};$amplitude=if($i -lt 1024){0.25}else{-0.75};$parity=[Numerics.BitOperations]::PopCount([uint32](($i%1024) -band $position))%2;$sign=if(($i -band 8) -eq 0){1}else{-1};$expected=[single]($amplitude*$(if($parity){-1}else{1})*$sign/32);Assert-Reference ([BitConverter]::SingleToInt32Bits([BitConverter]::ToSingle($raw,$i*4)) -eq [BitConverter]::SingleToInt32Bits($expected)) 'closed-form inverse differs'}
    }
    test_candidate_checkpoint_f16_round_is_observable_and_not_omitted={
        $fixture=New-ReconstructionFixture (Join-Path $scratch 'rounding') 1 1024;$raw=[IO.File]::ReadAllBytes($fixture.path);[Array]::Fill($raw,[byte]0x55,0,256);$raw[0]=0x56;$raw[32]=0x56;[Array]::Copy([BitConverter]::GetBytes([uint16]0x1000),0,$raw,[int]($fixture.scales+2),2);[Array]::Copy([BitConverter]::GetBytes([uint16]0x9000),0,$raw,[int]($fixture.biases+2),2);[IO.File]::WriteAllBytes($fixture.path,$raw);[void](Invoke-FixtureReconstruction $fixture)
        $observed=[BitConverter]::ToSingle([IO.File]::ReadAllBytes($fixture.output),0);Assert-Reference ($observed -eq [single]0.03125 -and [double]$observed -ne (1+[Math]::Pow(2,-11))/32) 'F16 checkpoint storage round was omitted'
    }
    test_reconstruction_rejects_invalid_codes_biases_signs_and_finite_domain={
        foreach($mode in 'code','bias','sign','scale'){$fixture=New-ReconstructionFixture (Join-Path $scratch $mode) 1 1024;$raw=[IO.File]::ReadAllBytes($fixture.path);switch($mode){code{$raw[0]=0xff}bias{$raw[[int]$fixture.biases+1]=0x3c}sign{[Array]::Clear($raw,[int]$fixture.signs,4)}scale{$raw[[int]$fixture.scales+1]=0x7c}};[IO.File]::WriteAllBytes($fixture.path,$raw);Assert-Rejected {Invoke-FixtureReconstruction $fixture}}
    }
    test_existing_comparison_bound_has_no_tolerance_search={
        . (Join-Path $PSScriptRoot 'audit_public_reference.ps1');$a=Get-AuditMetrics @([single]0.000009) @([single]0);$b=Get-AuditMetrics @([single]0.000011) @([single]0);Assert-Reference ($a.mismatchedElements -eq 0 -and $b.mismatchedElements -eq 1) 'fixed tolerance changed'
    }
    test_config_rejects_permutation_layout_missing_or_duplicate_sites={
        Assert-Reference ((Get-PublicConfigSites (Encode-Fixture (New-FixedConfig))).Count -eq 48) 'valid configuration rejected'
        foreach($mode in 'layout','schema','missing','duplicate','transform'){$config=New-FixedConfig;switch($mode){layout{$config.gdn_activation_layout='ungrouped'}schema{$config.schema_version=1}missing{$config.modules=$config.modules[0..46]}duplicate{$config.modules+=@($config.modules[0])}transform{$config.modules[0].block=512}};Assert-Rejected {Get-PublicConfigSites (Encode-Fixture $config)}}
    }
    test_hosted_admission_rejects_owner_nonmain_and_wrong_thread_policy={
        $names=@('GITHUB_ACTIONS','RUNNER_ENVIRONMENT','RUNNER_OS','RUNNER_ARCH','GITHUB_REPOSITORY','GITHUB_REF','GITHUB_EVENT_NAME','GITHUB_SHA');$old=@{};foreach($name in $names){$old[$name]=[Environment]::GetEnvironmentVariable($name)}
        try{$values=@('true','github-hosted','Windows','ARM64','symbiogenesis/SwiftBonsai2Runner','refs/heads/main','workflow_dispatch',('a'*40));for($i=0;$i -lt $names.Count;$i++){[Environment]::SetEnvironmentVariable($names[$i],$values[$i])};Assert-PublicHosted;foreach($pair in @(@('RUNNER_ENVIRONMENT','self-hosted'),@('GITHUB_REF','refs/heads/work'),@('RUNNER_ARCH','X64'),@('GITHUB_SHA','main'))){$prior=[Environment]::GetEnvironmentVariable($pair[0]);[Environment]::SetEnvironmentVariable($pair[0],$pair[1]);Assert-Rejected {Assert-PublicHosted};[Environment]::SetEnvironmentVariable($pair[0],$prior)}}finally{foreach($name in $names){[Environment]::SetEnvironmentVariable($name,$old[$name])}}
    }
    test_transfer_checks_complete_hash_length_before_publishing_input={
        $raw=[Text.Encoding]::UTF8.GetBytes('fixed synthetic input');$pin=@{bytes=$raw.Length;sha256=(Get-ReferenceDigest $raw)}
        foreach($received in @($raw,$raw[0..($raw.Length-2)],($raw+[byte[]]@(1)),[byte[]]::new($raw.Length))){$input=[IO.MemoryStream]::new([byte[]]$received,$false);$output=[IO.MemoryStream]::new();try{if(Test-ReferenceEqual $raw $received){Copy-PublicTransfer $input $output $pin}else{Assert-Rejected {Copy-PublicTransfer $input $output $pin}}}finally{$input.Dispose();$output.Dispose()}}
    }
    test_directory_admission_rejects_alias_escape_and_redirected_ancestors={
        $temporary=Join-Path $scratch 'admission';$outside=Join-Path $scratch 'admission-outside'
        [void][IO.Directory]::CreateDirectory($temporary);[void][IO.Directory]::CreateDirectory($outside)
        $data=Join-Path $temporary 'new/data';$evidence=Join-Path $temporary 'new/evidence'
        Assert-PublicDirectories $data $evidence $temporary
        Assert-Rejected {Assert-PublicDirectories 'relative' $evidence $temporary}
        Assert-Rejected {Assert-PublicDirectories (Join-Path $outside 'data') $evidence $temporary}
        Assert-Rejected {Assert-PublicDirectories (Join-Path $temporary '../escaped') $evidence $temporary}
        Assert-Rejected {Assert-PublicDirectories $data $data $temporary}
        if($IsWindows){Assert-Rejected {Assert-PublicDirectories $data $data.ToUpperInvariant() $temporary}}
        $file=Join-Path $temporary 'file';[IO.File]::WriteAllText($file,'non-directory fixture')
        Assert-Rejected {Assert-PublicDirectories (Join-Path $file 'data') $evidence $temporary}
        $redirect=Join-Path $temporary 'redirect'
        try {
            if($IsWindows){[void](New-Item -ItemType Junction -Path $redirect -Target $outside -ErrorAction Stop)}else{[void][IO.Directory]::CreateSymbolicLink($redirect,$outside)}
            Assert-Rejected {Assert-PublicDirectories (Join-Path $redirect 'data') $evidence $temporary}
            Assert-Rejected {Assert-PublicDirectories $data (Join-Path $redirect 'evidence') $temporary}
            Assert-Rejected {Assert-PublicDirectories (Join-Path $redirect 'data') (Join-Path $redirect 'evidence') $redirect}
            Assert-Reference (-not [IO.Directory]::Exists((Join-Path $outside 'data')) -and -not [IO.Directory]::Exists((Join-Path $outside 'evidence'))) 'rejected admission wrote outside runner temp'
        }finally{if([IO.Directory]::Exists($redirect)){[IO.Directory]::Delete($redirect)}}
        Assert-Reference (-not [IO.Directory]::Exists((Join-Path $temporary 'new'))) 'admission created directories before validation completed'
    }
    test_replay_bundle_preserves_exact_source_bytes_and_refuses_overwrite={
        $repository=Join-Path $scratch 'source';$evidence=Join-Path $scratch 'evidence';[void][IO.Directory]::CreateDirectory($evidence)
        foreach($relative in $script:ReplaySourceFiles){$target=Join-Path $repository $relative;[void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($target));[IO.File]::WriteAllText($target,$relative+"`r`n",[Text.UTF8Encoding]::new($false))}
        $binding=Save-PublicReplaySource $repository $evidence ('a'*40);$manifest=Read-ReferenceJson (Join-Path $evidence $binding.replaySourceManifestFile);Assert-Reference ($manifest.files.Count -eq $script:ReplaySourceFiles.Count) 'retained source selection differs'
        foreach($item in $manifest.files){$raw=Read-ReferenceBounded (Join-Path $evidence ('replay-source/'+$item.path)) $item.bytes $item.bytes $item.sha256;Assert-Reference ((Get-ReferenceDigest $raw) -ceq (Get-ReferenceFileDigest (Join-Path $repository $item.path))) 'retained source bytes differ'}
        Assert-Rejected {Save-PublicReplaySource $repository $evidence ('a'*40)}
    }
}
try{$selected=if($Name.Count){@($Name|ForEach-Object{($_ -split '\.')[-1]})}elseif($env:GITHUB_ACTIONS -ceq 'true'){@($tests.Keys)}else{throw 'Select a named focused regression locally; complete tests are CI-only.'};foreach($case in $selected){Assert-Reference ($tests.Contains($case)) 'unknown focused regression';& $tests[$case];Write-Output "PASS $case"}}finally{[IO.Directory]::Delete($scratch,$true)}
