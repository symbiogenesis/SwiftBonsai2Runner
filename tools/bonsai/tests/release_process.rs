//! Native release process controls; no model, shell, network or installation.
#![forbid(unsafe_code)]
#![cfg(all(windows, feature = "test-probe"))]
use serde_json::json;

fn invoke(mode: &str) -> bonsai_tools::Result<serde_json::Value> {
    bonsai_tools::release_tool::run(
        &json!({"operation":"invoke","filePath":env!("CARGO_BIN_EXE_bonsai-release-test-probe"),"arguments":[mode]}),
    )
}

#[test]
fn release_command_preserves_exact_unicode_success_and_separate_stderr() -> bonsai_tools::Result<()>
{
    let output = invoke("success")?;
    assert_eq!(output["stdout"], "exact-\u{03b2}\nsecond line\n");
    assert_eq!(output["stderr"], "success diagnostic-\u{03b2}\n");
    Ok(())
}
#[test]
fn release_command_preserves_failure_exit_codes_and_explicit_empty_output() {
    let failure = invoke("failure").err().unwrap_or_default();
    assert!(failure.contains("exit code 17:"));
    assert!(failure.contains("stdout: Native diagnostic-\u{03b2}"));
    assert!(failure.contains("stderr: stderr diagnostic-\u{03b2}"));
    let empty = invoke("empty").err().unwrap_or_default();
    assert!(empty.contains("exit code 19:"));
    assert!(empty.contains("stdout: <empty>"));
    assert!(empty.contains("stderr: <empty>"));
}
#[test]
fn release_command_retains_bounded_first_and_last_failure_context() {
    let large = invoke("large").err().unwrap_or_default();
    for marker in [
        "exit code 23:",
        "stdout-head",
        "stdout-tail",
        "stderr-head",
        "stderr-tail",
        "[... output truncated ...]",
    ] {
        assert!(large.contains(marker));
    }
    assert!(large.len() < 9000);
}
