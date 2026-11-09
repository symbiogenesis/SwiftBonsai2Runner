//! One bounded service scale-boundary/reset workload; never a numerical-state verdict.
#![expect(
    clippy::redundant_pub_crate,
    reason = "The private test module shares profile constants and requests only with its parent harness."
)]

use super::*;

pub(super) const PROFILE: &str = "ptq1-adapter-boundaries-row4";
pub(super) const CASE: &str =
    "adapter_boundaries::pinned_native_ptq1_adapter_row4_checks_scale_boundaries_and_reset";
pub(super) const ADAPTER_PROFILE: AdapterProfile = AdapterProfile {
    name: PROFILE,
    case: CASE,
    repacked: true,
    row_workers: 4,
    optional_requests: false,
    required_scale_history: false,
};

fn body(scale: f64) -> Value {
    adapter_request(adapter_messages(&ADAPTER_PROFILE, false), scale, 1)
}

fn context_observation(bytes: &[u8]) -> Value {
    let header_end = bytes.array_windows().position(|part| part == b"\r\n\r\n");
    let parsed = header_end.and_then(|end| {
        if end > 16 * 1024 {
            return None;
        }
        let head = std::str::from_utf8(&bytes[..end]).ok()?;
        let body = &bytes[end + 4..];
        let mut lines = head.split("\r\n");
        let status = lines.next()?;
        let mut length = None;
        for line in lines {
            if line.contains(['\r', '\n']) {
                return None;
            }
            let (name, value) = line.split_once(':')?;
            if name.eq_ignore_ascii_case("transfer-encoding") {
                return None;
            }
            if name.eq_ignore_ascii_case("content-length") {
                let value = value.trim();
                if length.is_some()
                    || value.is_empty()
                    || !value.bytes().all(|byte| byte.is_ascii_digit())
                {
                    return None;
                }
                length = Some(value.parse::<usize>().ok()?);
            }
        }
        Some(
            status == "HTTP/1.1 413 Content Too Large"
                && length == Some(body.len())
                && body == br#"{"error":{"code":"context_length_exceeded"}}"#,
        )
    });
    json!({"responseBytes":bytes.len(),"responseSha256":adapter_digest(bytes),
        "completeHeaderObserved":header_end.is_some(),"contextResponseShapeParsed":parsed.is_some(),
        "expectedContextRejectionObserved":parsed == Some(true),
        "scope":"Exact response byte count and digest plus fixed context-error validation; arbitrary response bytes are not copied into public receipts."})
}

fn replay_matches(first: &Value, replay: &Value) -> bool {
    replay_matches_phase(first, replay, "scale-1.3-reset-replay")
}

pub(super) fn replay_matches_phase(first: &Value, replay: &Value, phase: &str) -> bool {
    first["passed"] == true
        && replay["passed"] == true
        && first["phase"] == "required-scale-1.3-stream"
        && replay["phase"] == phase
        && first["requestedScaleF64Bits"] == format!("{:016x}", 1.3_f64.to_bits())
        && replay["requestedScaleF64Bits"] == first["requestedScaleF64Bits"]
        && first["requestedCompletionCap"] == 1
        && replay["requestedCompletionCap"] == first["requestedCompletionCap"]
        && first["requestMessagesSha256"]
            == adapter_digest(&serde_json::to_vec(&body(1.3)["messages"]).unwrap())
        && replay["requestMessagesSha256"] == first["requestMessagesSha256"]
        && first["visibleContent"].as_str().is_some_and(|text| {
            !text.is_empty()
                && first["visibleContentBytes"] == text.len()
                && first["visibleContentSha256"] == adapter_digest(text.as_bytes())
        })
        && replay["visibleContent"] == first["visibleContent"]
        && replay["visibleContentBytes"] == first["visibleContentBytes"]
        && replay["visibleContentSha256"] == first["visibleContentSha256"]
        && first["reportedCompletionTokens"] == 1
        && replay["reportedCompletionTokens"] == first["reportedCompletionTokens"]
        && ["length", "stop"].contains(&first["finishReason"].as_str().unwrap_or_default())
        && replay["finishReason"] == first["finishReason"]
}

pub(super) async fn requests(
    port: u16,
    credential: &str,
    evidence: &Path,
    deadline: tokio::time::Instant,
    report: &mut Value,
) -> bool {
    boundary_requests(port, credential, evidence, deadline, report, true).await
}

pub(super) async fn requests_before_restart(
    port: u16,
    credential: &str,
    evidence: &Path,
    deadline: tokio::time::Instant,
    report: &mut Value,
) -> bool {
    boundary_requests(port, credential, evidence, deadline, report, false).await
}

async fn boundary_requests(
    port: u16,
    credential: &str,
    evidence: &Path,
    deadline: tokio::time::Instant,
    report: &mut Value,
    same_owner_replay: bool,
) -> bool {
    let phases = [
        "scale-negative-zero",
        "scale-two",
        "invalid-scale",
        "context-rejection",
        "scale-1.3-reset-replay",
    ];
    let required = if same_owner_replay {
        phases.len()
    } else {
        phases.len() - 1
    };
    for &phase in &phases[..required] {
        report["phase"] = json!(phase);
        report["scaleBoundaryWorkload"]["phase"] = json!(phase);
        if deadline.saturating_duration_since(tokio::time::Instant::now())
            <= ADAPTER_LIFECYCLE_RESERVE
        {
            report["scaleBoundaryWorkload"]["pendingReason"] =
                json!("insufficient-request-and-cleanup-reserve");
            save_acceptance_report(evidence, report);
            return false;
        }
        let request = match phase {
            "scale-negative-zero" => body(-0.0),
            "scale-two" => body(2.0),
            "invalid-scale" => {
                let mut request = body(1.3);
                request["abliteration_scale"] = json!(2.000_000_01);
                request
            }
            "context-rejection" => {
                let mut request = body(1.3);
                request["messages"] = json!([{"role":"user","content":" cedar".repeat(9000)}]);
                request["max_completion_tokens"] = json!(4096);
                request
            }
            _ => body(1.3),
        };
        // Bind the exact canonical JSON sent by connect, including the sign bit
        // of negative zero. Requests alone are not a server scale acknowledgement.
        let request_digest = adapter_digest(&serde_json::to_vec(&request).unwrap());
        report["scaleBoundaryWorkload"]["requests"]
            .as_array_mut().unwrap()
            .push(json!({"phase":phase,"requestSha256":request_digest,
                "requestedScaleF64Bits":format!("{:016x}", request["abliteration_scale"].as_f64().unwrap().to_bits()),
                "requestedCompletionCap":request["max_completion_tokens"]}));
        save_acceptance_report(evidence, report);
        let passed = if phase == "invalid-scale" {
            let response =
                progress::try_exchange(port, credential, &request, evidence, report, phase).await;
            let rejected = response.as_deref() == Some(EMPTY_RESPONSE.as_slice());
            report["scaleBoundaryWorkload"]["invalidScaleRejected"] = json!(rejected);
            rejected
        } else if phase == "context-rejection" {
            let bytes =
                progress::try_exchange(port, credential, &request, evidence, report, phase).await;
            let observed = bytes.as_deref().map_or_else(
                || json!({"expectedContextRejectionObserved":false,"transportErrorObserved":true}),
                context_observation,
            );
            let rejected = observed["expectedContextRejectionObserved"] == true;
            report["scaleBoundaryWorkload"]["contextResponse"] = observed;
            report["scaleBoundaryWorkload"]["contextRejected"] = json!(rejected);
            rejected
        } else {
            let mut observed =
                adapter_stream(port, credential, &request, phase, evidence, report).await;
            observed["requestSha256"] = json!(request_digest);
            let mut passed = observed["passed"] == true;
            if phase == "scale-1.3-reset-replay" {
                passed &= replay_matches(&report["streams"][0], &observed);
                report["scaleBoundaryWorkload"]["sameOwnerReplayMatched"] = json!(passed);
                report["sameOwnerAfterInvalidScalePassed"] = json!(passed);
            }
            report["streams"].as_array_mut().unwrap().push(observed);
            passed
        };
        // Drop the large rejected-context fixture before proceeding.
        drop(request);
        save_acceptance_report(evidence, report);
        if !passed {
            report["scaleBoundaryWorkload"]["pendingReason"] =
                json!("required-observation-did-not-pass");
            save_acceptance_report(evidence, report);
            return false;
        }
    }
    report["scaleBoundaryWorkload"]["phase"] = json!(if same_owner_replay {
        "completed"
    } else {
        "awaiting-cancellation-and-fresh-owner-replay"
    });
    report["scaleBoundaryWorkload"]["status"] = json!(if same_owner_replay {
        "passed"
    } else {
        "boundaries-passed"
    });
    save_acceptance_report(evidence, report);
    true
}

#[test]
fn native_adapter_context_rejection_retains_unframed_and_malformed_observations() {
    let body = br#"{"error":{"code":"context_length_exceeded"}}"#;
    let good = format!(
        "HTTP/1.1 413 Content Too Large\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        std::str::from_utf8(body).unwrap()
    );
    assert_eq!(
        context_observation(good.as_bytes())["expectedContextRejectionObserved"],
        true
    );
    for bytes in [
        b"".as_slice(),
        b"HTTP/1.1 413 Content Too Large\r\n",
        good.as_bytes().split_last().unwrap().1,
        b"HTTP/1.1 413 Content Too Large\r\nContent-Length: 0\r\n\r\n",
        b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n",
    ] {
        let observed = context_observation(bytes);
        assert_eq!(observed["responseBytes"], bytes.len());
        assert_eq!(observed["responseSha256"], adapter_digest(bytes));
        assert_eq!(observed["expectedContextRejectionObserved"], false);
    }
    assert_eq!(context_observation(b"")["completeHeaderObserved"], false);
    for extra in [
        "Content-Length: 0",
        "content-length: 44",
        "Transfer-Encoding: chunked",
    ] {
        let ambiguous = good.replace("\r\n\r\n", &format!("\r\n{extra}\r\n\r\n"));
        assert_eq!(
            context_observation(ambiguous.as_bytes())["expectedContextRejectionObserved"],
            false
        );
    }
}

pub(super) fn pending_workload() -> Value {
    json!({"status":"pending","phase":"initial-owner-startup","requests":[],
        "invalidScaleRejected":false,"contextRejected":false,"sameOwnerReplayMatched":false,
        "minimumRemainingBudgetSeconds":ADAPTER_LIFECYCLE_RESERVE.as_secs(),
        "serverScaleAcknowledgementObserved":false,"completeStateParityQualified":false,
        "adapterEffectSizeQualified":false,"cancellationQualified":false,"freshOwnerRestartQualified":false})
}

#[test]
fn native_adapter_boundary_profile_preserves_scope_and_signed_zero() {
    let selected = adapter_profile(PROFILE).unwrap();
    assert_eq!(profile_case(PROFILE, false, false), Some(CASE));
    assert_eq!(profile_case(PROFILE, true, false), None);
    assert_eq!(profile_case(PROFILE, false, true), None);
    assert!(selected.repacked && !selected.optional_requests);
    assert!(!selected.required_scale_history);
    assert_eq!(selected.row_workers, 4);
    assert_eq!(adapter_completion_caps(selected), [1, 1, 1]);
    assert_eq!(
        body(1.3)["messages"],
        json!([{"role":"user","content":"Hi"}])
    );
    for scale in [-0.0_f64, 2.0, 1.3] {
        let encoded = serde_json::to_vec(&body(scale)).unwrap();
        let decoded: Value = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(
            decoded["abliteration_scale"].as_f64().unwrap().to_bits(),
            scale.to_bits()
        );
        assert_eq!(decoded["max_completion_tokens"], 1);
        assert_eq!(decoded["enable_thinking"], false);
    }
    assert_ne!(
        adapter_digest(&serde_json::to_vec(&body(-0.0)).unwrap()),
        adapter_digest(&serde_json::to_vec(&body(0.0)).unwrap())
    );
    let pending = pending_workload();
    for field in [
        "serverScaleAcknowledgementObserved",
        "completeStateParityQualified",
        "adapterEffectSizeQualified",
        "cancellationQualified",
        "freshOwnerRestartQualified",
    ] {
        assert_eq!(pending[field], false);
    }
}

#[test]
fn native_adapter_boundary_replay_requires_bound_output_usage_and_settings() {
    let mut first = adapter_summary(
        "required-scale-1.3-stream",
        1.3,
        1,
        &StreamSummary {
            finish_reason: "length".into(),
            content_bytes: 5,
            completion_tokens: Some(1),
        },
    );
    first["visibleContent"] = json!("Hello");
    first["visibleContentSha256"] = json!(adapter_digest(b"Hello"));
    first["requestMessagesSha256"] = json!(adapter_digest(
        &serde_json::to_vec(&body(1.3)["messages"]).unwrap()
    ));
    let mut replay = first.clone();
    replay["phase"] = json!("scale-1.3-reset-replay");
    assert!(replay_matches(&first, &replay));
    for (field, value) in [
        ("passed", json!(false)),
        ("phase", json!("required-scale-1.3-stream")),
        ("requestedScaleF64Bits", json!("0000000000000000")),
        ("requestedCompletionCap", json!(2)),
        ("requestMessagesSha256", json!("0".repeat(64))),
        ("visibleContent", json!("Other")),
        ("visibleContentBytes", json!(0)),
        ("visibleContentSha256", json!("0".repeat(64))),
        ("reportedCompletionTokens", Value::Null),
        ("finishReason", json!("stop")),
    ] {
        let mut changed = replay.clone();
        changed[field] = value;
        assert!(
            !replay_matches(&first, &changed),
            "Changed {field} cannot qualify replay."
        );
    }
}

#[test]
fn native_adapter_boundary_deadline_retains_pending_work_without_model_start() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let evidence = env::temp_dir().join(format!(
            "Bonsai-Adapter-Boundaries-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&evidence).unwrap();
        let mut report =
            json!({"passed":false,"streams":[],"scaleBoundaryWorkload":pending_workload()});
        let deadline = tokio::time::Instant::now() + ADAPTER_LIFECYCLE_RESERVE;
        assert!(!requests(0, "fixture-not-used", &evidence, deadline, &mut report).await);
        assert_eq!(report["scaleBoundaryWorkload"]["status"], "pending");
        assert_eq!(report["scaleBoundaryWorkload"]["requests"], json!([]));
        assert_eq!(
            report["scaleBoundaryWorkload"]["pendingReason"],
            "insufficient-request-and-cleanup-reserve"
        );
        assert_eq!(report["passed"], false);
        let saved: Value =
            serde_json::from_slice(&fs::read(evidence.join("acceptance.json")).unwrap()).unwrap();
        assert_eq!(saved, report);
        fs::remove_file(evidence.join("acceptance.json")).unwrap();
        fs::remove_dir(evidence).unwrap();
    });
}

#[test]
fn native_adapter_boundary_wire_sequence_preserves_rejections_and_failed_replay() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        timeout(Duration::from_secs(10),async {
            for final_tokens in [1,0] {
                let evidence = env::temp_dir().join(format!("Bonsai-Adapter-Boundary-Wire-{}-{}",
                    std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
                fs::create_dir(&evidence).unwrap();
                let initial = adapter_initial_fixture(&ADAPTER_PROFILE);
                let content = initial["visibleContent"].as_str().unwrap().to_owned();
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let port = listener.local_addr().unwrap().port();
                let fixture = tokio::spawn(async move {
                    for step in 0..5 {
                        let (peer,_) = listener.accept().await.unwrap();
                        let mut peer = BufReader::new(peer);
                        let mut length = None;
                        loop {
                            let line = prefix_line(&mut peer,16*1024).await;
                            if let Some(value) = line.strip_prefix("Content-Length: ") {
                                length = Some(value.trim().parse::<usize>().unwrap());
                            }
                            if line=="\r\n" { break; }
                        }
                        let mut bytes = vec![0;length.unwrap()];
                        peer.read_exact(&mut bytes).await.unwrap();
                        let selected: Value = serde_json::from_slice(&bytes).unwrap();
                        let expected_scale = [-0.0_f64,2.0,2.000_000_01,1.3,1.3][step];
                        assert_eq!(selected["abliteration_scale"].as_f64().unwrap().to_bits(),expected_scale.to_bits());
                        assert_eq!(selected["max_completion_tokens"],if step==3 {4096} else {1});
                        assert_eq!(selected["enable_thinking"],false);
                        assert_eq!(selected["messages"],if step==3 {
                            json!([{"role":"user","content":" cedar".repeat(9000)}])
                        } else { body(1.3)["messages"].clone() });
                        let response = if step==2 {
                            String::new()
                        } else if step==3 {
                            let payload = json!({"error":{"code":"context_length_exceeded"}}).to_string();
                            format!("HTTP/1.1 413 Content Too Large\r\nContent-Length: {}\r\n\r\n{payload}",payload.len())
                        } else {
                            let delta = json!({"choices":[{"delta":{"content":content},"finish_reason":null}]});
                            let terminal = json!({"choices":[{"delta":{},"finish_reason":"length"}],
                                "usage":{"completion_tokens":if step==4 {final_tokens} else {1}}});
                            let payload = format!("data: {delta}\n\ndata: {terminal}\n\ndata: [DONE]\n\n");
                            format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n{:X}\r\n{payload}\r\n0\r\n\r\n",payload.len())
                        };
                        peer.get_mut().write_all(response.as_bytes()).await.unwrap();
                        peer.get_mut().shutdown().await.unwrap();
                    }
                });
                let mut report = json!({"passed":false,"streams":[initial],"scaleBoundaryWorkload":pending_workload()});
                let passed = requests(port,"fixture",&evidence,tokio::time::Instant::now()+Duration::from_mins(2),&mut report).await;
                fixture.await.unwrap();
                assert_eq!(passed,final_tokens==1);
                assert_eq!(report["scaleBoundaryWorkload"]["invalidScaleRejected"],true);
                assert_eq!(report["scaleBoundaryWorkload"]["contextRejected"],true);
                assert_eq!(report["scaleBoundaryWorkload"]["sameOwnerReplayMatched"],final_tokens==1);
                assert_eq!(report["scaleBoundaryWorkload"]["status"],if final_tokens==1 {"passed"} else {"pending"});
                assert_eq!(report["streams"].as_array().unwrap().len(),4);
                assert_eq!(report["scaleBoundaryWorkload"]["requests"].as_array().unwrap().len(),5);
                assert_eq!(report["passed"],false,"Wire fixtures never qualify actual-model acceptance.");
                let saved: Value = serde_json::from_slice(&fs::read(evidence.join("acceptance.json")).unwrap()).unwrap();
                // Compare the complete persisted structure after the same JSON
                // round trip, which may round nondeterministic f64 telemetry.
                let serialized: Value = serde_json::from_slice(&serde_json::to_vec(&report).unwrap()).unwrap();
                assert_eq!(saved,serialized);
                for receipt in [&report,&saved] {
                    let progress = receipt["requestProgress"].as_array().unwrap();
                    assert_eq!(progress.len(),5);
                    assert!(progress.iter().all(|sample| sample["requestMilliseconds"]
                        .as_f64().is_some_and(|milliseconds| milliseconds.is_finite() && milliseconds>0.0)));
                }
                fs::remove_file(evidence.join("acceptance.json")).unwrap();
                fs::remove_dir(evidence).unwrap();
            }
        }).await.unwrap();
    });
}

#[test]
#[ignore = "Actual-model row4 scale-boundary/reset control; one bounded main-only owner CI job."]
fn pinned_native_ptq1_adapter_row4_checks_scale_boundaries_and_reset() {
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
    let export: Value =
        serde_json::from_str(include_str!("../../../../config/swift.ptq1-export.json")).unwrap();
    let mut report = json!({"schemaVersion":1,"case":CASE,"profile":PROFILE,
        "revision":env::var("GITHUB_SHA").unwrap(),"runId":env::var("GITHUB_RUN_ID").unwrap(),
        "runAttempt":env::var("GITHUB_RUN_ATTEMPT").unwrap(),"passed":false,"lifecyclePassed":false,
        "allRequestedWorkloadsPassed":false,"requiredNativeSandbox":true,
        "scope":"bounded-released-service-scale-boundaries-and-visible-reset-replay",
        "aggregateModelCaseBudgetSeconds":budget.as_secs(),"maximumModelCaseBudgetSeconds":maximum_case_budget().as_secs(),
        "wholeJobMaximumSeconds":whole_job_budget().as_secs(),"configuredNativeRowWorkers":4,"requiredStreams":4,
        "requestedAdapterScales":[1.3,-0.0,2.0,1.3],"requestedCompletionCaps":[1,1,1,1],
        "initialMessages":body(1.3)["messages"],"initialRequestSha256":adapter_digest(&serde_json::to_vec(&body(1.3)).unwrap()),
        "servingStatePolicy":"reset-every-request","weightRepresentation":"current-repacked-PTQ1_0",
        "modelIdentity":{"filename":export["OutputFilename"],"bytes":export["OutputBytes"],"sha256":export["OutputSha256"]},
        "modelExportReceipt":export,
        "runtimeLock":serde_json::from_str::<Value>(include_str!("../../../../config/prism-inference.lock.json")).unwrap(),
        "adapterLock":serde_json::from_str::<Value>(include_str!("../../../../config/swift-lora.lock.json")).unwrap(),
        "scaleBoundaryWorkload":pending_workload(),"defaultRuntimeQualified":false,
        "completeStateParityQualified":false,"authoringBasisQualified":false,"performanceQualified":false,
        "qualityQualified":false,"releasedServiceGateQualified":false,
        "remainingIndependentChecks":["adapter-numerical-effect-and-live-state",
            "cancellation-and-fresh-owner-restart","full-context-and-output","installation-and-rollback",
            "quality-and-sustained-comfort","sandbox-isolation"]});
    save_acceptance_report(Path::new(&evidence), &report);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(LocalSet::new().run_until(async {
        timeout(
            budget,
            bounded_adapter_acceptance(
                &ADAPTER_PROFILE,
                Path::new(&data),
                Path::new(&evidence),
                budget,
                &mut report,
            ),
        )
        .await
        .unwrap();
    }));
    assert_eq!(
        report["passed"], true,
        "Required adapter scale-boundary/reset observations remain pending."
    );
}
