//! Contributor source executable builds after a native compiler bootstrap.

use super::{
    Path, PathBuf, Result, Value, cleanup, completed, fs, json, native, require, resolve, version,
};
use serde::Deserialize;
use std::{collections::BTreeMap, fmt::Write as _};

#[derive(Deserialize)]
struct Workspace {
    workspace: WorkspaceTable,
}
#[derive(Deserialize)]
struct WorkspaceTable {
    package: PackageTable,
}
#[derive(Deserialize)]
struct PackageTable {
    version: String,
}

struct Plan {
    repository: PathBuf,
    data: PathBuf,
    target: PathBuf,
    shared_target: Option<PathBuf>,
    output: PathBuf,
    version: String,
    channel: String,
    revision: String,
}

fn plan(
    repository: &Path,
    data: &Path,
    target: Option<&Path>,
    output: Option<&Path>,
    revision: &str,
) -> Result<Plan> {
    let repository = resolve(repository, true, false)?;
    let data = resolve(data, true, true)?;
    let manifest = String::from_utf8(crate::release_tool::bytes(
        &repository.join("Cargo.toml"),
        2_097_152,
    )?)
    .map_err(|error| error.to_string())?;
    let workspace: Workspace = toml::from_str(&manifest).map_err(|error| error.to_string())?;
    let version = version(
        &format!("v{}", workspace.workspace.package.version),
        revision,
    )?;
    let channel = crate::source_build::channel(&repository)?;
    let output = resolve(
        &output.map_or_else(
            || data.join("build/distribution/SwiftBonsai2Runner.exe"),
            Path::to_owned,
        ),
        false,
        true,
    )?;
    require(
        output
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
            && !output.exists(),
        "Bundle output must be a new .exe path.",
    )?;
    let shared_target = target.map(|path| resolve(path, true, true)).transpose()?;
    let target = shared_target
        .clone()
        .unwrap_or_else(|| data.join("build/maintenance"));
    Ok(Plan {
        repository,
        data,
        target,
        shared_target,
        output,
        version,
        channel,
        revision: revision.into(),
    })
}
fn invoke(
    executable: &str,
    arguments: &Value,
    seconds: u64,
    build: bool,
    environment: &Value,
) -> Result<String> {
    let result = crate::release_tool::run(
        &json!({"operation":"invoke","filePath":executable,"arguments":arguments,"timeoutSeconds":seconds,"buildCommand":build,"environment":environment}),
    )?;
    Ok(result["stdout"]
        .as_str()
        .ok_or("Missing native command output.")?
        .into())
}
fn helper(command: &str, request: &Value) -> Result<Value> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let stdout = invoke(
        executable.to_str().ok_or("Non-Unicode maintenance path.")?,
        &json!([
            command,
            "--json",
            serde_json::to_string(request).map_err(|error| error.to_string())?
        ]),
        600,
        true,
        &json!({}),
    )?;
    serde_json::from_str(&stdout).map_err(|error| error.to_string())
}
fn execute(operation: &str, request: Value) -> Result<Value> {
    match operation {
        "launcher" => {
            invoke(
                "cargo.exe",
                &request["arguments"],
                600,
                true,
                &request["environment"],
            )?;
            Ok(Value::Null)
        }
        "chat" => helper("source-build", &request),
        "speech" => helper("speech-build", &request),
        "notices" => notices(
            &PathBuf::from(
                request["repositoryRoot"]
                    .as_str()
                    .ok_or("Missing source root.")?,
            ),
            &PathBuf::from(
                request["outputPath"]
                    .as_str()
                    .ok_or("Missing notice output.")?,
            ),
        ),
        "package" => crate::chat_package::run(request),
        "bundle" => crate::chat_package::bundle::run(request),
        _ => Err("Unknown executable build stage.".into()),
    }
}

fn build_with(plan: &Plan, mut execute: impl FnMut(&str, Value) -> Result<Value>) -> Result<Value> {
    let stage = std::env::temp_dir().join(format!(
        "SwiftBonsai-Executable-{}",
        crate::startup_process::unique()
    ));
    fs::create_dir(&stage).map_err(|error| error.to_string())?;
    let result = (|| {
        fs::create_dir_all(
            plan.output
                .parent()
                .ok_or("Missing bundle output parent.")?,
        )
        .map_err(|error| error.to_string())?;
        let arguments = json!([
            format!("+{}", plan.channel),
            "build",
            "--locked",
            "--release",
            "--manifest-path",
            plan.repository.join("Cargo.toml"),
            "--package",
            "bonsai-tools",
            "--bin",
            "bonsai-launcher",
            "--target",
            "aarch64-pc-windows-msvc",
            "--target-dir",
            plan.target
        ]);
        execute(
            "launcher",
            json!({"arguments":arguments,"environment":{"CARGO_BUILD_FINGERPRINT":"content","CARGO_UNSTABLE_CHECKSUM_FRESHNESS":"true","CARGO_ENCODED_RUSTFLAGS":"-C\u{1f}target-feature=+crt-static"}}),
        )?;
        let launcher = resolve(
            &plan
                .target
                .join("aarch64-pc-windows-msvc/release/bonsai-launcher.exe"),
            false,
            false,
        )?;
        let chat = execute(
            "chat",
            json!({"operation":"chat","repositoryRoot":plan.repository,"workingDirectory":stage,"dataDirectory":plan.data,"packages":["bonsai-chat","bonsai-broker"],"targetDirectory":plan.shared_target}),
        )?;
        let speech = execute(
            "speech",
            json!({"operation":"build","repositoryRoot":plan.repository,"workingDirectory":stage,"dataDirectory":plan.data,"model":"All","targetDirectory":plan.shared_target}),
        )?;
        let notices = stage.join("THIRD-PARTY-NOTICES.txt");
        execute(
            "notices",
            json!({"repositoryRoot":plan.repository,"outputPath":notices}),
        )?;
        let package = stage.join("package");
        let receipt = execute(
            "package",
            json!({"operation":"create","outputDirectory":package,"version":plan.version,"sourceRevision":plan.revision,"chatPath":chat["ChatPath"],"brokerPath":chat["BrokerPath"],"asrPath":speech["AsrPath"],"ttsPath":speech["TtsPath"],"toolPath":launcher,"thirdPartyNoticePath":notices}),
        )?;
        execute(
            "bundle",
            json!({"operation":"create","packageDirectory":package,"manifestSha256":receipt["ManifestSha256"],"outputPath":plan.output}),
        )
    })();
    completed(result, cleanup(&stage))
}

pub(super) fn build(
    repository: &Path,
    data: &Path,
    target: Option<&Path>,
    output: Option<&Path>,
) -> Result<Value> {
    native()?;
    resolve(repository, true, false)?;
    let revision = invoke(
        "git.exe",
        &json!(["-C", repository, "rev-parse", "HEAD"]),
        180,
        false,
        &json!({}),
    )?;
    build_with(
        &plan(repository, data, target, output, revision.trim())?,
        execute,
    )
}

fn github(arguments: &Value) -> Result<Value> {
    serde_json::from_str(&invoke("gh.exe", arguments, 180, false, &json!({}))?)
        .map_err(|error| error.to_string())
}
fn upstream(section: &Value, cache: &mut BTreeMap<String, String>) -> Result<String> {
    let identity = crate::release_tool::run(
        &json!({"operation":"notice-provenance","directory":section["directory"],"repository":section["repository"],"packageName":section["packageName"],"packageVersion":section["packageVersion"]}),
    )?;
    let upstream = identity["upstream"]
        .as_str()
        .ok_or("Missing notice upstream.")?;
    let revision = identity["revision"]
        .as_str()
        .ok_or("Missing notice revision.")?;
    let key = format!("{upstream}@{revision}");
    if let Some(cached) = cache.get(&key) {
        return Ok(cached.clone());
    }
    let tree = github(&json!([
        "api",
        format!("repos/{upstream}/git/trees/{revision}")
    ]))?;
    let files = crate::release_tool::run(&json!({"operation":"notice-tree","response":tree}))?;
    let mut result = String::new();
    if identity["recordedRevision"] != revision {
        writeln!(result,"Archive source commit: {}. Shared license material: reviewed 0.37.0 release commit {revision}.",identity["recordedRevision"].as_str().ok_or("Missing archive revision.")?).map_err(|error|error.to_string())?;
    }
    for file in files.as_array().ok_or("Missing selected notice files.")? {
        let sha = file["sha"].as_str().ok_or("Missing notice blob.")?;
        let blob = github(&json!(["api", format!("repos/{upstream}/git/blobs/{sha}")]))?;
        let notice = crate::release_tool::run(
            &json!({"operation":"notice-blob","response":blob,"file":file}),
        )?;
        writeln!(
            result,
            "\n--- https://github.com/{upstream}/blob/{revision}/{} sha256={} ---\n{}\n",
            file["path"].as_str().ok_or("Missing notice path.")?,
            notice["sha256"].as_str().ok_or("Missing notice digest.")?,
            notice["text"].as_str().ok_or("Missing notice text.")?
        )
        .map_err(|error| error.to_string())?;
    }
    cache.insert(key, result.clone());
    Ok(result)
}
pub(super) fn notices(repository: &Path, output: &Path) -> Result<Value> {
    let repository = resolve(repository, true, false)?;
    resolve(output, false, true)?;
    let channel = crate::source_build::channel(&repository)?;
    let metadata: Value = serde_json::from_str(&invoke(
        "cargo.exe",
        &json!([
            format!("+{channel}"),
            "metadata",
            "--locked",
            "--format-version",
            "1",
            "--filter-platform",
            "aarch64-pc-windows-msvc",
            "--manifest-path",
            repository.join("Cargo.toml")
        ]),
        180,
        true,
        &json!({}),
    )?)
    .map_err(|error| error.to_string())?;
    let sections = crate::release_tool::run(
        &json!({"operation":"notice-sources","metadata":metadata,"repositoryRoot":repository}),
    )?;
    let mut text = String::from(
        "Swift Bonsai 2 Runner - third-party copyright and license material\nGenerated from the locked Windows ARM64 dependency sources. Includes build/development dependencies conservatively; inclusion does not imply runtime use.\nUbuntu Light font: Copyright 2011 Canonical Ltd.\n",
    );
    let mut cache = BTreeMap::new();
    for section in sections.as_array().ok_or("Missing notice sections.")? {
        writeln!(
            text,
            "\n===== {} =====\n",
            section["label"].as_str().ok_or("Missing notice label.")?
        )
        .map_err(|error| error.to_string())?;
        let content = if section["upstreamRequired"] == true {
            upstream(section, &mut cache)?
        } else {
            section["text"]
                .as_str()
                .ok_or("Missing notice text.")?
                .into()
        };
        text.push_str(&content);
        text.push('\n');
    }
    fs::write(output, text).map_err(|error| error.to_string())?;
    Ok(json!({"NoticePath":output}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_build_derives_workspace_version_and_preserves_cache_selection_once() -> Result<()> {
        for mode in ["default", "environment", "explicit"] {
            let root = std::env::temp_dir().join(format!(
                "Bonsai-ExecutableFixture-{}",
                crate::startup_process::unique()
            ));
            fs::create_dir(&root).map_err(|error| error.to_string())?;
            let result = (|| {
                fs::write(
                    root.join("Cargo.toml"),
                    "[workspace.package]\nversion=\"0.2.2\"\n",
                )
                .and_then(|()| fs::write(root.join("Cargo.lock"), "version = 4\n"))
                .and_then(|()| {
                    fs::write(
                        root.join("rust-toolchain.toml"),
                        "[toolchain]\nchannel = \"nightly-2026-11-12\"\n",
                    )
                })
                .map_err(|error| error.to_string())?;
                let data = root.join(mode);
                let selected = (mode != "default").then(|| root.join(format!("{mode} cache")));
                let plan = plan(&root, &data, selected.as_deref(), None, &"a".repeat(40))?;
                require(
                    plan.version == "0.2.2",
                    "Workspace version was replaced by helper version.",
                )?;
                let mut calls = Vec::new();
                build_with(&plan, |operation, request| {
                    calls.push((operation.to_owned(), request.clone()));
                    match operation {
                        "launcher" => {
                            let launcher = plan
                                .target
                                .join("aarch64-pc-windows-msvc/release/bonsai-launcher.exe");
                            fs::create_dir_all(launcher.parent().ok_or("Missing fixture parent.")?)
                                .and_then(|()| fs::write(launcher, b"inert launcher"))
                                .map_err(|error| error.to_string())?;
                            Ok(Value::Null)
                        }
                        "chat" => Ok(json!({"ChatPath":"chat","BrokerPath":"broker"})),
                        "speech" => Ok(json!({"AsrPath":"asr","TtsPath":"tts"})),
                        "notices" => Ok(Value::Null),
                        "package" => Ok(json!({"ManifestSha256":"b".repeat(64)})),
                        "bundle" => {
                            fs::write(&plan.output, b"inert bundle")
                                .map_err(|error| error.to_string())?;
                            Ok(Value::Null)
                        }
                        _ => Err("Unexpected fixture stage.".into()),
                    }
                })?;
                require(
                    calls
                        .iter()
                        .map(|(name, _)| name.as_str())
                        .collect::<Vec<_>>()
                        == ["launcher", "chat", "speech", "notices", "package", "bundle"],
                    "Build duplicated or skipped preparation.",
                )?;
                let expected = selected
                    .clone()
                    .unwrap_or_else(|| data.join("build/maintenance"));
                require(plan.target == expected, "Target cache selection changed.")?;
                for index in [1, 2] {
                    require(
                        calls[index].1["targetDirectory"] == json!(selected),
                        "Shared cache override was not forwarded.",
                    )?;
                }
                require(
                    calls[0].1["environment"]["CARGO_ENCODED_RUSTFLAGS"]
                        == "-C\u{1f}target-feature=+crt-static"
                        && calls[4].1["version"] == "0.2.2"
                        && plan.output.is_file(),
                    "Source build CRT/version/output changed.",
                )
            })();
            completed(result, cleanup(&root))?;
        }
        Ok(())
    }
}
