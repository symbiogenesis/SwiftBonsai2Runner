//! Hosted source preparation and exact collector controls; no owner-device execution.

use super::{
    ADAPTER, BASELINE, BTreeSet, CONTROLS, MODEL, PACKAGES, PHASES, Path, PathBuf, REVISION,
    Result, SCHEDULE, Value, archive, array, bytes, digest, fs, hex, json, json_bytes, matches,
    messages, object, profile, read, reject_redirects, require, subset, validate_preparation,
};
use std::{collections::BTreeMap, ffi::OsString, io::Write as _, time::Duration};

const CHANNEL: &str = "nightly-2026-11-09";
const TARGET: &str = "aarch64-pc-windows-msvc";
const STATE: &str = "graph_parallel_rows_preserve_logits_checkpoints_and_recovery";
const CHECK_ARGUMENTS: [(&str, &[&str]); 6] = [
    (
        "clippy",
        &[
            "--locked",
            "--offline",
            "--release",
            "-p",
            "prism-inference",
            "--all-targets",
            "--features",
            "prism-inference/parallel-rows",
            "--",
            "-D",
            "warnings",
        ],
    ),
    (
        "check",
        &[
            "--locked",
            "--offline",
            "--release",
            "-p",
            "prism-inference",
            "--lib",
            "--no-default-features",
        ],
    ),
    (
        "test",
        &[
            "--locked",
            "--offline",
            "--release",
            "-p",
            "prism-inference",
            "--features",
            "parallel-rows",
            "--test",
            "graph",
            STATE,
            "--",
            "--exact",
            "--color",
            "never",
        ],
    ),
    (
        "clippy",
        &[
            "--locked",
            "--offline",
            "--release",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
    ),
    (
        "test",
        &[
            "--locked",
            "--offline",
            "--release",
            "--bin",
            "cpu-bundle-check",
            "--",
            "--color",
            "never",
        ],
    ),
    (
        "build",
        &[
            "--locked",
            "--offline",
            "--release",
            "--bin",
            "cpu-bundle-check",
        ],
    ),
];

fn arm_commands(root: &Path, driver: &Path) -> [Vec<String>; 6] {
    std::array::from_fn(|index| {
        let (verb, args) = CHECK_ARGUMENTS[index];
        cargo(verb, if index < 3 { root } else { driver }, args)
    })
}

fn variable(name: &str) -> String {
    std::env::var(name).unwrap_or_default()
}
fn path(request: &Value, name: &str) -> Result<PathBuf> {
    let result = PathBuf::from(
        request[name]
            .as_str()
            .ok_or_else(|| format!("Missing {name}."))?,
    );
    reject_redirects(&result)?;
    require(
        !result.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )
        }),
        "Preparation requires normalized expanded paths.",
    )?;
    Ok(result)
}
fn command(
    arguments: &[String],
    directory: &Path,
    environment: &BTreeMap<String, String>,
) -> Result<String> {
    let (executable, rest) = arguments
        .split_first()
        .ok_or("Empty preparation command.")?;
    let command_scratch = Scratch::new("command")?;
    let log = command_scratch.0.join("command.log");
    let executable = if cfg!(windows) && Path::new(executable).extension().is_none() {
        format!("{executable}.exe")
    } else {
        executable.clone()
    };
    let removed = if environment.contains_key("RUSTFLAGS") {
        vec!["CARGO_ENCODED_RUSTFLAGS".into()]
    } else {
        Vec::new()
    };
    let output = crate::startup_process::run_strict_with_removed(
        executable.as_ref(),
        &rest.iter().map(OsString::from).collect::<Vec<_>>(),
        directory,
        environment,
        &removed,
        Duration::from_secs(3600),
        &log,
    )?;
    require(
        output.code == 0,
        &format!(
            "Preparation command failed ({}): {} {}",
            output.code,
            output
                .stdout
                .chars()
                .rev()
                .take(6000)
                .collect::<String>()
                .chars()
                .rev()
                .collect::<String>(),
            output
                .stderr
                .chars()
                .rev()
                .take(6000)
                .collect::<String>()
                .chars()
                .rev()
                .collect::<String>()
        ),
    )?;
    Ok(output.stdout.trim().into())
}
fn git(root: &Path, args: &[&str]) -> Result<String> {
    let mut arguments = vec![
        "git".into(),
        "-c".into(),
        format!(
            "safe.directory={}",
            root.to_string_lossy().replace('\\', "/")
        ),
        "-C".into(),
        root.to_string_lossy().into_owned(),
    ];
    arguments.extend(args.iter().map(|arg| (*arg).to_owned()));
    command(&arguments, &std::env::temp_dir(), &BTreeMap::new())
}
fn source_identity(root: &Path, revision: &str) -> Result<Value> {
    reject_redirects(root)?;
    require(
        root.is_dir() && crate::identity::is_lower_hex(revision, 40),
        "Missing source identity/root.",
    )?;
    require(
        git(root, &["rev-parse", "HEAD"])? == revision
            && git(root, &["status", "--porcelain", "--untracked-files=normal"])?.is_empty(),
        "Source revision differs or checkout dirty.",
    )?;
    Ok(
        json!({"revision":revision,"tree":git(root,&["rev-parse","HEAD^{tree}"])?,"clean":true,"manifest_sha256":digest(&root.join("Cargo.toml"))?,"lock_sha256":digest(&root.join("Cargo.lock"))?,"toolchain_sha256":digest(&root.join("rust-toolchain.toml"))?}),
    )
}
pub(super) fn test_summary(text: &str, names: &[&str]) -> Result<Value> {
    let summaries = text
        .lines()
        .filter(|line| line.starts_with("test result:"))
        .collect::<Vec<_>>();
    require(
        summaries.len() == 1 && !names.is_empty(),
        "Require one complete positive libtest summary.",
    )?;
    let summary = summaries[0];
    let prefix = format!(
        "test result: ok. {} passed; 0 failed; 0 ignored; 0 measured; ",
        names.len()
    );
    let tail = summary
        .strip_prefix(&prefix)
        .ok_or("Libtest summary failed or control count differs.")?;
    let (filtered, elapsed) = tail
        .split_once(" filtered out; finished in ")
        .ok_or("Malformed libtest summary.")?;
    let digits = |text: &str| !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit());
    let seconds = elapsed
        .strip_suffix('s')
        .ok_or("Missing libtest duration unit.")?;
    let valid_seconds = seconds.split_once('.').map_or_else(
        || digits(seconds),
        |(whole, fraction)| digits(whole) && digits(fraction),
    );
    require(
        digits(filtered) && valid_seconds,
        "Malformed libtest summary.",
    )?;
    let executed = text
        .lines()
        .filter_map(|line| {
            line.strip_prefix("test ")
                .and_then(|line| line.strip_suffix(" ... ok"))
        })
        .collect::<BTreeSet<_>>();
    let expected = names.iter().copied().collect::<BTreeSet<_>>();
    require(
        executed == expected
            && text
                .lines()
                .filter(|line| line.starts_with("test ") && line.ends_with(" ... ok"))
                .count()
                == names.len(),
        "Libtest named inventory differs.",
    )?;
    Ok(
        json!({"passed":names.len(),"failed":0,"ignored":0,"measured":0,"names":executed,"summary":summary}),
    )
}
fn lock_packages(path: &Path) -> Result<Vec<Value>> {
    let text = String::from_utf8(bytes(path, 16_777_216)?).map_err(|e| e.to_string())?;
    let mut packages = Vec::new();
    let mut current = None;
    for line in text.lines() {
        if line == "[[package]]" {
            if let Some(package) = current.take() {
                packages.push(package);
            }
            current = Some(json!({}));
        } else if let Some(package) = &mut current {
            for key in ["name", "version", "source", "checksum"] {
                if let Some(value) = line.strip_prefix(&format!("{key} = ")) {
                    let value: Value = serde_json::from_str(value).map_err(|e| e.to_string())?;
                    require(
                        value.is_string() && package.get(key).is_none(),
                        "Unsupported or duplicate lock identity.",
                    )?;
                    package[key] = value;
                } else if line
                    .strip_prefix(key)
                    .is_some_and(|tail| tail.trim_start().starts_with('='))
                {
                    return Err("Unsupported lock identity syntax.".into());
                }
            }
        }
    }
    if let Some(package) = current {
        packages.push(package);
    }
    for package in &packages {
        require(
            package["name"].is_string() && package["version"].is_string(),
            "Incomplete lock package.",
        )?;
    }
    Ok(packages)
}
pub(super) fn lock_subset(driver: &Path, source: &Path, registry: bool) -> Result<()> {
    let sources = lock_packages(source)?;
    for item in lock_packages(driver)? {
        if item["name"] == "prism-cpu-bundle-driver" {
            continue;
        }
        if registry
            && let Some((_, version, hash)) =
                PACKAGES.iter().find(|item_pin| item["name"] == item_pin.0)
        {
            subset(
                &item,
                &json!({"version":version,"checksum":hash,"source":"registry+https://github.com/rust-lang/crates.io-index"}),
            )?;
            if sources.iter().any(|source| {
                source["name"] == item["name"]
                    && source["version"] == item["version"]
                    && source["source"].is_null()
            }) {
                continue;
            }
        }
        require(
            sources.iter().any(|source| {
                ["name", "version", "source", "checksum"]
                    .iter()
                    .all(|key| source[key] == item[key])
            }),
            "Collector resolved dependency outside source lock.",
        )?;
    }
    Ok(())
}
struct Scratch(PathBuf);
impl Scratch {
    fn new(label: &str) -> Result<Self> {
        let path =
            std::env::temp_dir().join(format!("cpu-{label}-{}", crate::startup_process::unique()));
        fs::create_dir(&path).map_err(|e| e.to_string())?;
        Ok(Self(path))
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn write(path: &Path, value: &str) -> Result<()> {
    fs::write(path, value).map_err(|e| e.to_string())
}
fn cargo(verb: &str, manifest: &Path, args: &[&str]) -> Vec<String> {
    let mut command = vec![
        "cargo".into(),
        format!("+{CHANNEL}"),
        verb.into(),
        "--manifest-path".into(),
        manifest.to_string_lossy().into_owned(),
    ];
    command.extend(args.iter().map(|arg| (*arg).into()));
    command
}
fn registry_source(directory: &Path) -> Result<Value> {
    let scratch = Scratch::new("registry")?;
    write(
        &scratch.0.join("lib.rs"),
        "// Registry identity resolution only.\n",
    )?;
    write(
        &scratch.0.join("Cargo.toml"),
        "[workspace]\n[package]\nname=\"prism-cpu-registry-resolver\"\nversion=\"0.0.0\"\nedition=\"2024\"\n[lib]\npath=\"lib.rs\"\n[dependencies]\nprism-inference={version=\"=0.1.25\",default-features=false,features=[\"cli\",\"experimental-neon\",\"parallel-rows\"]}\nbonsai-neon-dot=\"=0.1.2\"\nbonsai-native-hash=\"=0.1.3\"\n",
    )?;
    let metadata = json_bytes(
        command(
            &cargo(
                "metadata",
                &scratch.0.join("Cargo.toml"),
                &["--format-version", "1"],
            ),
            directory,
            &BTreeMap::new(),
        )?
        .as_bytes(),
    )?;
    let locked = lock_packages(&scratch.0.join("Cargo.lock"))?;
    let mut packages = json!({});
    for (name, version, hash) in PACKAGES {
        let locked_matches = locked
            .iter()
            .filter(|p| p["name"] == name && p["version"] == version)
            .collect::<Vec<_>>();
        require(locked_matches.len() == 1, "Ambiguous registry package.")?;
        subset(
            locked_matches[0],
            &json!({"checksum":hash,"source":"registry+https://github.com/rust-lang/crates.io-index"}),
        )?;
        let selected = array(&metadata["packages"])?
            .iter()
            .filter(|p| {
                p["name"] == name
                    && p["version"] == version
                    && p["source"] == locked_matches[0]["source"]
            })
            .collect::<Vec<_>>();
        require(
            selected.len() == 1,
            "Missing unambiguous registry metadata.",
        )?;
        let root = PathBuf::from(
            selected[0]["manifest_path"]
                .as_str()
                .ok_or("Missing registry manifest.")?,
        )
        .parent()
        .ok_or("Missing registry root.")?
        .to_owned();
        archive::verify(&root, hash)?;
        let vcs = read(&root.join(".cargo_vcs_info.json"))?;
        matches(&vcs["git"]["sha1"], &json!(REVISION))?;
        packages[name] = json!({"name":name,"version":version,"sha256":hash,"revision":REVISION,"source":locked_matches[0]["source"],"manifest_sha256":digest(&root.join("Cargo.toml"))?,"root":root});
    }
    let runtime = &packages["prism-inference"];
    let root = PathBuf::from(runtime["root"].as_str().ok_or("Missing runtime root.")?);
    Ok(
        json!({"root":root,"identity":{"kind":"registry","revision":REVISION,"clean":true,"clean_scope":"Cargo-verified immutable registry archive; not a Git checkout","version":"0.1.25","archive_sha256":PACKAGES[0].2,"manifest_sha256":runtime["manifest_sha256"],"lock_sha256":digest(&root.join("Cargo.lock"))?},"packages":packages}),
    )
}
fn manifest(root: &Path, lane: &Path, environment: &BTreeMap<String, String>) -> Result<Value> {
    let manifest = root.join("Cargo.toml");
    let metadata = json_bytes(
        command(
            &cargo(
                "metadata",
                &manifest,
                &["--format-version", "1", "--no-deps"],
            ),
            lane,
            environment,
        )?
        .as_bytes(),
    )?;
    let selected = array(&metadata["packages"])?
        .iter()
        .filter(|p| Path::new(p["manifest_path"].as_str().unwrap_or_default()) == manifest)
        .collect::<Vec<_>>();
    require(selected.len() == 1, "Missing unambiguous source manifest.")?;
    Ok(selected[0].clone())
}
fn write_driver(
    root: &Path,
    tooling: &Path,
    packages: &Value,
    legacy: bool,
    lane: &Path,
    environment: &BTreeMap<String, String>,
) -> Result<PathBuf> {
    let runtime_manifest = manifest(root, lane, environment)?;
    let hash_root = if legacy {
        root.join("tools/native-hash")
    } else {
        PathBuf::from(
            packages["bonsai-native-hash"]["root"]
                .as_str()
                .ok_or("Missing hash helper root.")?,
        )
    };
    let hash_manifest = manifest(&hash_root, lane, environment)?;
    let serde = array(&runtime_manifest["dependencies"])?
        .iter()
        .filter(|value| value["name"] == "serde_json")
        .collect::<Vec<_>>();
    require(serde.len() == 1, "Missing exact JSON dependency.")?;
    let serde_version = format!(
        "={}",
        serde[0]["req"]
            .as_str()
            .ok_or("Missing serde version.")?
            .trim_start_matches(['=', '^', '~'])
    );
    let runtime_dependency = if legacy {
        format!(
            "path={},version=\"={}\",",
            serde_json::to_string(&root.to_string_lossy().replace('\\', "/"))
                .map_err(|e| e.to_string())?,
            runtime_manifest["version"]
                .as_str()
                .ok_or("Missing runtime version.")?
        )
    } else {
        "version=\"=0.1.25\",".into()
    };
    let hash_dependency = if legacy {
        format!(
            "path={},version=\"={}\"",
            serde_json::to_string(&hash_root.to_string_lossy().replace('\\', "/"))
                .map_err(|e| e.to_string())?,
            hash_manifest["version"]
                .as_str()
                .ok_or("Missing hash version.")?
        )
    } else {
        "version=\"=0.1.3\"".into()
    };
    let driver = lane.join("Cargo.toml");
    let driver_path = serde_json::to_string(
        &tooling
            .join("examples/cpu_bundle_check.rs")
            .to_string_lossy()
            .replace('\\', "/"),
    )
    .map_err(|e| e.to_string())?;
    write(
        &driver,
        &format!(
            "[workspace]\nresolver=\"3\"\n[package]\nname=\"prism-cpu-bundle-driver\"\nversion=\"0.0.0\"\nedition=\"2024\"\n[[bin]]\nname=\"cpu-bundle-check\"\npath={driver_path}\n[dependencies]\nbonsai-prism-runtime={{package=\"prism-inference\",{runtime_dependency}default-features=false,features=[\"cli\",\"experimental-neon\",\"parallel-rows\"]}}\nbonsai-native-hash={{{hash_dependency}}}\nserde_json=\"{serde_version}\"\n"
        ),
    )?;
    fs::copy(root.join("Cargo.lock"), lane.join("Cargo.lock")).map_err(|e| e.to_string())?;
    command(
        &cargo("metadata", &driver, &["--offline", "--format-version", "1"]),
        lane,
        environment,
    )?;
    lock_subset(&lane.join("Cargo.lock"), &root.join("Cargo.lock"), !legacy)?;
    Ok(driver)
}
fn build_arm(
    role: &str,
    root: &Path,
    tooling: &Path,
    packages: &Value,
    directory: &Path,
) -> Result<Value> {
    let legacy = role == "baseline";
    let flags = format!(
        "--check-cfg=cfg(cpu_bundle_legacy_f16){}",
        if legacy {
            " --cfg cpu_bundle_legacy_f16"
        } else {
            ""
        }
    );
    let scratch = Scratch::new(role)?;
    let lane = &scratch.0;
    let environment = BTreeMap::from([
        ("RUSTFLAGS".into(), flags.clone()),
        (
            "CARGO_TARGET_DIR".into(),
            lane.join("build").to_string_lossy().into_owned(),
        ),
    ]);
    let toolchain = root.join("rust-toolchain.toml");
    if toolchain.exists() {
        let text = String::from_utf8(bytes(&toolchain, 65_536)?).map_err(|e| e.to_string())?;
        require(
            text.lines()
                .any(|line| line.trim() == format!("channel = \"{CHANNEL}\"")),
            "Unexpected pinned compiler.",
        )?;
    }
    let root_manifest = root.join("Cargo.toml");
    command(
        &cargo("fetch", &root_manifest, &["--locked"]),
        lane,
        &environment,
    )?;
    let driver = write_driver(root, tooling, packages, legacy, lane, &environment)?;
    let commands = arm_commands(&root_manifest, &driver);
    let mut checks = Vec::new();
    for (phase, args) in PHASES.into_iter().zip(commands) {
        let text = command(&args, lane, &environment)?;
        let mut check = json!({"phase":phase,"command":args,"exit_code":0});
        match phase {
            "state_fixture" => check["test_summary"] = test_summary(&text, &[STATE])?,
            "collector_controls" => check["test_summary"] = test_summary(&text, &CONTROLS)?,
            "source_analysis" => {
                let mut helpers = Vec::new();
                for package in ["bonsai-neon-dot", "bonsai-native-hash"] {
                    let helper_root = if legacy {
                        root.join(if package == "bonsai-neon-dot" {
                            "tools/neon-dot"
                        } else {
                            "tools/native-hash"
                        })
                    } else {
                        PathBuf::from(
                            packages[package]["root"]
                                .as_str()
                                .ok_or("Missing helper source.")?,
                        )
                    };
                    let manifest = helper_root.join("Cargo.toml");
                    let lock = if legacy {
                        root.join("Cargo.lock")
                    } else {
                        helper_root.join("Cargo.lock")
                    };
                    command(
                        &cargo("fetch", &manifest, &["--locked"]),
                        lane,
                        &environment,
                    )?;
                    let command_args = cargo(
                        "clippy",
                        &manifest,
                        &[
                            "--locked",
                            "--offline",
                            "--release",
                            "--all-targets",
                            "--",
                            "-D",
                            "warnings",
                        ],
                    );
                    command(&command_args, lane, &environment)?;
                    helpers.push(json!({"package":package,"command":command_args,"exit_code":0,"manifest_sha256":digest(&manifest)?,"lock_sha256":digest(&lock)?}));
                }
                check["helper_checks"] = helpers.into();
            }
            _ => {}
        }
        checks.push(check);
    }
    let executable = directory.join(format!("{role}.exe"));
    fs::copy(lane.join("build/release/cpu-bundle-check.exe"), &executable)
        .map_err(|e| e.to_string())?;
    Ok(
        json!({"legacy_f16_build":legacy,"executable":{"path":format!("{role}.exe"),"bytes":fs::metadata(&executable).map_err(|e|e.to_string())?.len(),"sha256":digest(&executable)?},"driver_manifest_sha256":digest(&driver)?,"driver_lock_sha256":digest(&lane.join("Cargo.lock"))?,"rustflags":flags,"checks":checks}),
    )
}
fn sources(root: &Path) -> Result<Value> {
    let mut names = vec!["examples/cpu_bundle_check.rs".to_owned()];
    let mut pending = vec![root.join("examples/cpu_bundle_check")];
    while let Some(directory) = pending.pop() {
        reject_redirects(&directory)?;
        for entry in fs::read_dir(directory).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            reject_redirects(&path)?;
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|value| value == "rs") {
                names.push(
                    path.strip_prefix(root)
                        .map_err(|e| e.to_string())?
                        .to_str()
                        .ok_or("Non-Unicode collector source.")?
                        .replace('\\', "/"),
                );
            }
            require(
                names.len() + pending.len() <= 256,
                "Excessive collector source inventory.",
            )?;
        }
    }
    names.sort();
    let mut sources = json!({});
    for name in names {
        sources[&name] = digest(&root.join(&name))?.into();
    }
    require(object(&sources)?.len() >= 2, "Missing collector controls.")?;
    Ok(sources)
}
#[expect(
    clippy::too_many_lines,
    reason = "A single source-bound hosted preparation rechecks all immutable inputs before publishing its complete schema-2 packet."
)]
pub(super) fn run(request: &Value) -> Result<Value> {
    require(
        cfg!(all(windows, target_arch = "aarch64")),
        "Preparation requires native Windows ARM64.",
    )?;
    for (name, value) in [
        ("GITHUB_ACTIONS", "true"),
        ("GITHUB_REF", "refs/heads/main"),
        ("RUNNER_ENVIRONMENT", "github-hosted"),
        ("RUNNER_ARCH", "ARM64"),
    ] {
        require(
            variable(name) == value,
            "Preparation requires owning hosted ARM64 main CI.",
        )?;
    }
    require(
        [
            "symbiogenesis/prism-inference",
            "symbiogenesis/SwiftBonsai2Runner",
        ]
        .contains(&variable("GITHUB_REPOSITORY").as_str()),
        "Preparation requires owning repository.",
    )?;
    let revision = hex(&request["runnerRevision"], 40)?;
    require(
        variable("GITHUB_SHA") == revision
            && !variable("GITHUB_RUN_ID").is_empty()
            && variable("GITHUB_RUN_ID")
                .bytes()
                .all(|b| b.is_ascii_digit()),
        "Missing hosted source/run identity.",
    )?;
    matches(&request["candidateRegistryVersion"], &json!("0.1.25"))?;
    let baseline = path(request, "baselineRoot")?;
    let tooling = path(request, "toolingRoot")?;
    let runner = path(request, "runnerRoot")?;
    let output = path(request, "output")?;
    bonsai_artifacts::validate_external_path(&output).map_err(|error| error.to_string())?;
    require(
        !fs::canonicalize(&baseline)
            .map_err(|error| error.to_string())?
            .to_string_lossy()
            .eq_ignore_ascii_case(
                &fs::canonicalize(&tooling)
                    .map_err(|error| error.to_string())?
                    .to_string_lossy(),
            ),
        "Use independent source checkouts.",
    )?;
    require(
        !output.exists() && !output.starts_with(&baseline) && !output.starts_with(&tooling),
        "Use a new external artifact directory.",
    )?;
    let runner_identity = source_identity(&runner, revision)?;
    let tooling_revision = git(&tooling, &["rev-parse", "HEAD"])?;
    let tooling_identity = source_identity(&tooling, &tooling_revision)?;
    let baseline_identity = source_identity(&baseline, BASELINE)?;
    let evaluator = runner.join("tools/cpu-bundle/cpu_bundle.ps1");
    let evaluator_hash = digest(&evaluator)?;
    let collector_sources = sources(&tooling)?;
    fs::create_dir_all(&output).map_err(|e| e.to_string())?;
    let registry = registry_source(&output)?;
    let packages = &registry["packages"];
    let messages_raw = serde_json::to_vec(&messages()).map_err(|e| e.to_string())?;
    fs::write(output.join("messages.json"), &messages_raw).map_err(|e| e.to_string())?;
    let compiler = command(
        &["rustc".into(), format!("+{CHANNEL}"), "-vV".into()],
        &output,
        &BTreeMap::new(),
    )?;
    require(
        compiler.contains(&format!("host: {TARGET}")),
        "Compiler not native Windows ARM64.",
    )?;
    let mut arms = json!({});
    for role in ["baseline", "candidate"] {
        let root = if role == "baseline" {
            baseline.clone()
        } else {
            PathBuf::from(registry["root"].as_str().ok_or("Missing registry root.")?)
        };
        let mut arm = build_arm(role, &root, &tooling, packages, &output)?;
        arm["source"] = if role == "baseline" {
            baseline_identity.clone()
        } else {
            registry["identity"].clone()
        };
        arms[role] = arm;
        if role == "baseline" {
            matches(&source_identity(&baseline, BASELINE)?, &baseline_identity)?;
        } else {
            for (_, value) in object(packages)? {
                archive::verify(
                    &PathBuf::from(value["root"].as_str().ok_or("Missing package root.")?),
                    value["sha256"].as_str().ok_or("Missing package digest.")?,
                )?;
            }
        }
    }
    matches(
        &source_identity(&tooling, &tooling_revision)?,
        &tooling_identity,
    )?;
    matches(&sources(&tooling)?, &collector_sources)?;
    matches(&source_identity(&runner, revision)?, &runner_identity)?;
    require(
        digest(&evaluator)? == evaluator_hash,
        "Runner evaluator changed during preparation.",
    )?;
    fs::copy(evaluator, output.join("evaluator.ps1")).map_err(|e| e.to_string())?;
    let mut registry_identities = packages.clone();
    for (_, value) in registry_identities
        .as_object_mut()
        .ok_or("Missing registry inventory.")?
    {
        value
            .as_object_mut()
            .ok_or("Missing package object.")?
            .remove("root");
    }
    let native = std::env::current_exe().map_err(|error| error.to_string())?;
    fs::copy(&native, output.join("native-evaluator.exe")).map_err(|error| error.to_string())?;
    let native_identity = json!({"path":"native-evaluator.exe","bytes":fs::metadata(&native).map_err(|error|error.to_string())?.len(),"sha256":digest(&native)?,"source_revision":revision,"language":"Rust","format":"bonsai-tools-cpu-bundle-v1"});
    let result = json!({"schema_version":2,"kind":"prism-cpu-bundle-preparation","compiler":compiler,"orchestrator":{"repository":variable("GITHUB_REPOSITORY"),"revision":revision,"run_id":variable("GITHUB_RUN_ID"),"run_attempt":variable("GITHUB_RUN_ATTEMPT"),"source":runner_identity},"evaluator_sha256":evaluator_hash,"evaluator_language":"PowerShell 7.6","tooling_source":tooling_identity,"registry_packages":registry_identities,"retained_files":["preparation.json","messages.json","evaluator.ps1","native-evaluator.exe"],"native_evaluator":native_identity,"target":TARGET,"features":["cli","experimental-neon","parallel-rows"],"collector_sources":collector_sources,"messages":{"path":"messages.json","sha256":crate::identity::sha256(&messages_raw)},"output_tokens":32,"model_sha256":MODEL,"model_bytes":7_206_168_928u64,"adapter_sha256":ADAPTER,"adapter_bytes":9_682_464,"per_arm_timeout_seconds":60,"schedule":SCHEDULE,"profile":profile(),"arms":arms,"minimum_relative_request_improvement":0.10,"maximum_owned_arm_regression":0.10,"model_executed":false,"promotion_qualified":false});
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output.join("preparation.json"))
        .map_err(|e| e.to_string())?;
    file.write_all(&serde_json::to_vec_pretty(&result).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    validate_preparation(&result, &output.join("preparation.json"))?;
    Ok(result)
}
