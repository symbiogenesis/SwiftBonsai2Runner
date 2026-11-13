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
$tool = Resolve-ReleasePath (Join-Path $PackageDirectory 'bin/bonsai-tools.exe') -Kind File
$request = @{
    operation = 'create'; packageDirectory = $PackageDirectory
    manifestSha256 = $ManifestSha256; outputPath = $OutputPath
} | ConvertTo-Json -Compress
# Native creation validates the launcher and complete payload before publication.
Invoke-ReleaseCommand $tool @('chat-bundle', '--json', $request) | ConvertFrom-Json
