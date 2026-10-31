use super::*;

fn policy_fixture() -> crate::Result<Value> {
    policy(&"a".repeat(40), 1, None)
}

fn evidence(policy: &Value) -> crate::Result<Value> {
    Ok(
        json!({"schemaVersion": 2, "policySha256": sha256(&serde_json::to_vec(policy).map_err(|error| error.to_string())?), "identities": policy["identities"], "gates": []}),
    )
}

fn bind_service(input: &mut Value, policy: &Value) -> crate::Result<()> {
    let record = input["gates"]
        .as_array_mut()
        .and_then(|records| {
            records
                .iter_mut()
                .find(|record| record["id"] == SERVICE_GATE)
        })
        .ok_or("Missing released service test record.")?;
    record["execution"]["revision"] = policy["identities"]["serviceHarnessRevision"].clone();
    record["testedRuntime"] = tested_runtime(policy);
    record["harness"] = json!({
        "workflowPath": ".github/workflows/private-service-acceptance.yml",
        "artifactId": "456",
        "artifactName": format!("private-service-check-{}-123-1", policy["identities"]["serviceHarnessRevision"].as_str().ok_or("Missing harness revision.")?),
        "package": record["report"]
    });
    Ok(())
}

#[test]
fn missing_evidence_stays_pending_and_policy_binds_standard_profile() -> crate::Result<()> {
    let policy = policy_fixture()?;
    let result = score(evidence(&policy)?, &policy, &std::env::temp_dir())?;
    assert_eq!(result["status"], "pending");
    assert_eq!(result["promotionReady"], false);
    assert_eq!(result["gates"].as_array().map(Vec::len), Some(GATES.len()));
    assert_eq!(policy["identities"]["adapterScale"], 1.3);
    assert_eq!(policy["identities"]["contextTokens"], 8192);
    assert_eq!(policy["identities"]["maxCompletionTokens"], 4096);
    assert_eq!(policy["identities"]["nativeRowWorkers"], 1);
    assert_eq!(
        policy["identities"]["servingStatePolicy"],
        "reset-every-request"
    );
    assert!(
        policy["identities"]["adapterSha256"]
            .as_str()
            .is_some_and(|id| is_lower_hex(id, 64))
    );
    let plan = run(json!({"operation": "plan", "runnerRevision": "a".repeat(40)}))?;
    assert_eq!(plan["policy"], policy);
    assert_eq!(
        score(
            plan["evidenceTemplate"].clone(),
            &policy,
            &std::env::temp_dir()
        )?["status"],
        "pending"
    );
    Ok(())
}

#[test]
fn row_worker_evidence_cannot_qualify_a_different_execution_policy() -> crate::Result<()> {
    let single = policy_fixture()?;
    let parallel = policy(&"a".repeat(40), 4, None)?;
    let plan = run(json!({
        "operation": "plan", "runnerRevision": "a".repeat(40), "nativeRowWorkers": 4
    }))?;
    assert_eq!(plan["policy"], parallel);
    assert_eq!(parallel["identities"]["nativeRowWorkers"], 4);
    assert!(score(evidence(&single)?, &parallel, &std::env::temp_dir()).is_err());
    assert!(score(evidence(&parallel)?, &single, &std::env::temp_dir()).is_err());
    for value in [
        json!(0),
        json!(2),
        json!(8),
        json!(256),
        json!(-1),
        json!("4"),
        Value::Null,
    ] {
        assert!(
            run(json!({
                "operation": "plan", "runnerRevision": "a".repeat(40), "nativeRowWorkers": value
            }))
            .is_err()
        );
    }
    let mut reused = evidence(&parallel)?;
    reused["identities"]["servingStatePolicy"] = json!("same-session-live-prefix");
    assert!(score(reused, &parallel, &std::env::temp_dir()).is_err());
    Ok(())
}

#[test]
fn stale_artifacts_unknown_duplicate_and_array_records_reject() -> crate::Result<()> {
    let policy = policy_fixture()?;
    let mut input = evidence(&policy)?;
    input["identities"]["adapterScale"] = json!(1);
    assert!(score(input, &policy, &std::env::temp_dir()).is_err());
    for records in [
        json!([[]]),
        json!([{"id":"unknown", "status":"pending", "note":"pending"}]),
        json!([{"id":"runner-foundation", "status":"pending", "note":"pending"},{"id":"runner-foundation", "status":"pending", "note":"pending"}]),
    ] {
        let mut input = evidence(&policy)?;
        input["gates"] = records;
        assert!(score(input, &policy, &std::env::temp_dir()).is_err());
    }
    Ok(())
}

fn assert_service_rejections(input: &Value, policy: &Value, directory: &Path) -> crate::Result<()> {
    for (pointer, value) in [
        ("/schemaVersion", json!(1)),
        ("/gates/0/testedRuntime", Value::Null),
        (
            "/gates/0/testedRuntime/revision",
            policy["identities"]["serviceHarnessRevision"].clone(),
        ),
        ("/gates/0/testedRuntime/workerSha256", json!("c".repeat(64))),
        ("/gates/0/testedRuntime/serverSha256", json!("c".repeat(64))),
        (
            "/gates/0/testedRuntime/archiveSha256",
            json!("c".repeat(64)),
        ),
        ("/gates/0/testedRuntime/version", json!("0.0.0")),
        ("/gates/0/harness", Value::Null),
        (
            "/gates/0/harness/workflowPath",
            json!(".github/workflows/work.yml"),
        ),
        ("/gates/0/harness/artifactId", json!("0")),
        ("/gates/0/harness/artifactName", json!("unbound-harness")),
        ("/gates/0/harness/package/file", json!("../harness.zip")),
        ("/gates/0/harness/package/sha256", json!("c".repeat(64))),
        (
            "/gates/0/execution/revision",
            policy["identities"]["runtimeRevision"].clone(),
        ),
        ("/gates/0/execution/gitRef", json!("refs/heads/candidate")),
        ("/gates/0/reviewerId", Value::Null),
    ] {
        let mut changed = input.clone();
        *changed
            .pointer_mut(pointer)
            .ok_or("Missing service provenance fixture field.")? = value;
        assert!(score(changed, policy, directory).is_err(), "{pointer}");
    }
    for gate in ["runtime-foundation", "runtime-conformance"] {
        let mut changed = input.clone();
        changed["gates"][0]["id"] = json!(gate);
        let _ = changed["gates"][0]
            .as_object_mut()
            .ok_or("Missing gate.")?
            .remove("testedRuntime");
        let _ = changed["gates"][0]
            .as_object_mut()
            .ok_or("Missing gate.")?
            .remove("harness");
        assert!(score(changed.clone(), policy, directory).is_err());
        changed["gates"][0]["execution"]["revision"] =
            policy["identities"]["runtimeRevision"].clone();
        assert_eq!(score(changed, policy, directory)?["status"], "pending");
    }
    Ok(())
}

#[test]
fn later_service_harness_preserves_exact_release_and_retained_package() -> crate::Result<()> {
    let harness_revision = "b".repeat(40);
    let policy = policy(&"a".repeat(40), 1, Some(&harness_revision))?;
    let plan = run(
        json!({"operation": "plan", "runnerRevision": "a".repeat(40), "serviceHarnessRevision": harness_revision}),
    )?;
    assert_eq!(plan["policy"], policy);
    for invalid in [
        json!(""),
        json!("B".repeat(40)),
        json!("b".repeat(39)),
        json!(1),
    ] {
        assert!(run(json!({"operation": "plan", "runnerRevision": "a".repeat(40), "serviceHarnessRevision": invalid})).is_err());
    }
    assert_ne!(
        policy["identities"]["serviceHarnessRevision"],
        policy["identities"]["runtimeRevision"]
    );
    let directory =
        std::env::temp_dir().join(format!("Prism-Service-Provenance-{}", std::process::id()));
    std::fs::create_dir(&directory).map_err(|error| error.to_string())?;
    let report = b"reviewed released service capture";
    let package = b"retained hosted harness package";
    std::fs::write(directory.join("report.json"), report).map_err(|error| error.to_string())?;
    std::fs::write(directory.join("harness.zip"), package).map_err(|error| error.to_string())?;
    let result = (|| {
        let mut input = evidence(&policy)?;
        input["gates"] = json!([{
            "id": SERVICE_GATE, "status": "passed", "note": "Independent release/harness review.",
            "report": {"file": "report.json", "sha256": sha256(report)},
            "execution": {"repository": PRISM, "revision": harness_revision,
                "gitRef": "refs/heads/main", "runId": "123", "runAttempt": 1,
                "architecture": "ARM64", "deviceModel": "Snapdragon X Elite", "selfHosted": true},
            "reviewerId": "independent-reviewer"
        }]);
        bind_service(&mut input, &policy)?;
        input["gates"][0]["harness"]["package"] =
            json!({"file": "harness.zip", "sha256": sha256(package)});
        let scored = score(input.clone(), &policy, &directory)?;
        assert_eq!(scored["status"], "pending");
        assert_eq!(scored["gates"][16]["status"], "passed");
        assert_eq!(scored["gates"][16]["executionRevision"], harness_revision);
        assert_eq!(
            scored["gates"][16]["testedRuntime"],
            tested_runtime(&policy)
        );
        assert_eq!(
            scored["gates"][16]["harness"]["packageSha256"],
            sha256(package)
        );
        assert_service_rejections(&input, &policy, &directory)?;
        std::fs::write(directory.join("harness.zip"), b"changed")
            .map_err(|error| error.to_string())?;
        assert!(score(input, &policy, &directory).is_err());
        Ok(())
    })();
    for name in ["report.json", "harness.zip"] {
        std::fs::remove_file(directory.join(name)).map_err(|error| error.to_string())?;
    }
    std::fs::remove_dir(&directory).map_err(|error| error.to_string())?;
    result
}

#[test]
fn reviewed_pass_requires_report_device_main_and_human_identity() -> crate::Result<()> {
    let policy = policy_fixture()?;
    let directory = std::env::temp_dir().join(format!(
        "Prism-Promotion-Test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos()
    ));
    std::fs::create_dir(&directory).map_err(|error| error.to_string())?;
    let report = b"retained reviewed report";
    std::fs::write(directory.join("report.json"), report).map_err(|error| error.to_string())?;
    let mut input = evidence(&policy)?;
    input["gates"] = json!(GATES.iter().map(|gate| json!({"id":gate.id, "status":"passed", "note":"Review checked the exact retained report and required scope.", "report":{"file":"report.json","sha256":sha256(report)}, "execution":{"repository":gate.repository,"revision":if gate.repository == PRISM {policy["identities"]["runtimeRevision"].clone()} else {policy["identities"]["runnerRevision"].clone()},"gitRef":"refs/heads/main","runId":"123","runAttempt":1,"architecture":"ARM64","deviceModel":"Snapdragon X Elite","selfHosted":true},"reviewerId":"external-reviewer","humanReviewed":gate.human})).collect::<Vec<_>>());
    bind_service(&mut input, &policy)?;
    let result = (|| {
        assert_eq!(
            score(input.clone(), &policy, &directory)?["status"],
            "passed"
        );
        let mut x2 = input.clone();
        for record in x2["gates"]
            .as_array_mut()
            .ok_or("Missing promotion gates.")?
        {
            if GATES
                .iter()
                .any(|gate| gate.device && record["id"] == gate.id)
            {
                record["execution"]["deviceModel"] = json!("Snapdragon X2 Elite Extreme");
                record["execution"]["processorName"] =
                    json!("Snapdragon(R) X2 Elite Extreme - X2E-96-100");
            }
        }
        let x2_result = score(x2.clone(), &policy, &directory)?;
        assert_eq!(x2_result["status"], "passed");
        assert_eq!(
            x2_result["gates"][4]["deviceModel"],
            "Snapdragon X2 Elite Extreme"
        );
        x2["gates"][4]["execution"] = input["gates"][4]["execution"].clone();
        assert!(
            score(x2, &policy, &directory).is_err(),
            "Mixed X1/X2 device evidence was accepted."
        );
        let mut mislabeled = input.clone();
        mislabeled["gates"][4]["execution"]["processorName"] = json!("Snapdragon X2 Elite Extreme");
        assert!(score(mislabeled, &policy, &directory).is_err());
        let manifest = directory.join("promotion.json");
        std::fs::write(
            &manifest,
            serde_json::to_vec(&input).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        assert_eq!(
            run(
                json!({"operation": "file-score", "runnerRevision": "a".repeat(40), "evidencePath": manifest})
            )?["promotionReady"],
            true
        );
        for (pointer, value) in [
            (
                "/gates/4/execution/deviceModel",
                json!("generic hosted ARM64"),
            ),
            ("/gates/4/execution/gitRef", json!("refs/heads/work")),
            ("/gates/4/reviewerId", Value::Null),
            ("/gates/10/humanReviewed", json!(false)),
            ("/gates/4/report/file", json!("../report.json")),
            ("/gates/4/report/sha256", json!("0".repeat(64))),
        ] {
            let mut changed = input.clone();
            *changed.pointer_mut(pointer).ok_or("Missing test field.")? = value;
            assert!(score(changed, &policy, &directory).is_err(), "{pointer}");
        }
        input["gates"][4]["status"] = json!("rejected");
        input["gates"][5]["status"] = json!("pending");
        assert_eq!(score(input, &policy, &directory)?["status"], "rejected");
        Ok(())
    })();
    std::fs::remove_file(directory.join("report.json")).map_err(|error| error.to_string())?;
    std::fs::remove_file(directory.join("promotion.json")).map_err(|error| error.to_string())?;
    std::fs::remove_dir(&directory).map_err(|error| error.to_string())?;
    result
}
