//! A bounded complete-response feasibility pair, independent of decode-TPS gates.
#![expect(
    clippy::redundant_pub_crate,
    reason = "The private test module shares profile constants only with its parent harness."
)]

use super::*;

pub(super) const PROFILE: &str = "ptq1-pq2-complete-response";
pub(super) const CASE: &str = "representation::pinned_native_ptq1_pq2_capture_complete_responses";
const CAP: u32 = 32;
const PROMPT: &str = "Name one tree. Answer with only its name.";
const NEXT_OWNER_RESERVE: Duration = Duration::from_secs(60);
const PQ2: AdapterProfile = AdapterProfile {
    name: PROFILE,
    case: CASE,
    repacked: false,
    row_workers: 1,
    optional_requests: false,
    required_scale_history: false,
};

fn body() -> Value {
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

fn model_identity(repacked: bool) -> Value {
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

async fn measured_response(port: u16, credential: &str) -> Value {
    // Parsing and hashing happen after the complete framed connection closes.
    // This timer includes prefill, generation and transport, excluding startup.
    let started = tokio::time::Instant::now();
    let bytes = exchange(port, credential, &body()).await;
    let elapsed = started.elapsed();
    let summary = streamed_response(&bytes);
    let mut observed = stream_observation(&summary, CAP);
    let content = speed::visible_output(&bytes);
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
    let observed = adapter_work_before_cleanup(deadline, measured_response(port, credential)).await;
    let completed = observed.is_some();
    report["samples"][index]["stream"] = observed.unwrap_or(Value::Null);
    if !completed {
        report["pendingReason"] = json!("request-budget-expired");
    }
    save_acceptance_report(evidence, report);
    completed
}

fn comparison(samples: &[Value]) -> Value {
    let pending = |reason| {
        json!({"status":"pending", "reason":reason,
        "ptq1OverPq2RequestLatency":null})
    };
    if samples.len() != 2 {
        return pending("incomplete-pair");
    }
    for (index, sample) in samples.iter().enumerate() {
        let stream = &sample["stream"];
        let expected = model_identity(index == 1);
        let content = stream["visibleContent"].as_str().unwrap_or("");
        if sample["modelIdentity"] != expected
            || sample["configuredNativeRowWorkers"] != 1
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
            return pending("missing-complete-response-identity-timing-or-cleanup");
        }
    }
    // Different complete responses remain available for independent review.
    // A raw latency ratio with different output work would be misleading.
    let matched = samples[0]["stream"]["visibleContent"] == samples[1]["stream"]["visibleContent"]
        && samples[0]["stream"]["reportedCompletionTokens"]
            == samples[1]["stream"]["reportedCompletionTokens"];
    json!({"status":"captured-complete-pair", "matchedOutputAndUsage":matched,
        "ptq1OverPq2RequestLatency":matched.then(|| samples[1]["stream"]["requestMilliseconds"].as_f64().unwrap()
            / samples[0]["stream"]["requestMilliseconds"].as_f64().unwrap()),
        "fixedOrder":true, "qualityReview":"pending", "repeatedTrialBenefitQualified":false})
}

async fn paired_responses(
    data: &Path,
    evidence: &Path,
    deadline: tokio::time::Instant,
    report: &mut Value,
) {
    for profile in [&PQ2, &PTQ1_ADAPTER_PROFILE] {
        // Decide before each startup, charging the first owner and all prior
        // setup against the same deadline. Missing arms may not become jobs.
        if deadline.saturating_duration_since(tokio::time::Instant::now()) <= NEXT_OWNER_RESERVE {
            report["pendingReason"] = json!("insufficient-next-owner-and-cleanup-reserve");
            break;
        }
        let index = report["samples"].as_array().unwrap().len();
        report["samples"].as_array_mut().unwrap().push(json!({
            "modelIdentity":model_identity(profile.repacked), "configuredNativeRowWorkers":1,
            "rowWorkerCountIndependentlyObserved":false, "verifiedReadinessObserved":false,
            "startupMilliseconds":null, "stream":null, "cleanup":null}));
        report["phase"] = json!(format!("arm-{index}-startup"));
        save_acceptance_report(evidence, report);
        let started = tokio::time::Instant::now();
        let Some(owner) =
            adapter_work_before_cleanup(deadline, start_adapter(profile, data, evidence)).await
        else {
            report["pendingReason"] = json!("startup-budget-expired");
            break;
        };
        report["samples"][index]["startupMilliseconds"] =
            json!(started.elapsed().as_secs_f64() * 1000.0);
        report["samples"][index]["verifiedReadinessObserved"] = json!(true);
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
        save_acceptance_report(evidence, report);
        if expired {
            break;
        }
    }
    report["comparison"] = comparison(report["samples"].as_array().unwrap());
    report["passed"] = json!(report["comparison"]["status"] == "captured-complete-pair");
    report["phase"] = json!("completed");
    save_acceptance_report(evidence, report);
}

#[test]
fn native_representation_pair_requires_completion_identity_and_cleanup() {
    let sample = |repacked| {
        json!({"modelIdentity":model_identity(repacked),
        "configuredNativeRowWorkers":1,"verifiedReadinessObserved":true,
        "stream":{"finishReason":"stop","naturallyStopped":true,"reportedCompletionTokens":3,
            "visibleContent":"Oak","visibleContentSha256":digest(b"Oak"),"requestMilliseconds":1000},
        "cleanup":{"ownedJobEmpty":true,"listenerReleased":true}})
    };
    let pair = vec![sample(false), sample(true)];
    assert_eq!(comparison(&pair)["ptq1OverPq2RequestLatency"], 1.0);
    for (path, value) in [
        ("/stream/finishReason", json!("length")),
        ("/stream/reportedCompletionTokens", Value::Null),
        ("/stream/reportedCompletionTokens", json!(33)),
        ("/stream/visibleContentSha256", json!("0".repeat(64))),
        ("/stream/requestMilliseconds", json!(0)),
        ("/modelIdentity/sha256", json!("0".repeat(64))),
        ("/configuredNativeRowWorkers", json!(4)),
        ("/cleanup/ownedJobEmpty", json!(false)),
        ("/cleanup/listenerReleased", json!(false)),
    ] {
        let mut changed = pair.clone();
        *changed[1].pointer_mut(path).unwrap() = value;
        assert_eq!(comparison(&changed)["status"], "pending", "{path}");
    }
    let mut changed = pair.clone();
    changed[1]["stream"]["visibleContent"] = json!("Elm");
    changed[1]["stream"]["visibleContentSha256"] = json!(digest(b"Elm"));
    let result = comparison(&changed);
    assert_eq!(result["status"], "captured-complete-pair");
    assert!(result["ptq1OverPq2RequestLatency"].is_null());
    assert_eq!(result["qualityReview"], "pending");
    assert_eq!(comparison(&pair[..1])["status"], "pending");
    assert_eq!(
        comparison(&[pair[1].clone(), pair[0].clone()])["status"],
        "pending"
    );
}

#[test]
fn native_representation_profile_is_explicit_and_keeps_original_deadline() {
    assert_eq!(profile_case(PROFILE, false, false), Some(CASE));
    assert_eq!(profile_case(PROFILE, true, false), None);
    assert_eq!(profile_case(PROFILE, false, true), None);
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
            assert_eq!(comparison(saved["samples"].as_array().unwrap())["status"],"pending");
            fs::remove_file(evidence.join("acceptance.json")).unwrap();
            fs::remove_dir(evidence).unwrap();
        }).await.unwrap();
    });
}

#[test]
#[ignore = "Actual-model complete-response PQ2/PTQ1 feasibility pair; one bounded main-only device CI job."]
fn pinned_native_ptq1_pq2_capture_complete_responses() {
    ci_only();
    let budget = case_budget();
    assert_eq!(
        profile_case(
            &env::var("BONSAI_NATIVE_PROFILE").unwrap(),
            env::var("BONSAI_NATIVE_EXTENDED").as_deref() == Ok("true"),
            env::var("BONSAI_NATIVE_STALLED_CONSUMER").as_deref() == Ok("true")
        ),
        Some(CASE)
    );
    let data = env::var_os("BONSAI_NATIVE_DATA").unwrap();
    let evidence = env::var_os("BONSAI_NATIVE_EVIDENCE").unwrap();
    let evidence = Path::new(&evidence);
    let mut report = json!({"schemaVersion":1,"case":CASE,"profile":PROFILE,
        "revision":env::var("GITHUB_SHA").unwrap(),"runId":env::var("GITHUB_RUN_ID").unwrap(),
        "runAttempt":env::var("GITHUB_RUN_ATTEMPT").unwrap(),"passed":false,
        "scope":"bounded-single-pair-complete-response-feasibility",
        "request":body(),"requestSha256":digest(&serde_json::to_vec(&body()).unwrap()),
        "representationOrder":["PQ2_0","current-repacked-PTQ1_0"],"configuredNativeRowWorkers":1,
        "configuredSeed":42,"requestedAdapterScale":1.3,"servingStatePolicy":"reset-every-request",
        "templateSha256":"c3cf9e34abf4f9e36c2d72165aa9c132d3e2a725b6c2586aaa3a8af9d7a81041",
        "sampler":{"temperature":1,"topP":0.95,"topK":20,"minP":0,
            "repeatPenalty":1,"presencePenalty":0,"frequencyPenalty":0},
        "metricDefinition":"complete framed request milliseconds including prefill, generation and transport; startup recorded separately; no decode timing",
        "aggregateModelCaseBudgetSeconds":budget.as_secs(),"maximumModelCaseBudgetSeconds":CASE_BUDGET.as_secs(),
        "wholeJobMaximumSeconds":300,"requiredNativeSandbox":true,"samples":[],"comparison":{"status":"pending"},
        "runtimeLock":serde_json::from_str::<Value>(include_str!("../../../../config/prism-inference.lock.json")).unwrap(),
        "adapterLock":serde_json::from_str::<Value>(include_str!("../../../../config/swift-lora.lock.json")).unwrap(),
        "ptq1ExportReceipt":serde_json::from_str::<Value>(include_str!("../../../../config/swift.ptq1-export.json")).unwrap(),
        "decodeTokensPerSecond":null,"performanceQualified":false,"qualityQualified":false,
        "authoringBasisQualified":false,"completeStateParityQualified":false,"defaultMayChange":false,
        "defaultRuntimeQualified":false,"matrixContinuationPermitted":false});
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
