Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$package = Join-Path $env:RUNNER_TEMP 'Native-Acceptance-Package'
[void][IO.Directory]::CreateDirectory($package)
$env:CARGO_TARGET_DIR = Join-Path $env:RUNNER_TEMP 'Native-Acceptance-Build'
$messages = Join-Path $env:RUNNER_TEMP 'native-acceptance-build.jsonl'
"BONSAI_NATIVE_BUILD_EVIDENCE=$messages" >> $env:GITHUB_ENV
$cargo = (Get-Command cargo -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source
& $cargo +nightly-2026-11-02 test --locked --release --no-run --message-format=json --manifest-path (Join-Path $env:GITHUB_WORKSPACE 'Cargo.toml') --package bonsai-broker --test native_model | Set-Content -LiteralPath $messages -Encoding utf8NoBOM
if ($LASTEXITCODE -ne 0) { throw 'Hosted acceptance binary build failed.' }
$artifacts = @(Get-Content -LiteralPath $messages | ForEach-Object { $_ | ConvertFrom-Json } | Where-Object { $_.reason -ceq 'compiler-artifact' -and $_.target.name -ceq 'native_model' -and $_.executable })
if ($artifacts.Count -ne 1) { throw 'Exactly one native_model test binary is required.' }
Copy-Item -LiteralPath $artifacts[0].executable -Destination (Join-Path $package 'native-model-acceptance.exe')
$modelFreeCases = @(& (Join-Path $package 'native-model-acceptance.exe') --list)
if ($LASTEXITCODE -ne 0) { throw 'Hosted acceptance case inventory failed.' }
foreach ($case in @('native_profile_selection_and_scale_receipts_preserve_bounded_scope', 'native_row4_profile_is_explicit_original_pq2_and_one_stream_only',
    'native_owned_resource_receipt_preserves_scopes_and_missing_queries',
    'native_worker_receipts_require_exact_release_and_verified_readiness',
    'speed::native_speed_metric_uses_reported_tokens_and_complete_request_time',
    'speed::native_speed_comparison_requires_matched_output_usage_and_cleanup',
    'speed::native_speed_stream_waits_for_complete_framing_and_uses_usage_not_events',
    'speed::native_speed_partial_pair_survives_cancelled_second_request',
    'speed::native_speed_profile_shares_deadline_and_rejects_extra_workloads',
    'representation::native_representation_pair_requires_completion_identity_and_cleanup',
    'representation::native_representation_comparison_requires_released_worker_observation',
    'representation::native_representation_resources_compare_matching_domains_only',
    'representation::native_representation_resources_reject_missing_or_inconsistent_observations',
    'representation::native_representation_profile_is_explicit_and_keeps_original_deadline',
    'representation::native_representation_row4_profile_is_explicit_and_preserves_pair',
    'representation::native_representation_rollback_observation_survives_pending_ptq1_arm',
    'representation::native_representation_power_gates_timing_without_erasing_answers_or_rollback',
    'representation::native_representation_profiles_leave_skipped_arms_pending_at_original_deadline',
    'representation::native_representation_complete_bytes_survive_expired_second_request',
    'progress::native_progress_decodes_fragmented_chunks_without_counting_events_as_tokens',
    'progress::native_progress_drop_retains_partial_content_without_qualifying_completion',
    'native_ptq1_lifecycle_minimizes_tokens_without_changing_required_steps',
    'native_ptq1_row4_lifecycle_is_explicit_and_preserves_the_required_workload',
    'native_adapter_scale_history_profile_requires_separate_explicit_selection',
    'native_adapter_scale_history_retains_partial_steps_without_qualifying_missing_work',
    'native_adapter_scale_history_requires_reported_bounded_terminal_usage',
    'native_adapter_stream_failures_retain_bound_requests_and_pending_response',
    'adapter_boundaries::native_adapter_boundary_profile_preserves_scope_and_signed_zero',
    'adapter_boundaries::native_adapter_boundary_replay_requires_bound_output_usage_and_settings',
    'adapter_boundaries::native_adapter_boundary_deadline_retains_pending_work_without_model_start',
    'adapter_boundaries::native_adapter_boundary_wire_sequence_preserves_rejections_and_failed_replay',
    'adapter_boundaries::native_adapter_context_rejection_retains_unframed_and_malformed_observations',
    'service_integration::native_service_integration_profile_preserves_bounds_and_pending_verdicts',
    'service_integration::native_pq2_service_integration_profile_preserves_original_default_and_bounds',
    'service_integration::native_pq2_service_integration_fresh_owner_reserve_keeps_original_deadline',
    'service_integration::native_service_integration_binds_prefix_and_reported_prompt_counts',
    'service_integration::native_service_integration_prefix_rejects_terminal_events_already_received',
    'service_integration::native_service_integration_cancellation_failures_return_without_unwinding',
    'service_integration::native_service_integration_cleanup_preserves_failures_and_listener_observations',
    'service_integration::native_service_integration_listener_release_requires_connection_refusal',
    'service_integration::native_service_integration_stuck_owner_is_dropped_inside_existing_stop_allowance',
    'service_integration::native_service_integration_rejects_natural_completion_and_missing_rejections',
    'service_integration::native_service_integration_fresh_owner_reserve_keeps_original_deadline',
    'service_rollback::native_service_rollback_profile_is_separate_and_keeps_explicit_original_selection',
    'service_rollback::native_service_rollback_reserves_tail_before_ptq1_owner',
    'service_rollback::native_service_rollback_requires_complete_original_response_and_post_cleanup_identity',
    'service_rollback::native_service_rollback_deadline_and_missing_ptq1_cleanup_never_start_an_owner',
    'service_rollback::native_service_rollback_keeps_original_stop_and_hash_reserves',
    'service_rollback::native_service_rollback_malformed_responses_wait_for_owned_cleanup',
    'service_rollback::native_service_rollback_transport_failure_retains_pending_progress',
    'native_adapter_generated_history_requires_exact_bound_initial_output',
    'native_adapter_empty_initial_response_keeps_history_pending',
    'native_adapter_first_request_cancellation_retains_ready_phase_and_missing_work',
    'native_adapter_startup_budget_retains_pending_receipt_and_cleanup_reserve',
    'native_adapter_startup_error_retains_unfinished_phase_without_qualifying_cleanup',
    'native_adapter_request_budget_drops_inflight_work_before_owned_cleanup',
    'native_bounded_history_and_stall_requests_preserve_distinct_output_limits',
    'native_extended_acceptance_distinguishes_output_usage_from_delta_count',
    'native_lifecycle_observations_separate_generation_usage_and_owned_cleanup',
    'native_bounded_history_retains_completed_steps_when_followup_is_cancelled',
    'native_stalled_consumer_retains_fragmented_prefix_connection_until_dropped',
    'native_stalled_consumer_never_qualifies_natural_completion_as_interruption',
    'native_stalled_consumer_rejects_error_or_unframed_stream_headers',
    'native_case_budget_rejects_missing_unbounded_and_malformed_allowances',
    'native_case_budget_extension_requires_a_valid_predeclared_total',
    'native_case_budget_cancels_owned_tasks_before_followup')) {
  if ($modelFreeCases -cnotcontains ($case + ': test')) { throw 'Required hosted acceptance regression was not found.' }
  & (Join-Path $package 'native-model-acceptance.exe') $case --exact
  if ($LASTEXITCODE -ne 0) { throw 'Hosted acceptance profile/argument checks failed.' }
}
foreach ($case in @('pinned_native_ptq1_adapter_checks_scale_change_rejection_and_history',
    'pinned_native_ptq1_adapter_row4_checks_scale_change_rejection_and_history',
    'adapter_boundaries::pinned_native_ptq1_adapter_row4_checks_scale_boundaries_and_reset',
    'service_integration::pinned_native_ptq1_adapter_row4_checks_released_service_integration',
    'service_integration::pinned_native_pq2_adapter_row4_checks_released_service_integration',
    'service_rollback::pinned_native_ptq1_service_and_original_pq2_rollback_row4',
    'pinned_native_pq2_adapter_row4_checks_scale_change_rejection_and_history',
    'pinned_native_pq2_adapter_row4_starts_and_streams_within_device_budget',
    'representation::pinned_native_ptq1_pq2_capture_complete_responses',
    'representation::pinned_native_ptq1_pq2_row4_capture_complete_responses')) {
  if ($modelFreeCases -cnotcontains ($case + ': test')) { throw 'Required bounded lifecycle case was not found.' }
}
Copy-Item -LiteralPath $env:BONSAI_TOOLS_PATH -Destination (Join-Path $package 'bonsai-tools.exe')
$analyzerReceipt = Join-Path $env:BONSAI_NATIVE_ANALYZER_EVIDENCE 'rust-checks.json'
$analyzers = Get-Content -LiteralPath $analyzerReceipt -Raw | ConvertFrom-Json
$compilerIdentity = (& rustc +nightly-2026-11-02 -vV) -join "`n"
if ($analyzers.SchemaVersion -ne 2 -or $analyzers.Success -ne $true -or
    $analyzers.Revision -cne $env:GITHUB_SHA -or
    $analyzers.Toolchain -cne 'nightly-2026-11-02-aarch64-pc-windows-msvc' -or
    [string]::IsNullOrWhiteSpace($compilerIdentity) -or $analyzers.CompilerIdentity -cne $compilerIdentity -or
    (@($analyzers.Packages | Sort-Object) -join ',') -cne 'bonsai-broker,bonsai-chat,bonsai-tools' -or
    @($analyzers.Checks).Count -ne 2) { throw 'Hosted strict Release analyzer/link receipt is not bound to this source and compiler.' }
foreach ($phase in @('clippy', 'build')) {
  $checks = @($analyzers.Checks | Where-Object Phase -CEQ $phase)
  if ($checks.Count -ne 1 -or $checks[0].Passed -ne $true -or $checks[0].ExitCode -ne 0 -or
      (@($checks[0].Packages | Sort-Object) -join ',') -cne 'bonsai-broker,bonsai-chat,bonsai-tools') { throw 'Hosted strict Release analyzer/link package coverage differs.' }
}
Copy-Item -LiteralPath $analyzerReceipt -Destination (Join-Path $package 'strict-release-analyzers.json')
$files = @('native-model-acceptance.exe', 'bonsai-tools.exe', 'strict-release-analyzers.json') | ForEach-Object {
  $path = Join-Path $package $_
  [ordered]@{ Name=$_; Bytes=(Get-Item -LiteralPath $path).Length; Sha256=(Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() }
}
[ordered]@{
  SchemaVersion=1; Revision=$env:GITHUB_SHA; RunId=$env:GITHUB_RUN_ID; RunAttempt=$env:GITHUB_RUN_ATTEMPT
  Toolchain='nightly-2026-11-02-aarch64-pc-windows-msvc'; CompilerIdentity=$compilerIdentity
  ToolsSourceId=$env:BONSAI_TOOLS_SOURCE_ID; Files=@($files); ModelExecuted=$false
} | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $package 'receipt.json') -Encoding utf8NoBOM
"BONSAI_ACCEPTANCE_PACKAGE=$package" >> $env:GITHUB_ENV
