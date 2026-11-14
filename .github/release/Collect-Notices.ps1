#Requires -Version 7.6
# Native notice collection adapter. Policy, immutable transport and rendering are Rust.
[CmdletBinding()]
param([Parameter(Mandatory)][string]$RepositoryRoot,[Parameter(Mandatory)][string]$OutputPath)
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
$PSNativeCommandUseErrorActionPreference=$false
$json=ConvertTo-Json -InputObject @{operation='collect-notices';repositoryRoot=$RepositoryRoot;outputPath=$OutputPath} -Compress
if ($env:BONSAI_TOOLS_PATH) { & $env:BONSAI_TOOLS_PATH release-package --json $json }
else { & cargo run --quiet --locked --manifest-path (Join-Path $RepositoryRoot 'Cargo.toml') -p bonsai-tools --bin bonsai-tools -- release-package --json $json }
if ($LASTEXITCODE -ne 0) { throw 'Native notice collection failed.' }