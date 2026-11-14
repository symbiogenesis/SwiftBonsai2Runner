use super::*;

const PINNED_TOOLCHAIN: &str = "nightly-2026-11-12-aarch64-pc-windows-msvc";

fn fixture(name: &str, restricted: bool) -> Package {
    Package {
        name: name.into(),
        manifest: format!("{name}/Cargo.toml"),
        group: "Broker".into(),
        features: if restricted { vec![] } else { vec!["*".into()] },
        compatibility_compile_features: restricted.then(|| vec!["native-loader".into()]),
        native_features: restricted.then(|| vec!["*".into()]),
    }
}

#[test]
fn restricted_features_remain_separate_and_formatting_has_no_features() -> Result<()> {
    let all = fixture("all", false);
    let restricted = fixture("restricted", true);
    for phase in ["clippy", "doc", "build"] {
        let arguments = arguments(
            &[&restricted],
            phase,
            "Compatibility",
            PINNED_TOOLCHAIN,
            "Cargo.toml",
            false,
        )?;
        assert!(arguments.contains(&"native-loader".into()));
        assert!(!arguments.contains(&"--all-features".into()));
    }
    let check = arguments(
        &[&restricted],
        "test",
        "Compatibility",
        PINNED_TOOLCHAIN,
        "Cargo.toml",
        false,
    )?;
    assert!(!check.contains(&"--features".into()));
    assert!(!check.contains(&"--all-features".into()));
    let check = arguments(
        &[&restricted],
        "clippy",
        "Native",
        PINNED_TOOLCHAIN,
        "Cargo.toml",
        false,
    )?;
    assert!(check.contains(&"--all-features".into()));
    let check = arguments(
        &[&all, &restricted],
        "fmt",
        "Native",
        PINNED_TOOLCHAIN,
        "Cargo.toml",
        false,
    )?;
    assert!(check.contains(&"--check".into()));
    assert!(!check.contains(&"--all-features".into()));
    assert!(
        arguments(
            &[&all, &restricted],
            "test",
            "Compatibility",
            PINNED_TOOLCHAIN,
            "Cargo.toml",
            false
        )
        .is_err()
    );
    let mut options = parse(&[
        "rust-plan".into(),
        "--phase".into(),
        "fmt".into(),
        "--phase".into(),
        "test".into(),
        "--profile".into(),
        "Compatibility".into(),
    ])?;
    options.root = PathBuf::from("workspace");
    let checks = plan(&options, &[&all, &restricted], PINNED_TOOLCHAIN)?;
    assert_eq!(checks.len(), 3);
    assert_eq!(checks[0].packages, ["all", "restricted"]);
    assert_eq!(checks[1].packages, ["all"]);
    assert_eq!(checks[2].packages, ["restricted"]);
    Ok(())
}

#[test]
fn production_batches_preserve_explicit_package_coverage_and_release_policy() -> Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let packages = registry(&root)?;
    assert_eq!(packages.len(), 12);
    for group in ["All", "Broker"] {
        for profile in ["Foundation", "Compatibility", "Native"] {
            let mut cli = vec![
                "rust-plan".into(),
                "--offline".into(),
                "--profile".into(),
                profile.into(),
            ];
            for phase in PHASES.iter().chain(std::iter::once(&"fmt")) {
                cli.extend(["--phase".into(), (*phase).into()]);
            }
            let mut options = parse(&cli)?;
            options.root.clone_from(&root);
            let toolchain = toolchain(&options)?;
            let selected = select(&packages, &[], group)?;
            let plan = plan(&options, &selected, &toolchain)?;
            assert_eq!(plan.len(), 5);
            for check in plan {
                assert_eq!(
                    check.packages,
                    selected
                        .iter()
                        .map(|package| package.name.clone())
                        .collect::<Vec<_>>()
                );
                assert_eq!(check.arguments[0], format!("+{toolchain}"));
                assert!(!check.arguments.contains(&"--workspace".into()));
                assert!(!check.arguments.contains(&"--all".into()));
                let names = check
                    .arguments
                    .array_windows::<2>()
                    .filter(|pair| pair[0] == "--package")
                    .map(|pair| pair[1].clone())
                    .collect::<Vec<_>>();
                assert_eq!(names, check.packages);
                if check.phase == "fmt" {
                    assert!(check.arguments.contains(&"--check".into()));
                    assert!(!check.arguments.contains(&"--all-features".into()));
                    assert!(!check.arguments.contains(&"--target".into()));
                    assert!(!check.arguments.contains(&"--config".into()));
                    continue;
                }
                assert_eq!(
                    check
                        .arguments
                        .array_windows::<2>()
                        .filter(|pair| pair[0] == "--target")
                        .map(|pair| pair[1].as_str())
                        .collect::<Vec<_>>(),
                    ["aarch64-pc-windows-msvc"]
                );
                assert!(
                    check
                        .arguments
                        .array_windows::<2>()
                        .any(|pair| pair[0] == "--config"
                            && pair[1] == root.join(".cargo/config.toml").to_string_lossy())
                );
                for required in ["--locked", "--release", "--offline", "--all-features"] {
                    assert!(check.arguments.contains(&required.into()));
                }
                if check.phase == "clippy" {
                    assert!(check.arguments.ends_with(&[
                        "--all-targets".into(),
                        "--".into(),
                        "-D".into(),
                        "warnings".into()
                    ]));
                }
                if check.phase == "doc" {
                    assert!(check.arguments.ends_with(&[
                        "--no-deps".into(),
                        "-Z".into(),
                        "unstable-options".into(),
                        "--output-format".into(),
                        "json".into()
                    ]));
                } else {
                    assert!(!check.arguments.contains(&"--output-format".into()));
                }
                if check.phase == "test" {
                    assert!(check.arguments.ends_with(&[
                        "--no-fail-fast".into(),
                        "--".into(),
                        "-Z".into(),
                        "unstable-options".into(),
                        "--report-time".into(),
                    ]));
                } else {
                    assert!(!check.arguments.contains(&"--report-time".into()));
                }
            }
        }
    }
    Ok(())
}

#[test]
fn static_runtime_compile_plans_reject_nonmatching_compiler_hosts() {
    let package = fixture("application", false);
    for toolchain in [
        "nightly-2026-11-12-x86_64-pc-windows-msvc",
        "nightly-2026-11-12-aarch64-unknown-linux-gnu",
        "nightly-2026-11-12",
    ] {
        for phase in ["clippy", "doc", "test", "build"] {
            assert_eq!(
                arguments(&[&package], phase, "Native", toolchain, "Cargo.toml", false),
                Err("Native Rust checks require a Windows ARM64 compiler host.".into())
            );
        }
    }
}

#[test]
fn complete_suites_require_main_actions_before_creating_outputs() -> Result<()> {
    for extra in [vec!["--phase", "test"], vec!["--full-tests"]] {
        let options = parse(
            &std::iter::once("rust-check")
                .chain(extra)
                .map(Into::into)
                .collect::<Vec<_>>(),
        )?;
        for (actions, reference) in [
            ("false", "refs/heads/main"),
            ("true", "refs/pull/1/merge"),
            ("", ""),
        ] {
            assert_eq!(
                suite_allowed(&options.phases, actions, reference),
                Err("Complete Rust suites run only in GitHub Actions on main.".into())
            );
        }
        assert!(suite_allowed(&options.phases, "true", "refs/heads/main").is_ok());
    }
    assert!(suite_allowed(&["build".into()], "false", "").is_ok());
    Ok(())
}

#[test]
fn lane_selection_rejects_unknown_packages_and_never_expands_empty_sets() -> Result<()> {
    let packages = [fixture("first", false), fixture("second", false)];
    let both = vec!["second".into(), "first".into()];
    assert_eq!(
        select_lane(&packages, &both, &["second".into()], "maintenance")?,
        ["second"]
    );
    assert_eq!(
        select_lane(&packages, &[], &both, "core")?,
        Vec::<String>::new()
    );
    assert_eq!(
        select_lane(&packages, &both, &[], "core")?,
        Vec::<String>::new()
    );
    assert!(select_lane(&packages, &["missing".into()], &both, "core").is_err());
    assert!(select_lane(&packages, &both, &["missing".into()], "core").is_err());
    assert!(select_lane(&packages, &both, &both, "bad\nname").is_err());
    Ok(())
}

#[test]
fn invalid_phases_profiles_and_execution_flags_are_rejected() {
    for values in [
        vec!["rust-plan", "--phase", "bench"],
        vec!["rust-plan", "--profile", "unsafe"],
        vec!["rust-check", "--timeout-seconds", "0"],
        vec!["rust-check", "--timeout-seconds", "3601"],
        vec!["rust-select", "--full-tests"],
        vec!["rust-plan", "--packages-json", "{}"],
        vec!["rust-plan", "--packages-json", "[]"],
    ] {
        assert!(parse(&values.into_iter().map(Into::into).collect::<Vec<_>>()).is_err());
    }
}

#[test]
fn focused_cases_bind_one_package_target_and_exact_test_invocations() -> Result<()> {
    let package = fixture("application", false);
    let mut options = parse(&[
        "rust-plan".into(),
        "--package".into(),
        "application".into(),
        "--test-case".into(),
        "module::first".into(),
        "--test-case".into(),
        "module::second".into(),
        "--test-kind".into(),
        "bin".into(),
        "--test-target".into(),
        "application".into(),
    ])?;
    options.root = PathBuf::from("workspace");
    assert_eq!(options.phases, ["test"]);
    let checks = plan(&options, &[&package], PINNED_TOOLCHAIN)?;
    assert_eq!(checks.len(), 2);
    for (index, check) in checks.iter().enumerate() {
        let case = &options.focused.cases[index];
        assert_eq!(check.test_case.as_ref(), Some(case));
        assert_eq!(check.name, format!("application-test-{}", index + 1));
        assert_eq!(check.packages, ["application"]);
        assert!(
            check
                .arguments
                .array_windows::<2>()
                .any(|pair| pair[0] == "--bin" && pair[1] == "application")
        );
        assert!(check.arguments.ends_with(&[
            case.clone(),
            "--".into(),
            "--exact".into(),
            "-Z".into(),
            "unstable-options".into(),
            "--report-time".into(),
        ]));
        for rejected in [
            "--no-fail-fast",
            "--ignored",
            "--workspace",
            "--doc",
            "--tests",
        ] {
            assert!(!check.arguments.contains(&rejected.into()));
        }
    }
    options.focused.kind = None;
    options.focused.target = None;
    let checks = plan(&options, &[&package], PINNED_TOOLCHAIN)?;
    assert!(checks[0].arguments.contains(&"--lib".into()));
    options.focused.kind = Some("test".into());
    options.focused.target = Some("native_isolation".into());
    let checks = plan(&options, &[&package], PINNED_TOOLCHAIN)?;
    assert!(
        checks[0]
            .arguments
            .array_windows::<2>()
            .any(|pair| pair[0] == "--test" && pair[1] == "native_isolation")
    );
    assert!(plan(&options, &[&package, &package], PINNED_TOOLCHAIN).is_err());
    Ok(())
}

#[test]
fn focused_cases_reject_ambiguous_targets_suite_flags_and_filter_injection() -> Result<()> {
    let base = ["rust-check", "--package", "application"];
    for tail in [
        vec!["--test-kind", "bin"],
        vec!["--test-target", "application"],
        vec!["--test-case", "module::case", "--test-kind", "bin"],
        vec![
            "--test-case",
            "module::case",
            "--test-target",
            "application",
        ],
        vec!["--test-case", "module::case", "--test-kind", "unsafe"],
        vec![
            "--test-case",
            "module::case",
            "--test-kind",
            "bin",
            "--test-target",
            "--ignored",
        ],
        vec!["--test-case", "module::case", "--full-tests"],
        vec!["--test-case", "module::case", "--phase", "clippy"],
        vec!["--test-case", "module::case", "--test-case", "module::case"],
        vec!["--test-case", "module::case", "--package", "other"],
    ] {
        let arguments = base
            .iter()
            .copied()
            .chain(tail)
            .map(Into::into)
            .collect::<Vec<_>>();
        assert!(parse(&arguments).is_err(), "accepted {arguments:?}");
    }
    for case in [
        "",
        "--ignored",
        "module::",
        "::case",
        "m::*",
        "mod::case\n",
        "m::bad-name",
        "m::é",
    ] {
        let arguments = base
            .iter()
            .copied()
            .chain(["--test-case", case])
            .map(Into::into)
            .collect::<Vec<_>>();
        assert!(parse(&arguments).is_err(), "accepted {case:?}");
    }
    assert!(
        parse(&[
            "rust-check".into(),
            "--test-case".into(),
            "module::case".into()
        ])
        .is_err()
    );
    let mut options = parse(
        &base
            .iter()
            .copied()
            .chain(["--test-case", "root_case"])
            .map(Into::into)
            .collect::<Vec<_>>(),
    )?;
    options.focused.cases = (0..17)
        .map(|index| format!("module::case_{index}"))
        .collect();
    assert!(validate_focused_tests(&options, false).is_err());
    Ok(())
}

#[test]
fn focused_case_verdict_rejects_zero_ignored_duplicate_or_failed_summaries() -> Result<()> {
    let mut check = Check {
        name: "package-test-1".into(),
        phase: "test".into(),
        packages: vec!["package".into()],
        arguments: Vec::new(),
        test_case: Some("module::case".into()),
    };
    let passed = "running 1 test\ntest module::case ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 9 filtered out; finished in 0.00s\n";
    assert_eq!(focused_test_result(&check, 0, passed)?, Some(1));
    for stdout in [
        String::new(),
        passed.replace("1 passed", "0 passed"),
        passed.replace("1 passed", "2 passed"),
        passed.replace("0 ignored", "1 ignored"),
        passed.replace("0 failed", "1 failed"),
        format!("{passed}{passed}"),
        passed.replace("finished in 0.00s", "finished in NaNs"),
    ] {
        assert!(focused_test_result(&check, 0, &stdout).is_err());
    }
    for exit in [1, 101, u32::MAX] {
        assert!(focused_test_result(&check, exit, passed).is_err());
    }
    check.test_case = None;
    assert_eq!(focused_test_result(&check, 0, "compiler output")?, None);
    Ok(())
}

#[test]
fn release_deadlines_require_the_explicit_exact_hosted_workflow() {
    let environment = std::collections::BTreeMap::from([
        ("BONSAI_RELEASE_BUILD", "true"),
        ("GITHUB_ACTIONS", "true"),
        ("RUNNER_ENVIRONMENT", "github-hosted"),
        ("RUNNER_OS", "Windows"),
        ("RUNNER_ARCH", "ARM64"),
        ("GITHUB_REPOSITORY", "symbiogenesis/SwiftBonsai2Runner"),
        ("GITHUB_REF", "refs/heads/main"),
        (
            "GITHUB_WORKFLOW_REF",
            "symbiogenesis/SwiftBonsai2Runner/.github/workflows/release.yml@refs/heads/main",
        ),
    ]);
    assert!(github_release_context(|name| environment[name].into()));
    for key in environment.keys() {
        for replacement in ["", "false", "self-hosted", "refs/heads/release"] {
            assert!(!github_release_context(|name| {
                if name == *key {
                    replacement.into()
                } else {
                    environment[name].into()
                }
            }));
        }
    }
    assert!(!github_release_context(|_| String::new()));
}
