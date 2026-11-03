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
// NativeProcess::drop may wait up to ten seconds for contained descendants.
// A timed-out task must be cancelled and observed inside the existing Stop
// allowance instead of being detached by dropping its JoinHandle.
const OWNER_DROP_RESERVE: Duration = Duration::from_secs(10);

pub(super) fn cleanup_passed(cleanup: &Value) -> bool {
    cleanup["status"] == "confirmed"
        && cleanup["ownedJobEmpty"] == true
        && cleanup["listenerReleased"] == true
}

fn listener_release_observation(probe: Option<Result<(), io::ErrorKind>>) -> Value {
    // Only an explicit refusal establishes that this frontend no longer listens.
    // Access, resource and network failures can also prevent a connection to a
    // live listener; preserve those observations as unknown instead of allowing
    // another owner or the post-cleanup artifact check.
    let (released, outcome) = match probe {
        Some(Ok(())) => (Some(false), "listener-accepted-connection"),
        Some(Err(io::ErrorKind::ConnectionRefused)) => (Some(true), "connection-refused"),
        Some(Err(_)) => (None, "probe-failed"),
        None => (None, "probe-deadline-expired"),
    };
    json!({"listenerReleased":released,"listenerProbeOutcome":outcome,
        "listenerProbeErrorKind":probe.and_then(Result::err).map(|kind|format!("{kind:?}"))})
}

pub(super) async fn stop_owner(
    stop: oneshot::Sender<()>,
    mut running: tokio::task::JoinHandle<Result<RunReport, LaunchError>>,
    port: u16,
    allow_closed_admission: bool,
    original_deadline: tokio::time::Instant,
) -> (Value, Option<RunReport>) {
    let deadline = original_deadline.min(tokio::time::Instant::now() + STOP_WAIT);
    let sent = stop.send(()).is_ok();
    let result = tokio::time::timeout_at(deadline - OWNER_DROP_RESERVE, &mut running).await;
    let mut observed = json!({"status":"pending","stopSignalDelivered":sent,
        "ownedJobEmpty":null,"listenerReleased":null,"exit":null,
        "completedOperations":null,"failedOperations":null,"peakInFlight":null,
        "backendResources":null,"ownerTaskAborted":false,"ownerTaskReleaseObserved":true,
        "pendingReason":null});
    let report = match result {
        Ok(Ok(Ok(report))) => Some(report),
        Ok(Ok(Err(_))) => {
            observed["pendingReason"] = json!("owned-run-failed");
            None
        }
        Ok(Err(_)) => {
            observed["pendingReason"] = json!("owned-task-failed");
            None
        }
        Err(_) => {
            running.abort();
            observed["ownerTaskAborted"] = json!(true);
            // Await cancellation so the task releases its owner before callers
            // retain power-after or consider any later owner. Native Drop has
            // the ten-second bound reserved above; its synchronous OS teardown
            // cannot be preempted by another Tokio timeout.
            let _ = running.await;
            observed["pendingReason"] = json!("owned-stop-confirmation-timed-out");
            None
        }
    };
    if let Some(report) = report {
        // A successful LaunchedService::run confirms the complete owned Job
        // empty even if its service exit is unsuitable for this workload.
        observed["ownedJobEmpty"] = json!(true);
        observed["completedOperations"] = json!(report.completed);
        observed["failedOperations"] = json!(report.failed);
        observed["peakInFlight"] = json!(report.peak_in_flight);
        observed["backendResources"] = backend_resource_observation(report.backend_resources);
        observed["exit"] = json!(match report.exit {
            RunExit::Shutdown => "shutdown",
            RunExit::ShutdownFailure => "shutdown-failure",
            RunExit::AdmissionClosed => "admission-closed",
            RunExit::AcceptFailure => "accept-failure",
        });
        if !sent && !allow_closed_admission {
            observed["pendingReason"] = json!("owned-stop-signal-not-delivered");
        } else if report.exit != RunExit::Shutdown
            && !(allow_closed_admission && report.exit == RunExit::AdmissionClosed)
        {
            observed["pendingReason"] = json!("unexpected-owned-run-exit");
        }
    }
    let probe = tokio::time::timeout_at(deadline, TcpStream::connect(("127.0.0.1", port)))
        .await
        .ok()
        .map(|result| result.map(|_| ()).map_err(|error| error.kind()));
    observed
        .as_object_mut()
        .unwrap()
        .extend(listener_release_observation(probe).as_object().unwrap().clone());
    observed["finishedBeforeStopDeadline"] = json!(tokio::time::Instant::now() < deadline);
    if observed["pendingReason"].is_null() && observed["finishedBeforeStopDeadline"] != true {
        observed["pendingReason"] = json!("owned-cleanup-deadline-expired");
    }
    if observed["pendingReason"].is_null() && observed["listenerReleased"] != true {
        observed["pendingReason"] = json!("frontend-listener-release-unconfirmed");
    }
    if observed["ownedJobEmpty"] == true && observed["pendingReason"].is_null() {
        observed["status"] = json!("confirmed");
    }
    (observed, report)
}

pub(super) fn body(cap: u32) -> Value {
    let mut body = request(adapter_messages(&ADAPTER_PROFILE, false), true, cap);
    body["enable_thinking"] = json!(false);
    body["abliteration_scale"] = json!(1.3);
    body
}

pub(super) fn pending_workload() -> Value {
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

async fn cancellation_line(reader: &mut BufReader<TcpStream>, limit: usize) -> io::Result<String> {
    let mut line = String::new();
    let count = reader
        .take(u64::try_from(limit + 1).unwrap())
        .read_line(&mut line)
        .await?;
    if count == 0 || count > limit || !line.ends_with("\r\n") {
        return Err(io::Error::other(
            "Invalid bounded cancellation response line.",
        ));
    }
    Ok(line)
}

async fn cancellation_prefix(
    client: TcpStream,
) -> io::Result<(BufReader<TcpStream>, Option<String>)> {
    let mut reader = BufReader::new(client);
    let mut header = String::new();
    loop {
        let line = cancellation_line(&mut reader, 16 * 1024 - header.len()).await?;
        header.push_str(&line);
        if line == "\r\n" {
            break;
        }
    }
    if header.lines().next() != Some("HTTP/1.1 200 OK")
        || [
            "Content-Type: text/event-stream",
            "Transfer-Encoding: chunked",
        ]
        .iter()
        .any(|required| {
            !header
                .lines()
                .any(|line| line.eq_ignore_ascii_case(required))
        })
    {
        return Err(io::Error::other("Invalid cancellation response headers."));
    }
    let mut payload = Vec::new();
    loop {
        let line = cancellation_line(&mut reader, 32).await?;
        let length = usize::from_str_radix(line.trim_end(), 16).map_err(io::Error::other)?;
        if length == 0 || length > 64 * 1024 - payload.len() {
            return Err(io::Error::other(
                "Invalid bounded cancellation response chunk.",
            ));
        }
        let first = payload.len();
        payload.resize(first + length, 0);
        reader.read_exact(&mut payload[first..]).await?;
        let mut ending = [0; 2];
        reader.read_exact(&mut ending).await?;
        if &ending != b"\r\n" {
            return Err(io::Error::other(
                "Invalid cancellation response chunk ending.",
            ));
        }
        // Inspect every complete SSE record already received, including records after
        // the first visible delta. A terminal event in this chunk cannot pass
        // merely because its visible content preceded that terminal event.
        let (content, terminal) = std::panic::catch_unwind(|| complete_prefix(&payload))
            .map_err(|_| io::Error::other("Invalid cancellation SSE record."))?;
        if terminal {
            return Ok((reader, None));
        }
        if let Some(content) = content {
            return Ok((reader, Some(content)));
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
        let captured = adapter_work_before_cleanup(deadline, async {
            let client = try_connect(port, &credential, &body(EXTENDED_COMPLETION_CAP)).await?;
            cancellation_prefix(client).await
        })
        .await;
        if captured.as_ref().is_some_and(Result::is_err) {
            report["serviceIntegrationWorkload"]["pendingReason"] =
                json!("cancellation-response-transport-or-framing-failed");
        }
        captured.and_then(Result::ok)
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
    let (cleanup, stopped_report) =
        stop_owner(stop, running, port, prefix.is_none(), deadline).await;
    let cleaned = cleanup_passed(&cleanup);
    report["initialCleanup"] = cleanup;
    report["serviceIntegrationWorkload"]["initialOwnerCleanupPassed"] = json!(cleaned);
    report["initialOwnerPowerAfter"] = power::snapshot();
    save_acceptance_report(evidence, report);
    if let Some((client, text)) = prefix {
        // Three completed scale streams and two failed rejection controls must
        // precede exactly one failed interrupted operation. Natural completion
        // increments completed instead and fails these independent counters.
        let interrupted = stopped_report
            .as_ref()
            .is_some_and(cancellation_counters_match)
            && text.is_some_and(|text| !text.is_empty());
        drop(client);
        report["serviceIntegrationWorkload"]["brokerOperationInterrupted"] = json!(interrupted);
    }
    let passed = cleaned
        && boundaries
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
    let fresh_credentials_differ = credential != previous_credential;
    report["serviceIntegrationWorkload"]["freshCredentialsDiffer"] =
        json!(fresh_credentials_differ);
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
    let (cleanup, _) = stop_owner(stop, running, port, !matched, deadline).await;
    let cleaned = cleanup_passed(&cleanup);
    report["freshOwnerCleanup"] = cleanup;
    report["serviceIntegrationWorkload"]["freshOwnerCleanupPassed"] = json!(cleaned);
    report["freshOwnerPowerAfter"] = power::snapshot();
    save_acceptance_report(evidence, report);
    matched
        && fresh_credentials_differ
        && cleaned
        && report["freshOwnerCleanup"]["completedOperations"] == 1
        && report["freshOwnerCleanup"]["failedOperations"] == 0
}

async fn acceptance(
    data: &Path,
    evidence: &Path,
    deadline: tokio::time::Instant,
    report: &mut Value,
) {
    power::begin(report);
    let passed = sequence(data, evidence, deadline, report).await;
    report["lifecyclePassed"] = json!(passed);
    report["allRequestedWorkloadsPassed"] = json!(passed);
    report["passed"] = json!(passed);
    power::finish(report);
    save_acceptance_report(evidence, report);
}

pub(super) async fn sequence(
    data: &Path,
    evidence: &Path,
    deadline: tokio::time::Instant,
    report: &mut Value,
) -> bool {
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
    save_acceptance_report(evidence, report);
    passed
}

#[test]
fn native_service_integration_listener_release_requires_connection_refusal() {
    for (probe, released, outcome) in [
        (Some(Ok(())), Some(false), "listener-accepted-connection"),
        (
            Some(Err(io::ErrorKind::ConnectionRefused)),
            Some(true),
            "connection-refused",
        ),
        (None, None, "probe-deadline-expired"),
        (Some(Err(io::ErrorKind::PermissionDenied)), None, "probe-failed"),
        (Some(Err(io::ErrorKind::AddrNotAvailable)), None, "probe-failed"),
        (Some(Err(io::ErrorKind::TimedOut)), None, "probe-failed"),
        (Some(Err(io::ErrorKind::Interrupted)), None, "probe-failed"),
        (Some(Err(io::ErrorKind::Other)), None, "probe-failed"),
    ] {
        let observation = listener_release_observation(probe);
        assert_eq!(observation["listenerReleased"], json!(released));
        assert_eq!(observation["listenerProbeOutcome"], outcome);
        assert_eq!(
            observation["listenerProbeErrorKind"],
            json!(probe.and_then(Result::err).map(|kind| format!("{kind:?}")))
        );
        let cleanup = json!({"status":"confirmed","ownedJobEmpty":true,
            "listenerReleased":observation["listenerReleased"]});
        assert_eq!(cleanup_passed(&cleanup), released == Some(true));
    }
}

fn released_loopback_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

#[test]
fn native_service_integration_cleanup_preserves_failures_and_listener_observations() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(LocalSet::new().run_until(async {
        for (exit, allow_closed, delivered, confirmed) in [
            (RunExit::Shutdown, false, true, true),
            (RunExit::AdmissionClosed, true, false, true),
            (RunExit::AdmissionClosed, false, true, false),
            (RunExit::AcceptFailure, true, true, false),
            (RunExit::Shutdown, false, false, false),
        ] {
            let (stop, stopped) = oneshot::channel();
            let report = RunReport {
                exit,
                completed: 1,
                failed: 0,
                peak_in_flight: 1,
                backend_resources: None,
            };
            let running = if delivered {
                spawn_local(async move {
                    let _ = stopped.await;
                    Ok(report)
                })
            } else {
                drop(stopped);
                spawn_local(async move { Ok(report) })
            };
            let deadline = tokio::time::Instant::now() + STOP_WAIT;
            let (cleanup, observed) =
                stop_owner(stop, running, released_loopback_port(), allow_closed, deadline).await;
            assert_eq!(cleanup_passed(&cleanup), confirmed);
            assert_eq!(cleanup["stopSignalDelivered"], delivered);
            assert_eq!(cleanup["ownedJobEmpty"], true);
            assert_eq!(cleanup["listenerReleased"], true);
            assert_eq!(cleanup["listenerProbeOutcome"], "connection-refused");
            assert_eq!(observed, Some(report));
        }
        let (stop, stopped) = oneshot::channel();
        let running = spawn_local(async move {
            let _ = stopped.await;
            Err(LaunchError::Stop)
        });
        let deadline = tokio::time::Instant::now() + STOP_WAIT;
        let (cleanup, observed) =
            stop_owner(stop, running, released_loopback_port(), true, deadline).await;
        assert!(observed.is_none());
        assert!(!cleanup_passed(&cleanup));
        assert!(cleanup["ownedJobEmpty"].is_null());
        assert_eq!(cleanup["pendingReason"], "owned-run-failed");
        assert_eq!(cleanup["listenerReleased"], true);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (stop, stopped) = oneshot::channel();
        let running = spawn_local(async move {
            let _ = stopped.await;
            Ok(RunReport {
                exit: RunExit::Shutdown,
                completed: 1,
                failed: 0,
                peak_in_flight: 1,
                backend_resources: None,
            })
        });
        let deadline = tokio::time::Instant::now() + STOP_WAIT;
        let (cleanup, _) = stop_owner(stop, running, port, false, deadline).await;
        assert!(!cleanup_passed(&cleanup));
        assert_eq!(cleanup["ownedJobEmpty"], true);
        assert_eq!(cleanup["listenerReleased"], false);
        assert_eq!(cleanup["listenerProbeOutcome"], "listener-accepted-connection");
        assert_eq!(
            cleanup["pendingReason"],
            "frontend-listener-release-unconfirmed"
        );
    }));
}

#[test]
fn native_service_integration_stuck_owner_is_dropped_inside_existing_stop_allowance() {
    struct DropMarker(std::rc::Rc<std::cell::Cell<bool>>);
    impl Drop for DropMarker {
        fn drop(&mut self) {
            self.0.set(true);
        }
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .start_paused(true)
        .build()
        .unwrap();
    runtime.block_on(LocalSet::new().run_until(async {
        for allowance in [STOP_WAIT, Duration::from_secs(15)] {
            let dropped = std::rc::Rc::new(std::cell::Cell::new(false));
            let marker = DropMarker(dropped.clone());
            let (stop, _stopped) = oneshot::channel();
            let running = spawn_local(async move {
                let _marker = marker;
                pending::<Result<RunReport, LaunchError>>().await
            });
            let started = tokio::time::Instant::now();
            let (cleanup, observed) =
                stop_owner(stop, running, released_loopback_port(), false, started + allowance).await;
            assert!(
                dropped.get(),
                "Timed-out ownership must not remain detached."
            );
            assert!(started.elapsed() <= allowance);
            assert_eq!(cleanup["ownerTaskAborted"], true);
            assert_eq!(cleanup["ownerTaskReleaseObserved"], true);
            assert_eq!(
                cleanup["pendingReason"],
                "owned-stop-confirmation-timed-out"
            );
            assert!(cleanup["ownedJobEmpty"].is_null());
            assert!(!cleanup_passed(&cleanup));
            assert!(observed.is_none());
        }
    }));
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
            assert_eq!(observed.unwrap().unwrap().1.as_deref(), terminal.is_none().then_some("Hello"));
            fixture.await.unwrap();
        }
    });
}

#[test]
fn native_service_integration_cancellation_failures_return_without_unwinding() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n";
        let malformed = "data: invalid-json\n\n";
        let responses = [
            Vec::new(),
            b"HTTP/1.1 200 OK\r\n".to_vec(),
            b"HTTP/1.1 503 Service Unavailable\r\n\r\n".to_vec(),
            format!("{head}FFFFFFFFFFFFFFFF\r\n").into_bytes(),
            format!("{head}4\r\nx").into_bytes(),
            format!("{head}{:X}\r\n{malformed}\r\n", malformed.len()).into_bytes(),
            format!("{head}0\r\n\r\n").into_bytes(),
            vec![b'x'; 16 * 1024 + 1],
        ];
        for response in responses {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = listener.local_addr().unwrap().port();
            let fixture = tokio::spawn(async move {
                let (mut peer, _) = listener.accept().await.unwrap();
                peer.write_all(&response).await.unwrap();
                peer.shutdown().await.unwrap();
            });
            let client = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
            let reading = tokio::spawn(cancellation_prefix(client));
            // A failed JoinHandle would signal an unwind through the live-owner
            // caller. An ordinary Err leaves its Stop and final power code live.
            let observed = timeout(Duration::from_secs(5), reading).await.unwrap().unwrap();
            assert!(observed.is_err());
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
        "aggregateModelCaseBudgetSeconds":budget.as_secs(),"maximumModelCaseBudgetSeconds":maximum_case_budget().as_secs(),
        "wholeJobMaximumSeconds":whole_job_budget().as_secs(),"configuredNativeRowWorkers":4,
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
        "Required released-service observations remain pending."
    );
}
