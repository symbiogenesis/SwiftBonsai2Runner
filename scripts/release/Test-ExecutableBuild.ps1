#Requires -Version 7.6
# Inert Rust source-build controls. No compiler, provisioning or CI runs inside fixtures.
[CmdletBinding()]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
$PSNativeCommandUseErrorActionPreference=$false
& cargo test --locked --manifest-path (Join-Path $PSScriptRoot '../../Cargo.toml') -p bonsai-tools --lib release_package::source::tests:: -- --nocapture
exit $LASTEXITCODE