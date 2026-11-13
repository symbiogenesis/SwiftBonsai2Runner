#Requires -Version 7.6
<#
.SYNOPSIS
Builds all five native application executables and bundles them into one .exe.
.DESCRIPTION
Uses the pinned Rust and native speech build paths, collects dependency notices,
and packages the result without launching chat or downloading model weights.
Reuses local caches; -TargetDirectory or CARGO_TARGET_DIR selects a CI cache root.
Use -PackageDirectory and -ManifestSha256 to bundle a reviewed prebuilt package.
.EXAMPLE
./scripts/Build-WindowsExecutable.ps1
.EXAMPLE
./scripts/Build-WindowsExecutable.ps1 -OutputPath C:\Bonsai\SwiftBonsai2Runner.exe
#>
[CmdletBinding(DefaultParameterSetName = 'Build')]
param(
    [Parameter(ParameterSetName = 'Build')]
    [string]$RepositoryRoot = (Split-Path $PSScriptRoot -Parent),
    [Parameter(ParameterSetName = 'Build')]
    [string]$TargetDirectory = $env:CARGO_TARGET_DIR,
    [string]$DataDirectory = (Join-Path $env:LOCALAPPDATA 'SwiftBonsai2'),
    [string]$OutputPath,
    [Parameter(Mandatory, ParameterSetName = 'Package')][string]$PackageDirectory,
    [Parameter(Mandatory, ParameterSetName = 'Package')]
    [ValidatePattern('^[a-f0-9]{64}$')][string]$ManifestSha256
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
. (Join-Path $PSScriptRoot 'release/Release-Common.ps1')
if (-not $IsWindows -or [Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne 'Arm64') {
    throw 'Application builds require Windows ARM64.'
}
$DataDirectory = Resolve-ReleasePath $DataDirectory -AllowMissing
if (-not $OutputPath) { $OutputPath = Join-Path $DataDirectory 'build/distribution/SwiftBonsai2Runner.exe' }
$OutputPath = Resolve-ReleasePath $OutputPath -Kind File -AllowMissing
if ([IO.Path]::GetExtension($OutputPath) -ine '.exe') { throw 'Bundle output must use the .exe extension.' }
if (Test-Path -LiteralPath $OutputPath) { throw 'Output already exists. Choose a new path or remove the previous local build explicitly.' }
[void][IO.Directory]::CreateDirectory((Split-Path -Parent $OutputPath))
if ($PSCmdlet.ParameterSetName -ceq 'Package') {
    & (Join-Path $PSScriptRoot 'release/Build-WindowsExecutable.ps1') -PackageDirectory $PackageDirectory -ManifestSha256 $ManifestSha256 -OutputPath $OutputPath
    return
}
$RepositoryRoot = Resolve-ReleasePath $RepositoryRoot
if ($TargetDirectory) { $TargetDirectory = Resolve-ReleasePath $TargetDirectory -AllowMissing }
$manifestPath = Resolve-ReleasePath (Join-Path $RepositoryRoot 'Cargo.toml') -Kind File
$toolchain = Get-Content -LiteralPath (Join-Path $RepositoryRoot 'rust-toolchain.toml') -Raw
$channel = [regex]::Match($toolchain, '(?m)^channel\s*=\s*"(nightly-[0-9]{4}-[0-9]{2}-[0-9]{2})"\s*$').Groups[1].Value
if (-not $channel) { throw 'The repository must pin a dated Rust nightly.' }
$channel += '-aarch64-pc-windows-msvc'
$manifest = Get-Content -LiteralPath $manifestPath -Raw
$workspace = [regex]::Match($manifest, '(?ms)^\[workspace\.package\]\s*\r?\n(.*?)(?=^\[|\z)').Groups[1].Value
$version = [regex]::Match($workspace, '(?m)^version\s*=\s*"([0-9]+\.[0-9]+\.[0-9]+)"\s*$').Groups[1].Value
if (-not $version) { throw 'The workspace version is missing.' }
$revision = (Invoke-ReleaseCommand 'git' @('-C', $RepositoryRoot, 'rev-parse', 'HEAD')).Trim()
if ($revision -cnotmatch '^[0-9a-f]{40}$') { throw 'A local Git source identity is required.' }
$scratch = Join-Path (Resolve-ReleasePath ([IO.Path]::GetTempPath())) ('SwiftBonsai-Executable-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($scratch)
try {
    $target = if ($TargetDirectory) { $TargetDirectory } else { Join-Path $DataDirectory 'build/maintenance' }
    $arguments = @("+$channel", 'build', '--locked', '--release', '--manifest-path', $manifestPath,
        '--package', 'bonsai-tools', '--bin', 'bonsai-tools', '--bin', 'bonsai-launcher', '--target-dir', $target)
    $binaryDirectory = Join-Path $target 'release'
    if ($TargetDirectory) {
        $arguments += @('--target', 'aarch64-pc-windows-msvc')
        $binaryDirectory = Join-Path $target 'aarch64-pc-windows-msvc/release'
    }
    Write-Host 'Building the console maintenance tool and windowless launcher with the repository compiler pin.'
    $null = Invoke-ReleaseCommand 'cargo' $arguments -TimeoutSeconds 600 -Environment @{
        CARGO_BUILD_FINGERPRINT = 'content'; CARGO_UNSTABLE_CHECKSUM_FRESHNESS = 'true'
        CARGO_ENCODED_RUSTFLAGS = ('-C' + [char]0x1f + 'target-feature=+crt-static')
    }
    $tool = Resolve-ReleasePath (Join-Path $binaryDirectory 'bonsai-tools.exe') -Kind File
    $launcher = Resolve-ReleasePath (Join-Path $binaryDirectory 'bonsai-launcher.exe') -Kind File
    $chatRequest = @{
        operation = 'chat'; repositoryRoot = $RepositoryRoot; workingDirectory = $scratch
        dataDirectory = $DataDirectory; packages = @('bonsai-chat', 'bonsai-broker')
        targetDirectory = $(if ($TargetDirectory) { $TargetDirectory } else { $null })
    } | ConvertTo-Json -Compress
    Write-Host 'Building or verifying the native window and broker.'
    $chat = Invoke-ReleaseCommand $tool @('source-build', '--json', $chatRequest) -TimeoutSeconds 600 | ConvertFrom-Json
    $speechRequest = @{
        operation = 'build'; repositoryRoot = $RepositoryRoot; workingDirectory = $scratch
        dataDirectory = $DataDirectory; model = 'All'
        targetDirectory = $(if ($TargetDirectory) { $TargetDirectory } else { $null })
    } | ConvertTo-Json -Compress
    Write-Host 'Building native ASR and Rust TTS; cold ASR compiler setup may take several minutes.'
    $speech = Invoke-ReleaseCommand $tool @('speech-build', '--json', $speechRequest) -TimeoutSeconds 600 | ConvertFrom-Json
    $notices = Join-Path $scratch 'THIRD-PARTY-NOTICES.txt'
    Write-Host 'Collecting locked dependency notices and Rust speech port attribution.'
    & (Join-Path $RepositoryRoot '.github/release/Collect-Notices.ps1') -RepositoryRoot $RepositoryRoot -OutputPath $notices
    $PackageDirectory = Join-Path $scratch 'package'
    $packageRequest = @{
        operation = 'create'; outputDirectory = $PackageDirectory; version = $version; sourceRevision = $revision
        chatPath = $chat.ChatPath; brokerPath = $chat.BrokerPath; asrPath = $speech.AsrPath
        ttsPath = $speech.TtsPath; toolPath = $launcher; thirdPartyNoticePath = $notices
    } | ConvertTo-Json -Compress
    $package = Invoke-ReleaseCommand $tool @('chat-package', '--json', $packageRequest) | ConvertFrom-Json
    & (Join-Path $PSScriptRoot 'release/Build-WindowsExecutable.ps1') -PackageDirectory $PackageDirectory -ManifestSha256 $package.ManifestSha256 -OutputPath $OutputPath
} finally {
    $resolved = Resolve-ReleasePath $scratch
    $temporaryRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\') + '\'
    if (-not $resolved.StartsWith($temporaryRoot, [StringComparison]::OrdinalIgnoreCase) -or
        [IO.Path]::GetFileName($resolved) -notlike 'SwiftBonsai-Executable-*') { throw 'Refusing cleanup outside the private executable stage.' }
    Remove-Item -LiteralPath $resolved -Recurse -Force
}
