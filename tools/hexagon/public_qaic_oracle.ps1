#Requires -Version 7.6
# Hosted public generation stays separate from SDK inventory and Windows ABI.
param([Parameter(Mandatory)][string]$Source,[Parameter(Mandatory)][string]$Pin,[Parameter(Mandatory)][string]$Output,[Parameter(Mandatory)][string]$Rustc)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$tool = if ($env:BONSAI_TOOLS_PATH) { $env:BONSAI_TOOLS_PATH } else { (Get-Command bonsai-tools -CommandType Application -ErrorAction Stop).Source }
@{ operation = 'public-control'; request = @{ operation = 'public-qaic'; repositoryDirectory = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..')); sourceDirectory = [IO.Path]::GetFullPath($Source); pinPath = [IO.Path]::GetFullPath($Pin); outputDirectory = [IO.Path]::GetFullPath($Output); rustc = [IO.Path]::GetFullPath($Rustc) } } | ConvertTo-Json -Compress | & $tool hexagon-tools --request -
exit $LASTEXITCODE
