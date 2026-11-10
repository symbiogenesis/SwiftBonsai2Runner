//! Model-free selection, immutable identity and successful-counter controls.

use super::*;
use std::path::Path;

fn request(backend: &str, workers: u8) -> crate::Result<Request> {
    serde_json::from_value(json!({"operation":"plan","captureMode":"request-timing",
        "repositoryDirectory":std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
        "rustBackend":backend,"rowWorkers":workers}))
    .map_err(|error| error.to_string())
}

fn checked() -> Value {
    json!({"Artifact":{"Sha256":"a".repeat(64)},
        "AdapterLock":{"adapter":{"sha256":"b".repeat(64)}},
        "Report":{"ModelPath":"C:/public-model.gguf","AdapterPath":"C:/approved-adapter.gguf"}})
}

fn expected_fixture() -> crate::Result<Value> {
    expected(
        &request("neon", 4)?,
        &json!({"rustSource":{"commit":"c".repeat(40)}}),
        &checked(),
        &json!("d".repeat(64)),
        &json!("e".repeat(64)),
        209_000,
    )
}

fn completed(output: u64) -> crate::Result<Value> {
    let mut receipt = expected_fixture()?;
    // Freeze the producer's Debug policy name independently of expected().
    receipt["backend_requested"] = json!("ExperimentalNeon");
    receipt["elapsed_milliseconds"] = json!(1000);
    receipt["load_milliseconds"] = json!(100);
    receipt["preparation_milliseconds"] = json!(100);
    receipt["session_milliseconds"] = json!(700);
    receipt["runtime_milliseconds"] = json!(600);
    receipt["prompt_tokens"] = json!(3);
    receipt["runtime_committed_generated_tokens"] = json!(output);
    receipt["session_committed_generated_tokens"] = json!(output);
    let phases: Vec<_> = [
        ("sampler-snapshot", 1, 0),
        ("rollback-snapshot", 1, 0),
        ("prefix-preparation", 1, 0),
        ("prefill-ingestion", 1, 2),
        ("prefill-evaluation", 1, 1),
        ("decode-evaluation", output, output),
        ("sampling", 1, 1),
        ("callback", output, output),
        ("completion-snapshot", 0, 0),
        ("rollback-restore", 0, 0),
        ("sampler-restore", 0, 0),
    ]
    .into_iter()
    .map(|(name, calls, tokens)| {
        json!({"phase":name,"milliseconds":if calls == 0 { 0.0 } else { 1.0 },
        "attempted_calls":calls,"completed_calls":calls,"attempted_tokens":tokens,
        "completed_tokens_lower_bound":tokens,"partial_progress_unknown":false})
    })
    .collect();
    receipt["phases"] = json!(phases);
    Ok(receipt)
}

fn completed_terminal(output: u64) -> crate::Result<Value> {
    let mut receipt = completed(output)?;
    // Independent producer fixture: retain the historical eleven positions,
    // omit the one unused decode and append only reset-only terminal ingestion.
    receipt["phases"] = json!(
        [
            ("sampler-snapshot", 1, 0),
            ("rollback-snapshot", 1, 0),
            ("prefix-preparation", 1, 0),
            ("prefill-ingestion", 1, 2),
            ("prefill-evaluation", 1, 1),
            ("decode-evaluation", 0, 0),
            ("sampling", 1, 1),
            ("callback", output, output),
            ("completion-snapshot", 0, 0),
            ("rollback-restore", 0, 0),
            ("sampler-restore", 0, 0),
            ("terminal-ingestion", output, output),
        ]
        .into_iter()
        .map(|(name, calls, tokens)| {
            json!({"phase":name,"milliseconds":if calls == 0 { 0.0 } else { 1.0 },
                "attempted_calls":calls,"completed_calls":calls,"attempted_tokens":tokens,
                "completed_tokens_lower_bound":tokens,"partial_progress_unknown":false})
        })
        .collect::<Vec<_>>()
    );
    Ok(receipt)
}

#[test]
fn timing_mode_is_explicit_one_standard_adapter_rust_request() -> crate::Result<()> {
    for backend in ["auto", "scalar", "neon"] {
        for workers in [1, 4] {
            let mut value = json!({"operation":"plan","captureMode":"request-timing",
                "repositoryDirectory":std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
                "rustBackend":backend,"rowWorkers":workers});
            let plan = super::super::run(value.clone())?;
            assert_eq!(plan["CaptureMode"], "request-timing");
            assert_eq!(plan["NativeSamples"], 0);
            assert_eq!(plan["RustSamples"], 1);
            assert_eq!(plan["RustBackend"], backend);
            assert_eq!(plan["RustRowWorkers"], workers);
            assert_eq!(plan["Messages"], messages());
            assert_eq!(plan["OutputTokens"], 1);
            assert_eq!(plan["CleanupAndVerificationReserveSeconds"], 32);
            assert_eq!(plan["ModelProcessStarted"], false);
            assert_eq!(plan["DefaultMayChange"], false);
            assert!(
                value
                    .as_object_mut()
                    .ok_or("Missing plan object.")?
                    .remove("captureMode")
                    .is_some()
            );
            assert_eq!(
                super::super::run(value.clone())?["CaptureMode"],
                "numerical"
            );
            value["captureMode"] = json!("request-timing");
            for (field, invalid) in [
                ("adapterCondition", json!("loaded-zero")),
                ("adapterCondition", json!("no-adapter")),
                ("kind", json!("rows")),
                ("captureMode", json!("both")),
            ] {
                let mut changed = value.clone();
                changed[field] = invalid;
                assert!(super::super::run(changed).is_err(), "{field}");
            }
        }
    }
    Ok(())
}

#[test]
fn timing_arguments_bind_fixed_public_work_and_original_deadline() -> crate::Result<()> {
    assert_eq!(deadline(240, 1000)?, 209_000);
    assert_eq!(deadline(33, 1000)?, 2000);
    assert!(deadline(32, 1000).is_err());
    assert!(deadline(241, 1000).is_err());
    assert!(deadline(240, u64::MAX).is_err());
    let checked = checked();
    for (backend, observed) in [
        ("auto", "Auto"),
        ("scalar", "Scalar"),
        ("neon", "ExperimentalNeon"),
    ] {
        for workers in [1, 4] {
            let selected = request(backend, workers)?;
            let arguments = arguments(
                &selected,
                &checked,
                Path::new("C:/evidence"),
                Path::new("C:/public-messages.json"),
                209_000,
                &json!({"commit":"c".repeat(40)}),
            );
            let expected = expected(
                &selected,
                &json!({"rustSource":{"commit":"c".repeat(40)}}),
                &checked,
                &json!("d".repeat(64)),
                &json!("e".repeat(64)),
                209_000,
            )?;
            assert_eq!(expected["backend_requested"], observed);
            assert_eq!(
                arguments,
                json!([
                    "--use-repacked-ptq1",
                    "--row-workers",
                    workers.to_string(),
                    "--rank-one-adapter",
                    "C:/approved-adapter.gguf",
                    "1.3",
                    "request-timing",
                    "C:/public-model.gguf",
                    "C:/evidence",
                    backend,
                    "C:/public-messages.json",
                    "1",
                    "209000",
                    "c".repeat(40)
                ])
            );
        }
    }
    Ok(())
}

#[test]
fn timing_receipts_require_every_exact_identity_and_allow_sampled_stop() -> crate::Result<()> {
    let expected = expected_fixture()?;
    for output in [0, 1] {
        for receipt in [completed(output)?, completed_terminal(output)?] {
            validate(&receipt, &expected)?;
            for field in expected
                .as_object()
                .ok_or("Missing fixture object.")?
                .keys()
            {
                for invalid in [Value::Null, json!("changed")] {
                    let mut changed = receipt.clone();
                    changed[field] = invalid;
                    assert!(validate(&changed, &expected).is_err(), "{field}");
                }
            }
        }
    }
    assert!(validate(&Value::Null, &expected).is_err());
    assert!(serde_json::from_slice::<Value>(b"").is_err());
    Ok(())
}

#[test]
fn timing_receipts_reject_mixed_terminal_and_decode_work() -> crate::Result<()> {
    let expected = expected_fixture()?;
    let terminal = completed_terminal(1)?;
    let legacy = completed(1)?;
    let mut double_counted = terminal.clone();
    double_counted["phases"][5] = legacy["phases"][5].clone();
    assert!(validate(&double_counted, &expected).is_err());

    let mut terminal_inventory_with_legacy_work = double_counted.clone();
    terminal_inventory_with_legacy_work["phases"][11] =
        completed_terminal(0)?["phases"][11].clone();
    assert!(validate(&terminal_inventory_with_legacy_work, &expected).is_err());

    let mut missing_terminal_work = terminal.clone();
    missing_terminal_work["phases"][11] = completed_terminal(0)?["phases"][11].clone();
    assert!(validate(&missing_terminal_work, &expected).is_err());

    let mut ingested_stop = completed_terminal(0)?;
    ingested_stop["phases"][11] = terminal["phases"][11].clone();
    assert!(validate(&ingested_stop, &expected).is_err());

    let mut wrong_units = terminal.clone();
    wrong_units["phases"][11]["attempted_tokens"] = json!(2);
    wrong_units["phases"][11]["completed_tokens_lower_bound"] = json!(2);
    assert!(validate(&wrong_units, &expected).is_err());

    for field in [
        "attempted_calls",
        "completed_calls",
        "attempted_tokens",
        "completed_tokens_lower_bound",
    ] {
        for invalid in [Value::Null, json!(0), json!(2)] {
            let mut changed = terminal.clone();
            changed["phases"][11][field] = invalid;
            assert!(validate(&changed, &expected).is_err(), "{field}");
        }
    }
    for (path, invalid) in [
        ("/phases/11/milliseconds", json!(-1)),
        ("/phases/11/milliseconds", json!(600)),
        ("/phases/11/partial_progress_unknown", json!(true)),
    ] {
        let mut changed = terminal.clone();
        *changed
            .pointer_mut(path)
            .ok_or("Missing terminal mutation path.")? = invalid;
        assert!(validate(&changed, &expected).is_err(), "{path}");
    }
    Ok(())
}

#[test]
fn timing_receipts_reject_duplicate_unknown_and_truncated_terminal_inventory() -> crate::Result<()>
{
    let expected = expected_fixture()?;
    let terminal = completed_terminal(1)?;
    for invalid in [json!("decode-evaluation"), json!("unknown"), Value::Null] {
        let mut changed = terminal.clone();
        changed["phases"][11]["phase"] = invalid;
        assert!(validate(&changed, &expected).is_err());
    }
    for mutation in 0..4 {
        let mut changed = terminal.clone();
        let phases = changed["phases"]
            .as_array_mut()
            .ok_or("Missing phase fixture.")?;
        match mutation {
            0 => {
                assert!(phases.pop().is_some());
            }
            1 => {
                assert_eq!(phases.remove(8)["phase"], "completion-snapshot");
            }
            2 => phases.push(phases[11].clone()),
            _ => phases.swap(5, 11),
        }
        assert!(validate(&changed, &expected).is_err());
    }
    Ok(())
}

#[test]
fn timing_receipts_reject_incomplete_timings_and_inconsistent_work() -> crate::Result<()> {
    let expected = expected_fixture()?;
    let original = completed(1)?;
    for field in [
        "elapsed_milliseconds",
        "load_milliseconds",
        "preparation_milliseconds",
        "session_milliseconds",
        "runtime_milliseconds",
    ] {
        for invalid in [Value::Null, json!(0), json!(-1), json!("unknown")] {
            let mut changed = original.clone();
            changed[field] = invalid;
            assert!(validate(&changed, &expected).is_err(), "{field}");
        }
    }
    for (path, invalid) in [
        ("/load_milliseconds", json!(2000)),
        ("/runtime_milliseconds", json!(800)),
        ("/capture_status", json!("in-progress")),
        ("/diagnostic_succeeded", json!(false)),
        ("/prompt_tokens", json!(0)),
        ("/prompt_tokens", json!(129)),
        ("/session_committed_generated_tokens", json!(2)),
        ("/runtime_committed_generated_tokens", Value::Null),
        ("/runtime_committed_generated_tokens", json!(0)),
        ("/phases/3/completed_tokens_lower_bound", json!(1)),
        ("/phases/4/completed_calls", json!(0)),
        ("/phases/4/partial_progress_unknown", json!(true)),
        ("/phases/4/milliseconds", json!(-1)),
        ("/phases/4/milliseconds", json!(600)),
        ("/phases/8/milliseconds", json!(1)),
        ("/phases/5/phase", json!("sampling")),
        ("/phases", json!([])),
    ] {
        let mut changed = original.clone();
        *changed.pointer_mut(path).ok_or("Missing mutation path.")? = invalid;
        assert!(validate(&changed, &expected).is_err(), "{path}");
    }
    Ok(())
}
