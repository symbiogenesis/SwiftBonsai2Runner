#Requires -Version 7.6
[CmdletBinding()]
param([Alias('data-directory')][string]$DataDirectory='',
    [Alias('evidence-directory')][string]$EvidenceDirectory='',[string]$ToolPath='')
. (Join-Path $PSScriptRoot 'public_pack_formats.ps1')
function Assert-PublicHosted { [void](Invoke-ReferencePolicy @{operation='public-hosted'}) }
function Get-PublicConfigSites([byte[]]$Raw) { return ,(Invoke-ReferencePolicy @{operation='public-config';rawHex=[Convert]::ToHexStringLower($Raw)}) }
function Save-PublicReplaySource([string]$Repository,[string]$Evidence,[string]$Revision) {
    # Compatibility fixtures construct the frozen schema-1 selection. Active native
    # production seals schema-2 source manifests including its Rust implementation.
    Invoke-ReferencePolicy @{operation='public-save-source';repository=$Repository;evidence=$Evidence;revision=$Revision;legacy=$true}
}
function Assert-PublicDirectories([string]$Data,[string]$Evidence,[string]$Temporary) { [void](Invoke-ReferencePolicy @{operation='public-directories';data=$Data;evidence=$Evidence;temporary=$Temporary}) }
function Copy-PublicTransfer([IO.Stream]$Source,[IO.Stream]$Output,$Pin) {
    $memory=[IO.MemoryStream]::new()
    try { $Source.CopyTo($memory);$raw=$memory.ToArray();[void](Invoke-ReferencePolicy @{operation='transfer-validate';rawHex=[Convert]::ToHexStringLower($raw);pin=$Pin});$Output.Write($raw) }
    finally {$memory.Dispose()}
}
function Invoke-PublicReference([string]$Data,[string]$Evidence,[string]$ToolPath='') {
    $result=Invoke-ReferencePolicy @{operation='public-reference';dataDirectory=$Data;evidenceDirectory=$Evidence;repository=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'));toolPath=(Get-ReferenceNumericTool $ToolPath)}
    return $result.all48PublicAComparisonPassed
}
if ($MyInvocation.InvocationName -ne '.') { if(-not (Invoke-PublicReference $DataDirectory $EvidenceDirectory $ToolPath)){exit 1} }
