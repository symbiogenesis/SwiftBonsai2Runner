//! Independent source, input and result checks for the experimental projection.

use super::{Inputs, Probe, bounded_text, capture, exact_directory, parsed, save};
use crate::{Result, identity, startup_cache};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::fs::File;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    binding_directory: PathBuf,
    activation_file: PathBuf,
    activation_sha256: String,
    output_directory: PathBuf,
    rows: usize,
    tile_rows: usize,
    maximum_seconds: u32,
    execution: String,
    package_directory: Option<PathBuf>,
}

pub(super) fn hold_request(probe: &Probe) -> Result<Option<File>> {
    if probe.kind != "projection" {
        return Ok(None);
    }
    let path = probe
        .projection_request_file
        .as_deref()
        .ok_or("Projection request missing.")?;
    super::super::hold(path, 8192).map(|(file, _)| Some(file))
}

fn request_record(probe: &Probe) -> Result<(Request, Value)> {
    let path = probe
        .projection_request_file
        .as_deref()
        .ok_or("Projection request file is required.")?;
    bonsai_artifacts::validate_external_path(path).map_err(|error| error.to_string())?;
    let value = parsed(&bounded_text(path, 8192)?)?;
    let request: Request =
        serde_json::from_value(value.clone()).map_err(|error| error.to_string())?;
    if request.execution != "device-guarded-q2"
        || request.package_directory.as_ref() != Some(&probe.package_directory)
        || !(4..=120).contains(&request.maximum_seconds)
        || u64::from(request.maximum_seconds) + 75 > u64::from(probe.maximum_seconds)
        || request.rows == 0
        || request.rows > 17_408
        || !request.rows.is_multiple_of(32)
        || request.tile_rows == 0
        || request.tile_rows > request.rows
        || !request.tile_rows.is_multiple_of(32)
        || !identity::is_lower_hex(&request.activation_sha256, 64)
    {
        return Err(
            "Projection request violates the signed package, shape or aggregate deadline contract."
                .into(),
        );
    }
    for path in [
        &request.binding_directory,
        &request.activation_file,
        &request.output_directory,
    ] {
        bonsai_artifacts::validate_external_path(path).map_err(|error| error.to_string())?;
    }
    Ok((request, value))
}

pub(super) fn validate_request(probe: &Probe) -> Result<()> {
    let (request, _) = request_record(probe)?;
    if request
        .output_directory
        .try_exists()
        .map_err(|error| error.to_string())?
        || !request.output_directory.parent().is_some_and(Path::is_dir)
        || !request.binding_directory.is_dir()
        || !request.activation_file.is_file()
    {
        return Err(
            "Projection requires existing external inputs and a new output directory.".into(),
        );
    }
    Ok(())
}

fn sources(inputs: &mut Inputs, entries: &Value, root: &Path, names: &[&str]) -> Result<()> {
    let entries = entries
        .as_array()
        .filter(|entries| entries.len() == names.len())
        .ok_or("Projection build source count differs.")?;
    let mut seen = BTreeSet::new();
    for entry in entries {
        let name = entry["Name"]
            .as_str()
            .ok_or("Projection build source name missing.")?;
        if !names.contains(&name)
            || !seen.insert(name)
            || entry["Sha256"].as_str() != Some(&inputs.digest(&root.join(name), 4 * 1024 * 1024)?)
        {
            return Err("Projection build source identity differs.".into());
        }
    }
    Ok(())
}

pub(super) fn verify_fixture(
    inputs: &mut Inputs,
    probe: &Probe,
    report: &mut Value,
    manifest: &str,
) -> Result<PathBuf> {
    let exe = "hexagon-projection-probe.exe";
    exact_directory(
        &probe.fixture_directory,
        &BTreeSet::from([exe.into(), "projection-build.json".into()]),
    )?;
    let path = probe.fixture_directory.join("projection-build.json");
    report["BuildReceiptSha256"] = json!(inputs.digest(&path, 65_536)?);
    let receipt = inputs.json(&path, 65_536)?;
    if receipt["SchemaVersion"] != 1
        || receipt["Kind"] != "hexagon-original-projection-build"
        || receipt["RunnerRevision"] != probe.expected_runner_revision
        || receipt["RustToolchain"] != super::super::TOOLCHAIN
        || receipt["Target"] != super::super::TARGET
        || receipt["ManifestSha256"] != manifest
        || receipt["HostedBuild"] != true
        || ["DeviceCodeLoaded", "ModelLoaded", "ProfileEligible"]
            .iter()
            .any(|field| receipt[*field] != false)
        || !receipt["CompilerIdentitySha256"]
            .as_str()
            .is_some_and(|value| identity::is_lower_hex(value, 64))
        || ["RunId", "RunAttempt"].iter().any(|field| {
            !receipt[*field]
                .as_str()
                .is_some_and(|value| super::super::digits(value, 32))
        })
        || receipt["Executable"]["Name"] != exe
    {
        return Err("Projection build is not the exact source-bound hosted ARM64 artifact.".into());
    }
    let root = probe.repository_directory.join("tools/hexagon");
    sources(
        inputs,
        &receipt["SourceFiles"],
        &root,
        super::super::projection_build::INPUTS,
    )?;
    sources(
        inputs,
        &receipt["NativeSourceFiles"],
        &root.join("native_rpc"),
        super::super::NATIVE_INPUTS,
    )?;
    let path = probe.fixture_directory.join(exe);
    inputs.pinned(
        &path,
        &receipt["Executable"]["Bytes"],
        &receipt["Executable"]["Sha256"],
    )?;
    startup_cache::assert_arm64(&path)?;
    report["ExecutableSha256"] = receipt["Executable"]["Sha256"].clone();
    Ok(path)
}

fn result(value: &Value, code: u32, probe: &Probe, driver: &Value) -> Result<()> {
    if value["schemaVersion"] != 1
        || value["kind"] != "swift-original-pq2-guarded-q2-projection"
        || value["profileEligible"] != false
        || !value["passed"].is_boolean()
    {
        return Err("Invalid native projection result.".into());
    }
    if value["passed"] != true {
        if value["outcome"] != "pending" || code == 0 {
            return Err("Contradictory pending projection result.".into());
        }
        return Ok(());
    }
    let (request, _) = request_record(probe)?;
    let expected = 40 * request.rows.div_ceil(request.tile_rows);
    if code != 0
        || value["outcome"] != "passed"
        || value["execution"] != "device-guarded-q2"
        || value["driverDll"] != driver["Path"]
        || value["selectedPrefixRows"] != request.rows
        || value["tileRows"] != request.tile_rows
        || value["expectedOperations"] != expected
        || value["exactLimbOperationsChecked"] != expected
        || value["activation"]["bytes"] != 20_480
        || value["activation"]["sha256"] != request.activation_sha256
        || [
            "everyLimbBitwiseParity",
            "completeSelectedProjectionBitwiseParity",
        ]
        .iter()
        .any(|field| value[*field] != true)
        || value["activation"]["sourceModelActivationQualified"] != false
        || value["activation"]["basis"] != "pre-Hadamard-input-last-dimension"
        || value["activation"]["provenance"] != "externally-supplied-unqualified"
        || value["tensor"] != "blk.0.ffn_up.weight"
        || value["columns"] != 5120
        || value["completeTensorRows"] != 17_408
        || value["artifactSha256"]
            != "5912bb739217cf25b4283a098c7b893d9baff36bdf2e72d3dbf4cb999d5633d5"
        || !value["nativeError"].is_null()
        || !value["numericalFailure"].is_null()
        || [
            "modelInferenceExecuted",
            "activationParityQualified",
            "logitStateParityQualified",
            "adapterParityQualified",
            "performanceQualified",
            "trustChanged",
            "boundsAreObservedTransfers",
        ]
        .iter()
        .any(|field| value[*field] != false)
    {
        return Err("Projection output lacks bound input and complete numerical parity.".into());
    }
    let native = &value["native"];
    if native["architecture"] != 81
        || native["effectiveDomain"] != 3
        || native["operationsSubmitted"] != expected
        || native["operationsCompleted"] != expected
        || [
            "processInitializationResult",
            "moduleOpenResult",
            "mappingResult",
            "queueCreateResult",
            "queueExportResult",
            "queueStartResult",
            "lastQueueWriteResult",
            "lastQueueReadResult",
            "queueStopResult",
            "queueCloseResult",
            "unmappingResult",
            "moduleCloseResult",
        ]
        .iter()
        .any(|field| native[*field] != 0)
        || [
            "unsignedProcessRequested",
            "freeCompleted",
            "ownerPinnedUntilExit",
            "workCompleted",
            "completedCycle",
        ]
        .iter()
        .any(|field| native[*field] != true)
        || [
            "poisoned",
            "deadlineExpired",
            "completeVendorTeardownQualified",
            "selectedDspBytesIndependentlyObserved",
        ]
        .iter()
        .any(|field| native[*field] != false)
    {
        return Err(
            "Projection native lifetime or complete operation counts are unverified.".into(),
        );
    }
    Ok(())
}

fn output(inputs: &mut Inputs, probe: &Probe, value: &Value) -> Result<()> {
    if value["passed"] != true {
        return Ok(());
    }
    let (request, _) = request_record(probe)?;
    let source = probe
        .repository_directory
        .join("tools/hexagon/projection_probe.rs");
    if value["probeSourceSha256"] != inputs.digest(&source, 4 * 1024 * 1024)?
        || value["weightSha256"]
            != "a0f21e1d4492de2496309b6383b958cba53c54f05c9de710348facdd31706354"
        || value["signsSha256"]
            != "5ab93190ef5cd4f8322d52c1006783b25a3e118453bda0d0f90b1bf301a67197"
        || value["result"]["file"] != "result.f32le"
        || value["result"]["bytes"] != request.rows * 4
        || value["result"]["encoding"] != "little-endian-f32"
    {
        return Err("Projection result identity differs.".into());
    }
    inputs.pinned(
        &request.output_directory.join("result.f32le"),
        &value["result"]["bytes"],
        &value["result"]["sha256"],
    )?;
    let binding_path = request.binding_directory.join("binding.json");
    if value["bindingManifestSha256"] != inputs.digest(&binding_path, 8192)? {
        return Err("Projection binding manifest identity differs.".into());
    }
    let binding = inputs.json(&binding_path, 8192)?;
    for (field, pointer) in [
        ("weightSha256", "/tensor/sha256"),
        ("signsSha256", "/signs/sha256"),
    ] {
        if value[field] != binding.pointer(pointer).cloned().unwrap_or(Value::Null) {
            return Err("Projection result weight/sign identity differs.".into());
        }
    }
    Ok(())
}

pub(super) fn execute(
    inputs: &mut Inputs,
    probe: &Probe,
    report: &mut Value,
    progress: &Path,
    executable: &Path,
) -> Result<()> {
    let path = probe
        .projection_request_file
        .as_deref()
        .ok_or("Projection request missing.")?;
    let (request, _) = request_record(probe)?;
    report["ProjectionRequestSha256"] = json!(inputs.digest(path, 8192)?);
    inputs.pinned(
        &request.activation_file,
        &json!(20_480),
        &json!(request.activation_sha256),
    )?;
    report["Stage"] = json!("original-projection");
    save(progress, report)?;
    let (code, text) = capture(
        executable,
        vec!["--request".into(), path.as_os_str().to_owned()],
        &probe.package_directory,
        report,
        progress,
        "projection",
        false,
        &mut inputs.holds,
    )?;
    let native = parsed(&text)?;
    result(&native, code, probe, &report["DriverIdentity"])?;
    output(inputs, probe, &native)?;
    report["NativeExitCode"] = json!(code);
    report["ComputeExecuted"] = json!(
        native["native"]["operationsSubmitted"]
            .as_u64()
            .is_some_and(|count| count > 0)
    );
    report["NativeResult"] = native;
    if report["NativeResult"]["passed"] == true {
        report["Outcome"] = json!("passed");
        report["Stage"] = json!("original-projection-complete");
        report["ProjectionBitwiseParityVerified"] = json!(true);
    } else {
        report["Failure"] = json!("native-original-projection-incomplete");
    }
    Ok(())
}

pub(super) fn worker_result(probe: &Probe, text: &str) -> Result<Value> {
    let worker = parsed(text)?;
    if worker["Kind"] != "runner-hexagon-projection-worker"
        || worker["SourceRevision"] != probe.expected_runner_revision
        || ![json!("pending"), json!("passed")].contains(&worker["Outcome"])
        || [
            "ModelLoaded",
            "SwiftQualified",
            "ProfileEligible",
            "TrustChanged",
        ]
        .iter()
        .any(|field| worker[*field] != false)
    {
        return Err("Invalid owned projection worker result.".into());
    }
    if worker["Outcome"] == "passed" {
        if worker["SignedPackageVerified"] != true
            || worker["RuntimeFilesVerified"] != 17
            || worker["ComputeExecuted"] != true
        {
            return Err("Projection package or execution is unverified.".into());
        }
        result(
            &worker["NativeResult"],
            u32::try_from(
                worker["NativeExitCode"]
                    .as_u64()
                    .ok_or("Missing projection exit code.")?,
            )
            .map_err(|error| error.to_string())?,
            probe,
            &worker["DriverIdentity"],
        )?;
        output(
            &mut Inputs { holds: Vec::new() },
            probe,
            &worker["NativeResult"],
        )?;
    }
    Ok(worker)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "The complete successful receipt and adversarial field mutations are reviewed together."
    )]
    fn projection_success_rejects_lost_correlation_cleanup_and_promotion_claims() -> Result<()> {
        let root = std::env::temp_dir().join(format!(
            "bonsai-hexagon-projection-policy-{}",
            crate::startup_process::unique()
        ));
        std::fs::create_dir(&root).map_err(|error| error.to_string())?;
        let path = root.join("request.json");
        let package = root.join("package");
        let activation_digest = "a".repeat(64);
        let request = json!({"bindingDirectory":root.join("binding"),"activationFile":root.join("activation.f32le"),"activationSha256":activation_digest,"outputDirectory":root.join("result"),"rows":17408,"tileRows":17408,"maximumSeconds":120,"execution":"device-guarded-q2","packageDirectory":package});
        std::fs::write(&path, request.to_string()).map_err(|error| error.to_string())?;
        let probe = Probe {
            kind: "projection".into(),
            repository_directory: root.clone(),
            package_directory: package,
            fixture_directory: root.clone(),
            output_directory: root.clone(),
            signtool_path: root.join("signtool.exe"),
            expected_runner_revision: "b".repeat(40),
            maximum_seconds: 240,
            reference_initialization: false,
            case: "F32".into(),
            projection_request_file: Some(path.clone()),
        };
        let driver = json!({"Path":"C:\\Windows\\System32\\DriverStore\\FileRepository\\fixture\\libcdsprpc.dll"});
        let mut value = json!({"schemaVersion":1,"kind":"swift-original-pq2-guarded-q2-projection","outcome":"passed","passed":true,"execution":"device-guarded-q2","profileEligible":false,"driverDll":driver["Path"],"selectedPrefixRows":17408,"tileRows":17408,"expectedOperations":40,"exactLimbOperationsChecked":40,"activation":{"bytes":20480,"sha256":activation_digest,"sourceModelActivationQualified":false,"basis":"pre-Hadamard-input-last-dimension","provenance":"externally-supplied-unqualified"},"everyLimbBitwiseParity":true,"completeSelectedProjectionBitwiseParity":true,"tensor":"blk.0.ffn_up.weight","columns":5120,"completeTensorRows":17408,"artifactSha256":"5912bb739217cf25b4283a098c7b893d9baff36bdf2e72d3dbf4cb999d5633d5","nativeError":null,"numericalFailure":null,"native":{"architecture":81,"effectiveDomain":3,"operationsSubmitted":40,"operationsCompleted":40}});
        for name in [
            "modelInferenceExecuted",
            "activationParityQualified",
            "logitStateParityQualified",
            "adapterParityQualified",
            "performanceQualified",
            "trustChanged",
            "boundsAreObservedTransfers",
        ] {
            value[name] = json!(false);
        }
        for name in [
            "processInitializationResult",
            "moduleOpenResult",
            "mappingResult",
            "queueCreateResult",
            "queueExportResult",
            "queueStartResult",
            "lastQueueWriteResult",
            "lastQueueReadResult",
            "queueStopResult",
            "queueCloseResult",
            "unmappingResult",
            "moduleCloseResult",
        ] {
            value["native"][name] = json!(0);
        }
        for name in [
            "unsignedProcessRequested",
            "freeCompleted",
            "ownerPinnedUntilExit",
            "workCompleted",
            "completedCycle",
        ] {
            value["native"][name] = json!(true);
        }
        for name in [
            "poisoned",
            "deadlineExpired",
            "completeVendorTeardownQualified",
            "selectedDspBytesIndependentlyObserved",
        ] {
            value["native"][name] = json!(false);
        }
        result(&value, 0, &probe, &driver)?;
        for (pointer, replacement) in [
            ("/native/operationsCompleted", json!(39)),
            ("/native/lastQueueWriteResult", json!(14)),
            ("/native/queueStopResult", Value::Null),
            ("/native/freeCompleted", json!(false)),
            ("/native/poisoned", json!(true)),
            ("/native/effectiveDomain", json!(0)),
            ("/performanceQualified", json!(true)),
            ("/modelInferenceExecuted", json!(true)),
            ("/profileEligible", json!(true)),
            ("/driverDll", json!("other")),
            ("/nativeError", json!("refused")),
            ("/activation/sha256", json!("c".repeat(64))),
            ("/columns", json!(256)),
        ] {
            let mut malformed = value.clone();
            *malformed
                .pointer_mut(pointer)
                .ok_or("Test pointer missing.")? = replacement;
            assert!(
                result(&malformed, 0, &probe, &driver).is_err(),
                "accepted {pointer}"
            );
        }
        assert!(result(&value, 101, &probe, &driver).is_err());
        let pending = json!({"schemaVersion":1,"kind":"swift-original-pq2-guarded-q2-projection","passed":false,"outcome":"pending","profileEligible":false});
        result(&pending, 1, &probe, &driver)?;
        assert!(result(&pending, 0, &probe, &driver).is_err());
        std::fs::remove_file(path)
            .and_then(|()| std::fs::remove_dir(root))
            .map_err(|error| error.to_string())
    }
}
