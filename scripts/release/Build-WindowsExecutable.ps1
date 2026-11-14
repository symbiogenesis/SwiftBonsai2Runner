#Requires -Version 7.6
# Forward a reviewed package to its native executable bundler.
[CmdletBinding()]
param([Parameter(Mandatory)][string]$PackageDirectory,[Parameter(Mandatory)][string]$ManifestSha256,
    [Parameter(Mandatory)][string]$OutputPath)
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
$PSNativeCommandUseErrorActionPreference=$false
$request=@{operation='create';packageDirectory=$PackageDirectory;manifestSha256=$ManifestSha256;outputPath=$OutputPath}
& (Join-Path $PackageDirectory 'bin/bonsai-tools.exe') chat-bundle --json (ConvertTo-Json -InputObject $request -Compress)
exit $LASTEXITCODE