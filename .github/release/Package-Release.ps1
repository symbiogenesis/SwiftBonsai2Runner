#Requires -Version 7.6
# Hosted release packaging adapter; native Rust owns intake, assembly and evidence.
[CmdletBinding()]
param([Parameter(Mandatory)][string]$RepositoryRoot,[Parameter(Mandatory)][string]$InputDirectory,
    [Parameter(Mandatory)][string]$OutputDirectory,[Parameter(Mandatory)][string]$EvidenceDirectory,
    [Parameter(Mandatory)][string]$Tag,[Parameter(Mandatory)][string]$SourceRevision)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot '../../scripts/release/Release-Common.ps1')
$request = @{ operation='package'; repositoryRoot=$RepositoryRoot; inputDirectory=$InputDirectory
    outputDirectory=$OutputDirectory; evidenceDirectory=$EvidenceDirectory; tag=$Tag; sourceRevision=$SourceRevision }
$json = ConvertTo-Json -InputObject $request -Compress
if ($env:BONSAI_TOOLS_PATH) { & $env:BONSAI_TOOLS_PATH release-package --json $json }
else { & cargo run --quiet --locked --manifest-path (Join-Path $RepositoryRoot 'Cargo.toml') -p bonsai-tools --bin bonsai-tools -- release-package --json $json }
if ($LASTEXITCODE -ne 0) { throw 'Native release packaging failed.' }