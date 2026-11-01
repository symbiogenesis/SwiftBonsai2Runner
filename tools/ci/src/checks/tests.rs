use super::*;

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
            "pinned",
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
        "pinned",
        "Cargo.toml",
        false,
    )?;
    assert!(!check.contains(&"--features".into()));
    assert!(!check.contains(&"--all-features".into()));
    let check = arguments(
        &[&restricted],
        "clippy",
        "Native",
        "pinned",
        "Cargo.toml",
        false,
    )?;
    assert!(check.contains(&"--all-features".into()));
    let check = arguments(
        &[&all, &restricted],
        "fmt",
        "Native",
        "pinned",
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
            "pinned",
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
    let checks = plan(&options, &[&all, &restricted], "pinned")?;
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
            for phase in PHASES.iter().chain(["fmt"].iter()) {
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
                    .windows(2)
                    .filter(|pair| pair[0] == "--package")
                    .map(|pair| pair[1].clone())
                    .collect::<Vec<_>>();
                assert_eq!(names, check.packages);
                if check.phase == "fmt" {
                    assert!(check.arguments.contains(&"--check".into()));
                    assert!(!check.arguments.contains(&"--all-features".into()));
                    continue;
                }
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
                    assert!(check.arguments.contains(&"--no-fail-fast".into()));
                }
            }
        }
    }
    Ok(())
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
    assert!(select_lane(&packages, &[], &both, "core")?.is_empty());
    assert!(select_lane(&packages, &both, &[], "core")?.is_empty());
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
