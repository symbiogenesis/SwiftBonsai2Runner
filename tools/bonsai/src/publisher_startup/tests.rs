use super::*;

#[test]
fn fixed_arguments_match_independent_archived_probes() -> crate::Result<()> {
    let evidence = pinned()?;
    let probes = cases();
    assert_eq!(probes.len(), 10);
    for (index, probe) in probes.iter().enumerate() {
        assert_eq!(probe["Name"], evidence["results"]["probes"][index]["name"]);
        assert_eq!(
            probe["Arguments"],
            evidence["results"]["probes"][index]["arguments"]
        );
    }
    assert_eq!(allowance(60, Duration::ZERO)?, 50);
    assert!(allowance(30, Duration::ZERO).is_err());
    assert!(allowance(90, Duration::from_secs(90)).is_err());
    assert_eq!(smoke_deadline(60)?, 24);
    assert_eq!(smoke_deadline(80)?, 30);
    assert!(smoke_deadline(59).is_err());
    Ok(())
}

#[test]
fn original_model_controls_cannot_gain_adapters_or_weaken_memory() -> crate::Result<()> {
    let model: Value = serde_json::from_str(MODEL).map_err(|error| error.to_string())?;
    let fixture: Value =
        serde_json::from_str(include_str!("../../tests/fixtures/cpu-reports-v1.json"))
            .map_err(|error| error.to_string())?;
    let control = fixture["cases"]
        .as_array()
        .ok_or("Missing fixtures.")?
        .iter()
        .map(|entry| &entry["expected"])
        .find(|entry| {
            entry["Profile"] == "swift-pq2-assessment"
                && entry["AdapterRequested"].as_bool() == Some(false)
                && entry["WeightRepresentation"] == "PQ2_0"
        })
        .ok_or("Missing frozen PQ2 control.")?;
    let path = std::env::temp_dir().join("Bonsai-Publisher-Model.gguf");
    let mut arguments: Vec<String> = serde_json::from_value(control["EffectiveArguments"].clone())
        .map_err(|error| error.to_string())?;
    let index = unique_argument(&arguments, "--model")?;
    arguments[index] = path.to_string_lossy().into_owned();
    let cpu = json!({"Policy":{"Name":"swift-pq2-assessment", "WeightRepresentation":"PQ2_0", "ServiceCeilingBytes":12_348_030_976_u64,
        "RequiredAvailableBytes":16_642_998_272_u64, "AvailableMemoryFloorBytes":4_294_967_296_u64}, "AdapterLock":null,
        "Lock":model, "ModelPath":path, "Artifact":{"Bytes":model["model"]["bytes"], "Sha256":model["model"]["sha256"]}, "Arguments":arguments});
    let smoke = smoke_plan(&cpu, &model)?;
    assert_eq!(smoke["Artifact"]["sha256"], model["model"]["sha256"]);
    let mut expected = arguments.clone();
    let index = unique_argument(&expected, "--n-predict")?;
    expected[index] = "4".into();
    let index = unique_argument(&expected, "--reasoning")?;
    expected[index] = "off".into();
    expected.retain(|argument| argument != "--no-perf");
    expected.push("--verbose-prompt".into());
    assert_eq!(smoke["Arguments"], json!(expected));
    assert_eq!(cpu["Arguments"], json!(arguments));
    for (pointer, invalid) in [
        ("/Policy/ServiceCeilingBytes", json!(12_348_030_975_u64)),
        ("/Artifact/Sha256", json!("0".repeat(64))),
        ("/AdapterLock", json!({"adapter":{}})),
        ("/Lock/model/revision", json!("stale")),
    ] {
        let mut changed = cpu.clone();
        *changed.pointer_mut(pointer).ok_or("Missing test field.")? = invalid;
        assert!(smoke_plan(&changed, &model).is_err());
    }
    let mut duplicated = cpu;
    duplicated["Arguments"]
        .as_array_mut()
        .ok_or("Missing arguments.")?
        .extend([json!("--n-predict"), json!("4096")]);
    assert!(smoke_plan(&duplicated, &model).is_err());
    Ok(())
}

#[test]
fn inference_requires_unique_consistent_counters_and_visible_output() -> crate::Result<()> {
    let diagnostic = "main: number of tokens in prompt = 12\ncommon_perf_print:    samplers time = 0.42 ms / 16 tokens\ncommon_perf_print:        eval time = 2.00 ms / 3 runs   (0.67 ms per token, 1500.00 tokens per second)\n";
    let result = observation("ready", diagnostic)?;
    assert_eq!(result["SampledOutputTokens"], 4);
    assert_eq!(result["DecodeEvaluationRuns"], 3);
    assert_eq!(result["PerformanceVerdict"], "not-evaluated");
    let mut prefixed = String::new();
    for line in diagnostic.replace("main:", "llama_completion:").lines() {
        prefixed.push_str("0.00.585.800 I ");
        prefixed.push_str(line);
        prefixed.push_str("\r\n");
    }
    assert_eq!(observation("ready", &prefixed)?, result);
    for invalid in [
        diagnostic.replace("/ 3 runs", "/ 2 runs"),
        "Version alone is model-free.".into(),
        diagnostic.repeat(2),
        diagnostic.replace("16 tokens", "999999999999999999999999 tokens"),
    ] {
        assert!(observation("ready", &invalid).is_err());
    }
    assert!(observation(" ", diagnostic).is_err());
    assert!(observation(&"x".repeat(65_537), diagnostic).is_err());
    Ok(())
}

#[test]
fn candidate_inventory_rejects_duplicates_unpinned_entries_and_changed_lengths() -> crate::Result<()>
{
    let root = std::env::temp_dir().join(format!(
        "Bonsai-Publisher-Inventory-{}",
        crate::native_startup::nonce()?
    ));
    fs::create_dir(&root).map_err(|error| error.to_string())?;
    let result = (|| {
        let mut files = Vec::new();
        for name in [
            "llama-completion.exe",
            "llama-server.exe",
            "llama-bench.exe",
            "test-headless-argv.exe",
            "ggml.dll",
        ] {
            fs::write(root.join(name), [1, 2, 3]).map_err(|error| error.to_string())?;
            files.push(json!({"Name":name, "Bytes":3, "Sha256":"a".repeat(64)}));
        }
        assert_eq!(
            inventory(&root, &json!(files))?.as_array().map(Vec::len),
            Some(5)
        );
        let mut duplicate = files.clone();
        duplicate.push(files[0].clone());
        assert!(inventory(&root, &json!(duplicate)).is_err());
        duplicate[5]["Name"] = json!("LLAMA-COMPLETION.EXE");
        assert!(inventory(&root, &json!(duplicate)).is_err());
        fs::write(root.join("extra.dll"), [1]).map_err(|error| error.to_string())?;
        assert!(inventory(&root, &json!(files)).is_err());
        fs::remove_file(root.join("extra.dll")).map_err(|error| error.to_string())?;
        fs::write(root.join("ggml.dll"), [1]).map_err(|error| error.to_string())?;
        assert!(inventory(&root, &json!(files)).is_err());
        files[0]["Name"] = json!("../llama-completion.exe");
        assert!(inventory(&root, &json!(files)).is_err());
        Ok(())
    })();
    fs::remove_dir_all(root).map_err(|error| error.to_string())?;
    result
}
