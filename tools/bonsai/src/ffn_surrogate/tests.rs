//! Whole-branch fitting, split isolation and bounded failure controls.

use super::*;

fn vector(first: f32, second: f32) -> Vec<f32> {
    let mut values = vec![0.0; WIDTH];
    values[0] = first;
    values[1] = second;
    values
}

fn fixture() -> Value {
    let source = json!({"modelSha256":"a".repeat(64),"runtimeSha256":"b".repeat(64),
        "runtimeSourceCommit":"c".repeat(40),"adapterSha256":null,
        "adapterScaleBits":"00000000","weightRepresentation":"PQ2_0",
        "executionProfile":PROFILE,"graphIdentity":"synthetic-unit-fixture"});
    let site = json!({"layer":15,"inputBasis":"original-ffn-normalized-input",
        "targetBasis":"complete-ffn-branch-delta","inputWidth":WIDTH,"outputWidth":WIDTH});
    let controls = json!({"rank":2,"ridge":0.000_001,"timeBudgetMilliseconds":5000});
    let rows = json!([
        {"sampleId":"fit-a","groupId":"fit-a-group","tokenIndex":0,"split":"fit",
            "input":vector(1.0,0.0),"target":vector(2.0,0.0)},
        {"sampleId":"fit-b","groupId":"fit-b-group","tokenIndex":0,"split":"fit",
            "input":vector(0.0,1.0),"target":vector(0.0,3.0)},
        {"sampleId":"dev","groupId":"dev-group","tokenIndex":0,"split":"development",
            "input":vector(1.0,1.0),"target":vector(2.0,3.0)}]);
    json!({"schemaVersion":1,"source":source,"site":site,"controls":controls,"rows":rows})
}

#[test]
fn recovers_linear_map_from_exported_factors_without_quality_claims() -> crate::Result<()> {
    let result = run(fixture())?;
    assert_eq!(result["status"], "fitted-unqualified");
    assert_eq!(result["effectiveRank"], 2);
    assert_eq!(result["factorPayloadBytes"], 81_920);
    let error = result["diagnostics"]["development"]["relativeSquaredError"]
        .as_f64()
        .ok_or("Missing development error.")?;
    assert!(error < 1e-10);
    for field in [
        "developmentUsedForFit",
        "finalHeldoutUsed",
        "sourceAuthenticated",
        "runtimeImplemented",
        "behaviorQualified",
        "performanceQualified",
        "processMemoryQualified",
        "installed",
    ] {
        assert_eq!(result[field], false, "{field}");
    }
    assert_eq!(
        result["artifact"]["composition"],
        "replace-complete-ffn-branch"
    );
    assert_eq!(result["artifact"]["applySkippedSiteAdapterAgain"], false);
    assert_eq!(result["artifact"]["enabled"], false);
    let a: Vec<Vec<f32>> = serde_json::from_value(result["artifact"]["factors"]["a"].clone())
        .map_err(|error| error.to_string())?;
    assert_eq!(result["artifact"]["factors"]["aSha256"], factor_hash(&a)?);
    Ok(())
}

#[test]
fn development_changes_neither_subspace_nor_regression() -> crate::Result<()> {
    let mut request = fixture();
    let first = run(request.clone())?;
    request["rows"][2]["input"] = json!(vector(-100.0, 100.0));
    request["rows"][2]["target"] = json!(vector(-500.0, -600.0));
    let second = run(request)?;
    assert_eq!(first["artifact"]["factors"], second["artifact"]["factors"]);
    assert_eq!(first["diagnostics"]["fit"], second["diagnostics"]["fit"]);
    assert_ne!(
        first["diagnostics"]["development"],
        second["diagnostics"]["development"]
    );
    Ok(())
}

#[test]
fn rank_deficient_targets_reduce_rank_and_duplicate_inputs_use_ridge() -> crate::Result<()> {
    let mut request = fixture();
    request["rows"][1]["input"] = json!(vector(1.0, 0.0));
    request["rows"][1]["target"] = json!(vector(2.0, 0.0));
    let result = run(request)?;
    assert_eq!(result["effectiveRank"], 1);
    assert_eq!(result["factorPayloadBytes"], 40_960);
    Ok(())
}

#[test]
fn rejects_final_holdout_and_sample_or_group_leakage() {
    for (field, value) in [
        ("split", "holdout"),
        ("sampleId", "fit-a"),
        ("groupId", "fit-a-group"),
    ] {
        let mut request = fixture();
        request["rows"][2][field] = json!(value);
        assert!(run(request).is_err(), "{field}");
    }
}

#[test]
fn input_split_isolation_survives_renamed_samples_and_groups() -> crate::Result<()> {
    let request: Request = serde_json::from_value(fixture()).map_err(|error| error.to_string())?;
    for fit_index in [0, 1] {
        let mut duplicate: Request =
            serde_json::from_value(fixture()).map_err(|error| error.to_string())?;
        duplicate.rows[2].input = duplicate.rows[fit_index].input.clone();
        assert_eq!(
            duplicate.validate().err().as_deref(),
            Some("An identical F32 input crosses fit/development splits.")
        );
    }
    let mut same_split = request;
    same_split.rows[1].input = same_split.rows[0].input.clone();
    same_split.validate()?;
    // Compare exact F32 inputs rather than introducing a numerical tolerance
    // into an admission guard: a one-bit change remains a distinct input.
    same_split.rows[2].input = same_split.rows[0].input.clone();
    same_split.rows[2].input[0] = f32::from_bits(1.0_f32.to_bits() + 1);
    same_split.validate()?;
    Ok(())
}

#[test]
fn rejects_wrong_basis_shapes_controls_and_nonfinite_values() -> crate::Result<()> {
    let mut request = fixture();
    request["site"]["inputBasis"] = json!("original-ffn-hidden");
    assert!(run(request).is_err());
    let mut request = fixture();
    request["rows"][0]["input"] = json!([1.0]);
    assert!(run(request).is_err());
    for (field, value) in [("rank", 17), ("rank", 3), ("timeBudgetMilliseconds", 0)] {
        let mut request = fixture();
        request["controls"][field] = json!(value);
        assert!(run(request).is_err());
    }
    let mut request: Request =
        serde_json::from_value(fixture()).map_err(|error| error.to_string())?;
    request.rows[0].target[0] = f32::NAN;
    assert!(request.validate().is_err());
    Ok(())
}

#[test]
fn rejects_zero_targets_and_expired_time_or_work() -> crate::Result<()> {
    let mut request = fixture();
    request["rows"][0]["target"] = json!(vector(0.0, 0.0));
    request["rows"][1]["target"] = json!(vector(0.0, 0.0));
    assert!(run(request).is_err());
    let request: Request = serde_json::from_value(fixture()).map_err(|error| error.to_string())?;
    let rows: Vec<_> = request
        .rows
        .iter()
        .filter(|row| row.split == Split::Fit)
        .collect();
    assert!(solver::fit(&rows, 2, 0.001, Instant::now()).is_err());
    let mut work = solver::MAX_WORK;
    assert!(solver::charge(&mut work, 1, Instant::now() + Duration::from_secs(1)).is_err());
    Ok(())
}
