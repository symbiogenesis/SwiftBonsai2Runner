#Requires -Version 7.6
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$RepositoryRoot,
    [Parameter(Mandatory)][string]$OutputPath,
    [Parameter(Mandatory)][string]$NativeSourceDirectory
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
. (Join-Path $PSScriptRoot '../../scripts/release/Release-Common.ps1')
$RepositoryRoot = Resolve-ReleasePath $RepositoryRoot
$NativeSourceDirectory = Resolve-ReleasePath $NativeSourceDirectory
$OutputPath = Resolve-ReleasePath $OutputPath -Kind File -AllowMissing

# Read exact resolved package sources, including build dependencies. Retaining
# extra source notices is intentional; an SPDX name alone is not a license text.
$metadata = Invoke-ReleaseCommand 'cargo' @('+nightly-2026-11-09-aarch64-pc-windows-msvc', 'metadata', '--locked', '--format-version', '1',
    '--filter-platform', 'aarch64-pc-windows-msvc', '--manifest-path', (Join-Path $RepositoryRoot 'Cargo.toml')) | ConvertFrom-Json
$resolved = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
foreach ($node in $metadata.resolve.nodes) { [void]$resolved.Add($node.id) }
$text = [Text.StringBuilder]::new()
[void]$text.AppendLine('Swift Bonsai 2 Runner - third-party copyright and license material')
[void]$text.AppendLine('Generated from the locked Windows ARM64 dependency sources. Includes build/development dependencies conservatively; inclusion does not imply runtime use.')
[void]$text.AppendLine('Ubuntu Light font: Copyright 2011 Canonical Ltd.')

function Add-SourceNotices([string]$Label, [string]$Directory, [bool]$Required, [string]$LicenseFile = '') {
    if (-not (Test-Path -LiteralPath $Directory -PathType Container)) { throw "Missing notice source: $Label" }
    $files = @(Get-ChildItem -LiteralPath $Directory -Recurse -File | Where-Object {
        $_.Name -match '^(LICENSE|LICENCE|COPYING|COPYRIGHT|NOTICE|AUTHORS)([._-].*)?$' -or
        $_.Name -in @('Hack-Regular.txt', 'UFL.txt', 'OFL.txt', 'emoji-icon-font-mit-license.txt', 'egui-icons.txt')
    })
    if ($LicenseFile) {
        $declared = if ([IO.Path]::IsPathFullyQualified($LicenseFile)) { $LicenseFile } else { Join-Path $Directory $LicenseFile }
        if (-not (Test-Path -LiteralPath $declared -PathType Leaf)) { throw "Missing declared license file for $Label" }
        $files += Get-Item -LiteralPath $declared
    }
    $files = @($files | Sort-Object FullName -Unique)
    if ($Required -and $files.Count -eq 0) { throw "No copyright/license text found for $Label; review this dependency before release." }
    [void]$text.AppendLine("`n===== $Label =====")
    foreach ($file in $files) {
        if ($file.Length -gt 4MB) { throw "Unexpectedly large license file: $($file.FullName)" }
        $relative = [IO.Path]::GetRelativePath($Directory, $file.FullName)
        [void]$text.AppendLine("`n--- $relative ---")
        [void]$text.AppendLine([IO.File]::ReadAllText($file.FullName))
    }
}

foreach ($package in @($metadata.packages | Sort-Object name, version)) {
    if ($null -eq $package.source -or -not $resolved.Contains($package.id)) { continue }
    if (-not $package.license -and -not $package.license_file) { throw "Missing license declaration: $($package.id)" }
    Add-SourceNotices "$($package.name) $($package.version) [$($package.license)] source=$($package.source)" (Split-Path -Parent $package.manifest_path) $true $package.license_file
}

$fonts = @($metadata.packages | Where-Object { $_.name -ceq 'epaint_default_fonts' })
if ($fonts.Count -ne 1) { throw 'Expected exactly one locked epaint_default_fonts package.' }
$fontRoot = Join-Path (Split-Path -Parent $fonts[0].manifest_path) 'fonts'
foreach ($name in @('Hack-Regular.txt', 'UFL.txt', 'emoji-icon-font-mit-license.txt', 'egui-icons.txt')) {
    if (-not (Test-Path -LiteralPath (Join-Path $fontRoot $name) -PathType Leaf)) { throw "Missing embedded font notice: $name" }
}
$pins = Get-Content -LiteralPath (Join-Path $RepositoryRoot 'config/native-speech.lock.json') -Raw | ConvertFrom-Json
Add-SourceNotices "Qwen native source $($pins.sources.qwen.revision) and GGML $($pins.sources.ggml.revision)" $NativeSourceDirectory $true
if (-not (Test-Path -LiteralPath (Join-Path $NativeSourceDirectory 'ggml/LICENSE') -PathType Leaf)) { throw 'Missing bundled GGML license.' }
[IO.File]::WriteAllText($OutputPath, $text.ToString(), [Text.UTF8Encoding]::new($false))
