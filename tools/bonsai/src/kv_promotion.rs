//! Offline review of practical, selected-device compressed-KV comparisons.

use bonsai_cpu_policy::hardware::SnapdragonFamily;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::identity::{is_lower_hex, sha256};

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "kebab-case", deny_unknown_fields)]
enum Request {
    Plan {
        declaration: Declaration,
    },
    Review {
        declaration: Declaration,
        evidence: Box<Evidence>,
    },
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Declaration {
    identities: Identities,
    workloads: [Workload; 3],
}

#[derive(Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Identities {
    runner_revision: String,
    prism_revision: String,
    runtime_sha256: String,
    model_sha256: String,
    adapter_sha256: String,
    template_sha256: String,
    compiler_identity: String,
    device_identity: String,
    processor_name: String,
    driver_identity: String,
    codec_identity: String,
    sampler_sha256: String,
    adapter_scale: f64,
    seed: u32,
    context_tokens: u32,
    max_completion_tokens: u32,
    row_workers: u8,
    projection_backend: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Workload {
    id: String,
    prompt_sha256: String,
    objective_checks: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Evidence {
    plan_sha256: String,
    controls: Controls,
    pairs: [Pair; 3],
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "Independent evidence attestations are simultaneous checks, not mutually exclusive lifecycle states."
)]
struct Controls {
    exact_packed_restore: bool,
    cancellation_recovery: bool,
    edited_prefix_recovery: bool,
    adapter_scale_recovery: bool,
    explicit_f16_rollback: bool,
    owned_cleanup: bool,
    no_full_history_mirror: bool,
    comparable_whole_task_memory_coverage: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Pair {
    id: String,
    prompt: String,
    baseline: Arm,
    candidate: Arm,
    quality: Quality,
    objective_checks: Vec<Check>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Arm {
    identities: Identities,
    kv_cache: String,
    graph_identity: String,
    input_tokens: u32,
    generated_tokens: u32,
    occupied_tokens: u32,
    complete: bool,
    answer: String,
    answer_sha256: String,
    peak_private_bytes: u64,
    peak_kv_bytes: u64,
    kv_observation_tokens: u32,
    extra_failed_or_retried_requests: u32,
    end_to_end_milliseconds: Option<f64>,
    power_before: Power,
    power_after: Power,
    uninterrupted_power_observed: bool,
}

#[derive(Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Power {
    ac_connected: Option<bool>,
    battery_saver: Option<bool>,
    battery_percent: Option<u8>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Quality {
    reviewer_id: String,
    blind: bool,
    baseline_answer_sha256: String,
    candidate_answer_sha256: String,
    baseline_usefulness: u8,
    candidate_usefulness: u8,
    major_regression: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Check {
    id: String,
    baseline_passed: bool,
    candidate_passed: bool,
}

fn require(condition: bool, message: &str) -> crate::Result<()> {
    condition.ok_or_else(|| message.into())
}

fn concrete(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= 1024
        && !value.trim().eq_ignore_ascii_case("unknown")
        && !value.chars().any(char::is_control)
}

fn validate_identities(ids: &Identities) -> crate::Result<()> {
    require(
        [&ids.runner_revision, &ids.prism_revision]
            .iter()
            .all(|value| is_lower_hex(value, 40))
            && [
                &ids.runtime_sha256,
                &ids.model_sha256,
                &ids.adapter_sha256,
                &ids.template_sha256,
                &ids.codec_identity,
                &ids.sampler_sha256,
            ]
            .iter()
            .all(|value| is_lower_hex(value, 64)),
        "Comparison requires full source and numerical/artifact SHA identities.",
    )?;
    require(
        [
            &ids.compiler_identity,
            &ids.device_identity,
            &ids.driver_identity,
        ]
        .iter()
        .all(|value| concrete(value))
            && matches!(
                SnapdragonFamily::from_processor_name(&ids.processor_name),
                Some(SnapdragonFamily::XElite | SnapdragonFamily::X2EliteExtreme)
            ),
        "Comparison requires a concrete compiler, device, driver and observed X1 Elite or X2 Elite Extreme.",
    )
}

fn policy(declaration: &Declaration) -> crate::Result<Value> {
    validate_identities(&declaration.identities)?;
    let ids = &declaration.identities;
    require(
        ids.adapter_scale.to_bits() == 1.3_f64.to_bits()
            && ids.seed == 42
            && ids.context_tokens == 8192
            && (1..=4096).contains(&ids.max_completion_tokens)
            && matches!(ids.row_workers, 1 | 4)
            && concrete(&ids.projection_backend),
        "Declare the standard adapter/seed, fixed 8K bound, supported worker count and projection placement.",
    )?;
    for (workload, expected) in declaration.workloads.iter().zip([
        "deep-retrieval",
        "instructions",
        "creative-continuation",
    ]) {
        require(
            workload.id == expected
                && is_lower_hex(&workload.prompt_sha256, 64)
                && !workload.objective_checks.is_empty()
                && workload.objective_checks.len() <= 16
                && workload.objective_checks.iter().all(|id| concrete(id))
                && workload
                    .objective_checks
                    .iter()
                    .enumerate()
                    .all(|(index, id)| !workload.objective_checks[..index].contains(id)),
            "Declare the three ordered held-out prompts and unique objective checks before capture.",
        )?;
    }
    Ok(json!({
        "schemaVersion": 1, "declaration": declaration,
        "profile": {"baseline": "f16", "candidate": "rsq4", "contextTokens": 8192,
            "adapterScale": 1.3, "seed": 42, "reserveTokens": 64, "target": "aarch64-pc-windows-msvc",
            "recurrentState": "unchanged-f32"},
        "thresholds": {"deepOccupiedTokensMinimum": 7168, "kvReductionPercentMinimum": 65,
            "deepPeakPrivateSavingsBytesMinimum": 134_217_728,
            "minimumUsefulness": 3, "meanUsefulnessDropMaximum": 0.5, "individualUsefulnessDropMaximum": 1,
            "qualifiedLatencyRegressionPercentMaximum": 10},
        "scope": "Three held-out pairs support selected-device development review only. No exact FP16 answer/logit parity, speedup or cross-device claim."
    }))
}

fn digest(value: &Value) -> crate::Result<String> {
    let bytes = serde_json::to_vec(value).map_err(|error| error.to_string())?;
    let result = sha256(&bytes);
    require(
        is_lower_hex(&result, 64),
        "Unable to compute comparison integrity digest.",
    )?;
    Ok(result)
}

fn validate_arm(arm: &Arm, ids: &Identities, profile: &str) -> crate::Result<()> {
    require(
        arm.identities == *ids
            && arm.kv_cache == profile
            && concrete(&arm.graph_identity)
            && arm.graph_identity.contains(&format!("kv-{profile}")),
        "Capture source, model, adapter, device or cache identity differs from the declaration.",
    )?;
    let total = arm
        .input_tokens
        .checked_add(arm.generated_tokens)
        .ok_or("Capture token counts overflow.")?;
    let reserved = arm
        .input_tokens
        .checked_add(ids.max_completion_tokens)
        .and_then(|count| count.checked_add(64))
        .ok_or("Capture reservation overflows.")?;
    require(
        arm.complete
            && !arm.answer.trim().is_empty()
            && arm.answer.len() <= 1024 * 1024
            && is_lower_hex(&arm.answer_sha256, 64)
            && sha256(arm.answer.as_bytes()) == arm.answer_sha256
            && (1..=8192).contains(&arm.input_tokens)
            && (1..=4096).contains(&arm.generated_tokens)
            && arm.occupied_tokens >= total.saturating_sub(1)
            && arm.occupied_tokens <= total
            && arm.generated_tokens <= ids.max_completion_tokens
            && arm.occupied_tokens <= 8192
            && total <= 8192
            && reserved <= 8192
            && arm.kv_observation_tokens > 0
            && arm.kv_observation_tokens <= arm.occupied_tokens
            && arm.extra_failed_or_retried_requests == 0,
        "Capture requires complete retained answer bytes, a matching digest and valid 8K token counts.",
    )?;
    require(
        arm.peak_private_bytes > 0
            && arm.peak_private_bytes <= (1 << 40)
            && arm.peak_kv_bytes > 0
            && arm.peak_kv_bytes <= arm.peak_private_bytes
            && [
                arm.power_before.battery_percent,
                arm.power_after.battery_percent,
            ]
            .iter()
            .all(|charge| charge.is_none_or(|value| value <= 100))
            && arm
                .end_to_end_milliseconds
                .is_none_or(|value| value.is_finite() && value > 0.0),
        "Capture has invalid complete-task peak memory, power or latency observations.",
    )
}

fn comparable_power(baseline: &Arm, candidate: &Arm) -> bool {
    let observations = [
        &baseline.power_before,
        &baseline.power_after,
        &candidate.power_before,
        &candidate.power_after,
    ];
    baseline.uninterrupted_power_observed
        && candidate.uninterrupted_power_observed
        && observations[0].ac_connected.is_some()
        && observations[0].battery_saver.is_some()
        && observations.iter().all(|power| {
            power.ac_connected == observations[0].ac_connected
                && power.battery_saver == observations[0].battery_saver
        })
}

fn pair_result(pair: &Pair, workload: &Workload, ids: &Identities) -> crate::Result<Value> {
    require(
        pair.id == workload.id && sha256(pair.prompt.as_bytes()) == workload.prompt_sha256,
        "Capture prompt bytes or workload order differs from the declared held-out corpus.",
    )?;
    validate_arm(&pair.baseline, ids, "f16")?;
    validate_arm(&pair.candidate, ids, "rsq4")?;
    require(
        pair.baseline.input_tokens == pair.candidate.input_tokens
            && pair.baseline.graph_identity != pair.candidate.graph_identity
            && pair.baseline.kv_observation_tokens == pair.candidate.kv_observation_tokens,
        "Pairs must use identical input tokens and distinct cache graph identities.",
    )?;
    let review = &pair.quality;
    require(
        concrete(&review.reviewer_id)
            && review.blind
            && review.baseline_answer_sha256 == pair.baseline.answer_sha256
            && review.candidate_answer_sha256 == pair.candidate.answer_sha256
            && (1..=5).contains(&review.baseline_usefulness)
            && (1..=5).contains(&review.candidate_usefulness),
        "Independent blind review must identify and score these exact complete answers.",
    )?;
    require(
        pair.objective_checks.len() == workload.objective_checks.len()
            && pair
                .objective_checks
                .iter()
                .zip(&workload.objective_checks)
                .all(|(check, id)| check.id == *id),
        "Review must retain every predeclared objective check in order.",
    )?;
    let quality = review.baseline_usefulness >= 3
        && review.candidate_usefulness >= 3
        && !review.major_regression
        && i16::from(review.baseline_usefulness) - i16::from(review.candidate_usefulness) <= 1
        && pair
            .objective_checks
            .iter()
            .all(|check| check.baseline_passed && check.candidate_passed);
    let cache_memory = pair.id != "deep-retrieval"
        || pair.candidate.peak_kv_bytes * 100 <= pair.baseline.peak_kv_bytes * 35;
    let private_savings = pair
        .baseline
        .peak_private_bytes
        .checked_sub(pair.candidate.peak_private_bytes);
    let deep_memory = pair.id != "deep-retrieval"
        || (pair.baseline.kv_observation_tokens >= 7168
            && pair.candidate.kv_observation_tokens >= 7168
            && private_savings.is_some_and(|bytes| bytes >= 128 * 1024 * 1024));
    let latency = if comparable_power(&pair.baseline, &pair.candidate) {
        pair.baseline
            .end_to_end_milliseconds
            .zip(pair.candidate.end_to_end_milliseconds)
            .map(|(baseline, candidate)| candidate <= baseline * 1.10)
    } else {
        None
    };
    Ok(json!({"id": pair.id, "qualityPassed": quality,
        "cacheMemoryPassed": cache_memory, "deepProcessMemoryPassed": deep_memory,
        "privateSavingsBytes": private_savings, "qualifiedLatencyPassed": latency,
        "baselineAnswerSha256": pair.baseline.answer_sha256,
        "candidateAnswerSha256": pair.candidate.answer_sha256,
        "power": {"baselineBefore": pair.baseline.power_before, "baselineAfter": pair.baseline.power_after,
            "candidateBefore": pair.candidate.power_before, "candidateAfter": pair.candidate.power_after},
        "passed": quality && cache_memory && deep_memory && latency != Some(false)}))
}

fn review(declaration: &Declaration, evidence: &Evidence, plan: &Value) -> crate::Result<Value> {
    require(
        evidence.plan_sha256 == digest(plan)?,
        "Evidence does not match the predeclared comparison plan.",
    )?;
    let pairs = evidence
        .pairs
        .iter()
        .zip(&declaration.workloads)
        .map(|(pair, workload)| pair_result(pair, workload, &declaration.identities))
        .collect::<crate::Result<Vec<_>>>()?;
    let quality_drop: i16 = evidence
        .pairs
        .iter()
        .map(|pair| {
            i16::from(pair.quality.baseline_usefulness)
                - i16::from(pair.quality.candidate_usefulness)
        })
        .sum();
    let controls = serde_json::to_value(&evidence.controls).map_err(|error| error.to_string())?;
    let controls_pass = controls
        .as_object()
        .is_some_and(|fields| fields.values().all(|value| value == true));
    let passed =
        pairs.iter().all(|pair| pair["passed"] == true) && quality_drop <= 1 && controls_pass;
    let unqualified_latency = pairs
        .iter()
        .any(|pair| pair["qualifiedLatencyPassed"].is_null());
    Ok(
        json!({"schemaVersion": 1, "planSha256": evidence.plan_sha256,
        "status": if passed {"ready-for-owner-review"} else {"no-go"},
        "readyForOwnerReview": passed, "promotionApplied": false,
        "selectedDevice": declaration.identities.device_identity,
        "meanQualityPassed": quality_drop <= 1, "controlsPassed": controls_pass,
        "controls": controls, "pairs": pairs,
        "ownerResponsivenessAcceptanceRequired": unqualified_latency,
        "limits": "Offline integrity and reviewer-attested practical evidence only; does not authenticate observations, collect trials, qualify releases or change defaults. Three prompts have limited statistical confidence. Unknown/mismatched or endpoint-only power provides no timing/energy verdict. Capacity-only approval requires recorded owner acceptance of responsiveness. Preserve explicit FP16 recovery and existing context/admission bounds."}),
    )
}

/// Freeze a practical three-prompt comparison or review its retained complete answers.
///
/// This pure command never launches a model or enables a serving profile. The
/// declaration must precede capture; its digest binds all identities and prompts.
///
/// # Errors
/// Rejects malformed plans, mismatched identities or prompts, incomplete/tampered
/// answers and reviews not bound to those exact answer bytes. Valid losing
/// comparisons return `no-go` with their individual results retained.
pub fn run(request: Value) -> crate::Result<Value> {
    let request: Request = serde_json::from_value(request).map_err(|error| error.to_string())?;
    match request {
        Request::Plan { declaration } => {
            let plan = policy(&declaration)?;
            Ok(json!({"planSha256": digest(&plan)?, "policy": plan,
                "captureRequired": true, "promotionApplied": false}))
        }
        Request::Review {
            declaration,
            evidence,
        } => {
            let plan = policy(&declaration)?;
            review(&declaration, &evidence, &plan)
        }
    }
}

#[cfg(test)]
mod tests;
