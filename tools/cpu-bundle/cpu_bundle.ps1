#Requires -Version 7.6
# Source-bound forwarding entry. Schema-2 retains these exact wrapper bytes for replay.
[CmdletBinding()]
param([ValidateSet('prepare','evaluate')][string]$Operation,
    [string]$BaselineRoot, [string]$ToolingRoot, [ValidateSet('0.1.25')][string]$CandidateRegistryVersion,
    [string]$RunnerRevision, [string]$Output, [string]$Preparation, [string]$Owner,
    [string]$ToolsPath = $env:BONSAI_TOOLS_PATH)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
$retained = if ($Operation -eq 'evaluate' -and $Preparation) { Join-Path ([IO.Path]::GetDirectoryName($Preparation)) 'native-evaluator.exe' }
if ($retained -and [IO.File]::Exists($retained)) { $ToolsPath = $retained }
if (-not $ToolsPath) { $ToolsPath = (Get-Command bonsai-tools -CommandType Application -ErrorAction Stop).Source }
$request = switch ($Operation) {
    prepare { @{ operation = 'bundle-prepare'; baselineRoot = $BaselineRoot; toolingRoot = $ToolingRoot
        candidateRegistryVersion = $CandidateRegistryVersion; runnerRevision = $RunnerRevision
        output = $Output; runnerRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..')) } }
    evaluate { @{ operation = 'bundle-evaluate'; preparationPath = $Preparation; ownerPath = $Owner } }
    default { throw 'Supply -Operation prepare or evaluate.' }
}
& $ToolsPath cpu-evaluation --json (ConvertTo-Json -InputObject $request -Depth 5 -Compress)
exit $LASTEXITCODE
