//! Native Cargo selection, feature policy and evidence-preserving checks.

use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::Result;

#[cfg(windows)]
mod execute;
#[cfg(test)]
mod tests;

const USAGE: &str = "bonsai-ci rust-select [--root PATH]\n       bonsai-ci rust-plan|rust-check [--root PATH] [--package NAME] [--packages-json JSON] [--group All|Broker] [--profile Foundation|Compatibility|Native] [--phase fmt|clippy|doc|test|build] [--toolchain NAME] [--offline] [--full-tests] [--build] [--test-case NAME --test-kind lib|bin|test --test-target NAME] [--target PATH --evidence PATH --timeout-seconds N] [--github-release]";
const PHASES: &[&str] = &["fmt", "clippy", "doc", "test", "build"];
const NATIVE_TARGET: &str = "aarch64-pc-windows-msvc";
const TEST_SUFFIX: &[&str] = &[
    "--no-fail-fast",
    "--",
    "-Z",
    "unstable-options",
    "--report-time",
];

#[derive(Default)]
struct FocusedTests {
    cases: Vec<String>,
    kind: Option<String>,
    target: Option<String>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
struct Package {
    name: String,
    manifest: String,
    group: String,
    features: Vec<String>,
    compatibility_compile_features: Option<Vec<String>>,
    native_features: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
struct Registry {
    packages: Vec<Package>,
}

struct Options {
    command: String,
    root: PathBuf,
    packages: Vec<String>,
    group: String,
    profile: String,
    phases: Vec<String>,
    toolchain: Option<String>,
    offline: bool,
    target: Option<PathBuf>,
    evidence: Option<PathBuf>,
    timeout_seconds: u64,
    github_release: bool,
    focused: FocusedTests,
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct Check {
    name: String,
    phase: String,
    packages: Vec<String>,
    arguments: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    test_case: Option<String>,
}

fn parse(arguments: &[OsString]) -> Result<Options> {
    let mut values = arguments.iter();
    let mut options = Options {
        command: values
            .next()
            .and_then(|value| value.to_str())
            .ok_or(USAGE)?
            .into(),
        root: std::env::var_os("GITHUB_WORKSPACE")
            .map_or_else(std::env::current_dir, |path| Ok(path.into()))
            .map_err(|error| error.to_string())?,
        packages: Vec::new(),
        group: "All".into(),
        profile: "Foundation".into(),
        phases: Vec::new(),
        toolchain: None,
        offline: false,
        target: None,
        evidence: None,
        timeout_seconds: 900,
        github_release: false,
        focused: FocusedTests::default(),
    };
    let mut extra_phases = Vec::new();
    let mut full_tests = false;
    while let Some(option) = values.next() {
        let option = option.to_str().ok_or(USAGE)?;
        if options.command == "rust-select" && option != "--root" {
            return Err(USAGE.into());
        }
        match option {
            "--offline" => options.offline = true,
            "--full-tests" => {
                full_tests = true;
                extra_phases.push("test".into());
            }
            "--build" => extra_phases.push("build".into()),
            "--github-release" => options.github_release = true,
            _ => {
                let value = values.next().ok_or(USAGE)?;
                parse_value(&mut options, option, value)?;
            }
        }
    }
    if options.phases.is_empty() {
        options.phases = if options.focused.cases.is_empty() {
            ["fmt", "clippy", "doc"].map(String::from).to_vec()
        } else {
            vec!["test".into()]
        };
    }
    options.phases.extend(extra_phases);
    let mut seen = BTreeSet::new();
    options.phases.retain(|phase| seen.insert(phase.clone()));
    validate_focused_tests(&options, full_tests)?;
    if !["All", "Broker"].contains(&options.group.as_str())
        || !["Foundation", "Compatibility", "Native"].contains(&options.profile.as_str())
        || options
            .phases
            .iter()
            .any(|phase| !PHASES.contains(&phase.as_str()))
        || !(1..=3600).contains(&options.timeout_seconds)
    {
        return Err(USAGE.into());
    }
    options.root = std::path::absolute(&options.root).map_err(|error| error.to_string())?;
    Ok(options)
}

fn parse_value(options: &mut Options, option: &str, value: &OsStr) -> Result<()> {
    match option {
        "--root" => options.root = value.into(),
        "--target" => options.target = Some(value.into()),
        "--evidence" => options.evidence = Some(value.into()),
        "--package" => options.packages.push(value.to_str().ok_or(USAGE)?.into()),
        "--packages-json" => {
            let selected = serde_json::from_str::<Vec<String>>(value.to_str().ok_or(USAGE)?)
                .map_err(|error| error.to_string())?;
            if selected.is_empty() {
                return Err("An explicit Rust package selection must not be empty.".into());
            }
            options.packages.extend(selected);
        }
        "--phase" => options.phases.push(value.to_str().ok_or(USAGE)?.into()),
        "--group" => options.group = value.to_str().ok_or(USAGE)?.into(),
        "--profile" => options.profile = value.to_str().ok_or(USAGE)?.into(),
        "--toolchain" => options.toolchain = Some(value.to_str().ok_or(USAGE)?.into()),
        "--test-case" => options
            .focused
            .cases
            .push(value.to_str().ok_or(USAGE)?.into()),
        "--test-kind" => options.focused.kind = Some(value.to_str().ok_or(USAGE)?.into()),
        "--test-target" => options.focused.target = Some(value.to_str().ok_or(USAGE)?.into()),
        "--timeout-seconds" => {
            options.timeout_seconds = value.to_str().ok_or(USAGE)?.parse().map_err(|_| USAGE)?;
        }
        _ => return Err(USAGE.into()),
    }
    Ok(())
}

fn validate_focused_tests(options: &Options, full_tests: bool) -> Result<()> {
    let focused = &options.focused;
    if focused.cases.is_empty() {
        return if focused.kind.is_none() && focused.target.is_none() {
            Ok(())
        } else {
            Err("Focused target options require at least one exact --test-case.".into())
        };
    }
    let mut unique = BTreeSet::new();
    if full_tests
        || options.packages.len() != 1
        || !options.phases.iter().any(|phase| phase == "test")
        || focused.cases.len() > 16
        || focused.cases.iter().any(|case| {
            case.len() > 1024
                || !case.split("::").all(|part| {
                    !part.is_empty()
                        && (part.as_bytes()[0].is_ascii_alphabetic() || part.starts_with('_'))
                        && part
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                })
                || !unique.insert(case)
        })
    {
        return Err("Focused checks require one explicit package and 1..16 unique exact Rust test names, without --full-tests.".into());
    }
    match (focused.kind.as_deref().unwrap_or("lib"), &focused.target) {
        ("lib", None) => Ok(()),
        ("bin" | "test", Some(target)) if identifier(target) => Ok(()),
        _ => Err("Use lib without --test-target, or bin/test with one named --test-target.".into()),
    }
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
        && value.as_bytes()[0].is_ascii_alphanumeric()
}

fn registry(root: &Path) -> Result<Vec<Package>> {
    let registry: Registry = serde_json::from_slice(
        &fs::read(root.join("config/rust-packages.json")).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let mut names = BTreeSet::new();
    for package in &registry.packages {
        if !identifier(&package.name)
            || !names.insert(&package.name)
            || package.group != "Broker"
            || Path::new(&package.manifest).is_absolute()
            || package
                .manifest
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
            || package.manifest.contains('\\')
        {
            return Err("Invalid Rust package registry.".into());
        }
        let manifest: toml::Value = toml::from_str(
            &fs::read_to_string(root.join(&package.manifest)).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        if manifest
            .get("package")
            .and_then(|value| value.get("name"))
            .and_then(toml::Value::as_str)
            != Some(package.name.as_str())
        {
            return Err("Rust registry package differs from its manifest.".into());
        }
        for features in [
            &package.features,
            package
                .compatibility_compile_features
                .as_ref()
                .unwrap_or(&package.features),
            package
                .native_features
                .as_ref()
                .unwrap_or(&package.features),
        ] {
            if (features.iter().any(|feature| feature == "*") && features.len() != 1)
                || features
                    .iter()
                    .any(|feature| feature != "*" && !identifier(feature))
            {
                return Err("Invalid Rust feature policy.".into());
            }
        }
    }
    if registry.packages.is_empty() {
        return Err("The Rust package registry is empty.".into());
    }
    Ok(registry.packages)
}

fn select<'a>(packages: &'a [Package], names: &[String], group: &str) -> Result<Vec<&'a Package>> {
    let selected: Vec<_> = packages
        .iter()
        .filter(|package| group == "All" || package.group == group)
        .collect();
    for name in names {
        if !selected.iter().any(|package| package.name == *name) {
            return Err(format!("Unknown Rust package in group {group}: {name}"));
        }
    }
    Ok(selected
        .into_iter()
        .filter(|package| names.is_empty() || names.contains(&package.name))
        .collect())
}

fn features<'a>(package: &'a Package, phase: &str, profile: &str) -> &'a [String] {
    if profile == "Native" {
        package
            .native_features
            .as_deref()
            .unwrap_or(&package.features)
    } else if profile == "Compatibility" && phase != "test" {
        package
            .compatibility_compile_features
            .as_deref()
            .unwrap_or(&package.features)
    } else {
        &package.features
    }
}

fn all_features(features: &[String]) -> bool {
    features.len() == 1 && features[0] == "*"
}

fn arguments(
    packages: &[&Package],
    phase: &str,
    profile: &str,
    toolchain: &str,
    manifest: &str,
    offline: bool,
) -> Result<Vec<String>> {
    let first = packages
        .first()
        .ok_or("Select at least one registered package.")?;
    let mut arguments = vec![
        format!("+{toolchain}"),
        phase.into(),
        "--manifest-path".into(),
        manifest.into(),
    ];
    for package in packages {
        arguments.extend(["--package".into(), package.name.clone()]);
    }
    if phase == "fmt" {
        arguments.push("--check".into());
        return Ok(arguments);
    }
    if !toolchain.ends_with(&format!("-{NATIVE_TARGET}")) {
        return Err("Native Rust checks require a Windows ARM64 compiler host.".into());
    }
    if packages.len() > 1
        && packages
            .iter()
            .any(|package| !all_features(features(package, phase, profile)))
    {
        return Err("Only all-feature packages can share a Cargo compile command.".into());
    }
    // Checks execute outside the checkout. Load its target policy explicitly;
    // --manifest-path alone does not discover the workspace's Cargo config.
    let root = Path::new(manifest)
        .parent()
        .ok_or("Missing manifest parent.")?;
    arguments.extend([
        "--config".into(),
        root.join(".cargo/config.toml")
            .to_string_lossy()
            .into_owned(),
        // An explicit target keeps static CRT flags off host proc-macro builds.
        "--target".into(),
        NATIVE_TARGET.into(),
    ]);
    arguments.extend(["--locked".into(), "--release".into()]);
    if offline {
        arguments.push("--offline".into());
    }
    let selected_features = features(first, phase, profile);
    if all_features(selected_features) {
        arguments.push("--all-features".into());
    } else if !selected_features.is_empty() {
        arguments.extend(["--features".into(), selected_features.join(",")]);
    }
    let suffix: &[&str] = match phase {
        "clippy" => &["--all-targets", "--", "-D", "warnings"],
        "doc" => &[
            "--no-deps",
            "-Z",
            "unstable-options",
            "--output-format",
            "json",
        ],
        "test" => TEST_SUFFIX,
        _ => &[],
    };
    arguments.extend(suffix.iter().map(|value| (*value).into()));
    Ok(arguments)
}

fn plan(options: &Options, packages: &[&Package], toolchain: &str) -> Result<Vec<Check>> {
    let manifest = options.root.join("Cargo.toml");
    let manifest = manifest
        .to_str()
        .ok_or("The workspace path must be UTF-8.")?;
    let mut plan = Vec::new();
    for phase in &options.phases {
        if phase == "test" && !options.focused.cases.is_empty() {
            plan.extend(focused_plan(options, packages, toolchain, manifest)?);
            continue;
        }
        let batches = if phase == "fmt" {
            vec![packages.to_vec()]
        } else {
            let (all, restricted): (Vec<_>, Vec<_>) = packages
                .iter()
                .copied()
                .partition(|package| all_features(features(package, phase, &options.profile)));
            let mut batches = Vec::new();
            if !all.is_empty() {
                batches.push(all);
            }
            batches.extend(restricted.into_iter().map(|package| vec![package]));
            batches
        };
        for batch in batches {
            plan.push(Check {
                name: format!(
                    "{}-{phase}",
                    if batch.len() == 1 {
                        batch[0].name.as_str()
                    } else {
                        "packages"
                    }
                ),
                phase: phase.clone(),
                packages: batch.iter().map(|package| package.name.clone()).collect(),
                arguments: arguments(
                    &batch,
                    phase,
                    &options.profile,
                    toolchain,
                    manifest,
                    options.offline,
                )?,
                test_case: None,
            });
        }
    }
    Ok(plan)
}

fn focused_plan(
    options: &Options,
    packages: &[&Package],
    toolchain: &str,
    manifest: &str,
) -> Result<Vec<Check>> {
    if packages.len() != 1 {
        return Err("Focused checks require one selected package.".into());
    }
    options
        .focused
        .cases
        .iter()
        .enumerate()
        .map(|(index, case)| {
            let mut arguments = arguments(
                packages,
                "test",
                &options.profile,
                toolchain,
                manifest,
                options.offline,
            )?;
            arguments.truncate(arguments.len() - TEST_SUFFIX.len());
            match options.focused.kind.as_deref().unwrap_or("lib") {
                "lib" => arguments.push("--lib".into()),
                kind => arguments.extend([
                    format!("--{kind}"),
                    options
                        .focused
                        .target
                        .clone()
                        .ok_or("Missing focused test target.")?,
                ]),
            }
            arguments.extend([
                case.clone(),
                "--".into(),
                "--exact".into(),
                "-Z".into(),
                "unstable-options".into(),
                "--report-time".into(),
            ]);
            Ok(Check {
                name: format!("{}-test-{}", packages[0].name, index + 1),
                phase: "test".into(),
                packages: vec![packages[0].name.clone()],
                arguments,
                test_case: Some(case.clone()),
            })
        })
        .collect()
}

#[cfg(any(windows, test))]
fn focused_test_result(check: &Check, code: u32, stdout: &str) -> Result<Option<u32>> {
    check
        .test_case
        .as_ref()
        .map(|_| {
            super::native_acceptance_guard::test_output(
                &stdout.lines().map(String::from).collect::<Vec<_>>(),
                i32::try_from(code).map_err(|error| error.to_string())?,
                Some(1),
            )
        })
        .transpose()
}

fn toolchain(options: &Options) -> Result<String> {
    if let Some(toolchain) = &options.toolchain {
        return identifier(toolchain)
            .then(|| toolchain.clone())
            .ok_or_else(|| "Invalid Rust toolchain.".into());
    }
    let pin: toml::Value = toml::from_str(
        &fs::read_to_string(options.root.join("rust-toolchain.toml"))
            .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let channel = pin
        .get("toolchain")
        .and_then(|value| value.get("channel"))
        .and_then(toml::Value::as_str)
        .filter(|channel| identifier(channel))
        .ok_or("Missing pinned Rust toolchain.")?;
    Ok(format!("{channel}-aarch64-pc-windows-msvc"))
}

fn suite_allowed(phases: &[String], actions: &str, reference: &str) -> Result<()> {
    if phases.iter().any(|phase| phase == "test")
        && (actions != "true" || reference != "refs/heads/main")
    {
        return Err("Complete Rust suites run only in GitHub Actions on main.".into());
    }
    Ok(())
}

fn github_release_context(value: impl Fn(&str) -> String) -> bool {
    value("BONSAI_RELEASE_BUILD") == "true"
        && value("GITHUB_ACTIONS") == "true"
        && value("RUNNER_ENVIRONMENT") == "github-hosted"
        && value("RUNNER_OS") == "Windows"
        && value("RUNNER_ARCH") == "ARM64"
        && value("GITHUB_REPOSITORY") == "symbiogenesis/SwiftBonsai2Runner"
        && value("GITHUB_REF") == "refs/heads/main"
        && value("GITHUB_WORKFLOW_REF")
            == "symbiogenesis/SwiftBonsai2Runner/.github/workflows/release.yml@refs/heads/main"
}

fn select_lane(
    packages: &[Package],
    affected: &[String],
    owned: &[String],
    lane: &str,
) -> Result<Vec<String>> {
    if lane.is_empty()
        || !lane.as_bytes()[0].is_ascii_lowercase()
        || !lane
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte == b'-')
    {
        return Err("Invalid Rust lane name.".into());
    }
    select(packages, affected, "Broker")?;
    select(packages, owned, "Broker")?;
    Ok(packages
        .iter()
        .filter(|package| affected.contains(&package.name) && owned.contains(&package.name))
        .map(|package| package.name.clone())
        .collect())
}

pub(super) fn run(arguments: &[OsString]) -> Result<String> {
    let options = parse(arguments)?;
    if options.github_release
        && (options.command != "rust-check"
            || !github_release_context(|name| std::env::var(name).unwrap_or_default()))
    {
        return Err(
            "GitHub-only build deadlines require the hosted Windows Release workflow.".into(),
        );
    }
    if options.command == "rust-check" {
        if options.focused.cases.is_empty() {
            suite_allowed(
                &options.phases,
                &std::env::var("GITHUB_ACTIONS").unwrap_or_default(),
                &std::env::var("GITHUB_REF").unwrap_or_default(),
            )?;
        }
        if options.target.is_none() || options.evidence.is_none() {
            return Err("rust-check requires --target and --evidence.".into());
        }
    }
    let packages = registry(&options.root)?;
    if options.command == "rust-select" {
        let list = |name| -> Result<Vec<String>> {
            serde_json::from_str(&std::env::var(name).map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())
        };
        let selected = select_lane(
            &packages,
            &list("AFFECTED_PACKAGES")?,
            &list("LANE_PACKAGES")?,
            &std::env::var("RUST_LANE").map_err(|error| error.to_string())?,
        )?;
        let serialized = serde_json::to_string(&selected).map_err(|error| error.to_string())?;
        super::append_environment_file(
            "GITHUB_OUTPUT",
            &format!("has-packages={}\n", !selected.is_empty()),
        )?;
        super::append_environment_file("GITHUB_ENV", &format!("BROKER_PACKAGES={serialized}\n"))?;
        return Ok(serialized);
    }
    let selected = select(&packages, &options.packages, &options.group)?;
    let toolchain = toolchain(&options)?;
    let plan = plan(&options, &selected, &toolchain)?;
    if options.command == "rust-plan" {
        return serde_json::to_string(&plan).map_err(|error| error.to_string());
    }
    #[cfg(windows)]
    {
        execute::run(&options, &selected, &toolchain, &plan)
    }
    #[cfg(not(windows))]
    {
        Err("Rust check execution requires native Windows; planning is portable.".into())
    }
}
