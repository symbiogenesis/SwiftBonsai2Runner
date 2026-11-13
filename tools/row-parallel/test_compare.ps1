#requires -Version 7.6
[CmdletBinding()]
param([string]$ToolPath = $env:BONSAI_REFERENCE_NUMERIC_TOOL,
    [ValidateSet('complete','extent','nonfinite','signed_zero','dispatch','controls','records','duration','binding','inventory')]
    [string[]]$Case = @('complete','extent','nonfinite','signed_zero','dispatch','controls','records','duration','binding','inventory'))
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'compare.ps1') -ToolPath $ToolPath
function Expect-Rejection([scriptblock]$Action, [string]$Pattern = '.') {
    $caught = $false
    try { & $Action | Out-Null } catch { $caught = $true; Assert-Row ($_.Exception.Message -match $Pattern) "Unexpected rejection: $_" }
    Assert-Row $caught 'Expected comparison rejection'
}
function Write-Records([string]$Directory, $Records) {
    [IO.File]::WriteAllText((Join-Path $Directory 'stdout.txt'), (($Records | ForEach-Object { ConvertTo-Json -InputObject $_ -Compress -Depth 10 }) -join "`n") + "`n", [Text.UTF8Encoding]::new($false))
}
function Mutate-Records([string]$Directory, [scriptblock]$Operation) {
    $path = Join-Path $Directory 'stdout.txt'; $records = @([IO.File]::ReadAllLines($path) | ForEach-Object { ConvertFrom-Json -InputObject $_ -AsHashtable })
    & $Operation $records; Write-Records $Directory $records
}
function Write-Floats([string]$Path, [single[]]$Values) {
    $bytes = [byte[]]::new($Values.Length * 4)
    for ($i=0; $i -lt $Values.Length; $i++) { [BitConverter]::GetBytes($Values[$i]).CopyTo($bytes, $i*4) }
    [IO.File]::WriteAllBytes($Path, $bytes)
}
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('row-compare-' + [guid]::NewGuid().ToString('N')); [void][IO.Directory]::CreateDirectory($scratch)
try {
    foreach ($selected in $Case) {
        $root = Join-Path $scratch $selected; $serialPath = Join-Path $root 'serial'; $parallelPath = Join-Path $root 'parallel'
        $geometry = @{layers=4; width=2; convolution=2; recurrent=3; vocabulary=8}
        # Independent miniature: three recurrent blocks and one attention block.
        $files = @{'logits-0.f32le'=8; 'state-0/block-0-convolution.f32le'=2; 'state-0/block-0-recurrent.f32le'=3
            'state-0/block-1-convolution.f32le'=2; 'state-0/block-1-recurrent.f32le'=3; 'state-0/block-2-convolution.f32le'=2
            'state-0/block-2-recurrent.f32le'=3; 'state-0/block-3-key-0.f32le'=2; 'state-0/block-3-value-0.f32le'=2}
        foreach ($spec in @(@($serialPath,1,0,200), @($parallelPath,4,6,100))) {
            $directory=$spec[0]; [void][IO.Directory]::CreateDirectory((Join-Path $directory 'state-0'))
            Write-Records $directory @(@{event='weights-loaded'; model_sha256=$script:RowModel; weight_representation='PQ2_0'},
                @{event='rank-one-adapter-loaded'; adapter_sha256=$script:RowAdapter; model_sha256=$script:RowModel; target_pairs=129; scale_bits='3fa66666'},
                @{event='forward-step'; position=0; input_token=248045; backend_requested='auto'; logit_count=8; row_workers=$spec[1]
                    parallel_row_operations=$spec[2]; evaluation_milliseconds=$spec[3]; identity=$script:RowGraph; greedy_token=2; checkpoint_bytes=76; logits=(Join-Path $directory 'logits-0.f32le')})
            foreach ($relative in $files.Keys) { $values = [single[]]@(for ($i=0; $i -lt $files[$relative]; $i++) { @(1.0, [BitConverter]::Int32BitsToSingle([int]::MinValue), 2.0)[$i % 3] }); Write-Floats (Join-Path $directory $relative) $values }
        }
        $evaluate = { Invoke-RowComparison $serialPath $parallelPath $ToolPath $geometry }
        $recordsPath = Join-Path $parallelPath 'stdout.txt'; $original = [IO.File]::ReadAllText($recordsPath)
        switch ($selected) {
            complete { $r = & $evaluate; Assert-Row ($r.files -eq 9 -and $r.elements -eq 27 -and $r.observedEvaluationSpeedup -eq 2 -and -not $r.performanceGatePassed -and -not $r.defaultMayChange) 'Complete fixture scope changed' }
            extent { foreach ($directory in @($serialPath,$parallelPath)) { Write-Floats (Join-Path $directory 'state-0/block-1-recurrent.f32le') @(1) }; Expect-Rejection $evaluate 'extent'; foreach ($directory in @($serialPath,$parallelPath)) { [IO.File]::Delete((Join-Path $directory 'state-0/block-1-recurrent.f32le')) }; Expect-Rejection $evaluate 'inventory' }
            nonfinite { foreach ($directory in @($serialPath,$parallelPath)) { Write-Floats (Join-Path $directory 'state-0/block-3-key-0.f32le') @([single]::NaN,1) }; Expect-Rejection $evaluate 'nonfinite' }
            signed_zero { Write-Floats (Join-Path $parallelPath 'state-0/block-3-key-0.f32le') @(1,0); Expect-Rejection $evaluate 'bits differ'; Write-Floats (Join-Path $parallelPath 'extra.f32le') @(1); Expect-Rejection $evaluate 'inventory' }
            dispatch { Mutate-Records $parallelPath { param($r) $r[2].parallel_row_operations=0 }; Expect-Rejection $evaluate 'dispatch' }
            controls { foreach ($change in @(@(0,'model_sha256',('0'*64)),@(1,'scale_bits','00000000'),@(2,'input_token',7),@(2,'row_workers',2),@(2,'position',0.0))) { Mutate-Records $parallelPath {param($r) $r[$change[0]][$change[1]]=$change[2] }; Expect-Rejection $evaluate; [IO.File]::WriteAllText($recordsPath,$original) } }
            records {
                $lines = [IO.File]::ReadAllLines($recordsPath)
                foreach ($changed in @(@($lines[0],$lines[1]), @($lines[0],$lines[1],$lines[2],$lines[2]), @($lines[0],$lines[1],$lines[2],'{"event":"error"}'))) { [IO.File]::WriteAllLines($recordsPath,[string[]]$changed); Expect-Rejection $evaluate }
                [IO.File]::WriteAllText($recordsPath,(' '*65537)); Expect-Rejection $evaluate 'byte bound'
            }
            duration { foreach ($value in @(0,-1,$true,240001)) { Mutate-Records $parallelPath {param($r) $r[2].evaluation_milliseconds=$value }; Expect-Rejection $evaluate 'duration'; [IO.File]::WriteAllText($recordsPath,$original) }; foreach ($value in @('NaN','Infinity')) { [IO.File]::WriteAllText($recordsPath,($original -replace '"evaluation_milliseconds":100', ('"evaluation_milliseconds":'+$value))); Expect-Rejection $evaluate; [IO.File]::WriteAllText($recordsPath,$original) } }
            binding {
                $serialRecords = Join-Path $serialPath 'stdout.txt'; $serialOriginal = [IO.File]::ReadAllText($serialRecords)
                foreach ($change in @(@('identity','wrong-unbound-graph'),@('logits','unrelated.f32le'),@('greedy_token',999999),@('checkpoint_bytes',0),@('greedy_token',7))) {
                    foreach ($directory in @($serialPath,$parallelPath)) { Mutate-Records $directory {param($r) $r[2][$change[0]]=$change[1] } }; Expect-Rejection $evaluate
                    [IO.File]::WriteAllText($recordsPath,$original); [IO.File]::WriteAllText($serialRecords,$serialOriginal)
                }
            }
            inventory { $nested=Join-Path $parallelPath 'state-0/nested'; [void][IO.Directory]::CreateDirectory($nested); Expect-Rejection $evaluate 'nested'; [IO.Directory]::Delete($nested); for ($i=0; $i -lt 33; $i++) { [IO.File]::WriteAllText((Join-Path $parallelPath "extra-$i.txt"),'bounded fixture') }; Expect-Rejection $evaluate 'entry bound' }
        }
        Write-Output "PASS $selected"
    }
} finally { [IO.Directory]::Delete($scratch,$true) }
