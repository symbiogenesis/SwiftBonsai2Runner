//! Opt-in actual-model application acceptance. Ordinary suites never load weights.
#![cfg(windows)]
#![forbid(unsafe_code)]
#![cfg_attr(
    windows,
    expect(
        clippy::future_not_send,
        reason = "Acceptance observers intentionally share thread-confined Rc/RefCell response capture and native hash state; current-thread runtimes, LocalSet and spawn_local never transfer these futures between threads."
    )
)]
#![expect(
    clippy::unwrap_used,
    reason = "Acceptance assertions must fail loudly."
)]

use bonsai_broker::{
    launcher::{LaunchConfig, LaunchError, LaunchedService, StartupPhase, StartupProgress},
    runtime::{BackendResources, RunExit, RunReport},
};
use serde_json::{Value, json};
use std::{
    env, ffi::OsString, fs, future::pending, io, net::TcpListener, path::Path, sync::Mutex,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
    sync::oneshot,
    task::{LocalSet, spawn_local},
    time::timeout,
};

#[path = "native_model/adapter_boundaries.rs"]
mod adapter_boundaries;
#[path = "native_model/power.rs"]
mod power;
#[path = "native_model/progress.rs"]
mod progress;
#[path = "native_model/representation.rs"]
mod representation;
#[path = "native_model/response_capture.rs"]
mod response_capture;
#[path = "native_model/service_integration.rs"]
mod service_integration;
#[path = "native_model/service_rollback.rs"]
mod service_rollback;
#[path = "native_model/speed.rs"]
mod speed;

const CASE: &str = "pinned_native_model_streams_recovers_and_restarts_in_required_sandbox";
const PTQ1_ADAPTER_CASE: &str =
    "pinned_native_ptq1_adapter_starts_and_streams_within_device_budget";
const PTQ1_ADAPTER_LIFECYCLE_CASE: &str =
    "pinned_native_ptq1_adapter_checks_scale_change_rejection_and_history";
const PTQ1_ADAPTER_LIFECYCLE_ROW4_CASE: &str =
    "pinned_native_ptq1_adapter_row4_checks_scale_change_rejection_and_history";
const PQ2_ADAPTER_ROW4_CASE: &str =
    "pinned_native_pq2_adapter_row4_starts_and_streams_within_device_budget";
const PQ2_ADAPTER_LIFECYCLE_CASE: &str =
    "pinned_native_pq2_adapter_row4_checks_scale_change_rejection_and_history";
const PQ2_ADAPTER_SPEED_PROFILE: &str = "pq2-adapter-speed";
const PQ2_ADAPTER_SPEED_CASE: &str = "speed::pinned_native_pq2_adapter_compares_request_throughput";
const CASE_BUDGET: Duration = Duration::from_secs(240);
const OPTIONAL_RESERVE: Duration = Duration::from_secs(120);
const WAIT: Duration = Duration::from_secs(600);
const EXTENDED_COMPLETION_CAP: u32 = 4096;
const BOUNDED_HISTORY_REPETITIONS: usize = 128;
const SHORT_COMPLETION_CAP: u32 = 4;
const STALLED_CONSUMER_PAUSE: Duration = Duration::from_secs(1);
const STOP_WAIT: Duration = Duration::from_secs(30);
const EMPTY_RESPONSE: [u8; 0] = [];
// This one-owner workload needs no fresh startup. Keep thirty seconds for
// teardown and require at least fifteen seconds before each further request.
const ADAPTER_LIFECYCLE_RESERVE: Duration = Duration::from_secs(45);

fn whole_job_budget_setting(setting: Option<&str>) -> Option<Duration> {
    let Some(setting) = setting else {
        return Some(Duration::from_secs(300));
    };
    if setting.is_empty()
        || setting.starts_with('0')
        || !setting.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let seconds = setting.parse::<u64>().ok()?;
    (300..=900)
        .contains(&seconds)
        .then_some(Duration::from_secs(seconds))
}

fn whole_job_budget() -> Duration {
    whole_job_budget_setting(env::var("BONSAI_NATIVE_WHOLE_JOB_SECONDS").ok().as_deref()).unwrap()
}

fn maximum_case_budget() -> Duration {
    whole_job_budget()
        .checked_sub(Duration::from_secs(60))
        .unwrap()
}

fn case_budget_setting(setting: Option<&str>, whole_job: Option<&str>) -> Option<Duration> {
    let maximum = whole_job_budget_setting(whole_job)?.checked_sub(Duration::from_secs(60))?;
    let setting = setting?;
    if setting.is_empty() || !setting.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let seconds = setting.parse::<u64>().ok()?;
    (1..=maximum.as_secs())
        .contains(&seconds)
        .then_some(Duration::from_secs(seconds))
}

fn case_budget() -> Duration {
    // Hosted preparation passes only the allowance left after owner-job setup.
    // Missing/malformed limits must never silently restore a fresh allowance.
    case_budget_setting(
        env::var("BONSAI_NATIVE_CASE_BUDGET_SECONDS")
            .ok()
            .as_deref(),
        env::var("BONSAI_NATIVE_WHOLE_JOB_SECONDS").ok().as_deref(),
    )
    .unwrap()
}

struct Workloads {
    extended_history: bool,
    stalled_consumer: bool,
}

struct AdapterProfile {
    name: &'static str,
    case: &'static str,
    repacked: bool,
    row_workers: u8,
    optional_requests: bool,
    required_scale_history: bool,
}

const PTQ1_ADAPTER_PROFILE: AdapterProfile = AdapterProfile {
    name: "ptq1-adapter",
    case: PTQ1_ADAPTER_CASE,
    repacked: true,
    row_workers: 1,
    optional_requests: true,
    required_scale_history: false,
};
const PTQ1_ADAPTER_LIFECYCLE_PROFILE: AdapterProfile = AdapterProfile {
    name: "ptq1-adapter-lifecycle",
    case: PTQ1_ADAPTER_LIFECYCLE_CASE,
    repacked: true,
    row_workers: 1,
    optional_requests: false,
    required_scale_history: true,
};
const PTQ1_ADAPTER_LIFECYCLE_ROW4_PROFILE: AdapterProfile = AdapterProfile {
    name: "ptq1-adapter-lifecycle-row4",
    case: PTQ1_ADAPTER_LIFECYCLE_ROW4_CASE,
    repacked: true,
    row_workers: 4,
    optional_requests: false,
    required_scale_history: true,
};
const PQ2_ADAPTER_ROW4_PROFILE: AdapterProfile = AdapterProfile {
    name: "pq2-adapter-row4",
    case: PQ2_ADAPTER_ROW4_CASE,
    repacked: false,
    row_workers: 4,
    optional_requests: false,
    required_scale_history: false,
};
const PQ2_ADAPTER_LIFECYCLE_PROFILE: AdapterProfile = AdapterProfile {
    name: "pq2-adapter-lifecycle-row4",
    case: PQ2_ADAPTER_LIFECYCLE_CASE,
    repacked: false,
    row_workers: 4,
    optional_requests: false,
    required_scale_history: true,
};

fn native_four_worker_release(version: Option<&str>) -> bool {
    matches!(
        version,
        Some("0.1.9" | "0.1.10" | "0.1.11" | "0.1.12" | "0.1.13" | "0.1.14")
    )
}

fn worker_readiness_receipt(version: Option<&str>, expected: u8, verified: bool) -> Value {
    // These reviewed owners require a matching field from the instantiated graph.
    // Historical versions may omit it, and later versions need another review.
    // A configured argument or a receipt written before start cannot supply it.
    let observed = matches!(version, Some("0.1.11" | "0.1.12" | "0.1.13" | "0.1.14"))
        && matches!(expected, 1 | 4)
        && verified;
    json!({"configuredNativeRowWorkers":expected, "verifiedReadinessObserved":verified,
        "rowWorkerReadinessVersion":version,
        "rowWorkerCountIndependentlyObserved":observed,
        "observedNativeRowWorkers":observed.then_some(expected),
        "rowWorkerObservationScope":"effective configured graph budget verified by private readiness; no executing-thread, parallel-operation or benefit evidence"})
}

fn record_worker_readiness(report: &mut Value, expected: u8, verified: bool) {
    // Read the same compiled lock as LaunchConfig; never trust a mutable report
    // field to turn a legacy or missing readiness result into observed identity.
    let runtime_lock: Value =
        serde_json::from_str(include_str!("../../../config/prism-inference.lock.json")).unwrap();
    let observation =
        worker_readiness_receipt(runtime_lock["version"].as_str(), expected, verified);
    report
        .as_object_mut()
        .unwrap()
        .extend(observation.as_object().unwrap().clone());
}

#[test]
fn native_worker_receipts_require_exact_release_and_verified_readiness() {
    for (version, four_admitted, worker_field_required) in [
        (Some("0.1.8"), false, false),
        (Some("0.1.9"), true, false),
        (Some("0.1.10"), true, false),
        (Some("0.1.11"), true, true),
        (Some("0.1.12"), true, true),
        (Some("0.1.13"), true, true),
        (Some("0.1.14"), true, true),
        (Some("0.1.15"), false, false),
        (Some("v0.1.14"), false, false),
        (Some("0.1.14-dev"), false, false),
        (Some("0.1.14.0"), false, false),
        (Some("0.1.014"), false, false),
        (Some("v0.1.13"), false, false),
        (Some("0.1.13-dev"), false, false),
        (Some("0.1.13.0"), false, false),
        (Some("0.1.013"), false, false),
        (Some("v0.1.12"), false, false),
        (Some("0.1.12-dev"), false, false),
        (Some("0.1.12.0"), false, false),
        (Some("v0.1.11"), false, false),
        (Some("0.1.11-dev"), false, false),
        (Some("0.1.11.0"), false, false),
        (Some("0.1.011"), false, false),
        (Some(""), false, false),
        (None, false, false),
    ] {
        assert_eq!(native_four_worker_release(version), four_admitted);
        for expected in [0, 1, 2, 4, 16] {
            for verified in [false, true] {
                let observed = worker_field_required && matches!(expected, 1 | 4) && verified;
                let receipt = worker_readiness_receipt(version, expected, verified);
                assert_eq!(receipt["rowWorkerReadinessVersion"], json!(version));
                assert_eq!(receipt["verifiedReadinessObserved"], verified);
                assert_eq!(receipt["configuredNativeRowWorkers"], expected);
                assert_eq!(receipt["rowWorkerCountIndependentlyObserved"], observed);
                assert_eq!(
                    receipt["observedNativeRowWorkers"],
                    json!(observed.then_some(expected))
                );
            }
        }
    }
    let runtime_lock: Value =
        serde_json::from_str(include_str!("../../../config/prism-inference.lock.json")).unwrap();
    let mut report = json!({"runtimeLock":{"version":"untrusted-report-value"},
        "rowWorkerCountIndependentlyObserved":true,"observedNativeRowWorkers":4});
    record_worker_readiness(&mut report, 4, false);
    assert_eq!(report["rowWorkerReadinessVersion"], runtime_lock["version"]);
    assert_eq!(report["rowWorkerCountIndependentlyObserved"], false);
    assert_eq!(report["observedNativeRowWorkers"], Value::Null);
    record_worker_readiness(&mut report, 4, true);
    assert_eq!(
        report["rowWorkerCountIndependentlyObserved"],
        matches!(
            runtime_lock["version"].as_str(),
            Some("0.1.11" | "0.1.12" | "0.1.13" | "0.1.14")
        )
    );
}

#[derive(Debug, PartialEq, Eq)]
struct StreamSummary {
    finish_reason: String,
    content_bytes: usize,
    completion_tokens: Option<u64>,
}

fn stream_observation(summary: &StreamSummary, cap: u32) -> Value {
    assert!(summary.content_bytes > 0);
    assert!(["length", "stop"].contains(&summary.finish_reason.as_str()));
    assert!(
        summary
            .completion_tokens
            .is_none_or(|count| count <= u64::from(cap))
    );
    json!({"requestedCompletionCap":cap, "finishReason":summary.finish_reason,
        "visibleContentBytes":summary.content_bytes,
        "reportedCompletionTokens":summary.completion_tokens})
}

fn cleanup_observation(report: &RunReport, expected: RunExit) -> Value {
    assert_eq!(report.exit, expected);
    assert!(matches!(
        expected,
        RunExit::Shutdown | RunExit::AdmissionClosed
    ));
    let exit = if expected == RunExit::Shutdown {
        "shutdown"
    } else {
        "admission-closed"
    };
    // LaunchedService::run returns success only after confirming the owned Job
    // empty. Callers separately verify the frontend listener is released.
    json!({"exit":exit, "completedOperations":report.completed,
        "failedOperations":report.failed, "peakInFlight":report.peak_in_flight,
        "backendResources":backend_resource_observation(report.backend_resources),
        "ownedJobEmpty":true, "listenerReleased":true})
}

fn backend_resource_observation(resources: Option<BackendResources>) -> Value {
    resources.map_or(Value::Null, |resources| {
        let sampled = resources.sample_count > 0;
        json!({"successfulGuardSamples":resources.sample_count,
            "guardIntervalMilliseconds":250,
            "sampledPeakRootPrivateBytes":sampled.then_some(resources.sampled_peak_root_private_bytes),
            "sampledPeakRootWorkingSetBytes":sampled.then_some(resources.sampled_peak_root_working_set_bytes),
            "sampledMinimumSystemAvailablePhysicalBytes":resources.sampled_min_available_physical_bytes,
            "osLifetimePeakJobCommitBytes":resources.os_peak_job_commit_bytes,
            "osLifetimePeakRootWorkingSetBytes":resources.os_peak_root_working_set_bytes,
            "scope":"Job commitment includes descendants and startup; root working sets include mapped residency and exclude descendants; sampled bounds cover serving guard observations only; residency and commitment are non-additive",
            "fullResourceGateQualified":false, "pagingQualified":false,
            "sustainedComfortQualified":false})
    })
}

#[test]
fn native_owned_resource_receipt_preserves_scopes_and_missing_queries() {
    assert_eq!(backend_resource_observation(None), Value::Null);
    let missing = backend_resource_observation(Some(BackendResources::default()));
    assert_eq!(missing["successfulGuardSamples"], 0);
    for field in [
        "sampledPeakRootPrivateBytes",
        "sampledPeakRootWorkingSetBytes",
        "sampledMinimumSystemAvailablePhysicalBytes",
        "osLifetimePeakJobCommitBytes",
        "osLifetimePeakRootWorkingSetBytes",
    ] {
        assert_eq!(missing[field], Value::Null);
    }
    let resources = BackendResources {
        sample_count: 2,
        sampled_peak_root_private_bytes: 40,
        sampled_peak_root_working_set_bytes: 90,
        sampled_min_available_physical_bytes: Some(1000),
        os_peak_job_commit_bytes: Some(150),
        os_peak_root_working_set_bytes: Some(110),
    };
    let report = RunReport {
        exit: RunExit::Shutdown,
        completed: 3,
        failed: 0,
        peak_in_flight: 1,
        backend_resources: Some(resources),
    };
    let retained = cleanup_observation(&report, RunExit::Shutdown);
    let observed = &retained["backendResources"];
    assert_eq!(observed["successfulGuardSamples"], 2);
    assert_eq!(observed["sampledPeakRootPrivateBytes"], 40);
    assert_eq!(observed["sampledPeakRootWorkingSetBytes"], 90);
    assert_eq!(observed["sampledMinimumSystemAvailablePhysicalBytes"], 1000);
    assert_eq!(observed["osLifetimePeakJobCommitBytes"], 150);
    assert_eq!(observed["osLifetimePeakRootWorkingSetBytes"], 110);
    for field in [
        "fullResourceGateQualified",
        "pagingQualified",
        "sustainedComfortQualified",
    ] {
        assert_eq!(observed[field], false);
    }
    assert_eq!(retained["ownedJobEmpty"], true);
    assert_eq!(retained["listenerReleased"], true);
}

fn ci_only() {
    assert_eq!(env::var("GITHUB_ACTIONS").as_deref(), Ok("true"));
    assert_eq!(env::var("GITHUB_REF").as_deref(), Ok("refs/heads/main"));
    assert_eq!(env::consts::ARCH, "aarch64");
    let revision = env::var("GITHUB_SHA").unwrap();
    assert_eq!(revision.len(), 40);
    assert!(revision.bytes().all(|byte| byte.is_ascii_hexdigit()));
}

fn configuration(data: &Path, evidence: &Path) -> LaunchConfig {
    // Production PID checks detect any race between releasing this reservation
    // and the child binding it. No existing listener is ever accepted.
    let reservation = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = reservation.local_addr().unwrap().port().to_string();
    drop(reservation);
    let mut args: Vec<OsString> = [
        "--launch-development",
        "--require-native-sandbox",
        "--native-engine",
        "--native-row-workers",
        "1",
        "--profile",
        "swift-pq2-assessment",
        "--frontend-port",
        "0",
        "--backend-port",
        &port,
        "--data-directory",
    ]
    .into_iter()
    .map(OsString::from)
    .collect();
    args.push(data.as_os_str().to_owned());
    args.push("--evidence-parent".into());
    args.push(evidence.as_os_str().to_owned());
    LaunchConfig::parse(args).unwrap()
}

async fn start(data: &Path, evidence: &Path) -> LaunchedService {
    timeout(
        WAIT,
        configuration(data, evidence).start(pending::<io::Result<()>>()),
    )
    .await
    .unwrap()
    .unwrap()
}

fn key(owner: &LaunchedService) -> String {
    // Never print credentials, raw HTTP requests, or the protected directory.
    let value = fs::read_to_string(owner.evidence_directory().join("frontend.key")).unwrap();
    value.trim().to_owned()
}

fn request(messages: Value, streaming: bool, limit: u32) -> Value {
    let mut body = json!({"stream": streaming, "max_completion_tokens": limit});
    body["messages"] = messages;
    body
}

fn short_request(messages: Value, streaming: bool) -> Value {
    let mut body = request(messages, streaming, SHORT_COMPLETION_CAP);
    body["enable_thinking"] = json!(false);
    body
}

async fn connect(port: u16, key: &str, body: &Value) -> TcpStream {
    try_connect(port, key, body).await.unwrap()
}

async fn try_connect(port: u16, key: &str, body: &Value) -> io::Result<TcpStream> {
    let bytes = serde_json::to_vec(body).unwrap();
    let mut stream = TcpStream::connect(("127.0.0.1", port)).await?;
    let head = format!(
        "POST /v1/chat/completions HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {key}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        bytes.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(&bytes).await?;
    Ok(stream)
}

async fn exchange(port: u16, key: &str, body: &Value) -> Vec<u8> {
    timeout(WAIT, async {
        let stream = connect(port, key, body).await;
        let mut bytes = Vec::new();
        stream
            .take(4 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .await
            .unwrap();
        assert!(bytes.len() <= 4 * 1024 * 1024);
        bytes
    })
    .await
    .unwrap()
}

fn response<'a>(bytes: &'a [u8], status: &str) -> (&'a str, &'a [u8]) {
    let end = bytes
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .unwrap();
    let head = std::str::from_utf8(&bytes[..end]).unwrap();
    assert_eq!(head.lines().next(), Some(status));
    (head, &bytes[end + 4..])
}

fn json_response(bytes: &[u8], status: &str) -> Value {
    let (head, body) = response(bytes, status);
    let length: usize = head
        .lines()
        .find_map(|line| line.strip_prefix("Content-Length: "))
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(length, body.len());
    serde_json::from_slice(body).unwrap()
}

fn stream_payload(bytes: &[u8]) -> Vec<u8> {
    let (head, mut body) = response(bytes, "HTTP/1.1 200 OK");
    assert!(
        head.lines()
            .any(|line| line.eq_ignore_ascii_case("Transfer-Encoding: chunked"))
    );
    let mut payload = Vec::new();
    loop {
        let end = body.windows(2).position(|part| part == b"\r\n").unwrap();
        let length = usize::from_str_radix(std::str::from_utf8(&body[..end]).unwrap(), 16).unwrap();
        body = &body[end + 2..];
        assert!(body.len() >= length + 2);
        payload.extend_from_slice(&body[..length]);
        assert_eq!(&body[length..length + 2], b"\r\n");
        body = &body[length + 2..];
        if length == 0 {
            break;
        }
    }
    assert_eq!(body, b"");
    payload
}

fn streamed_response(bytes: &[u8]) -> StreamSummary {
    let summary = framed_stream_response(bytes);
    assert!(summary.content_bytes > 0);
    summary
}

fn framed_stream_response(bytes: &[u8]) -> StreamSummary {
    let payload = stream_payload(bytes);
    let payload = std::str::from_utf8(&payload).unwrap();
    let mut terminal = false;
    let mut done = false;
    let mut finish_reason = String::new();
    let mut content_bytes = 0;
    let mut completion_tokens = None;
    for line in payload
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
    {
        assert!(!done, "No event may follow DONE.");
        if line == "[DONE]" {
            assert!(terminal);
            done = true;
            continue;
        }
        let event: Value = serde_json::from_str(line).unwrap();
        assert!(event.get("error").is_none());
        if let Some(tokens) = event["usage"]["completion_tokens"].as_u64() {
            assert!(completion_tokens.replace(tokens).is_none());
        }
        let choices = event["choices"].as_array().unwrap();
        for choice in choices {
            assert!(
                !terminal,
                "No generation event may follow a terminal reason."
            );
            if let Some(text) = choice["delta"]["content"].as_str() {
                content_bytes += text.len();
            }
            if let Some(reason) = choice["finish_reason"].as_str() {
                assert!(!terminal);
                assert!(["length", "stop"].contains(&reason));
                terminal = true;
                reason.clone_into(&mut finish_reason);
            }
        }
    }
    assert!(terminal && done);
    StreamSummary {
        finish_reason,
        content_bytes,
        completion_tokens,
    }
}

fn extended_history(repetitions: usize) -> Value {
    json!([
        {"role":"user", "content":"Keep the following fictional forest notes for the next scene."},
        {"role":"assistant", "content":format!("Forest notes:{}", " tree".repeat(repetitions))},
        {"role":"user", "content":"Name a tree from these notes."}
    ])
}

fn bounded_history_request() -> Value {
    short_request(extended_history(BOUNDED_HISTORY_REPETITIONS), true)
}

async fn extended_acceptance(port: u16, credential: &str, evidence: &Path, report: &mut Value) {
    report["extendedWorkload"] = json!({
        "enabled":true, "requested":true, "status":"pending",
        "phase":"context-rejection",
        "scope":"bounded-supplied-history-and-same-owner-reuse",
        "rejectedHistoryRepetitions":4500,
        "rejectedRequestedCompletionCap":EXTENDED_COMPLETION_CAP,
        "admittedHistoryRepetitions":BOUNDED_HISTORY_REPETITIONS,
        "requestedCompletionCap":SHORT_COMPLETION_CAP,
        "thinkingEnabled":false,
        "contextRejectionPassed":false, "historyStreamPassed":false,
        "postCompletionRequestPassed":false,
        "exactPromptTokenCountKnown":false, "fullContextQualified":false,
        "fullOutputQualified":false
    });
    save_acceptance_report(evidence, report);
    // Tokenization is deliberately exercised through ordinary public admission.
    // These are fixed fixture sizes, never bytes/characters relabeled as tokens.
    let rejected = exchange(
        port,
        credential,
        &request(extended_history(4500), true, EXTENDED_COMPLETION_CAP),
    )
    .await;
    assert_eq!(
        json_response(&rejected, "HTTP/1.1 413 Content Too Large")["error"]["code"],
        "context_length_exceeded"
    );
    report["extendedWorkload"]["contextRejectionPassed"] = json!(true);
    report["extendedWorkload"]["phase"] = json!("bounded-history-stream");
    save_acceptance_report(evidence, report);
    let summary = streamed_response(&exchange(port, credential, &bounded_history_request()).await);
    // Validate before recording success, and persist it before a later request
    // can fail or consume the remaining aggregate allowance.
    let observation = stream_observation(&summary, SHORT_COMPLETION_CAP);
    let workload = &mut report["extendedWorkload"];
    workload["historyStreamPassed"] = json!(true);
    workload["historyStream"] = observation;
    workload["finishReason"] = json!(summary.finish_reason);
    workload["visibleContentBytes"] = json!(summary.content_bytes);
    workload["reportedCompletionTokens"] = json!(summary.completion_tokens);
    workload["phase"] = json!("same-owner-reuse");
    save_acceptance_report(evidence, report);
    // A bounded supplied-history request must leave the same instance usable.
    let reused = short_request(json!([{"role":"user", "content":"Name a bird."}]), true);
    report["extendedWorkload"]["postCompletionStream"] = stream_observation(
        &streamed_response(&exchange(port, credential, &reused).await),
        SHORT_COMPLETION_CAP,
    );
    report["extendedWorkload"]["postCompletionRequestPassed"] = json!(true);
    report["extendedWorkload"]["phase"] = json!("completed");
    report["extendedWorkload"]["status"] = json!("passed");
    save_acceptance_report(evidence, report);
}

async fn prefix_line(reader: &mut BufReader<TcpStream>, limit: usize) -> String {
    let mut line = String::new();
    let count = reader
        .take(u64::try_from(limit + 1).unwrap())
        .read_line(&mut line)
        .await
        .unwrap();
    assert!(count > 0 && count <= limit && line.ends_with("\r\n"));
    line
}

async fn read_until_content(client: TcpStream) -> (BufReader<TcpStream>, usize) {
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
        // A chunk may split either UTF-8 or an SSE record. Only parse complete
        // lines; incomplete text is retained until another chunk arrives.
        for bytes in payload.split_inclusive(|byte| *byte == b'\n') {
            if bytes.last() != Some(&b'\n') {
                continue;
            }
            let Ok(line) = std::str::from_utf8(bytes) else {
                continue;
            };
            let Some(data) = line.trim_end().strip_prefix("data: ") else {
                continue;
            };
            if let Ok(event) = serde_json::from_str::<Value>(data)
                && let Some(text) = event["choices"].as_array().and_then(|choices| {
                    choices.iter().find_map(|choice| {
                        choice["delta"]["content"]
                            .as_str()
                            .filter(|text| !text.is_empty())
                    })
                })
            {
                return (reader, text.len());
            }
        }
    }
}

fn stalled_prefix_observation(content_bytes: usize) -> Value {
    assert!(content_bytes > 0);
    json!({
        "enabled":true, "requested":true, "status":"pending",
        "phase":"nonreading-pause",
        "requestedCompletionCap":EXTENDED_COMPLETION_CAP,
        "thinkingEnabled":false, "observedVisiblePrefixBytes":content_bytes,
        "nonreadingPauseMilliseconds":STALLED_CONSUMER_PAUSE.as_millis(),
        "nonreadingPauseCompleted":false,
        "brokerOperationInterrupted":false, "ownedJobStopped":false,
        "freshRestartAfterStopPassed":false,
        "backendGeneratingAtStopKnown":false, "socketBackpressureQualified":false,
        "performanceQualified":false
    })
}

fn stalled_stop_summary(
    report: &RunReport,
    completed_before: usize,
    failed_before: usize,
    content_bytes: usize,
) -> Value {
    assert_eq!(report.exit, RunExit::Shutdown);
    // A naturally completed response cannot qualify interruption merely because
    // its client stopped reading. These are broker operation counters, not proof
    // that the native backend was still generating at the instant of Stop.
    assert_eq!(report.completed, completed_before);
    assert_eq!(report.failed, failed_before + 1);
    assert!(content_bytes > 0);
    json!({
        "enabled":true, "requested":true, "status":"interrupted-awaiting-recovery",
        "scope":"explicit-stop-with-nonreading-client",
        "requestedCompletionCap":EXTENDED_COMPLETION_CAP,
        "thinkingEnabled":false,
        "observedVisiblePrefixBytes":content_bytes,
        "nonreadingPauseMilliseconds":STALLED_CONSUMER_PAUSE.as_millis(),
        "nonreadingPauseCompleted":true,
        "stopCompletionBoundSeconds":STOP_WAIT.as_secs(),
        "completedOperationsBeforeStall":completed_before,
        "failedOperationsBeforeStall":failed_before,
        "completedOperationsAtStop":report.completed,
        "failedOperationsAtStop":report.failed,
        "brokerOperationInterrupted":true, "ownedJobStopped":true,
        "backendGeneratingAtStopKnown":false, "socketBackpressureQualified":false,
        "freshRestartAfterStopPassed":false, "performanceQualified":false
    })
}

fn stalled_request() -> Value {
    let mut body = request(
        json!([{"role":"user", "content":"Write a long, detailed story about a cedar forest. Develop several scenes, characters and a complete ending."}]),
        true,
        EXTENDED_COMPLETION_CAP,
    );
    // Preserve a large output reservation without spending the laptop budget
    // generating it. Stop follows the first visible prefix and a fixed pause.
    body["enable_thinking"] = json!(false);
    body
}

async fn nonreading_prefix(port: u16, credential: &str) -> (BufReader<TcpStream>, usize) {
    let body = stalled_request();
    timeout(WAIT, async {
        let client = connect(port, credential, &body).await;
        read_until_content(client).await
    })
    .await
    .unwrap()
}

async fn supplied_history_acceptance(port: u16, credential: &str) -> Value {
    let history = json!([
        {"role":"user", "content":"Remember the word cedar."},
        {"role":"assistant", "content":"I will remember cedar.", "reasoning_content":"The supplied word is cedar."},
        {"role":"user", "content":"Write one sentence about cedar."}
    ]);
    stream_observation(
        &streamed_response(&exchange(port, credential, &short_request(history, true)).await),
        SHORT_COMPLETION_CAP,
    )
}

async fn history_and_context_acceptance(
    port: u16,
    credential: &str,
    evidence: &Path,
    report: &mut Value,
) {
    report["phase"] = json!("supplied-history-and-context-rejection-reuse");
    save_acceptance_report(evidence, report);
    report["suppliedHistoryStream"] = supplied_history_acceptance(port, credential).await;
    save_acceptance_report(evidence, report);
    // Exact tokenizer counting must reject before generation, then permit a
    // fresh request in the same owned model instance. This is not an 8K decode.
    let oversized = json!([{"role":"user", "content":" cedar".repeat(9000)}]);
    let rejected = exchange(port, credential, &request(oversized, false, 32)).await;
    assert_eq!(
        json_response(&rejected, "HTTP/1.1 413 Content Too Large")["error"]["code"],
        "context_length_exceeded"
    );
    report["contextRejection"] = json!({"status":413, "code":"context_length_exceeded"});
    save_acceptance_report(evidence, report);
    let small = short_request(json!([{"role":"user", "content":"Name a tree."}]), false);
    let completed = json_response(&exchange(port, credential, &small).await, "HTTP/1.1 200 OK");
    report["sameOwnerAfterContextRejection"] = stream_observation(
        &StreamSummary {
            finish_reason: completed["choices"][0]["finish_reason"]
                .as_str()
                .unwrap()
                .into(),
            content_bytes: completed["choices"][0]["message"]["content"]
                .as_str()
                .unwrap()
                .len(),
            completion_tokens: completed["usage"]["completion_tokens"].as_u64(),
        },
        SHORT_COMPLETION_CAP,
    );
    save_acceptance_report(evidence, report);
}

async fn acceptance(
    data: &Path,
    evidence: &Path,
    workloads: &Workloads,
    budget: Duration,
    report: &mut Value,
) {
    let deadline = tokio::time::Instant::now() + budget;
    power::begin(report);
    report["phase"] = json!("initial-owner-startup");
    save_acceptance_report(evidence, report);
    let owner = start(data, evidence).await;
    let port = owner.port();
    let credential = key(&owner);
    let (stop, stopped) = oneshot::channel();
    let running = spawn_local(owner.run(async { stopped.await.map_err(io::Error::other) }));
    history_and_context_acceptance(port, &credential, evidence, report).await;
    let extended_enabled = optional_workload_enabled(
        workloads.extended_history,
        deadline.saturating_duration_since(tokio::time::Instant::now()),
    );
    if extended_enabled {
        report["phase"] = json!("bounded-history-and-same-owner-reuse");
        save_acceptance_report(evidence, report);
        extended_acceptance(port, &credential, evidence, report).await;
    } else {
        report["extendedWorkload"] = skipped_workload(workloads.extended_history);
    }
    save_acceptance_report(evidence, report);
    let stalled_enabled = optional_workload_enabled(
        workloads.stalled_consumer,
        deadline.saturating_duration_since(tokio::time::Instant::now()),
    );
    let stalled = if stalled_enabled {
        report["phase"] = json!("stalled-consumer-first-content-and-pause");
        save_acceptance_report(evidence, report);
        let observed = nonreading_prefix(port, &credential).await;
        report["stalledConsumerWorkload"] = stalled_prefix_observation(observed.1);
        save_acceptance_report(evidence, report);
        // Preserve the first-prefix observation before this cancellable wait.
        // Keep the socket/read-ahead bytes without consuming more output.
        tokio::time::sleep(STALLED_CONSUMER_PAUSE).await;
        report["stalledConsumerWorkload"]["nonreadingPauseCompleted"] = json!(true);
        report["stalledConsumerWorkload"]["phase"] = json!("awaiting-owned-stop");
        Some(observed)
    } else {
        None
    };
    report["phase"] = json!("initial-owner-stop");
    save_acceptance_report(evidence, report);
    stop.send(()).unwrap();
    let stopped_report = timeout(STOP_WAIT, running).await.unwrap().unwrap().unwrap();
    assert_eq!(stopped_report.exit, RunExit::Shutdown);
    assert!(TcpStream::connect(("127.0.0.1", port)).await.is_err());
    report["initialCleanup"] = cleanup_observation(&stopped_report, RunExit::Shutdown);
    save_acceptance_report(evidence, report);
    let mut stalled_report = if let Some((client, content_bytes)) = stalled {
        let extra = usize::from(extended_enabled);
        let summary =
            stalled_stop_summary(&stopped_report, 2 + 2 * extra, 1 + extra, content_bytes);
        // The client handle stays held through owned shutdown/listener cleanup.
        drop(client);
        summary
    } else {
        skipped_workload(workloads.stalled_consumer)
    };
    report["stalledConsumerWorkload"] = stalled_report.clone();
    fresh_owner_recovery(data, evidence, report).await;
    if stalled_enabled {
        stalled_report["freshRestartAfterStopPassed"] = json!(true);
        stalled_report["status"] = json!("passed");
    }
    report["stalledConsumerWorkload"] = stalled_report;
    report["phase"] = json!("completed");
    report["lifecyclePassed"] = json!(true);
    report["allRequestedWorkloadsPassed"] = json!(
        (!workloads.extended_history || extended_enabled)
            && (!workloads.stalled_consumer || stalled_enabled)
    );
    report["passed"] = report["allRequestedWorkloadsPassed"].clone();
    power::finish(report);
    save_acceptance_report(evidence, report);
}

async fn fresh_owner_recovery(data: &Path, evidence: &Path, report: &mut Value) {
    report["phase"] = json!("fresh-owner-abandonment");
    save_acceptance_report(evidence, report);
    let owner = start(data, evidence).await;
    let port = owner.port();
    let credential = key(&owner);
    let running = spawn_local(owner.run(pending::<io::Result<()>>()));
    let long = stalled_request();
    let client = connect(port, &credential, &long).await;
    let (client, content_bytes) = timeout(WAIT, read_until_content(client)).await.unwrap();
    report["freshOwnerAbandonmentPrefix"] = json!({"observedVisiblePrefixBytes":content_bytes});
    save_acceptance_report(evidence, report);
    drop(client);
    let abandoned_report = timeout(WAIT, running).await.unwrap().unwrap().unwrap();
    assert_eq!(abandoned_report.exit, RunExit::AdmissionClosed);
    assert!(TcpStream::connect(("127.0.0.1", port)).await.is_err());
    report["abandonmentCleanup"] = cleanup_observation(&abandoned_report, RunExit::AdmissionClosed);
    save_acceptance_report(evidence, report);

    // Fresh ownership after a cancelled generation must work without replay.
    report["phase"] = json!("fresh-owner-complete-stream-and-cleanup");
    save_acceptance_report(evidence, report);
    let owner = start(data, evidence).await;
    let port = owner.port();
    let credential = key(&owner);
    let (stop, stopped) = oneshot::channel();
    let running = spawn_local(owner.run(async { stopped.await.map_err(io::Error::other) }));
    report["freshOwnerStream"] = stream_observation(
        &streamed_response(
            &exchange(
                port,
                &credential,
                &short_request(json!([{"role":"user", "content":"Name a flower."}]), true),
            )
            .await,
        ),
        SHORT_COMPLETION_CAP,
    );
    save_acceptance_report(evidence, report);
    stop.send(()).unwrap();
    let completed_report = timeout(WAIT, running).await.unwrap().unwrap().unwrap();
    assert_eq!(completed_report.exit, RunExit::Shutdown);
    assert!(TcpStream::connect(("127.0.0.1", port)).await.is_err());
    report["freshOwnerCleanup"] = cleanup_observation(&completed_report, RunExit::Shutdown);
    save_acceptance_report(evidence, report);
}

fn optional_workload_enabled(requested: bool, remaining: Duration) -> bool {
    requested && remaining > OPTIONAL_RESERVE
}

fn skipped_workload(requested: bool) -> Value {
    json!({"enabled":false, "requested":requested,
        "status":if requested {"pending"} else {"not-requested"},
        "reason":if requested {Some("insufficient-cleanup-and-recovery-reserve")} else {None},
        "requiredRemainingBudgetSeconds":OPTIONAL_RESERVE.as_secs()})
}

#[test]
fn native_stalled_consumer_retains_fragmented_prefix_connection_until_dropped() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        timeout(Duration::from_secs(10), async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let client = TcpStream::connect(listener.local_addr().unwrap()).await.unwrap();
            let (mut peer, _) = listener.accept().await.unwrap();
            peer.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n")
                .await
                .unwrap();
            let payload = "data: {\"choices\":[{\"delta\":{\"content\":\"cèdre\"},\"finish_reason\":null}]}\n\n";
            let split = payload.find('è').unwrap() + 1;
            for part in [&payload.as_bytes()[..split], &payload.as_bytes()[split..]] {
                peer.write_all(format!("{:X}\r\n", part.len()).as_bytes())
                    .await
                    .unwrap();
                peer.write_all(part).await.unwrap();
                peer.write_all(b"\r\n").await.unwrap();
            }
            let observed = read_until_content(client).await;
            assert_eq!(observed.1, "cèdre".len());
            {
                let mut byte = [0];
                let mut reading = std::pin::pin!(peer.read(&mut byte));
                assert!(
                    std::future::poll_fn(|cx| {
                        std::task::Poll::Ready(reading.as_mut().poll(cx).is_pending())
                    })
                    .await
                );
            }
            drop(observed);
            assert_eq!(peer.read(&mut [0]).await.unwrap(), 0);
        })
        .await
        .unwrap();
    });
}

#[test]
fn native_stalled_consumer_rejects_error_or_unframed_stream_headers() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        timeout(Duration::from_secs(10), async {
            for head in [
                "HTTP/1.1 429 Too Many Requests\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n",
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n",
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 20\r\n\r\n",
            ] {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let client = TcpStream::connect(listener.local_addr().unwrap()).await.unwrap();
                let (mut peer, _) = listener.accept().await.unwrap();
                peer.write_all(head.as_bytes()).await.unwrap();
                let reading = tokio::spawn(read_until_content(client));
                assert!(reading.await.unwrap_err().is_panic());
            }
        })
        .await
        .unwrap();
    });
}

#[test]
fn native_stalled_consumer_never_qualifies_natural_completion_as_interruption() {
    let prefix = stalled_prefix_observation(5);
    assert_eq!(prefix["status"], "pending");
    assert_eq!(prefix["nonreadingPauseCompleted"], false);
    assert_eq!(prefix["observedVisiblePrefixBytes"], 5);
    for field in [
        "brokerOperationInterrupted",
        "ownedJobStopped",
        "freshRestartAfterStopPassed",
    ] {
        assert_eq!(prefix[field], false);
    }
    let interrupted = RunReport {
        exit: RunExit::Shutdown,
        completed: 2,
        failed: 2,
        peak_in_flight: 1,
        backend_resources: None,
    };
    let summary = stalled_stop_summary(&interrupted, 2, 1, 5);
    assert_eq!(summary["nonreadingPauseCompleted"], true);
    assert_eq!(summary["brokerOperationInterrupted"], true);
    assert_eq!(summary["backendGeneratingAtStopKnown"], false);
    assert_eq!(summary["socketBackpressureQualified"], false);
    assert_eq!(summary["performanceQualified"], false);
    for report in [
        RunReport {
            completed: 3,
            failed: 1,
            ..interrupted
        },
        RunReport {
            failed: 1,
            ..interrupted
        },
        RunReport {
            failed: 3,
            ..interrupted
        },
        RunReport {
            exit: RunExit::AdmissionClosed,
            ..interrupted
        },
    ] {
        assert!(std::panic::catch_unwind(|| stalled_stop_summary(&report, 2, 1, 5)).is_err());
    }
    assert!(std::panic::catch_unwind(|| stalled_stop_summary(&interrupted, 2, 1, 0)).is_err());
}

async fn history_progress_fixture(
    listener: tokio::net::TcpListener,
    completed_steps: usize,
    reached: oneshot::Sender<()>,
) {
    for step in 0..completed_steps {
        let (peer, _) = listener.accept().await.unwrap();
        let mut peer = BufReader::new(peer);
        let mut length = None;
        loop {
            let line = prefix_line(&mut peer, 16 * 1024).await;
            if let Some(value) = line.strip_prefix("Content-Length: ") {
                length = Some(value.trim().parse::<usize>().unwrap());
            }
            if line == "\r\n" {
                break;
            }
        }
        let mut body = vec![0; length.unwrap()];
        peer.read_exact(&mut body).await.unwrap();
        let response = if step == 0 {
            let body = r#"{"error":{"code":"context_length_exceeded"}}"#;
            format!(
                "HTTP/1.1 413 Content Too Large\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            )
        } else {
            let payload = concat!(
                "data: {\"choices\":[{\"delta\":{\"content\":\"cedar\"},\"finish_reason\":null}]}\n\n",
                "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"length\"}],\"usage\":{\"completion_tokens\":4}}\n\n",
                "data: [DONE]\n\n"
            );
            format!(
                "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{:X}\r\n{payload}\r\n0\r\n\r\n",
                payload.len()
            )
        };
        peer.get_mut().write_all(response.as_bytes()).await.unwrap();
        peer.get_mut().shutdown().await.unwrap();
    }
    // Hold the next operation open until the caller cancels it.
    let (_peer, _) = listener.accept().await.unwrap();
    reached.send(()).unwrap();
    pending::<()>().await;
}

#[test]
fn native_bounded_history_retains_completed_steps_when_followup_is_cancelled() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        timeout(Duration::from_secs(10), async {
            for completed_steps in [1, 2] {
                let evidence = env::temp_dir().join(format!(
                    "Bonsai-History-Progress-{}-{}-{completed_steps}",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos()
                ));
                fs::create_dir(&evidence).unwrap();
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let port = listener.local_addr().unwrap().port();
                let (reached, waiting) = oneshot::channel();
                let fixture =
                    tokio::spawn(history_progress_fixture(listener, completed_steps, reached));
                let mut report = json!({"passed":false, "allRequestedWorkloadsPassed":false});
                let mut running = Box::pin(extended_acceptance(
                    port,
                    "model-free-fixture",
                    &evidence,
                    &mut report,
                ));
                let mut waiting = std::pin::pin!(waiting);
                let completed = std::future::poll_fn(|cx| {
                    if let std::task::Poll::Ready(result) = waiting.as_mut().poll(cx) {
                        result.unwrap();
                        return std::task::Poll::Ready(false);
                    }
                    running.as_mut().poll(cx).map(|()| true)
                })
                .await;
                assert!(!completed);
                drop(running);
                fixture.abort();
                assert!(fixture.await.unwrap_err().is_cancelled());
                let saved: Value =
                    serde_json::from_slice(&fs::read(evidence.join("acceptance.json")).unwrap())
                        .unwrap();
                assert_eq!(saved["passed"], false);
                assert_eq!(saved["allRequestedWorkloadsPassed"], false);
                let observed = &saved["extendedWorkload"];
                assert_eq!(observed["status"], "pending");
                assert_eq!(observed["contextRejectionPassed"], true);
                assert_eq!(observed["historyStreamPassed"], completed_steps == 2);
                assert_eq!(observed["postCompletionRequestPassed"], false);
                if completed_steps == 2 {
                    assert_eq!(observed["historyStream"]["reportedCompletionTokens"], 4);
                    assert_eq!(observed["historyStream"]["visibleContentBytes"], 5);
                    assert_eq!(observed["phase"], "same-owner-reuse");
                } else {
                    assert_eq!(observed["phase"], "bounded-history-stream");
                }
                fs::remove_file(evidence.join("acceptance.json")).unwrap();
                fs::remove_dir(evidence).unwrap();
            }
        })
        .await
        .unwrap();
    });
}

#[test]
fn native_acceptance_requires_framed_terminal_generation() {
    fn framed(payload: &str) -> Vec<u8> {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n{:X}\r\n{payload}\r\n0\r\n\r\n",
            payload.len()
        ).into_bytes()
    }
    let content =
        "data: {\"choices\":[{\"delta\":{\"content\":\"cedar\"},\"finish_reason\":null}]}\n\n";
    let terminal = "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"length\"}]}\n\n";
    let done = "data: [DONE]\n\n";
    streamed_response(&framed(&format!("{content}{terminal}{done}")));
    let mut truncated = framed(&format!("{content}{terminal}{done}"));
    truncated.pop();
    for invalid in [
        framed(&format!("{content}{terminal}")),
        framed(&format!("{content}{done}")),
        framed(&format!("{terminal}{done}")),
        framed(&format!("{content}{terminal}{content}{done}")),
        framed(&format!("{content}{terminal}{done}{terminal}")),
        truncated,
    ] {
        assert!(std::panic::catch_unwind(|| streamed_response(&invalid)).is_err());
    }
}

#[test]
fn native_extended_acceptance_distinguishes_output_usage_from_delta_count() {
    let framed = |payload: &str| {
        format!(
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{:X}\r\n{payload}\r\n0\r\n\r\n",
            payload.len()
        )
    };
    let content =
        "data: {\"choices\":[{\"delta\":{\"content\":\"cedar\"},\"finish_reason\":null}]}\n\n";
    let terminal = "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n";
    let terminal_usage = "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"completion_tokens\":7}}\n\n";
    let usage = "data: {\"choices\":[],\"usage\":{\"completion_tokens\":7}}\n\n";
    let done = "data: [DONE]\n\n";
    let expected = StreamSummary {
        finish_reason: "stop".into(),
        content_bytes: 5,
        completion_tokens: Some(7),
    };
    for payload in [
        format!("{content}{terminal_usage}{done}"),
        format!("{content}{terminal}{usage}{done}"),
    ] {
        assert_eq!(streamed_response(framed(&payload).as_bytes()), expected);
    }
    assert_eq!(
        streamed_response(framed(&format!("{content}{terminal}{done}")).as_bytes()),
        StreamSummary {
            finish_reason: "stop".into(),
            content_bytes: 5,
            completion_tokens: None,
        }
    );
    let duplicate = framed(&format!("{content}{terminal_usage}{usage}{done}"));
    assert!(std::panic::catch_unwind(|| streamed_response(duplicate.as_bytes())).is_err());
}

#[test]
fn native_lifecycle_observations_separate_generation_usage_and_owned_cleanup() {
    let summary = StreamSummary {
        finish_reason: "length".into(),
        content_bytes: "cèdre".len(),
        completion_tokens: Some(4),
    };
    let observed = stream_observation(&summary, 4);
    assert_eq!(observed["visibleContentBytes"], 6);
    assert_eq!(observed["reportedCompletionTokens"], 4);
    assert_eq!(observed["requestedCompletionCap"], 4);
    assert!(std::panic::catch_unwind(|| stream_observation(&summary, 2)).is_err());
    assert_eq!(
        stream_observation(
            &StreamSummary {
                completion_tokens: None,
                ..summary
            },
            4
        )["reportedCompletionTokens"],
        Value::Null
    );
    let abandoned = RunReport {
        exit: RunExit::AdmissionClosed,
        completed: 0,
        failed: 1,
        peak_in_flight: 1,
        backend_resources: None,
    };
    let cleanup = cleanup_observation(&abandoned, RunExit::AdmissionClosed);
    assert_eq!(cleanup["exit"], "admission-closed");
    assert_eq!(cleanup["completedOperations"], 0);
    assert_eq!(cleanup["failedOperations"], 1);
    assert_eq!(cleanup["ownedJobEmpty"], true);
    assert_eq!(cleanup["listenerReleased"], true);
    assert!(
        std::panic::catch_unwind(|| cleanup_observation(&abandoned, RunExit::Shutdown)).is_err()
    );
    let failed = RunReport {
        exit: RunExit::AcceptFailure,
        ..abandoned
    };
    assert!(
        std::panic::catch_unwind(|| cleanup_observation(&failed, RunExit::AcceptFailure)).is_err()
    );
}

#[test]
fn native_bounded_history_and_stall_requests_preserve_distinct_output_limits() {
    let body = bounded_history_request();
    assert_eq!(body["max_completion_tokens"], 4);
    assert_eq!(body["enable_thinking"], false);
    assert_eq!(body["messages"].as_array().unwrap().len(), 3);
    assert_eq!(body["messages"][1]["role"], "assistant");
    assert_eq!(
        body["messages"][1]["content"],
        format!("Forest notes:{}", " tree".repeat(128))
    );
    let stalled = stalled_request();
    assert_eq!(stalled["max_completion_tokens"], 4096);
    assert_eq!(stalled["enable_thinking"], false);
    assert_eq!(STALLED_CONSUMER_PAUSE, Duration::from_secs(1));
    assert_eq!(STOP_WAIT, Duration::from_secs(30));
    assert_eq!(CASE_BUDGET, Duration::from_secs(240));
    let skipped = skipped_workload(true);
    assert_eq!(skipped["status"], "pending");
    assert_eq!(
        skipped["reason"],
        "insufficient-cleanup-and-recovery-reserve"
    );
    assert_eq!(skipped["requiredRemainingBudgetSeconds"], 120);
    assert_eq!(skipped_workload(false)["status"], "not-requested");
    assert!(!optional_workload_enabled(false, CASE_BUDGET));
    assert!(!optional_workload_enabled(true, OPTIONAL_RESERVE));
    assert!(!optional_workload_enabled(true, Duration::ZERO));
    assert!(optional_workload_enabled(
        true,
        OPTIONAL_RESERVE + Duration::from_secs(1)
    ));
    for (extended, stalled) in [(false, false), (true, false), (false, true), (true, true)] {
        assert_eq!(
            profile_case("pq2-no-adapter", extended, stalled),
            Some(CASE)
        );
    }
}

#[test]
#[ignore = "Actual pinned model; explicitly dispatched main-only device CI, never ordinary suites."]
fn pinned_native_model_streams_recovers_and_restarts_in_required_sandbox() {
    ci_only();
    let budget = case_budget();
    let data = env::var_os("BONSAI_NATIVE_DATA").unwrap();
    let evidence = env::var_os("BONSAI_NATIVE_EVIDENCE").unwrap();
    let data = Path::new(&data);
    let evidence = Path::new(&evidence);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let enabled = |name: &str| {
        let setting = env::var(name);
        assert!(matches!(
            setting.as_deref(),
            Ok("true" | "false") | Err(env::VarError::NotPresent)
        ));
        setting.as_deref() == Ok("true")
    };
    let workloads = Workloads {
        extended_history: enabled("BONSAI_NATIVE_EXTENDED"),
        stalled_consumer: enabled("BONSAI_NATIVE_STALLED_CONSUMER"),
    };
    assert_eq!(
        profile_case(
            "pq2-no-adapter",
            workloads.extended_history,
            workloads.stalled_consumer
        ),
        Some(CASE)
    );
    let mut report = json!({
        "schemaVersion":1, "case":CASE, "revision":env::var("GITHUB_SHA").unwrap(),
        "runId":env::var("GITHUB_RUN_ID").unwrap(), "runAttempt":env::var("GITHUB_RUN_ATTEMPT").unwrap(),
        "scope":"bounded-actual-model-runner-lifecycle", "passed":false,
        "lifecyclePassed":false, "allRequestedWorkloadsPassed":false,
        "aggregateModelCaseBudgetSeconds":budget.as_secs(), "maximumModelCaseBudgetSeconds":maximum_case_budget().as_secs(),
        "wholeJobMaximumSeconds":whole_job_budget().as_secs(),
        "boundedHistoryRequested":workloads.extended_history,
        "stalledConsumerRequested":workloads.stalled_consumer,
        "runtimeLock":serde_json::from_str::<Value>(include_str!("../../../config/prism-inference.lock.json")).unwrap(),
        "modelSha256":"5912bb739217cf25b4283a098c7b893d9baff36bdf2e72d3dbf4cb999d5633d5",
        "requiredNativeSandbox":true, "adapterEnabled":false, "fullContextQualified":false,
        "fullModelNumericalParityQualified":false, "performanceQualified":false,
        "dailyServiceIsolationQualified":false, "defaultRuntimeQualified":false,
        "extendedWorkload":{"enabled":false, "requested":workloads.extended_history, "status":"pending"},
        "stalledConsumerWorkload":{"enabled":false, "requested":workloads.stalled_consumer, "status":"pending"}
    });
    runtime.block_on(LocalSet::new().run_until(async {
        timeout(
            budget,
            acceptance(data, evidence, &workloads, budget, &mut report),
        )
        .await
        .unwrap();
    }));
    // Cleanup and fresh-owner recovery finish before a skipped requested gate
    // fails the CI case. A successful core lifecycle cannot qualify that gate.
    assert_eq!(
        report["passed"], true,
        "A requested workload remains pending."
    );
}

fn profile_case(profile: &str, extended: bool, stalled: bool) -> Option<&'static str> {
    if profile != "pq2-no-adapter" && (extended || stalled) {
        return None;
    }
    match profile {
        "pq2-no-adapter" => Some(CASE),
        PQ2_ADAPTER_SPEED_PROFILE => Some(PQ2_ADAPTER_SPEED_CASE),
        representation::PROFILE => Some(representation::CASE),
        representation::ROW4_PROFILE => Some(representation::ROW4_CASE),
        service_rollback::PROFILE => Some(service_rollback::CASE),
        name => adapter_profile(name).map(|selected| selected.case),
    }
}

fn adapter_profile(name: &str) -> Option<&'static AdapterProfile> {
    match name {
        "ptq1-adapter" => Some(&PTQ1_ADAPTER_PROFILE),
        "ptq1-adapter-lifecycle" => Some(&PTQ1_ADAPTER_LIFECYCLE_PROFILE),
        "ptq1-adapter-lifecycle-row4" => Some(&PTQ1_ADAPTER_LIFECYCLE_ROW4_PROFILE),
        "pq2-adapter-row4" => Some(&PQ2_ADAPTER_ROW4_PROFILE),
        "pq2-adapter-lifecycle-row4" => Some(&PQ2_ADAPTER_LIFECYCLE_PROFILE),
        adapter_boundaries::PROFILE => Some(&adapter_boundaries::ADAPTER_PROFILE),
        service_integration::PROFILE => Some(&service_integration::ADAPTER_PROFILE),
        service_integration::PQ2_PROFILE => Some(&service_integration::PQ2_ADAPTER_PROFILE),
        _ => None,
    }
}

fn adapter_arguments(profile: &AdapterProfile, port: &str) -> Vec<OsString> {
    let mut args: Vec<OsString> = [
        "--launch-development",
        "--require-native-sandbox",
        "--native-engine",
        "--profile",
        "swift-pq2-assessment",
        "--use-abliterate-lora",
        "--frontend-port",
        "0",
        "--backend-port",
        port,
    ]
    .into_iter()
    .map(OsString::from)
    .collect();
    if profile.repacked {
        args.push("--use-repacked-ptq1".into());
    } else {
        // Rollback must keep selecting the retained original artifact even if
        // the application's representation default changes after qualification.
        args.push("--use-original-pq2".into());
    }
    args.extend([
        "--native-row-workers".into(),
        profile.row_workers.to_string().into(),
    ]);
    args
}

fn adapter_configuration(profile: &AdapterProfile, data: &Path, evidence: &Path) -> LaunchConfig {
    let reservation = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = reservation.local_addr().unwrap().port().to_string();
    drop(reservation);
    let mut args = adapter_arguments(profile, &port);
    args.push("--data-directory".into());
    args.push(data.as_os_str().to_owned());
    args.push("--evidence-parent".into());
    args.push(evidence.as_os_str().to_owned());
    LaunchConfig::parse(args).unwrap()
}

async fn start_adapter(profile: &AdapterProfile, data: &Path, evidence: &Path) -> LaunchedService {
    start_adapter_with_progress(profile, data, evidence, |_| {})
        .await
        .unwrap()
}

async fn start_adapter_with_progress(
    profile: &AdapterProfile,
    data: &Path,
    evidence: &Path,
    observe: impl FnMut(StartupProgress),
) -> Result<LaunchedService, LaunchError> {
    adapter_startup_with_timeout(
        adapter_configuration(profile, data, evidence)
            .start_with_progress(pending::<io::Result<()>>(), observe),
    )
    .await
}

async fn adapter_startup_with_timeout<T>(
    startup: impl Future<Output = Result<T, LaunchError>>,
) -> Result<T, LaunchError> {
    // The shared startup cap can expire before an admitted fifteen-minute
    // case's absolute work deadline. Return an ordinary startup failure after
    // dropping its owned future so callers still retain progress and power
    // endpoints instead of unwinding past their terminal receipt.
    timeout(WAIT, startup)
        .await
        .map_err(|_| LaunchError::Backend)?
}

fn adapter_request(messages: Value, scale: f64, limit: u32) -> Value {
    assert!(scale.is_finite() && (0.0..=2.0).contains(&scale));
    assert!([1, 2, 4].contains(&limit));
    let mut body = request(messages, true, limit);
    body["enable_thinking"] = json!(false);
    body["abliteration_scale"] = json!(scale);
    body
}

fn adapter_completion_caps(profile: &AdapterProfile) -> [u32; 3] {
    if minimal_adapter_requests(profile) {
        // The current-release PTQ1 trial exhausted its first four-token request.
        // Keep every required lifecycle transition with less generation work.
        [1, 1, 1]
    } else {
        [4, 2, 2]
    }
}

fn adapter_messages(profile: &AdapterProfile, supplied_history: bool) -> Value {
    if minimal_adapter_requests(profile) {
        if supplied_history {
            json!([
                {"role":"user","content":"Hi"},
                {"role":"assistant","content":"Hi"},
                {"role":"user","content":"Hi"}
            ])
        } else {
            json!([{"role":"user","content":"Hi"}])
        }
    } else if supplied_history {
        json!([
            {"role":"user","content":"Name a tree."},
            {"role":"assistant","content":"Oak."},
            {"role":"user","content":"Name another tree."}
        ])
    } else {
        json!([{"role":"user","content":"Name a tree."}])
    }
}

fn minimal_adapter_requests(profile: &AdapterProfile) -> bool {
    service_integration::is_service_profile(profile)
        || (profile.repacked
            && (profile.required_scale_history || profile.name == adapter_boundaries::PROFILE))
}

fn adapter_summary(label: &str, scale: f64, cap: u32, summary: &StreamSummary) -> Value {
    assert!(["length", "stop"].contains(&summary.finish_reason.as_str()));
    // Visible deltas cannot establish that the worker honored its token cap.
    // Keep malformed/missing usage as a failed observation rather than panicking
    // before the caller can save the response and complete owned teardown.
    let pending_reason = if summary.content_bytes == 0 {
        Some("stream-completed-without-visible-content")
    } else {
        match summary.completion_tokens {
            None => Some("stream-completed-without-reported-token-usage"),
            Some(0) => Some("visible-stream-reported-zero-completion-tokens"),
            Some(tokens) if tokens > u64::from(cap) => Some("stream-reported-over-cap-token-usage"),
            Some(tokens) if summary.finish_reason == "length" && tokens != u64::from(cap) => {
                Some("length-termination-before-reported-token-cap")
            }
            Some(_) => None,
        }
    };
    json!({"phase":label, "requestedScale":scale, "passed":pending_reason.is_none(),
        "pendingReason":pending_reason,
        "requestedScaleF64Bits":format!("{:016x}", scale.to_bits()),
        "requestedCompletionCap":cap, "finishReason":summary.finish_reason,
        "visibleContentBytes":summary.content_bytes,
        "reportedCompletionTokens":summary.completion_tokens})
}

fn save_acceptance_report(evidence: &Path, report: &Value) {
    fs::write(
        evidence.join("acceptance.json"),
        serde_json::to_vec_pretty(report).unwrap(),
    )
    .unwrap();
}

async fn adapter_stream(
    port: u16,
    credential: &str,
    body: &Value,
    label: &str,
    evidence: &Path,
    report: &mut Value,
) -> Value {
    let bytes = progress::try_exchange(port, credential, body, evidence, report, label).await;
    adapter_stream_observation(bytes.as_deref(), body, label)
}

fn adapter_stream_observation(bytes: Option<&[u8]>, body: &Value, label: &str) -> Value {
    let scale = body["abliteration_scale"].as_f64().unwrap();
    let cap = u32::try_from(body["max_completion_tokens"].as_u64().unwrap()).unwrap();
    // These strict reference parsers intentionally assert malformed framing.
    // Contain only their synchronous parsing failure, so a disconnected or
    // malformed service cannot unwind past the caller's owned Stop and power
    // observations. Failed captures never become successful stream evidence.
    let parsed = bytes.and_then(|bytes| {
        std::panic::catch_unwind(|| {
            let mut observed = adapter_summary(label, scale, cap, &framed_stream_response(bytes));
            let content = speed::visible_output(bytes);
            observed["visibleContentSha256"] = json!(adapter_digest(content.as_bytes()));
            observed["visibleContent"] = json!(content);
            observed["reportedPromptTokens"] =
                json!(service_integration::reported_prompt_tokens(bytes));
            observed
        })
        .ok()
    });
    let parsed_response = parsed.is_some();
    let mut observed = parsed.unwrap_or_else(|| {
        json!({"phase":label,"requestedScale":scale,"requestedCompletionCap":cap,
            "requestedScaleF64Bits":format!("{:016x}",scale.to_bits()),"passed":false,
            "pendingReason":if bytes.is_some() {"malformed-or-incomplete-stream-response"}
                else {"stream-transport-failed"},
            "visibleContent":null,"visibleContentBytes":0,"visibleContentSha256":null,
            "reportedPromptTokens":null,"reportedCompletionTokens":null,"finishReason":null})
    });
    observed["responseParsePassed"] = json!(parsed_response);
    observed["responseBytes"] = json!(bytes.map(<[u8]>::len));
    observed["responseSha256"] = json!(bytes.map(adapter_digest));
    observed["requestMessagesSha256"] = json!(adapter_digest(
        &serde_json::to_vec(&body["messages"]).unwrap()
    ));
    observed["requestSha256"] = json!(adapter_digest(&serde_json::to_vec(body).unwrap()));
    observed
}

fn adapter_digest(bytes: &[u8]) -> String {
    bonsai_artifacts::hash_reader(
        &mut io::Cursor::new(bytes),
        u64::try_from(bytes.len()).unwrap(),
        &mut [0; 4096],
        &mut || Ok(()),
    )
    .unwrap()
}

#[test]
fn native_adapter_stream_failures_retain_bound_requests_and_pending_response() {
    let body = service_integration::body(1);
    for bytes in [
        None,
        Some(b"".as_slice()),
        Some(b"HTTP/1.1 200 OK\r\n"),
        Some(b"\xff"),
    ] {
        let observed = adapter_stream_observation(bytes, &body, "failed-stream");
        assert_eq!(observed["passed"], false);
        assert_eq!(observed["responseParsePassed"], false);
        assert_eq!(observed["requestedCompletionCap"], 1);
        assert_eq!(
            observed["requestedScaleF64Bits"],
            format!("{:016x}", 1.3_f64.to_bits())
        );
        assert_eq!(
            observed["requestSha256"],
            adapter_digest(&serde_json::to_vec(&body).unwrap())
        );
        assert_eq!(observed["responseBytes"], json!(bytes.map(<[u8]>::len)));
        assert_eq!(observed["responseSha256"], json!(bytes.map(adapter_digest)));
        assert!(observed["visibleContent"].is_null());
        assert!(observed["reportedCompletionTokens"].is_null());
        assert_eq!(
            observed["pendingReason"],
            if bytes.is_some() {
                "malformed-or-incomplete-stream-response"
            } else {
                "stream-transport-failed"
            }
        );
    }
    let payload = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"length\"}],\"usage\":{\"prompt_tokens\":11,\"completion_tokens\":1}}\n\n",
        "data: [DONE]\n\n"
    );
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n{:X}\r\n{payload}\r\n0\r\n\r\n",
        payload.len()
    );
    let observed = adapter_stream_observation(Some(response.as_bytes()), &body, "complete-stream");
    assert_eq!(observed["passed"], true);
    assert_eq!(observed["responseParsePassed"], true);
    assert_eq!(observed["visibleContent"], "Hello");
    assert_eq!(observed["reportedPromptTokens"], 11);
    assert_eq!(observed["reportedCompletionTokens"], 1);
}

fn generated_adapter_history(profile: &AdapterProfile, report: &Value) -> Option<Value> {
    let initial = &report["streams"][0];
    let content = initial["visibleContent"].as_str()?;
    if initial["phase"] != "required-scale-1.3-stream"
        || initial["passed"] != true
        || initial["requestedScaleF64Bits"] != format!("{:016x}", 1.3_f64.to_bits())
        || initial["requestedCompletionCap"] != adapter_completion_caps(profile)[0]
        || !["stop", "length"].contains(&initial["finishReason"].as_str()?)
        || initial["reportedCompletionTokens"]
            .as_u64()
            .is_none_or(|count| {
                !(1..=u64::from(adapter_completion_caps(profile)[0])).contains(&count)
            })
        || initial["visibleContentBytes"] != content.len()
        || content.is_empty()
        || initial["visibleContentSha256"] != adapter_digest(content.as_bytes())
        || initial["requestMessagesSha256"]
            != adapter_digest(&serde_json::to_vec(&adapter_messages(profile, false)).unwrap())
    {
        return None;
    }
    let mut messages = adapter_messages(profile, true);
    messages[1]["content"] = json!(content);
    Some(messages)
}

async fn adapter_owned_stop(
    stop: oneshot::Sender<()>,
    running: tokio::task::JoinHandle<Result<RunReport, LaunchError>>,
    port: u16,
    allow_closed_admission: bool,
) -> Value {
    let sent = stop.send(());
    assert!(sent.is_ok() || allow_closed_admission);
    let report = timeout(STOP_WAIT, running).await.unwrap().unwrap().unwrap();
    assert!(
        report.exit == RunExit::Shutdown
            || (allow_closed_admission && report.exit == RunExit::AdmissionClosed)
    );
    assert!(TcpStream::connect(("127.0.0.1", port)).await.is_err());
    cleanup_observation(&report, report.exit)
}

async fn required_adapter_scale_history(
    profile: &AdapterProfile,
    port: u16,
    credential: &str,
    evidence: &Path,
    deadline: tokio::time::Instant,
    report: &mut Value,
) -> bool {
    let [_, zero_cap, history_cap] = adapter_completion_caps(profile);
    // Persist each completed observation before admitting another request.
    // A later budget expiry must not erase a successful zero or rejection step.
    for phase in [
        "scale-zero",
        "invalid-scale",
        "scale-change-back-with-history",
    ] {
        report["scaleHistoryWorkload"]["phase"] = json!(phase);
        if deadline.saturating_duration_since(tokio::time::Instant::now())
            <= ADAPTER_LIFECYCLE_RESERVE
        {
            report["scaleHistoryWorkload"]["reason"] = json!("insufficient-owned-cleanup-reserve");
            save_acceptance_report(evidence, report);
            return false;
        }
        save_acceptance_report(evidence, report);
        if phase == "invalid-scale" {
            let mut body = adapter_request(adapter_messages(profile, false), 1.3, zero_cap);
            body["abliteration_scale"] = json!(2.000_000_01);
            // Generic schema failures close the authenticated connection;
            // framed HTTP 400 is reserved for model-control token rejection.
            assert_eq!(exchange(port, credential, &body).await, EMPTY_RESPONSE);
            report["scaleHistoryWorkload"]["invalidScaleRejected"] = json!(true);
            report["scaleHistoryWorkload"]["invalidScaleDisposition"] =
                json!("connection-closed-without-response");
            report["invalidScaleRejected"] = json!(true);
        } else {
            let (scale, messages, cap) = if phase == "scale-zero" {
                (0.0, adapter_messages(profile, false), zero_cap)
            } else {
                let Some(messages) = generated_adapter_history(profile, report) else {
                    report["scaleHistoryWorkload"]["reason"] =
                        json!("missing-bound-initial-model-output");
                    save_acceptance_report(evidence, report);
                    return false;
                };
                // Bind the exact public response and request before awaiting the
                // stream, so cancellation preserves the history actually selected.
                report["scaleHistoryWorkload"]["suppliedAssistantHistorySource"] =
                    report["streams"][0].clone();
                report["scaleHistoryWorkload"]["suppliedAssistantHistoryIsCapturedModelOutput"] =
                    json!(true);
                report["suppliedHistoryMessages"] = messages.clone();
                save_acceptance_report(evidence, report);
                (1.3, messages, history_cap)
            };
            let summary = adapter_stream(
                port,
                credential,
                &adapter_request(messages, scale, cap),
                phase,
                evidence,
                report,
            )
            .await;
            let passed = summary["passed"] == true;
            let pending_reason = summary["pendingReason"].clone();
            report["streams"].as_array_mut().unwrap().push(summary);
            if !passed {
                report["scaleHistoryWorkload"]["reason"] = pending_reason;
                save_acceptance_report(evidence, report);
                return false;
            }
            let field = if phase == "scale-zero" {
                "scaleZeroPassed"
            } else {
                "changeBackHistoryPassed"
            };
            report["scaleHistoryWorkload"][field] = json!(true);
            if phase == "scale-change-back-with-history" {
                report["sameOwnerAfterInvalidScalePassed"] = json!(true);
            }
        }
        save_acceptance_report(evidence, report);
    }
    report["scaleHistoryWorkload"]["phase"] = json!("completed");
    report["scaleHistoryWorkload"]["status"] = json!("passed");
    save_acceptance_report(evidence, report);
    true
}

fn pending_adapter_scale_history() -> Value {
    json!({"status":"pending", "phase":"initial-owner-startup",
        "scaleZeroPassed":false, "invalidScaleRejected":false, "changeBackHistoryPassed":false,
        "suppliedAssistantHistoryIsCapturedModelOutput":false,
        "generatedHistoryStateReuseQualified":false,
        "minimumRemainingBudgetSeconds":ADAPTER_LIFECYCLE_RESERVE.as_secs()})
}

fn begin_adapter_requests(profile: &AdapterProfile, evidence: &Path, report: &mut Value) {
    report["phase"] = json!("required-scale-1.3-stream");
    report["streams"] = json!([]);
    if profile.required_scale_history {
        report["scaleHistoryWorkload"]["phase"] = report["phase"].clone();
    }
    save_acceptance_report(evidence, report);
}

fn adapter_request_budget_expired(profile: &AdapterProfile, evidence: &Path, report: &mut Value) {
    report["requestBudgetExpired"] = json!(true);
    report["requestFailurePhase"] = if profile.required_scale_history {
        report["scaleHistoryWorkload"]["phase"].clone()
    } else {
        report["phase"].clone()
    };
    if profile.required_scale_history {
        report["scaleHistoryWorkload"]["reason"] = json!("request-budget-expired");
    }
    save_acceptance_report(evidence, report);
}

async fn bounded_adapter_acceptance(
    profile: &AdapterProfile,
    data: &Path,
    evidence: &Path,
    budget: Duration,
    report: &mut Value,
) {
    let deadline = tokio::time::Instant::now() + budget;
    power::begin(report);
    record_worker_readiness(report, profile.row_workers, false);
    if profile.required_scale_history {
        report["scaleHistoryWorkload"] = pending_adapter_scale_history();
    }
    report["phase"] = json!("initial-owner-startup");
    save_acceptance_report(evidence, report);
    // The observer records only fixed values in memory; it never performs
    // receipt I/O during credential creation or other startup operations.
    let progress = Mutex::new(Vec::new());
    let Some(owner) = adapter_start_before_cleanup(
        deadline,
        evidence,
        report,
        &progress,
        start_adapter_with_progress(profile, data, evidence, |observation| {
            progress.lock().unwrap().push(observation);
        }),
    )
    .await
    else {
        power::finish(report);
        save_acceptance_report(evidence, report);
        return;
    };
    record_worker_readiness(report, profile.row_workers, true);
    report["adapterEnabled"] = json!(true);
    begin_adapter_requests(profile, evidence, report);
    let port = owner.port();
    let credential = key(&owner);
    let (stop, stopped) = oneshot::channel();
    let running = spawn_local(owner.run(async { stopped.await.map_err(io::Error::other) }));
    // Every HTTP phase, including the first required stream and optional PTQ1
    // requests, must finish before the complete owned-cleanup allowance starts.
    let required_passed = adapter_work_before_cleanup(
        deadline,
        adapter_requests(profile, port, &credential, evidence, deadline, report),
    )
    .await
    .unwrap_or_else(|| {
        adapter_request_budget_expired(profile, evidence, report);
        false
    });
    report["phase"] = json!("owned-cleanup");
    save_acceptance_report(evidence, report);
    report["initialCleanup"] = adapter_owned_stop(stop, running, port, !required_passed).await;
    report["phase"] = json!("completed");
    report["lifecyclePassed"] = json!(true);
    report["allRequestedWorkloadsPassed"] = json!(required_passed);
    report["passed"] = json!(required_passed);
    power::finish(report);
    save_acceptance_report(evidence, report);
}

async fn adapter_work_before_cleanup<T>(
    deadline: tokio::time::Instant,
    work: impl Future<Output = T>,
) -> Option<T> {
    if deadline.saturating_duration_since(tokio::time::Instant::now()) <= STOP_WAIT {
        return None;
    }
    tokio::time::timeout_at(deadline - STOP_WAIT, work)
        .await
        .ok()
}

async fn adapter_start_before_cleanup<T>(
    deadline: tokio::time::Instant,
    evidence: &Path,
    report: &mut Value,
    progress: &Mutex<Vec<StartupProgress>>,
    startup: impl Future<Output = Result<T, LaunchError>>,
) -> Option<T> {
    // Startup cancellation begins at the same request-work deadline. Dropping
    // LaunchConfig's startup future performs bounded owned teardown, which may
    // consume part of the reserve. Synchronous verification is not preemptible.
    // Never wait for a fresh readiness timeout or restart the case clock.
    let started = tokio::time::Instant::now();
    let result = adapter_work_before_cleanup(deadline, startup).await;
    report["startupMilliseconds"] = json!(started.elapsed().as_secs_f64() * 1000.0);
    // The short memory-only guard is released at this statement, before receipt
    // I/O. The callback likewise releases its guard before startup continues.
    report["startupProgress"] = json!(&*progress.lock().unwrap());
    report["startupProgressScope"] = json!(
        "broker operation brackets, including observer overhead; readiness combines unobserved backend verification/loading/graph work and polling; unfinished phases remain pending; synchronous verification is not preemptible"
    );
    report["startupBudgetExpired"] = json!(result.is_none());
    let (owner, pending_reason) = match result {
        Some(Ok(owner)) => (Some(owner), None),
        Some(Err(_)) => (None, Some("startup-failed")),
        None => (None, Some("startup-budget-expired")),
    };
    if let Some(reason) = pending_reason {
        report["phase"] = json!(reason);
        report["pendingReason"] = json!(reason);
        if report["scaleHistoryWorkload"].is_object() {
            report["scaleHistoryWorkload"]["reason"] = json!(reason);
        }
    }
    save_acceptance_report(evidence, report);
    owner
}

async fn adapter_requests(
    profile: &AdapterProfile,
    port: u16,
    credential: &str,
    evidence: &Path,
    deadline: tokio::time::Instant,
    report: &mut Value,
) -> bool {
    let first_cap = adapter_completion_caps(profile)[0];
    let mut streams = vec![
        adapter_stream(
            port,
            credential,
            &adapter_request(adapter_messages(profile, false), 1.3, first_cap),
            "required-scale-1.3-stream",
            evidence,
            report,
        )
        .await,
    ];
    report["streams"] = json!(streams);
    save_acceptance_report(evidence, report);
    if streams[0]["passed"] != true {
        report["pendingReason"] = streams[0]["pendingReason"].clone();
        if profile.required_scale_history {
            report["scaleHistoryWorkload"]["reason"] = report["pendingReason"].clone();
        }
        save_acceptance_report(evidence, report);
        return false;
    }
    if profile.name == adapter_boundaries::PROFILE {
        return adapter_boundaries::requests(port, credential, evidence, deadline, report).await;
    }
    if service_integration::is_service_profile(profile) {
        return adapter_boundaries::requests_before_restart(
            port, credential, evidence, deadline, report,
        )
        .await;
    }
    // Optional requests share this one aggregate budget; no matrix or restart
    // can continue in another job. Preserve a conservative teardown reserve.
    if profile.optional_requests
        && deadline.saturating_duration_since(tokio::time::Instant::now()) > OPTIONAL_RESERVE
    {
        report["phase"] = json!("optional-scale-zero");
        save_acceptance_report(evidence, report);
        streams.push(
            adapter_stream(
                port,
                credential,
                &adapter_request(adapter_messages(profile, false), 0.0, 2),
                "optional-scale-zero",
                evidence,
                report,
            )
            .await,
        );
        report["streams"] = json!(streams);
        save_acceptance_report(evidence, report);
        if streams.last().unwrap()["passed"] != true {
            report["pendingReason"] = streams.last().unwrap()["pendingReason"].clone();
            save_acceptance_report(evidence, report);
            return false;
        }
        if deadline.saturating_duration_since(tokio::time::Instant::now()) > OPTIONAL_RESERVE {
            report["phase"] = json!("optional-invalid-scale-and-reuse");
            save_acceptance_report(evidence, report);
            let mut body =
                adapter_request(json!([{"role":"user","content":"Name a tree."}]), 1.3, 2);
            body["abliteration_scale"] = json!(2.000_000_01);
            assert_eq!(exchange(port, credential, &body).await, EMPTY_RESPONSE);
            report["invalidScaleRejected"] = json!(true);
            report["invalidScaleDisposition"] = json!("connection-closed-without-response");
            save_acceptance_report(evidence, report);
            streams.push(
                adapter_stream(
                    port,
                    credential,
                    &adapter_request(adapter_messages(profile, false), 1.3, 2),
                    "same-owner-after-invalid-scale",
                    evidence,
                    report,
                )
                .await,
            );
            report["streams"] = json!(streams);
            report["sameOwnerAfterInvalidScalePassed"] = streams.last().unwrap()["passed"].clone();
            save_acceptance_report(evidence, report);
            if streams.last().unwrap()["passed"] != true {
                report["pendingReason"] = streams.last().unwrap()["pendingReason"].clone();
                save_acceptance_report(evidence, report);
                return false;
            }
        }
    }
    if profile.required_scale_history {
        report["phase"] = json!("required-scale-history-lifecycle");
        save_acceptance_report(evidence, report);
        required_adapter_scale_history(profile, port, credential, evidence, deadline, report).await
    } else {
        true
    }
}

#[test]
fn native_profile_selection_and_scale_receipts_preserve_bounded_scope() {
    assert_eq!(profile_case("pq2-no-adapter", false, false), Some(CASE));
    assert_eq!(profile_case("pq2-no-adapter", true, true), Some(CASE));
    assert_eq!(
        profile_case("ptq1-adapter", false, false),
        Some(PTQ1_ADAPTER_CASE)
    );
    for (profile, extended, stalled) in [
        ("ptq1-adapter", true, false),
        ("ptq1-adapter", false, true),
        ("pq2-adapter-row4", true, false),
        ("pq2-adapter-row4", false, true),
        ("unknown", false, false),
    ] {
        assert_eq!(profile_case(profile, extended, stalled), None);
    }
    let summary = adapter_summary(
        "synthetic",
        -0.0,
        4,
        &StreamSummary {
            finish_reason: "length".into(),
            content_bytes: 5,
            completion_tokens: Some(4),
        },
    );
    assert_eq!(summary["requestedScaleF64Bits"], "8000000000000000");
    let body = adapter_request(json!([{"role":"user","content":"Name a tree."}]), -0.0, 2);
    assert_eq!(
        body["abliteration_scale"].as_f64().unwrap().to_bits(),
        (-0.0_f64).to_bits()
    );
    assert_eq!(body["max_completion_tokens"], 2);
    assert_eq!(body["enable_thinking"], false);
    assert!(std::panic::catch_unwind(|| adapter_request(json!([]), f64::NAN, 4)).is_err());
    assert!(std::panic::catch_unwind(|| adapter_request(json!([]), 1.3, 4096)).is_err());
    assert_eq!(
        adapter_summary(
            "synthetic",
            1.3,
            4,
            &StreamSummary {
                finish_reason: "length".into(),
                content_bytes: 5,
                completion_tokens: Some(5),
            }
        )["passed"],
        false
    );
}

#[test]
fn native_row4_profile_is_explicit_original_pq2_and_one_stream_only() {
    assert_eq!(
        profile_case("pq2-adapter-row4", false, false),
        Some(PQ2_ADAPTER_ROW4_CASE)
    );
    assert!(adapter_profile("pq2-no-adapter").is_none());
    assert!(adapter_profile("pq2-adapter").is_none());
    for profile in [&PTQ1_ADAPTER_PROFILE, &PQ2_ADAPTER_ROW4_PROFILE] {
        assert_eq!(adapter_profile(profile.name).unwrap().case, profile.case);
        let args = adapter_arguments(profile, "8081");
        for required in [
            "--native-engine",
            "--require-native-sandbox",
            "--use-abliterate-lora",
        ] {
            assert_eq!(args.iter().filter(|arg| *arg == required).count(), 1);
        }
        assert!(
            args.windows(2)
                .any(|pair| pair[0] == "--profile" && pair[1] == "swift-pq2-assessment")
        );
        assert_eq!(
            args.iter().any(|arg| arg == "--use-repacked-ptq1"),
            profile.repacked
        );
        assert_eq!(
            args.iter()
                .filter(|arg| *arg == "--native-row-workers")
                .count(),
            1
        );
        if profile.row_workers == 4 {
            assert!(
                args.windows(2)
                    .any(|pair| pair[0] == "--native-row-workers" && pair[1] == "4")
            );
            assert!(!profile.repacked);
            assert!(!profile.optional_requests);
            assert!(!profile.required_scale_history);
        } else {
            assert_eq!(profile.row_workers, 1);
            assert!(profile.optional_requests);
        }
    }
    let body = adapter_request(json!([{"role":"user","content":"Name a tree."}]), 1.3, 4);
    assert_eq!(body["abliteration_scale"], 1.3);
    assert_eq!(body["max_completion_tokens"], 4);
    assert_eq!(body["enable_thinking"], false);
    assert_eq!(CASE_BUDGET, Duration::from_secs(240));
}

async fn adapter_scale_history_fixture(
    profile: &'static AdapterProfile,
    listener: tokio::net::TcpListener,
    completed_steps: usize,
    reached: oneshot::Sender<()>,
    final_usage: Option<Value>,
) {
    for step in 0..completed_steps {
        let (peer, _) = listener.accept().await.unwrap();
        let mut peer = BufReader::new(peer);
        let mut length = None;
        loop {
            let line = prefix_line(&mut peer, 16 * 1024).await;
            if let Some(value) = line.strip_prefix("Content-Length: ") {
                length = Some(value.trim().parse::<usize>().unwrap());
            }
            if line == "\r\n" {
                break;
            }
        }
        let mut body = vec![0; length.unwrap()];
        peer.read_exact(&mut body).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        let cap = adapter_completion_caps(profile)[if step == 2 { 2 } else { 1 }];
        assert_eq!(body["max_completion_tokens"], cap);
        assert_eq!(body["enable_thinking"], false);
        assert_eq!(body["stream"], true);
        let expected_scale: f64 = [0.0, 2.000_000_01, 1.3][step];
        assert_eq!(
            body["abliteration_scale"].as_f64().unwrap().to_bits(),
            expected_scale.to_bits()
        );
        let messages = if step == 2 {
            generated_adapter_history(
                profile,
                &json!({"streams":[adapter_initial_fixture(profile)]}),
            )
            .unwrap()
        } else {
            adapter_messages(profile, false)
        };
        assert_eq!(body["messages"], messages);
        let response = if step == 1 {
            // Match the production ChatSchema transport disposition.
            String::new()
        } else {
            let mut terminal = json!({"choices":[{"delta":{},"finish_reason":"length"}],
                "usage":{"completion_tokens":cap}});
            if step + 1 == completed_steps
                && let Some(usage) = &final_usage
            {
                if usage.is_null() {
                    terminal.as_object_mut().unwrap().remove("usage");
                } else {
                    terminal["usage"]["completion_tokens"] = usage.clone();
                }
            }
            let payload = format!(
                "data: {{\"choices\":[{{\"delta\":{{\"content\":\"cedar\"}},\"finish_reason\":null}}]}}\n\ndata: {terminal}\n\ndata: [DONE]\n\n"
            );
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n{:X}\r\n{payload}\r\n0\r\n\r\n",
                payload.len()
            )
        };
        peer.get_mut().write_all(response.as_bytes()).await.unwrap();
        peer.get_mut().shutdown().await.unwrap();
    }
    if completed_steps < 3 && final_usage.is_none() {
        let (_peer, _) = listener.accept().await.unwrap();
        reached.send(()).unwrap();
        pending::<()>().await;
    }
}

fn adapter_initial_fixture(profile: &AdapterProfile) -> Value {
    let content = "cèdre\n";
    let cap = adapter_completion_caps(profile)[0];
    let mut captured = adapter_summary(
        "required-scale-1.3-stream",
        1.3,
        cap,
        &StreamSummary {
            finish_reason: "length".into(),
            content_bytes: content.len(),
            completion_tokens: Some(u64::from(cap)),
        },
    );
    captured["visibleContent"] = json!(content);
    captured["visibleContentSha256"] = json!(adapter_digest(content.as_bytes()));
    captured["requestMessagesSha256"] = json!(adapter_digest(
        &serde_json::to_vec(&adapter_messages(profile, false)).unwrap()
    ));
    captured
}

#[test]
fn native_adapter_generated_history_requires_exact_bound_initial_output() {
    let profile = &PTQ1_ADAPTER_LIFECYCLE_PROFILE;
    let report = json!({"streams":[adapter_initial_fixture(profile)]});
    let messages = generated_adapter_history(profile, &report).unwrap();
    assert_eq!(messages[0], adapter_messages(profile, false)[0]);
    assert_eq!(messages[1], json!({"role":"assistant","content":"cèdre\n"}));
    assert_eq!(messages[2], adapter_messages(profile, true)[2]);
    for (field, value) in [
        ("phase", json!("scale-zero")),
        ("passed", json!(false)),
        ("requestedScaleF64Bits", json!("0000000000000000")),
        ("requestedCompletionCap", json!(4)),
        ("finishReason", json!("error")),
        ("reportedCompletionTokens", Value::Null),
        ("reportedCompletionTokens", json!(2)),
        ("visibleContent", json!("fabricated")),
        ("visibleContentBytes", json!(0)),
        ("visibleContentSha256", json!("0".repeat(64))),
        ("requestMessagesSha256", json!("0".repeat(64))),
    ] {
        let mut changed = report.clone();
        changed["streams"][0][field] = value;
        assert!(
            generated_adapter_history(profile, &changed).is_none(),
            "{field}"
        );
    }
    assert!(generated_adapter_history(profile, &json!({"streams":[]})).is_none());
}

#[test]
fn native_adapter_empty_initial_response_keeps_history_pending() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        timeout(Duration::from_secs(10), async {
            let evidence = env::temp_dir().join(format!(
                "Bonsai-Adapter-Empty-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir(&evidence).unwrap();
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = listener.local_addr().unwrap().port();
            let fixture = tokio::spawn(async move {
                let (peer, _) = listener.accept().await.unwrap();
                let mut peer = BufReader::new(peer);
                let mut length = None;
                loop {
                    let line = prefix_line(&mut peer, 16 * 1024).await;
                    if let Some(value) = line.strip_prefix("Content-Length: ") {
                        length = Some(value.trim().parse::<usize>().unwrap());
                    }
                    if line == "\r\n" {
                        break;
                    }
                }
                let mut body = vec![0; length.unwrap()];
                peer.read_exact(&mut body).await.unwrap();
                let payload = concat!(
                    "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"length\"}],\"usage\":{\"completion_tokens\":1}}\n\n",
                    "data: [DONE]\n\n"
                );
                let wire = format!(
                    "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{:X}\r\n{payload}\r\n0\r\n\r\n",
                    payload.len()
                );
                peer.get_mut().write_all(wire.as_bytes()).await.unwrap();
                peer.get_mut().shutdown().await.unwrap();
            });
            let profile = &PTQ1_ADAPTER_LIFECYCLE_PROFILE;
            let mut report = json!({"passed":false, "allRequestedWorkloadsPassed":false,
                "scaleHistoryWorkload":pending_adapter_scale_history()});
            begin_adapter_requests(profile, &evidence, &mut report);
            assert!(!adapter_requests(
                profile,
                port,
                "model-free-fixture",
                &evidence,
                tokio::time::Instant::now() + CASE_BUDGET,
                &mut report,
            ).await);
            fixture.await.unwrap();
            let saved: Value =
                serde_json::from_slice(&fs::read(evidence.join("acceptance.json")).unwrap()).unwrap();
            assert_eq!(saved["streams"].as_array().unwrap().len(), 1);
            assert_eq!(saved["streams"][0]["visibleContent"], "");
            assert_eq!(saved["streams"][0]["visibleContentSha256"], adapter_digest(b""));
            assert_eq!(saved["streams"][0]["reportedCompletionTokens"], 1);
            assert_eq!(saved["pendingReason"], "stream-completed-without-visible-content");
            assert_eq!(saved["scaleHistoryWorkload"]["status"], "pending");
            assert_eq!(saved["scaleHistoryWorkload"]["suppliedAssistantHistoryIsCapturedModelOutput"], false);
            assert_eq!(saved["allRequestedWorkloadsPassed"], false);
            assert!(generated_adapter_history(profile, &saved).is_none());
            fs::remove_file(evidence.join("acceptance.json")).unwrap();
            fs::remove_dir(evidence).unwrap();
        }).await.unwrap();
    });
}

async fn adapter_first_request_fixture(
    listener: tokio::net::TcpListener,
    evidence: std::path::PathBuf,
    reached: oneshot::Sender<()>,
) {
    let (peer, _) = listener.accept().await.unwrap();
    let saved: Value =
        serde_json::from_slice(&fs::read(evidence.join("acceptance.json")).unwrap()).unwrap();
    assert_eq!(saved["verifiedReadinessObserved"], true);
    assert_eq!(
        saved["scaleHistoryWorkload"]["phase"],
        "required-scale-1.3-stream"
    );
    let mut peer = BufReader::new(peer);
    let mut length = None;
    loop {
        let line = prefix_line(&mut peer, 16 * 1024).await;
        if let Some(value) = line.strip_prefix("Content-Length: ") {
            length = Some(value.trim().parse::<usize>().unwrap());
        }
        if line == "\r\n" {
            break;
        }
    }
    let mut body = vec![0; length.unwrap()];
    peer.read_exact(&mut body).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["max_completion_tokens"], 1);
    assert_eq!(body["abliteration_scale"], 1.3);
    reached.send(()).unwrap();
    pending::<()>().await;
}

#[test]
fn native_adapter_first_request_cancellation_retains_ready_phase_and_missing_work() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        timeout(Duration::from_secs(10), async {
            let evidence = env::temp_dir().join(format!(
                "Bonsai-Adapter-First-Request-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir(&evidence).unwrap();
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = listener.local_addr().unwrap().port();
            let (reached, waiting) = oneshot::channel();
            let fixture = tokio::spawn(adapter_first_request_fixture(
                listener,
                evidence.clone(),
                reached,
            ));
            let profile = &PTQ1_ADAPTER_LIFECYCLE_PROFILE;
            let mut report = json!({"passed":false, "allRequestedWorkloadsPassed":false,
                "verifiedReadinessObserved":true,
                "scaleHistoryWorkload":pending_adapter_scale_history()});
            begin_adapter_requests(profile, &evidence, &mut report);
            let deadline = tokio::time::Instant::now() + CASE_BUDGET;
            let mut running = Box::pin(adapter_requests(
                profile,
                port,
                "model-free-fixture",
                &evidence,
                deadline,
                &mut report,
            ));
            let mut waiting = std::pin::pin!(waiting);
            std::future::poll_fn(|cx| {
                if let std::task::Poll::Ready(result) = waiting.as_mut().poll(cx) {
                    result.unwrap();
                    return std::task::Poll::Ready(());
                }
                assert!(running.as_mut().poll(cx).is_pending());
                std::task::Poll::Pending
            })
            .await;
            drop(running);
            adapter_request_budget_expired(profile, &evidence, &mut report);
            // The outer owner advances through cleanup after cancellation.
            // Preserve the exact failing request independently of that phase.
            report["phase"] = json!("completed");
            save_acceptance_report(&evidence, &report);
            fixture.abort();
            assert!(fixture.await.unwrap_err().is_cancelled());
            let saved: Value =
                serde_json::from_slice(&fs::read(evidence.join("acceptance.json")).unwrap())
                    .unwrap();
            assert_eq!(saved["requestFailurePhase"], "required-scale-1.3-stream");
            assert_eq!(saved["requestBudgetExpired"], true);
            assert_eq!(
                saved["scaleHistoryWorkload"]["reason"],
                "request-budget-expired"
            );
            assert_eq!(saved["scaleHistoryWorkload"]["status"], "pending");
            for field in [
                "scaleZeroPassed",
                "invalidScaleRejected",
                "changeBackHistoryPassed",
            ] {
                assert_eq!(saved["scaleHistoryWorkload"][field], false);
            }
            assert_eq!(saved["streams"], json!([]));
            assert_eq!(saved["allRequestedWorkloadsPassed"], false);
            assert_eq!(saved["passed"], false);
            fs::remove_file(evidence.join("acceptance.json")).unwrap();
            fs::remove_dir(evidence).unwrap();
        })
        .await
        .unwrap();
    });
}

#[test]
fn native_adapter_scale_history_retains_partial_steps_without_qualifying_missing_work() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        timeout(Duration::from_secs(10), async {
            for (profile, completed_steps) in [
                &PTQ1_ADAPTER_LIFECYCLE_PROFILE,
                &PTQ1_ADAPTER_LIFECYCLE_ROW4_PROFILE,
                &PQ2_ADAPTER_LIFECYCLE_PROFILE,
            ]
            .into_iter()
            .flat_map(|profile| (0..=3).map(move |step| (profile, step)))
            {
                let evidence = env::temp_dir().join(format!(
                    "Bonsai-Adapter-Progress-{}-{}-{completed_steps}",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos()
                ));
                fs::create_dir(&evidence).unwrap();
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let port = listener.local_addr().unwrap().port();
                let (reached, waiting) = oneshot::channel();
                let fixture = tokio::spawn(adapter_scale_history_fixture(
                    profile,
                    listener,
                    completed_steps,
                    reached,
                    None,
                ));
                let mut report = json!({"passed":false, "allRequestedWorkloadsPassed":false,
                    "streams":[adapter_initial_fixture(profile)],
                    "scaleHistoryWorkload":pending_adapter_scale_history()});
                let deadline = tokio::time::Instant::now()
                    + if completed_steps == 0 {
                        ADAPTER_LIFECYCLE_RESERVE
                    } else {
                        CASE_BUDGET
                    };
                let mut running = Box::pin(required_adapter_scale_history(
                    profile,
                    port,
                    "model-free-fixture",
                    &evidence,
                    deadline,
                    &mut report,
                ));
                let mut waiting = std::pin::pin!(waiting);
                let passed = std::future::poll_fn(|cx| {
                    if completed_steps > 0
                        && completed_steps < 3
                        && let std::task::Poll::Ready(result) = waiting.as_mut().poll(cx)
                    {
                        result.unwrap();
                        return std::task::Poll::Ready(false);
                    }
                    running.as_mut().poll(cx)
                })
                .await;
                drop(running);
                if completed_steps < 3 {
                    fixture.abort();
                    assert!(fixture.await.unwrap_err().is_cancelled());
                } else {
                    fixture.await.unwrap();
                }
                let saved: Value =
                    serde_json::from_slice(&fs::read(evidence.join("acceptance.json")).unwrap())
                        .unwrap();
                let observed = &saved["scaleHistoryWorkload"];
                assert_eq!(passed, completed_steps == 3);
                assert_eq!(
                    observed["status"],
                    if passed { "passed" } else { "pending" }
                );
                assert_eq!(observed["scaleZeroPassed"], completed_steps >= 1);
                assert_eq!(observed["invalidScaleRejected"], completed_steps >= 2);
                assert_eq!(observed["changeBackHistoryPassed"], completed_steps == 3);
                assert_generated_history_retention(profile, completed_steps, &saved);
                assert_eq!(saved["passed"], false);
                assert_eq!(saved["allRequestedWorkloadsPassed"], false);
                assert_eq!(
                    saved["streams"].as_array().unwrap().len(),
                    1 + usize::from(completed_steps >= 1) + usize::from(completed_steps == 3)
                );
                if completed_steps == 0 {
                    assert_eq!(observed["reason"], "insufficient-owned-cleanup-reserve");
                }
                fs::remove_file(evidence.join("acceptance.json")).unwrap();
                fs::remove_dir(evidence).unwrap();
            }
        })
        .await
        .unwrap();
    });
}

#[test]
fn native_adapter_scale_history_requires_reported_bounded_terminal_usage() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        timeout(Duration::from_secs(10), async {
            for profile in [
                &PTQ1_ADAPTER_LIFECYCLE_PROFILE,
                &PTQ1_ADAPTER_LIFECYCLE_ROW4_PROFILE,
                &PQ2_ADAPTER_LIFECYCLE_PROFILE,
            ] {
                for completed_steps in [1, 3] {
                    let cap =
                        adapter_completion_caps(profile)[if completed_steps == 1 { 1 } else { 2 }];
                    let mut cases = vec![
                        (Value::Null, "stream-completed-without-reported-token-usage"),
                        (json!(0), "visible-stream-reported-zero-completion-tokens"),
                        (json!(cap + 1), "stream-reported-over-cap-token-usage"),
                    ];
                    if cap > 1 {
                        cases.push((
                            json!(cap - 1),
                            "length-termination-before-reported-token-cap",
                        ));
                    }
                    for (usage, reason) in cases {
                        let evidence = env::temp_dir().join(format!(
                            "Bonsai-Adapter-Usage-{}-{}",
                            std::process::id(),
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap()
                                .as_nanos()
                        ));
                        fs::create_dir(&evidence).unwrap();
                        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                        let port = listener.local_addr().unwrap().port();
                        let (reached, _waiting) = oneshot::channel();
                        let fixture = tokio::spawn(adapter_scale_history_fixture(
                            profile,
                            listener,
                            completed_steps,
                            reached,
                            Some(usage.clone()),
                        ));
                        let mut report = json!({"passed":false, "allRequestedWorkloadsPassed":false,
                            "streams":[adapter_initial_fixture(profile)],
                            "scaleHistoryWorkload":pending_adapter_scale_history()});
                        assert!(
                            !required_adapter_scale_history(
                                profile,
                                port,
                                "model-free-fixture",
                                &evidence,
                                tokio::time::Instant::now() + CASE_BUDGET,
                                &mut report,
                            )
                            .await
                        );
                        fixture.await.unwrap();
                        let saved: Value = serde_json::from_slice(
                            &fs::read(evidence.join("acceptance.json")).unwrap(),
                        )
                        .unwrap();
                        let observed = &saved["scaleHistoryWorkload"];
                        assert_eq!(observed["status"], "pending");
                        assert_eq!(observed["reason"], reason);
                        assert_eq!(observed["scaleZeroPassed"], completed_steps == 3);
                        assert_eq!(observed["invalidScaleRejected"], completed_steps == 3);
                        assert_eq!(observed["changeBackHistoryPassed"], false);
                        let stream = saved["streams"].as_array().unwrap().last().unwrap();
                        assert_eq!(stream["passed"], false);
                        assert_eq!(stream["pendingReason"], reason);
                        assert_eq!(stream["visibleContent"], "cedar");
                        assert_eq!(stream["reportedCompletionTokens"], usage);
                        assert_eq!(saved["passed"], false);
                        assert_eq!(saved["allRequestedWorkloadsPassed"], false);
                        fs::remove_file(evidence.join("acceptance.json")).unwrap();
                        fs::remove_dir(evidence).unwrap();
                    }
                }
            }
        })
        .await
        .unwrap();
    });
}

fn assert_generated_history_retention(
    profile: &AdapterProfile,
    completed_steps: usize,
    saved: &Value,
) {
    let observed = &saved["scaleHistoryWorkload"];
    assert_eq!(
        observed["suppliedAssistantHistoryIsCapturedModelOutput"],
        completed_steps >= 2
    );
    assert_eq!(observed["generatedHistoryStateReuseQualified"], false);
    if completed_steps >= 2 {
        assert_eq!(
            observed["suppliedAssistantHistorySource"],
            adapter_initial_fixture(profile)
        );
        assert_eq!(saved["suppliedHistoryMessages"][1]["content"], "cèdre\n");
    }
}

#[test]
fn native_adapter_scale_history_profile_requires_separate_explicit_selection() {
    for (name, case, repacked, workers) in [
        (
            "pq2-adapter-lifecycle-row4",
            PQ2_ADAPTER_LIFECYCLE_CASE,
            false,
            4,
        ),
        (
            "ptq1-adapter-lifecycle",
            PTQ1_ADAPTER_LIFECYCLE_CASE,
            true,
            1,
        ),
        (
            "ptq1-adapter-lifecycle-row4",
            PTQ1_ADAPTER_LIFECYCLE_ROW4_CASE,
            true,
            4,
        ),
    ] {
        let selected = adapter_profile(name).unwrap();
        assert_eq!(profile_case(name, false, false), Some(case));
        assert_eq!(profile_case(name, true, false), None);
        assert_eq!(profile_case(name, false, true), None);
        assert!(selected.required_scale_history);
        assert!(!selected.optional_requests);
        assert_eq!(selected.repacked, repacked);
        assert_eq!(selected.row_workers, workers);
        let args = adapter_arguments(selected, "8081");
        assert!(args.iter().any(|arg| arg == "--use-abliterate-lora"));
        assert!(args.iter().any(|arg| arg == "--require-native-sandbox"));
        assert_eq!(
            args.iter().any(|arg| arg == "--use-repacked-ptq1"),
            repacked
        );
        assert_eq!(
            args.windows(2)
                .any(|pair| pair[0] == "--native-row-workers" && pair[1] == "4"),
            workers == 4
        );
    }
    assert!(
        !adapter_profile("pq2-adapter-row4")
            .unwrap()
            .required_scale_history
    );
    assert!(
        !adapter_profile("ptq1-adapter")
            .unwrap()
            .required_scale_history
    );
}

#[test]
fn native_ptq1_row4_lifecycle_is_explicit_and_preserves_the_required_workload() {
    let single = &PTQ1_ADAPTER_LIFECYCLE_PROFILE;
    let parallel = adapter_profile("ptq1-adapter-lifecycle-row4").unwrap();
    assert_ne!(single.name, parallel.name);
    assert_ne!(single.case, parallel.case);
    assert_eq!(single.row_workers, 1);
    assert_eq!(parallel.row_workers, 4);
    assert!(parallel.repacked && parallel.required_scale_history && !parallel.optional_requests);
    assert_eq!(adapter_completion_caps(parallel), [1, 1, 1]);
    for history in [false, true] {
        assert_eq!(
            adapter_messages(single, history),
            adapter_messages(parallel, history)
        );
    }
    for scale in [1.3, 0.0] {
        assert_eq!(
            adapter_request(adapter_messages(single, false), scale, 1),
            adapter_request(adapter_messages(parallel, false), scale, 1)
        );
    }
    let mut expected = adapter_arguments(single, "8081");
    *expected.last_mut().unwrap() = "4".into();
    let mut actual = adapter_arguments(parallel, "8081");
    assert_eq!(actual, expected);
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
    assert!(config.uses_native_engine() && config.uses_repacked_ptq1());
    assert!(config.requires_native_sandbox());
    let report = json!({"streams":[adapter_initial_fixture(parallel)]});
    assert_eq!(
        generated_adapter_history(parallel, &report),
        generated_adapter_history(single, &report)
    );
}

#[test]
fn native_ptq1_lifecycle_minimizes_tokens_without_changing_required_steps() {
    let ptq1 = &PTQ1_ADAPTER_LIFECYCLE_PROFILE;
    assert_eq!(adapter_completion_caps(ptq1), [1, 1, 1]);
    assert_eq!(
        adapter_messages(ptq1, false),
        json!([{"role":"user","content":"Hi"}])
    );
    let history = adapter_messages(ptq1, true);
    assert_eq!(history.as_array().unwrap().len(), 3);
    for (index, role) in ["user", "assistant", "user"].into_iter().enumerate() {
        assert_eq!(history[index], json!({"role":role, "content":"Hi"}));
    }
    assert!(ptq1.required_scale_history && !ptq1.optional_requests);
    assert_eq!(ptq1.row_workers, 1);
    for profile in [
        &PTQ1_ADAPTER_PROFILE,
        &PQ2_ADAPTER_ROW4_PROFILE,
        &PQ2_ADAPTER_LIFECYCLE_PROFILE,
    ] {
        assert_eq!(adapter_completion_caps(profile), [4, 2, 2]);
        assert_eq!(
            adapter_messages(profile, false)[0]["content"],
            "Name a tree."
        );
        assert_eq!(adapter_messages(profile, true)[1]["content"], "Oak.");
    }
    let body = adapter_request(adapter_messages(ptq1, false), 1.3, 1);
    assert_eq!(body["max_completion_tokens"], 1);
    assert_eq!(body["abliteration_scale"], 1.3);
    assert_eq!(body["enable_thinking"], false);
    assert_eq!(body["stream"], true);
    assert!(std::panic::catch_unwind(|| adapter_request(json!([]), 1.3, 0)).is_err());
}

#[test]
fn native_adapter_startup_budget_retains_pending_receipt_and_cleanup_reserve() {
    struct InFlight(std::rc::Rc<std::cell::Cell<bool>>);
    impl Drop for InFlight {
        fn drop(&mut self) {
            self.0.set(true);
        }
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap();
    runtime.block_on(async {
        let evidence = env::temp_dir().join(format!(
            "Bonsai-Adapter-Startup-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&evidence).unwrap();
        for allowance in [STOP_WAIT, STOP_WAIT + Duration::from_secs(1)] {
            let mut report = json!({"passed":false,"lifecyclePassed":false,
                "allRequestedWorkloadsPassed":false,"verifiedReadinessObserved":false,
                "scaleHistoryWorkload":pending_adapter_scale_history()});
            let started = std::cell::Cell::new(false);
            let dropped = std::rc::Rc::new(std::cell::Cell::new(false));
            let in_flight = InFlight(dropped.clone());
            let deadline = tokio::time::Instant::now() + allowance;
            let progress = Mutex::new(Vec::new());
            let owner =
                adapter_start_before_cleanup(deadline, &evidence, &mut report, &progress, async {
                    let _in_flight = in_flight;
                    started.set(true);
                    progress.lock().unwrap().push(StartupProgress {
                        phase: StartupPhase::ModelVerification,
                        completed: false,
                        elapsed_milliseconds: 0.0,
                        phase_milliseconds: None,
                    });
                    pending::<Result<(), LaunchError>>().await
                })
                .await;
            assert!(owner.is_none());
            assert_eq!(started.get(), allowance > STOP_WAIT);
            assert!(dropped.get());
            // This model-free future has no native teardown or synchronous
            // verification; only the async timeout boundary is established.
            assert_eq!(deadline - tokio::time::Instant::now(), STOP_WAIT);
            let saved: Value =
                serde_json::from_slice(&fs::read(evidence.join("acceptance.json")).unwrap())
                    .unwrap();
            assert_eq!(saved["phase"], "startup-budget-expired");
            assert_eq!(saved["pendingReason"], "startup-budget-expired");
            assert_eq!(
                saved["scaleHistoryWorkload"]["reason"],
                "startup-budget-expired"
            );
            assert_eq!(saved["startupBudgetExpired"], true);
            assert_eq!(saved["verifiedReadinessObserved"], false);
            assert_eq!(saved["passed"], false);
            assert_eq!(saved["lifecyclePassed"], false);
            assert_eq!(saved["allRequestedWorkloadsPassed"], false);
            assert_eq!(
                saved["startupProgress"].as_array().unwrap().len(),
                usize::from(started.get())
            );
            if started.get() {
                assert_eq!(saved["startupProgress"][0]["phase"], "model-verification");
                assert_eq!(saved["startupProgress"][0]["completed"], false);
                assert!(saved["startupProgress"][0]["phaseMilliseconds"].is_null());
            }
            // Future-drop ownership is not an independently queried cleanup pass.
            assert!(saved["initialCleanup"].is_null());
        }
        let mut report = json!({"passed":false});
        let deadline = tokio::time::Instant::now() + CASE_BUDGET;
        let progress = Mutex::new(Vec::new());
        assert_eq!(
            adapter_start_before_cleanup(deadline, &evidence, &mut report, &progress, async {
                Ok(7)
            })
            .await,
            Some(7)
        );
        assert_eq!(report["startupBudgetExpired"], false);
        assert_eq!(deadline - tokio::time::Instant::now(), CASE_BUDGET);
        assert!(report["pendingReason"].is_null());
        fs::remove_file(evidence.join("acceptance.json")).unwrap();
        fs::remove_dir(evidence).unwrap();
    });
}

#[test]
fn native_adapter_startup_timeout_retains_failure_before_original_deadline() {
    struct InFlight(std::rc::Rc<std::cell::Cell<bool>>);
    impl Drop for InFlight {
        fn drop(&mut self) {
            self.0.set(true);
        }
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap();
    runtime.block_on(async {
        let evidence = env::temp_dir().join(format!(
            "Bonsai-Adapter-Startup-Timeout-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&evidence).unwrap();
        let deadline = tokio::time::Instant::now() + WAIT + STOP_WAIT + Duration::from_secs(2);
        let mut report = json!({"passed":false,"verifiedReadinessObserved":false});
        let progress = Mutex::new(Vec::new());
        let dropped = std::rc::Rc::new(std::cell::Cell::new(false));
        let in_flight = InFlight(dropped.clone());
        let owner = adapter_start_before_cleanup(
            deadline,
            &evidence,
            &mut report,
            &progress,
            adapter_startup_with_timeout(async {
                let _in_flight = in_flight;
                progress.lock().unwrap().push(StartupProgress {
                    phase: StartupPhase::AuthenticatedReadiness,
                    completed: false,
                    elapsed_milliseconds: 0.0,
                    phase_milliseconds: None,
                });
                pending::<Result<(), LaunchError>>().await
            }),
        )
        .await;
        assert!(owner.is_none());
        assert!(dropped.get());
        assert_eq!(
            deadline - tokio::time::Instant::now(),
            STOP_WAIT + Duration::from_secs(2)
        );
        assert_eq!(report["pendingReason"], "startup-failed");
        assert_eq!(report["startupBudgetExpired"], false);
        assert_eq!(
            report["startupProgress"][0]["phase"],
            "authenticated-readiness"
        );
        assert_eq!(report["startupProgress"][0]["completed"], false);
        assert_eq!(report["verifiedReadinessObserved"], false);
        assert_eq!(report["passed"], false);
        assert!(report["initialCleanup"].is_null());
        let saved: Value =
            serde_json::from_slice(&fs::read(evidence.join("acceptance.json")).unwrap()).unwrap();
        assert_eq!(saved, report);
        assert_eq!(adapter_startup_with_timeout(async { Ok(7) }).await, Ok(7));
        assert_eq!(
            adapter_startup_with_timeout(async { Err::<(), _>(LaunchError::Process) }).await,
            Err(LaunchError::Process)
        );
        fs::remove_file(evidence.join("acceptance.json")).unwrap();
        fs::remove_dir(evidence).unwrap();
    });
}

#[test]
fn native_adapter_startup_error_retains_unfinished_phase_without_qualifying_cleanup() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap();
    runtime.block_on(async {
        let evidence = env::temp_dir().join(format!(
            "Bonsai-Adapter-Startup-Error-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&evidence).unwrap();
        let deadline = tokio::time::Instant::now() + CASE_BUDGET;
        let mut report = json!({"passed":false});
        let progress = Mutex::new(vec![StartupProgress {
            phase: StartupPhase::ProcessCreation,
            completed: false,
            elapsed_milliseconds: 1.0,
            phase_milliseconds: None,
        }]);
        assert_eq!(
            adapter_start_before_cleanup(deadline, &evidence, &mut report, &progress, async {
                Err::<(), _>(LaunchError::Process)
            })
            .await,
            None
        );
        assert_eq!(report["pendingReason"], "startup-failed");
        assert_eq!(report["startupBudgetExpired"], false);
        assert_eq!(report["startupProgress"][0]["phase"], "process-creation");
        assert_eq!(report["startupProgress"][0]["completed"], false);
        assert_eq!(report["passed"], false);
        assert!(report["initialCleanup"].is_null());
        let saved: Value =
            serde_json::from_slice(&fs::read(evidence.join("acceptance.json")).unwrap()).unwrap();
        assert_eq!(saved, report);
        fs::remove_file(evidence.join("acceptance.json")).unwrap();
        fs::remove_dir(evidence).unwrap();
    });
}

#[test]
fn native_adapter_request_budget_drops_inflight_work_before_owned_cleanup() {
    struct InFlight(std::rc::Rc<std::cell::Cell<bool>>);
    impl Drop for InFlight {
        fn drop(&mut self) {
            self.0.set(true);
        }
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap();
    runtime.block_on(async {
        for allowance in [
            Duration::ZERO,
            STOP_WAIT,
            STOP_WAIT + Duration::from_secs(1),
        ] {
            let started = std::cell::Cell::new(false);
            let dropped = std::rc::Rc::new(std::cell::Cell::new(false));
            let in_flight = InFlight(dropped.clone());
            let deadline = tokio::time::Instant::now() + allowance;
            let result = adapter_work_before_cleanup(deadline, async {
                let _in_flight = in_flight;
                started.set(true);
                pending::<()>().await;
            })
            .await;
            assert!(result.is_none());
            assert_eq!(started.get(), allowance > STOP_WAIT);
            // Cancellation releases request-owned state before the caller
            // begins Stop; no request is started inside the cleanup reserve.
            assert!(dropped.get());
            assert_eq!(
                deadline.saturating_duration_since(tokio::time::Instant::now()),
                allowance.min(STOP_WAIT)
            );
        }
        let deadline = tokio::time::Instant::now() + CASE_BUDGET;
        assert_eq!(
            adapter_work_before_cleanup(deadline, async { 7 }).await,
            Some(7)
        );
        assert_eq!(
            deadline.saturating_duration_since(tokio::time::Instant::now()),
            CASE_BUDGET
        );
    });
}

#[test]
#[ignore = "Actual PTQ1 and approved adapter; explicit bounded main-only device CI, never ordinary suites."]
fn pinned_native_ptq1_adapter_starts_and_streams_within_device_budget() {
    ci_only();
    let budget = case_budget();
    assert_eq!(
        profile_case(
            &env::var("BONSAI_NATIVE_PROFILE").unwrap(),
            env::var("BONSAI_NATIVE_EXTENDED").as_deref() == Ok("true"),
            env::var("BONSAI_NATIVE_STALLED_CONSUMER").as_deref() == Ok("true")
        ),
        Some(PTQ1_ADAPTER_CASE)
    );
    let data = env::var_os("BONSAI_NATIVE_DATA").unwrap();
    let evidence = env::var_os("BONSAI_NATIVE_EVIDENCE").unwrap();
    let evidence = Path::new(&evidence);
    let mut report = json!({"schemaVersion":1, "case":PTQ1_ADAPTER_CASE,
        "revision":env::var("GITHUB_SHA").unwrap(), "runId":env::var("GITHUB_RUN_ID").unwrap(),
        "runAttempt":env::var("GITHUB_RUN_ATTEMPT").unwrap(), "scope":"bounded-actual-model-ptq1-adapter-lifecycle",
        "profile":"ptq1-adapter", "passed":false, "requiredNativeSandbox":true,
        "aggregateModelCaseBudgetSeconds":budget.as_secs(), "maximumModelCaseBudgetSeconds":maximum_case_budget().as_secs(),
        "wholeJobMaximumSeconds":whole_job_budget().as_secs(),
        "sameOwnerAfterInvalidScalePassed":false, "invalidScaleRejected":false,
        "unrunIndependentGates":["signed-zero-and-scale-2", "context-rejection-and-reuse", "fresh-owner-restart",
            "supplied-history", "extended-history", "stalled-consumer", "full-context-and-output"],
        "runtimeLock":serde_json::from_str::<Value>(include_str!("../../../config/prism-inference.lock.json")).unwrap(),
        "modelExportReceipt":serde_json::from_str::<Value>(include_str!("../../../config/swift.ptq1-export.json")).unwrap(),
        "adapterLock":serde_json::from_str::<Value>(include_str!("../../../config/swift-lora.lock.json")).unwrap(),
        "adapterRequested":true, "adapterEnabled":false, "verifiedReadinessObserved":false, "completeStateParityQualified":false, "authoringBasisQualified":false,
        "fullContextQualified":false, "fullOutputQualified":false, "performanceQualified":false,
        "qualityQualified":false, "dailyServiceIsolationQualified":false, "defaultRuntimeQualified":false});
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(LocalSet::new().run_until(async {
        timeout(
            budget,
            bounded_adapter_acceptance(
                &PTQ1_ADAPTER_PROFILE,
                Path::new(&data),
                evidence,
                budget,
                &mut report,
            ),
        )
        .await
        .unwrap();
    }));
    assert_eq!(
        report["passed"], true,
        "Adapter acceptance remains pending."
    );
}

#[test]
#[ignore = "Actual original PQ2 and approved adapter with four row workers; explicit bounded main-only device CI."]
fn pinned_native_pq2_adapter_row4_starts_and_streams_within_device_budget() {
    ci_only();
    let budget = case_budget();
    assert_eq!(
        profile_case(
            &env::var("BONSAI_NATIVE_PROFILE").unwrap(),
            env::var("BONSAI_NATIVE_EXTENDED").as_deref() == Ok("true"),
            env::var("BONSAI_NATIVE_STALLED_CONSUMER").as_deref() == Ok("true")
        ),
        Some(PQ2_ADAPTER_ROW4_CASE)
    );
    let runtime_lock =
        serde_json::from_str::<Value>(include_str!("../../../config/prism-inference.lock.json"))
            .unwrap();
    assert!(native_four_worker_release(runtime_lock["version"].as_str()));
    let data = env::var_os("BONSAI_NATIVE_DATA").unwrap();
    let evidence = env::var_os("BONSAI_NATIVE_EVIDENCE").unwrap();
    let evidence = Path::new(&evidence);
    let mut report = json!({"schemaVersion":1, "case":PQ2_ADAPTER_ROW4_CASE,
        "revision":env::var("GITHUB_SHA").unwrap(), "runId":env::var("GITHUB_RUN_ID").unwrap(),
        "runAttempt":env::var("GITHUB_RUN_ATTEMPT").unwrap(), "scope":"bounded-actual-model-pq2-adapter-row4-lifecycle",
        "profile":PQ2_ADAPTER_ROW4_PROFILE.name, "passed":false, "requiredNativeSandbox":true,
        "aggregateModelCaseBudgetSeconds":budget.as_secs(), "maximumModelCaseBudgetSeconds":maximum_case_budget().as_secs(),
        "wholeJobMaximumSeconds":whole_job_budget().as_secs(),
        "configuredNativeRowWorkers":4, "rowWorkerCountIndependentlyObserved":false,
        "requiredStreams":1, "requestedAdapterScale":1.3, "requestedCompletionCap":4,
        "weightRepresentation":"PQ2_0", "runtimeLock":runtime_lock,
        "modelIdentity":serde_json::from_str::<Value>(include_str!("../../../config/swift-assessment.lock.json")).unwrap()["model"],
        "adapterLock":serde_json::from_str::<Value>(include_str!("../../../config/swift-lora.lock.json")).unwrap(),
        "adapterRequested":true, "adapterEnabled":false, "verifiedReadinessObserved":false,
        "sameOwnerAfterInvalidScalePassed":false, "invalidScaleRejected":false,
        "unrunIndependentGates":["multi-scale-and-signed-zero", "invalid-scale-and-reuse", "context-rejection-and-reuse",
            "fresh-owner-restart", "supplied-history", "extended-history", "stalled-consumer", "full-context-and-output",
            "performance-and-TPS", "creative-quality", "daily-service-isolation"],
        "completeStateParityQualified":false, "authoringBasisQualified":false,
        "fullContextQualified":false, "fullOutputQualified":false, "performanceQualified":false,
        "qualityQualified":false, "dailyServiceIsolationQualified":false, "defaultRuntimeQualified":false});
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(LocalSet::new().run_until(async {
        timeout(
            budget,
            bounded_adapter_acceptance(
                &PQ2_ADAPTER_ROW4_PROFILE,
                Path::new(&data),
                evidence,
                budget,
                &mut report,
            ),
        )
        .await
        .unwrap();
    }));
    assert_eq!(
        report["passed"], true,
        "Adapter acceptance remains pending."
    );
}

#[test]
#[ignore = "Actual PQ2 adapter scale/history lifecycle; explicit bounded main-only device CI."]
fn pinned_native_pq2_adapter_row4_checks_scale_change_rejection_and_history() {
    adapter_scale_history_acceptance(&PQ2_ADAPTER_LIFECYCLE_PROFILE);
}

#[test]
#[ignore = "Actual PTQ1 adapter scale/history lifecycle; explicit bounded main-only device CI."]
fn pinned_native_ptq1_adapter_checks_scale_change_rejection_and_history() {
    adapter_scale_history_acceptance(&PTQ1_ADAPTER_LIFECYCLE_PROFILE);
}

#[test]
#[ignore = "Actual PTQ1 adapter scale/history with four row workers; explicit bounded main-only device CI."]
fn pinned_native_ptq1_adapter_row4_checks_scale_change_rejection_and_history() {
    adapter_scale_history_acceptance(&PTQ1_ADAPTER_LIFECYCLE_ROW4_PROFILE);
}

fn adapter_scale_history_acceptance(profile: &AdapterProfile) {
    ci_only();
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
    let representation = if profile.repacked {
        "current-repacked-PTQ1_0"
    } else {
        "PQ2_0"
    };
    let scope = if profile.repacked && profile.row_workers == 4 {
        "bounded-actual-model-ptq1-adapter-scale-history-lifecycle-row4"
    } else if profile.repacked {
        "bounded-actual-model-ptq1-adapter-scale-history-lifecycle"
    } else {
        "bounded-actual-model-pq2-adapter-scale-history-lifecycle"
    };
    let mut report = json!({"schemaVersion":1, "case":profile.case,
        "revision":env::var("GITHUB_SHA").unwrap(), "runId":env::var("GITHUB_RUN_ID").unwrap(),
        "runAttempt":env::var("GITHUB_RUN_ATTEMPT").unwrap(),
        "scope":scope,
        "profile":profile.name, "passed":false, "lifecyclePassed":false,
        "allRequestedWorkloadsPassed":false, "requiredNativeSandbox":true,
        "aggregateModelCaseBudgetSeconds":budget.as_secs(), "maximumModelCaseBudgetSeconds":maximum_case_budget().as_secs(),
        "wholeJobMaximumSeconds":whole_job_budget().as_secs(), "configuredNativeRowWorkers":profile.row_workers,
        "rowWorkerCountIndependentlyObserved":false, "requiredStreams":3,
        "requestedAdapterScales":[1.3,0.0,1.3], "requestedCompletionCaps":adapter_completion_caps(profile),
        "initialMessages":adapter_messages(profile, false),
        "suppliedHistoryMessages":null,
        "weightRepresentation":representation,
        "runtimeLock":serde_json::from_str::<Value>(include_str!("../../../config/prism-inference.lock.json")).unwrap(),
        "modelIdentity":serde_json::from_str::<Value>(include_str!("../../../config/swift-assessment.lock.json")).unwrap()["model"],
        "adapterLock":serde_json::from_str::<Value>(include_str!("../../../config/swift-lora.lock.json")).unwrap(),
        "adapterRequested":true, "adapterEnabled":false, "verifiedReadinessObserved":false,
        "sameOwnerAfterInvalidScalePassed":false, "invalidScaleRejected":false,
        "unrunIndependentGates":["signed-zero-and-scale-2", "context-rejection-and-reuse", "fresh-owner-restart",
            "edited-or-generated-history-reuse", "extended-history", "stalled-consumer", "full-context-and-output",
            "performance-and-TPS", "creative-quality", "daily-service-isolation"],
        "completeStateParityQualified":false, "authoringBasisQualified":false,
        "fullContextQualified":false, "fullOutputQualified":false, "performanceQualified":false,
        "qualityQualified":false, "dailyServiceIsolationQualified":false, "defaultRuntimeQualified":false});
    if profile.repacked {
        let export: Value =
            serde_json::from_str(include_str!("../../../config/swift.ptq1-export.json")).unwrap();
        report["modelIdentity"] = json!({"filename":export["OutputFilename"],
            "bytes":export["OutputBytes"], "sha256":export["OutputSha256"]});
        report["modelExportReceipt"] = export;
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(LocalSet::new().run_until(async {
        timeout(
            budget,
            bounded_adapter_acceptance(
                profile,
                Path::new(&data),
                Path::new(&evidence),
                budget,
                &mut report,
            ),
        )
        .await
        .unwrap();
    }));
    // Admission failure, budget exhaustion or a skipped required step cannot
    // turn a successful initial smoke/owned cleanup into lifecycle acceptance.
    assert_eq!(
        report["passed"], true,
        "Required adapter lifecycle work remains pending."
    );
}

#[test]
fn native_case_budget_rejects_missing_unbounded_and_malformed_allowances() {
    for invalid in [
        None,
        Some(""),
        Some("0"),
        Some("241"),
        Some("3600"),
        Some("-1"),
        Some("+120"),
        Some("120.0"),
        Some(" 120"),
        Some("120\n"),
        Some("18446744073709551616"),
    ] {
        assert_eq!(case_budget_setting(invalid, None), None);
    }
    for seconds in [1, 30, 120, 121, 239, 240] {
        let budget = case_budget_setting(Some(&seconds.to_string()), None).unwrap();
        assert_eq!(budget, Duration::from_secs(seconds));
        // Setup can reduce the allowance below the optional-workload reserve;
        // neither workload may use the original maximum to reopen admission.
        assert_eq!(optional_workload_enabled(true, budget), seconds > 120);
    }
}

#[test]
fn native_case_budget_extension_requires_a_valid_predeclared_total() {
    assert_eq!(
        whole_job_budget_setting(None),
        Some(Duration::from_secs(300))
    );
    assert_eq!(case_budget_setting(Some("241"), None), None);
    for (whole, maximum) in [("300", 240), ("600", 540), ("900", 840)] {
        assert_eq!(
            case_budget_setting(Some(&maximum.to_string()), Some(whole)),
            Some(Duration::from_secs(maximum))
        );
        assert_eq!(
            case_budget_setting(Some(&(maximum + 1).to_string()), Some(whole)),
            None
        );
    }
    for invalid in [
        "",
        "0",
        "299",
        "901",
        "0900",
        "-1",
        "+900",
        "900.0",
        " 900",
        "900\n",
        "18446744073709551616",
    ] {
        assert_eq!(whole_job_budget_setting(Some(invalid)), None);
        assert_eq!(case_budget_setting(Some("1"), Some(invalid)), None);
    }
    assert_eq!(case_budget_setting(None, Some("900")), None);
}

#[test]
fn native_case_budget_cancels_owned_tasks_before_followup() {
    struct Owned(std::rc::Rc<std::cell::Cell<bool>>);
    impl Drop for Owned {
        fn drop(&mut self) {
            self.0.set(true);
        }
    }
    let dropped = std::rc::Rc::new(std::cell::Cell::new(false));
    let followed = std::rc::Rc::new(std::cell::Cell::new(false));
    let owner = Owned(dropped.clone());
    let observed = followed.clone();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap();
    runtime.block_on(LocalSet::new().run_until(async {
        let result = timeout(CASE_BUDGET, async {
            spawn_local(async move {
                let _owner = owner;
                pending::<()>().await;
            });
            tokio::task::yield_now().await;
            pending::<()>().await;
            observed.set(true);
        })
        .await;
        assert!(result.is_err());
    }));
    assert!(dropped.get());
    assert!(!followed.get());
}
