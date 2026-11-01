//! Bounded trusted developer-tool execution without a PowerShell process bridge.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    executable: PathBuf,
    #[serde(default)]
    arguments: Vec<String>,
    working_directory: PathBuf,
    log_path: PathBuf,
    timeout_seconds: u64,
    #[serde(default)]
    environment: BTreeMap<String, String>,
}

/// Runs a trusted compiler or developer tool in an owned process tree.
///
/// This developer command is not an inference or public broker endpoint. The
/// caller selects the executable; model qualification keeps its dedicated gates.
///
/// # Errors
/// Rejects malformed requests, unsafe output paths, excessive deadlines, failed
/// process ownership, bounded-output overflow and surviving descendants.
pub fn run(value: Value) -> crate::Result<Value> {
    let request: Request = serde_json::from_value(value).map_err(|error| error.to_string())?;
    let hosted = std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true")
        && std::env::var("RUNNER_ENVIRONMENT").as_deref() == Ok("github-hosted");
    let maximum = if hosted { 3600 } else { 290 };
    if !(1..=maximum).contains(&request.timeout_seconds)
        || request.arguments.len() > 4096
        || !request.executable.is_absolute()
    {
        return Err("Use an absolute tool and a feasible bounded deadline (owner maximum 290 seconds).".into());
    }
    bonsai_artifacts::validate_external_path(&request.log_path).map_err(|error| error.to_string())?;
    let output = crate::startup_process::run(
        request.executable.as_os_str(),
        &request.arguments.into_iter().map(Into::into).collect::<Vec<_>>(),
        &request.working_directory,
        &request.environment,
        Duration::from_secs(request.timeout_seconds),
        &request.log_path,
    )?;
    Ok(json!({"ExitCode": output.code, "StandardOutput": output.stdout,
        "StandardError": output.stderr, "LogPath": request.log_path}))
}
