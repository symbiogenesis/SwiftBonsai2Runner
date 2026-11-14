//! Hosted native-model artifact selection and fixed fixture inventory.

use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{Read as _, Write as _},
    path::PathBuf,
};

const MODEL_FREE_CASES: &[&str] = &[
    "native_profile_selection_and_scale_receipts_preserve_bounded_scope",
    "native_row4_profile_is_explicit_original_pq2_and_one_stream_only",
    "native_owned_resource_receipt_preserves_scopes_and_missing_queries",
    "native_worker_receipts_require_exact_release_and_verified_readiness",
    "speed::native_speed_metric_uses_reported_tokens_and_complete_request_time",
    "speed::native_speed_comparison_requires_matched_output_usage_and_cleanup",
    "speed::native_speed_stream_waits_for_complete_framing_and_uses_usage_not_events",
    "speed::native_speed_partial_pair_survives_cancelled_second_request",
    "speed::native_speed_profile_shares_deadline_and_rejects_extra_workloads",
    "representation::native_representation_pair_requires_completion_identity_and_cleanup",
    "representation::native_representation_comparison_requires_released_worker_observation",
    "representation::native_representation_resources_compare_matching_domains_only",
    "representation::native_representation_resources_reject_missing_or_inconsistent_observations",
    "representation::native_representation_profile_is_explicit_and_keeps_original_deadline",
    "representation::native_representation_row4_profile_is_explicit_and_preserves_pair",
    "representation::native_representation_rollback_observation_survives_pending_ptq1_arm",
    "representation::native_representation_power_gates_timing_without_erasing_answers_or_rollback",
    "representation::native_representation_profiles_leave_skipped_arms_pending_at_original_deadline",
    "representation::native_representation_complete_bytes_survive_expired_second_request",
    "progress::native_progress_decodes_fragmented_chunks_without_counting_events_as_tokens",
    "progress::native_progress_drop_retains_partial_content_without_qualifying_completion",
    "progress::native_progress_read_error_retains_partial_response_identity_before_cleanup",
    "response_capture::native_response_capture_hashes_fragmented_errors_without_retaining_payloads",
    "response_capture::native_response_capture_cancelled_read_retains_prefix_and_original_deadline",
    "response_capture::native_response_capture_caps_header_storage_and_classifies_connection_failure",
    "native_ptq1_lifecycle_minimizes_tokens_without_changing_required_steps",
    "native_ptq1_row4_lifecycle_is_explicit_and_preserves_the_required_workload",
    "native_adapter_scale_history_profile_requires_separate_explicit_selection",
    "native_adapter_scale_history_retains_partial_steps_without_qualifying_missing_work",
    "native_adapter_scale_history_requires_reported_bounded_terminal_usage",
    "native_adapter_stream_failures_retain_bound_requests_and_pending_response",
    "adapter_boundaries::native_adapter_boundary_profile_preserves_scope_and_signed_zero",
    "adapter_boundaries::native_adapter_boundary_replay_requires_bound_output_usage_and_settings",
    "adapter_boundaries::native_adapter_boundary_deadline_retains_pending_work_without_model_start",
    "adapter_boundaries::native_adapter_boundary_wire_sequence_preserves_rejections_and_failed_replay",
    "adapter_boundaries::native_adapter_context_rejection_retains_unframed_and_malformed_observations",
    "service_integration::native_service_integration_profile_preserves_bounds_and_pending_verdicts",
    "service_integration::native_pq2_service_integration_profile_preserves_original_default_and_bounds",
    "service_integration::native_pq2_service_integration_fresh_owner_reserve_keeps_original_deadline",
    "service_integration::native_service_integration_binds_prefix_and_reported_prompt_counts",
    "service_integration::native_service_integration_prefix_rejects_terminal_events_already_received",
    "service_integration::native_service_integration_cancellation_failures_return_without_unwinding",
    "service_integration::native_service_integration_cancellation_failure_retains_response_identity",
    "service_integration::native_service_integration_cleanup_preserves_failures_and_listener_observations",
    "service_integration::native_service_integration_listener_release_requires_connection_refusal",
    "service_integration::native_service_integration_stuck_owner_is_dropped_inside_existing_stop_allowance",
    "service_integration::native_service_integration_rejects_natural_completion_and_missing_rejections",
    "service_integration::native_service_integration_fresh_owner_reserve_keeps_original_deadline",
    "service_rollback::native_service_rollback_profile_is_separate_and_keeps_explicit_original_selection",
    "service_rollback::native_service_rollback_reserves_tail_before_ptq1_owner",
    "service_rollback::native_service_rollback_requires_complete_original_response_and_post_cleanup_identity",
    "service_rollback::native_service_rollback_deadline_and_missing_ptq1_cleanup_never_start_an_owner",
    "service_rollback::native_service_rollback_keeps_original_stop_and_hash_reserves",
    "service_rollback::native_service_rollback_malformed_responses_wait_for_owned_cleanup",
    "service_rollback::native_service_rollback_transport_failure_retains_pending_progress",
    "native_adapter_generated_history_requires_exact_bound_initial_output",
    "native_adapter_empty_initial_response_keeps_history_pending",
    "native_adapter_first_request_cancellation_retains_ready_phase_and_missing_work",
    "native_adapter_startup_budget_retains_pending_receipt_and_cleanup_reserve",
    "native_adapter_startup_timeout_retains_failure_before_original_deadline",
    "native_adapter_startup_error_retains_unfinished_phase_without_qualifying_cleanup",
    "native_adapter_request_budget_drops_inflight_work_before_owned_cleanup",
    "native_bounded_history_and_stall_requests_preserve_distinct_output_limits",
    "native_extended_acceptance_distinguishes_output_usage_from_delta_count",
    "native_lifecycle_observations_separate_generation_usage_and_owned_cleanup",
    "native_bounded_history_retains_completed_steps_when_followup_is_cancelled",
    "native_stalled_consumer_retains_fragmented_prefix_connection_until_dropped",
    "native_stalled_consumer_never_qualifies_natural_completion_as_interruption",
    "native_stalled_consumer_rejects_error_or_unframed_stream_headers",
    "native_case_budget_rejects_missing_unbounded_and_malformed_allowances",
    "native_case_budget_extension_requires_a_valid_predeclared_total",
    "native_case_budget_cancels_owned_tasks_before_followup",
];
const IGNORED_CASES: &[&str] = &[
    "pinned_native_ptq1_adapter_checks_scale_change_rejection_and_history",
    "pinned_native_ptq1_adapter_row4_checks_scale_change_rejection_and_history",
    "adapter_boundaries::pinned_native_ptq1_adapter_row4_checks_scale_boundaries_and_reset",
    "service_integration::pinned_native_ptq1_adapter_row4_checks_released_service_integration",
    "service_integration::pinned_native_pq2_adapter_row4_checks_released_service_integration",
    "service_rollback::pinned_native_ptq1_service_and_original_pq2_rollback_row4",
    "pinned_native_pq2_adapter_row4_checks_scale_change_rejection_and_history",
    "pinned_native_pq2_adapter_row4_starts_and_streams_within_device_budget",
    "representation::pinned_native_ptq1_pq2_capture_complete_responses",
    "representation::pinned_native_ptq1_pq2_row4_capture_complete_responses",
];

fn text<'a>(request: &'a Value, key: &str) -> crate::Result<&'a str> {
    request[key]
        .as_str()
        .ok_or_else(|| format!("Missing hosted acceptance string: {key}"))
}

fn artifact(raw: &[u8]) -> crate::Result<Value> {
    if raw.len() > 16 * 1024 * 1024 {
        return Err("Cargo artifact messages exceed their byte bound.".into());
    }
    let raw = std::str::from_utf8(raw).map_err(|error| error.to_string())?;
    let mut selected = None;
    for line in raw.lines().filter(|line| !line.trim().is_empty()) {
        let message: Value = serde_json::from_str(line).map_err(|error| error.to_string())?;
        if message["reason"] == "compiler-artifact"
            && message["target"]["name"] == "native_model"
            && !message["executable"].is_null()
        {
            let executable = PathBuf::from(text(&message, "executable")?);
            if !executable.is_absolute()
                || !message["target"]["kind"]
                    .as_array()
                    .is_some_and(|kinds| kinds.iter().any(|kind| kind == "test"))
                || selected.replace(executable).is_some()
            {
                return Err("Exactly one native_model test binary is required.".into());
            }
        }
    }
    Ok(json!({"Executable":selected.ok_or("Exactly one native_model test binary is required.")?}))
}

fn inventory(lines: &[String]) -> crate::Result<Value> {
    if lines.len() > 16_384 || lines.iter().any(|line| line.len() > 4096) {
        return Err("Hosted acceptance case inventory exceeds its bound.".into());
    }
    let mut seen = BTreeSet::new();
    for line in lines {
        if line.ends_with(": test") && !seen.insert(line.as_str()) {
            return Err("Hosted acceptance inventory duplicates a case.".into());
        }
    }
    for case in MODEL_FREE_CASES.iter().chain(IGNORED_CASES) {
        if !seen.contains(format!("{case}: test").as_str()) {
            return Err(format!(
                "Required hosted acceptance case was not found: {case}"
            ));
        }
    }
    Ok(
        json!({"ModelFreeCases":MODEL_FREE_CASES,"IgnoredCases":IGNORED_CASES,"ModelFreeCount":MODEL_FREE_CASES.len(),"IgnoredCount":IGNORED_CASES.len()}),
    )
}

fn digest(path: &std::path::Path) -> crate::Result<(u64, String)> {
    let mut file = bonsai_artifacts::open_locked_file(path).map_err(|error| error.to_string())?;
    let length = file.metadata().map_err(|error| error.to_string())?.len();
    if length == 0 || length > 134_217_728 {
        return Err("Hosted acceptance file extent is invalid.".into());
    }
    let mut hash = crate::identity::Hasher::new()?;
    let mut buffer = vec![0; 65_536];
    let mut total = 0_u64;
    loop {
        let count = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        total += u64::try_from(count).map_err(|error| error.to_string())?;
        if total > length {
            return Err("Hosted acceptance file grew during publication.".into());
        }
        hash.update(&buffer[..count])?;
    }
    if total != length {
        return Err("Hosted acceptance file changed during publication.".into());
    }
    Ok((length, hash.finalize_hex()?))
}

fn package(request: &Value) -> crate::Result<Value> {
    if !cfg!(windows)
        || std::env::consts::ARCH != "aarch64"
        || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
    {
        return Err("Acceptance package publication requires hosted Windows ARM64.".into());
    }
    let root = PathBuf::from(text(request, "packageDirectory")?);
    bonsai_artifacts::validate_external_path(&root).map_err(|error| error.to_string())?;
    let names = [
        "native-model-acceptance.exe",
        "bonsai-tools.exe",
        "bonsai-ci.exe",
        "strict-release-analyzers.json",
        "workflow-contract.json",
    ];
    let actual = fs::read_dir(&root)
        .map_err(|error| error.to_string())?
        .map(|entry| {
            entry.map_err(|error| error.to_string()).and_then(|entry| {
                let metadata =
                    fs::symlink_metadata(entry.path()).map_err(|error| error.to_string())?;
                if !metadata.is_file() || bonsai_artifacts::is_redirect(&metadata) {
                    return Err("Redirected or nested acceptance package entry.".into());
                }
                entry
                    .file_name()
                    .into_string()
                    .map_err(|_| "Invalid acceptance package member name.".into())
            })
        })
        .collect::<crate::Result<BTreeSet<_>>>()?;
    if actual.iter().map(String::as_str).collect::<BTreeSet<_>>() != names.iter().copied().collect()
    {
        return Err("Hosted acceptance package requires exactly its five selected files before receipt publication.".into());
    }
    let mut files = Vec::new();
    for name in names {
        let (bytes, hash) = digest(&root.join(name))?;
        files.push(json!({"Name":name,"Bytes":bytes,"Sha256":hash}));
    }
    let mut receipt = json!({"SchemaVersion":2,"Revision":std::env::var("GITHUB_SHA").unwrap_or_default(),"RunId":std::env::var("GITHUB_RUN_ID").unwrap_or_default(),"RunAttempt":std::env::var("GITHUB_RUN_ATTEMPT").unwrap_or_default(),"Toolchain":"nightly-2026-11-12-aarch64-pc-windows-msvc","CompilerIdentity":text(request,"compilerIdentity")?,"ToolsSourceId":text(request,"toolsSourceId")?,"Files":files,"ModelExecuted":false});
    super::package_identity(
        &receipt,
        text(&receipt, "Revision")?,
        text(&receipt, "RunId")?,
        text(&receipt, "RunAttempt")?,
    )?;
    if !crate::identity::is_lower_hex(text(&receipt, "Revision")?, 40)
        || text(&receipt, "RunId")?.is_empty()
        || text(&receipt, "RunAttempt")?.is_empty()
    {
        return Err("Hosted acceptance source/run identity is missing.".into());
    }
    let destination = root.join("receipt.json");
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&destination)
        .map_err(|error| error.to_string())?;
    serde_json::to_writer_pretty(&mut output, &receipt).map_err(|error| error.to_string())?;
    output
        .write_all(b"\n")
        .and_then(|()| output.sync_all())
        .map_err(|error| error.to_string())?;
    receipt["ReceiptPath"] = json!(destination);
    Ok(receipt)
}

pub(super) fn run(request: &Value) -> crate::Result<Value> {
    match text(request, "operation")? {
        "hosted-artifact" => {
            let path = PathBuf::from(text(request, "messagesPath")?);
            let file = File::open(path).map_err(|error| error.to_string())?;
            artifact(
                &crate::bounded_input::read(file, 16 * 1024 * 1024).map_err(|error| {
                    error.into_message("Cargo artifact messages exceed their byte bound.")
                })?,
            )
        }
        "hosted-inventory" => inventory(
            &serde_json::from_value::<Vec<String>>(request["outputLines"].clone())
                .map_err(|error| error.to_string())?,
        ),
        "hosted-package" => package(request),
        _ => Err("Unknown hosted acceptance operation.".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn artifact_selection_requires_one_exact_native_model_test() -> crate::Result<()> {
        let executable = std::env::temp_dir().join("native-model.exe");
        let valid=json!({"reason":"compiler-artifact","target":{"name":"native_model","kind":["test"]},"executable":executable}).to_string();
        assert_eq!(artifact(valid.as_bytes())?["Executable"], json!(executable));
        for changed in [
            String::new(),
            format!("{valid}\n{valid}"),
            valid.replace("\"test\"", "\"bin\""),
            valid.replace("native_model", "different"),
            "{bad-json}".into(),
        ] {
            assert!(artifact(changed.as_bytes()).is_err());
        }
        Ok(())
    }
    #[test]
    fn full_hosted_inventory_is_admitted_before_any_selected_test() -> crate::Result<()> {
        assert_eq!(MODEL_FREE_CASES.len(), 72);
        assert_eq!(IGNORED_CASES.len(), 10);
        let mut valid = MODEL_FREE_CASES
            .iter()
            .chain(IGNORED_CASES)
            .map(|name| format!("{name}: test"))
            .collect::<Vec<_>>();
        inventory(&valid)?;
        valid.pop();
        assert!(inventory(&valid).is_err());
        valid.push(format!("{}: test", IGNORED_CASES[9]));
        valid.push(valid[0].clone());
        assert!(inventory(&valid).is_err());
        Ok(())
    }
}
