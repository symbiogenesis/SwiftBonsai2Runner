use super::*;

fn fixture() -> crate::Result<(Value, Value)> {
    let prompts = ["Find the archive key.", "Follow the format.", "Continue the scene."];
    let ids = json!({"runnerRevision": "a".repeat(40), "prismRevision": "b".repeat(40),
        "runtimeSha256": "c".repeat(64), "modelSha256": "d".repeat(64),
        "adapterSha256": "e".repeat(64), "templateSha256": "f".repeat(64),
        "compilerIdentity": "nightly-2026-11-12 aarch64-pc-windows-msvc",
        "deviceIdentity": "owner-device-1", "processorName": "Snapdragon X2 Elite Extreme",
        "driverIdentity": "reviewed-driver-1", "codecIdentity": "1".repeat(64),
        "samplerSha256": "2".repeat(64), "adapterScale": 1.3, "seed": 42,
        "contextTokens": 8192, "maxCompletionTokens": 256, "rowWorkers": 4,
        "projectionBackend": "cpu"});
    let names = ["deep-retrieval", "instructions", "creative-continuation"];
    let declaration = json!({"identities": ids, "workloads": names.iter().zip(prompts)
        .map(|(id, prompt)| json!({"id": id, "promptSha256": sha256(prompt.as_bytes()),
            "objectiveChecks": ["task-completed"]})).collect::<Vec<_>>()});
    let plan = run(json!({"operation": "plan", "declaration": declaration}))?;
    let answer = "A complete answer.";
    let arm = |profile: &str, input_tokens: u32, private: u64, kv: u64, milliseconds: u32| json!({
        "identities": ids, "kvCache": profile,
        "graphIdentity": format!("model:swift-graph:kv-{profile}:gdn-f32"),
        "inputTokens": input_tokens, "generatedTokens": 16, "occupiedTokens": input_tokens + 16,
        "complete": true, "answer": answer, "answerSha256": sha256(answer.as_bytes()),
        "peakPrivateBytes": private, "peakKvBytes": kv, "kvObservationTokens": input_tokens, "extraFailedOrRetriedRequests": 0, "endToEndMilliseconds": milliseconds,
        "powerBefore": {"acConnected": true, "batterySaver": false, "batteryPercent": 80},
        "powerAfter": {"acConnected": true, "batterySaver": false, "batteryPercent": 80},
        "uninterruptedPowerObserved": true});
    let pairs = names.iter().zip(prompts).map(|(id, prompt)| json!({
        "id": id, "prompt": prompt,
        "baseline": arm("f16", if *id == "deep-retrieval" {7200} else {100},
            8_u64 << 30, 450_u64 << 20, 1000),
        "candidate": arm("rsq4", if *id == "deep-retrieval" {7200} else {100},
            (8_u64 << 30) - (256_u64 << 20), 120_u64 << 20, 1050),
        "quality": {"reviewerId": "independent-human", "blind": true,
            "baselineAnswerSha256": sha256(answer.as_bytes()),
            "candidateAnswerSha256": sha256(answer.as_bytes()), "baselineUsefulness": 4,
            "candidateUsefulness": 4, "majorRegression": false},
        "objectiveChecks": [{"id": "task-completed", "baselinePassed": true, "candidatePassed": true}]
    })).collect::<Vec<_>>();
    let evidence = json!({"planSha256": plan["planSha256"],
        "controls": {"exactPackedRestore": true, "cancellationRecovery": true,
            "editedPrefixRecovery": true, "adapterScaleRecovery": true,
            "explicitF16Rollback": true, "ownedCleanup": true, "noFullHistoryMirror": true, "comparableWholeTaskMemoryCoverage": true}, "pairs": pairs});
    Ok((declaration, evidence))
}

#[test]
fn review_retains_no_go_results_and_rejects_tampered_or_incomplete_pairs() -> crate::Result<()> {
    let (declaration, evidence) = fixture()?;
    let evaluate = |value| run(json!({"operation": "review", "declaration": declaration, "evidence": value}));
    let result = evaluate(evidence.clone())?;
    assert_eq!(result["status"], "ready-for-owner-review");
    assert_eq!(result["promotionApplied"], false);
    for (pointer, value) in [
        ("/controls/exactPackedRestore", json!(false)),
        ("/pairs/0/candidate/peakPrivateBytes", json!(8_u64 << 30)),
        ("/pairs/0/candidate/peakKvBytes", json!(400_u64 << 20)),
        ("/pairs/1/candidate/endToEndMilliseconds", json!(1101)),
        ("/pairs/1/quality/majorRegression", json!(true)),
        ("/pairs/1/quality/candidateUsefulness", json!(2)),
        ("/pairs/2/objectiveChecks/0/candidatePassed", json!(false)),
    ] {
        let mut losing = evidence.clone();
        *losing.pointer_mut(pointer).ok_or("Missing fixture field.")? = value;
        let result = evaluate(losing)?;
        assert_eq!(result["status"], "no-go", "{pointer}");
        assert_eq!(result["pairs"].as_array().map(Vec::len), Some(3));
    }
    for (pointer, value) in [
        ("/planSha256", json!("0".repeat(64))),
        ("/pairs/0/prompt", json!("different input")),
        ("/pairs/0/candidate/answer", json!("changed bytes")),
        ("/pairs/0/candidate/identities/modelSha256", json!("0".repeat(64))),
        ("/pairs/0/candidate/complete", json!(false)),
        ("/pairs/0/candidate/inputTokens", json!(4294967295_u32)),
        ("/pairs/0/candidate/inputTokens", json!(8000)),
        ("/pairs/0/candidate/kvObservationTokens", json!(7199)),
        ("/pairs/0/candidate/extraFailedOrRetriedRequests", json!(1)),
        ("/pairs/0/quality/candidateAnswerSha256", json!("0".repeat(64))),
        ("/pairs/0/quality/blind", json!(false)),
        ("/pairs/0/objectiveChecks/0/id", json!("different criterion")),
    ] {
        let mut invalid = evidence.clone();
        *invalid.pointer_mut(pointer).ok_or("Missing fixture field.")? = value;
        assert!(evaluate(invalid).is_err(), "{pointer}");
    }
    let mut capacity_only = evidence;
    capacity_only["pairs"][0]["candidate"]["powerAfter"]["acConnected"] = Value::Null;
    capacity_only["pairs"][0]["candidate"]["endToEndMilliseconds"] = json!(2000);
    let result = evaluate(capacity_only)?;
    assert_eq!(result["status"], "ready-for-owner-review");
    assert_eq!(result["ownerResponsivenessAcceptanceRequired"], true);
    assert_eq!(result["pairs"][0]["qualifiedLatencyPassed"], Value::Null);
    Ok(())
}
