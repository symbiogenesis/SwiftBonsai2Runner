#Requires -Version 7.6
# Native Windows sharing and exact-private-path cleanup controls.
[CmdletBinding()]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
$PSNativeCommandUseErrorActionPreference=$true
& cargo test --locked --manifest-path (Join-Path $PSScriptRoot '../../Cargo.toml') -p bonsai-tools --lib release_cleanup_
if ($LASTEXITCODE -ne 0) { throw 'Native release cleanup controls failed.' }
