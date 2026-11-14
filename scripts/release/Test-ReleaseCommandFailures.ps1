#Requires -Version 7.6
# Native release command regressions use an inert Rust probe, never a shell child.
[CmdletBinding()]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
$PSNativeCommandUseErrorActionPreference=$true
& cargo test --locked --manifest-path (Join-Path $PSScriptRoot '../../Cargo.toml') -p bonsai-tools --features test-probe --test release_process release_command_
if ($LASTEXITCODE -ne 0) { throw 'Native release command controls failed.' }
