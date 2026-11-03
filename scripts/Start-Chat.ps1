#requires -Version 5.1
<#
.SYNOPSIS
Launches native chat with the Rust inference engine development default.
.DESCRIPTION
Uses a prebuilt bonsai-tools executable. Rust owns startup and application policy.
.EXAMPLE
./scripts/Start-Chat.ps1
.EXAMPLE
./scripts/Start-Chat.ps1 -PublisherEngine
.EXAMPLE
./scripts/Start-Chat.ps1 -GraphicsBackend dx12
#>
[CmdletBinding()]
[OutputType([string])]
param(
    [switch]$NativeEngine,
    [switch]$PublisherEngine,
    [ValidateSet(1, 4)]
    [int]$NativeRowWorkers = 4,
    [switch]$UseRepackedPtq1,
    [switch]$UseOriginalPq2,
    [ValidateSet('dx12', 'vulkan', 'gl')]
    [string]$GraphicsBackend,
    [string]$DataDirectory,
    [string]$ToolPath = $env:BONSAI_TOOLS_PATH
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false

if ($NativeEngine -and $PublisherEngine) {
    throw 'NativeEngine and PublisherEngine are mutually exclusive.'
}
if ($PublisherEngine -and $PSBoundParameters.ContainsKey('NativeRowWorkers') -and $NativeRowWorkers -ne 1) {
    throw 'Four native row workers require the Rust engine.'
}

if (-not $ToolPath -and $env:LOCALAPPDATA) {
    $cachedTool = Join-Path $env:LOCALAPPDATA 'SwiftBonsai2/build/maintenance/release/bonsai-tools.exe'
    if (Test-Path -LiteralPath $cachedTool -PathType Leaf) { $ToolPath = $cachedTool }
}
if (-not $ToolPath) {
    $command = Get-Command bonsai-tools.exe -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($command) { $ToolPath = $command.Source }
}
if (-not $ToolPath -or -not (Test-Path -LiteralPath $ToolPath -PathType Leaf)) {
    $message = 'Select a prebuilt bonsai-tools executable with -ToolPath or BONSAI_TOOLS_PATH.'
    if ($ToolPath) {
        $message += ' Correct the selected path, or remove the override to use default discovery.'
    }
    if ($env:LOCALAPPDATA) {
        $manifest = (Join-Path (Split-Path $PSScriptRoot -Parent) 'Cargo.toml').Replace("'", "''")
        $target = (Join-Path $env:LOCALAPPDATA 'SwiftBonsai2/build/maintenance').Replace("'", "''")
        $message += "`nTo prepare the default tool, run this command from the repository root:`n" +
            "cargo build --locked --release --manifest-path '$manifest' --package bonsai-tools --bin bonsai-tools --target-dir '$target'"
    }
    $message += "`nSee docs/development-guide.md#native-chat-app for setup instructions."
    throw $message
}
$ToolPath = (Resolve-Path -LiteralPath $ToolPath).ProviderPath

$launchArguments = @('chat', '--repository-root', (Split-Path $PSScriptRoot -Parent))
if ($DataDirectory) { $launchArguments += @('--data-directory', $DataDirectory) }
if ($PublisherEngine) {
    $launchArguments += @('--publisher-engine', '--native-row-workers', '1')
} else {
    $launchArguments += @('--native-engine', '--native-row-workers', [string]$NativeRowWorkers)
}
if ($UseRepackedPtq1) { $launchArguments += '--use-repacked-ptq1' }
if ($UseOriginalPq2) { $launchArguments += '--use-original-pq2' }
if ($GraphicsBackend) { $launchArguments += @('--graphics-backend', $GraphicsBackend.ToLowerInvariant()) }
& $ToolPath @launchArguments
exit $LASTEXITCODE
