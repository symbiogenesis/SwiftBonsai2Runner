#Requires -Version 7.6
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$RepositoryRoot,
    [Parameter(Mandatory)][string]$AssetDirectory,
    [Parameter(Mandatory)][string]$Tag,
    [Parameter(Mandatory)][string]$SourceRevision,
    [Parameter(Mandatory)][bool]$Prerelease
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
. (Join-Path $PSScriptRoot '../../scripts/release/Release-Common.ps1')
. (Join-Path $PSScriptRoot '../../scripts/release/Release-Assets.ps1')
. (Join-Path $PSScriptRoot '../../scripts/release/Release-Publication.ps1')
if ($Tag -cnotmatch '^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$' -or $SourceRevision -cnotmatch '^[0-9a-f]{40}$') { throw 'Invalid release identity.' }
if ($env:GITHUB_REPOSITORY -cne 'symbiogenesis/SwiftBonsai2Runner' -or $env:GITHUB_REF -cne 'refs/heads/main' -or $env:GITHUB_SHA -cne $SourceRevision) { throw 'Publishing requires this repository main at the dispatched revision.' }
$RepositoryRoot = Resolve-ReleasePath $RepositoryRoot
$AssetDirectory = Resolve-ReleasePath $AssetDirectory
Publish-PortableRelease -RepositoryRoot $RepositoryRoot -AssetDirectory $AssetDirectory -Tag $Tag -SourceRevision $SourceRevision -Prerelease $Prerelease -Repository $env:GITHUB_REPOSITORY
