#Requires -Version 7.6
# Model-free Rust fixture entry. Complete fixture execution belongs to owning CI.
[CmdletBinding()]
param([ValidateSet('summary','lock','archive','archive_traversal',
    'schema2_json_and_confined_members_reject_ambiguous_or_redirected_inputs',
    'schema2_replay_preserves_admitted_losing_and_length_scopes',
    'schema2_replay_rejects_identity_types_terminal_power_budget_and_resource_mutations',
    'telemetry_host_registry_and_preparation_controls',
    'native_evaluator_identity_rejects_reinterpretation_missing_bytes_and_unsupported_format')][string]$Test)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$filter = if ($Test) { 'cpu_evaluation::bundle::tests::' + $Test } else { 'cpu_evaluation::bundle::tests::' }
$arguments = @('+nightly-2026-11-12', 'test', '--locked', '--manifest-path', (Join-Path $root 'Cargo.toml'), '-p', 'bonsai-tools', '--lib', $filter, '--', '--nocapture')
if ($Test) { $arguments += '--exact' }
if ($env:BONSAI_TOOLS_TARGET) { $arguments = $arguments[0..1] + @('--target-dir', $env:BONSAI_TOOLS_TARGET) + $arguments[2..($arguments.Count - 1)] }
& cargo @arguments
exit $LASTEXITCODE
