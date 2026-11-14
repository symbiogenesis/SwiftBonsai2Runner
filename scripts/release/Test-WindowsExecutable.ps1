#Requires -Version 7.6
# Native model-free executable smoke adapter; never starts a model or installs software.
[CmdletBinding()]
param([Parameter(Mandatory)][string]$ExecutablePath,[Parameter(Mandatory)][string]$ManifestSha256,
    [Parameter(Mandatory)][string]$EvidenceDirectory)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$request = @{operation='executable-smoke';executablePath=$ExecutablePath;manifestSha256=$ManifestSha256;evidenceDirectory=$EvidenceDirectory}
$json = ConvertTo-Json -InputObject $request -Compress
if ($env:BONSAI_TOOLS_PATH) { & $env:BONSAI_TOOLS_PATH release-package --json $json }
else { & cargo run --quiet --locked --manifest-path (Join-Path $PSScriptRoot '../../Cargo.toml') -p bonsai-tools --bin bonsai-tools -- release-package --json $json }
if ($LASTEXITCODE -ne 0) { throw 'Native executable smoke failed.' }