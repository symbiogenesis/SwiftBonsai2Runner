#Requires -Version 7.6
[CmdletBinding()]
param([string]$Serial,[string]$Parallel,[string]$Output,[string]$ToolPath=$env:BONSAI_REFERENCE_NUMERIC_TOOL)
. (Join-Path $PSScriptRoot '../native-reference/public_pack_formats.ps1')
$script:RowModel=([IO.File]::ReadAllText((Join-Path $PSScriptRoot '../../config/swift-assessment.lock.json'))|ConvertFrom-Json -AsHashtable).model.sha256
$script:RowAdapter='f1669534803d340a496015f5c45125f3437b4d13ec764f40e34488ce83967f42'
$script:RowGraph=$script:RowModel+':qwen35-text-v1:nonflash-f16:kv-f16:gdn-f32:prism-arm64-fused-v1:rank-one-lora-v1:'+$script:RowAdapter+':3fa66666'
function Assert-Row($Condition,[string]$Message) { Assert-Reference $Condition $Message }
function Get-RowControls([string]$Directory,[int]$Workers,[hashtable]$Geometry) { Invoke-ReferencePolicy @{operation='row-controls';directory=$Directory;workers=$Workers;geometry=$Geometry} }
function Invoke-RowComparison([string]$SerialDirectory,[string]$ParallelDirectory,[string]$NumericTool,[hashtable]$Geometry=@{layers=64;width=1024;convolution=30720;recurrent=786432;vocabulary=248320}) {
    Invoke-ReferencePolicy @{operation='row-compare';serial=$SerialDirectory;parallel=$ParallelDirectory;geometry=$Geometry}
}
if ($MyInvocation.InvocationName -ne '.') {
    $report=Invoke-ReferencePolicy @{operation='row-compare';serial=$Serial;parallel=$Parallel;output=$Output};$report.Remove('comparisons');$report|ConvertTo-Json -Compress
}
