#Requires -Version 7.6
<#
.SYNOPSIS
Bundles a verified native application package into one Windows executable.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$PackageDirectory,
    [Parameter(Mandatory)][ValidatePattern('^[a-f0-9]{64}$')][string]$ManifestSha256,
    [Parameter(Mandatory)][string]$OutputPath
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
. (Join-Path $PSScriptRoot 'Release-Common.ps1')
if (-not $IsWindows -or [Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne 'Arm64') {
    throw 'Executable packaging requires Windows ARM64.'
}
$PackageDirectory = Resolve-ReleasePath $PackageDirectory
$OutputPath = Resolve-ReleasePath $OutputPath -Kind File -AllowMissing
if ([IO.Path]::GetExtension($OutputPath) -ine '.exe') { throw 'Bundle output must use the .exe extension.' }
if (Test-Path -LiteralPath $OutputPath) { throw 'Executable output already exists; bundles are immutable.' }
$tool = Resolve-ReleasePath (Join-Path $PackageDirectory 'bin/bonsai-tools.exe') -Kind File
$capabilities = Invoke-ReleaseCommand $tool @('--version') | ConvertFrom-Json
if ('chat-bundle' -cnotin $capabilities.Commands) { throw 'Rebuild bonsai-tools with single-executable bundle support.' }
$request = @{
    operation = 'create'; packageDirectory = $PackageDirectory
    manifestSha256 = $ManifestSha256; outputPath = $OutputPath
} | ConvertTo-Json -Compress
$created = Invoke-ReleaseCommand $tool @('chat-bundle', '--json', $request) | ConvertFrom-Json
$inspect = @{ operation = 'inspect'; executablePath = $OutputPath } | ConvertTo-Json -Compress
$checked = Invoke-ReleaseCommand $tool @('chat-bundle', '--json', $inspect) | ConvertFrom-Json
if ($created.ManifestSha256 -cne $ManifestSha256 -or $checked.ManifestSha256 -cne $ManifestSha256 -or
    $created.Id -cne $checked.Id -or $checked.Manifest.target -cne 'aarch64-pc-windows-msvc') {
    throw 'The bundled executable differs from its verified package.'
}
$checked
