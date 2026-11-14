//! Shared, typed admission checks for the hosted native acceptance package.

use std::ffi::OsString;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::Result;

const TOOLCHAIN: &str = "nightly-2026-11-12-aarch64-pc-windows-msvc";
const PACKAGES: [&str; 3] = ["bonsai-broker", "bonsai-chat", "bonsai-tools"];
const SOURCES: [&str; 7] = [
    ".github/actions/native-regressions/action.yml",
    ".github/workflows/native-runtime-acceptance.yml",
    "scripts/Start-Chat.ps1",
    "tools/ci/Cargo.toml",
    "tools/ci/src/checks/regressions.rs",
    "tools/ci/src/command.rs",
    "tools/ci/src/native_acceptance_guard.rs",
];
const CONTROLS: [&str; 2] = [
    "command::native_acceptance_guard::tests::negative_fixtures_cover_receipts_and_test_output",
    "command::native_acceptance_guard::tests::workflow_receipt_requires_source_bound_complete_controls",
];
const POWERSHELL_BLOCKS: u32 = 11;

#[derive(Deserialize)]
#[serde(
    tag = "operation",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum Request {
    Analyzer {
        receipt: Value,
        revision: String,
        compiler_identity: String,
    },
    TestOutput {
        output: Vec<String>,
        exit_code: i32,
        expected_passed: Option<u32>,
    },
    SelfCheck {
        revision: String,
    },
    WorkflowReceipt {
        receipt: Value,
        revision: String,
        sources: Vec<Source>,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Analyzer {
    schema_version: u32,
    success: bool,
    revision: String,
    toolchain: String,
    compiler_identity: String,
    packages: Vec<String>,
    checks: Vec<Check>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Check {
    phase: String,
    packages: Vec<String>,
    passed: bool,
    exit_code: i32,
}

#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
struct Source {
    path: String,
    bytes: u64,
    sha256: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
struct WorkflowReceipt {
    schema_version: u32,
    kind: String,
    revision: String,
    success: bool,
    receipt_fixtures: u32,
    output_fixtures: u32,
    parsed_power_shell_blocks: u32,
    parsed_launcher: bool,
    exact_rust_controls: Vec<String>,
    source_digest_scope: String,
    sources: Vec<Source>,
}

fn require(condition: bool, message: &str) -> Result<()> {
    condition.then_some(()).ok_or_else(|| message.into())
}

fn lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn packages_match(packages: &[String]) -> bool {
    let mut packages = packages.iter().map(String::as_str).collect::<Vec<_>>();
    packages.sort_unstable();
    packages == PACKAGES
}

fn analyzer(value: Value, revision: &str, compiler: &str) -> Result<()> {
    let receipt: Analyzer = serde_json::from_value(value).map_err(|error| error.to_string())?;
    require(
        lower_hex(revision, 40)
            && !compiler.trim().is_empty()
            && receipt.schema_version == 2
            && receipt.success
            && receipt.revision == revision
            && receipt.toolchain == TOOLCHAIN
            && receipt.compiler_identity == compiler
            && packages_match(&receipt.packages)
            && receipt.checks.len() == 2,
        "Hosted analyzer/link receipt differs from the exact source, compiler or package inventory.",
    )?;
    for phase in ["clippy", "build"] {
        let selected = receipt
            .checks
            .iter()
            .filter(|check| check.phase == phase)
            .collect::<Vec<_>>();
        require(
            selected.len() == 1
                && selected[0].passed
                && selected[0].exit_code == 0
                && packages_match(&selected[0].packages),
            "Both analyzer/link phases require one successful zero-exit check of every package.",
        )?;
    }
    Ok(())
}

pub(super) fn test_output(
    output: &[String],
    exit_code: i32,
    expected_passed: Option<u32>,
) -> Result<u32> {
    let summaries = output
        .iter()
        .filter(|line| line.starts_with("test result:"))
        .collect::<Vec<_>>();
    require(
        exit_code == 0 && summaries.len() == 1,
        "Test command must succeed with exactly one summary.",
    )?;
    let (passed, suffix) = summaries[0]
        .trim_end_matches('\r')
        .strip_prefix("test result: ok. ")
        .and_then(|summary| summary.split_once(" passed; 0 failed; 0 ignored;"))
        .ok_or("Test summary must report positive passes, zero failures and zero ignored tests.")?;
    let passed = passed
        .parse::<u32>()
        .ok()
        .filter(|passed| *passed > 0)
        .ok_or("Test summary must report positive passes, zero failures and zero ignored tests.")?;
    require(
        summary_suffix(suffix),
        "Test summary suffix is malformed or contains trailing output.",
    )?;
    require(
        expected_passed.is_none_or(|expected| passed == expected),
        "Exact test count differs.",
    )?;
    Ok(passed)
}

fn summary_suffix(value: &str) -> bool {
    value
        .strip_prefix(" 0 measured; ")
        .and_then(|tail| tail.split_once(" filtered out; finished in "))
        .is_some_and(|(filtered, elapsed)| {
            filtered.parse::<u32>().is_ok()
                && elapsed
                    .strip_suffix('s')
                    .and_then(|elapsed| elapsed.parse::<f64>().ok())
                    .is_some_and(|elapsed| elapsed.is_finite() && elapsed >= 0.0)
        })
}

fn source_inventory(mut sources: Vec<Source>) -> Result<Vec<Source>> {
    sources.sort_by(|left, right| left.path.cmp(&right.path));
    require(
        sources
            .iter()
            .map(|source| source.path.as_str())
            .collect::<Vec<_>>()
            == SOURCES
            && sources
                .iter()
                .all(|source| source.bytes > 0 && lower_hex(&source.sha256, 64)),
        "Workflow contract needs the exact source inventory with nonempty lengths and SHA-256 identities.",
    )?;
    Ok(sources)
}

fn workflow_receipt(value: Value, revision: &str, sources: Vec<Source>) -> Result<()> {
    let receipt: WorkflowReceipt =
        serde_json::from_value(value).map_err(|error| error.to_string())?;
    require(
        lower_hex(revision, 40)
            && receipt.schema_version == 1
            && receipt.kind == "native-acceptance-workflow-contract"
            && receipt.revision == revision
            && receipt.success
            && receipt.receipt_fixtures == 23
            && receipt.output_fixtures == 13
            && receipt.parsed_power_shell_blocks == POWERSHELL_BLOCKS
            && receipt.parsed_launcher
            && receipt.source_digest_scope == "utf8-no-bom-lf"
            && receipt
                .exact_rust_controls
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                == CONTROLS,
        "Workflow syntax/negative-control receipt is incomplete or belongs to another source.",
    )?;
    require(
        source_inventory(receipt.sources)? == source_inventory(sources)?,
        "Workflow syntax/control receipt source-file identities differ.",
    )
}

fn analyzer_fixture() -> Value {
    json!({"SchemaVersion":2,"Success":true,"Revision":"1".repeat(40),"Toolchain":TOOLCHAIN,
        "CompilerIdentity":"fixture compiler; no compiler executed","Packages":PACKAGES,
        "Checks":[{"Phase":"clippy","Packages":PACKAGES,"Passed":true,"ExitCode":0},
                  {"Phase":"build","Packages":PACKAGES,"Passed":true,"ExitCode":0}]})
}

fn receipt_fixtures() -> Result<u32> {
    let valid = analyzer_fixture();
    let revision = "1".repeat(40);
    let compiler = "fixture compiler; no compiler executed";
    analyzer(valid.clone(), &revision, compiler)?;
    let mut reordered = valid.clone();
    reordered["Checks"]
        .as_array_mut()
        .ok_or("Missing fixture checks.")?
        .reverse();
    reordered["Packages"]
        .as_array_mut()
        .ok_or("Missing fixture packages.")?
        .reverse();
    analyzer(reordered, &revision, compiler)?;
    let corruptions = [
        ("/SchemaVersion", json!(1)),
        ("/Success", json!(false)),
        ("/Success", json!("true")),
        ("/Revision", json!("0".repeat(40))),
        ("/CompilerIdentity", json!("another compiler")),
        ("/Toolchain", json!("another toolchain")),
        ("/Checks", json!([valid["Checks"][0].clone()])),
        ("/Checks/1/Phase", json!("clippy")),
        ("/Checks/1/Phase", json!("test")),
        ("/Checks/1/Passed", json!(false)),
        ("/Checks/1/Passed", json!("true")),
        ("/Checks/1/ExitCode", json!(1)),
        ("/Checks/1/ExitCode", json!(null)),
        ("/Checks/1/ExitCode", json!("0")),
        (
            "/Checks/0/Packages",
            json!(["bonsai-broker", "bonsai-tools"]),
        ),
        (
            "/Checks/1/Packages",
            json!(["bonsai-broker", "bonsai-tools"]),
        ),
        (
            "/Checks/1/Packages",
            json!([
                "bonsai-broker",
                "bonsai-chat",
                "bonsai-tools",
                "bonsai-tools"
            ]),
        ),
        ("/Packages", json!(["bonsai-broker", "bonsai-tools"])),
        (
            "/Checks",
            json!([
                valid["Checks"][0].clone(),
                valid["Checks"][1].clone(),
                valid["Checks"][1].clone()
            ]),
        ),
        ("/Checks/1/Packages", json!([])),
        ("/Checks/1/Passed", json!(null)),
    ];
    let count = u32::try_from(corruptions.len() + 2).map_err(|error| error.to_string())?;
    for (pointer, replacement) in corruptions {
        let mut altered = valid.clone();
        *altered
            .pointer_mut(pointer)
            .ok_or("Missing receipt fixture field.")? = replacement;
        require(
            analyzer(altered, &revision, compiler).is_err(),
            "Analyzer negative fixture was admitted.",
        )?;
    }
    Ok(count)
}

fn output_fixtures() -> Result<u32> {
    let passed = "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 42 filtered out; finished in 0.00s";
    test_output(&[passed.into()], 0, Some(1))?;
    test_output(&["test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 42 filtered out; finished in 0.00s".into()], 0, None)?;
    let rejected = [
        (
            vec!["test result: ok. 0 passed; 0 failed; 0 ignored;"],
            0,
            Some(1),
        ),
        (
            vec!["test result: FAILED. 0 passed; 1 failed; 0 ignored;"],
            0,
            Some(1),
        ),
        (
            vec!["test result: ok. 0 passed; 0 failed; 1 ignored;"],
            0,
            Some(1),
        ),
        (
            vec!["test result: ok. 1 passed; 0 failed; 1 ignored;"],
            0,
            Some(1),
        ),
        (
            vec![
                "test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 42 filtered out; finished in 0.00s",
            ],
            0,
            Some(1),
        ),
        (vec![passed], 1, Some(1)),
        (vec![], 0, Some(1)),
        (vec![passed, passed], 0, Some(1)),
        (
            vec![
                passed,
                "test result: FAILED. 0 passed; 1 failed; 0 ignored;",
            ],
            0,
            Some(1),
        ),
        (
            vec!["test result: ok. 1 passed; 0 failed; 1 ignored;"],
            0,
            None,
        ),
        (
            vec![
                "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 42 filtered out; finished in 0.00s; test result: FAILED",
            ],
            0,
            Some(1),
        ),
    ];
    let count = u32::try_from(rejected.len() + 2).map_err(|error| error.to_string())?;
    for (output, code, expected) in rejected {
        let output = output.into_iter().map(String::from).collect::<Vec<_>>();
        require(
            test_output(&output, code, expected).is_err(),
            "Test-output negative fixture was admitted.",
        )?;
    }
    Ok(count)
}

pub(super) fn run(arguments: &[OsString]) -> Result<String> {
    require(
        arguments.len() == 3 && arguments[1] == "--json",
        "Use native-acceptance-guard --json REQUEST.",
    )?;
    let request = arguments[2]
        .to_str()
        .ok_or("Guard request must be UTF-8.")?;
    require(
        request.len() <= 1024 * 1024,
        "Guard request exceeds its one-MiB bound.",
    )?;
    let request: Request = serde_json::from_str(request).map_err(|error| error.to_string())?;
    let result = match request {
        Request::Analyzer {
            receipt,
            revision,
            compiler_identity,
        } => {
            analyzer(receipt, &revision, &compiler_identity)?;
            json!({"SchemaVersion":1,"Success":true,"Revision":revision})
        }
        Request::TestOutput {
            output,
            exit_code,
            expected_passed,
        } => {
            json!({"SchemaVersion":1,"Success":true,"Passed":test_output(&output,exit_code,expected_passed)?})
        }
        Request::SelfCheck { revision } => {
            require(
                lower_hex(&revision, 40),
                "Self-check receipt requires an exact source revision.",
            )?;
            json!({"SchemaVersion":1,"Success":true,"Revision":revision,
                "ReceiptFixtures":receipt_fixtures()?,"OutputFixtures":output_fixtures()?})
        }
        Request::WorkflowReceipt {
            receipt,
            revision,
            sources,
        } => {
            workflow_receipt(receipt, &revision, sources)?;
            json!({"SchemaVersion":1,"Success":true,"Revision":revision})
        }
    };
    serde_json::to_string(&result).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sources() -> Vec<Source> {
        SOURCES
            .iter()
            .map(|path| Source {
                path: (*path).into(),
                bytes: 1,
                sha256: "2".repeat(64),
            })
            .collect()
    }

    fn receipt() -> Value {
        json!({"SchemaVersion":1,"Kind":"native-acceptance-workflow-contract","Revision":"1".repeat(40),
            "Success":true,"ReceiptFixtures":23,"OutputFixtures":13,"ParsedPowerShellBlocks":POWERSHELL_BLOCKS,
            "ParsedLauncher":true,"ExactRustControls":CONTROLS,"SourceDigestScope":"utf8-no-bom-lf","Sources":sources()})
    }

    #[test]
    fn negative_fixtures_cover_receipts_and_test_output() -> Result<()> {
        assert_eq!(receipt_fixtures()?, 23);
        assert_eq!(output_fixtures()?, 13);
        assert!(
            run(&[
                "native-acceptance-guard".into(),
                "--unknown".into(),
                "{}".into()
            ])
            .is_err()
        );
        assert!(run(&["native-acceptance-guard".into(),"--json".into(),
            r#"{"operation":"self-check","revision":"1111111111111111111111111111111111111111","extra":true}"#.into()]).is_err());
        Ok(())
    }

    #[test]
    fn workflow_receipt_requires_source_bound_complete_controls() -> Result<()> {
        let valid = receipt();
        let revision = "1".repeat(40);
        workflow_receipt(valid.clone(), &revision, sources())?;
        let without_action: Vec<_> = sources()
            .into_iter()
            .filter(|source| source.path != ".github/actions/native-regressions/action.yml")
            .collect();
        let mut missing_action = valid.clone();
        missing_action["Sources"] = json!(without_action);
        assert!(workflow_receipt(missing_action, &revision, without_action).is_err());
        for (pointer, replacement) in [
            ("/Success", json!(false)),
            ("/Success", json!("true")),
            ("/Revision", json!("0".repeat(40))),
            ("/ReceiptFixtures", json!(22)),
            ("/OutputFixtures", json!(12)),
            ("/ParsedPowerShellBlocks", json!(POWERSHELL_BLOCKS - 1)),
            ("/ParsedLauncher", json!(false)),
            ("/ExactRustControls", json!([])),
            ("/SourceDigestScope", json!("raw-file-bytes")),
            ("/Sources/0/Sha256", json!("3".repeat(64))),
            ("/Sources/0/Bytes", json!(2)),
            ("/Sources/0/Sha256", json!("not-a-sha256")),
            ("/Sources/0/Path", json!("another-file")),
            ("/Sources", json!([])),
        ] {
            let mut altered = valid.clone();
            *altered
                .pointer_mut(pointer)
                .ok_or("Missing contract fixture field.")? = replacement;
            assert!(
                workflow_receipt(altered, &revision, sources()).is_err(),
                "{pointer}"
            );
        }
        Ok(())
    }
}
