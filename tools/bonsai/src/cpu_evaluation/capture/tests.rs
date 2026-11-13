//! Independent observation, original deadline and result-comparison controls.
use super::*;

fn observation() -> Value {
    json!({"schema_version":1,"kind":"runner-native-cpu-observations","status":"observed","architecture":"aarch64",
        "scope":"current-process","source":"GetSystemCpuSetInformation","topology":{"logical_processors":18},
        "host_identity":{"status":"observed","source":crate::host_cpu::HOST_IDENTITY_SOURCE,
            "processor_name":"Snapdragon X2 Elite Extreme - X2E94100","processor_identifier":"ARMv8 (64-bit) Family 8 Model 0 Revision 1",
            "vendor_identifier":"Qualcomm Technologies Inc","windows_version":[10,0,26300],"processor_features":[19,43,66]},
        "power_snapshot":{"status":"observed","source":"GetSystemPowerStatus","ac_online":true,"battery_saver":false},
        "power_policy_snapshot":{"status":"observed","source":"PowerGetActiveScheme/PowerRegisterForEffectivePowerModeNotifications",
            "notification_version":2,"active_scheme":"381b4222-f694-41f0-9685-ff5bb260df2e","effective_mode":"balanced"}})
}

fn request() -> crate::Result<Request> {
    serde_json::from_value(json!({"operation":"observed-plan","dataDirectory":std::env::temp_dir(),
        "cimProcessors":[{"name":"Snapdragon X2 Elite Extreme - X2E94100","logicalProcessors":18}]}))
        .map_err(|error|error.to_string())
}

#[test]
fn observed_plan_requires_independent_cim_native_agreement() -> crate::Result<()> {
    let mut request = request()?;
    let native = observation();
    let plan = observed_plan(&request, &native)?;
    assert_eq!(plan["HostIdentity"], native["host_identity"]);
    assert_eq!(plan["InstrumentedAttribution"], false);
    assert_eq!(plan["MaximumSamples"], 4);
    for field in [
        "processor_name",
        "processor_identifier",
        "windows_version",
        "processor_features",
    ] {
        let mut incomplete = native.clone();
        incomplete["host_identity"][field] = Value::Null;
        assert!(observed_plan(&request, &incomplete).is_err());
    }
    request.cim_processors[0].logical_processors = 17;
    assert!(observed_plan(&request, &native).is_err());
    request.cim_processors[0].logical_processors = 18;
    request.cim_processors[0].name = "Snapdragon X2 Elite".into();
    assert!(observed_plan(&request, &native).is_err());
    request.cim_processors.clear();
    assert!(observed_plan(&request, &native).is_err());
    Ok(())
}

#[test]
fn original_workload_clock_never_renews_and_reserves_complete_batch() -> crate::Result<()> {
    assert_eq!(
        allowance(240, Duration::from_secs(10), 24_000, 1000, 100)?,
        210
    );
    assert_eq!(allowance(240, Duration::ZERO, 24_000, 5000, 100)?, 170);
    assert_eq!(
        allowance(100, Duration::from_secs(10), 24_000, 1000, 100)?,
        70
    );
    for (cap, deadline, now, frequency) in [
        (241, 24_000, 1000, 100),
        (240, 0, 0, 100),
        (240, 24_000, 24_000, 100),
        (240, 24_000, 22_000, 100),
        (240, 24_000, 1000, 0),
    ] {
        assert!(allowance(cap, Duration::ZERO, deadline, now, frequency).is_err());
    }
    let mut plan = observed_plan(&request()?, &observation())?;
    assert_eq!(batch_allowance(201, &plan)?, 191);
    assert!(batch_allowance(200, &plan).is_err());
    plan["MaximumSamples"] = 2.into();
    assert!(batch_allowance(220, &plan).is_err());
    Ok(())
}

#[test]
fn executable_receipt_requires_current_source_revision_and_bounded_digest() -> crate::Result<()> {
    let receipt = json!({"SchemaVersion":3,"Revision":"a".repeat(40),"Sha256":"b".repeat(64)});
    assert_eq!(
        maintenance_identity(&receipt, &"a".repeat(40), 1024)?,
        "b".repeat(64)
    );
    assert!(maintenance_identity(&receipt, &"c".repeat(40), 1024).is_err());
    for length in [0, 134_217_729] {
        assert!(maintenance_identity(&receipt, &"a".repeat(40), length).is_err());
    }
    for (field, value) in [
        ("SchemaVersion", json!(1)),
        ("SchemaVersion", json!(2)),
        ("Sha256", json!("invalid")),
        ("Revision", Value::Null),
    ] {
        let mut changed = receipt.clone();
        changed[field] = value;
        assert!(maintenance_identity(&changed, &"a".repeat(40), 1024).is_err());
    }
    Ok(())
}

fn pair() -> [Value; 2] {
    ["baseline","candidate"].map(|role|json!({"Role":role,"CandidateId":"d6-p6","Workload":"short","Repetition":1,
        "ReachedTokenCap":true,"Timing":{"PromptTokens":100,"DecodeTokens":15,"PromptMilliseconds":if role == "baseline" {20} else {10},
            "DecodeMilliseconds":if role == "baseline" {40} else {10}},"WallMilliseconds":if role == "baseline" {100} else {50},
        "ResponseSha256":"a".repeat(64),"CpuConditions":{"Before":observation(),"After":observation()}}))
}

#[test]
fn pair_ratios_require_uninstrumented_stable_complete_fixed_token_observations() -> crate::Result<()>
{
    let execution = json!({"Comparable":true,"Status":"observed-stable"});
    let pair = pair();
    let compared = compare_pair(&pair, &execution, false)?;
    assert_eq!(compared["ComparableAtFixedTokenCap"], true);
    assert_eq!(compared["CandidateOverBaselinePromptMilliseconds"], 0.5);
    assert_eq!(compared["CandidateOverBaselineDecodeMilliseconds"], 0.25);
    assert_eq!(compared["CandidateOverBaselineWallMilliseconds"], 0.5);
    assert_eq!(
        compare_pair(&[pair[1].clone(), pair[0].clone()], &execution, false)?,
        compared
    );
    let mut cases = vec![
        (pair.clone(), execution.clone(), true),
        (
            pair.clone(),
            json!({"Comparable":false,"Status":"unknown"}),
            false,
        ),
    ];
    for (field, value) in [
        ("ReachedTokenCap", json!(false)),
        ("WallMilliseconds", json!(0)),
        ("CpuConditions", Value::Null),
    ] {
        let mut changed = pair.clone();
        changed[1][field] = value;
        cases.push((changed, execution.clone(), false));
    }
    let mut changed = pair.clone();
    changed[1]["Timing"]["PromptTokens"] = 101.into();
    cases.push((changed, execution.clone(), false));
    let mut changed = pair;
    changed[1]["CpuConditions"]["After"]["power_snapshot"]["ac_online"] = false.into();
    cases.push((changed, execution, false));
    for (pair, execution, instrumented) in cases {
        let compared = compare_pair(&pair, &execution, instrumented)?;
        assert_eq!(compared["ComparableAtFixedTokenCap"], false);
        for field in [
            "CandidateOverBaselinePromptMilliseconds",
            "CandidateOverBaselineDecodeMilliseconds",
            "CandidateOverBaselineWallMilliseconds",
        ] {
            assert!(compared[field].is_null());
        }
    }
    Ok(())
}

#[test]
fn records_reject_bad_capture_identity_and_retain_output_attribution() -> crate::Result<()> {
    let trial = json!({"Id":"trial","Role":"baseline"});
    let mut sample = json!({"Id":"trial","Status":"completed","Counts":{"GeneratedSampledTokens":16},
        "ProcessMemory":{"OutputProgress":{"FirstObservedNonemptyStdoutMilliseconds":42}}});
    let mut capture =
        json!({"SchemaVersion":1,"Text":"fixture","Sha256":crate::identity::sha256(b"fixture")});
    let result = record(&trial, &sample, &capture, &json!(16))?;
    assert_eq!(result["ProcessMemory"], sample["ProcessMemory"]);
    assert_eq!(result["ReachedTokenCap"], true);
    sample["Status"] = "failed".into();
    assert!(record(&trial, &sample, &capture, &json!(16)).is_err());
    sample["Status"] = "completed".into();
    capture["Text"] = "modified".into();
    assert!(record(&trial, &sample, &capture, &json!(16)).is_err());
    Ok(())
}

#[test]
fn atomic_phase_evidence_survives_failed_publication_and_enforces_read_bounds() -> crate::Result<()>
{
    let directory = std::env::temp_dir().join(format!(
        "Bonsai-X2Report-{}",
        crate::startup_process::unique()
    ));
    fs::create_dir(&directory).map_err(|error| error.to_string())?;
    let path = directory.join("cpu-evaluation.json");
    save(
        &path,
        &json!({"Status":"preparing","ModelTrialsRequested":false}),
    )?;
    let failed = json!({"Status":"failed","ModelTrialsRequested":false,"WorkerPreflight":{"Status":"failed"}});
    save(&path, &failed)?;
    assert_eq!(read_json(&path, 1024)?, failed);
    assert!(read_json(&path, 1).is_err());
    assert!(save(&path, &json!({"oversized":"x".repeat(16*1024*1024)})).is_err());
    assert_eq!(read_json(&path, 1024)?, failed);
    fs::remove_dir_all(directory).map_err(|error| error.to_string())?;
    Ok(())
}
