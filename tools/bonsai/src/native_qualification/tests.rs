//! Model-free controls for fixed plans, original deadlines and retained receipts.

use super::{
    Kind, read_json, remaining_seconds, require_runtime_source, run, save_report, source_pins,
};
use serde_json::json;
use std::{fs, path::PathBuf, time::Duration};

fn repository() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap_or_default()
}

#[test]
fn checked_in_ptq1_capture_source_matches_released_runtime() -> crate::Result<()> {
    let root = repository();
    let pins = read_json(&root.join("tools/native-reference/pins.json"), 65_536)?;
    let runtime = read_json(&root.join("config/prism-inference.lock.json"), 65_536)?;
    assert_eq!(runtime["commit"], runtime["Source"]["Commit"]);
    assert_eq!(
        pins["rustSource"],
        json!({"commit":runtime["Source"]["Commit"], "url":runtime["Source"]["Url"],
            "bytes":runtime["Source"]["Bytes"], "sha256":runtime["Source"]["Sha256"]})
    );
    Ok(())
}

#[test]
fn plans_keep_one_selected_ptq1_pair_and_the_fixed_row_pair() -> crate::Result<()> {
    for workers in [1, 4] {
        for backend in ["auto", "scalar", "neon"] {
            let result = run(
                json!({"operation": "plan", "repositoryDirectory": repository(), "rustBackend": backend, "rowWorkers": workers}),
            )?;
            assert_eq!(result["RustBackend"], backend);
            assert_eq!(result["RustRowWorkers"], workers);
            assert_eq!(result["NativeSamples"], 1);
            assert_eq!(result["RustSamples"], 1);
            assert_eq!(result["SuppliedTokens"], json!([248_045]));
            assert_eq!(result["WorkBudgetSeconds"], 240);
            assert_eq!(result["ModelProcessStarted"], false);
            assert_eq!(result["DefaultMayChange"], false);
        }
    }
    let rows =
        run(json!({"operation": "plan", "kind": "rows", "repositoryDirectory": repository()}))?;
    assert_eq!(
        rows["Conditions"],
        json!([{"Name":"serial", "RowWorkers":1}, {"Name":"parallel", "RowWorkers":4}])
    );
    assert_eq!(rows["Representation"], "PQ2_0");
    assert_eq!(rows["AppTpsQualified"], false);
    for (field, value) in [
        ("rowWorkers", json!(0)),
        ("rowWorkers", json!(2)),
        ("rowWorkers", json!(16)),
        ("rustBackend", json!("AUTO")),
        ("rustBackend", json!("unknown")),
    ] {
        let mut request = json!({"operation":"plan", "repositoryDirectory": repository()});
        request[field] = value;
        assert!(run(request).is_err());
    }
    Ok(())
}

#[test]
fn adapter_conditions_select_one_bound_pair_and_cannot_change_the_row_control() -> crate::Result<()>
{
    for condition in ["standard", "loaded-zero", "no-adapter"] {
        let request = json!({"operation":"plan", "repositoryDirectory":repository(), "adapterCondition":condition});
        let report = run(request.clone())?;
        assert_eq!(report["AdapterCondition"], condition);
        assert_eq!(report["AdapterLoaded"], condition != "no-adapter");
        assert_eq!(
            report["AdapterScale"],
            if condition == "standard" { 1.3 } else { 0.0 }
        );
        assert_eq!(report["NativeSamples"], 1);
        assert_eq!(report["RustSamples"], 1);
        assert_eq!(report["WorkBudgetSeconds"], 240);
        assert_eq!(report["DefaultMayChange"], false);
        let decoded: super::Request =
            serde_json::from_value(request.clone()).map_err(|error| error.to_string())?;
        let admission =
            json!({"ModelPath":"C:/models/ptq1.gguf", "AdapterPath":"C:/adapters/approved.gguf"});
        let runtime = json!({"Runtime":{"runtimeDirectory":"C:/runtime"}});
        let lane = std::path::Path::new("C:/evidence");
        let scale = if condition == "standard" { "1.3" } else { "0" };
        let native = super::sample_arguments(&decoded, &admission, Some(&runtime), lane, 0)?;
        assert_eq!(
            native,
            json!([
                "capture",
                "C:/runtime",
                admission["ModelPath"],
                lane,
                if condition == "no-adapter" {
                    json!("none")
                } else {
                    admission["AdapterPath"].clone()
                },
                scale
            ])
        );
        assert!(super::sample_arguments(&decoded, &admission, None, lane, 0).is_err());
        for workers in [1, 4] {
            let rust = super::sample_arguments(&decoded, &admission, None, lane, workers)?;
            let mut expected = vec![
                json!("--use-repacked-ptq1"),
                json!("--row-workers"),
                json!(workers.to_string()),
            ];
            if condition != "no-adapter" {
                expected.extend([
                    json!("--rank-one-adapter"),
                    admission["AdapterPath"].clone(),
                    json!(scale),
                ]);
            }
            expected.extend([
                json!("forward"),
                admission["ModelPath"].clone(),
                json!(lane),
                json!("auto"),
                json!("248045"),
            ]);
            assert_eq!(rust, json!(expected));
        }
        if condition != "standard" {
            let mut rows = request;
            rows["kind"] = json!("rows");
            assert!(run(rows).is_err());
        }
    }
    for invalid in [
        json!("zero"),
        json!("STANDARD"),
        json!(null),
        json!(["standard", "loaded-zero"]),
    ] {
        assert!(run(json!({"operation":"plan", "repositoryDirectory":repository(), "adapterCondition":invalid})).is_err());
    }
    Ok(())
}

#[test]
fn capture_source_cannot_substitute_an_internally_consistent_stale_checkout() -> crate::Result<()> {
    let pins = source_pins(&repository(), Kind::Ptq1)?;
    let compiled = read_json(
        &repository().join("config/prism-inference.lock.json"),
        65_536,
    )?;
    require_runtime_source(&pins["rustSource"], &compiled)?;
    let mut stale_source = pins["rustSource"].clone();
    let mut stale_runtime = compiled.clone();
    let stale_commit = "0".repeat(40);
    stale_source["commit"] = json!(stale_commit);
    stale_runtime["commit"] = json!(stale_commit);
    stale_runtime["Source"]["Commit"] = json!(stale_commit);
    require_runtime_source(&stale_source, &stale_runtime)?;
    assert!(require_runtime_source(&stale_source, &compiled).is_err());
    Ok(())
}

#[test]
fn every_deadline_includes_elapsed_setup_and_cannot_be_renewed() -> crate::Result<()> {
    assert_eq!(
        remaining_seconds(
            Duration::from_mins(4),
            Duration::from_millis(1500),
            15_000,
            10_000,
            1000
        )?,
        5
    );
    assert_eq!(
        remaining_seconds(
            Duration::from_mins(4),
            Duration::from_millis(1500),
            300_000,
            10_000,
            1000
        )?,
        238
    );
    for (deadline, now, frequency) in [
        (1, 2, 1000),
        (2, 2, 1000),
        (2000, 1001, 1000),
        (2000, 1000, 0),
    ] {
        assert!(
            remaining_seconds(
                Duration::from_mins(4),
                Duration::ZERO,
                deadline,
                now,
                frequency
            )
            .is_err()
        );
    }
    assert!(
        remaining_seconds(
            Duration::from_mins(4),
            Duration::from_mins(4),
            300_000,
            0,
            1000
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn current_runtime_source_and_bounded_atomic_receipts_fail_closed() -> crate::Result<()> {
    let fixture = std::env::temp_dir().join(format!(
        "Bonsai-NativeQualificationTests-{}",
        crate::native_startup::nonce()?
    ));
    fs::create_dir_all(fixture.join("tools/native-reference"))
        .map_err(|error| error.to_string())?;
    fs::create_dir_all(fixture.join("config")).map_err(|error| error.to_string())?;
    let original = source_pins(&repository(), Kind::Ptq1)?;
    let pins_path = fixture.join("tools/native-reference/pins.json");
    fs::copy(
        repository().join("config/prism-inference.lock.json"),
        fixture.join("config/prism-inference.lock.json"),
    )
    .map_err(|error| error.to_string())?;
    let result = (|| {
        for field in ["commit", "url", "bytes", "sha256"] {
            let mut pins = original.clone();
            pins["rustSource"][field] = if field == "bytes" {
                json!(1)
            } else {
                json!("stale")
            };
            fs::write(
                &pins_path,
                serde_json::to_vec(&pins).map_err(|error| error.to_string())?,
            )
            .map_err(|error| error.to_string())?;
            assert!(source_pins(&fixture, Kind::Ptq1).is_err());
        }
        fs::write(
            &pins_path,
            serde_json::to_vec(&original).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        assert_eq!(source_pins(&fixture, Kind::Ptq1)?, original);
        let path = fixture.join("numerics.json");
        save_report(
            &path,
            &json!({"Status":"preparing", "Phase":"capture-tool-intake", "ModelTrialsRequested":false}),
        )?;
        let failed =
            json!({"Status":"failed", "Phase":"capture-tool-intake", "ModelTrialsRequested":false});
        save_report(&path, &failed)?;
        assert_eq!(read_json(&path, super::LIMIT)?, failed);
        assert!(save_report(&path, &json!({"Failure":"x".repeat(1_048_577)})).is_err());
        assert_eq!(read_json(&path, super::LIMIT)?, failed);
        assert!(save_report(&fixture.join("config"), &failed).is_err());
        assert!(
            !fs::read_dir(&fixture)
                .map_err(|error| error.to_string())?
                .any(|entry| entry.is_ok_and(|entry| entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "tmp")))
        );
        Ok(())
    })();
    fs::remove_dir_all(&fixture).map_err(|error| error.to_string())?;
    result
}

#[cfg(windows)]
#[test]
fn missing_or_expired_owner_allowances_reject_before_preparation() -> crate::Result<()> {
    for seconds in [0, 241] {
        let failure = run(json!({"operation":"capture", "repositoryDirectory": repository(), "workBudgetSeconds":seconds})).err().ok_or("Invalid allowance accepted.")?;
        assert!(failure.contains("remaining complete owner-job work allowance"));
    }
    let failure = run(json!({"operation":"capture", "repositoryDirectory": repository(), "workBudgetSeconds":240})).err().ok_or("Missing deadline accepted.")?;
    assert!(failure.contains("original monotonic owner-job work deadline"));
    assert!(run(json!({"operation":"capture", "repositoryDirectory": repository(), "workBudgetSeconds":240, "workDeadlineTick":1})).is_err());
    Ok(())
}
