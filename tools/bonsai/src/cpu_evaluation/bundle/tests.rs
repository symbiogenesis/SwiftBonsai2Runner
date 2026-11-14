//! Synthetic schema-2 replay and admission mutations; no model or device access.

use super::*;
use std::io::Write as _;

fn save(path: &Path, value: &Value) -> Result<()> {
    fs::create_dir_all(path.parent().ok_or("Missing fixture directory.")?)
        .map_err(|e| e.to_string())?;
    fs::write(path, serde_json::to_vec(value).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}

struct Fixture {
    root: PathBuf,
    packet: Value,
    owner: Value,
    receipts: Vec<Value>,
    supervisors: Vec<Value>,
    starts: Vec<Value>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
impl Fixture {
    fn new() -> Result<Self> {
        let root = std::env::temp_dir().join(format!(
            "bonsai-bundle-{}",
            crate::startup_process::unique()
        ));
        fs::create_dir_all(root.join("packet")).map_err(|e| e.to_string())?;
        save(&root.join("packet/messages.json"), &messages())?;
        fs::write(
            root.join("packet/evaluator.ps1"),
            "# retained schema-2 evaluator identity\n",
        )
        .map_err(|e| e.to_string())?;
        let power = json!({"status":"observed","source":"GetSystemPowerStatus","ac_online":true,"battery_saver":false,"battery_percent":90});
        let host = json!({"status":"observed","source":crate::host_cpu::HOST_IDENTITY_SOURCE,"processor_name":"Snapdragon(R) X2 Elite Extreme - X2E94100 - Qualcomm Oryon(TM) CPU","processor_identifier":"ARMv8 (64-bit)","vendor_identifier":"Qualcomm Technologies Inc","windows_version":[10,0,26300],"processor_features":[19,43,66]});
        let native = json!({"schema_version":1,"kind":"runner-native-cpu-observations","status":"observed","architecture":"aarch64","scope":"current-process","source":"GetSystemCpuSetInformation","host_identity":host,"topology":{"logical_processors":18,"cpu_sets":[{"id":1}]},"power_snapshot":power,"power_policy_snapshot":{"status":"observed","notification_version":2,"source":"PowerGetActiveScheme/PowerRegisterForEffectivePowerModeNotifications","active_scheme":"381b4222-f694-41f0-9685-ff5bb260df2e","effective_mode":"balanced"}});
        let mut packet = json!({"schema_version":2,"kind":"prism-cpu-bundle-preparation","target":"aarch64-pc-windows-msvc","features":["cli","experimental-neon","parallel-rows"],"schedule":SCHEDULE,"profile":profile(),"model_executed":false,"promotion_qualified":false,"retained_files":["preparation.json","messages.json","evaluator.ps1"],"evaluator_language":"PowerShell 7.6","evaluator_sha256":digest(&root.join("packet/evaluator.ps1"))?,"collector_sources":{"examples/cpu_bundle_check.rs":"1".repeat(64),"examples/cpu_bundle_check/tests.rs":"2".repeat(64)},"messages":{"path":"messages.json","sha256":digest(&root.join("packet/messages.json"))?},"model_sha256":MODEL,"adapter_sha256":ADAPTER,"per_arm_timeout_seconds":60,"output_tokens":32,"minimum_relative_request_improvement":0.10,"maximum_owned_arm_regression":0.10,"arms":{},"registry_packages":{}});
        for (name, version, hash) in PACKAGES {
            packet["registry_packages"][name] =
                json!({"name":name,"version":version,"sha256":hash,"revision":REVISION});
        }
        for role in ["baseline", "candidate"] {
            let checks=PHASES.map(|phase| {
                let mut check=json!({"phase":phase,"exit_code":0,"command":["synthetic-fixture"]});
                let names=match phase {"state_fixture"=>Some(vec!["graph_parallel_rows_preserve_logits_checkpoints_and_recovery"]),"collector_controls"=>Some(CONTROLS.to_vec()),_=>None};
                if let Some(names)=names {check["test_summary"]=json!({"passed":names.len(),"failed":0,"ignored":0,"measured":0,"names":names});}
                if phase=="source_analysis" {check["helper_checks"]=json!(["bonsai-neon-dot","bonsai-native-hash"].map(|package|json!({"package":package,"exit_code":0,"command":["synthetic-helper"],"manifest_sha256":"1".repeat(64),"lock_sha256":"2".repeat(64)})));}
                check
            });
            let source = if role == "baseline" {
                json!({"revision":BASELINE,"clean":true})
            } else {
                json!({"revision":REVISION,"clean":true,"kind":"registry","version":"0.1.25","archive_sha256":PACKAGES[0].2})
            };
            packet["arms"][role] = json!({"source":source,"legacy_f16_build":role=="baseline","executable":{"sha256":if role=="baseline" {"b".repeat(64)}else{"c".repeat(64)}},"checks":checks});
        }
        let mut owner = json!({"schema_version":1,"kind":"prism-cpu-bundle-owner","workload_predeclared":true,"completed":true,"budget_seconds":600,"clock_frequency":1000,"started_tick":100_000,"deadline_tick":700_000,"retention_completed_tick":300_000,"original_deadline_unix_milliseconds":2_000_000_000_000u64,"model_sha256":MODEL,"adapter_sha256":ADAPTER,"host_identity":host,"power_before":power,"power_after":power,"arms":[]});
        let mut receipts = Vec::new();
        let mut supervisors = Vec::new();
        let mut starts = Vec::new();
        for (index, role) in SCHEDULE.into_iter().enumerate() {
            let candidate = role == "candidate";
            let begin = 110_000 + u64::try_from(index).map_err(|e| e.to_string())? * 20_000;
            let end = begin + if candidate { 1000 } else { 1500 };
            let execution = begin + 60_000;
            let unix = 2_000_000_000_000u64 - (700_000 - execution);
            let text = "1, 2, 3, 4, 5, 6, 7, 8, 9, 10";
            let mut receipt = profile();
            let extra = json!({"schema_version":1,"kind":"swift-cpu-bundle-arm","source_commit_declaration":packet["arms"][role]["source"]["revision"],"executable_sha256":packet["arms"][role]["executable"]["sha256"],"input_sha256":packet["messages"]["sha256"],"legacy_f16_build":role=="baseline","output_tokens_reserved":32,"original_deadline_unix_milliseconds":unix,"capture_status":"complete","arm_succeeded":true,"session_succeeded":true,"terminal_delivered":true,"stage":"complete","terminal_accounting_matches":true,"reused_tokens":0,"model_sha256":MODEL,"adapter_sha256":ADAPTER,"model_bytes":7_206_168_928u64,"weight_representation":"PQ2_0","effective_row_workers":4,"observed_adapter_sha256":ADAPTER,"model_loaded":true,"output_committed":true,"receipt_reserve_milliseconds":2000,"sampler":"production SwiftSampler and model-declared stops","prompt_tokens":20,"generated_tokens":29,"finish":"stop","output_text":text,"output_bytes":text.len(),"output_sha256":crate::identity::sha256(text.as_bytes()),"load_milliseconds":if candidate{500}else{800},"preparation_milliseconds":10,"request_milliseconds":if candidate{300}else{500},"elapsed_milliseconds":if candidate{850}else{1350}});
            object(&extra)?
                .iter()
                .for_each(|(key, value)| receipt[key] = value.clone());
            starts.push(json!({"source_commit_declaration":receipt["source_commit_declaration"],"original_deadline_unix_milliseconds":unix}));
            receipts.push(receipt);
            supervisors.push(json!({"Status":"completed","ExitCode":0,"ModelProcessStarted":true,"WallMilliseconds":if candidate{900}else{1400},"MinimumAvailablePhysicalBytes":8_589_934_592u64,"PeakJobMemoryBytes":2_147_483_648u64,"CpuConditions":{"Before":native,"After":native}}));
            owner["arms"].as_array_mut().ok_or("Missing arms.")?.push(json!({"id":format!("arm-{}",index+1),"role":role,"started_tick":begin,"cleanup_completed_tick":end,"execution_deadline_tick":execution,"collector_deadline_unix_milliseconds":unix,"host_identity":host,"power_before":power,"power_after":power,"owned_job_empty":true,"cleanup_succeeded":true}));
        }
        Ok(Self {
            root,
            packet,
            owner,
            receipts,
            supervisors,
            starts,
        })
    }
    fn evaluate(&mut self) -> Result<Value> {
        save(&self.root.join("packet/preparation.json"), &self.packet)?;
        self.owner["preparation_sha256"] =
            json!(digest(&self.root.join("packet/preparation.json"))?);
        for index in 0..4 {
            for (name, value) in [
                ("receipt", &self.receipts[index]),
                ("started_receipt", &self.starts[index]),
                ("supervision", &self.supervisors[index]),
            ] {
                let relative = format!("arm-{}/{name}.json", index + 1);
                let path = self.root.join(&relative);
                save(&path, value)?;
                self.owner["arms"][index][name] = json!({"path":relative,"sha256":digest(&path)?});
            }
        }
        save(&self.root.join("owner.json"), &self.owner)?;
        run(
            &json!({"operation":"bundle-evaluate","preparationPath":self.root.join("packet/preparation.json"),"ownerPath":self.root.join("owner.json")}),
        )
    }
}

#[test]
fn summary() -> Result<()> {
    let name = "tests::example";
    let text = format!(
        "test {name} ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 3 filtered out; finished in 0.01s"
    );
    prepare::test_summary(&text, &[name])?;
    for invalid in [
        text.replace("1 passed", "0 passed"),
        text.replace(name, "tests::wrong"),
        format!("{text}\n{text}"),
        text.replace("0.01s", "NaNs"),
        text.replace("0.01s", "1e2s"),
    ] {
        require(
            prepare::test_summary(&invalid, &[name]).is_err(),
            "Invalid libtest inventory admitted.",
        )?;
    }
    Ok(())
}

#[test]
fn native_evaluator_identity_rejects_reinterpretation_missing_bytes_and_unsupported_format()
-> Result<()> {
    let mut fixture = Fixture::new()?;
    let current = std::env::current_exe().map_err(|error| error.to_string())?;
    let retained = fixture.root.join("packet/native-evaluator.exe");
    fs::copy(&current, &retained).map_err(|error| error.to_string())?;
    let revision = "a".repeat(40);
    fixture.packet["orchestrator"] = json!({"revision":revision,"source":{"revision":revision}});
    fixture.packet["native_evaluator"] = json!({"path":"native-evaluator.exe","bytes":fs::metadata(&retained).map_err(|error|error.to_string())?.len(),"sha256":digest(&retained)?,"source_revision":revision,"language":"Rust","format":"bonsai-tools-cpu-bundle-v1"});
    fixture.packet["retained_files"] = json!([
        "preparation.json",
        "messages.json",
        "evaluator.ps1",
        "native-evaluator.exe"
    ]);
    fixture.evaluate()?;
    let expected = fixture.packet["native_evaluator"].clone();
    for (field, value) in [
        ("source_revision", json!("b".repeat(40))),
        ("format", json!("bonsai-tools-cpu-bundle-v2")),
        ("bytes", json!(1)),
        ("sha256", json!("0".repeat(64))),
        ("language", json!("PowerShell 7.6")),
    ] {
        fixture.packet["native_evaluator"][field] = value;
        require(
            fixture.evaluate().is_err(),
            "Changed native evaluator identity admitted.",
        )?;
        fixture.packet["native_evaluator"] = expected.clone();
    }
    fixture
        .packet
        .as_object_mut()
        .ok_or("Missing packet.")?
        .remove("native_evaluator");
    require(
        fixture.evaluate().is_err(),
        "Current packet reinterpreted as historical without native identity.",
    )?;
    fixture.packet["native_evaluator"] = expected;
    fixture.packet["retained_files"] =
        json!(["preparation.json", "messages.json", "evaluator.ps1"]);
    require(
        fixture.evaluate().is_err(),
        "Historical inventory silently assigned new executable.",
    )?;
    fixture.packet["retained_files"] = json!([
        "preparation.json",
        "messages.json",
        "evaluator.ps1",
        "native-evaluator.exe"
    ]);
    fs::OpenOptions::new()
        .append(true)
        .open(&retained)
        .and_then(|mut file| file.write_all(b"changed"))
        .map_err(|error| error.to_string())?;
    require(
        fixture.evaluate().is_err(),
        "Changed native replay bytes admitted.",
    )?;
    fs::remove_file(retained).map_err(|error| error.to_string())?;
    require(
        fixture.evaluate().is_err(),
        "Missing native replay bytes admitted.",
    )
}

#[test]
fn lock() -> Result<()> {
    let fixture = Fixture::new()?;
    let source = fixture.root.join("source.lock");
    let driver = fixture.root.join("driver.lock");
    fs::write(
        &source,
        "[[package]]\nname = \"example\"\nversion = \"1.0.0\"\n",
    )
    .map_err(|e| e.to_string())?;
    fs::write(
        &driver,
        "[[package]]\nname = \"example\"\nversion = \"1.0.1\"\n",
    )
    .map_err(|e| e.to_string())?;
    require(
        prepare::lock_subset(&driver, &source, false).is_err(),
        "Unlocked dependency admitted.",
    )?;
    fs::write(
        &driver,
        "[[package]]\nname = \"example\"\nversion = \"1.0.0\"\n",
    )
    .map_err(|e| e.to_string())?;
    prepare::lock_subset(&driver, &source, false)
}

fn registry_fixture(fixture: &Fixture, member: &str) -> Result<(PathBuf, PathBuf)> {
    let cache = fixture.root.join("registry/cache/index");
    let source = fixture.root.join("registry/src/index/example-1.0.0");
    fs::create_dir_all(&cache)
        .and_then(|()| fs::create_dir_all(&source))
        .map_err(|e| e.to_string())?;
    fs::write(source.join("lib.rs"), b"reviewed source").map_err(|e| e.to_string())?;
    let archive = cache.join("example-1.0.0.crate");
    let file = fs::File::create(&archive).map_err(|e| e.to_string())?;
    let gzip = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    let mut writer = tar::Builder::new(gzip);
    let mut header = tar::Header::new_ustar();
    header.set_entry_type(tar::EntryType::Regular);
    header.set_size(15);
    header.set_mode(0o644);
    // Write the raw header name to admit a deliberate traversal fixture without
    // asking the archive writer to normalize or reject the adversarial path.
    let raw = header.as_mut_bytes();
    require(member.len() < 100, "Fixture member too long.")?;
    raw[..member.len()].copy_from_slice(member.as_bytes());
    header.set_cksum();
    writer
        .append(&header, b"reviewed source".as_slice())
        .map_err(|e| e.to_string())?;
    writer
        .into_inner()
        .map_err(|e| e.to_string())?
        .finish()
        .map_err(|e| e.to_string())?;
    Ok((source, archive))
}

#[test]
fn archive() -> Result<()> {
    let fixture = Fixture::new()?;
    let (source, archive_path) = registry_fixture(&fixture, "example-1.0.0/lib.rs")?;
    let sha = digest(&archive_path)?;
    archive::verify(&source, &sha)?;
    fs::write(source.join("extra.rs"), b"unreviewed source").map_err(|e| e.to_string())?;
    require(
        archive::verify(&source, &sha).is_err(),
        "Unexpected registry source admitted.",
    )?;
    fs::remove_file(source.join("extra.rs")).map_err(|e| e.to_string())?;
    fs::write(source.join("lib.rs"), b"changed source").map_err(|e| e.to_string())?;
    require(
        archive::verify(&source, &sha).is_err(),
        "Changed registry source admitted.",
    )
}

#[test]
fn archive_traversal() -> Result<()> {
    let fixture = Fixture::new()?;
    let (source, archive_path) = registry_fixture(&fixture, "example-1.0.0/../escape")?;
    require(
        archive::verify(&source, &digest(&archive_path)?).is_err(),
        "Escaping archive member admitted.",
    )?;
    require(
        !fixture.root.join("registry/src/index/escape").exists(),
        "Archive extraction occurred.",
    )
}

#[test]
fn telemetry_host_registry_and_preparation_controls() -> Result<()> {
    let mut fixture = Fixture::new()?;
    for receipt in &mut fixture.receipts {
        receipt["first_text_delta_milliseconds"] = 100.into();
        receipt["generation_phases"] = json!([{"phase":"decode","milliseconds":100,"attempted_calls":1,"completed_calls":1,"attempted_tokens":1,"completed_tokens_lower_bound":1,"partial_progress_unknown":false}]);
    }
    let report = fixture.evaluate()?;
    require(
        report["observations"][1]["first_text_delta_milliseconds"] == 100,
        "Callback telemetry lost.",
    )?;
    fixture.receipts[1]["first_text_delta_milliseconds"] = 301.into();
    require(
        fixture.evaluate().is_err(),
        "Callback outside request admitted.",
    )?;
    fixture.receipts[1]["first_text_delta_milliseconds"] = 100.into();
    for (field, value) in [
        ("schema_version", json!("2")),
        ("per_arm_timeout_seconds", json!("60")),
        ("minimum_relative_request_improvement", json!("0.10")),
    ] {
        let old = fixture.packet[field].clone();
        fixture.packet[field] = value;
        require(fixture.evaluate().is_err(), "Wrong identity type admitted.")?;
        fixture.packet[field] = old;
    }
    fixture.supervisors[1]["CpuConditions"]["After"]["host_identity"]["windows_version"][2] =
        26301.into();
    require(fixture.evaluate().is_err(), "Changed native host admitted.")?;
    fixture.supervisors[1]["CpuConditions"]["After"]["host_identity"]["windows_version"][2] =
        26300.into();
    fixture.supervisors[1]["CpuConditions"]["After"]["power_policy_snapshot"]["effective_mode"] =
        "high-performance".into();
    require(
        fixture.evaluate().is_err(),
        "Changed effective power mode admitted.",
    )?;
    fixture.supervisors[1]["CpuConditions"]["After"]["power_policy_snapshot"]["effective_mode"] =
        "balanced".into();
    fixture.packet["registry_packages"]["bonsai-neon-dot"]["sha256"] = "0".repeat(64).into();
    require(
        fixture.evaluate().is_err(),
        "Changed registry dependency admitted.",
    )
}

#[test]
fn schema2_replay_preserves_admitted_losing_and_length_scopes() -> Result<()> {
    let mut fixture = Fixture::new()?;
    let report = fixture.evaluate()?;
    require(
        report["status"] == "admitted"
            && report["short_request_benefit_observed"] == true
            && report["promotion_qualified"] == false
            && report["DefaultMayChange"] == false,
        "Admitted scope changed.",
    )?;
    for index in [1, 2] {
        fixture.supervisors[index]["WallMilliseconds"] = 1500.into();
        let started = integer(&fixture.owner["arms"][index]["started_tick"], 0, u64::MAX)?;
        fixture.owner["arms"][index]["cleanup_completed_tick"] = (started + 1600).into();
    }
    require(
        fixture.evaluate()?["short_request_benefit_observed"] == false,
        "Losing outcome promoted.",
    )?;
    for receipt in &mut fixture.receipts {
        receipt["finish"] = "length".into();
    }
    require(
        fixture.evaluate()?["complete_answer_observed"] == false,
        "Length claimed complete.",
    )
}

#[test]
fn schema2_replay_rejects_identity_types_terminal_power_budget_and_resource_mutations() -> Result<()>
{
    let mut fixture = Fixture::new()?;
    for (field, bad) in [
        ("row_workers", json!(4.0)),
        ("model_sha256", json!("0".repeat(64))),
        ("terminal_delivered", json!(false)),
        ("generated_tokens", json!(0)),
        ("output_text", json!("")),
        ("backend_requested", json!("Auto")),
    ] {
        let old = fixture.receipts[1][field].clone();
        fixture.receipts[1][field] = bad;
        require(fixture.evaluate().is_err(), "Invalid arm admitted.")?;
        fixture.receipts[1][field] = old;
    }
    for (field, bad) in [
        ("MinimumAvailablePhysicalBytes", Value::Null),
        ("PeakJobMemoryBytes", json!(12_348_030_977u64)),
        ("ExitCode", json!(false)),
        ("WallMilliseconds", json!(60002)),
    ] {
        let old = fixture.supervisors[1][field].clone();
        fixture.supervisors[1][field] = bad;
        require(fixture.evaluate().is_err(), "Invalid supervision admitted.")?;
        fixture.supervisors[1][field] = old;
    }
    fixture.owner["arms"][1]["power_after"]["ac_online"] = Value::Null;
    require(fixture.evaluate().is_err(), "Unknown arm power admitted.")?;
    fixture.owner["arms"][1]["power_after"]["ac_online"] = true.into();
    fixture.owner["retention_completed_tick"] = 700_001.into();
    require(
        fixture.evaluate().is_err(),
        "Expired original allowance admitted.",
    )?;
    fixture.owner["retention_completed_tick"] = 300_000.into();
    fixture.packet["arms"]["candidate"]["checks"][2]["test_summary"]["passed"] = 0.into();
    require(
        fixture.evaluate().is_err(),
        "Zero executed controls admitted.",
    )
}

#[test]
fn schema2_json_and_confined_members_reject_ambiguous_or_redirected_inputs() -> Result<()> {
    for raw in [
        br#"{"a":1,"a":2}"#.as_slice(),
        br#"{"a":{"x":1,"x":2}}"#.as_slice(),
        br#"{"a":NaN}"#.as_slice(),
    ] {
        require(json_bytes(raw).is_err(), "Ambiguous JSON admitted.")?;
    }
    let mut fixture = Fixture::new()?;
    fixture.evaluate()?;
    require(
        member(&fixture.root, &json!("../escape")).is_err()
            && member(&fixture.root, &json!("arm\\receipt.json")).is_err(),
        "Escaping member admitted.",
    )?;
    let receipt = fixture.owner["arms"][1]["receipt"].clone();
    let path = member(&fixture.root, &receipt["path"])?;
    fs::OpenOptions::new()
        .append(true)
        .open(path)
        .and_then(|mut file| file.write_all(b" "))
        .map_err(|e| e.to_string())?;
    require(
        checked(&fixture.root, &receipt, &mut BTreeSet::new()).is_err(),
        "Changed replay input admitted.",
    )
}
