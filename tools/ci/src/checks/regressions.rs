//! Typed shared regression selection with complete admission before execution.
use crate::Result;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    ffi::OsString,
    io::Read as _,
    path::{Path, PathBuf},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    root: PathBuf,
    target_directory: PathBuf,
    package: String,
    toolchain: String,
    target_kind: String,
    target_name: String,
    selections: Vec<Selection>,
    #[serde(default)]
    plan_only: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Selection {
    name: String,
    exact: bool,
    #[serde(rename = "expected-passed")]
    #[serde(deserialize_with = "count")]
    expected_passed: Option<u32>,
}
fn count<'de, D: serde::Deserializer<'de>>(
    decoder: D,
) -> std::result::Result<Option<u32>, D::Error> {
    Option::<u32>::deserialize(decoder)
}
#[derive(Serialize)]
struct Control {
    name: String,
    arguments: Vec<String>,
    expected_passed: Option<u32>,
}

fn normalized(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_uppercase()
}
fn overlapping(a: &Path, b: &Path) -> bool {
    let a = normalized(a);
    let b = normalized(b);
    a == b || a.starts_with(&(b.clone() + "\\")) || b.starts_with(&(a + "\\"))
}
fn name(value: &str, colon: bool) -> bool {
    value.len() <= 1024
        && value
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || (colon && b == b':'))
}
fn plan(request: &Request) -> Result<Vec<Control>> {
    if !request.root.is_absolute()
        || !request.target_directory.is_absolute()
        || request.target_directory.to_string_lossy().contains('%')
        || overlapping(&request.root, &request.target_directory)
    {
        return Err("Regression build outputs must be external and not overlap source.".into());
    }
    let package = request
        .package
        .strip_prefix("bonsai-")
        .ok_or("An explicit Bonsai package is required.")?;
    if package.is_empty()
        || !package
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err("Invalid regression package.".into());
    }
    let date = request
        .toolchain
        .strip_prefix("nightly-")
        .and_then(|s| s.strip_suffix("-aarch64-pc-windows-msvc"))
        .ok_or("An explicit native compiler is required.")?;
    if date.len() != 10
        || !date.bytes().enumerate().all(|(index, b)| {
            if [4, 7].contains(&index) {
                b == b'-'
            } else {
                b.is_ascii_digit()
            }
        })
    {
        return Err("Invalid native compiler pin.".into());
    }
    match request.target_kind.as_str() {
        "lib" if request.target_name.is_empty() => {}
        "bin" | "test"
            if name(&request.target_name.replace('-', "_"), false)
                && request
                    .target_name
                    .bytes()
                    .next()
                    .is_some_and(|b| b.is_ascii_lowercase())
                && request.target_name.bytes().all(|b| {
                    b.is_ascii_lowercase() || b.is_ascii_digit() || b"_-".contains(&b)
                }) => {}
        _ => return Err("Invalid Cargo regression target selection.".into()),
    }
    if !(1..=128).contains(&request.selections.len()) {
        return Err("Select one to 128 explicit regression controls.".into());
    }
    let mut seen = BTreeSet::new();
    let mut controls = Vec::new();
    for selection in &request.selections {
        if !name(&selection.name, true)
            || !seen.insert(&selection.name)
            || selection.expected_passed == Some(0)
            || (selection.exact && selection.expected_passed != Some(1))
        {
            return Err("Malformed or duplicate regression selection.".into());
        }
        let mut arguments = vec![
            format!("+{}", request.toolchain),
            "test".into(),
            "--locked".into(),
            "--release".into(),
            "--manifest-path".into(),
            request
                .root
                .join("Cargo.toml")
                .to_str()
                .ok_or("Regression source paths must be Unicode.")?
                .into(),
            "--package".into(),
            request.package.clone(),
            format!("--{}", request.target_kind),
        ];
        if request.target_kind != "lib" {
            arguments.push(request.target_name.clone());
        }
        arguments.push(selection.name.clone());
        if selection.exact {
            arguments.extend(["--".into(), "--exact".into()]);
        }
        controls.push(Control {
            name: selection.name.clone(),
            arguments,
            expected_passed: selection.expected_passed,
        });
    }
    Ok(controls)
}

#[cfg(any(windows, test))]
fn selected(
    controls: &[Control],
    mut execute: impl FnMut(&Control) -> Result<(u32, String)>,
) -> Result<String> {
    let mut counts = Vec::new();
    for control in controls {
        let (code, output) = execute(control)?;
        let lines = output.lines().map(str::to_owned).collect::<Vec<_>>();
        let code = i32::try_from(code)
            .map_err(|_| "Regression exit status exceeds signed command range.")?;
        let count = super::super::native_acceptance_guard::test_output(
            &lines,
            code,
            control.expected_passed,
        )?;
        counts.push(serde_json::json!({"name":control.name,"passed":count}));
    }
    serde_json::to_string(&serde_json::json!({"Passed":true,"Controls":counts}))
        .map_err(|e| e.to_string())
}

#[cfg(windows)]
fn execute(request: &Request, controls: &[Control]) -> Result<String> {
    use bonsai_windows_process::ToolProcessSpec;
    use std::{
        collections::BTreeMap,
        fs,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };
    bonsai_artifacts::validate_external_path(&request.target_directory)
        .map_err(|e| e.to_string())?;
    if std::env::consts::ARCH != "aarch64" || !request.root.join("Cargo.toml").is_file() {
        return Err(
            "Native regression execution requires Windows ARM64 and a source workspace.".into(),
        );
    }
    let cargo = std::env::split_paths(&std::env::var_os("PATH").ok_or("PATH unavailable.")?)
        .filter(|path| path.is_absolute())
        .map(|path| path.join("cargo.exe"))
        .find(|path| path.is_file())
        .ok_or("Cargo executable missing.")?;
    let serial = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let scratch = std::env::temp_dir().join(format!(
        "bonsai-regressions-{}-{serial}",
        std::process::id()
    ));
    fs::create_dir(&scratch).map_err(|e| e.to_string())?;
    let mut index = 0;
    let allowance = if super::github_release_context(|name| std::env::var(name).unwrap_or_default())
    {
        None
    } else {
        Some(Duration::from_mins(15))
    };
    let result = selected(controls, |control| {
        index += 1;
        let stdout = scratch.join(format!("{index}.stdout"));
        let stderr = scratch.join(format!("{index}.stderr"));
        let log = scratch.join(format!("{index}.log"));
        let spec = ToolProcessSpec {
            executable: cargo.clone(),
            arguments: control.arguments.iter().map(OsString::from).collect(),
            working_directory: scratch.clone(),
            stdout: stdout.clone(),
            stderr: stderr.clone(),
            environment: BTreeMap::from([
                (
                    "CARGO_TARGET_DIR".into(),
                    request
                        .target_directory
                        .to_str()
                        .ok_or("Regression target must be Unicode.")?
                        .into(),
                ),
                ("CARGO_UNSTABLE_CHECKSUM_FRESHNESS".into(), "true".into()),
                ("CARGO_BUILD_FINGERPRINT".into(), "content".into()),
            ]),
        };
        let result = super::execute::command(&spec, allowance, &log)?;
        let lines = result.1.lines().map(str::to_owned).collect::<Vec<_>>();
        let disposition = super::super::native_acceptance_guard::test_output(
            &lines,
            i32::try_from(result.0).map_err(|_| "Invalid regression exit code.")?,
            control.expected_passed,
        );
        if disposition.is_err() {
            super::execute::write_failure_logs(&mut std::io::stderr().lock(), &spec);
        }
        // One combined diagnostic is sufficient after a completed failure;
        // passing controls retain no successful build chatter or empty captures.
        for path in [&stdout, &stderr] {
            fs::remove_file(path).map_err(|e| e.to_string())?;
        }
        if disposition.is_ok() {
            fs::remove_file(&log).map_err(|e| e.to_string())?;
        } else if fs::metadata(&log).map_err(|e| e.to_string())?.len() == 0 {
            fs::write(
                &log,
                "Regression produced no typed test result; empty output is not success.\n",
            )
            .map_err(|e| e.to_string())?;
        }
        disposition.map_err(|error| format!("{error} Evidence: {}", log.display()))?;
        Ok(result)
    });
    if fs::read_dir(&scratch)
        .map_err(|e| e.to_string())?
        .next()
        .is_none()
    {
        fs::remove_dir(&scratch).map_err(|e| e.to_string())?;
    }
    result
}

pub(super) fn run(arguments: &[OsString]) -> Result<String> {
    let mut raw = Vec::new();
    match &arguments[1..] {
        [option, source] if option == "--request" && source == "-" => {
            std::io::stdin()
                .take(1_048_577)
                .read_to_end(&mut raw)
                .map_err(|e| e.to_string())?;
        }
        [option, source] if option == "--json" => raw.extend_from_slice(
            source
                .to_str()
                .ok_or("Regression request must be Unicode.")?
                .as_bytes(),
        ),
        _ => return Err("Use native-regressions --request - or --json OBJECT.".into()),
    }
    if raw.len() > 1_048_576 {
        return Err("Regression request exceeds byte bound.".into());
    }
    let request: Request = serde_json::from_slice(&raw).map_err(|e| e.to_string())?;
    let controls = plan(&request)?;
    if request.plan_only {
        return serde_json::to_string(&controls).map_err(|e| e.to_string());
    }
    #[cfg(windows)]
    {
        execute(&request, &controls)
    }
    #[cfg(not(windows))]
    {
        Err("Regression execution requires native Windows ARM64.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Request {
        Request {
            root: std::env::temp_dir().join("source"),
            target_directory: std::env::temp_dir().join("compiler"),
            package: "bonsai-tools".into(),
            toolchain: "nightly-2026-11-12-aarch64-pc-windows-msvc".into(),
            target_kind: "lib".into(),
            target_name: String::new(),
            selections: vec![
                Selection {
                    name: "module::exact_case".into(),
                    exact: true,
                    expected_passed: Some(1),
                },
                Selection {
                    name: "owner_shutdown_".into(),
                    exact: false,
                    expected_passed: None,
                },
            ],
            plan_only: true,
        }
    }
    #[test]
    fn exact_grouped_arguments_and_typed_counts() -> Result<()> {
        let plan = plan(&fixture())?;
        assert_eq!(
            &plan[0].arguments[..4],
            [
                "+nightly-2026-11-12-aarch64-pc-windows-msvc",
                "test",
                "--locked",
                "--release"
            ]
        );
        assert_eq!(
            &plan[0].arguments[plan[0].arguments.len() - 4..],
            ["--lib", "module::exact_case", "--", "--exact"]
        );
        assert_eq!(plan[1].expected_passed, None);
        assert_eq!(
            &plan[1].arguments[plan[1].arguments.len() - 2..],
            ["--lib", "owner_shutdown_"]
        );
        Ok(())
    }
    #[test]
    fn binary_target_without_ignored_tests() -> Result<()> {
        let mut request = fixture();
        request.target_kind = "bin".into();
        request.target_name = "bonsai-chat".into();
        let controls = plan(&request)?;
        assert!(controls[0].arguments.iter().any(|arg| arg == "--bin"));
        assert!(!controls[0].arguments.iter().any(|arg| arg == "--ignored"));
        Ok(())
    }
    #[test]
    fn complete_selection_validated_before_execution() {
        for bad in ["--ignored", "a b", ""] {
            let mut request = fixture();
            request.selections[1].name = bad.into();
            assert!(plan(&request).is_err());
        }
        let mut request = fixture();
        request.selections[1].name = request.selections[0].name.clone();
        assert!(plan(&request).is_err());
        let mut request = fixture();
        request.selections[0].expected_passed = None;
        assert!(plan(&request).is_err());
    }
    #[test]
    fn cargo_failure_reaches_guard_and_stops() -> Result<()> {
        let controls = plan(&fixture())?;
        let mut calls = 0;
        assert!(selected(&controls,|_| {calls+=1;Ok((9,"test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s".into()))}).is_err());
        assert_eq!(calls, 1);
        Ok(())
    }
    #[test]
    fn guard_failure_stops_action() -> Result<()> {
        let controls = plan(&fixture())?;
        let mut calls = 0;
        assert!(selected(&controls,|_| {calls+=1;Ok((0,"test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s".into()))}).is_err());
        assert_eq!(calls, 1);
        Ok(())
    }
}
