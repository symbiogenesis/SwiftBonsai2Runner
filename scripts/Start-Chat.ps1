#Requires -Version 7.6
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
    [int]$NativeRowWorkers,
    [ValidateSet('disabled', 'latest')]
    [string]$PromptReuse,
    [ValidateSet('f16', 'rsq4', 'rsq4k3v')]
    [string]$KvCache,
    [ValidateSet('auto', 'cpu')]
    [string]$Placement,
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

if (-not $ToolPath -and $env:LOCALAPPDATA) {
    foreach ($relative in @(
        'SwiftBonsai2/build/maintenance/aarch64-pc-windows-msvc/release/bonsai-tools.exe',
        'SwiftBonsai2/build/maintenance/release/bonsai-tools.exe'
    )) {
        $cachedTool = Join-Path $env:LOCALAPPDATA $relative
        if (Test-Path -LiteralPath $cachedTool -PathType Leaf) {
            $ToolPath = $cachedTool
            break
        }
    }
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
            "cargo --config 'target.aarch64-pc-windows-msvc.rustflags=[`"-C`",`"target-feature=+crt-static`"]' build --locked --release --target aarch64-pc-windows-msvc --manifest-path '$manifest' --package bonsai-tools --bin bonsai-tools --target-dir '$target'"
    }
    $message += "`nSee docs/development-guide.md#native-chat-app for setup instructions."
    throw $message
}
$resolvedTool = Resolve-Path -LiteralPath $ToolPath
if ($resolvedTool.Provider.Name -cne 'FileSystem' -or [IO.Path]::GetExtension($resolvedTool.ProviderPath) -ine '.exe') {
    throw 'The native launcher must be a filesystem executable (.exe).'
}
$ToolPath = $resolvedTool.ProviderPath

$launchArguments = @('chat', '--repository-root', (Split-Path $PSScriptRoot -Parent))
if ($DataDirectory) { $launchArguments += @('--data-directory', $DataDirectory) }
if ($NativeEngine) { $launchArguments += '--native-engine' }
if ($PublisherEngine) { $launchArguments += '--publisher-engine' }
if ($PSBoundParameters.ContainsKey('NativeRowWorkers')) {
    $launchArguments += @('--native-row-workers', [string]$NativeRowWorkers)
}
if ($UseRepackedPtq1) { $launchArguments += '--use-repacked-ptq1' }
if ($UseOriginalPq2) { $launchArguments += '--use-original-pq2' }
if ($GraphicsBackend) { $launchArguments += @('--graphics-backend', $GraphicsBackend.ToLowerInvariant()) }
if ($PromptReuse) { $launchArguments += @('--prompt-reuse', $PromptReuse.ToLowerInvariant()) }
if ($KvCache) { $launchArguments += @('--kv-cache', $KvCache.ToLowerInvariant()) }
if ($Placement) { $launchArguments += @('--placement', $Placement.ToLowerInvariant()) }
& $ToolPath @launchArguments
exit $LASTEXITCODE
