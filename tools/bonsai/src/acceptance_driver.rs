//! Native owner-workflow admission, absolute budget bridges and process ownership.
#![cfg_attr(
    not(any(windows, test)),
    expect(
        dead_code,
        reason = "Owner execution uses native Windows boundaries; pure policy is exercised by portable tests."
    )
)]

use serde_json::{Value, json};

mod hosted;

#[cfg(windows)]
mod windows;

const REMOVED_CREDENTIALS: &[&str] = &[
    "ACTIONS_RUNTIME_TOKEN",
    "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
    "GITHUB_TOKEN",
    "GH_TOKEN",
    "ACTIONS_RUNNER_INPUT_TOKEN",
];

fn profile(name: &str, extended: bool, stalled: bool) -> crate::Result<&'static str> {
    if name != "pq2-no-adapter" && (extended || stalled) {
        return Err(
            "The selected bounded profile does not admit extended_history or stalled_consumer."
                .into(),
        );
    }
    Ok(match name {
        "pq2-no-adapter" => "pinned_native_model_streams_recovers_and_restarts_in_required_sandbox",
        "ptq1-adapter" => "pinned_native_ptq1_adapter_starts_and_streams_within_device_budget",
        "ptq1-adapter-lifecycle" => {
            "pinned_native_ptq1_adapter_checks_scale_change_rejection_and_history"
        }
        "ptq1-adapter-lifecycle-row4" => {
            "pinned_native_ptq1_adapter_row4_checks_scale_change_rejection_and_history"
        }
        "ptq1-adapter-boundaries-row4" => {
            "adapter_boundaries::pinned_native_ptq1_adapter_row4_checks_scale_boundaries_and_reset"
        }
        "ptq1-adapter-service-row4" => {
            "service_integration::pinned_native_ptq1_adapter_row4_checks_released_service_integration"
        }
        "ptq1-adapter-service-rollback-row4" => {
            "service_rollback::pinned_native_ptq1_service_and_original_pq2_rollback_row4"
        }
        "pq2-adapter-row4" => {
            "pinned_native_pq2_adapter_row4_starts_and_streams_within_device_budget"
        }
        "pq2-adapter-lifecycle-row4" => {
            "pinned_native_pq2_adapter_row4_checks_scale_change_rejection_and_history"
        }
        "pq2-adapter-service-row4" => {
            "service_integration::pinned_native_pq2_adapter_row4_checks_released_service_integration"
        }
        "pq2-adapter-speed" => "speed::pinned_native_pq2_adapter_compares_request_throughput",
        "ptq1-pq2-complete-response" => {
            "representation::pinned_native_ptq1_pq2_capture_complete_responses"
        }
        "ptq1-pq2-complete-response-row4" => {
            "representation::pinned_native_ptq1_pq2_row4_capture_complete_responses"
        }
        _ => return Err("Unknown actual-model acceptance profile.".into()),
    })
}

fn startup_markers(stderr: &str) -> Vec<String> {
    stderr
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .filter(|line| {
            let Some(rest) = line.strip_prefix("Native backend startup failure: stage=") else {
                return false;
            };
            let Some((stage, rest)) = rest.split_once(", error=") else {
                return false;
            };
            let Some((error, process)) = rest.split_once(", process=") else {
                return false;
            };
            ["authenticated-readiness", "native-identity"].contains(&stage)
                && ["Backend", "Memory", "Cancelled"].contains(&error)
                && (["running", "status-unavailable"].contains(&process)
                    || process.strip_prefix("exited-0x").is_some_and(|code| {
                        code.len() == 8
                            && code
                                .bytes()
                                .all(|byte| byte.is_ascii_digit() || (b'A'..=b'F').contains(&byte))
                    }))
        })
        .map(str::to_owned)
        .collect()
}

fn number(value: &Value, key: &str) -> crate::Result<u64> {
    value[key]
        .as_u64()
        .ok_or_else(|| format!("Missing acceptance integer: {key}"))
}

fn package_identity(value: &Value, revision: &str, run: &str, attempt: &str) -> crate::Result<()> {
    if value["SchemaVersion"] != 2
        || value["Revision"] != revision
        || value["RunId"] != run
        || value["RunAttempt"] != attempt
        || value["Toolchain"] != "nightly-2026-11-12-aarch64-pc-windows-msvc"
        || value["ModelExecuted"] != false
        || value["CompilerIdentity"]
            .as_str()
            .is_none_or(|s| s.trim().is_empty())
        || value["ToolsSourceId"]
            .as_str()
            .is_none_or(|s| !crate::identity::is_lower_hex(s, 64))
    {
        return Err("Hosted acceptance source/run identity mismatch.".into());
    }
    Ok(())
}

struct Bridge {
    hard_deadline: u64,
    work_deadline: u64,
    hard_seconds: u64,
    case_seconds: u64,
    elapsed_seconds: f64,
}

fn bridge(limits: &Value, snapshot: u64, now: u64, frequency: u64) -> crate::Result<Bridge> {
    if frequency == 0 || now < snapshot {
        return Err("Acceptance bridge monotonic clocks are invalid.".into());
    }
    let hard = number(limits, "HardLimitSeconds")?;
    let case = number(limits, "CaseBudgetSeconds")?;
    if hard == 0
        || hard > 850
        || case == 0
        || case > 840
        || case.checked_add(10).is_none_or(|n| n > hard)
    {
        return Err("Acceptance bridge limits are invalid.".into());
    }
    let charge = (now - snapshot).div_ceil(frequency);
    let hard_seconds = hard
        .checked_sub(charge)
        .filter(|n| *n > 0)
        .ok_or("Preparation exhausted the acceptance process allowance.")?;
    let case_seconds = case
        .checked_sub(charge)
        .filter(|n| *n > 0)
        .ok_or("Preparation exhausted the acceptance allowance; qualification remains pending.")?;
    let deadline = |seconds: u64| -> crate::Result<u64> {
        seconds
            .checked_mul(frequency)
            .and_then(|n| snapshot.checked_add(n))
            .ok_or_else(|| "Acceptance bridge deadline overflow.".into())
    };
    #[allow(
        clippy::cast_precision_loss,
        clippy::as_conversions,
        reason = "Reporting bounded clock intervals as fractional seconds never governs an admission or deadline."
    )]
    let elapsed_seconds = (now - snapshot) as f64 / frequency as f64;
    Ok(Bridge {
        hard_deadline: deadline(hard)?,
        work_deadline: deadline(case)?,
        hard_seconds,
        case_seconds,
        elapsed_seconds,
    })
}

fn apply_budget(report: &mut Value, limits: &Value, bridge: &Bridge) {
    report["OwnerDeadlineApplied"] = limits["OwnerDeadlineApplied"].clone();
    if limits["OwnerDeadlineApplied"] == true {
        report["OwnerElapsedMilliseconds"] = json!(
            ((limits["OwnerElapsedSeconds"].as_f64().unwrap_or(0.0) + bridge.elapsed_seconds)
                * 1000.0)
                .ceil()
        );
        report["OwnerWorkSeconds"] = limits["OwnerWorkSeconds"].clone();
        report["OwnerProcessExitReserveSeconds"] = limits["OwnerProcessExitReserveSeconds"].clone();
    }
    for (target, source) in [
        (
            "PreparationElapsedMilliseconds",
            "PreparationElapsedSeconds",
        ),
        (
            "PreparationWallElapsedMilliseconds",
            "PreparationWallElapsedSeconds",
        ),
        (
            "PreparationMonotonicElapsedMilliseconds",
            "PreparationMonotonicElapsedSeconds",
        ),
    ] {
        report[target] = limits[source].as_f64().map_or(Value::Null, |seconds| {
            json!(((seconds + bridge.elapsed_seconds) * 1000.0).ceil())
        });
    }
    report["ModelCaseBudgetSeconds"] = bridge.case_seconds.into();
    report["TestProcessHardLimitSeconds"] = bridge.hard_seconds.into();
}

fn carries_workload_verdict(profile: &str) -> bool {
    [
        "pq2-no-adapter",
        "pq2-adapter-lifecycle-row4",
        "pq2-adapter-service-row4",
        "ptq1-adapter-lifecycle",
        "ptq1-adapter-lifecycle-row4",
        "ptq1-adapter-boundaries-row4",
        "ptq1-adapter-service-row4",
        "ptq1-adapter-service-rollback-row4",
    ]
    .contains(&profile)
}

/// Run a source-bound native owner workflow or inspect its pure admission policy.
///
/// # Errors
/// Rejects unknown profiles, missing original clocks or prebuilt tools, exhausted
/// budgets, changed inventories, failed children and uncertain process-tree cleanup.
pub fn run(request: &Value) -> crate::Result<Value> {
    match request["operation"].as_str() {
        Some("hosted-artifact" | "hosted-inventory" | "hosted-package") => hosted::run(request),
        Some("profile") => Ok(
            json!({"case":profile(request["profile"].as_str().ok_or("Missing acceptance profile.")?,request["extended"].as_bool().ok_or("Missing extended flag.")?,request["stalled"].as_bool().ok_or("Missing stalled flag.")?)?}),
        ),
        Some("startup-markers") => Ok(
            json!({"markers":startup_markers(request["stderr"].as_str().ok_or("Missing startup stderr.")?)}),
        ),
        Some("ptq1" | "acceptance") => {
            #[cfg(windows)]
            {
                windows::run(
                    request["operation"]
                        .as_str()
                        .ok_or("Missing owner operation.")?,
                )
            }
            #[cfg(not(windows))]
            {
                Err("Owner acceptance requires native Windows ARM64.".into())
            }
        }
        _ => Err("Unknown acceptance-driver operation.".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bridge_charges_every_fraction_without_restarting_deadlines() -> crate::Result<()> {
        let limits = json!({"HardLimitSeconds":150,"CaseBudgetSeconds":140});
        let result = bridge(&limits, 1000, 1001, 100)?;
        assert_eq!(result.hard_deadline, 16_000);
        assert_eq!(result.work_deadline, 15_000);
        assert_eq!(result.hard_seconds, 149);
        assert_eq!(result.case_seconds, 139);
        assert!(bridge(&limits, 1000, 15_000, 100).is_err());
        assert!(bridge(&limits, 1000, 999, 100).is_err());
        assert!(bridge(&limits, 1000, 1000, 0).is_err());
        Ok(())
    }
    #[test]
    fn profile_and_markers_require_exact_fixed_controls() -> crate::Result<()> {
        assert_eq!(
            profile("pq2-no-adapter", true, true)?,
            "pinned_native_model_streams_recovers_and_restarts_in_required_sandbox"
        );
        for profile_name in [
            "ptq1-adapter",
            "pq2-adapter-service-row4",
            "ptq1-pq2-complete-response-row4",
        ] {
            assert!(profile(profile_name, true, false).is_err());
            assert!(profile(profile_name, false, true).is_err());
            assert!(profile(profile_name, false, false).is_ok());
        }
        assert!(profile("unknown", false, false).is_err());
        let accepted = "Native backend startup failure: stage=authenticated-readiness, error=Backend, process=exited-0xC0000142";
        assert_eq!(
            startup_markers(&format!("{accepted}\r")),
            vec![accepted.to_owned()]
        );
        assert!(startup_markers(&format!("{accepted}\r\r\n")).is_empty());
        let text = format!(
            "{accepted}\r\n{accepted} extra\nsecret {accepted}\nNative backend startup failure: stage=native-identity, error=Memory, process=running\n"
        );
        assert_eq!(startup_markers(&text),vec![accepted.to_owned(),"Native backend startup failure: stage=native-identity, error=Memory, process=running".to_owned()]);
        assert_eq!(REMOVED_CREDENTIALS.len(), 5);
        Ok(())
    }

    #[test]
    fn hosted_package_identity_rejects_changed_source_run_and_untyped_flags() -> crate::Result<()> {
        let receipt = json!({"SchemaVersion":2,"Revision":"a".repeat(40),"RunId":"123","RunAttempt":"1","Toolchain":"nightly-2026-11-12-aarch64-pc-windows-msvc","ModelExecuted":false,"CompilerIdentity":"rustc fixed","ToolsSourceId":"b".repeat(64)});
        package_identity(&receipt, &"a".repeat(40), "123", "1")?;
        for (field, value) in [
            ("Revision", json!("c".repeat(40))),
            ("RunId", json!("124")),
            ("RunAttempt", json!(2)),
            ("ModelExecuted", json!(0)),
            ("CompilerIdentity", json!(" ")),
            ("ToolsSourceId", json!("B".repeat(64))),
        ] {
            let mut changed = receipt.clone();
            changed[field] = value;
            assert!(package_identity(&changed, &"a".repeat(40), "123", "1").is_err());
        }
        Ok(())
    }
}
