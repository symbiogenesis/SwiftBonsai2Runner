#requires -Version 7.6
<# Compare fixed serial/four-worker PQ2 state. Timings are observations, never promotion. #>
[CmdletBinding()]
param([string]$Serial, [string]$Parallel, [string]$Output, [string]$ToolPath = $env:BONSAI_REFERENCE_NUMERIC_TOOL)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$script:RowModel = ([IO.File]::ReadAllText((Join-Path $PSScriptRoot '../../config/swift-assessment.lock.json')) | ConvertFrom-Json -AsHashtable).model.sha256
$script:RowAdapter = 'f1669534803d340a496015f5c45125f3437b4d13ec764f40e34488ce83967f42'
$script:RowGraph = $script:RowModel + ':qwen35-text-v1:nonflash-f16:kv-f16:gdn-f32:prism-arm64-fused-v1:rank-one-lora-v1:' + $script:RowAdapter + ':3fa66666'
function Assert-Row($Condition, [string]$Message) { if (-not $Condition) { throw $Message } }
function Test-RowInteger($Value) { $Value -is [long] -or $Value -is [int] -or $Value -is [System.Numerics.BigInteger] }
function Read-RowBytes([string]$Path, [long]$Limit) {
    $metadata = Get-Item -LiteralPath $Path -Force
    Assert-Row (-not $metadata.PSIsContainer -and -not ($metadata.Attributes -band [IO.FileAttributes]::ReparsePoint)) 'reparse capture input'
    Assert-Row ($metadata.Length -gt 0 -and $metadata.Length -le $Limit) 'capture input exceeds byte bound'
    $file = [IO.File]::Open($Path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
    try {
        Assert-Row ($file.Length -gt 0 -and $file.Length -le $Limit) 'capture input grew beyond byte bound'
        $data = [byte[]]::new($file.Length); $file.ReadExactly($data)
        Assert-Row ($file.ReadByte() -eq -1) 'capture input grew beyond byte bound'
        return ,$data
    } finally { $file.Dispose() }
}
function Assert-RowDirectory([string]$Path) {
    $metadata = Get-Item -LiteralPath $Path -Force
    Assert-Row ($metadata.PSIsContainer -and -not ($metadata.Attributes -band [IO.FileAttributes]::ReparsePoint)) 'capture directory or ancestor is a reparse point'
}
function Get-RowInventory([string]$Directory) {
    Assert-Row ([IO.Path]::IsPathFullyQualified($Directory)) 'capture directory must be a bounded absolute path'
    $ancestor = [IO.DirectoryInfo]::new($Directory); $depth = 0
    while ($null -ne $ancestor) { Assert-RowDirectory $ancestor.FullName; $depth++; Assert-Row ($depth -le 65) 'capture ancestry exceeds bound'; $ancestor = $ancestor.Parent }
    $state = Join-Path $Directory 'state-0'; Assert-RowDirectory $state
    $payloads = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    foreach ($spec in @(@($Directory,32), @($state,128))) {
        $count = 0
        foreach ($path in [IO.Directory]::EnumerateFileSystemEntries($spec[0])) {
            $count++; Assert-Row ($count -le $spec[1]) 'capture directory entry bound exceeded'
            $item = Get-Item -LiteralPath $path -Force
            Assert-Row (-not ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) 'reparse capture entry'
            if ($item.PSIsContainer) { Assert-Row ($spec[0] -ceq $Directory -and $path -ceq $state) 'unexpected nested capture directory' }
            else {
                if ($spec[0] -ceq $state) { Assert-Row ($item.Extension -ceq '.f32le') 'unexpected state component' }
                if ($item.Extension -ceq '.f32le') { [void]$payloads.Add([IO.Path]::GetRelativePath($Directory, $path).Replace('\','/')) }
            }
        }
    }
    return ,$payloads
}
function Get-RowControls([string]$Directory, [int]$Workers, [hashtable]$Geometry) {
    $raw = [Text.UTF8Encoding]::new($false, $true).GetString((Read-RowBytes (Join-Path $Directory 'stdout.txt') 65536))
    $records = @($raw -split '\r?\n' | Where-Object { $_.Length } | ForEach-Object { ConvertFrom-Json -InputObject $_ -AsHashtable })
    $weights = @($records | Where-Object { $_.event -ceq 'weights-loaded' }); $adapters = @($records | Where-Object { $_.event -ceq 'rank-one-adapter-loaded' }); $steps = @($records | Where-Object { $_.event -ceq 'forward-step' })
    Assert-Row (@($records | Where-Object { $_.event -ceq 'error' }).Count -eq 0) 'checker reported an error'
    Assert-Row ($weights.Count -eq 1 -and $weights[0].model_sha256 -ceq $script:RowModel -and $weights[0].weight_representation -ceq 'PQ2_0') 'PQ2 model control changed'
    Assert-Row ($adapters.Count -eq 1 -and $adapters[0].adapter_sha256 -ceq $script:RowAdapter -and $adapters[0].model_sha256 -ceq $script:RowModel -and (Test-RowInteger $adapters[0].target_pairs) -and $adapters[0].target_pairs -eq 129 -and $adapters[0].scale_bits -ceq '3fa66666') 'adapter control changed'
    Assert-Row ($steps.Count -eq 1) 'expected exactly one fixed forward step'; $step = $steps[0]
    foreach ($key in @('position','input_token','logit_count')) { Assert-Row (Test-RowInteger $step[$key]) 'forward controls changed' }
    Assert-Row ($step.position -eq 0 -and $step.input_token -eq 248045 -and $step.backend_requested -ceq 'auto' -and $step.logit_count -eq $Geometry.vocabulary) 'forward controls changed'
    Assert-Row ($step.identity -ceq $script:RowGraph) 'graph identity changed'
    Assert-Row ($step.logits -is [string] -and [IO.Path]::IsPathFullyQualified($step.logits) -and $step.logits -ceq (Join-Path $Directory 'logits-0.f32le')) 'emitted logits path changed'
    Assert-Row ((Test-RowInteger $step.checkpoint_bytes) -and $step.checkpoint_bytes -gt 0 -and $step.checkpoint_bytes -le 12348030976) 'invalid checkpoint extent'
    Assert-Row ((Test-RowInteger $step.greedy_token) -and $step.greedy_token -ge 0 -and $step.greedy_token -lt $Geometry.vocabulary) 'invalid greedy token'
    Assert-Row ((Test-RowInteger $step.row_workers) -and $step.row_workers -eq $Workers) 'row worker control changed'
    $operations = $step.parallel_row_operations
    Assert-Row ((Test-RowInteger $operations) -and $operations -ge 0) 'invalid parallel operation count'
    Assert-Row (($Workers -eq 1 -and $operations -eq 0) -or ($Workers -eq 4 -and $operations -gt 0)) 'requested row dispatch was not observed'
    $milliseconds = $step.evaluation_milliseconds
    Assert-Row (($milliseconds -is [double] -or (Test-RowInteger $milliseconds)) -and [double]::IsFinite($milliseconds) -and $milliseconds -gt 0 -and $milliseconds -le 240000) 'invalid fixed-step evaluation duration'
    $step
}
function Invoke-RowComparison([string]$SerialDirectory, [string]$ParallelDirectory, [string]$NumericTool, [hashtable]$Geometry = @{layers=64; width=1024; convolution=30720; recurrent=786432; vocabulary=248320}) {
    Assert-Row ([IO.Path]::IsPathFullyQualified($NumericTool) -and [IO.File]::Exists($NumericTool)) 'Supply absolute BONSAI_REFERENCE_NUMERIC_TOOL or -ToolPath from the checked bundle'
    $expected = [ordered]@{ 'logits-0.f32le' = $Geometry.vocabulary }
    for ($block = 0; $block -lt $Geometry.layers; $block++) {
        $components = if ($block % 4 -eq 3) { @{ 'key-0'=$Geometry.width; 'value-0'=$Geometry.width } } else { @{ convolution=$Geometry.convolution; recurrent=$Geometry.recurrent } }
        foreach ($label in ($components.Keys | Sort-Object)) { $expected["state-0/block-$block-$label.f32le"] = $components[$label] }
    }
    foreach ($directory in @($SerialDirectory,$ParallelDirectory)) { $inventory = Get-RowInventory $directory; Assert-Row ($inventory.SetEquals([string[]]@($expected.Keys))) 'complete logical state inventory changed' }
    $first = Get-RowControls $SerialDirectory 1 $Geometry; $second = Get-RowControls $ParallelDirectory 4 $Geometry
    Assert-Row ($first.identity -ceq $second.identity) 'graph identity changed'
    Assert-Row ($first.greedy_token -eq $second.greedy_token) 'greedy token changed'
    Assert-Row ($first.checkpoint_bytes -eq $second.checkpoint_bytes) 'checkpoint extent changed'
    $files = [Collections.Generic.List[object]]::new(); $total = 0L
    foreach ($relative in $expected.Keys) {
        $elements = $expected[$relative]; $paths = @((Join-Path $SerialDirectory $relative), (Join-Path $ParallelDirectory $relative))
        foreach ($path in $paths) {
            $metadata = Get-Item -LiteralPath $path -Force
            Assert-Row (-not $metadata.PSIsContainer -and -not ($metadata.Attributes -band [IO.FileAttributes]::ReparsePoint) -and $metadata.Length -eq $elements * 4L) 'component extent changed'
        }
        $result = & $NumericTool row $paths[0] $paths[1] ([string]$elements)
        Assert-Row ($LASTEXITCODE -eq 0) 'nonfinite compared value or component extent changed'
        $compared = ($result -join "`n") | ConvertFrom-Json -AsHashtable
        Assert-Row ($compared.elements -eq $elements -and $compared.byteExact -ceq $true) "serial/parallel arithmetic bits differ: $relative"
        if ($relative -ceq 'logits-0.f32le') { Assert-Row ($first.greedy_token -eq $compared.expectedGreedyToken -and $second.greedy_token -eq $compared.actualGreedyToken) 'greedy token does not match compared logits' }
        $files.Add(@{path=$relative; elements=$elements; sha256=(Get-FileHash -LiteralPath $paths[0] -Algorithm SHA256).Hash.ToLowerInvariant()}); $total += $elements
    }
    @{status='fixed-step-serial-parallel-byte-exact'; modelSha256=$script:RowModel; adapterSha256=$script:RowAdapter; adapterScale=1.3
        suppliedTokens=@(248045); rowWorkers=@(1,4); backend='auto'; files=$files.Count; elements=$total
        serialEvaluationMilliseconds=$first.evaluation_milliseconds; parallelEvaluationMilliseconds=$second.evaluation_milliseconds
        observedEvaluationSpeedup=$first.evaluation_milliseconds / $second.evaluation_milliseconds; parallelRowOperations=$second.parallel_row_operations
        timingScope='One fixed forward evaluation per condition; excludes loading and capture I/O. No application TPS, statistical benefit, prefill or performance qualification.'
        performanceGatePassed=$false; defaultMayChange=$false; comparisons=$files.ToArray()}
}
if ($MyInvocation.InvocationName -ne '.') {
    if (-not $Serial -or -not $Parallel -or -not $Output) { throw 'Supply -Serial, -Parallel and -Output.' }
    $report = Invoke-RowComparison $Serial $Parallel $ToolPath
    $file = [IO.File]::Open($Output, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
    try { $bytes = [Text.UTF8Encoding]::new($false).GetBytes((ConvertTo-Json -InputObject $report -Depth 20) + "`n"); $file.Write($bytes) } finally { $file.Dispose() }
    $report.Remove('comparisons'); ConvertTo-Json -InputObject $report -Compress
}
