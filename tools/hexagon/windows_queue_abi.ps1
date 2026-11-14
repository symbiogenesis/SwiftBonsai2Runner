#Requires -Version 7.6
# The independent native Windows ABI lane owns compiler selection and validation.
param([Parameter(Mandatory)][string]$Output,[Parameter(Mandatory)][string]$Rustc)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$tool = if ($env:BONSAI_TOOLS_PATH) { $env:BONSAI_TOOLS_PATH } else { (Get-Command bonsai-tools -CommandType Application -ErrorAction Stop).Source }
@{ operation = 'public-control'; request = @{ operation = 'windows-abi'; repositoryDirectory = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..')); outputDirectory = [IO.Path]::GetFullPath($Output); rustc = [IO.Path]::GetFullPath($Rustc) } } | ConvertTo-Json -Compress | & $tool hexagon-tools --request -
exit $LASTEXITCODE
