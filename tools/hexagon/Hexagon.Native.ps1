#Requires -Version 7.6
# Native Hexagon command composition; validation and policy live in Rust.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
function Invoke-HexagonAudit([string]$Operation,[hashtable]$Parameters = @{}) {
    $tool = $env:BONSAI_TOOLS_PATH
    if (-not $tool) { $tool = (Get-Command bonsai-tools -CommandType Application -ErrorAction Stop).Source }
    $request = @{ operation = $Operation; repositoryDirectory = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..')) }
    foreach ($key in $Parameters.Keys) { $request[$key] = $Parameters[$key] }
    @{ operation = 'audit'; request = $request } | ConvertTo-Json -Depth 64 -Compress | & $tool hexagon-policy --request -
    if ($LASTEXITCODE -ne 0) { throw "Native Hexagon $Operation failed (exit $LASTEXITCODE)." }
}
