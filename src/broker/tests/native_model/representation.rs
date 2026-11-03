//! A bounded complete-response feasibility pair, independent of decode-TPS gates.
#![expect(
    clippy::redundant_pub_crate,
    reason = "The private test module shares profile constants only with its parent harness."
)]

use super::*;

pub(super) const PROFILE: &str = "ptq1-pq2-complete-response";
pub(super) const CASE: &str = "representation::pinned_native_ptq1_pq2_capture_complete_responses";
pub(super) const ROW4_PROFILE: &str = "ptq1-pq2-complete-response-row4";
pub(super) const ROW4_CASE: &str =
    "representation::pinned_native_ptq1_pq2_row4_capture_complete_responses";
const CAP: u32 = 32;
const PROMPT: &str = "Name one tree. Answer with only its name.";
const NEXT_OWNER_RESERVE: Duration = Duration::from_secs(60);
fn profiles(row_workers: u8) -> [AdapterProfile; 2] {
    assert!([1, 4].contains(&row_workers));
    let (name, case) = if row_workers == 1 {
        (PROFILE, CASE)
    } else {
        (ROW4_PROFILE, ROW4_CASE)
    };
    [false, true].map(|repacked| AdapterProfile {
        name,
        case,
        repacked,
        row_workers,
        optional_requests: false,
        required_scale_history: false,
    })
}

pub(super) fn body() -> Value {
    let mut body = request(json!([{"role":"user","content":PROMPT}]), true, CAP);
    body["enable_thinking"] = json!(false);
    body["abliteration_scale"] = json!(1.3);
    body
}

fn digest(bytes: &[u8]) -> String {
    bonsai_artifacts::hash_reader(
        &mut io::Cursor::new(bytes),
        u64::try_from(bytes.len()).unwrap(),
        &mut [0; 4096],
        &mut || Ok(()),
    )
    .unwrap()
}

pub(super) fn model_identity(repacked: bool) -> Value {
    if repacked {
        let export: Value =
            serde_json::from_str(include_str!("../../../../config/swift.ptq1-export.json"))
                .unwrap();
        json!({"filename":export["OutputFilename"], "bytes":export["OutputBytes"],
            "sha256":export["OutputSha256"], "weightRepresentation":"current-repacked-PTQ1_0"})
    } else {
        let lock: Value = serde_json::from_str(include_str!(
            "../../../../config/swift-assessment.lock.json"
        ))
        .unwrap();
        let mut identity = lock["model"].clone();
        identity["weightRepresentation"] = json!("PQ2_0");
        identity
    }
}

async fn measured_response(
    port: u16,
    credential: &str,
    evidence: &Path,
    report: &mut Value,
) -> Value {
    // Final parsing and hashing follow connection closure. This timer includes
    // prefill, generation, transport and incremental observer/receipt I/O.
    let started = tokio::time::Instant::now();
    let bytes = progress::exchange(
        port,
        credential,
        &body(),
        evidence,
        report,
        "representation-complete-response",
    )
    .await;
    let elapsed = started.elapsed();
    response_observation(&bytes, elapsed)
}

pub(super) fn response_observation(bytes: &[u8], elapsed: Duration) -> Value {
    let summary = streamed_response(bytes);
    let mut observed = stream_observation(&summary, CAP);
    let content = speed::visible_output(bytes);
    observed["requestMilliseconds"] = json!(elapsed.as_secs_f64() * 1000.0);
    observed["visibleContentSha256"] = json!(digest(content.as_bytes()));
    observed["visibleContent"] = json!(content);
    observed["naturallyStopped"] = json!(summary.finish_reason == "stop");
    observed
}

async fn record_response(
    port: u16,
    credential: &str,
    evidence: &Path,
    deadline: tokio::time::Instant,
    report: &mut Value,
    index: usize,
) -> bool {
    let observed = adapter_work_before_cleanup(
        deadline,
        measured_response(port, credential, evidence, report),
    )
    .await;
    let completed = observed.is_some();
    report["samples"][index]["stream"] = observed.unwrap_or(Value::Null);
    if !completed {
        report["pendingReason"] = json!("request-budget-expired");
    }
    save_acceptance_report(evidence, report);
    completed
}

fn sample_pending_reason(sample: &Value, repacked: bool, row_workers: u8) -> Option<&'static str> {
    if !matches!(row_workers, 1 | 4) {
        return Some("unsupported-worker-policy");
    }
    let runtime_lock: Value =
        serde_json::from_str(include_str!("../../../../config/prism-inference.lock.json")).unwrap();
    let worker_identity =
        worker_readiness_receipt(runtime_lock["version"].as_str(), row_workers, true);
    let stream = &sample["stream"];
    let content = stream["visibleContent"].as_str().unwrap_or("");
    if [
        "rowWorkerReadinessVersion",
        "rowWorkerCountIndependentlyObserved",
        "observedNativeRowWorkers",
    ]
    .iter()
    .any(|field| sample[field] != worker_identity[field])
    {
        return Some("missing-or-mismatched-worker-readiness-identity");
    }
    if sample["modelIdentity"] != model_identity(repacked)
        || sample["configuredNativeRowWorkers"] != row_workers
        || sample["verifiedReadinessObserved"] != true
        || stream["finishReason"] != "stop"
        || stream["naturallyStopped"] != true
        || stream["reportedCompletionTokens"]
            .as_u64()
            .is_none_or(|n| !(1..=u64::from(CAP)).contains(&n))
        || content.trim().is_empty()
        || stream["visibleContentSha256"] != digest(content.as_bytes())
        || stream["requestMilliseconds"]
            .as_f64()
            .is_none_or(|ms| !ms.is_finite() || ms <= 0.0)
        || sample["cleanup"]["ownedJobEmpty"] != true
        || sample["cleanup"]["listenerReleased"] != true
    {
        return Some("missing-complete-response-identity-timing-or-cleanup");
    }
    None
}

pub(super) fn original_pq2_rollback_observation(samples: &[Value], row_workers: u8) -> Value {
    // This observation concerns only the original-artifact arm. A later PTQ1
    // failure cannot erase it or turn it into complete-pair/performance evidence.
    let reason = samples
        .first()
        .map_or(Some("original-pq2-arm-not-started"), |sample| {
            sample_pending_reason(sample, false, row_workers)
        });
    json!({"status":if reason.is_none() {"captured-original-pq2-arm"} else {"pending"},
        "reason":reason, "sampleIndex":0, "configuredNativeRowWorkers":row_workers,
        "explicitRepresentationArgument":"--use-original-pq2",
        "gateId":"original-pq2-rollback", "independentGateReview":"pending",
        "scope":"Original PQ2 readiness, complete response and owned cleanup under the compiled release/adapter/admission policy; exact artifact retention and CI identities require independent review",
        "defaultMayChange":false})
}

fn comparison(samples: &[Value], row_workers: u8) -> Value {
    let pending = |reason| {
        json!({"status":"pending", "reason":reason,
        "ptq1OverPq2RequestLatency":null,"performanceComparisonEligible":false})
    };
    if !matches!(row_workers, 1 | 4) || samples.len() != 2 {
        return pending("incomplete-pair");
    }
    for (index, sample) in samples.iter().enumerate() {
        if let Some(reason) = sample_pending_reason(sample, index == 1, row_workers) {
            return pending(reason);
        }
    }
    // Different complete responses remain available for independent review.
    // A raw latency ratio with different output work would be misleading.
    let matched = samples[0]["stream"]["visibleContent"] == samples[1]["stream"]["visibleContent"]
        && samples[0]["stream"]["reportedCompletionTokens"]
            == samples[1]["stream"]["reportedCompletionTokens"];
    let power_conditions = bonsai_cpu_policy::power::compare_samples(samples);
    let eligible = matched && power_conditions["performanceComparisonEligible"] == true;
    let observed_ratio = matched.then(|| {
        samples[1]["stream"]["requestMilliseconds"]
            .as_f64()
            .unwrap()
            / samples[0]["stream"]["requestMilliseconds"]
                .as_f64()
                .unwrap()
    });
    json!({"status":"captured-complete-pair", "matchedOutputAndUsage":matched,
        "ptq1OverPq2RequestLatency":eligible.then_some(observed_ratio).flatten(),
        "observedUnqualifiedRequestLatencyRatio":observed_ratio,
        "performanceComparisonEligible":eligible,"powerConditions":power_conditions,
        "resources":resource_comparison(samples, matched),
        "fixedOrder":true, "qualityReview":"pending", "repeatedTrialBenefitQualified":false})
}

const RESOURCE_METRICS: [(&str, &str); 4] = [
    (
        "osLifetimePeakJobCommitBytes",
        "OS lifetime peak commitment for the complete owned Job, including descendants and startup",
    ),
    (
        "osLifetimePeakRootWorkingSetBytes",
        "OS lifetime peak root-process residency, including mapped pages and startup, excluding descendants",
    ),
    (
        "sampledPeakRootPrivateBytes",
        "maximum root-process private commitment observed by serving guard samples; startup is excluded",
    ),
    (
        "sampledPeakRootWorkingSetBytes",
        "maximum root-process residency observed by serving guard samples; startup and descendants are excluded",
    ),
];

fn resource_comparison(samples: &[Value], matched: bool) -> Value {
    let pending = |reason| {
        json!({"status":"pending", "reason":reason, "metrics":[],
            "fullResourceGateQualified":false,"pagingQualified":false,
            "sustainedComfortQualified":false,"resourceBenefitQualified":false})
    };
    if samples.len() != 2 {
        return pending("incomplete-pair");
    }
    if !matched {
        return pending("different-complete-output-or-usage");
    }
    let resources = [
        &samples[0]["cleanup"]["backendResources"],
        &samples[1]["cleanup"]["backendResources"],
    ];
    for resource in &resources {
        if resource["successfulGuardSamples"]
            .as_u64()
            .is_none_or(|n| n == 0)
            || resource["guardIntervalMilliseconds"] != 250
            || resource["sampledMinimumSystemAvailablePhysicalBytes"]
                .as_u64()
                .is_none_or(|n| n == 0)
            || RESOURCE_METRICS
                .iter()
                .any(|(field, _)| resource[*field].as_u64().is_none_or(|n| n == 0))
        {
            return pending("missing-positive-resource-observations");
        }
        if resource["sampledPeakRootPrivateBytes"].as_u64().unwrap()
            > resource["osLifetimePeakJobCommitBytes"].as_u64().unwrap()
            || resource["sampledPeakRootWorkingSetBytes"].as_u64().unwrap()
                > resource["osLifetimePeakRootWorkingSetBytes"]
                    .as_u64()
                    .unwrap()
        {
            return pending("inconsistent-sampled-and-lifetime-resource-observations");
        }
    }
    let metrics: Vec<_> = RESOURCE_METRICS
        .iter()
        .map(|(field, scope)| {
            json!({"field":field,"scope":scope,"pq2Bytes":resources[0][*field],
            "ptq1Bytes":resources[1][*field],
            "ptq1OverPq2":resources[1][*field].as_f64().unwrap()
                / resources[0][*field].as_f64().unwrap()})
        })
        .collect();
    let serving_observations: Vec<_> = resources.iter().map(|resource| {
        json!({"successfulGuardSamples":resource["successfulGuardSamples"],
            "guardIntervalMilliseconds":resource["guardIntervalMilliseconds"],
            "sampledMinimumSystemAvailablePhysicalBytes":resource["sampledMinimumSystemAvailablePhysicalBytes"]})
    }).collect();
    json!({"status":"observed-bounded-pair", "metrics":metrics,
        "servingObservationsInRepresentationOrder":serving_observations,
        "scope":"One fixed-order matched complete-answer pair; commitment and residency are non-additive; sampled bounds are not lifetime peaks; system available memory is context, not attributed model consumption; no full-context, paging or sustained-use evidence",
        "fullResourceGateQualified":false,"pagingQualified":false,
        "sustainedComfortQualified":false,"resourceBenefitQualified":false})
}

async fn paired_responses(
    data: &Path,
    evidence: &Path,
    deadline: tokio::time::Instant,
    row_workers: u8,
    report: &mut Value,
) {
    power::begin(report);
    for profile in &profiles(row_workers) {
        // Decide before each startup, charging the first owner and all prior
        // setup against the same deadline. Missing arms may not become jobs.
        if !can_start_owner(deadline) {
            report["pendingReason"] = json!("insufficient-next-owner-and-cleanup-reserve");
            break;
        }
        let index = report["samples"].as_array().unwrap().len();
        report["samples"].as_array_mut().unwrap().push(json!({
            "modelIdentity":model_identity(profile.repacked), "configuredNativeRowWorkers":profile.row_workers,
            "rowWorkerCountIndependentlyObserved":false, "verifiedReadinessObserved":false,
            "startupMilliseconds":null, "stream":null, "cleanup":null}));
        record_worker_readiness(&mut report["samples"][index], profile.row_workers, false);
        power::begin(&mut report["samples"][index]);
        report["phase"] = json!(format!("arm-{index}-startup"));
        save_acceptance_report(evidence, report);
        let started = tokio::time::Instant::now();
        let Some(owner) =
            adapter_work_before_cleanup(deadline, start_adapter(profile, data, evidence)).await
        else {
            report["pendingReason"] = json!("startup-budget-expired");
            power::finish(&mut report["samples"][index]);
            break;
        };
        report["samples"][index]["startupMilliseconds"] =
            json!(started.elapsed().as_secs_f64() * 1000.0);
        record_worker_readiness(&mut report["samples"][index], profile.row_workers, true);
        let port = owner.port();
        let credential = key(&owner);
        let (stop, stopped) = oneshot::channel();
        let running = spawn_local(owner.run(async { stopped.await.map_err(io::Error::other) }));
        report["phase"] = json!(format!("arm-{index}-request"));
        save_acceptance_report(evidence, report);
        let expired = !record_response(port, &credential, evidence, deadline, report, index).await;
        report["phase"] = json!(format!("arm-{index}-cleanup"));
        save_acceptance_report(evidence, report);
        report["samples"][index]["cleanup"] =
            adapter_owned_stop(stop, running, port, expired).await;
        power::finish(&mut report["samples"][index]);
        report["originalPq2Rollback"] =
            original_pq2_rollback_observation(report["samples"].as_array().unwrap(), row_workers);
        save_acceptance_report(evidence, report);
        if expired {
            break;
        }
    }
    report["comparison"] = comparison(report["samples"].as_array().unwrap(), row_workers);
    report["originalPq2Rollback"] =
        original_pq2_rollback_observation(report["samples"].as_array().unwrap(), row_workers);
    report["passed"] = json!(report["comparison"]["status"] == "captured-complete-pair");
    report["phase"] = json!("completed");
    power::finish(report);
    save_acceptance_report(evidence, report);
}

fn can_start_owner(deadline: tokio::time::Instant) -> bool {
    deadline.saturating_duration_since(tokio::time::Instant::now()) > NEXT_OWNER_RESERVE
}

#[test]
fn native_representation_pair_requires_completion_identity_and_cleanup() {
    for row_workers in [1, 4] {
        let pair = vec![
            complete_sample(false, row_workers),
            complete_sample(true, row_workers),
        ];
        assert_eq!(
            comparison(&pair, row_workers)["ptq1OverPq2RequestLatency"],
            1.0
        );
        for (path, value) in [
            ("/stream/finishReason", json!("length")),
            ("/stream/reportedCompletionTokens", Value::Null),
            ("/stream/reportedCompletionTokens", json!(0)),
            ("/stream/reportedCompletionTokens", json!(33)),
            ("/stream/visibleContentSha256", json!("0".repeat(64))),
            ("/stream/requestMilliseconds", json!(0)),
            ("/modelIdentity/sha256", json!("0".repeat(64))),
            (
                "/configuredNativeRowWorkers",
                json!(if row_workers == 1 { 4 } else { 1 }),
            ),
            ("/cleanup/ownedJobEmpty", json!(false)),
            ("/cleanup/listenerReleased", json!(false)),
        ] {
            let mut changed = pair.clone();
            *changed[1].pointer_mut(path).unwrap() = value;
            assert_eq!(
                comparison(&changed, row_workers)["status"],
                "pending",
                "{path}"
            );
        }
        let mut changed = pair.clone();
        changed[1]["stream"]["visibleContent"] = json!("Elm");
        changed[1]["stream"]["visibleContentSha256"] = json!(digest(b"Elm"));
        let result = comparison(&changed, row_workers);
        assert_eq!(result["status"], "captured-complete-pair");
        assert!(result["ptq1OverPq2RequestLatency"].is_null());
        assert_eq!(result["qualityReview"], "pending");
        assert_eq!(comparison(&pair[..1], row_workers)["status"], "pending");
        assert_eq!(
            comparison(&pair, if row_workers == 1 { 4 } else { 1 })["status"],
            "pending"
        );
        assert_eq!(
            comparison(&[pair[1].clone(), pair[0].clone()], row_workers)["status"],
            "pending"
        );
    }
}

pub(super) fn complete_sample(repacked: bool, row_workers: u8) -> Value {
    let power = bonsai_cpu_policy::power::observed_snapshot(Some(true), false, Some(80));
    let mut sample = json!({"modelIdentity":model_identity(repacked),
        "powerBefore":power,"powerAfter":power,
        "configuredNativeRowWorkers":row_workers,"verifiedReadinessObserved":true,
        "stream":{"finishReason":"stop","naturallyStopped":true,"reportedCompletionTokens":3,
            "visibleContent":"Oak","visibleContentSha256":digest(b"Oak"),"requestMilliseconds":1000},
        "cleanup":{"ownedJobEmpty":true,"listenerReleased":true}});
    sample["cleanup"]["backendResources"] = backend_resource_observation(Some(BackendResources {
        sample_count: 2,
        sampled_peak_root_private_bytes: 800,
        sampled_peak_root_working_set_bytes: 1600,
        sampled_min_available_physical_bytes: Some(4 * 1024 * 1024 * 1024),
        os_peak_job_commit_bytes: Some(1000),
        os_peak_root_working_set_bytes: Some(2000),
    }));
    record_worker_readiness(&mut sample, row_workers, true);
    sample
}

#[test]
fn native_representation_power_gates_timing_without_erasing_answers_or_rollback() {
    let pair = vec![complete_sample(false, 4), complete_sample(true, 4)];
    let rollback = original_pq2_rollback_observation(&pair, 4);
    assert_eq!(comparison(&pair, 4)["performanceComparisonEligible"], true);
    for (sample, endpoint, observation) in [
        (0, "powerBefore", Value::Null),
        (
            1,
            "powerAfter",
            bonsai_cpu_policy::power::unknown_snapshot(),
        ),
        (
            0,
            "powerAfter",
            bonsai_cpu_policy::power::observed_snapshot(None, false, Some(80)),
        ),
        (
            1,
            "powerBefore",
            bonsai_cpu_policy::power::observed_snapshot(Some(false), false, Some(80)),
        ),
        (
            1,
            "powerAfter",
            bonsai_cpu_policy::power::observed_snapshot(Some(true), true, Some(80)),
        ),
    ] {
        let mut changed = pair.clone();
        changed[sample][endpoint] = observation;
        let compared = comparison(&changed, 4);
        assert_eq!(compared["status"], "captured-complete-pair");
        assert_eq!(compared["performanceComparisonEligible"], false);
        assert!(compared["ptq1OverPq2RequestLatency"].is_null());
        assert_eq!(compared["observedUnqualifiedRequestLatencyRatio"], 1.0);
        assert_eq!(original_pq2_rollback_observation(&changed, 4), rollback);
    }
    let mut other_source = pair;
    let battery = bonsai_cpu_policy::power::observed_snapshot(Some(false), false, Some(79));
    other_source[1]["powerBefore"] = battery.clone();
    other_source[1]["powerAfter"] = battery;
    assert_eq!(
        comparison(&other_source, 4)["performanceComparisonEligible"],
        false
    );
}

#[test]
fn native_representation_rollback_observation_survives_pending_ptq1_arm() {
    for row_workers in [1, 4] {
        assert_eq!(
            original_pq2_rollback_observation(&[], row_workers)["reason"],
            "original-pq2-arm-not-started"
        );
        let original = complete_sample(false, row_workers);
        let observed =
            original_pq2_rollback_observation(std::slice::from_ref(&original), row_workers);
        assert_eq!(observed["status"], "captured-original-pq2-arm");
        assert_eq!(observed["independentGateReview"], "pending");
        assert_eq!(observed["defaultMayChange"], false);
        let pending_pair = [original.clone(), Value::Null];
        assert_eq!(
            original_pq2_rollback_observation(&pending_pair, row_workers),
            observed
        );
        assert_eq!(comparison(&pending_pair, row_workers)["status"], "pending");
        for (path, value) in [
            ("/modelIdentity/sha256", json!("0".repeat(64))),
            ("/verifiedReadinessObserved", json!(false)),
            ("/observedNativeRowWorkers", Value::Null),
            ("/stream/finishReason", json!("length")),
            ("/stream/reportedCompletionTokens", json!(0)),
            ("/stream/visibleContentSha256", json!("0".repeat(64))),
            ("/cleanup/ownedJobEmpty", json!(false)),
            ("/cleanup/listenerReleased", json!(false)),
        ] {
            let mut changed = original.clone();
            *changed.pointer_mut(path).unwrap() = value;
            assert_eq!(
                original_pq2_rollback_observation(&[changed], row_workers)["status"],
                "pending",
                "{path}"
            );
        }
        assert_eq!(
            original_pq2_rollback_observation(&[complete_sample(true, row_workers)], row_workers)["status"],
            "pending"
        );
        assert_eq!(
            original_pq2_rollback_observation(&[original], if row_workers == 1 { 4 } else { 1 })["status"],
            "pending"
        );
    }
}

#[test]
fn native_representation_resources_compare_matching_domains_only() {
    for row_workers in [1, 4] {
        let mut pair = vec![
            complete_sample(false, row_workers),
            complete_sample(true, row_workers),
        ];
        for (field, _) in RESOURCE_METRICS {
            let previous = pair[1]["cleanup"]["backendResources"][field]
                .as_u64()
                .unwrap();
            pair[1]["cleanup"]["backendResources"][field] = json!(previous / 2);
        }
        pair[1]["cleanup"]["backendResources"]["successfulGuardSamples"] = json!(3);
        let observed = comparison(&pair, row_workers);
        let resources = &observed["resources"];
        assert_eq!(resources["status"], "observed-bounded-pair");
        let metrics = resources["metrics"].as_array().unwrap();
        assert_eq!(metrics.len(), RESOURCE_METRICS.len());
        for (metric, (field, scope)) in metrics.iter().zip(RESOURCE_METRICS) {
            assert_eq!(metric["field"], field);
            assert_eq!(metric["scope"], scope);
            assert_eq!(
                metric["pq2Bytes"],
                pair[0]["cleanup"]["backendResources"][field]
            );
            assert_eq!(
                metric["ptq1Bytes"],
                pair[1]["cleanup"]["backendResources"][field]
            );
            assert_eq!(metric["ptq1OverPq2"], 0.5);
        }
        let serving = &resources["servingObservationsInRepresentationOrder"];
        assert_eq!(serving[0]["successfulGuardSamples"], 2);
        assert_eq!(serving[1]["successfulGuardSamples"], 3);
        assert_eq!(
            serving[0]["sampledMinimumSystemAvailablePhysicalBytes"],
            4_u64 * 1024 * 1024 * 1024
        );
        for field in [
            "fullResourceGateQualified",
            "pagingQualified",
            "sustainedComfortQualified",
            "resourceBenefitQualified",
        ] {
            assert_eq!(resources[field], false);
        }
        pair[1]["stream"]["reportedCompletionTokens"] = json!(4);
        let unmatched = comparison(&pair, row_workers);
        assert_eq!(unmatched["status"], "captured-complete-pair");
        assert!(unmatched["ptq1OverPq2RequestLatency"].is_null());
        assert_eq!(
            unmatched["resources"]["reason"],
            "different-complete-output-or-usage"
        );
        assert_eq!(unmatched["resources"]["metrics"], json!([]));
    }
}

#[test]
fn native_representation_resources_reject_missing_or_inconsistent_observations() {
    let pair = vec![complete_sample(false, 4), complete_sample(true, 4)];
    assert_eq!(
        resource_comparison(&pair[..1], true)["reason"],
        "incomplete-pair"
    );
    for index in 0..2 {
        let mut changed = pair.clone();
        changed[index]["cleanup"]["backendResources"] = Value::Null;
        let missing = comparison(&changed, 4);
        assert_eq!(missing["status"], "captured-complete-pair");
        assert_eq!(missing["resources"]["status"], "pending");
        assert_eq!(missing["resources"]["metrics"], json!([]));
        for field in RESOURCE_METRICS.iter().map(|(field, _)| *field).chain([
            "successfulGuardSamples",
            "sampledMinimumSystemAvailablePhysicalBytes",
        ]) {
            for invalid in [Value::Null, json!(0), json!(-1), json!(0.5)] {
                let mut changed = pair.clone();
                changed[index]["cleanup"]["backendResources"][field] = invalid;
                let result = comparison(&changed, 4);
                assert_eq!(
                    result["resources"]["reason"], "missing-positive-resource-observations",
                    "{field}"
                );
                assert_eq!(result["resources"]["metrics"], json!([]));
            }
        }
        for (field, invalid) in [
            ("guardIntervalMilliseconds", Value::Null),
            ("guardIntervalMilliseconds", json!(500)),
            ("sampledPeakRootPrivateBytes", json!(1001)),
            ("sampledPeakRootWorkingSetBytes", json!(2001)),
        ] {
            let mut changed = pair.clone();
            changed[index]["cleanup"]["backendResources"][field] = invalid;
            let result = comparison(&changed, 4);
            assert_eq!(result["resources"]["status"], "pending", "{field}");
            assert_eq!(result["resources"]["metrics"], json!([]));
        }
    }
}

#[test]
fn native_representation_comparison_requires_released_worker_observation() {
    for row_workers in [1, 4] {
        let pair = vec![
            complete_sample(false, row_workers),
            complete_sample(true, row_workers),
        ];
        for (field, mismatched) in [
            ("rowWorkerReadinessVersion", json!("unreviewed-release")),
            (
                "rowWorkerCountIndependentlyObserved",
                json!(
                    !pair[0]["rowWorkerCountIndependentlyObserved"]
                        .as_bool()
                        .unwrap()
                ),
            ),
            (
                "observedNativeRowWorkers",
                json!(if row_workers == 1 { 4 } else { 1 }),
            ),
        ] {
            for index in 0..2 {
                for value in [Value::Null, mismatched.clone()] {
                    let mut changed = pair.clone();
                    changed[index][field] = value;
                    assert_eq!(
                        comparison(&changed, row_workers)["reason"],
                        "missing-or-mismatched-worker-readiness-identity",
                        "{field}, sample {index}"
                    );
                    assert!(
                        comparison(&changed, row_workers)["ptq1OverPq2RequestLatency"].is_null()
                    );
                }
            }
        }
    }
}

#[test]
fn native_representation_profile_is_explicit_and_keeps_original_deadline() {
    for (profile, case) in [(PROFILE, CASE), (ROW4_PROFILE, ROW4_CASE)] {
        assert_eq!(profile_case(profile, false, false), Some(case));
        assert_eq!(profile_case(profile, true, false), None);
        assert_eq!(profile_case(profile, false, true), None);
    }
    assert_eq!(body()["max_completion_tokens"], CAP);
    assert_eq!(body()["enable_thinking"], false);
    assert_eq!(body()["abliteration_scale"], 1.3);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        tokio::time::pause();
        let deadline = tokio::time::Instant::now() + STOP_WAIT;
        let mut started = false;
        assert_eq!(
            adapter_work_before_cleanup(deadline, async {
                started = true;
            })
            .await,
            None
        );
        assert!(!started);
        assert_eq!(
            deadline.saturating_duration_since(tokio::time::Instant::now()),
            STOP_WAIT
        );
    });
}

#[test]
fn native_representation_row4_profile_is_explicit_and_preserves_pair() {
    assert_ne!(PROFILE, ROW4_PROFILE);
    assert_ne!(CASE, ROW4_CASE);
    for (single, parallel) in profiles(1).iter().zip(profiles(4).iter()) {
        assert_eq!(
            (single.name, single.case, single.row_workers),
            (PROFILE, CASE, 1)
        );
        assert_eq!(
            (parallel.name, parallel.case, parallel.row_workers),
            (ROW4_PROFILE, ROW4_CASE, 4)
        );
        assert_eq!(single.repacked, parallel.repacked);
        assert!(!parallel.optional_requests && !parallel.required_scale_history);
        let mut expected = adapter_arguments(single, "8081");
        expected.extend(["--native-row-workers", "4"].map(OsString::from));
        let mut actual = adapter_arguments(parallel, "8081");
        assert_eq!(actual, expected);
        assert_eq!(
            actual.iter().any(|arg| arg == "--use-original-pq2"),
            !parallel.repacked
        );
        assert_eq!(
            actual.iter().any(|arg| arg == "--use-repacked-ptq1"),
            parallel.repacked
        );
        actual.extend(
            [
                "--data-directory",
                "C:/model-not-opened",
                "--evidence-parent",
                "C:/evidence-not-opened",
            ]
            .map(OsString::from),
        );
        let config = LaunchConfig::parse(actual).unwrap();
        assert!(config.uses_native_engine() && config.requires_native_sandbox());
        assert_eq!(config.uses_repacked_ptq1(), parallel.repacked);
    }
    assert!(!profiles(4)[0].repacked && profiles(4)[1].repacked);
    assert_eq!(NEXT_OWNER_RESERVE, Duration::from_secs(60));
    assert_eq!(STOP_WAIT, Duration::from_secs(30));
    assert_eq!(CASE_BUDGET, Duration::from_secs(240));
}

#[test]
fn native_representation_profiles_leave_skipped_arms_pending_at_original_deadline() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        tokio::time::pause();
        let evidence = env::temp_dir().join(format!(
            "Bonsai-Representation-Reserve-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&evidence).unwrap();
        let deadline = tokio::time::Instant::now() + NEXT_OWNER_RESERVE + Duration::from_secs(1);
        assert!(can_start_owner(deadline));
        tokio::time::advance(Duration::from_secs(1)).await;
        assert!(!can_start_owner(deadline));
        for row_workers in [1, 4] {
            for samples in [vec![], vec![complete_sample(false, row_workers)]] {
                let mut report = json!({"passed":false,"samples":samples});
                paired_responses(
                    Path::new("C:/model-not-opened"),
                    &evidence,
                    deadline,
                    row_workers,
                    &mut report,
                )
                .await;
                assert_eq!(report["samples"], json!(samples));
                assert_eq!(report["passed"], false);
                assert_eq!(report["comparison"]["status"], "pending");
                assert_eq!(
                    report["pendingReason"],
                    "insufficient-next-owner-and-cleanup-reserve"
                );
                assert_eq!(
                    deadline.saturating_duration_since(tokio::time::Instant::now()),
                    NEXT_OWNER_RESERVE
                );
                let retained: Value =
                    serde_json::from_slice(&fs::read(evidence.join("acceptance.json")).unwrap())
                        .unwrap();
                assert_eq!(retained, report);
            }
        }
        fs::remove_file(evidence.join("acceptance.json")).unwrap();
        fs::remove_dir(evidence).unwrap();
    });
}

#[test]
fn native_representation_complete_bytes_survive_expired_second_request() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        timeout(Duration::from_secs(10), async {
            let evidence = env::temp_dir().join(format!("Bonsai-Representation-Partial-{}-{}",
                std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
            fs::create_dir(&evidence).unwrap();
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = listener.local_addr().unwrap().port();
            let fixture = tokio::spawn(async move {
                for step in 0..2 {
                    let (peer,_) = listener.accept().await.unwrap();
                    let mut peer = BufReader::new(peer);
                    let mut length = None;
                    loop {
                        let line = prefix_line(&mut peer,16*1024).await;
                        if let Some(value) = line.strip_prefix("Content-Length: ") {
                            length = Some(value.trim().parse::<usize>().unwrap());
                        }
                        if line == "\r\n" { break; }
                    }
                    let mut bytes = vec![0;length.unwrap()];
                    peer.read_exact(&mut bytes).await.unwrap();
                    assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(),body());
                    if step == 0 {
                        let payload = concat!(
                            "data: {\"choices\":[{\"delta\":{\"content\":\"cèdre\\n\"},\"finish_reason\":null}]}\n\n",
                            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"completion_tokens\":3}}\n\n",
                            "data: [DONE]\n\n");
                        let response = format!("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{:X}\r\n{payload}\r\n",payload.len());
                        peer.get_mut().write_all(response.as_bytes()).await.unwrap();
                        tokio::time::sleep(Duration::from_millis(20)).await;
                        peer.get_mut().write_all(b"0\r\n\r\n").await.unwrap();
                        peer.get_mut().shutdown().await.unwrap();
                    } else {
                        assert_eq!(peer.read(&mut [0]).await.unwrap(),0);
                    }
                }
            });
            let deadline = tokio::time::Instant::now()+STOP_WAIT+Duration::from_millis(300);
            let mut report = json!({"passed":false,"samples":[{},{}]});
            assert!(record_response(port,"fixture",&evidence,deadline,&mut report,0).await);
            let first: Value = serde_json::from_slice(&fs::read(evidence.join("acceptance.json")).unwrap()).unwrap();
            assert!(!record_response(port,"fixture",&evidence,deadline,&mut report,1).await);
            fixture.await.unwrap();
            let saved: Value = serde_json::from_slice(&fs::read(evidence.join("acceptance.json")).unwrap()).unwrap();
            assert_eq!(saved["samples"][0],first["samples"][0]);
            assert_eq!(saved["samples"][0]["stream"]["visibleContent"],"cèdre\n");
            assert_eq!(saved["samples"][0]["stream"]["visibleContentSha256"],digest("cèdre\n".as_bytes()));
            assert_eq!(saved["samples"][0]["stream"]["reportedCompletionTokens"],3);
            assert_eq!(saved["samples"][0]["stream"]["naturallyStopped"],true);
            assert!(saved["samples"][0]["stream"]["requestMilliseconds"].as_f64().unwrap()>=20.0);
            assert!(saved["samples"][1]["stream"].is_null());
            assert_eq!(saved["pendingReason"],"request-budget-expired");
            assert_eq!(saved["passed"],false);
            assert_eq!(comparison(saved["samples"].as_array().unwrap(),1)["status"],"pending");
            fs::remove_file(evidence.join("acceptance.json")).unwrap();
            fs::remove_dir(evidence).unwrap();
        }).await.unwrap();
    });
}

#[test]
#[ignore = "Actual-model complete-response PQ2/PTQ1 feasibility pair; one bounded main-only device CI job."]
fn pinned_native_ptq1_pq2_capture_complete_responses() {
    capture_complete_responses(1);
}

#[test]
#[ignore = "Actual-model complete-response four-worker PQ2/PTQ1 feasibility pair; one bounded main-only device CI job."]
fn pinned_native_ptq1_pq2_row4_capture_complete_responses() {
    capture_complete_responses(4);
}

fn capture_complete_responses(row_workers: u8) {
    ci_only();
    let [profile, _] = profiles(row_workers);
    let budget = case_budget();
    assert_eq!(
        profile_case(
            &env::var("BONSAI_NATIVE_PROFILE").unwrap(),
            env::var("BONSAI_NATIVE_EXTENDED").as_deref() == Ok("true"),
            env::var("BONSAI_NATIVE_STALLED_CONSUMER").as_deref() == Ok("true")
        ),
        Some(profile.case)
    );
    let data = env::var_os("BONSAI_NATIVE_DATA").unwrap();
    let evidence = env::var_os("BONSAI_NATIVE_EVIDENCE").unwrap();
    let evidence = Path::new(&evidence);
    let mut report = json!({"schemaVersion":1,"case":profile.case,"profile":profile.name,
        "revision":env::var("GITHUB_SHA").unwrap(),"runId":env::var("GITHUB_RUN_ID").unwrap(),
        "runAttempt":env::var("GITHUB_RUN_ATTEMPT").unwrap(),"passed":false,
        "scope":"bounded-single-pair-complete-response-feasibility",
        "request":body(),"requestSha256":digest(&serde_json::to_vec(&body()).unwrap()),
        "representationOrder":["PQ2_0","current-repacked-PTQ1_0"],"configuredNativeRowWorkers":row_workers,
        "configuredSeed":42,"requestedAdapterScale":1.3,"servingStatePolicy":"reset-every-request",
        "templateSha256":"c3cf9e34abf4f9e36c2d72165aa9c132d3e2a725b6c2586aaa3a8af9d7a81041",
        "sampler":{"temperature":1,"topP":0.95,"topK":20,"minP":0,
            "repeatPenalty":1,"presencePenalty":0,"frequencyPenalty":0},
        "metricDefinition":"complete framed request milliseconds including prefill, generation, transport and incremental observer/receipt I/O; startup recorded separately; no decode timing",
        "aggregateModelCaseBudgetSeconds":budget.as_secs(),"maximumModelCaseBudgetSeconds":CASE_BUDGET.as_secs(),
        "wholeJobMaximumSeconds":300,"requiredNativeSandbox":true,"samples":[],"comparison":{"status":"pending"},
        "runtimeLock":serde_json::from_str::<Value>(include_str!("../../../../config/prism-inference.lock.json")).unwrap(),
        "adapterLock":serde_json::from_str::<Value>(include_str!("../../../../config/swift-lora.lock.json")).unwrap(),
        "ptq1ExportReceipt":serde_json::from_str::<Value>(include_str!("../../../../config/swift.ptq1-export.json")).unwrap(),
        "decodeTokensPerSecond":null,"performanceQualified":false,"qualityQualified":false,
        "authoringBasisQualified":false,"completeStateParityQualified":false,"defaultMayChange":false,
        "defaultRuntimeQualified":false,"matrixContinuationPermitted":false});
    report["originalPq2Rollback"] = original_pq2_rollback_observation(&[], row_workers);
    save_acceptance_report(evidence, &report);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(LocalSet::new().run_until(async {
        timeout(
            budget,
            paired_responses(
                Path::new(&data),
                evidence,
                tokio::time::Instant::now() + budget,
                row_workers,
                &mut report,
            ),
        )
        .await
        .unwrap();
    }));
    assert_eq!(
        report["passed"], true,
        "Complete-response feasibility pair remains pending."
    );
}
