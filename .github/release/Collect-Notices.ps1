#Requires -Version 7.6
[CmdletBinding()]
param([Parameter(Mandatory)][string]$RepositoryRoot,[Parameter(Mandatory)][string]$OutputPath)
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
$PSNativeCommandUseErrorActionPreference=$true
. (Join-Path $PSScriptRoot '../../scripts/release/Release-Common.ps1')
. (Join-Path $PSScriptRoot 'Notice-Common.ps1')
$RepositoryRoot=Resolve-ReleasePath $RepositoryRoot
$OutputPath=Resolve-ReleasePath $OutputPath -Kind File -AllowMissing
$metadata=Invoke-ReleaseCommand 'cargo' @('+nightly-2026-11-12-aarch64-pc-windows-msvc','metadata','--locked','--format-version','1','--filter-platform','aarch64-pc-windows-msvc','--manifest-path',(Join-Path $RepositoryRoot 'Cargo.toml')) -BuildCommand | ConvertFrom-Json
$sections=Invoke-NativeReleasePolicy @{operation='notice-sources';metadata=$metadata;repositoryRoot=$RepositoryRoot}
$text=[Text.StringBuilder]::new()
[void]$text.AppendLine('Swift Bonsai 2 Runner - third-party copyright and license material')
[void]$text.AppendLine('Generated from the locked Windows ARM64 dependency sources. Includes build/development dependencies conservatively; inclusion does not imply runtime use.')
[void]$text.AppendLine('Ubuntu Light font: Copyright 2011 Canonical Ltd.')
foreach ($section in $sections) {
    [void]$text.AppendLine("`n===== $($section.label) =====")
    if ($section.upstreamRequired) {
        [void]$text.AppendLine((Get-UpstreamNoticeText -Directory $section.directory -Repository $section.repository -PackageName $section.packageName -PackageVersion $section.packageVersion))
    } else { [void]$text.AppendLine($section.text) }
}
[IO.File]::WriteAllText($OutputPath,$text.ToString(),[Text.UTF8Encoding]::new($false))
