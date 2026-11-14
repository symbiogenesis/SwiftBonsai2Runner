#Requires -Version 7.6
# Regenerate the native icon through the Rust asset tool.
[CmdletBinding()]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$root = Split-Path $PSScriptRoot -Parent
& cargo run --locked --manifest-path (Join-Path $root 'Cargo.toml') -p bonsai-tools --bin bonsai-assets -- icons --assets (Join-Path $root 'src/chat-ui/assets')
exit $LASTEXITCODE
