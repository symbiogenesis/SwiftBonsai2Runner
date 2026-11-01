//! Native CPU admission and launch-disabled server planning without models.
#![forbid(unsafe_code)]
#![cfg(all(windows, target_arch = "aarch64"))]

use serde_json::json;

#[test]
fn planning_never_creates_storage_or_enables_a_model() -> Result<(), String> {
    let data = std::env::temp_dir().join(format!(
        "Bonsai-native-plan-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos()
    ));
    for profile in ["reference-cpu", "swift-pq2-assessment"] {
        let report = bonsai_tools::cpu_check::run(json!({
            "dataDirectory": data, "profile": profile, "checkOnly": true
        }))?;
        assert_eq!(report["Profile"], profile);
        assert_eq!(report["ModelProcessStarted"], false);
        assert!(
            report["AvailablePhysicalBytesAtAdmission"]
                .as_u64()
                .is_some_and(|bytes| bytes > 0)
        );
        assert_eq!(report["EffectiveArguments"][1], report["ModelPath"]);
        assert!(
            report["RecordedAtUtc"]
                .as_str()
                .is_some_and(|value| value.len() == 28 && value.ends_with('Z'))
        );
    }
    let report = bonsai_tools::cpu_check::run(json!({
        "dataDirectory": data, "profile": "swift-pq2-assessment", "checkOnly": true,
        "useAbliterateLora": true, "useRepackedPtq1": true
    }))?;
    assert_eq!(report["WeightRepresentation"], "current-repacked-PTQ1_0");
    assert_eq!(report["EstimatedWeightBytes"], 5_946_648_928_u64);
    assert_eq!(report["AdapterRequested"], true);
    let plan = bonsai_tools::server::plan(json!({"dataDirectory": data, "port": 18080}))?;
    assert_eq!(plan["Origin"], "http://127.0.0.1:18080");
    assert_eq!(plan["LaunchEnabled"], false);
    assert_eq!(plan["ModelProcessStarted"], false);
    assert_eq!(plan["ArtifactVerification"], "not-performed");
    assert_eq!(plan["SecurityQualification"], "not-qualified");
    assert!(!data.exists());
    Ok(())
}

#[test]
fn native_planners_reject_unsafe_paths_and_unreviewed_options() {
    for path in [
        "relative",
        "C:relative",
        "C:\\",
        "\\\\server.invalid\\share",
        "//?/C:/Bonsai",
        "//./C:/Bonsai",
    ] {
        assert!(bonsai_tools::server::plan(json!({"dataDirectory": path})).is_err());
    }
    let data = std::env::temp_dir().join("Bonsai-native-plan-rejection");
    for extra in [
        json!({"port": 0}),
        json!({"verification": "download"}),
        json!({"arguments": ["--tools"]}),
    ] {
        let mut request = extra;
        request["dataDirectory"] = json!(data);
        assert!(bonsai_tools::server::plan(request).is_err());
    }
    assert!(
        bonsai_tools::cpu_check::run(json!({"dataDirectory": data,
        "checkOnly": true, "useAbliterateLora": true}))
        .is_err()
    );
}
