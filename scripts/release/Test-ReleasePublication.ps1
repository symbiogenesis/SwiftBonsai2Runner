#Requires -Version 7.6
# Stateful native publication regressions use only an in-memory transport.
[CmdletBinding()]
param([ValidateSet('all','resume','ambiguous','published','reject','smoke')][string]$Case='all')
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
$PSNativeCommandUseErrorActionPreference=$true
& cargo test --locked --manifest-path (Join-Path $PSScriptRoot '../../Cargo.toml') -p bonsai-tools --lib release_publication_
if ($LASTEXITCODE -ne 0) { throw 'Native publication controls failed.' }
