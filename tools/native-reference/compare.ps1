#Requires -Version 7.6
[CmdletBinding()]
param([string]$Native='', [string]$Rust='', [string]$Backend='',
    [Alias('row-workers')][int]$RowWorkers=1,[Alias('adapter-condition')][string]$AdapterCondition='standard',
    [string]$Output='', [string]$ToolPath='')
. (Join-Path $PSScriptRoot 'public_pack_formats.ps1')
$script:NativeModel='e658f0f0736a03a4549535a439f9b07254ebf8ab88ae27cbe6e52d098ca5d1f5'
$script:NativeRevision='73bfe6cc485554822f80c473a3c0c4f25e80f41b'
$script:NativeAdapter='f1669534803d340a496015f5c45125f3437b4d13ec764f40e34488ce83967f42'
function Get-NativeReferenceComponents([string]$Path,[int]$Position,
    [int[]]$Attention=@(3..63|Where-Object{$_%4 -eq 3}),[int[]]$Recurrent=@(0..63|Where-Object{$_%4 -ne 3}),
    [int]$Layers=64,[int]$Width=1024,[int]$Convolution=30720,[int]$Matrix=786432) {
    return ,(Invoke-ReferencePolicy @{operation='native-components';path=$Path;position=$Position;attention=$Attention;recurrent=$Recurrent;layers=$Layers;width=$Width;convolution=$Convolution;matrix=$Matrix})
}
function Assert-NativeAdapterCondition($Capture,[string]$Condition) { [void](Invoke-ReferencePolicy @{operation='adapter-condition';capture=$Capture;condition=$Condition}) }
function Assert-NativeRustControls([string]$Rust,$Capture,[string]$Backend,[int]$RowWorkers) { return ,@((Invoke-ReferencePolicy @{operation='native-controls';rust=$Rust;capture=$Capture;backend=$Backend;rowWorkers=$RowWorkers})) }
function Invoke-NativeReferenceComparison([string]$Native,[string]$Rust,[string]$Backend,[int]$RowWorkers=1,[string]$AdapterCondition='standard',[string]$ToolPath='') {
    Invoke-ReferencePolicy @{operation='native-compare';native=$Native;rust=$Rust;backend=$Backend;rowWorkers=$RowWorkers;adapterCondition=$AdapterCondition}
}
if ($MyInvocation.InvocationName -ne '.') {
    $report=Invoke-ReferencePolicy @{operation='native-compare';native=$Native;rust=$Rust;backend=$Backend;rowWorkers=$RowWorkers;adapterCondition=$AdapterCondition;output=$Output}
    if($report.status -ceq 'failed'){exit 1};$report | ConvertTo-Json -Depth 64
}
