#Requires -Version 7.6
# The shared action's typed selection/output fixtures execute entirely in Rust.
[CmdletBinding()]
param([ValidateSet('exact_grouped_arguments_and_typed_counts','binary_target_without_ignored_tests','complete_selection_validated_before_execution','cargo_failure_reaches_guard_and_stops','guard_failure_stops_action')][string]$Case)
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
$PSNativeCommandUseErrorActionPreference=$true
$filter='command::checks::regressions::tests::'
$arguments=@('test','--locked','--manifest-path',(Join-Path $PSScriptRoot '../../Cargo.toml'),'-p','bonsai-ci','--lib')
if ($Case) { $filter+=$Case; $arguments+=@($filter,'--','--exact') } else { $arguments+=$filter }
$PSNativeCommandUseErrorActionPreference=$false
$output=@(& cargo @arguments)
$code=$LASTEXITCODE
$output | Write-Output
$request=@{operation='test-output';output=@($output);exitCode=$code;expectedPassed=$(if($Case){1}else{5})} | ConvertTo-Json -Depth 4 -Compress
if($env:BONSAI_CI_PATH) { & $env:BONSAI_CI_PATH native-acceptance-guard --json $request }
else { & cargo run --quiet --locked --manifest-path (Join-Path $PSScriptRoot '../../Cargo.toml') -p bonsai-ci --bin bonsai-ci -- native-acceptance-guard --json $request }
if ($LASTEXITCODE -ne 0) { throw 'Native regression selection/output controls failed.' }
