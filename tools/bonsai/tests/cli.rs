//! Packaged JSON command discovery and compatibility with legacy scoring limits.
#![forbid(unsafe_code)]

use std::fs;
use std::io::Write as _;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

const HEX: &[u8; 16] = b"0123456789abcdef";

#[test]
fn version_advertises_policy_commands_and_compiled_inference_without_request_files()
-> Result<(), Box<dyn std::error::Error>> {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let scratch = std::env::temp_dir().join(format!(
        "SwiftBonsai-CommandDiscovery-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir(&scratch)?;
    let output = Command::new(env!("CARGO_BIN_EXE_bonsai-tools"))
        .arg("--version")
        .current_dir(&scratch)
        .output()?;
    fs::remove_dir(scratch)?;
    assert!(output.status.success());
    assert_eq!(output.stderr, Vec::<u8>::new());
    let version: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(version["Name"], "bonsai-tools");
    assert_eq!(version["ProtocolVersion"], 5);
    assert_eq!(version["StartupProtocolVersion"], 1);
    assert_eq!(version["ChatLaunchProtocolVersion"], 1);
    assert_eq!(version["BootstrapProtocolVersion"], 1);
    let inference: Value =
        serde_json::from_str(include_str!("../../../config/prism-inference.lock.json"))?;
    assert_eq!(
        version["CompiledInference"],
        json!({
            "Version": inference["version"], "Commit": inference["commit"],
            "Target": inference["target"], "ArchiveSha256": inference["asset"]["sha256"]
        })
    );
    let commands = version["Commands"]
        .as_array()
        .ok_or("Missing command capabilities")?;
    for name in [
        "server-policy",
        "cpu-check-report",
        "cpu-check",
        "server-plan",
        "host-cpu",
        "host-gpu",
        "host-opencl",
        "cpu-evaluation",
        "publisher-startup",
        "runtime-stage",
        "runtime-verify",
        "runtime-promotion",
        "weight-patch",
        "speech-models",
        "inference-lock",
        "inference-capabilities",
        "speech-build",
        "source-build",
        "chat-launch",
        "chat-prepare",
        "chat-package",
        "gguf",
        "ptq1-capture",
        "native-qualification",
        "ptq1-packing",
        "host-capabilities",
        "native-acceptance-budget",
        "startup-diagnostics",
        "hexagon-policy",
    ] {
        assert!(commands.iter().any(|command| command == name));
    }
    for name in ["prism-source", "prism-baseline"] {
        assert!(!commands.iter().any(|command| command == name));
    }
    Ok(())
}

#[test]
fn startup_rejects_invalid_caller_identity_before_build_work()
-> Result<(), Box<dyn std::error::Error>> {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let scratch = std::env::temp_dir().join(format!(
        "SwiftBonsai-StartupIdentity-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir(&scratch)?;
    let request = scratch.join("request.json");
    for identity in [
        json!({"parentProcessId": 1}),
        json!({"parentProcessId": 0, "parentCreationTimeFileTime": 1}),
        json!({"parentProcessId": 1, "parentCreationTimeFileTime": "invalid"}),
    ] {
        for command in ["source-build", "chat-launch", "chat-package"] {
            let mut payload = identity.clone();
            if command == "chat-package" {
                payload["operation"] = "launch".into();
            }
            fs::write(&request, serde_json::to_vec(&payload)?)?;
            let output = Command::new(env!("CARGO_BIN_EXE_bonsai-tools"))
                .args([command, "--request"])
                .arg(&request)
                .current_dir(&scratch)
                .output()?;
            assert!(!output.status.success());
            assert_eq!(output.stdout, Vec::<u8>::new());
            assert!(String::from_utf8_lossy(&output.stderr).contains("startup caller identity"));
        }
    }
    fs::remove_file(request)?;
    fs::remove_dir(scratch)?;
    Ok(())
}

#[test]
fn large_legacy_in_memory_evidence_remains_accepted()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let golden: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/lora-qualification-v1.json"
    ))?;
    let mut evidence = golden["scenarios"][0]["evidence"].clone();
    let mut review = golden["scenarios"][0]["review"].clone();
    let text = "x".repeat(750_000);
    let digest: String = Sha256::digest(text.as_bytes())
        .iter()
        .flat_map(|byte| {
            [
                char::from(HEX[usize::from(byte >> 4)]),
                char::from(HEX[usize::from(byte & 15)]),
            ]
        })
        .collect();
    for sample in evidence["samples"]
        .as_array_mut()
        .ok_or("Missing samples")?
    {
        sample["responseText"] = text.clone().into();
        sample["responseSha256"] = digest.clone().into();
    }
    for record in review["reviews"].as_array_mut().ok_or("Missing reviews")? {
        record["responseSha256"] = digest.clone().into();
    }
    for check in evidence["zeroScaleChecks"]
        .as_array_mut()
        .ok_or("Missing zero-scale checks")?
    {
        check["baselineResponseSha256"] = digest.clone().into();
        check["zeroScaleResponseSha256"] = digest.clone().into();
    }
    let bytes = serde_json::to_vec(&json!({
        "operation": "score", "evidence": evidence, "review": review
    }))?;
    assert!(bytes.len() > 16 * 1024 * 1024);
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let scratch = std::env::temp_dir().join(format!(
        "SwiftBonsai-LargeScore-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir(&scratch)?;
    let request = scratch.join("request.json");
    fs::write(&request, bytes)?;
    let output = Command::new(env!("CARGO_BIN_EXE_bonsai-tools"))
        .args(["qualification", "--request"])
        .arg(&request)
        .current_dir(&scratch)
        .output()?;
    assert!(
        output.status.success(),
        "Packaged score failed: {}. Evidence: {}",
        String::from_utf8_lossy(&output.stderr),
        scratch.display()
    );
    let result: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(result["Status"], "accepted");
    fs::remove_file(request)?;
    fs::remove_dir(scratch)?;
    Ok(())
}

#[test]
fn inline_and_stdin_json_preserve_native_promotion_roundtrip()
-> Result<(), Box<dyn std::error::Error>> {
    let payload = json!({"operation": "plan", "runnerRevision": "a".repeat(40),
        "nativeRowWorkers": 4, "weightRepresentation": "current-repacked-PTQ1_0"});
    let text = serde_json::to_string(&payload)?;
    let output = Command::new(env!("CARGO_BIN_EXE_bonsai-tools"))
        .args(["runtime-promotion", "--json", &text])
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let plan: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(plan["policy"]["identities"]["nativeRowWorkers"], 4);
    assert_eq!(plan["policy"]["identities"]["adapterScale"], 1.3);
    let mut child = Command::new(env!("CARGO_BIN_EXE_bonsai-tools"))
        .args(["runtime-promotion", "--request", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or("Missing command stdin.")?
        .write_all(text.as_bytes())?;
    let streamed = child.wait_with_output()?;
    assert!(streamed.status.success());
    assert_eq!(serde_json::from_slice::<Value>(&streamed.stdout)?, plan);
    let lane = std::env::temp_dir().join(format!(
        "Bonsai-NativePromotion-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    ));
    fs::create_dir(&lane)?;
    let evidence = lane.join("evidence.json");
    let bytes = serde_json::to_vec(&plan["evidenceTemplate"])?;
    fs::write(&evidence, &bytes)?;
    let mut score_request = payload;
    score_request["operation"] = "file-score".into();
    score_request["evidencePath"] = serde_json::to_value(&evidence)?;
    let result = Command::new(env!("CARGO_BIN_EXE_bonsai-tools"))
        .args([
            "runtime-promotion",
            "--json",
            &serde_json::to_string(&score_request)?,
        ])
        .current_dir(&lane)
        .output()?;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let score: Value = serde_json::from_slice(&result.stdout)?;
    assert_eq!(score["promotionReady"], false);
    assert_eq!(score["status"], "pending");
    assert_eq!(score["policySha256"], plan["policySha256"]);
    assert_eq!(score["identities"], plan["policy"]["identities"]);
    assert_eq!(score["evidenceManifest"]["bytes"], bytes.len());
    let gates = score["gates"].as_array().ok_or("Missing gates.")?;
    let requirements = plan["policy"]["requiredGates"]
        .as_array()
        .ok_or("Missing requirements.")?;
    assert_eq!(gates.len(), requirements.len());
    for (gate, requirement) in gates.iter().zip(requirements) {
        assert_eq!(gate["status"], "pending");
        for field in [
            "id",
            "repository",
            "requirement",
            "snapdragonRequired",
            "humanReviewRequired",
        ] {
            assert_eq!(gate[field], requirement[field]);
        }
        assert!(gate["reviewerId"].is_null());
        assert_eq!(gate["humanReviewed"], false);
    }
    fs::remove_file(evidence)?;
    fs::remove_dir(lane)?;
    for invalid in ["[]", "null", "{} trailing"] {
        let result = Command::new(env!("CARGO_BIN_EXE_bonsai-tools"))
            .args(["runtime-promotion", "--json", invalid])
            .output()?;
        assert!(!result.status.success());
        assert_eq!(result.stdout, Vec::<u8>::new());
    }
    Ok(())
}
