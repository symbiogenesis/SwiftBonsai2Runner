//! Explicit service integration followed by original-PQ2 rollback in one allowance.
#![expect(
    clippy::redundant_pub_crate,
    reason = "The parent test harness shares only this explicit profile and case."
)]

use super::*;

pub(super) const PROFILE: &str = "ptq1-adapter-service-rollback-row4";
pub(super) const CASE: &str =
    "service_rollback::pinned_native_ptq1_service_and_original_pq2_rollback_row4";
const HASH_RESERVE: Duration = Duration::from_secs(15);
const NEXT_OWNER_RESERVE: Duration = Duration::from_secs(75);
const ROLLBACK_PROFILE: AdapterProfile = AdapterProfile {
    name: PROFILE,
    case: CASE,
    repacked: false,
    row_workers: 4,
    optional_requests: false,
    required_scale_history: false,
};

fn pending_rollback() -> Value {
    json!({"status":"pending","phase":"awaiting-complete-ptq1-service-sequence",
        "gateId":"original-pq2-rollback","independentGateReview":"pending",
        "ownerIndex":2,"explicitRepresentationArgument":"--use-original-pq2",
        "request":representation::body(),
        "requestSha256":adapter_digest(&serde_json::to_vec(&representation::body()).unwrap()),
        "minimumRemainingBeforeOwnerSeconds":NEXT_OWNER_RESERVE.as_secs(),
        "tailReserveWithheldFromPtq1Seconds":NEXT_OWNER_RESERVE.as_secs(),
        "ownedCleanupReserveSeconds":STOP_WAIT.as_secs(),"postCleanupHashReserveSeconds":HASH_RESERVE.as_secs(),
        "sample":null,"responseCapture":null,"retention":null,
        "powerBefore":null,"powerAfter":null,"performanceComparisonQualified":false})
}

fn ptq1_owners_reaped(report: &Value) -> bool {
    report["serviceIntegrationWorkload"]["status"] == "passed"
        && ["initialCleanup", "freshOwnerCleanup"].iter().all(|field| {
            report[*field]["ownedJobEmpty"] == true && report[*field]["listenerReleased"] == true
        })
        && report["freshOwnerCleanup"]["completedOperations"] == 1
        && report["freshOwnerCleanup"]["failedOperations"] == 0
}

fn rollback_verified(rollback: &Value) -> bool {
    let identity = representation::model_identity(false);
    representation::original_pq2_rollback_observation(std::slice::from_ref(&rollback["sample"]), 4)
        ["status"]
        == "captured-original-pq2-arm"
        && service_integration::cleanup_passed(&rollback["sample"]["cleanup"])
        && rollback["sample"]["cleanup"]["completedOperations"] == 1
        && rollback["sample"]["cleanup"]["failedOperations"] == 0
        && rollback["sample"]["cleanup"]["exit"] == "shutdown"
        && rollback["retention"]["status"] == "verified-complete-original-file"
        && rollback["retention"]["bytes"] == identity["bytes"]
        && rollback["retention"]["sha256"] == identity["sha256"]
        && rollback["retention"]["afterOwnedCleanup"] == true
        && rollback["retention"]["completeFileThroughEof"] == true
        && rollback["retention"]["finishedBeforeOriginalDeadline"] == true
        && rollback["retention"]["fileAndAncestorGuardsRetainedThroughFinalReceipt"] == true
}

fn can_start_rollback(deadline: tokio::time::Instant, report: &Value) -> bool {
    ptq1_owners_reaped(report)
        && deadline.saturating_duration_since(tokio::time::Instant::now()) > NEXT_OWNER_RESERVE
}

fn ptq1_service_deadline(deadline: tokio::time::Instant) -> Option<tokio::time::Instant> {
    let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
    // A successful PTQ1 sequence must still admit its required fresh owner.
    // Reject before loading any model if even that existing floor cannot fit
    // after withholding the original-PQ2 startup/request/cleanup/hash tail.
    (remaining > NEXT_OWNER_RESERVE + service_integration::NEXT_OWNER_RESERVE)
        .then(|| deadline - NEXT_OWNER_RESERVE)
}

async fn service_before_rollback(
    data: &Path,
    evidence: &Path,
    deadline: tokio::time::Instant,
    report: &mut Value,
) -> bool {
    let Some(service_deadline) = ptq1_service_deadline(deadline) else {
        report["serviceIntegrationWorkload"]["pendingReason"] =
            json!("insufficient-ptq1-service-and-original-pq2-tail-reserve");
        save_acceptance_report(evidence, report);
        return false;
    };
    // All PTQ1 work and both owned teardowns use this earlier absolute deadline.
    // The third owner retains the original case deadline and its separate
    // thirty-second Stop and fifteen-second complete-file verification reserves.
    service_integration::sequence(data, evidence, service_deadline, report).await
}

fn verify_original_retention(
    data: &Path,
    evidence: &Path,
    deadline: tokio::time::Instant,
    report: &mut Value,
) -> Option<bonsai_artifacts::VerifiedFile> {
    let identity = representation::model_identity(false);
    let path = data
        .join("models")
        .join(identity["revision"].as_str().unwrap())
        .join(identity["filename"].as_str().unwrap());
    let bytes = identity["bytes"].as_u64().unwrap();
    let hash = identity["sha256"].as_str().unwrap();
    report["phase"] = json!("original-pq2-post-cleanup-full-file-verification");
    report["originalPq2Rollback"]["phase"] = report["phase"].clone();
    save_acceptance_report(evidence, report);
    let started = tokio::time::Instant::now();
    let guard = bonsai_artifacts::VerifiedFile::verify_with(&path, bytes, hash, || {
        if tokio::time::Instant::now() < deadline {
            Ok(())
        } else {
            Err(bonsai_artifacts::ArtifactError::Cancelled)
        }
    })
    .ok();
    let verified = guard.is_some() && tokio::time::Instant::now() < deadline;
    report["originalPq2Rollback"]["retention"] = json!({
        "status":if verified {"verified-complete-original-file"} else {"pending"},
        "bytes":identity["bytes"],"sha256":identity["sha256"],
        "afterOwnedCleanup":true,"completeFileThroughEof":verified,
        "finishedBeforeOriginalDeadline":verified,"fileAndAncestorGuardsRetainedThroughFinalReceipt":verified,
        "milliseconds":started.elapsed().as_secs_f64()*1000.0,
        "method":"bonsai-artifacts complete-file length/SHA-256 verification using Windows CNG, with cancellation checks before each bounded read; blocking OS work is not preemptible"});
    save_acceptance_report(evidence, report);
    if verified { guard } else { None }
}

fn response_after_cleanup(bytes: &[u8], elapsed: Duration, cleanup: &Value) -> Option<Value> {
    if cleanup["ownedJobEmpty"] != true || cleanup["listenerReleased"] != true {
        return None;
    }
    // The established assertive parser runs only after every owned model has
    // stopped. A malformed/truncated capture remains pending, retaining cleanup
    // and power observations instead of unwinding through a live owner.
    std::panic::catch_unwind(|| representation::response_observation(bytes, elapsed)).ok()
}

async fn rollback_owner(
    data: &Path,
    evidence: &Path,
    deadline: tokio::time::Instant,
    report: &mut Value,
) -> (bool, Option<bonsai_artifacts::VerifiedFile>) {
    if !can_start_rollback(deadline, report) {
        report["originalPq2Rollback"]["pendingReason"] =
            json!("ptq1-cleanup-or-next-owner-and-post-hash-reserve-missing");
        save_acceptance_report(evidence, report);
        return (false, None);
    }
    let work_deadline = deadline - HASH_RESERVE;
    report["originalPq2Rollback"]["powerBefore"] = power::snapshot();
    report["originalPq2Rollback"]["sample"] = json!({
        "modelIdentity":representation::model_identity(false),"configuredNativeRowWorkers":4,
        "verifiedReadinessObserved":false,"stream":null,"cleanup":null});
    record_worker_readiness(&mut report["originalPq2Rollback"]["sample"], 4, false);
    report["phase"] = json!("original-pq2-rollback-startup");
    report["originalPq2Rollback"]["phase"] = report["phase"].clone();
    save_acceptance_report(evidence, report);
    let progress = Mutex::new(Vec::new());
    let owner = adapter_start_before_cleanup(
        work_deadline,
        evidence,
        report,
        &progress,
        start_adapter_with_progress(&ROLLBACK_PROFILE, data, evidence, |observation| {
            progress.lock().unwrap().push(observation);
        }),
    )
    .await;
    report["originalPq2Rollback"]["startupProgress"] = report["startupProgress"].clone();
    report["originalPq2Rollback"]["startupMilliseconds"] = report["startupMilliseconds"].clone();
    let Some(owner) = owner else {
        report["originalPq2Rollback"]["powerAfter"] = power::snapshot();
        save_acceptance_report(evidence, report);
        return (false, None);
    };
    record_worker_readiness(&mut report["originalPq2Rollback"]["sample"], 4, true);
    let port = owner.port();
    let credential = key(&owner);
    let (stop, stopped) = oneshot::channel();
    let running = spawn_local(owner.run(async { stopped.await.map_err(io::Error::other) }));
    report["phase"] = json!("original-pq2-rollback-complete-tree-response");
    report["originalPq2Rollback"]["phase"] = report["phase"].clone();
    save_acceptance_report(evidence, report);
    let started = tokio::time::Instant::now();
    let response = adapter_work_before_cleanup(
        work_deadline,
        progress::try_exchange(
            port,
            &credential,
            &representation::body(),
            evidence,
            report,
            "original-pq2-rollback",
        ),
    )
    .await;
    let elapsed = started.elapsed();
    let response = response.flatten();
    report["phase"] = json!("original-pq2-rollback-owned-cleanup");
    report["originalPq2Rollback"]["phase"] = report["phase"].clone();
    save_acceptance_report(evidence, report);
    // A failed transport or owner may already have closed broker admission.
    // Preserve failed cleanup and power endpoints without permitting another
    // owner or converting unknown Job emptiness into a successful observation.
    let (cleanup, _) =
        service_integration::stop_owner(stop, running, port, true, work_deadline).await;
    let cleaned = service_integration::cleanup_passed(&cleanup);
    report["originalPq2Rollback"]["sample"]["cleanup"] = cleanup;
    save_acceptance_report(evidence, report);
    if let Some(bytes) = response {
        report["originalPq2Rollback"]["responseBytes"] = json!(bytes.len());
        report["originalPq2Rollback"]["responseSha256"] = json!(adapter_digest(&bytes));
        let stream = response_after_cleanup(
            &bytes,
            elapsed,
            &report["originalPq2Rollback"]["sample"]["cleanup"],
        );
        report["originalPq2Rollback"]["responseParsePassed"] = json!(stream.is_some());
        report["originalPq2Rollback"]["sample"]["stream"] = stream.unwrap_or(Value::Null);
    }
    let sample = &report["originalPq2Rollback"]["sample"];
    report["originalPq2Rollback"]["responseCapture"] =
        representation::original_pq2_rollback_observation(std::slice::from_ref(sample), 4);
    save_acceptance_report(evidence, report);
    // Retain post-cleanup identity even when an available response was truncated;
    // that identity cannot turn an incomplete response into a rollback pass.
    let guard = cleaned
        .then(|| verify_original_retention(data, evidence, deadline, report))
        .flatten();
    report["originalPq2Rollback"]["powerAfter"] = power::snapshot();
    let passed = rollback_verified(&report["originalPq2Rollback"]);
    report["originalPq2Rollback"]["status"] = json!(if passed {
        "captured-rollback-and-retention"
    } else {
        "pending"
    });
    save_acceptance_report(evidence, report);
    (passed, guard)
}

async fn acceptance(
    data: &Path,
    evidence: &Path,
    deadline: tokio::time::Instant,
    report: &mut Value,
) {
    power::begin(report);
    let service = service_before_rollback(data, evidence, deadline, report).await;
    report["serviceIntegration"] = report["serviceIntegrationWorkload"].clone();
    save_acceptance_report(evidence, report);
    let (rollback, guard) = if service {
        rollback_owner(data, evidence, deadline, report).await
    } else {
        (false, None)
    };
    report["phase"] = json!("completed");
    report["serviceIntegrationPassed"] = json!(service);
    report["originalPq2RollbackPassed"] = json!(rollback);
    report["lifecyclePassed"] = json!(service && rollback);
    report["allRequestedWorkloadsPassed"] = json!(service && rollback);
    report["passed"] = json!(service && rollback);
    power::finish(report);
    save_acceptance_report(evidence, report);
    drop(guard);
}

#[test]
fn native_service_rollback_profile_is_separate_and_keeps_explicit_original_selection() {
    assert_eq!(profile_case(PROFILE, false, false), Some(CASE));
    assert_eq!(profile_case(PROFILE, true, false), None);
    assert_eq!(profile_case(PROFILE, false, true), None);
    assert_eq!(
        profile_case("ptq1-adapter-service-rollback", false, false),
        None
    );
    assert_eq!(
        profile_case(service_integration::PROFILE, false, false),
        Some(service_integration::CASE)
    );
    let args = adapter_arguments(&ROLLBACK_PROFILE, "43210");
    for required in [
        "--native-engine",
        "--require-native-sandbox",
        "--use-original-pq2",
        "--use-abliterate-lora",
    ] {
        assert_eq!(args.iter().filter(|arg| *arg == required).count(), 1);
    }
    assert!(!args.iter().any(|arg| arg == "--use-repacked-ptq1"));
    assert_eq!(
        args.array_windows()
            .filter(|[flag, value]| flag == "--native-row-workers" && value == "4")
            .count(),
        1
    );
    assert_eq!(representation::body()["max_completion_tokens"], 32);
    assert_eq!(representation::body()["abliteration_scale"], 1.3);
    assert_eq!(representation::body()["enable_thinking"], false);
    assert_eq!(NEXT_OWNER_RESERVE, Duration::from_mins(1) + HASH_RESERVE);
}

#[test]
fn native_service_rollback_requires_complete_original_response_and_post_cleanup_identity() {
    let mut rollback = pending_rollback();
    assert!(!rollback_verified(&rollback));
    rollback["sample"] = representation::complete_sample(false, 4);
    rollback["sample"]["cleanup"]["completedOperations"] = json!(1);
    rollback["sample"]["cleanup"]["failedOperations"] = json!(0);
    rollback["sample"]["cleanup"]["exit"] = json!("shutdown");
    rollback["sample"]["cleanup"]["status"] = json!("confirmed");
    let model = representation::model_identity(false);
    rollback["retention"] = json!({"status":"verified-complete-original-file","bytes":model["bytes"],
        "sha256":model["sha256"],"afterOwnedCleanup":true,"completeFileThroughEof":true,"finishedBeforeOriginalDeadline":true,
        "fileAndAncestorGuardsRetainedThroughFinalReceipt":true});
    assert!(rollback_verified(&rollback));
    for (path, value) in [
        ("/sample/modelIdentity/sha256", json!("0".repeat(64))),
        ("/sample/observedNativeRowWorkers", json!(1)),
        ("/sample/stream/finishReason", json!("length")),
        ("/sample/stream/reportedCompletionTokens", Value::Null),
        ("/sample/cleanup/ownedJobEmpty", json!(false)),
        ("/sample/cleanup/status", json!("pending")),
        ("/sample/cleanup/listenerReleased", json!(false)),
        ("/sample/cleanup/completedOperations", json!(2)),
        ("/sample/cleanup/failedOperations", json!(1)),
        ("/sample/cleanup/exit", json!("admission-closed")),
        ("/retention/sha256", json!("0".repeat(64))),
        ("/retention/bytes", json!(0)),
        ("/retention/afterOwnedCleanup", json!(false)),
        ("/retention/completeFileThroughEof", json!(false)),
        ("/retention/finishedBeforeOriginalDeadline", json!(false)),
        (
            "/retention/fileAndAncestorGuardsRetainedThroughFinalReceipt",
            json!(false),
        ),
        (
            "/retention/fileAndAncestorGuardsRetainedThroughFinalReceipt",
            Value::Null,
        ),
    ] {
        let mut changed = rollback.clone();
        *changed.pointer_mut(path).unwrap() = value;
        assert!(!rollback_verified(&changed), "{path}");
    }
}

#[test]
fn native_service_rollback_malformed_responses_wait_for_owned_cleanup() {
    let payload = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"Oak\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"completion_tokens\":1}}\n\n",
        "data: [DONE]\n\n"
    );
    let bytes = format!(
        "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{:X}\r\n{payload}\r\n0\r\n\r\n",
        payload.len()
    )
    .into_bytes();
    let cleanup = json!({"ownedJobEmpty":true,"listenerReleased":true});
    assert!(response_after_cleanup(&bytes, Duration::from_millis(1), &cleanup).is_some());
    for field in ["ownedJobEmpty", "listenerReleased"] {
        let mut pending_cleanup = cleanup.clone();
        pending_cleanup[field] = json!(false);
        assert!(
            response_after_cleanup(&bytes, Duration::from_millis(1), &pending_cleanup).is_none()
        );
    }
    for malformed in [
        b"".as_slice(),
        b"HTTP/1.1 503 Unavailable\r\n\r\n",
        &bytes[..bytes.len() - 2],
    ] {
        assert!(response_after_cleanup(malformed, Duration::from_millis(1), &cleanup).is_none());
    }
}

#[test]
fn native_service_rollback_transport_failure_retains_pending_progress() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let evidence = env::temp_dir().join(format!(
            "Bonsai-Rollback-Transport-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&evidence).unwrap();
        let mut report = json!({"passed":false,"originalPq2Rollback":pending_rollback()});
        let response = timeout(
            Duration::from_secs(5),
            progress::try_exchange(
                0,
                "fixture-not-a-model-credential",
                &representation::body(),
                &evidence,
                &mut report,
                "original-pq2-rollback",
            ),
        )
        .await
        .unwrap();
        assert!(response.is_none());
        assert_eq!(report["requestProgress"][0]["transportErrorObserved"], true);
        let saved: Value =
            serde_json::from_slice(&fs::read(evidence.join("acceptance.json")).unwrap()).unwrap();
        assert_eq!(saved["requestProgress"][0]["transportErrorObserved"], true);
        assert_eq!(saved["originalPq2Rollback"]["status"], "pending");
        assert_eq!(saved["passed"], false);
        fs::remove_file(evidence.join("acceptance.json")).unwrap();
        fs::remove_dir(evidence).unwrap();
    });
}

#[test]
fn native_service_rollback_deadline_and_missing_ptq1_cleanup_never_start_an_owner() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .start_paused(true)
        .build()
        .unwrap();
    runtime.block_on(async {
        let evidence = env::temp_dir().join(format!("Bonsai-Service-Rollback-Reserve-{}-{}",std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        fs::create_dir(&evidence).unwrap();
        let mut report = json!({"passed":false,"originalPq2Rollback":pending_rollback(),
            "serviceIntegrationWorkload":{"status":"passed"},
            "initialCleanup":{"ownedJobEmpty":true,"listenerReleased":true},
            "freshOwnerCleanup":{"ownedJobEmpty":true,"listenerReleased":true,"completedOperations":1,"failedOperations":0}});
        let deadline = tokio::time::Instant::now()+NEXT_OWNER_RESERVE;
        assert!(!rollback_owner(Path::new("unused-no-model-start"),&evidence,deadline,&mut report).await.0);
        assert!(report["originalPq2Rollback"]["sample"].is_null());
        assert_eq!(report["originalPq2Rollback"]["status"],"pending");
        for field in ["initialCleanup","freshOwnerCleanup"] {
            let mut changed = report.clone();
            changed[field]["ownedJobEmpty"] = json!(false);
            assert!(!can_start_rollback(deadline+Duration::from_mins(1),&changed));
            changed[field]["ownedJobEmpty"] = json!(true);
            changed[field]["listenerReleased"] = json!(false);
            assert!(!can_start_rollback(deadline+Duration::from_mins(1),&changed));
        }
        let mut pending_service = report.clone();
        pending_service["serviceIntegrationWorkload"]["status"] = json!("pending");
        assert!(!can_start_rollback(deadline+Duration::from_mins(1),&pending_service));
        assert_eq!(deadline-tokio::time::Instant::now(),NEXT_OWNER_RESERVE);
        assert_eq!(report["passed"],false);
        fs::remove_file(evidence.join("acceptance.json")).unwrap();
        fs::remove_dir(evidence).unwrap();
    });
}

#[test]
fn native_service_rollback_reserves_tail_before_ptq1_owner() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap();
    runtime.block_on(async {
        let started = tokio::time::Instant::now();
        let minimum = NEXT_OWNER_RESERVE + service_integration::NEXT_OWNER_RESERVE;
        for allowance in [Duration::ZERO, NEXT_OWNER_RESERVE, minimum] {
            assert!(ptq1_service_deadline(started + allowance).is_none());
        }
        let deadline = started + minimum + Duration::from_secs(2);
        let service_deadline = ptq1_service_deadline(deadline).unwrap();
        assert_eq!(deadline - service_deadline, NEXT_OWNER_RESERVE);
        assert!(
            adapter_work_before_cleanup(service_deadline, pending::<()>())
                .await
                .is_none()
        );
        assert_eq!(
            deadline - tokio::time::Instant::now(),
            NEXT_OWNER_RESERVE + STOP_WAIT
        );

        let evidence = env::temp_dir().join(format!(
            "Bonsai-Service-Rollback-Tail-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&evidence).unwrap();
        let mut report = json!({"passed":false,"originalPq2Rollback":pending_rollback(),
            "serviceIntegrationWorkload":service_integration::pending_workload()});
        assert!(
            !service_before_rollback(
                Path::new("unused-no-model-start"),
                &evidence,
                tokio::time::Instant::now() + minimum,
                &mut report,
            )
            .await
        );
        assert_eq!(report["serviceIntegrationWorkload"]["status"], "pending");
        assert_eq!(
            report["serviceIntegrationWorkload"]["pendingReason"],
            "insufficient-ptq1-service-and-original-pq2-tail-reserve"
        );
        assert!(report["startupProgress"].is_null());
        assert!(report["originalPq2Rollback"]["sample"].is_null());
        assert_eq!(report["passed"], false);
        let saved: Value =
            serde_json::from_slice(&fs::read(evidence.join("acceptance.json")).unwrap()).unwrap();
        assert_eq!(saved, report);
        fs::remove_file(evidence.join("acceptance.json")).unwrap();
        fs::remove_dir(evidence).unwrap();
    });
}

#[test]
fn native_service_rollback_keeps_original_stop_and_hash_reserves() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap();
    runtime.block_on(async {
        let deadline =
            tokio::time::Instant::now() + STOP_WAIT + HASH_RESERVE + Duration::from_secs(2);
        assert!(
            adapter_work_before_cleanup(deadline - HASH_RESERVE, pending::<()>())
                .await
                .is_none()
        );
        assert_eq!(
            deadline - tokio::time::Instant::now(),
            STOP_WAIT + HASH_RESERVE
        );
    });
}

#[test]
#[ignore = "Actual-model service integration plus original-PQ2 rollback; one separately authorized bounded owner CI job."]
fn pinned_native_ptq1_service_and_original_pq2_rollback_row4() {
    ci_only();
    let budget = case_budget();
    assert_eq!(
        profile_case(&env::var("BONSAI_NATIVE_PROFILE").unwrap(), false, false),
        Some(CASE)
    );
    assert_ne!(env::var("BONSAI_NATIVE_EXTENDED").as_deref(), Ok("true"));
    assert_ne!(
        env::var("BONSAI_NATIVE_STALLED_CONSUMER").as_deref(),
        Ok("true")
    );
    let data = env::var_os("BONSAI_NATIVE_DATA").unwrap();
    let evidence = env::var_os("BONSAI_NATIVE_EVIDENCE").unwrap();
    let mut report = json!({"schemaVersion":1,"case":CASE,"profile":PROFILE,
        "revision":env::var("GITHUB_SHA").unwrap(),"runId":env::var("GITHUB_RUN_ID").unwrap(),
        "runAttempt":env::var("GITHUB_RUN_ATTEMPT").unwrap(),"passed":false,"lifecyclePassed":false,
        "allRequestedWorkloadsPassed":false,"requiredNativeSandbox":true,"wholeJobMaximumSeconds":whole_job_budget().as_secs(),
        "aggregateModelCaseBudgetSeconds":budget.as_secs(),"maximumModelCaseBudgetSeconds":maximum_case_budget().as_secs(),
        "configuredNativeRowWorkers":4,"servingStatePolicy":"reset-every-request",
        "weightRepresentation":"current-repacked-PTQ1_0","originalPq2Rollback":pending_rollback(),
        "serviceIntegration":{"status":"pending"},
        "requestedPtq1AdapterScales":[1.3,-0.0,2.0,1.3,1.3],"requestedPtq1CompletionCaps":[1,1,1,4096,1],
        "initialMessages":service_integration::body(1)["messages"],
        "initialRequestSha256":adapter_digest(&serde_json::to_vec(&service_integration::body(1)).unwrap()),
        "scaleBoundaryWorkload":adapter_boundaries::pending_workload(),
        "serviceIntegrationWorkload":service_integration::pending_workload(),
        "defaultRuntimeQualified":false,"releasedServiceGateQualified":false,
        "completeStateParityQualified":false,"authoringBasisQualified":false,
        "performanceQualified":false,"qualityQualified":false,"numericalParityQualified":false});
    report["runtimeLock"] =
        serde_json::from_str::<Value>(include_str!("../../../../config/prism-inference.lock.json"))
            .unwrap();
    assert!(
        reviewed_memory_context_release(
            report["runtimeLock"]["version"].as_str(),
            report["runtimeLock"]["commit"].as_str(),
        ) || (report["runtimeLock"]["version"] == "0.1.25"
            && report["runtimeLock"]["commit"] == "99bab71270b1e6a6fb6d47eec2e34b8097cdcd4c")
    );
    report["adapterLock"] =
        serde_json::from_str::<Value>(include_str!("../../../../config/swift-lora.lock.json"))
            .unwrap();
    report["modelExportReceipt"] =
        serde_json::from_str::<Value>(include_str!("../../../../config/swift.ptq1-export.json"))
            .unwrap();
    save_acceptance_report(Path::new(&evidence), &report);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(LocalSet::new().run_until(async {
        let deadline = tokio::time::Instant::now() + budget;
        tokio::time::timeout_at(
            deadline,
            acceptance(
                Path::new(&data),
                Path::new(&evidence),
                deadline,
                &mut report,
            ),
        )
        .await
        .unwrap();
    }));
    assert_eq!(
        report["passed"], true,
        "Service integration or same-release original-PQ2 rollback remains pending."
    );
}
