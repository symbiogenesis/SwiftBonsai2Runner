//! One explicitly owner-authorized short capture; never ordinary-suite model work.

use super::*;

const PROFILE: &str = "pq2-ptq1-token-timing-row4";
const CASE: &str = "token_timing::pinned_native_row4_captures_separated_token_timings";
const CAP: u32 = 8;
const NEXT_OWNER_RESERVE: Duration = Duration::from_secs(60);

fn body() -> Value {
    let mut body = request(
        json!([{"role":"user","content":"Count from 1 to 100."}]),
        true,
        CAP,
    );
    body["enable_thinking"] = json!(false);
    body["abliteration_scale"] = json!(1.3);
    body
}

fn metrics(progress: &Value) -> Value {
    let mut measured = json!({"status":"pending","timeToFirstTokenSeconds":null,
        "generationTokensPerSecond":null,"generationIntervalSeconds":null,
        "generationIntervalTokens":null,"reportedCompletionTokens":progress["reportedCompletionTokens"],
        "visibleContent":null,"tokenBoundaryEvidence":"not-established"});
    let Some(deltas) = progress["contentDeltas"].as_array() else {
        return measured;
    };
    let mut content = String::new();
    for delta in deltas {
        let Some(text) = delta["text"].as_str() else {
            return measured;
        };
        if text.is_empty() || !text.is_ascii() {
            return measured;
        }
        content.push_str(text);
    }
    measured["visibleContent"] = json!(content);
    // Exact pinned server source uses complete_observed -> generate_sampled_until:
    // one callback for each committed non-stop token; usage counts callbacks.
    // ASCII cannot be buffered as incomplete UTF-8. Text shorter than the server's
    // 1024-byte chunk bound is not split. These independent source conditions,
    // together with reported usage, establish one content event per token here.
    // SSE event counts alone never establish token counts for other responses.
    if progress["reportedCompletionTokens"] != CAP
        || progress["terminalReasonObserved"] != "length"
        || progress["reasoningContentObserved"] != false
        || progress["framingRejected"] != false
        || progress["doneMarkerObserved"] != true
        || progress["finalChunkObserved"] != true
        || progress["connectionClosed"] != true
        || progress["cancelledBeforeConnectionClose"] != false
        || deltas.len() != usize::try_from(CAP).unwrap()
        || content.len() >= 1024
    {
        return measured;
    }
    let Some(first) = deltas
        .first()
        .and_then(|delta| delta["milliseconds"].as_f64())
    else {
        return measured;
    };
    let Some(last) = deltas
        .last()
        .and_then(|delta| delta["milliseconds"].as_f64())
    else {
        return measured;
    };
    let mut previous = first;
    for delta in deltas {
        let Some(time) = delta["milliseconds"].as_f64() else {
            return measured;
        };
        if !time.is_finite() || time < previous {
            return measured;
        }
        previous = time;
    }
    if !first.is_finite() || first <= 0.0 || last <= first {
        return measured;
    }
    let interval = (last - first) / 1000.0;
    let rate = f64::from(CAP - 1) / interval;
    if !rate.is_finite() || rate <= 0.0 {
        return measured;
    }
    measured["status"] = json!("measured-short-window");
    measured["timeToFirstTokenSeconds"] = json!(first / 1000.0);
    measured["generationTokensPerSecond"] = json!(rate);
    measured["generationIntervalSeconds"] = json!(interval);
    measured["generationIntervalTokens"] = json!(CAP - 1);
    measured["tokenBoundaryEvidence"] = json!(
        "reviewed-pinned-nonspeculative-per-committed-token-callback; ASCII-only; below-1024-byte-split-bound; eight-nonempty-deltas-match-eight-reported-tokens"
    );
    measured
}

fn sample_measured(sample: &Value) -> bool {
    sample["measurement"]["status"] == "measured-short-window"
        && sample["verifiedReadinessObserved"] == true
        && sample["observedNativeRowWorkers"] == 4
        && service_integration::cleanup_passed(&sample["cleanup"])
}

async fn paired_capture(
    data: &Path,
    evidence: &Path,
    deadline: tokio::time::Instant,
    report: &mut Value,
) {
    power::begin(report);
    for repacked in [false, true] {
        if deadline.saturating_duration_since(tokio::time::Instant::now()) <= NEXT_OWNER_RESERVE {
            report["pendingReason"] = json!("insufficient-next-owner-and-cleanup-reserve");
            break;
        }
        let profile = AdapterProfile {
            name: PROFILE,
            case: CASE,
            repacked,
            row_workers: 4,
            optional_requests: false,
            required_scale_history: false,
        };
        let index = report["samples"].as_array().unwrap().len();
        report["samples"].as_array_mut().unwrap().push(json!({
            "modelIdentity":representation::model_identity(repacked),
            "configuredNativeRowWorkers":4,"verifiedReadinessObserved":false,
            "startupMilliseconds":null,"requestProgress":null,"measurement":null,"cleanup":null,
            "effectiveAccelerationBackend":null,
            "accelerationObservation":"effective backend not exposed by authenticated readiness; configured NPU policy is not an execution observation"}));
        power::begin(&mut report["samples"][index]);
        report["phase"] = json!(format!("arm-{index}-startup"));
        save_acceptance_report(evidence, report);
        let startup_progress = Mutex::new(Vec::new());
        let owner = adapter_start_before_cleanup(
            deadline,
            evidence,
            &mut report["samples"][index],
            &startup_progress,
            start_adapter_with_progress(&profile, data, evidence, |progress| {
                startup_progress.lock().unwrap().push(progress);
            }),
        )
        .await;
        let Some(owner) = owner else {
            report["pendingReason"] = json!("owner-startup-failed-or-expired");
            power::finish(&mut report["samples"][index]);
            save_acceptance_report(evidence, report);
            break;
        };
        record_worker_readiness(&mut report["samples"][index], 4, true);
        let port = owner.port();
        let credential = key(&owner);
        let (stop, stopped) = oneshot::channel();
        let running = spawn_local(owner.run(async { stopped.await.map_err(io::Error::other) }));
        report["phase"] = json!(format!("arm-{index}-request"));
        save_acceptance_report(evidence, report);
        let outcome = adapter_work_before_cleanup(
            deadline,
            progress::try_exchange(
                port,
                &credential,
                &body(),
                evidence,
                report,
                &format!("token-timing-arm-{index}"),
            ),
        )
        .await;
        let completed = outcome.is_some_and(|bytes| bytes.is_some());
        report["samples"][index]["requestProgress"] = report["requestProgress"]
            .as_array()
            .and_then(|observations| observations.last())
            .cloned()
            .unwrap_or(Value::Null);
        report["phase"] = json!(format!("arm-{index}-cleanup"));
        save_acceptance_report(evidence, report);
        let (cleanup, _) =
            service_integration::stop_owner(stop, running, port, !completed, deadline).await;
        report["samples"][index]["cleanup"] = cleanup;
        power::finish(&mut report["samples"][index]);
        report["samples"][index]["measurement"] =
            metrics(&report["samples"][index]["requestProgress"]);
        let confirmed = service_integration::cleanup_passed(&report["samples"][index]["cleanup"]);
        save_acceptance_report(evidence, report);
        if !completed || !confirmed {
            report["pendingReason"] = json!("request-incomplete-or-cleanup-unconfirmed");
            break;
        }
    }
    power::finish(report);
    let samples = report["samples"].as_array().unwrap();
    let passed = samples.len() == 2 && samples.iter().all(sample_measured);
    let power_conditions = bonsai_cpu_policy::power::compare_samples(samples);
    report["samplePowerConditions"] = power_conditions;
    report["passed"] = json!(passed);
    report["phase"] = json!(if passed { "measured" } else { "pending" });
    save_acceptance_report(evidence, report);
}

#[test]
fn native_token_timing_requires_source_supported_boundaries_and_positive_interval() {
    let mut progress = json!({"contentDeltas":[],"reportedCompletionTokens":8,
        "terminalReasonObserved":"length","reasoningContentObserved":false,
        "framingRejected":false,"doneMarkerObserved":true,"finalChunkObserved":true,
        "connectionClosed":true,"cancelledBeforeConnectionClose":false});
    for index in 0_u32..CAP {
        progress["contentDeltas"].as_array_mut().unwrap().push(
            json!({"text":index.to_string(),"milliseconds":f64::from(index).mul_add(500.0, 1000.0)}),
        );
    }
    let measured = metrics(&progress);
    assert_eq!(measured["timeToFirstTokenSeconds"], 1.0);
    assert_eq!(measured["generationTokensPerSecond"], 2.0);
    for (field, value) in [
        ("reportedCompletionTokens", json!(9)),
        ("reasoningContentObserved", json!(true)),
        ("connectionClosed", json!(false)),
        ("doneMarkerObserved", json!(false)),
        ("finalChunkObserved", json!(false)),
        ("framingRejected", json!(true)),
        ("terminalReasonObserved", json!("stop")),
    ] {
        let mut changed = progress.clone();
        changed[field] = value;
        assert!(metrics(&changed)["generationTokensPerSecond"].is_null());
    }
    let mut coalesced = progress.clone();
    for delta in coalesced["contentDeltas"].as_array_mut().unwrap() {
        delta["milliseconds"] = json!(1000.0);
    }
    assert!(metrics(&coalesced)["generationTokensPerSecond"].is_null());
    progress["contentDeltas"][0]["text"] = json!("é");
    assert!(metrics(&progress)["timeToFirstTokenSeconds"].is_null());
}

#[test]
#[ignore = "Exactly two short row4 model requests, only after explicit owner authorization and one aggregate budget."]
fn pinned_native_row4_captures_separated_token_timings() {
    assert_eq!(
        env::var("BONSAI_NATIVE_OWNER_TIMING").as_deref(),
        Ok("true")
    );
    assert_eq!(env::consts::ARCH, "aarch64");
    assert_eq!(env::var("BONSAI_NATIVE_PROFILE").as_deref(), Ok(PROFILE));
    assert_ne!(env::var("BONSAI_NATIVE_EXTENDED").as_deref(), Ok("true"));
    assert_ne!(
        env::var("BONSAI_NATIVE_STALLED_CONSUMER").as_deref(),
        Ok("true")
    );
    let revision = env::var("BONSAI_NATIVE_OWNER_REVISION").unwrap();
    assert_eq!(revision.len(), 40);
    assert!(revision.bytes().all(|byte| byte.is_ascii_hexdigit()));
    let runtime_lock: Value =
        serde_json::from_str(include_str!("../../../../config/prism-inference.lock.json")).unwrap();
    assert!(matches!(
        (
            runtime_lock["version"].as_str(),
            runtime_lock["commit"].as_str()
        ),
        (
            Some("0.1.22"),
            Some("6b4f4520b465e5feb665a71af64e7b0e21eb750a")
        ) | (
            Some("0.1.23"),
            Some("055c7430cb4771dfddb24ddaa30494af6ad182da")
        )
    ));
    let budget = case_budget();
    let data = env::var_os("BONSAI_NATIVE_DATA").unwrap();
    let evidence = env::var_os("BONSAI_NATIVE_EVIDENCE").unwrap();
    let evidence = Path::new(&evidence);
    let mut report = json!({"schemaVersion":1,"case":CASE,"profile":PROFILE,
        "revision":revision,"runId":env::var("BONSAI_NATIVE_OWNER_RUN_ID").unwrap(),
        "executionScope":"explicitly-authorized-local-owner-capture; no CI acceptance claimed",
        "passed":false,"samples":[],"runtimeLock":runtime_lock,
        "adapterLock":serde_json::from_str::<Value>(include_str!("../../../../config/swift-lora.lock.json")).unwrap(),
        "request":body(),"configuredSeed":42,"requestedAdapterScale":1.3,
        "aggregateModelCaseBudgetSeconds":budget.as_secs(),
        "wholeJobMaximumSeconds":whole_job_budget().as_secs(),
        "maximumModelCaseBudgetSeconds":maximum_case_budget().as_secs(),
        "requiredNativeSandbox":true,"representationOrder":["PQ2_0","current-repacked-PTQ1_0"],
        "metricScope":"Client-observed public broker HTTP/SSE; first-request TTFT begins after verified HTTP readiness and may include remaining asynchronous model/device preparation; generation (N-1)/(last-first) excludes TTFT; receive coalescing and observer receipt I/O are included; not isolated kernel decode or sustained speed",
        "powerScope":"case and per-arm endpoints; enclosing owner must separately retain aggregate pre-build/post-retention observations; endpoints do not prove uninterrupted stability",
        "effectiveAccelerationBackend":null,
        "defaultPq2AccelerationPolicy":"attempt default Snapdragon X2 native-q2 with automatic CPU fallback when unavailable; effective execution unobserved",
        "performanceQualified":false,"ciAcceptanceQualified":false,"qualityQualified":false,
        "completeStateParityQualified":false,"defaultRuntimeQualified":false});
    save_acceptance_report(evidence, &report);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(LocalSet::new().run_until(async {
        let deadline = tokio::time::Instant::now() + budget;
        paired_capture(Path::new(&data), evidence, deadline, &mut report).await;
    }));
    assert_eq!(
        report["passed"], true,
        "Short token timing remains pending."
    );
}
