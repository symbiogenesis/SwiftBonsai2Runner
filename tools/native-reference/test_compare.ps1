#Requires -Version 7.6
[CmdletBinding()]
param([Parameter(ValueFromRemainingArguments)][string[]]$Name=@(),[string]$ToolPath='')
. (Join-Path $PSScriptRoot 'compare.ps1') -ToolPath $ToolPath
function Assert-Rejected([scriptblock]$Action){$rejected=$false;try{& $Action 2>$null|Out-Null}catch{$rejected=$true};Assert-Reference $rejected 'fixture expected rejection'}
function Write-FloatFixture([string]$Path,[single[]]$Values){$bytes=[byte[]]::new($Values.Length*4);[Buffer]::BlockCopy($Values,0,$bytes,0,$bytes.Length);[IO.File]::WriteAllBytes($Path,$bytes)}
function Write-NativeFixture([string]$Path,[bool]$Transposed){
    $memory=[IO.MemoryStream]::new();$writer=[IO.BinaryWriter]::new($memory)
    $writer.Write([Convert]::ToUInt32('AF143CD8',16));$writer.Write([int]0);$writer.Write([uint32]1);$writer.Write([uint32]2)
    foreach($token in 0,1){$writer.Write([int]$token);$writer.Write([uint32]1);$writer.Write([int]$token);$writer.Write([int]$token);$writer.Write([int]0)}
    $writer.Write([uint32]$Transposed);$writer.Write([uint32]1);$writer.Write([int]1);$writer.Write([uint64]4)
    foreach($value in 1,2,3,4){$writer.Write([BitConverter]::HalfToUInt16Bits([Half]$value))}
    $writer.Write([int]1)
    if($Transposed){$writer.Write([uint32]2);$writer.Write([uint32]2);$values=5,7,6,8}else{$writer.Write([uint64]4);$values=5,6,7,8}
    foreach($value in $values){$writer.Write([BitConverter]::HalfToUInt16Bits([Half]$value))}
    $writer.Write([uint32]1);$writer.Write([int]1);$writer.Write([uint32]0);$writer.Write([uint32]0);$writer.Write([uint32]4)
    foreach($values in @(@(9,10,11),@(12,13,14,15))){$writer.Write([int]0);$writer.Write([uint64]($values.Count*4));foreach($value in $values){$writer.Write([single]$value)}}
    $writer.Flush();[IO.File]::WriteAllBytes($Path,$memory.ToArray());$writer.Dispose();$memory.Dispose()
}
function Read-SmallComponents([string]$Path){Get-NativeReferenceComponents $Path 1 @(3) @(0) 4 2 3 4}
$scratch=Join-Path ([IO.Path]::GetTempPath()) ('Bonsai-NativeReference-Tests-'+[guid]::NewGuid().ToString('N'));[void][IO.Directory]::CreateDirectory($scratch)
$tests=[ordered]@{
    test_transposed_v_and_complete_recurrent_inventory_match_logical_rows={
        $tool=Get-ReferenceNumericTool $ToolPath
        foreach($transposed in $false,$true){
            $path=Join-Path $scratch ('state-'+$transposed);Write-NativeFixture $path $transposed;$components=Read-SmallComponents $path
            Assert-Reference ($components.Count -eq 6) 'complete recurrent inventory differs'
            $expected=@{'block-3-key-0.f32le'=@(1,2);'block-3-key-1.f32le'=@(3,4);'block-3-value-0.f32le'=@(5,6);'block-3-value-1.f32le'=@(7,8);'block-0-convolution.f32le'=@(9,10,11);'block-0-recurrent.f32le'=@(12,13,14,15)}
            foreach($component in $components){$actual=Join-Path $scratch 'actual';Write-FloatFixture $actual $expected[$component.path];$result=Invoke-ReferenceNumeric $tool @('compare',$path,$actual,$component.code,[string]$component.offset,[string]$component.elements,[string]$component.stride,'0.001','0.001','plain');Assert-Reference $result.byteExact 'logical row differs'}
        }
    }
    test_state_decoder_rejects_truncation_trailing_bytes_and_changed_schema={
        $path=Join-Path $scratch 'state';Write-NativeFixture $path $true;$raw=[IO.File]::ReadAllBytes($path)
        $bad=[byte[]]::new($raw.Length-1);[Array]::Copy($raw,$bad,$bad.Length);[IO.File]::WriteAllBytes($path,$bad);Assert-Rejected {Read-SmallComponents $path}
        [IO.File]::WriteAllBytes($path,($raw+[byte[]]@(1)));Assert-Rejected {Read-SmallComponents $path}
        foreach($offset in 0,4,8){$bad=$raw.Clone();$bad[$offset]=2;[IO.File]::WriteAllBytes($path,$bad);Assert-Rejected {Read-SmallComponents $path}}
    }
    test_capture_pre_read_bounds_reject_oversized_json_and_logits={
        $path=Join-Path $scratch 'bounded';[IO.File]::WriteAllBytes($path,[byte[]]::new(17));Assert-Rejected {Read-ReferenceBounded $path 16}
    }
    test_comparator_enforces_finite_values_exact_extents_and_original_tolerance={
        $tool=Get-ReferenceNumericTool $ToolPath;$reference=Join-Path $scratch 'reference';$actual=Join-Path $scratch 'actual'
        [IO.File]::WriteAllBytes($reference,[byte[]]@(0,60,0,128))
        foreach($pair in @(@(1.001,0),@(1.003,1))){Write-FloatFixture $actual @($pair[0],-0.0);$result=Invoke-ReferenceNumeric $tool @('compare',$reference,$actual,'f16','0','2','2','0.001','0.001','plain');Assert-Reference ($result.failedElements -eq $pair[1]) 'fixed tolerance differs'}
        Write-FloatFixture $actual @([single]::NaN,0);Assert-Rejected {Invoke-ReferenceNumeric $tool @('compare',$reference,$actual,'f16','0','2','2','0.001','0.001','plain')}
        Write-FloatFixture $actual @(1);Assert-Rejected {Invoke-ReferenceNumeric $tool @('compare',$reference,$actual,'f16','0','2','2','0.001','0.001','plain')}
    }
    test_exact_chunks_reject_nonfinite_values_and_preserve_signed_zero={
        $tool=Get-ReferenceNumericTool $ToolPath;$reference=Join-Path $scratch 'reference';$actual=Join-Path $scratch 'actual';$values=[single[]]::new(4097)
        for($i=0;$i -lt $values.Length;$i++){$values[$i]=@(0.0,-0.0,[Math]::Pow(2,-24),-65504.0)[$i%4]}
        foreach($format in 'f16','f32'){
            if($format -ceq 'f16'){$raw=[byte[]]::new($values.Length*2);for($i=0;$i -lt $values.Length;$i++){[Array]::Copy([BitConverter]::GetBytes([BitConverter]::HalfToUInt16Bits([Half]$values[$i])),0,$raw,$i*2,2)}}else{$raw=[byte[]]::new($values.Length*4);[Buffer]::BlockCopy($values,0,$raw,0,$raw.Length)}
            [IO.File]::WriteAllBytes($reference,$raw);Write-FloatFixture $actual $values;$stride=if($format -ceq 'f16'){'2'}else{'4'}
            $result=Invoke-ReferenceNumeric $tool @('compare',$reference,$actual,$format,'0','4097',$stride,'0.001','0.001','plain');Assert-Reference ($result.byteExact -and $result.failedElements -eq 0) 'exact chunks differ'
            $bytes=[IO.File]::ReadAllBytes($actual);$bytes[7]=$bytes[7] -bxor 0x80;[IO.File]::WriteAllBytes($actual,$bytes);$result=Invoke-ReferenceNumeric $tool @('compare',$reference,$actual,$format,'0','4097',$stride,'0.001','0.001','plain');Assert-Reference (-not $result.byteExact -and $result.failedElements -eq 0) 'signed-zero exactness lost'
            foreach($index in 0,4095,4096){foreach($special in [single]::NaN,[single]::PositiveInfinity,[single]::NegativeInfinity){$changed=$values.Clone();$changed[$index]=$special;Write-FloatFixture $actual $changed;Assert-Rejected {Invoke-ReferenceNumeric $tool @('compare',$reference,$actual,$format,'0','4097',$stride,'0.001','0.001','plain')}}}
        }
    }
    test_selected_adapter_condition_binds_loading_scale_and_rust_identity={
        foreach($condition in 'standard','loaded-zero','no-adapter'){
            $capture=@{adapterLoaded=($condition -cne 'no-adapter');adapterScale=$(if($condition -ceq 'standard'){1.3}else{0})};Assert-NativeAdapterCondition $capture $condition
            foreach($other in 'standard','loaded-zero','no-adapter','unknown'){if($other -cne $condition){Assert-Rejected {Assert-NativeAdapterCondition $capture $other}}}
        }
        foreach($scale in $true,$null,'0',-0.0,[double]::NaN,[double]::PositiveInfinity){Assert-Rejected {Assert-NativeAdapterCondition @{adapterLoaded=$true;adapterScale=$scale} 'loaded-zero'}}
    }
    test_explicit_backend_controls_reject_substitution_and_unknown_backends={
        foreach($backend in 'auto','scalar','neon'){
            Write-ControlFixture $backend 1 0
            [void](Assert-NativeRustControls $scratch @{adapterLoaded=$true;adapterScale=1.3} $backend 1)
            foreach($other in 'auto','scalar','neon','unknown'){if($other -cne $backend){Assert-Rejected {Assert-NativeRustControls $scratch @{adapterLoaded=$true;adapterScale=1.3} $other 1}}}
        }
    }
    test_control_binding_rejects_changed_adapter_scale_tokens_and_backend={
        Write-ControlFixture 'auto' 1 0
        foreach($capture in @(@{adapterLoaded=$true;adapterScale=0},@{adapterLoaded=$false;adapterScale=0})){Assert-Rejected {Assert-NativeRustControls $scratch $capture 'auto' 1}}
        foreach($field in 'model_sha256','adapter_sha256','target_pairs','scale_bits','input_token'){
            Write-ControlFixture 'auto' 1 0;$path=Join-Path $scratch 'stdout.txt';$records=@([IO.File]::ReadAllLines($path)|ForEach-Object{ConvertFrom-ReferenceJson ([Text.Encoding]::UTF8.GetBytes($_))})
            $index=if($field -ceq 'input_token'){2}else{1};$records[$index][$field]=if($field -ceq 'target_pairs'){128}elseif($field -ceq 'input_token'){198}else{'substituted'}
            Write-ControlRecords $records;Assert-Rejected {Assert-NativeRustControls $scratch @{adapterLoaded=$true;adapterScale=1.3} 'auto' 1}
        }
    }
    test_row_worker_controls_reject_cross_worker_and_missing_parallel_execution={
        foreach($workers in 1,4){foreach($backend in 'auto','scalar','neon'){$operations=if($workers -eq 4){123}else{0};Write-ControlFixture $backend $workers $operations;[void](Assert-NativeRustControls $scratch @{adapterLoaded=$true;adapterScale=1.3} $backend $workers);$other=if($workers -eq 4){1}else{4};Assert-Rejected {Assert-NativeRustControls $scratch @{adapterLoaded=$true;adapterScale=1.3} $backend $other}}}
        foreach($field in 'row_workers','parallel_row_operations'){foreach($value in $null,$true,'4',-1,1.0){Write-ControlFixture 'auto' 4 123;$records=Read-ControlRecords;if($null -eq $value){$records[2].Remove($field)}else{$records[2][$field]=$value};Write-ControlRecords $records;Assert-Rejected {Assert-NativeRustControls $scratch @{adapterLoaded=$true;adapterScale=1.3} 'auto' 4}}}
        Write-ControlFixture 'auto' 4 0;Assert-Rejected {Assert-NativeRustControls $scratch @{adapterLoaded=$true;adapterScale=1.3} 'auto' 4}
        Write-ControlFixture 'auto' 1 1;Assert-Rejected {Assert-NativeRustControls $scratch @{adapterLoaded=$true;adapterScale=1.3} 'auto' 1}
    }
}
function Write-ControlRecords($Records){$lines=@($Records|ForEach-Object{ConvertTo-Json -InputObject $_ -Compress -Depth 8});[IO.File]::WriteAllLines((Join-Path $scratch 'stdout.txt'),$lines,[Text.UTF8Encoding]::new($false))}
function Read-ControlRecords{return ,@([IO.File]::ReadAllLines((Join-Path $scratch 'stdout.txt'))|ForEach-Object{ConvertFrom-ReferenceJson ([Text.Encoding]::UTF8.GetBytes($_))})}
function Write-ControlFixture([string]$Backend,$Workers,$Operations){Write-ControlRecords @(@{event='weights-loaded';model_sha256=$script:NativeModel;weight_representation='current-repacked-PTQ1_0'},@{event='rank-one-adapter-loaded';model_sha256=$script:NativeModel;adapter_sha256=$script:NativeAdapter;target_pairs=129;scale_bits='3fa66666'},@{event='forward-step';position=0;input_token=248045;backend_requested=$Backend;row_workers=$Workers;parallel_row_operations=$Operations})}
try {
    $selected=if($Name.Count -gt 0){@($Name|ForEach-Object{($_ -split '\.')[-1]})}elseif($env:GITHUB_ACTIONS -ceq 'true'){@($tests.Keys)}else{throw 'Select a named focused regression locally; complete tests are CI-only.'}
    foreach($case in $selected){Assert-Reference ($tests.Contains($case)) 'unknown focused regression';& $tests[$case];Write-Output "PASS $case"}
}finally{[IO.Directory]::Delete($scratch,$true)}
