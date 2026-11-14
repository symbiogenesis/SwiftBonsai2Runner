//! Model-free packaging of an already built, exact-source owner probe.
use super::{plain, write_json};
use crate::{Result, bounded_input, hexagon_policy, identity, startup_process};
use bonsai_artifacts::{hash_reader, is_redirect, validate_external_path};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    fs::{self, File},
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Request {
    runner_repository: PathBuf,
    prism_repository: PathBuf,
    probe: PathBuf,
    supervisor: PathBuf,
    compiler: PathBuf,
    successful_build_receipt: PathBuf,
    output_directory: PathBuf,
    git: PathBuf,
    #[serde(default = "adapter")]
    adapter_case: String,
    #[serde(default = "tokens")]
    token_ids: Vec<u64>,
    #[serde(default = "projections")]
    projections: Vec<String>,
    #[serde(default)]
    resident_host_weights: bool,
    #[serde(default)]
    batch_four_operations: bool,
    #[serde(default)]
    batch_ten_operations: bool,
}
fn adapter() -> String {
    "standard-scale-1.3".to_owned()
}
fn tokens() -> Vec<u64> {
    vec![42, 43, 128]
}
fn projections() -> Vec<String> {
    vec!["blk.0.ffn_up.weight".to_owned()]
}
fn require(valid: bool, message: &str) -> Result<()> {
    if valid {
        Ok(())
    } else {
        Err(message.to_owned())
    }
}

fn pin(path: &Path) -> Result<Value> {
    plain(path)?;
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    require(
        metadata.is_file() && !is_redirect(&metadata),
        "Source identity requires an ordinary file.",
    )?;
    let digest = hash_reader(&mut file, metadata.len(), &mut vec![0; 65536], &mut || {
        Ok(())
    })
    .map_err(|error| error.to_string())?;
    Ok(json!({"bytes":metadata.len(),"sha256":digest}))
}
fn read(path: &Path) -> Result<Value> {
    let raw = bounded_input::read(
        File::open(path).map_err(|error| error.to_string())?,
        1_048_576,
    )
    .map_err(|error| error.into_message("Control JSON exceeds its byte bound."))?;
    hexagon_policy::data_json(std::str::from_utf8(&raw).map_err(|error| error.to_string())?)
}
fn revision(git: &Path, source: &Path, scratch: &Path) -> Result<String> {
    let environment = BTreeMap::new();
    let execute = |suffix: &[&str]| -> Result<String> {
        let log = scratch.join(format!("git-{}.log", startup_process::unique()));
        let args = [
            OsString::from("-c"),
            format!("safe.directory={}", source.display()).into(),
            "-C".into(),
            source.as_os_str().to_owned(),
        ]
        .into_iter()
        .chain(suffix.iter().map(OsString::from))
        .collect::<Vec<_>>();
        let result = startup_process::run_strict(
            git.as_os_str(),
            &args,
            scratch,
            &environment,
            Duration::from_secs(20),
            &log,
        )?;
        require(result.code == 0, "Source identity command failed.")?;
        Ok(result.stdout.trim().to_owned())
    };
    require(
        execute(&["status", "--porcelain"])?.is_empty(),
        "Built source must be clean.",
    )?;
    let revision = execute(&["rev-parse", "HEAD"])?;
    require(
        identity::is_lower_hex(&revision, 40),
        "Source revision identity unavailable.",
    )?;
    Ok(revision)
}
fn inventory(root: &Path) -> Result<Vec<Value>> {
    let mut names = BTreeSet::from(["Cargo.toml".to_owned(), "Cargo.lock".to_owned()]);
    for name in ["build.rs", "rust-toolchain.toml", ".cargo/config.toml"] {
        if root.join(name).is_file() {
            names.insert(name.to_owned());
        }
    }
    let mut pending = vec![root.join("src"), root.join("tools")];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let metadata = fs::symlink_metadata(entry.path()).map_err(|error| error.to_string())?;
            require(
                !is_redirect(&metadata),
                "Source identity cannot traverse redirects.",
            )?;
            let path = entry.path();
            if metadata.is_dir() {
                if !["target", ".git", ".worktrees"]
                    .iter()
                    .any(|name| entry.file_name() == *name)
                {
                    pending.push(path);
                }
            } else {
                let name = path
                    .strip_prefix(root)
                    .map_err(|error| error.to_string())?
                    .to_str()
                    .ok_or("Source path is not Unicode.")?
                    .replace('\\', "/");
                names.insert(name);
            }
        }
    }
    names
        .into_iter()
        .map(|name| {
            let mut value = pin(&root.join(&name))?;
            value["path"] = json!(name);
            Ok(value)
        })
        .collect()
}
fn intake(request: &Request, prism_revision: &str, runner_revision: &str) -> Result<Value> {
    let output = &request.output_directory;
    let bundle = output.join("model-bundle");
    fs::create_dir(&bundle).map_err(|error| error.to_string())?;
    let probe = bundle.join("prism-hexagon-model-probe.exe");
    let supervisor = bundle.join("bonsai-tools.exe");
    for (original, target) in [
        (&request.probe, &probe),
        (&request.supervisor, &supervisor),
        (
            &request.successful_build_receipt,
            &output.join("observed-probe-build.json"),
        ),
    ] {
        pin(original)?;
        fs::copy(original, target).map_err(|error| error.to_string())?;
    }
    let sources = inventory(&request.prism_repository)?;
    let compiler_pin = pin(&request.compiler)?;
    let receipt = json!({"schemaVersion":1,"kind":"local-prism-hexagon-model-probe-build","hostedBuild":false,"buildSucceeded":true,"cargoLocked":true,"target":"aarch64-pc-windows-msvc","prismRevision":prism_revision,"compiler":{"path":request.compiler,"bytes":compiler_pin["bytes"],"sha256":compiler_pin["sha256"]},"executable":pin(&probe)?,"sources":sources});
    let receipt_path = output.join("local-model-build.json");
    write_json(&receipt_path, &receipt)?;
    let mut envelope = read(
        &request
            .runner_repository
            .join("docs/evidence/hexagon-adapter-session-20261103/model-device-request.json"),
    )?;
    let command = envelope["request"]
        .as_object_mut()
        .ok_or("Retained request envelope missing.")?;
    let updates = json!({"repositoryDirectory":request.runner_repository,"prismRepositoryDirectory":request.prism_repository,"expectedPrismRevision":prism_revision,"executablePath":probe,"executableSha256":receipt["executable"]["sha256"],"buildReceiptPath":receipt_path,"outputDirectory":output.join("device"),"tokenId":request.token_ids[0],"tokenIds":request.token_ids,"projections":request.projections,"executionBackend":"hmx-bitplane","residentHostWeights":request.resident_host_weights,"batchFourOperations":request.batch_four_operations,"batchTenOperations":request.batch_ten_operations,"adapterCase":request.adapter_case,"cpuBackend":"neon","rowWorkers":4,"maximumSeconds":300});
    for (key, value) in updates
        .as_object()
        .ok_or("Control request update missing.")?
    {
        command.insert(key.clone(), value.clone());
    }
    if request.adapter_case == "none" {
        command.remove("adapterPath");
    }
    for name in ["modelPath", "packageDirectory", "signtoolPath"] {
        require(
            command[name]
                .as_str()
                .is_some_and(|value| Path::new(value).exists()),
            "Retained control input is absent.",
        )?;
    }
    if request.adapter_case != "none" {
        let adapter = command["adapterPath"]
            .as_str()
            .ok_or("Retained adapter path absent.")?;
        require(
            pin(Path::new(adapter))?
                == json!({"bytes":9_682_464,"sha256":"f1669534803d340a496015f5c45125f3437b4d13ec764f40e34488ce83967f42"}),
            "Approved standard adapter identity differs.",
        )?;
    }
    let request_path = output.join("model-device-request.json");
    write_json(&request_path, &envelope)?;
    let intake = json!({"runnerRevision":runner_revision,"prismRevision":prism_revision,"supervisor":pin(&supervisor)?,"probe":pin(&probe)?,"sourceFiles":sources.len(),"observedBuildReceipt":pin(&output.join("observed-probe-build.json"))?,"deviceCommand":[supervisor.to_string_lossy(),"hexagon-tools","--request",request_path.to_string_lossy()],"deviceAttempted":false,"performanceQualified":false});
    write_json(&output.join("intake.json"), &intake)?;
    Ok(intake)
}
pub(super) fn run(request: &Request) -> Result<Value> {
    require(
        !(request.batch_four_operations && request.batch_ten_operations),
        "Select at most one explicit HMX batch mode.",
    )?;
    require(
        (1..=3).contains(&request.token_ids.len()),
        "Snapshot budget permits one to three token IDs.",
    )?;
    let maximum = if request.resident_host_weights { 2 } else { 4 };
    require(
        !request.projections.is_empty()
            && request.projections.len() <= maximum
            && request.projections[0] == "blk.0.ffn_up.weight"
            && request.projections.iter().collect::<BTreeSet<_>>().len()
                == request.projections.len(),
        "Select unique FFN projections beginning with blk.0.ffn_up.weight within the arena bound.",
    )?;
    require(
        ["none", "loaded-zero", "standard-scale-1.3"].contains(&request.adapter_case.as_str()),
        "Unsupported adapter control.",
    )?;
    let built = read(&request.successful_build_receipt)?;
    require(
        built["exitCode"].as_i64() == Some(0)
            && built.get("timedOut").is_none_or(|value| value == false),
        "Build receipt must record a successful build.",
    )?;
    for root in [&request.runner_repository, &request.prism_repository] {
        plain(root)?;
    }
    validate_external_path(&request.output_directory).map_err(|error| error.to_string())?;
    require(
        !request.output_directory.exists(),
        "New output directory required.",
    )?;
    let scratch =
        std::env::temp_dir().join(format!("Bonsai-Control-{}", startup_process::unique()));
    fs::create_dir(&scratch).map_err(|error| error.to_string())?;
    let result = (|| {
        let prism = revision(&request.git, &request.prism_repository, &scratch)?;
        let runner = revision(&request.git, &request.runner_repository, &scratch)?;
        fs::create_dir(&request.output_directory).map_err(|error| error.to_string())?;
        intake(request, &prism, &runner)
    })();
    fs::remove_dir_all(&scratch).map_err(|error| error.to_string())?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn model_control_rejects_changed_workload_and_arena_admission_before_intake() -> Result<()> {
        let base = json!({"runnerRepository":"C:/missing-runner","prismRepository":"C:/missing-prism","probe":"C:/missing-probe","supervisor":"C:/missing-tools","compiler":"C:/missing-rustc","successfulBuildReceipt":"C:/missing-build.json","outputDirectory":"C:/missing-control","git":"C:/missing-git"});
        for changes in [
            json!({"tokenIds":[]}),
            json!({"tokenIds":[1,2,3,4]}),
            json!({"projections":["blk.1.ffn_up.weight"]}),
            json!({"projections":["blk.0.ffn_up.weight","blk.0.ffn_up.weight"]}),
            json!({"residentHostWeights":true,"projections":["blk.0.ffn_up.weight","blk.1.ffn_up.weight","blk.2.ffn_up.weight"]}),
            json!({"batchFourOperations":true,"batchTenOperations":true}),
            json!({"adapterCase":"arbitrary"}),
        ] {
            let mut value = base.clone();
            for (key, change) in changes.as_object().ok_or("Fixture update missing.")? {
                value[key] = change.clone();
            }
            let request: Request =
                serde_json::from_value(value).map_err(|error| error.to_string())?;
            assert!(run(&request).is_err());
        }
        for invalid in [json!([-1]), json!([1.5]), json!([true])] {
            let mut value = base.clone();
            value["tokenIds"] = invalid;
            assert!(serde_json::from_value::<Request>(value).is_err());
        }
        Ok(())
    }
}
