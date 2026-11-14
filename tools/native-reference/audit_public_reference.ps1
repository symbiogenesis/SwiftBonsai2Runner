#Requires -Version 7.6
[CmdletBinding()]
param([Alias('evidence-directory')][string]$EvidenceDirectory='',[string]$Adapter='',[string]$Direction='',
    [Alias('expected-source-revision')][string]$ExpectedSourceRevision='',
    [Alias('expected-run-id')][string]$ExpectedRunId='',
    [Alias('expected-run-attempt')][string]$ExpectedRunAttempt='')
. (Join-Path $PSScriptRoot 'public_pack_formats.ps1')
function Get-AuditMetrics([single[]]$Actual,[single[]]$Reference,[double]$Absolute=0.00001,[double]$Relative=0.00001) {
    $actualRaw=[byte[]]::new($Actual.Length*4);[Buffer]::BlockCopy($Actual,0,$actualRaw,0,$actualRaw.Length)
    $referenceRaw=[byte[]]::new($Reference.Length*4);[Buffer]::BlockCopy($Reference,0,$referenceRaw,0,$referenceRaw.Length)
    Invoke-ReferencePolicy @{operation='audit-metrics';actualHex=[Convert]::ToHexStringLower($actualRaw);referenceHex=[Convert]::ToHexStringLower($referenceRaw);count=$Actual.Length;absolute=$Absolute;relative=$Relative}
}
function Read-AuditAdapter([byte[]]$Raw) { Invoke-ReferenceAdapter $Raw 'audit-adapter' }
function Get-AuditDescriptors([string]$Evidence,$Report,$Pins) { Invoke-ReferencePolicy @{operation='audit-descriptors';evidence=$Evidence;report=$Report;pins=$Pins} }
function Invoke-PublicReferenceReplay([string]$Evidence,[string]$Adapter,[string]$Direction,[string]$Revision,[string]$RunId,[string]$RunAttempt,[string]$RepositoryRoot='') {
    if(-not $RepositoryRoot){$RepositoryRoot=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))}
    Invoke-ReferencePolicy @{operation='public-replay';evidenceDirectory=$Evidence;adapter=$Adapter;direction=$Direction;expectedSourceRevision=$Revision;expectedRunId=$RunId;expectedRunAttempt=$RunAttempt;repository=$RepositoryRoot}
}
if ($MyInvocation.InvocationName -ne '.') { Invoke-PublicReferenceReplay $EvidenceDirectory $Adapter $Direction $ExpectedSourceRevision $ExpectedRunId $ExpectedRunAttempt | ConvertTo-Json -Depth 64 }
