//! Literal geometry answers and malformed-plan admission controls.

use super::*;

fn request(sites: Value) -> Value {
    json!({"schemaVersion":1,"weightRepresentation":"PQ2_0","sites":sites})
}

fn repeated_sites(groups: &[u32], rank: u32) -> Value {
    json!(
        (0..64)
            .map(|layer| json!({"layer":layer,"omittedGroups":groups,"recoveryRank":rank}))
            .collect::<Vec<_>>()
    )
}

#[test]
fn heterogeneous_sites_keep_factor_and_packed_work_separate() -> crate::Result<()> {
    let plan = run(request(json!([
        {"layer":1,"omittedGroups":[0],"recoveryRank":1},
        {"layer":63,"omittedGroups":[2,16],"recoveryRank":16}
    ])))?;
    assert_eq!(plan["selectedSiteCount"], 2);
    assert_eq!(plan["sites"][0]["omittedGroups"], json!([0]));
    assert_eq!(plan["sites"][1]["recoveryRank"], 16);
    assert_eq!(
        plan["sites"][0]["accounting"]["grossOmittedPackedPayloadBytes"],
        4_177_920
    );
    assert_eq!(
        plan["sites"][1]["accounting"]["newFactorPayloadBytes"],
        655_360
    );
    assert_eq!(
        plan["aggregate"],
        json!({"grossOmittedPackedPayloadBytes":12_533_760,
            "newFactorPayloadBytes":696_320,"netPackedMinusFactorPayloadBytes":11_837_440,
            "omittedBaseProjectionMultiplyAccumulatesPerToken":47_185_920,
            "newFactorProjectionMultiplyAccumulatesPerToken":174_080})
    );
    assert_eq!(
        plan["materialTarget"]["payloadEnvelopeReachesTarget"],
        false
    );
    assert_eq!(plan["packedAndF32MacsHaveDifferentCosts"], true);
    for field in [
        "sourceAuthenticated",
        "bottleneckSelected",
        "runtimeImplemented",
        "selectiveLoadingImplemented",
        "performanceQualified",
        "processMemoryQualified",
        "behaviorQualified",
    ] {
        assert_eq!(plan[field], false, "{field}");
    }
    for field in [
        "actualProcessMemoryBenefitBytes",
        "usefulAnswerLatencyBenefit",
        "heldoutQualityBenefit",
    ] {
        assert!(plan[field].is_null(), "{field}");
    }
    Ok(())
}

#[test]
fn fixed_material_envelope_rejects_two_groups_and_accepts_three() -> crate::Result<()> {
    let two = run(request(repeated_sites(&[0, 16], 16)))?;
    assert_eq!(
        two["aggregate"]["grossOmittedPackedPayloadBytes"],
        534_773_760
    );
    assert_eq!(two["aggregate"]["newFactorPayloadBytes"], 41_943_040);
    assert_eq!(
        two["aggregate"]["netPackedMinusFactorPayloadBytes"],
        492_830_720
    );
    assert_eq!(
        two["materialTarget"]["netPackedMinusFactorPayloadBytes"],
        536_870_912
    );
    assert_eq!(two["materialTarget"]["payloadEnvelopeReachesTarget"], false);
    let three = run(request(repeated_sites(&[0, 8, 16], 16)))?;
    assert_eq!(
        three["aggregate"]["netPackedMinusFactorPayloadBytes"],
        760_217_600
    );
    assert_eq!(
        three["materialTarget"]["payloadEnvelopeReachesTarget"],
        true
    );
    let mut ptq1 = request(repeated_sites(&[0, 8, 16], 16));
    ptq1["weightRepresentation"] = json!("current-repacked-PTQ1_0");
    let ptq1 = run(ptq1)?;
    assert_eq!(ptq1["geometry"]["packedBlockBytes"], 28);
    assert_eq!(
        ptq1["aggregate"]["grossOmittedPackedPayloadBytes"],
        660_602_880
    );
    assert_eq!(
        ptq1["aggregate"]["netPackedMinusFactorPayloadBytes"],
        618_659_840
    );
    assert_eq!(ptq1["materialTarget"]["payloadEnvelopeReachesTarget"], true);
    Ok(())
}

#[test]
fn rejects_unsupported_shapes_and_noncanonical_sites() {
    let valid = json!([{"layer":0,"omittedGroups":[0,16],"recoveryRank":1}]);
    for groups in [
        json!([]),
        json!([0, 0]),
        json!([16, 0]),
        json!([17]),
        json!((0..17).collect::<Vec<_>>()),
    ] {
        let mut sites = valid.clone();
        sites[0]["omittedGroups"] = groups;
        assert!(run(request(sites)).is_err());
    }
    for (field, value) in [
        ("layer", json!(64)),
        ("layer", json!(-1)),
        ("recoveryRank", json!(0)),
        ("recoveryRank", json!(17)),
        ("recoveryRank", json!(1.5)),
        ("unknown", json!(true)),
    ] {
        let mut sites = valid.clone();
        sites[0][field] = value;
        assert!(run(request(sites)).is_err(), "{field}");
    }
    assert!(run(request(json!([]))).is_err());
    assert!(run(request(json!([valid[0], valid[0]]))).is_err());
    assert!(
        run(request(json!(
            (0..65)
                .map(|layer| json!({"layer":layer,"omittedGroups":[0],"recoveryRank":1}))
                .collect::<Vec<_>>()
        )))
        .is_err()
    );
    for (field, value) in [
        ("schemaVersion", json!(2)),
        ("weightRepresentation", json!("PTQ1_0")),
        ("weightRepresentation", json!("F16")),
        ("targetBytes", json!(1)),
    ] {
        let mut value_request = request(valid.clone());
        value_request[field] = value;
        assert!(run(value_request).is_err(), "{field}");
    }
}
