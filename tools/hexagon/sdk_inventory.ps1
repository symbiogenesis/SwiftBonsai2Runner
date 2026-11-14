#Requires -Version 7.6
# Compatibility entry point; archive policy and receipt validation are native Rust.
param([Parameter(Mandatory)][string]$Archive,[Parameter(Mandatory)][string]$Pin,
    [Parameter(Mandatory)][string]$Output,[Parameter(Mandatory)][string]$PublicInputDirectory,
    [string]$Reader)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if (-not $Reader) {
    if ($env:RUNNER_ENVIRONMENT -cne 'github-hosted' -or -not $env:RUNNER_TEMP) {
        throw 'Supply a prebuilt SDK reader; compilation is hosted-only.'
    }
    $target = Join-Path $env:RUNNER_TEMP 'Bonsai-Sdk-Reader'
    & cargo build --locked --release --manifest-path (Join-Path $PSScriptRoot 'sdk-reader/Cargo.toml') --target-dir $target
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    $Reader = Join-Path $target ('release/bonsai-sdk-reader' + $(if ($IsWindows) { '.exe' } else { '' }))
}
& $Reader inventory $Archive $Pin $Output $PublicInputDirectory
exit $LASTEXITCODE
