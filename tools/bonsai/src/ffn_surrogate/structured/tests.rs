//! Independent paired-subtraction, screening and provenance-boundary controls.

use super::*;

fn vector(first: f32, second: f32) -> Vec<f32> {
    let mut values = vec![0.0; WIDTH];
    values[0] = first;
    values[1] = second;
    values
}

fn row(
    name: &str,
    split: &str,
    input: Vec<f32>,
    teacher: Vec<f32>,
    retained: Vec<f32>,
    original: Vec<f32>,
) -> crate::Result<Value> {
    Ok(
        json!({"sampleId":name,"groupId":format!("group-{name}"),"tokenIndex":0,
        "split":split,"sharedInputSha256":vector_hash(&input)?,"input":input,
        "teacherTarget":teacher,"retainedBranch":retained,"originalPrefixTeacherTarget":original,
        "studentPrefixSha256":hash(format!("student-{name}").as_bytes())?,
        "originalPrefixSha256":hash(format!("original-{name}").as_bytes())?}),
    )
}

fn fixture() -> crate::Result<Value> {
    Ok(json!({"schemaVersion":2,
        "source":{"modelSha256":"a".repeat(64),"runtimeSha256":"b".repeat(64),
            "runtimeSourceCommit":"c".repeat(40),"adapterSha256":null,
            "adapterScaleBits":"00000000","weightRepresentation":"PQ2_0",
            "executionProfile":super::super::PROFILE,"graphIdentity":"full-synthetic"},
        "studentExecution":{"executionProfile":STUDENT_PROFILE,"graphIdentity":"omitted-synthetic",
            "prefixPolicySha256":"d".repeat(64)},"captureSha256":"e".repeat(64),
        "site":{"layer":15,"inputBasis":"original-ffn-normalized-input",
            "targetBasis":"teacher-minus-retained-ffn-branch","inputWidth":WIDTH,"outputWidth":WIDTH},
        "omission":{"groupWidth":1024,"originalHiddenWidth":17408,"omittedGroups":[0,16],
            "transform":TRANSFORM,"retainedAdapterPolicy":ADAPTER_POLICY,
            "signIdentity":{"fullSignsSha256":"f".repeat(64),"downSignOffset":11264,"downSignCount":17408}},
        "controls":{"rank":2,"ridge":0.000_001,"timeBudgetMilliseconds":5000},
        "rows":[
            row("fit-a","fit",vector(1.0,0.0),vector(2.0,0.0),vector(1.0,0.0),vector(2.0,0.0))?,
            row("fit-b","fit",vector(0.0,1.0),vector(0.0,3.0),vector(0.0,2.0),vector(0.0,3.0))?,
            row("dev","development",vector(1.0,1.0),vector(2.0,3.0),vector(1.0,2.0),vector(3.0,4.0))?]}))
}

fn run_fixture(request: Value) -> crate::Result<Value> {
    super::super::run(request)
}

fn number(value: &Value) -> crate::Result<f64> {
    value
        .as_f64()
        .ok_or_else(|| "Expected a finite diagnostic number.".into())
}

#[test]
fn paired_subtraction_recovers_missing_map_and_keeps_prefix_drift_visible() -> crate::Result<()> {
    let result = run_fixture(fixture()?)?;
    assert_eq!(result["status"], "development-screen-passed-unqualified");
    let development = &result["diagnostics"]["development"];
    // The omitted residual is [1,1], while the COMPLETE teacher is [2,3].
    assert!(
        (number(&development["uncompensatedRelativeSquaredError"])? - 2.0 / 13.0).abs() < 1e-14
    );
    assert_eq!(development["teacherSquaredEnergy"], 13.0);
    assert!(number(&development["relativeSquaredError"])? < 1e-10);
    // A successful local recovery still differs from the original-prefix [3,4].
    assert!(
        (number(&development["teacherPrefixDriftRelativeSquaredError"])? - 2.0 / 25.0).abs()
            < 1e-14
    );
    assert!(
        number(&development["correctedBranchVersusOriginalPrefixRelativeSquaredError"])? > 0.079
    );
    assert_eq!(result["artifact"]["applyStandardAdapterAgain"], false);
    assert_eq!(
        result["artifact"]["omission"]["retainedAdapterPolicy"],
        ADAPTER_POLICY
    );
    assert_eq!(result["artifact"]["enabled"], false);
    for field in [
        "sourceAuthenticated",
        "captureAndPrefixDeclarationsAuthenticated",
        "signPayloadAuthenticated",
        "developmentUsedForFit",
        "finalHeldoutUsed",
        "runtimeImplemented",
        "behaviorQualified",
        "performanceQualified",
        "processMemoryQualified",
        "installed",
    ] {
        assert_eq!(result[field], false, "{field}");
    }
    Ok(())
}

#[test]
fn development_never_changes_factors_and_rejected_candidates_are_not_published() -> crate::Result<()>
{
    let request = fixture()?;
    let first = run_fixture(request.clone())?;
    let mut changed = request.clone();
    changed["rows"][2]["teacherTarget"] = json!(vector(2.1, 3.1));
    let second = run_fixture(changed)?;
    assert_eq!(first["artifact"]["factors"], second["artifact"]["factors"]);
    assert_eq!(first["diagnostics"]["fit"], second["diagnostics"]["fit"]);
    let mut rejected = request;
    rejected["rows"][2]["retainedBranch"] = json!(vector(2.0, 3.0));
    let rejected = run_fixture(rejected)?;
    assert_eq!(rejected["status"], "development-rejected");
    assert_eq!(rejected["developmentScreen"]["passed"], false);
    assert!(rejected["artifact"].is_null());
    assert!(rejected["artifactSha256"].is_null());
    assert!(rejected["outputPath"].is_null());
    assert!(
        rejected["developmentScreen"]["rejectionReasons"]
            .as_array()
            .ok_or("Missing rejection reasons.")?
            .iter()
            .any(|reason| reason == "compensation-worsens-uncompensated-development-error")
    );
    Ok(())
}

#[test]
fn rejects_group_sign_adapter_and_paired_input_mismatches() -> crate::Result<()> {
    for groups in [
        json!([]),
        json!([1, 0]),
        json!([0, 0]),
        json!([17]),
        json!((0..17).collect::<Vec<_>>()),
    ] {
        let mut request = fixture()?;
        request["omission"]["omittedGroups"] = groups;
        assert!(run_fixture(request).is_err());
    }
    for (field, value) in [
        ("groupWidth", json!(512)),
        ("originalHiddenWidth", json!(16384)),
        (
            "retainedAdapterPolicy",
            json!("apply-full-adapter-after-compensation"),
        ),
    ] {
        let mut request = fixture()?;
        request["omission"][field] = value;
        assert!(run_fixture(request).is_err(), "{field}");
    }
    let mut request = fixture()?;
    request["omission"]["signIdentity"]["downSignOffset"] = json!(0);
    assert!(run_fixture(request).is_err());
    let mut request = fixture()?;
    request["rows"][0]["input"][0] = json!(2.0);
    assert!(run_fixture(request).is_err());
    let mut request = fixture()?;
    request["studentExecution"]["graphIdentity"] = request["source"]["graphIdentity"].clone();
    assert!(run_fixture(request).is_err());
    let mut request = fixture()?;
    request["rows"][0]
        .as_object_mut()
        .ok_or("Missing row.")?
        .remove("originalPrefixTeacherTarget");
    assert!(run_fixture(request).is_err());
    Ok(())
}

#[test]
fn rejects_sample_group_input_and_prefix_split_leakage() -> crate::Result<()> {
    for field in [
        "sampleId",
        "groupId",
        "studentPrefixSha256",
        "originalPrefixSha256",
    ] {
        let mut request = fixture()?;
        request["rows"][2][field] = request["rows"][0][field].clone();
        assert!(run_fixture(request).is_err(), "{field}");
    }
    let mut request = fixture()?;
    for field in ["input", "sharedInputSha256"] {
        request["rows"][2][field] = request["rows"][0][field].clone();
    }
    assert!(run_fixture(request).is_err());
    let mut request = fixture()?;
    request["rows"][2]["split"] = json!("holdout");
    assert!(run_fixture(request).is_err());
    Ok(())
}

#[test]
fn zero_energy_and_worse_development_are_explicit_no_go_outcomes() -> crate::Result<()> {
    let mut request = fixture()?;
    request["rows"][2]["teacherTarget"] = json!(vector(0.0, 0.0));
    let result = run_fixture(request)?;
    assert_eq!(result["status"], "development-rejected");
    assert!(result["diagnostics"]["development"]["relativeSquaredError"].is_null());
    let mut request = fixture()?;
    request["rows"][0]["retainedBranch"] = request["rows"][0]["teacherTarget"].clone();
    request["rows"][1]["retainedBranch"] = request["rows"][1]["teacherTarget"].clone();
    assert!(run_fixture(request).is_err());
    let totals = [
        Totals {
            teacher_energy: 1.0,
            original_teacher_energy: 1.0,
            ..Totals::default()
        },
        Totals {
            teacher_energy: 1.0,
            original_teacher_energy: 1.0,
            uncompensated_error: 0.2,
            compensated_error: 0.06,
            ..Totals::default()
        },
    ];
    let result = screen(&totals);
    assert_eq!(result["passed"], false);
    assert_eq!(
        result["rejectionReasons"],
        json!(["development-minus-fit-relative-squared-error-exceeds-limit"])
    );
    Ok(())
}

#[test]
fn exact_geometry_separates_factor_payload_from_process_memory() -> crate::Result<()> {
    let mut request: Request =
        serde_json::from_value(fixture()?).map_err(|error| error.to_string())?;
    let two = geometry(&request, 16);
    assert_eq!(two["perSite"]["grossOmittedPackedPayloadBytes"], 8_355_840);
    assert_eq!(two["perSite"]["newFactorPayloadBytes"], 655_360);
    assert_eq!(
        two["perSite"]["newFactorProjectionMultiplyAccumulatesPerToken"],
        163_840
    );
    assert_eq!(
        two["samePlanAtAll64Sites"]["netPackedMinusFactorPayloadBytes"],
        470 * 1024 * 1024
    );
    request.omission.omitted_groups = vec![0, 1, 16];
    assert_eq!(
        geometry(&request, 16)["samePlanAtAll64Sites"]["netPackedMinusFactorPayloadBytes"],
        725 * 1024 * 1024
    );
    request.source.weight_representation = "current-repacked-PTQ1_0".into();
    let three_ptq1 = geometry(&request, 16);
    assert_eq!(
        three_ptq1["perSite"]["grossOmittedPackedPayloadBytes"],
        10_321_920
    );
    assert_eq!(
        three_ptq1["samePlanAtAll64Sites"]["netPackedMinusFactorPayloadBytes"],
        590 * 1024 * 1024
    );
    request.omission.omitted_groups = vec![0, 16];
    assert_eq!(
        geometry(&request, 16)["samePlanAtAll64Sites"]["netPackedMinusFactorPayloadBytes"],
        380 * 1024 * 1024
    );
    assert_eq!(two["processMemoryQualified"], false);
    assert_eq!(two["selectiveLoadingImplemented"], false);
    let original = sign_hash(&request)?;
    request.source.model_sha256 = "b".repeat(64);
    assert_ne!(original, sign_hash(&request)?);
    Ok(())
}

#[test]
fn preprocessing_shares_one_deadline_and_work_counter() -> crate::Result<()> {
    let request: Request = serde_json::from_value(fixture()?).map_err(|error| error.to_string())?;
    let mut work = 0;
    assert!(request.validate(Instant::now(), &mut work).is_err());
    let mut work = solver::MAX_WORK;
    assert!(
        request
            .validate(Instant::now() + Duration::from_secs(1), &mut work)
            .is_err()
    );
    let mut request = fixture()?;
    request["controls"]["rank"] = json!(17);
    assert!(run_fixture(request).is_err());
    Ok(())
}
