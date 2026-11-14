use super::*;

struct Fixture {
    root: PathBuf,
    packet: Value,
    owner: Value,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // This uniquely created fixture directory contains only files written by
        // this test; never remove its parent or any caller-selected directory.
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn save(root: &Path, name: &str, value: &Value) -> Result<Value> {
    let bytes = serde_json::to_vec(value).map_err(|error| error.to_string())?;
    std::fs::write(root.join(name), &bytes).map_err(|error| error.to_string())?;
    Ok(json!({"path":name,"sha256":crate::identity::sha256(&bytes)}))
}

fn snapshot() -> Value {
    json!({"schema_version":1,"kind":"runner-native-cpu-observations","status":"observed",
        "architecture":"aarch64","scope":"current-process","source":"GetSystemCpuSetInformation",
        "topology":{"logical_processors":18,"cpu_sets":[{"id":1}]},
        "host_identity":{"status":"observed","source":crate::host_cpu::HOST_IDENTITY_SOURCE,
            "processor_name":"Snapdragon X2 Elite Extreme - X2E94100",
            "processor_identifier":"ARMv8 Family 8","vendor_identifier":"Qualcomm",
            "windows_version":[10,0,26300],"processor_features":[19,43,66]},
        "power_snapshot":{"status":"observed","source":"GetSystemPowerStatus",
            "ac_online":true,"battery_saver":false,"battery_percent":80},
        "power_policy_snapshot":{"status":"observed","source":"PowerGetActiveScheme/PowerRegisterForEffectivePowerModeNotifications",
            "notification_version":2,"active_scheme":"381b4222-f694-41f0-9685-ff5bb260df2e","effective_mode":"balanced"}})
}

fn build(revision: &str) -> Value {
    let phases = [
        "source_analysis",
        "neon_helper_analysis",
        "hash_helper_analysis",
        "portable_library",
        "state_fixture",
        "collector_analysis",
        "collector_controls",
        "collector_link",
    ];
    let checks: Vec<_> = phases.into_iter().map(|phase| {
        let command = canonical_command(phase, "nightly-2026-11-09-aarch64-pc-windows-msvc");
        let passed: Vec<_> = match phase {
            "collector_controls" => CONTROLS.to_vec(),
            "state_fixture" => vec![STATE_CONTROL],
            _ => Vec::new(),
        };
        json!({"phase":phase,"command":command,"exit_code":0,"passed":passed,"failed":0,"ignored":0,"measured":0})
    }).collect();
    let inputs: BTreeMap<_, _> = INPUTS
        .into_iter()
        .map(|name| (name, "a".repeat(64)))
        .collect();
    json!({"source_revision":revision,"clean":true,"compiler":"rustc nightly exact host commit",
        "toolchain":"nightly-2026-11-09-aarch64-pc-windows-msvc", "rustc_sha256":"3".repeat(64),"cargo_sha256":"4".repeat(64),
        "helper_source_sha256":{"bonsai-native-hash":"5".repeat(64),"bonsai-neon-dot":"6".repeat(64)},
        "target":TARGET,"features":FEATURES,"inputs":inputs,"executable_sha256":"b".repeat(64),
        "executable_bytes":100,"checks":checks})
}

fn fixture() -> Result<Fixture> {
    let root = std::env::temp_dir().join(format!(
        "bonsai-cpu-source-{}",
        crate::startup_process::unique()
    ));
    std::fs::create_dir(&root).map_err(|error| error.to_string())?;
    let messages = save(
        &root,
        "messages.json",
        &json!([{"role":"user","content":"Count from 1 to 10. Answer with only the numbers."}]),
    )?;
    let evaluator = std::fs::read(std::env::current_exe().map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())?;
    let packet = json!({"schema_version":1,"kind":"prism-cpu-source-comparison-preparation",
        "runner_revision":"c".repeat(40),"evaluator_sha256":crate::identity::sha256(&evaluator),
        "profile":{"kv_cache":"k4-v4","context_tokens":8192,"cpu_prefill_tokens":1,"cpu_prefill_scratch_bytes":0},
        "messages":messages,"output_tokens":32,"model_sha256":MODEL,"adapter_sha256":ADAPTER,
        "schedule":SCHEDULE,"arms":{"baseline":build(&"1".repeat(40)),"candidate":build(&"2".repeat(40))}});
    let observation = snapshot();
    let mut arms = Vec::new();
    for (index, role) in SCHEDULE.into_iter().enumerate() {
        let order = u64::try_from(index).map_err(|error| error.to_string())?;
        let begin = 1000 + order * 100_000;
        let deadline = begin + 60_000;
        let unix = 1_000_000 + deadline;
        let text = "1 2 3 4 5 6 7 8 9 10";
        let receipt = json!({"schema_version":2,"kind":"swift-cpu-source-candidate-arm",
            "source_commit_declaration":packet["arms"][role]["source_revision"],
            "model_sha256":MODEL,"model_bytes":7_206_168_928_u64,"weight_representation":"PQ2_0",
            "adapter_sha256":ADAPTER,"observed_adapter_sha256":ADAPTER,"adapter_scale_bits":"3fa66666",
            "row_workers":4,"effective_row_workers":4,"backend_requested":"ExperimentalNeon",
            "execution_device":"cpu","execution_basis":"direct-session-without-projection-executor",
            "backend_statistics":null,"kv_cache":"k4-v4","context_tokens":8192,"cpu_prefill_tokens":1,
            "cpu_prefill_scratch_bytes":0,"legacy_f16_build":false,"prompt_reuse":"disabled","seed":42,
            "thinking_enabled":false,"maximum_prompt_tokens":128,"output_tokens_reserved":32,
            "original_deadline_unix_milliseconds":unix,"receipt_reserve_milliseconds":2000,
            "sampler":"production SwiftSampler and model-declared stops",
            "executable_sha256":"b".repeat(64),"input_sha256":packet["messages"]["sha256"],
            "arm_succeeded":true,"session_succeeded":true,"terminal_delivered":true,
            "terminal_accounting_matches":true,"model_loaded":true,"output_committed":true,
            "capture_status":"complete","stage":"complete","error":null,"prompt_tokens":20,
            "generated_tokens":10,"reused_tokens":0,"finish":"stop","output_text":text,
            "output_bytes":text.len(),"output_sha256":crate::identity::sha256(text.as_bytes()),
            "load_milliseconds":10,"preparation_milliseconds":1,"request_milliseconds":20,"elapsed_milliseconds":40});
        let mut started = receipt.clone();
        for field in [
            "arm_succeeded",
            "session_succeeded",
            "terminal_delivered",
            "terminal_accounting_matches",
            "model_loaded",
            "output_committed",
        ] {
            started[field] = false.into();
        }
        started["stage"] = "started".into();
        started["capture_status"] = "incomplete".into();
        let supervision = json!({"Status":"completed","ExitCode":0,"ModelProcessStarted":true,
            "MinimumAvailablePhysicalBytes":5_000_000_000_u64,"PeakJobMemoryBytes":10_000_000_000_u64,
            "WallMilliseconds":if role == "candidate" {80} else {100},
            "CpuConditions":{"Before":observation,"After":observation}});
        arms.push(json!({"id":format!("arm-{}", index+1),"role":role,"started_tick":begin,
            "execution_deadline_tick":deadline,"collector_deadline_unix_milliseconds":unix,
            "cleanup_completed_tick":begin+200,"power_before":observation["power_snapshot"],
            "power_after":observation["power_snapshot"],"owned_job_empty":true,"cleanup_succeeded":true,
            "minimum_admission_bytes":16_642_998_272_u64,"maximum_job_memory_bytes":12_348_030_976_u64,
            "receipt":save(&root,&format!("arm-{index}-final.json"),&receipt)?,
            "started_receipt":save(&root,&format!("arm-{index}-started.json"),&started)?,
            "supervision":save(&root,&format!("arm-{index}-supervision.json"),&supervision)?}));
    }
    let owner = json!({"schema_version":1,"kind":"prism-cpu-source-comparison-owner",
        "preparation_sha256":"d".repeat(64),"workload_predeclared":true,"completed":true,
        "budget_seconds":600,"fixed_work":"Four complete ABBA arms, each reserving 110 seconds; intake60 and retention100.",
        "estimated_seconds":540,"budget_reason":"Four loading and complete-request arms cannot fit five minutes.",
        "clock_frequency":1000,"started_tick":0,"deadline_tick":600_000,
        "original_deadline_unix_milliseconds":1_600_000,"retention_completed_tick":400_000,
        "power_before":observation["power_snapshot"],"power_after":observation["power_snapshot"],"arms":arms});
    Ok(Fixture {
        root,
        packet,
        owner,
    })
}

impl Fixture {
    fn request(&mut self) -> Result<Value> {
        let packet = save(&self.root, "preparation.json", &self.packet)?;
        self.owner["preparation_sha256"] = packet["sha256"].clone();
        let owner = save(&self.root, "owner.json", &self.owner)?;
        Ok(
            json!({"operation":"evaluate-source-comparison","preparationPath":self.root.join("preparation.json"),
            "preparationSha256":packet["sha256"],"ownerPath":self.root.join("owner.json"),"ownerSha256":owner["sha256"]}),
        )
    }
}

#[test]
fn source_comparison_replays_checked_pairs_and_retains_losing_outcomes() -> Result<()> {
    let mut fixture = fixture()?;
    let result = super::super::run(fixture.request()?)?;
    assert_eq!(result["status"], "admitted");
    assert_eq!(result["short_request_benefit_observed"], true);
    assert_eq!(result["promotion_qualified"], false);
    assert_eq!(result["continuous_power_stability_proven"], false);
    for index in [1, 2] {
        let reference = fixture.owner["arms"][index]["supervision"].clone();
        let path = fixture
            .root
            .join(reference["path"].as_str().ok_or("Missing path.")?);
        let mut supervisor = json_bytes(&std::fs::read(path).map_err(|error| error.to_string())?)?;
        supervisor["WallMilliseconds"] = 120.into();
        fixture.owner["arms"][index]["supervision"] = save(
            &fixture.root,
            reference["path"].as_str().ok_or("Missing path.")?,
            &supervisor,
        )?;
    }
    let losing = super::super::run(fixture.request()?)?;
    assert_eq!(losing["status"], "admitted");
    assert_eq!(losing["short_request_benefit_observed"], false);
    Ok(())
}

#[test]
fn source_comparison_rejects_legacy_profiles_unbound_builds_and_missing_controls() -> Result<()> {
    let mut fixture = fixture()?;
    let original = fixture.packet.clone();
    for pointer in [
        "/schema_version",
        "/profile/cpu_prefill_tokens",
        "/profile/cpu_prefill_scratch_bytes",
        "/output_tokens",
    ] {
        fixture.packet = original.clone();
        *fixture
            .packet
            .pointer_mut(pointer)
            .ok_or("Missing mutation field.")? = 7.into();
        assert!(super::super::run(fixture.request()?).is_err(), "{pointer}");
    }
    fixture.packet = original.clone();
    fixture.packet["arms"]["candidate"]["checks"][6]["passed"] = json!([]);
    assert!(super::super::run(fixture.request()?).is_err());
    fixture.packet = original.clone();
    fixture.packet["arms"]["candidate"]["checks"][1]["command"] = json!([
        "cargo",
        "clippy",
        "--release",
        "--offline",
        "--locked",
        "--all-targets",
        "-D",
        "warnings"
    ]);
    assert!(super::super::run(fixture.request()?).is_err());
    fixture.packet = original;
    fixture.packet["arms"]["candidate"]["inputs"]["examples/cpu_candidate_check.rs"] =
        "e".repeat(64).into();
    assert!(super::super::run(fixture.request()?).is_err());
    let clean = build(&"2".repeat(40));
    fixture.packet["arms"]["candidate"] = clean.clone();
    for suppressor in ["--ignored", "--list", "--skip", "--no-run"] {
        fixture.packet["arms"]["candidate"] = clean.clone();
        fixture.packet["arms"]["candidate"]["checks"][6]["command"]
            .as_array_mut()
            .ok_or("Missing command.")?
            .push(suppressor.into());
        assert!(
            super::super::run(fixture.request()?).is_err(),
            "{suppressor}"
        );
    }
    Ok(())
}

#[test]
fn source_comparison_rejects_unknown_power_scratch_deadlines_cleanup_and_escaping_members()
-> Result<()> {
    let mut fixture = fixture()?;
    let original = fixture.owner.clone();
    for (pointer, replacement) in [
        ("/power_before/ac_online", Value::Null),
        ("/arms/1/power_after/ac_online", false.into()),
        ("/arms/1/minimum_admission_bytes", 1.into()),
        ("/arms/1/owned_job_empty", false.into()),
        ("/arms/1/collector_deadline_unix_milliseconds", 1.into()),
        ("/retention_completed_tick", 600_001.into()),
        ("/estimated_seconds", 601.into()),
        ("/budget_reason", " ".into()),
        ("/arms/1/receipt/path", "../escape.json".into()),
    ] {
        fixture.owner = original.clone();
        *fixture
            .owner
            .pointer_mut(pointer)
            .ok_or("Missing mutation field.")? = replacement;
        assert!(super::super::run(fixture.request()?).is_err(), "{pointer}");
    }
    Ok(())
}

#[test]
fn source_comparison_rejects_duplicate_json_fields_and_changed_proof_bytes() -> Result<()> {
    assert!(json_bytes(br#"{"outer":{"count":1,"count":2}}"#).is_err());
    let mut fixture = fixture()?;
    let request = fixture.request()?;
    std::fs::write(fixture.root.join("arm-0-final.json"), b"{}")
        .map_err(|error| error.to_string())?;
    assert!(super::super::run(request).is_err());
    Ok(())
}
