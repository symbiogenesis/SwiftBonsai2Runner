#requires -Version 5.1
<#
.SYNOPSIS
Launches native chat with an optional Rust inference engine.
.DESCRIPTION
Uses a prebuilt bonsai-tools executable. Rust owns startup and application policy.
.EXAMPLE
./scripts/Start-Chat.ps1 -NativeEngine
#>
[CmdletBinding()]
[OutputType([string])]
param(
    [switch]$NativeEngine,
    [switch]$UseRepackedPtq1,
    [switch]$UseOriginalPq2,
    [string]$DataDirectory,
    [string]$ToolPath = $env:BONSAI_TOOLS_PATH
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false

if (-not $ToolPath -and $env:LOCALAPPDATA) {
    $cachedTool = Join-Path $env:LOCALAPPDATA 'SwiftBonsai2/build/maintenance/release/bonsai-tools.exe'
    if (Test-Path -LiteralPath $cachedTool -PathType Leaf) { $ToolPath = $cachedTool }
}
if (-not $ToolPath) {
    $command = Get-Command bonsai-tools.exe -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($command) { $ToolPath = $command.Source }
}
if (-not $ToolPath -or -not (Test-Path -LiteralPath $ToolPath -PathType Leaf)) {
    throw 'Select a prebuilt bonsai-tools executable with -ToolPath or BONSAI_TOOLS_PATH. See docs/development-guide.md#native-chat-app for build instructions.'
}
$ToolPath = (Resolve-Path -LiteralPath $ToolPath).ProviderPath

$launchArguments = @('chat', '--repository-root', (Split-Path $PSScriptRoot -Parent))
if ($DataDirectory) { $launchArguments += @('--data-directory', $DataDirectory) }
if ($NativeEngine) { $launchArguments += '--native-engine' }
if ($UseRepackedPtq1) { $launchArguments += '--use-repacked-ptq1' }
if ($UseOriginalPq2) { $launchArguments += '--use-original-pq2' }
& $ToolPath @launchArguments
exit $LASTEXITCODE
