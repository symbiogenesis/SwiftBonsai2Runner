//! Native device supervision; all mutable lifetime belongs to an outer Windows Job.
#![cfg(windows)]

use super::{NATIVE_INPUTS, hold, plain, write_json};
use crate::{Result, identity, startup_cache, startup_process};
use bonsai_artifacts::{is_redirect, validate_external_path};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[path = "projection_device.rs"]
mod projection;

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Probe {
    kind: String,
    repository_directory: PathBuf,
    package_directory: PathBuf,
    fixture_directory: PathBuf,
    output_directory: PathBuf,
    signtool_path: PathBuf,
    expected_runner_revision: String,
    maximum_seconds: u32,
    #[serde(default)]
    reference_initialization: bool,
    #[serde(default = "default_case")]
    case: String,
    #[serde(default)]
    projection_request_file: Option<PathBuf>,
}
fn default_case() -> String {
    "F32".into()
}

fn validate(request: &Probe) -> Result<()> {
    if !cfg!(all(windows, target_arch = "aarch64")) {
        return Err("Native Windows ARM64 is required for Hexagon device probes.".into());
    }
    let operation = match request.kind.as_str() {
        "dispatch" => "dispatch-plan",
        "session" => "session-plan",
        "compute" | "projection" => "compute-plan",
        _ => return Err("Select one fixed Hexagon device probe.".into()),
    };
    let mut plan = json!({"operation":operation,"maximumSeconds":request.maximum_seconds});
    if request.kind == "session" {
        plan["referenceInitialization"] = json!(request.reference_initialization);
    }
    if request.kind == "compute" || request.kind == "projection" {
        plan["case"] = json!(if request.kind == "projection" {
            "GuardedQ2NoBlock"
        } else {
            &request.case
        });
    }
    crate::hexagon_policy::run(plan)?;
    if request.kind == "projection" {
        projection::validate_request(request)?;
    } else if request.projection_request_file.is_some() {
        return Err("Projection inputs apply only to the selected projection operation.".into());
    }
    if request.reference_initialization && request.kind != "session" {
        return Err("Reference initialization applies only to a selected session probe.".into());
    }
    if !identity::is_lower_hex(&request.expected_runner_revision, 40) {
        return Err("An exact expected Runner revision is required.".into());
    }
    plain(&request.repository_directory)?;
    for path in [
        &request.package_directory,
        &request.fixture_directory,
        &request.output_directory,
        &request.signtool_path,
    ] {
        validate_external_path(path).map_err(|error| error.to_string())?;
    }
    if !request.package_directory.is_dir()
        || !request.fixture_directory.is_dir()
        || !request.signtool_path.is_file()
    {
        return Err("Select existing package, fixture and catalog verifier inputs.".into());
    }
    Ok(())
}

fn save(path: &Path, value: &Value) -> Result<()> {
    let temporary = path.with_extension(format!("{}.tmp", startup_process::unique()));
    write_json(&temporary, value)?;
    fs::rename(&temporary, path).map_err(|error| error.to_string())
}

fn bounded_text(path: &Path, maximum: u64) -> Result<String> {
    String::from_utf8(bounded_bytes(path, maximum)?).map_err(|error| error.to_string())
}

fn bounded_bytes(path: &Path, maximum: u64) -> Result<Vec<u8>> {
    let file = File::open(path).map_err(|error| error.to_string())?;
    if file.metadata().map_err(|error| error.to_string())?.len() > maximum {
        return Err("Hexagon capture exceeded its fixed byte cap.".into());
    }
    let mut bytes = Vec::new();
    file.take(maximum + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if u64::try_from(bytes.len()).map_err(|error| error.to_string())? > maximum {
        return Err("Hexagon capture exceeded its fixed byte cap.".into());
    }
    Ok(bytes)
}

fn marker<'a>(text: &'a str, kind: &str) -> Option<&'a str> {
    let (prefix, allowed): (&str, &[&str]) = match kind {
        "dispatch" => (
            "hexagon-fixture:stage=",
            &[
                "load",
                "registry",
                "backend-init",
                "context",
                "allocation",
                "transfer",
                "dispatch",
                "readback",
            ],
        ),
        "session" => (
            "hexagon-session:stage=",
            &[
                "reference-process-init-entered",
                "module-open-entered",
                "allocation-entered",
                "descriptor-entered",
                "fd-mapping-entered",
                "fd-unmapping-entered",
                "free-entered",
                "module-close-entered",
            ],
        ),
        "compute" => ("hexagon-compute:stage=", &[]),
        "projection" => ("hexagon-projection:stage=", &[]),
        _ => return None,
    };
    text.split_inclusive('\n')
        .filter_map(|line| {
            let label = line
                .strip_suffix('\n')?
                .trim_end_matches('\r')
                .strip_prefix(prefix)?;
            if if kind == "compute" {
                !label.is_empty()
                    && label.len() <= 96
                    && label.as_bytes()[0].is_ascii_lowercase()
                    && label.bytes().all(|byte| {
                        byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'
                    })
            } else {
                allowed.contains(&label)
            } {
                Some(label)
            } else {
                None
            }
        })
        .next_back()
}

#[cfg(windows)]
pub(super) fn removals() -> Vec<String> {
    let names = [
        "ACTIONS_RUNTIME_TOKEN",
        "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
        "ACTIONS_ID_TOKEN_REQUEST_URL",
        "GITHUB_TOKEN",
        "GH_TOKEN",
        "ACTIONS_RUNNER_INPUT_TOKEN",
        "LD_LIBRARY_PATH",
        "LD_PRELOAD",
    ];
    let prefixes = [
        "GGML_", "GENIEX_", "QNN_", "QAIRT_", "HEXAGON_", "ADSP_", "CDSP_", "FASTRPC_", "RPCMEM_",
    ];
    std::env::vars_os()
        .map(|(name, _)| name.to_string_lossy().into_owned())
        .filter(|name| {
            names
                .iter()
                .any(|selected| name.eq_ignore_ascii_case(selected))
                || prefixes
                    .iter()
                    .any(|prefix| name.to_ascii_uppercase().starts_with(prefix))
        })
        .collect()
}

#[cfg(windows)]
pub(super) fn driver(path: &Path, holds: &mut Vec<File>) -> Result<Value> {
    let system = std::env::var_os("SystemRoot").ok_or("SystemRoot unavailable.")?;
    let root = PathBuf::from(system).join("System32/DriverStore/FileRepository");
    let normalized = path.to_string_lossy().replace('/', "\\");
    let root = root.to_string_lossy().replace('/', "\\") + "\\";
    if !normalized
        .to_ascii_lowercase()
        .starts_with(&root.to_ascii_lowercase())
        || path.file_name().is_none_or(|name| {
            name.to_str()
                .is_none_or(|name| !name.eq_ignore_ascii_case("libcdsprpc.dll"))
        })
    {
        return Err("Observed FastRPC driver must belong to the Windows DriverStore.".into());
    }
    let (file, digest) = hold(path, 32 * 1024 * 1024)?;
    let signer = bonsai_windows_process::verify_authenticode(&file, path)
        .map_err(|error| error.to_string())?;
    if signer.common_name != "Microsoft Windows Hardware Compatibility Publisher" {
        return Err("Observed driver lacks the reviewed Microsoft publisher.".into());
    }
    let value = json!({"Path":path,"Bytes":file.metadata().map_err(|error| error.to_string())?.len(),"Sha256":digest,"SignatureStatus":"Valid","Publisher":signer.common_name,"SignerThumbprint":signer.thumbprint,"FileHeldAgainstMutation":true});
    crate::hexagon_policy::run(
        json!({"operation":"driver-identity","json":serde_json::to_string(&value).map_err(|error| error.to_string())?}),
    )?;
    holds.push(file);
    Ok(value)
}

#[cfg(windows)]
#[expect(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "Explicit capture inputs bind process ownership, progress and retained artifact guards."
)]
fn capture(
    executable: &Path,
    arguments: Vec<OsString>,
    working: &Path,
    report: &mut Value,
    progress: &Path,
    kind: &str,
    observe: bool,
    holds: &mut Vec<File>,
) -> Result<(u32, String)> {
    use bonsai_windows_process::{OwnedProcess, ToolProcessSpec};
    let serial = startup_process::unique();
    let scratch = std::env::temp_dir().join(format!("bonsai-hexagon-capture-{serial}"));
    validate_external_path(&scratch).map_err(|error| error.to_string())?;
    fs::create_dir(&scratch).map_err(|error| error.to_string())?;
    let stdout = scratch.join("stdout");
    let stderr = scratch.join("stderr");
    let system = PathBuf::from(std::env::var_os("SystemRoot").ok_or("SystemRoot unavailable.")?)
        .join("System32");
    let environment = BTreeMap::from([
        (
            "PATH".into(),
            if kind == "dispatch" {
                format!("{};{}", working.display(), system.display())
            } else {
                system.to_string_lossy().into_owned()
            },
        ),
        (
            "ADSP_LIBRARY_PATH".into(),
            working.to_string_lossy().into_owned(),
        ),
    ]);
    let process = OwnedProcess::start_tool_with_removed(
        &ToolProcessSpec {
            executable: executable.to_owned(),
            arguments,
            working_directory: working.to_owned(),
            stdout: stdout.clone(),
            stderr: stderr.clone(),
            environment,
        },
        &removals(),
    )
    .map_err(|error| error.to_string())?;
    let clock = Instant::now();
    let result = (|| -> Result<(u32, String)> {
        loop {
            if fs::metadata(&stdout)
                .map_err(|error| error.to_string())?
                .len()
                > 131_072
                || fs::metadata(&stderr)
                    .map_err(|error| error.to_string())?
                    .len()
                    > 65_536
            {
                return Err("Hexagon fixture stream exceeded its fixed byte cap.".into());
            }
            let error_bytes = bounded_bytes(&stderr, 65_536)?;
            let error_text = String::from_utf8_lossy(&error_bytes);
            if let Some(label) = marker(&error_text, kind) {
                let field = if kind == "dispatch" {
                    "LastFixtureStage"
                } else {
                    "LastNativeStage"
                };
                if report[field] != label {
                    report[field] = json!(label);
                    save(progress, report)?;
                }
            }
            if observe && report["LoadedDriverObservation"] == "not-observed" {
                match bonsai_windows_process::process_module_paths(process.id(), "libcdsprpc.dll") {
                    Ok(paths) if paths.len() == 1 => {
                        match driver(&paths[0], holds) {
                            Ok(identity) => {
                                report["LoadedDriverIdentity"] = identity;
                                report["LoadedDriverObservation"] = json!("observed-verified");
                            }
                            Err(_) => {
                                report["LoadedDriverObservation"] = json!("observation-rejected");
                            }
                        }
                        save(progress, report)?;
                    }
                    Ok(paths) if paths.len() > 1 => {
                        report["LoadedDriverObservation"] = json!("observation-rejected");
                        save(progress, report)?;
                    }
                    _ => {}
                }
            }
            if let Some(code) = process.try_wait().map_err(|error| error.to_string())? {
                // Root exit can race the last progress read. Observe complete
                // stderr once more after all child writes have finished.
                let bytes = bounded_bytes(&stderr, 65_536)?;
                if let Some(label) = marker(&String::from_utf8_lossy(&bytes), kind) {
                    let field = if kind == "dispatch" {
                        "LastFixtureStage"
                    } else {
                        "LastNativeStage"
                    };
                    if report[field] != label {
                        report[field] = json!(label);
                        save(progress, report)?;
                    }
                }
                return Ok((code, bounded_text(&stdout, 131_072)?));
            }
            let deadline = if kind == "catalog" {
                Duration::from_secs(10)
            } else {
                Duration::from_mins(4)
            };
            if clock.elapsed() >= deadline {
                return Err("Hexagon capture exhausted its worker deadline.".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    })();
    // Keep verified input handles alive until teardown is confirmed. A refused
    // termination remains inside the outer supervisor's deadline-owned Job.
    while process.stop(Duration::from_secs(1)).is_err() {
        std::thread::sleep(Duration::from_millis(10));
    }
    let _ = fs::remove_file(stdout);
    let _ = fs::remove_file(stderr);
    let _ = fs::remove_dir(scratch);
    result
}

fn parsed(text: &str) -> Result<Value> {
    crate::hexagon_policy::run(json!({"operation":"json","json":text}))
}

#[derive(Default)]
pub(super) struct Inputs {
    pub(super) holds: Vec<File>,
}
impl Inputs {
    pub(super) fn digest(&mut self, path: &Path, maximum: u64) -> Result<String> {
        let (file, digest) = hold(path, maximum)?;
        self.holds.push(file);
        Ok(digest)
    }
    pub(super) fn json(&mut self, path: &Path, maximum: u64) -> Result<Value> {
        let (mut file, _) = hold(path, maximum)?;
        let mut text = String::new();
        file.read_to_string(&mut text)
            .map_err(|error| error.to_string())?;
        self.holds.push(file);
        parsed(&text)
    }
    pub(super) fn pinned(&mut self, path: &Path, bytes: &Value, digest: &Value) -> Result<()> {
        let bytes = bytes
            .as_u64()
            .filter(|bytes| (1..=128 * 1024 * 1024).contains(bytes))
            .ok_or("Invalid bounded runtime length.")?;
        let expected = digest
            .as_str()
            .filter(|digest| identity::is_lower_hex(digest, 64))
            .ok_or("Invalid runtime digest.")?;
        let (file, actual) = hold(path, bytes)?;
        if file.metadata().map_err(|error| error.to_string())?.len() != bytes || actual != expected
        {
            return Err("Hexagon file differs from exact pinned bytes.".into());
        }
        self.holds.push(file);
        Ok(())
    }
}

fn exact_directory(path: &Path, names: &BTreeSet<String>) -> Result<()> {
    let mut seen = BTreeSet::new();
    for entry in fs::read_dir(path).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "Invalid Hexagon bundle name.".to_string())?;
        let metadata = fs::symlink_metadata(entry.path()).map_err(|error| error.to_string())?;
        if !metadata.is_file()
            || is_redirect(&metadata)
            || !names.contains(&name)
            || !seen.insert(name)
        {
            return Err("Hexagon directory contains an unpinned entry or redirect.".into());
        }
    }
    if seen != *names {
        return Err("Hexagon directory omits pinned entries.".into());
    }
    Ok(())
}

fn file_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name.as_bytes()[0].is_ascii_alphanumeric()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
}
fn runtime(
    inputs: &mut Inputs,
    request: &Probe,
    lock: &Value,
    report: &mut Value,
) -> Result<Vec<String>> {
    let compute = request.kind == "compute" || request.kind == "projection";
    let (files, members) = if compute {
        let path = request
            .repository_directory
            .join("config/hexagon-signed-q2-candidate.lock.json");
        report["CandidateLockSha256"] = json!(inputs.digest(&path, 1024 * 1024)?);
        let candidate = inputs.json(&path, 1024 * 1024)?;
        if candidate["candidate"]["tag"] != "v0.10.0"
            || candidate["candidate"]["gitlink"]["commit"]
                != "19cce6919992fdffd92e0ac152caaf1e2b8f254e"
        {
            return Err("The compute package selection changed.".into());
        }
        (
            candidate["verifiedRuntimeFiles"]
                .as_array()
                .filter(|files| files.len() == 17)
                .ok_or("Invalid compute file set.")?
                .clone(),
            candidate["verifiedCatalogMembers"]
                .as_array()
                .filter(|members| members.len() == 5)
                .ok_or("Invalid compute catalog set.")?
                .clone(),
        )
    } else {
        if lock["schemaVersion"] != 1
            || lock["ggmlCommit"] != "94256114c229674ef96e76eb2dea596e65b43818"
            || lock["catalog"]["name"] != "libggml-htp.cat"
            || lock["catalog"]["member"] != "libggml-htp-v81.so"
        {
            return Err("The reviewed signed runtime selection changed.".into());
        }
        (
            lock["files"]
                .as_array()
                .filter(|files| files.len() == 16)
                .ok_or("Invalid runtime file set.")?
                .clone(),
            lock["catalog"]["members"]
                .as_array()
                .filter(|members| members.len() == 4)
                .ok_or("Invalid catalog member set.")?
                .clone(),
        )
    };
    let mut expected_members = vec![
        "libggml-htp-v73.so".to_owned(),
        "libggml-htp-v75.so".into(),
        "libggml-htp-v79.so".into(),
        "libggml-htp-v81.so".into(),
    ];
    if compute {
        expected_members.push("libggml-htp-v85.so".into());
    }
    if members
        != expected_members
            .iter()
            .map(|member| json!(member))
            .collect::<Vec<_>>()
    {
        return Err(
            "Every possible bundled DSP must match the reviewed catalog member set.".into(),
        );
    }
    let mut names = BTreeSet::new();
    let mut case_names = BTreeSet::new();
    let mut total = 0_u64;
    for pin in &files {
        let name = pin[if compute { "Name" } else { "name" }]
            .as_str()
            .filter(|name| file_name(name))
            .ok_or("Invalid runtime filename.")?;
        if !names.insert(name.to_owned()) || !case_names.insert(name.to_ascii_lowercase()) {
            return Err("Duplicate runtime filename.".into());
        }
        let bytes = &pin[if compute { "Bytes" } else { "bytes" }];
        inputs.pinned(
            &request.package_directory.join(name),
            bytes,
            &pin[if compute { "Sha256" } else { "sha256" }],
        )?;
        total = total
            .checked_add(bytes.as_u64().ok_or("Invalid runtime byte count.")?)
            .ok_or("Runtime length overflow.")?;
        if total > 256 * 1024 * 1024 {
            return Err("Runtime exceeds its aggregate byte bound.".into());
        }
        report["RuntimeFilesVerified"] = json!(names.len());
    }
    if !["libggml-htp.cat", "libggml-htp.inf"]
        .iter()
        .all(|name| names.contains(*name))
        || !expected_members.iter().all(|name| names.contains(name))
    {
        return Err("Runtime omits signed deployment inputs.".into());
    }
    exact_directory(&request.package_directory, &names)?;
    Ok(expected_members)
}

/// Reuse the complete signed compute-package intake for the local model probe.
pub(super) fn compute_package(
    inputs: &mut Inputs,
    repository: &Path,
    package: &Path,
    report: &mut Value,
) -> Result<Vec<String>> {
    let request = Probe {
        kind: "projection".into(),
        repository_directory: repository.to_owned(),
        package_directory: package.to_owned(),
        fixture_directory: PathBuf::new(),
        output_directory: PathBuf::new(),
        signtool_path: PathBuf::new(),
        expected_runner_revision: String::new(),
        maximum_seconds: 300,
        reference_initialization: false,
        case: default_case(),
        projection_request_file: None,
    };
    runtime(inputs, &request, &Value::Null, report)
}

fn worker_report(request: &Probe) -> Value {
    let mut report = json!({"SchemaVersion":1,"Kind":format!("runner-hexagon-{}-worker",request.kind),"SourceRevision":request.expected_runner_revision,"Outcome":"pending","Stage":"source-identity","Failure":null,"RuntimeFilesVerified":0,"ModelLoaded":false,"SwiftQualified":false,"ProfileEligible":false,"TrustChanged":false});
    let extra = match request.kind.as_str() {
        "dispatch" => {
            json!({"CatalogMemberVerified":false,"CatalogMembersVerified":0,"CatalogMemberNames":[],"SelectedDspArchitecture":"unknown","LastFixtureStage":"not-started","DeviceDispatchVerified":false,"FixtureResult":null,"LoadedDriverObservation":"not-started","LoadedDriverIdentity":null,"FixtureSha256":null,"FixtureSourceSha256":null,"ManifestSha256":null,"LockSha256":null,"SigntoolSha256":null,"DriverInstalled":false})
        }
        "session" => {
            json!({"LastNativeStage":"not-started","ReferenceProcessInitializationSelected":request.reference_initialization,"UnsignedProcessRequested":false,"ProcessInitializationResult":null,"SignedPackageVerified":false,"DriverIdentityVerified":false,"DriverIdentity":null,"MappingCycleVerified":false,"NativeResult":null,"ExecutableSha256":null,"BuildReceiptSha256":null,"ManifestSha256":null,"LockSha256":null,"DriverInstalled":false,"ComputeExecuted":false})
        }
        _ => {
            json!({"ComputeCase":request.case,"LastNativeStage":"not-started","SignedPackageVerified":false,"DriverIdentity":null,"NativeResult":null,"BuildReceiptSha256":null,"CandidateLockSha256":null,"ComputeExecuted":false})
        }
    };
    if let (Some(target), Some(extra)) = (report.as_object_mut(), extra.as_object()) {
        target.extend(extra.clone());
    }
    report
}

fn verify_fixture(
    inputs: &mut Inputs,
    request: &Probe,
    report: &mut Value,
    lock_digest: &str,
    manifest_digest: &str,
) -> Result<PathBuf> {
    if request.kind == "projection" {
        return projection::verify_fixture(inputs, request, report, manifest_digest, lock_digest);
    }
    let dispatch = request.kind == "dispatch";
    let exe = if dispatch {
        "hexagon-dispatch-fixture.exe"
    } else {
        "native-hexagon-query.exe"
    };
    let receipt_name = if dispatch {
        "fixture-build.json"
    } else {
        "native-build.json"
    };
    exact_directory(
        &request.fixture_directory,
        &BTreeSet::from([exe.to_owned(), receipt_name.to_owned()]),
    )?;
    let receipt_path = request.fixture_directory.join(receipt_name);
    let receipt_digest = inputs.digest(&receipt_path, 65_536)?;
    let receipt = inputs.json(&receipt_path, 65_536)?;
    if dispatch {
        let source = inputs.digest(
            &request
                .repository_directory
                .join("tools/hexagon/dispatch_fixture.rs"),
            1024 * 1024,
        )?;
        report["FixtureSourceSha256"] = json!(source);
        crate::hexagon_policy::run(
            json!({"operation":"dispatch-build","json":serde_json::to_string(&receipt).map_err(|error| error.to_string())?,"expectedRunnerRevision":request.expected_runner_revision,"sourceSha256":source,"manifestSha256":manifest_digest,"lockSha256":lock_digest}),
        )?;
    } else {
        report["BuildReceiptSha256"] = json!(receipt_digest);
        let mut sources = serde_json::Map::new();
        for name in NATIVE_INPUTS {
            sources.insert(
                (*name).to_owned(),
                json!(
                    inputs.digest(
                        &request
                            .repository_directory
                            .join("tools/hexagon/native_rpc")
                            .join(name),
                        1024 * 1024
                    )?
                ),
            );
        }
        crate::hexagon_policy::run(
            json!({"operation":"session-build","json":serde_json::to_string(&receipt).map_err(|error| error.to_string())?,"expectedRunnerRevision":request.expected_runner_revision,"sourceDigests":sources,"manifestSha256":manifest_digest,"lockSha256":lock_digest}),
        )?;
    }
    let path = request.fixture_directory.join(exe);
    inputs.pinned(
        &path,
        &receipt["Executable"]["Bytes"],
        &receipt["Executable"]["Sha256"],
    )?;
    startup_cache::assert_arm64(&path)?;
    if dispatch {
        report["FixtureSha256"] = receipt["Executable"]["Sha256"].clone();
    } else if request.kind == "session" {
        report["ExecutableSha256"] = receipt["Executable"]["Sha256"].clone();
    }
    Ok(path)
}

#[cfg(windows)]
#[expect(
    clippy::too_many_lines,
    reason = "One ordered worker owns immutable source/runtime guards through catalog verification, driver inspection and the fixed device probe."
)]
fn execute_worker(request: &Probe, report: &mut Value, progress: &Path) -> Result<()> {
    let mut inputs = Inputs { holds: Vec::new() };
    let lock_path = request
        .repository_directory
        .join("config/hexagon-dispatch.lock.json");
    let lock_digest = inputs.digest(&lock_path, 1024 * 1024)?;
    let lock = inputs.json(&lock_path, 1024 * 1024)?;
    let manifest = request
        .repository_directory
        .join("tools/hexagon/diagnostic.manifest");
    let manifest_digest = inputs.digest(&manifest, 65_536)?;
    if fs::read(&manifest).map_err(|error| error.to_string())?
        != include_bytes!("../../../hexagon/diagnostic.manifest")
    {
        return Err("The fixed non-elevating diagnostic manifest changed.".into());
    }
    if request.kind != "compute" && request.kind != "projection" {
        report["LockSha256"] = json!(lock_digest);
        report["ManifestSha256"] = json!(manifest_digest);
    }
    let exe = verify_fixture(&mut inputs, request, report, &lock_digest, &manifest_digest)?;
    report["Stage"] = json!("runtime-identity");
    save(progress, report)?;
    let members = runtime(&mut inputs, request, &lock, report)?;
    report["Stage"] = json!("catalog-verification");
    save(progress, report)?;
    inputs.pinned(
        &request.signtool_path,
        &lock["signtool"]["bytes"],
        &lock["signtool"]["sha256"],
    )?;
    if request.kind == "dispatch" {
        report["SigntoolSha256"] = lock["signtool"]["sha256"].clone();
    }
    let mut arguments = vec![
        "verify".into(),
        "/pa".into(),
        "/v".into(),
        "/c".into(),
        request
            .package_directory
            .join("libggml-htp.cat")
            .into_os_string(),
    ];
    arguments.extend(
        members
            .iter()
            .map(|name| request.package_directory.join(name).into_os_string()),
    );
    let (status, _) = capture(
        &request.signtool_path,
        arguments,
        &request.package_directory,
        report,
        progress,
        "catalog",
        false,
        &mut inputs.holds,
    )?;
    if status != 0 {
        return Err("DSP catalog membership remains unverified.".into());
    }
    if request.kind == "dispatch" {
        report["CatalogMemberVerified"] = json!(true);
        report["CatalogMembersVerified"] = json!(members.len());
        report["CatalogMemberNames"] = json!(members);
        report["Stage"] = json!("device-dispatch");
        report["LoadedDriverObservation"] = json!("not-observed");
        save(progress, report)?;
        let (code, text) = capture(
            &exe,
            vec![
                "--runtime-directory".into(),
                request.package_directory.clone().into_os_string(),
            ],
            &request.package_directory,
            report,
            progress,
            "dispatch",
            true,
            &mut inputs.holds,
        )?;
        let native = crate::hexagon_policy::run(
            json!({"operation":"dispatch-result","json":text,"exitCode":i32::try_from(code).map_err(|_| "Invalid dispatch exit code.")?}),
        )?;
        for name in ["Outcome", "Stage", "Failure", "DeviceDispatchVerified"] {
            report[name] = native[name].clone();
        }
        report["FixtureResult"] = native;
        return Ok(());
    }
    report["SignedPackageVerified"] = json!(true);
    report["Stage"] = json!("driver-inspection");
    save(progress, report)?;
    let (status, text) = capture(
        &exe,
        vec!["--inspect".into()],
        &request.package_directory,
        report,
        progress,
        "inspection",
        false,
        &mut inputs.holds,
    )?;
    let inspected = parsed(&text)?;
    if status != 0
        || inspected["schemaVersion"] != 1
        || inspected["kind"] != "native-fastrpc-inspection"
        || ["driverLoaded", "dspCodeLoaded", "profileEligible"]
            .iter()
            .any(|field| inspected[*field] != false)
    {
        return Err("Native driver inspection failed its non-execution contract.".into());
    }
    let driver_path = PathBuf::from(
        inspected["driverDll"]
            .as_str()
            .ok_or("Native inspection omitted driver path.")?,
    );
    report["DriverIdentity"] = driver(&driver_path, &mut inputs.holds)?;
    if request.kind == "projection" {
        return projection::execute(&mut inputs, request, report, progress, &exe);
    }
    let (mode, stage, operation) = if request.kind == "session" {
        report["DriverIdentityVerified"] = json!(true);
        if request.reference_initialization {
            report["UnsignedProcessRequested"] = Value::Null;
        }
        (
            if request.reference_initialization {
                "--reference-session-mapping-probe"
            } else {
                "--session-mapping-probe"
            },
            "session-mapping",
            "session-result",
        )
    } else {
        (
            match request.case.as_str() {
                "F32" => "--reference-queue-compute-probe",
                "GuardedQ2" => "--reference-queue-q2-probe",
                "F32NoBlock" => "--reference-queue-noblock-probe",
                "GuardedQ2NoBlock" => "--reference-queue-q2-noblock-probe",
                _ => return Err("Select one fixed compute case.".into()),
            },
            "queue-compute",
            "compute-result",
        )
    };
    report["Stage"] = json!(stage);
    save(progress, report)?;
    let (code, text) = capture(
        &exe,
        vec![
            mode.into(),
            request.package_directory.clone().into_os_string(),
        ],
        &request.package_directory,
        report,
        progress,
        &request.kind,
        false,
        &mut inputs.holds,
    )?;
    let mut policy = json!({"operation":operation,"json":text,"exitCode":i32::try_from(code).map_err(|_| "Invalid native probe exit code.")?});
    if request.kind == "session" {
        policy["referenceInitialization"] = json!(request.reference_initialization);
    } else {
        policy["case"] = json!(request.case);
    }
    let native = crate::hexagon_policy::run(policy)?;
    if native["driverDll"] != report["DriverIdentity"]["Path"] {
        return Err("Native execution used a different inspected driver.".into());
    }
    if request.kind == "session" {
        report["UnsignedProcessRequested"] = native["unsignedProcessRequested"].clone();
        report["ProcessInitializationResult"] = native["processInitializationResult"].clone();
        report["MappingCycleVerified"] = native["passed"].clone();
    } else {
        report["ComputeExecuted"] = native["computeExecuted"].clone();
    }
    if native["passed"] == true {
        report["Outcome"] = json!("passed");
        report["Stage"] = json!(format!("{stage}-complete"));
    } else {
        report["Failure"] = json!(format!("native-{stage}-incomplete"));
    }
    report["NativeResult"] = native;
    Ok(())
}

pub(super) fn worker(request: &Probe) -> Result<Value> {
    let _projection_request_guard = projection::hold_request(request)?;
    validate(request)?;
    if !request.output_directory.is_dir() {
        return Err("The device supervisor must create evidence storage first.".into());
    }
    let progress = request.output_directory.join("worker.json");
    let mut report = worker_report(request);
    save(&progress, &report)?;
    #[cfg(windows)]
    let result = execute_worker(request, &mut report, &progress);
    #[cfg(not(windows))]
    let result: Result<()> = Err("Hexagon execution requires Windows.".into());
    if result.is_err() {
        report["Failure"] = json!(format!(
            "{}-rejected",
            report["Stage"].as_str().unwrap_or("source-identity")
        ));
        report["Outcome"] = json!("pending");
    }
    save(&progress, &report)?;
    Ok(report)
}

fn worker_result(request: &Probe, text: &str) -> Result<Value> {
    if request.kind == "projection" {
        return projection::worker_result(request, text);
    }
    let operation = match request.kind.as_str() {
        "dispatch" => "dispatch-worker",
        "session" => "session-worker",
        _ => "json",
    };
    let mut policy = json!({"operation":operation,"json":text});
    if request.kind != "compute" {
        policy["expectedRunnerRevision"] = json!(request.expected_runner_revision);
    }
    if request.kind == "session" {
        policy["referenceInitialization"] = json!(request.reference_initialization);
    }
    let worker = crate::hexagon_policy::run(policy)?;
    if request.kind == "compute" {
        if worker["Kind"] != "runner-hexagon-compute-worker"
            || worker["ComputeCase"] != request.case
            || worker["SourceRevision"] != request.expected_runner_revision
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
            return Err("Invalid owned compute worker result.".into());
        }
        if worker["Outcome"] == "passed" {
            let native = crate::hexagon_policy::run(
                json!({"operation":"compute-result","json":serde_json::to_string(&worker["NativeResult"]).map_err(|error| error.to_string())?,"exitCode":0,"case":request.case}),
            )?;
            if native["passed"] != true
                || worker["SignedPackageVerified"] != true
                || worker["RuntimeFilesVerified"] != 17
                || worker["ComputeExecuted"] != true
            {
                return Err("Incomplete verified computation.".into());
            }
        }
    }
    Ok(worker)
}

pub(super) fn supervise(request: &Probe) -> Result<Value> {
    let clock = Instant::now();
    let _projection_request_guard = projection::hold_request(request)?;
    validate(request)?;
    fs::create_dir(&request.output_directory)
        .map_err(|_| "Hexagon output must be new external storage.".to_string())?;
    let mut report = json!({"SchemaVersion":1,"Kind":format!("runner-hexagon-{}",request.kind),"Outcome":"pending","Failure":null,"ExpectedRunnerRevision":request.expected_runner_revision,"MaximumSeconds":request.maximum_seconds,"CleanupReserveSeconds":75,"WorkerAllowanceSeconds":0,"ElapsedMilliseconds":0,"WorkerAttempted":false,"OwnedJobTeardown":"not-started","Worker":null,"ModelLoaded":false,"SwiftQualified":false,"ProfileEligible":false,"DriverInstalled":false,"TrustChanged":false});
    let field = match request.kind.as_str() {
        "dispatch" => "DeviceDispatchVerified",
        "session" => "MappingCycleVerified",
        _ => "ComputeExecuted",
    };
    report[field] = json!(false);
    if request.kind == "session" {
        report["ReferenceProcessInitializationSelected"] = json!(request.reference_initialization);
        report["UnsignedProcessRequested"] = json!(false);
        report["ProcessInitializationResult"] = Value::Null;
    }
    if request.kind == "compute" {
        report["ComputeCase"] = json!(request.case);
    }
    let allowance = Duration::from_secs(u64::from(request.maximum_seconds))
        .saturating_sub(clock.elapsed())
        .saturating_sub(Duration::from_secs(75));
    if allowance < Duration::from_secs(1) {
        report["Failure"] = json!("insufficient-owned-cleanup-reserve");
    } else {
        let path = request.output_directory.join("worker-request.json");
        write_json(
            &path,
            &json!({"operation":"device-worker","request":request}),
        )?;
        report["WorkerAllowanceSeconds"] = json!(allowance.as_secs());
        report["WorkerAttempted"] = json!(true);
        report["OwnedJobTeardown"] = json!("unconfirmed");
        let current = std::env::current_exe().map_err(|error| error.to_string())?;
        let result = startup_process::run(
            current.as_os_str(),
            &[
                "hexagon-tools".into(),
                "--request".into(),
                path.into_os_string(),
            ],
            &request.output_directory,
            &BTreeMap::new(),
            allowance,
            &request.output_directory.join("supervisor.txt"),
        );
        match result {
            Ok(output)
                if output.code == 0
                    && output.stdout.len() <= 131_072
                    && output.stderr.len() <= 65_536 =>
            {
                report["OwnedJobTeardown"] = json!("confirmed-empty");
                if let Ok(worker) = worker_result(request, &output.stdout) {
                    for name in ["Outcome", "Failure", field] {
                        report[name] = worker[name].clone();
                    }
                    if request.kind == "session" {
                        report["UnsignedProcessRequested"] =
                            worker["UnsignedProcessRequested"].clone();
                        report["ProcessInitializationResult"] =
                            worker["ProcessInitializationResult"].clone();
                    }
                    report["Worker"] = worker;
                } else {
                    report["Failure"] = json!("owned-worker-result-rejected");
                }
            }
            _ => {
                report["Failure"] = json!(format!("owned-{}-incomplete", request.kind));
                if let Ok(text) =
                    bounded_text(&request.output_directory.join("worker.json"), 65_536)
                    && let Ok(worker) = worker_result(request, &text)
                {
                    report["Worker"] = worker;
                }
            }
        }
    }
    report["ElapsedMilliseconds"] = json!(clock.elapsed().as_millis());
    write_json(
        &request
            .output_directory
            .join(format!("{}.json", request.kind)),
        &report,
    )?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_capture_bounds_streams_and_retains_entered_stage() -> Result<()> {
        let scratch = std::env::temp_dir().join(format!(
            "bonsai-hexagon-capture-test-{}",
            startup_process::unique()
        ));
        fs::create_dir(&scratch).map_err(|error| error.to_string())?;
        let current = std::env::current_exe().map_err(|error| error.to_string())?;
        let mut report = json!({"LastFixtureStage":"not-started"});
        let progress = scratch.join("progress.json");
        let arguments = vec![
            "--exact".into(),
            "hexagon_tools::device::tests::fixture_streams".into(),
            "--ignored".into(),
            "--nocapture".into(),
        ];
        let (code, text) = capture(
            &current,
            arguments,
            &scratch,
            &mut report,
            &progress,
            "dispatch",
            false,
            &mut Vec::new(),
        )?;
        assert_eq!(code, 0);
        assert!(text.contains("fixture-ok"));
        assert_eq!(report["LastFixtureStage"], "context");
        let saved: Value =
            serde_json::from_slice(&fs::read(&progress).map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())?;
        assert_eq!(saved["LastFixtureStage"], "context");
        let arguments = vec![
            "--exact".into(),
            "hexagon_tools::device::tests::fixture_overflow".into(),
            "--ignored".into(),
            "--nocapture".into(),
        ];
        assert!(
            capture(
                &current,
                arguments,
                &scratch,
                &mut report,
                &progress,
                "dispatch",
                false,
                &mut Vec::new()
            )
            .is_err()
        );
        fs::remove_dir_all(&scratch).map_err(|error| error.to_string())?;
        Ok(())
    }
    #[test]
    #[ignore = "Executed only in the owned subprocess capture regression."]
    fn fixture_streams() -> std::io::Result<()> {
        use std::io::Write as _;
        std::io::stdout().write_all(b"fixture-ok")?;
        let mut stderr = std::io::stderr();
        stderr.write_all(b"hexagon-fixture:stage=private-path\nhexagon-fixture:stage=con")?;
        stderr.flush()?;
        std::thread::sleep(Duration::from_millis(30));
        stderr.write_all(b"text\n")
    }
    #[test]
    #[ignore = "Executed only in the owned subprocess overflow regression."]
    fn fixture_overflow() -> std::io::Result<()> {
        use std::io::Write as _;
        std::io::stdout().write_all(&vec![b'x'; 131_073])
    }
    #[test]
    fn progress_requires_complete_public_markers() {
        assert_eq!(
            marker(
                "hexagon-fixture:stage=private-path\nhexagon-fixture:stage=con",
                "dispatch"
            ),
            None
        );
        assert_eq!(
            marker(
                "hexagon-fixture:stage=private-path\nhexagon-fixture:stage=context\n",
                "dispatch"
            ),
            Some("context")
        );
        assert_eq!(
            marker(
                "hexagon-session:stage=reference-process-init-entered\n",
                "session"
            ),
            Some("reference-process-init-entered")
        );
        assert_eq!(
            marker("hexagon-compute:stage=C:/private\n", "compute"),
            None
        );
        assert!(!file_name("../untrusted.dll"));
        assert!(!file_name("file:stream"));
    }
}
