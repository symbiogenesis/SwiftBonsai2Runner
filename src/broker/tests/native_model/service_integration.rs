//! One released-service workload combining scale boundaries, Stop and fresh ownership.
#![expect(
    clippy::redundant_pub_crate,
    reason = "The parent harness shares only explicit profile constants and bounded helpers."
)]

use super::*;

pub(super) const PROFILE: &str = "ptq1-adapter-service-row4";
pub(super) const CASE: &str =
    "service_integration::pinned_native_ptq1_adapter_row4_checks_released_service_integration";
pub(super) const ADAPTER_PROFILE: AdapterProfile = AdapterProfile {
    name: PROFILE,
    case: CASE,
    repacked: true,
    row_workers: 4,
    optional_requests: false,
    required_scale_history: false,
};

const NEXT_OWNER_RESERVE: Duration = Duration::from_secs(60);

fn body(cap: u32) -> Value {
    let mut body = request(adapter_messages(&ADAPTER_PROFILE, false), true, cap);
    body["enable_thinking"] = json!(false);
    body["abliteration_scale"] = json!(1.3);
    body
}

fn pending_workload() -> Value {
    json!({"status":"pending","phase":"initial-owner-startup",
        "cancellationRequest":body(EXTENDED_COMPLETION_CAP),
        "cancellationRequestSha256":adapter_digest(&serde_json::to_vec(&body(EXTENDED_COMPLETION_CAP)).unwrap()),
        "cancellationRequestMessagesSha256":adapter_digest(&serde_json::to_vec(&body(EXTENDED_COMPLETION_CAP)["messages"]).unwrap()),
        "cancellationRequestedScaleF64Bits":format!("{:016x}",1.3_f64.to_bits()),
        "cancellationRequestBoundToBaseline":false,
        "cancellationPrefixMatchedBaseline":false,"terminalObservedBeforeStop":false,
        "brokerOperationInterrupted":false,"initialOwnerCleanupPassed":false,
        "freshOwnerReadinessPassed":false,"freshCredentialsDiffer":false,
        "freshOwnerReplayMatched":false,"freshOwnerCleanupPassed":false,
        "backendGeneratingAtStopKnown":false,"completeStateParityQualified":false,
        "serverPerRequestScaleAcknowledgementObserved":false,
        "canonicalCountGenerationWireCaptureObserved":false,
        "canonicalCountGenerationContract":{
            "scope":"Source-bound broker admission counts and forwards the same retained canonical body; released generation and context rejection exercise the contract, without a private wire capture or independent numerical adapter-effect verdict.",
            "source":["src/broker/src/supervisor.rs","src/broker/src/backend.rs","src/broker/src/service.rs"],
            "hostedRegression":"pinned_abliteration_scale_is_identical_for_counting_and_generation"},
        "minimumRemainingBeforeFreshOwnerSeconds":NEXT_OWNER_RESERVE.as_secs(),
        "ownedCleanupReserveSeconds":STOP_WAIT.as_secs()})
}

fn prefix_matches(first: &Value, text: &str) -> bool {
    first["passed"] == true
        && !text.is_empty()
        && first["visibleContent"] == text
        && first["visibleContentBytes"] == text.len()
        && first["visibleContentSha256"] == adapter_digest(text.as_bytes())
        && first["requestedScaleF64Bits"] == format!("{:016x}", 1.3_f64.to_bits())
        && first["requestedCompletionCap"] == 1
        && first["reportedCompletionTokens"] == 1
        && first["requestMessagesSha256"]
            == adapter_digest(&serde_json::to_vec(&body(1)["messages"]).unwrap())
}

fn cancellation_request_matches_baseline(first: &Value) -> bool {
    first["passed"] == true
        && first["requestSha256"] == adapter_digest(&serde_json::to_vec(&body(1)).unwrap())
        && first["requestMessagesSha256"]
            == adapter_digest(
                &serde_json::to_vec(&body(EXTENDED_COMPLETION_CAP)["messages"]).unwrap(),
            )
        && first["requestedScaleF64Bits"] == format!("{:016x}", 1.3_f64.to_bits())
        && first["requestedCompletionCap"] == 1
}

pub(super) fn reported_prompt_tokens(bytes: &[u8]) -> Option<u64> {
    let payload = stream_payload(bytes);
    let payload = std::str::from_utf8(&payload).unwrap();
    let mut observed = None;
    for event in payload
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
    {
        if let Ok(event) = serde_json::from_str::<Value>(event)
            && let Some(tokens) = event["usage"]["prompt_tokens"].as_u64()
        {
            assert!(observed.replace(tokens).is_none());
        }
    }
    observed
}

fn prompt_counts_match(streams: &Value, replay: Option<&Value>) -> bool {
    let Some(streams) = streams.as_array().filter(|streams| streams.len() == 3) else {
        return false;
    };
    let Some(tokens) = streams[0]["reportedPromptTokens"]
        .as_u64()
        .filter(|tokens| (1..=8192).contains(tokens))
    else {
        return false;
    };
    streams
        .iter()
        .all(|stream| stream["reportedPromptTokens"] == tokens)
        && replay.is_none_or(|replay| replay["reportedPromptTokens"] == tokens)
}

fn cancellation_counters_match(report: &RunReport) -> bool {
    report.exit == RunExit::Shutdown && report.completed == 3 && report.failed == 3
}

fn complete_prefix(payload: &[u8]) -> (Option<String>, bool) {
    let mut record = Vec::new();
    let mut content = None;
    let mut terminal = false;
    for byte in payload {
        record.push(*byte);
        if !record.ends_with(b"\n\n") && !record.ends_with(b"\r\n\r\n") {
            continue;
        }
        let text = std::str::from_utf8(&record).unwrap();
        let mut data_lines = text.lines().filter_map(|line| line.strip_prefix("data: "));
        if let Some(data) = data_lines.next() {
            assert!(data_lines.next().is_none());
            if data == "[DONE]" {
                terminal = true;
            } else {
                let event: Value = serde_json::from_str(data).unwrap();
                assert!(event.get("error").is_none());
                if let Some(choices) = event["choices"].as_array() {
                    for choice in choices {
                        terminal |= !choice["finish_reason"].is_null();
                        if content.is_none()
                            && let Some(text) = choice["delta"]["content"]
                                .as_str()
                                .filter(|text| !text.is_empty())
                        {
                            content = Some(text.to_owned());
                        }
                    }
                }
            }
        }
        record.clear();
    }
    (content, terminal)
}

async fn cancellation_prefix(client: TcpStream) -> (BufReader<TcpStream>, Option<String>) {
    let mut reader = BufReader::new(client);
    let mut header = String::new();
    loop {
        let line = prefix_line(&mut reader, 16 * 1024 - header.len()).await;
        header.push_str(&line);
        if line == "\r\n" {
            break;
        }
    }
    let (head, _) = response(header.as_bytes(), "HTTP/1.1 200 OK");
    for required in [
        "Content-Type: text/event-stream",
        "Transfer-Encoding: chunked",
    ] {
        assert!(head.lines().any(|line| line.eq_ignore_ascii_case(required)));
    }
    let mut payload = Vec::new();
    loop {
        let line = prefix_line(&mut reader, 32).await;
        let length = usize::from_str_radix(line.trim_end(), 16).unwrap();
        assert!(length > 0 && payload.len() + length <= 64 * 1024);
        let first = payload.len();
        payload.resize(first + length, 0);
        reader.read_exact(&mut payload[first..]).await.unwrap();
        let mut ending = [0; 2];
        reader.read_exact(&mut ending).await.unwrap();
        assert_eq!(&ending, b"\r\n");
        // Inspect every complete SSE record already received, including records after
        // the first visible delta. A terminal event in this chunk cannot pass
        // merely because its visible content preceded that terminal event.
        let (content, terminal) = complete_prefix(&payload);
        if terminal {
            return (reader, None);
        }
        if let Some(content) = content {
            return (reader, Some(content));
        }
    }
}

async fn start_before_deadline(
    data: &Path,
    evidence: &Path,
    deadline: tokio::time::Instant,
    report: &mut Value,
    label: &str,
) -> Option<LaunchedService> {
    record_worker_readiness(report, 4, false);
    report["phase"] = json!(label);
    report["serviceIntegrationWorkload"]["phase"] = json!(label);
    save_acceptance_report(evidence, report);
    let progress = Mutex::new(Vec::new());
    let owner = adapter_start_before_cleanup(
        deadline,
        evidence,
        report,
        &progress,
        start_adapter_with_progress(&ADAPTER_PROFILE, data, evidence, |observation| {
            progress.lock().unwrap().push(observation);
        }),
    )
    .await;
    record_worker_readiness(report, 4, owner.is_some());
    report[label] = json!({"startupMilliseconds":report["startupMilliseconds"],
        "startupProgress":report["startupProgress"],
        "verifiedReadinessObserved":report["verifiedReadinessObserved"],
        "observedNativeRowWorkers":report["observedNativeRowWorkers"],
        "expectedAdapterSha256":report["adapterLock"]["adapter"]["sha256"],
        "expectedWeightRepresentation":"current-repacked-PTQ1_0",
        "scope":"Production launcher independently verifies exact model, adapter, runtime, template, scale capability/range and effective row-worker readiness before returning ownership."});
    save_acceptance_report(evidence, report);
    owner
}

async fn initial_owner(
    data: &Path,
    evidence: &Path,
    deadline: tokio::time::Instant,
    report: &mut Value,
) -> Option<String> {
    report["initialOwnerPowerBefore"] = report["powerBefore"].clone();
    report["initialOwnerPowerAfter"] = Value::Null;
    let Some(owner) =
        start_before_deadline(data, evidence, deadline, report, "initialOwnerReadiness").await
    else {
        report["initialOwnerPowerAfter"] = power::snapshot();
        return None;
    };
    report["adapterEnabled"] = json!(true);
    begin_adapter_requests(&ADAPTER_PROFILE, evidence, report);
    let port = owner.port();
    let credential = key(&owner);
    let (stop, stopped) = oneshot::channel();
    let running = spawn_local(owner.run(async { stopped.await.map_err(io::Error::other) }));
    let boundaries = adapter_work_before_cleanup(
        deadline,
        adapter_requests(
            &ADAPTER_PROFILE,
            port,
            &credential,
            evidence,
            deadline,
            report,
        ),
    )
    .await
    .unwrap_or_else(|| {
        adapter_request_budget_expired(&ADAPTER_PROFILE, evidence, report);
        false
    });
    let prefix = if boundaries
        && deadline.saturating_duration_since(tokio::time::Instant::now()) > NEXT_OWNER_RESERVE
    {
        report["phase"] = json!("scale-1.3-first-prefix-and-explicit-stop");
        report["serviceIntegrationWorkload"]["phase"] = report["phase"].clone();
        save_acceptance_report(evidence, report);
        adapter_work_before_cleanup(deadline, async {
            let client = connect(port, &credential, &body(EXTENDED_COMPLETION_CAP)).await;
            cancellation_prefix(client).await
        })
        .await
    } else {
        None
    };
    if let Some((_, text)) = &prefix {
        report["serviceIntegrationWorkload"]["terminalObservedBeforeStop"] = json!(text.is_none());
    }
    if let Some((_, Some(text))) = &prefix {
        report["serviceIntegrationWorkload"]["observedCancellationPrefix"] = json!(text);
        report["serviceIntegrationWorkload"]["observedCancellationPrefixSha256"] =
            json!(adapter_digest(text.as_bytes()));
        report["serviceIntegrationWorkload"]["cancellationPrefixMatchedBaseline"] =
            json!(prefix_matches(&report["streams"][0], text));
        // The same owner retains MT19937 draw order across requests. Prefix
        // equality is informational; exact output equality is required only
        // after fresh ownership resets the seed for the baseline replay.
        report["serviceIntegrationWorkload"]["cancellationRequestBoundToBaseline"] =
            json!(!text.is_empty() && cancellation_request_matches_baseline(&report["streams"][0]));
        save_acceptance_report(evidence, report);
    }
    report["phase"] = json!("initial-owner-owned-stop");
    save_acceptance_report(evidence, report);
    let sent = stop.send(());
    assert!(sent.is_ok() || prefix.is_none());
    let stopped_report = timeout(STOP_WAIT, running).await.unwrap().unwrap().unwrap();
    assert!(
        stopped_report.exit == RunExit::Shutdown
            || (prefix.is_none() && stopped_report.exit == RunExit::AdmissionClosed)
    );
    assert!(TcpStream::connect(("127.0.0.1", port)).await.is_err());
    report["initialCleanup"] = cleanup_observation(&stopped_report, stopped_report.exit);
    report["serviceIntegrationWorkload"]["initialOwnerCleanupPassed"] = json!(true);
    report["initialOwnerPowerAfter"] = power::snapshot();
    save_acceptance_report(evidence, report);
    if let Some((client, text)) = prefix {
        // Three completed scale streams and two failed rejection controls must
        // precede exactly one failed interrupted operation. Natural completion
        // increments completed instead and fails these independent counters.
        let interrupted = cancellation_counters_match(&stopped_report)
            && text.is_some_and(|text| !text.is_empty());
        drop(client);
        report["serviceIntegrationWorkload"]["brokerOperationInterrupted"] = json!(interrupted);
    }
    let passed = boundaries
        && prompt_counts_match(&report["streams"], None)
        && report["serviceIntegrationWorkload"]["cancellationRequestBoundToBaseline"] == true
        && report["serviceIntegrationWorkload"]["brokerOperationInterrupted"] == true;
    save_acceptance_report(evidence, report);
    passed.then_some(credential)
}

async fn fresh_owner(
    data: &Path,
    evidence: &Path,
    deadline: tokio::time::Instant,
    previous_credential: &str,
    report: &mut Value,
) -> bool {
    if deadline.saturating_duration_since(tokio::time::Instant::now()) <= NEXT_OWNER_RESERVE {
        report["serviceIntegrationWorkload"]["pendingReason"] =
            json!("insufficient-fresh-owner-and-cleanup-reserve");
        save_acceptance_report(evidence, report);
        return false;
    }
    report["freshOwnerPowerBefore"] = power::snapshot();
    report["freshOwnerPowerAfter"] = Value::Null;
    let Some(owner) =
        start_before_deadline(data, evidence, deadline, report, "freshOwnerReadiness").await
    else {
        report["freshOwnerPowerAfter"] = power::snapshot();
        return false;
    };
    report["serviceIntegrationWorkload"]["freshOwnerReadinessPassed"] = json!(true);
    let port = owner.port();
    let credential = key(&owner);
    assert!(credential != previous_credential);
    report["serviceIntegrationWorkload"]["freshCredentialsDiffer"] = json!(true);
    let (stop, stopped) = oneshot::channel();
    let running = spawn_local(owner.run(async { stopped.await.map_err(io::Error::other) }));
    let replay = adapter_work_before_cleanup(
        deadline,
        adapter_stream(
            port,
            &credential,
            &body(1),
            "fresh-owner-scale-1.3-replay",
            evidence,
            report,
        ),
    )
    .await;
    let matched = replay.as_ref().is_some_and(|observed| {
        adapter_boundaries::replay_matches_phase(
            &report["streams"][0],
            observed,
            "fresh-owner-scale-1.3-replay",
        ) && prompt_counts_match(&report["streams"], Some(observed))
    });
    report["freshOwnerStream"] = replay.unwrap_or(Value::Null);
    report["serviceIntegrationWorkload"]["freshOwnerReplayMatched"] = json!(matched);
    report["serviceIntegrationWorkload"]["matchingReportedPromptCounts"] = json!(matched);
    save_acceptance_report(evidence, report);
    report["freshOwnerCleanup"] = adapter_owned_stop(stop, running, port, !matched).await;
    report["serviceIntegrationWorkload"]["freshOwnerCleanupPassed"] = json!(true);
    report["freshOwnerPowerAfter"] = power::snapshot();
    save_acceptance_report(evidence, report);
    matched
        && report["freshOwnerCleanup"]["completedOperations"] == 1
        && report["freshOwnerCleanup"]["failedOperations"] == 0
}

async fn acceptance(data: &Path, evidence: &Path, budget: Duration, report: &mut Value) {
    let deadline = tokio::time::Instant::now() + budget;
    power::begin(report);
    let initial = initial_owner(data, evidence, deadline, report).await;
    let passed = if let Some(credential) = initial {
        fresh_owner(data, evidence, deadline, &credential, report).await
    } else {
        false
    };
    report["phase"] = json!("completed");
    report["serviceIntegrationWorkload"]["status"] =
        json!(if passed { "passed" } else { "pending" });
    report["serviceIntegrationWorkload"]["phase"] = report["phase"].clone();
    report["lifecyclePassed"] = json!(passed);
    report["allRequestedWorkloadsPassed"] = json!(passed);
    report["passed"] = json!(passed);
    power::finish(report);
    save_acceptance_report(evidence, report);
}

#[test]
fn native_service_integration_profile_preserves_bounds_and_pending_verdicts() {
    let selected = adapter_profile(PROFILE).unwrap();
    assert!(selected.repacked && !selected.optional_requests && !selected.required_scale_history);
    assert_eq!(selected.row_workers, 4);
    assert_eq!(profile_case(PROFILE, false, false), Some(CASE));
    assert_eq!(profile_case(PROFILE, true, false), None);
    assert_eq!(profile_case(PROFILE, false, true), None);
    assert_eq!(adapter_completion_caps(selected), [1, 1, 1]);
    assert_eq!(
        body(1)["messages"],
        adapter_messages(&adapter_boundaries::ADAPTER_PROFILE, false)
    );
    assert_eq!(body(1)["max_completion_tokens"], 1);
    assert_eq!(body(EXTENDED_COMPLETION_CAP)["max_completion_tokens"], 4096);
    assert_eq!(
        body(1)["messages"],
        body(EXTENDED_COMPLETION_CAP)["messages"]
    );
    assert_eq!(body(1)["abliteration_scale"], 1.3);
    assert_eq!(body(1)["enable_thinking"], false);
    let pending = pending_workload();
    for field in [
        "cancellationPrefixMatchedBaseline",
        "cancellationRequestBoundToBaseline",
        "brokerOperationInterrupted",
        "initialOwnerCleanupPassed",
        "freshOwnerReadinessPassed",
        "freshCredentialsDiffer",
        "freshOwnerReplayMatched",
        "freshOwnerCleanupPassed",
        "backendGeneratingAtStopKnown",
        "completeStateParityQualified",
        "serverPerRequestScaleAcknowledgementObserved",
        "canonicalCountGenerationWireCaptureObserved",
    ] {
        assert_eq!(pending[field], false);
    }
}

#[test]
fn native_service_integration_binds_prefix_and_reported_prompt_counts() {
    let mut first = adapter_initial_fixture(&ADAPTER_PROFILE);
    first["requestSha256"] = json!(adapter_digest(&serde_json::to_vec(&body(1)).unwrap()));
    assert!(cancellation_request_matches_baseline(&first));
    let text = first["visibleContent"].as_str().unwrap();
    assert!(prefix_matches(&first, text));
    for (field, value) in [
        ("passed", json!(false)),
        ("visibleContentSha256", json!("0".repeat(64))),
        ("visibleContentBytes", json!(0)),
        ("requestedCompletionCap", json!(2)),
        ("reportedCompletionTokens", Value::Null),
        ("requestedScaleF64Bits", json!("0000000000000000")),
        ("requestMessagesSha256", json!("0".repeat(64))),
    ] {
        let mut changed = first.clone();
        changed[field] = value;
        assert!(
            !prefix_matches(&changed, text),
            "Changed {field} must fail prefix binding."
        );
    }
    assert!(!prefix_matches(&first, ""));
    assert!(!prefix_matches(&first, "different"));
    for field in [
        "requestSha256",
        "requestMessagesSha256",
        "requestedScaleF64Bits",
    ] {
        let mut changed = first.clone();
        changed[field] = json!("0".repeat(64));
        assert!(!cancellation_request_matches_baseline(&changed));
    }
    let mut streams = json!([first.clone(), first.clone(), first]);
    assert!(!prompt_counts_match(&streams, None));
    for stream in streams.as_array_mut().unwrap() {
        stream["reportedPromptTokens"] = json!(11);
    }
    assert!(prompt_counts_match(&streams, None));
    let mut replay = streams[0].clone();
    assert!(prompt_counts_match(&streams, Some(&replay)));
    replay["reportedPromptTokens"] = json!(12);
    assert!(!prompt_counts_match(&streams, Some(&replay)));
    streams[1]["reportedPromptTokens"] = Value::Null;
    assert!(!prompt_counts_match(&streams, None));
    for tokens in [0, 8193] {
        for stream in streams.as_array_mut().unwrap() {
            stream["reportedPromptTokens"] = json!(tokens);
        }
        assert!(!prompt_counts_match(&streams, None));
    }
}

#[test]
fn native_service_integration_prefix_rejects_terminal_events_already_received() {
    let complete =
        b"data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"},\"finish_reason\":null}]}\n\n";
    for length in 0..complete.len() {
        assert_eq!(complete_prefix(&complete[..length]), (None, false));
    }
    assert_eq!(complete_prefix(complete), (Some("Hello".into()), false));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        for terminal in [None, Some("stop"), Some("done")] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = listener.local_addr().unwrap().port();
            let fixture = tokio::spawn(async move {
                let (mut peer, _) = listener.accept().await.unwrap();
                let content = "data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"},\"finish_reason\":null}]}\n\n";
                let terminal = match terminal {
                    Some("stop") => "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
                    Some(_) => "data: [DONE]\n\n",
                    None => "",
                };
                let payload = format!("{content}{terminal}");
                let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n";
                let response = format!("{head}{:X}\r\n{payload}\r\n", payload.len());
                peer.write_all(response.as_bytes()).await.unwrap();
                peer.shutdown().await.unwrap();
            });
            let client = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
            let reading = tokio::spawn(cancellation_prefix(client));
            let observed = timeout(Duration::from_secs(5), reading).await.unwrap();
            assert_eq!(observed.unwrap().1.as_deref(), terminal.is_none().then_some("Hello"));
            fixture.await.unwrap();
        }
    });
}

#[test]
fn native_service_integration_rejects_natural_completion_and_missing_rejections() {
    let interrupted = RunReport {
        exit: RunExit::Shutdown,
        completed: 3,
        failed: 3,
        peak_in_flight: 1,
        backend_resources: None,
    };
    assert!(cancellation_counters_match(&interrupted));
    for (completed, failed) in [(4, 2), (3, 2), (3, 1), (2, 3)] {
        assert!(!cancellation_counters_match(&RunReport {
            completed,
            failed,
            exit: RunExit::Shutdown,
            peak_in_flight: 1,
            backend_resources: None
        }));
    }
}

#[test]
fn native_service_integration_fresh_owner_reserve_keeps_original_deadline() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let evidence = env::temp_dir().join(format!(
            "Bonsai-Service-Reserve-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&evidence).unwrap();
        let deadline = tokio::time::Instant::now() + NEXT_OWNER_RESERVE;
        let mut report = json!({"passed":false,"serviceIntegrationWorkload":pending_workload()});
        assert!(
            !fresh_owner(
                Path::new("unused-no-model-start"),
                &evidence,
                deadline,
                "unused-fixture-credential",
                &mut report
            )
            .await
        );
        assert_eq!(
            report["serviceIntegrationWorkload"]["pendingReason"],
            "insufficient-fresh-owner-and-cleanup-reserve"
        );
        assert_eq!(
            report["serviceIntegrationWorkload"]["freshOwnerReadinessPassed"],
            false
        );
        assert_eq!(
            report["serviceIntegrationWorkload"]["freshOwnerReplayMatched"],
            false
        );
        assert_eq!(report["passed"], false);
        assert!(report["freshOwnerReadiness"].is_null());
        fs::remove_file(evidence.join("acceptance.json")).unwrap();
        fs::remove_dir(evidence).unwrap();
    });
}

#[test]
#[ignore = "Actual released PTQ1 service integration; one bounded main-only owner CI job."]
fn pinned_native_ptq1_adapter_row4_checks_released_service_integration() {
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
        "allRequestedWorkloadsPassed":false,"requiredNativeSandbox":true,
        "scope":"bounded-released-service-scale-boundaries-stop-and-fresh-owner-replay",
        "aggregateModelCaseBudgetSeconds":budget.as_secs(),"maximumModelCaseBudgetSeconds":CASE_BUDGET.as_secs(),
        "wholeJobMaximumSeconds":300,"configuredNativeRowWorkers":4,
        "requestedAdapterScales":[1.3,-0.0,2.0,1.3,1.3],
        "requestedCompletionCaps":[1,1,1,4096,1],
        "initialMessages":body(1)["messages"],"initialRequestSha256":adapter_digest(&serde_json::to_vec(&body(1)).unwrap()),
        "servingStatePolicy":"reset-every-request","weightRepresentation":"current-repacked-PTQ1_0",
        "scaleBoundaryWorkload":adapter_boundaries::pending_workload(),
        "serviceIntegrationWorkload":pending_workload(),
        "defaultRuntimeQualified":false,"releasedServiceGateQualified":false,
        "completeStateParityQualified":false,"authoringBasisQualified":false,
        "performanceQualified":false,"qualityQualified":false});
    report["modelExportReceipt"] =
        serde_json::from_str::<Value>(include_str!("../../../../config/swift.ptq1-export.json"))
            .unwrap();
    report["runtimeLock"] =
        serde_json::from_str::<Value>(include_str!("../../../../config/prism-inference.lock.json"))
            .unwrap();
    report["adapterLock"] =
        serde_json::from_str::<Value>(include_str!("../../../../config/swift-lora.lock.json"))
            .unwrap();
    save_acceptance_report(Path::new(&evidence), &report);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(LocalSet::new().run_until(async {
        timeout(
            budget,
            acceptance(Path::new(&data), Path::new(&evidence), budget, &mut report),
        )
        .await
        .unwrap();
    }));
    assert_eq!(
        report["passed"], true,
        "Required released-service observations remain pending."
    );
}
