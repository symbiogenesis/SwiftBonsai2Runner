//! Nonlaunching server policy shared with the broker's fixed invocation.

/// Validate environment names or prepare the fixed, launch-disabled server plan.
///
/// This low-level operation performs no filesystem or process actions; use
/// [`plan`] for native path validation and optional offline artifact verification.
///
/// # Errors
/// Rejects malformed requests, inherited overrides, unsafe ports or profile drift.
pub fn run(request: serde_json::Value) -> crate::Result<serde_json::Value> {
    bonsai_cpu_policy::server::run(request)
}

/// Prepare the fixed reference server plan without starting inference.
///
/// `verification` is `none`, `runtime` or `artifacts`; the latter includes the
/// existing offline model. Verification never provisions files or creates keys.
///
/// # Errors
/// Rejects unsafe data storage, inherited overrides, unsupported hosts, missing
/// or changed artifacts and server help missing any required exact switch.
pub fn plan(request: serde_json::Value) -> crate::Result<serde_json::Value> {
    #[cfg(windows)]
    {
        windows::plan(request)
    }
    #[cfg(not(windows))]
    {
        let _ = request;
        Err("Server planning requires Windows.".into())
    }
}

#[cfg(any(windows, test))]
fn verify_help(help: &str, arguments: &[serde_json::Value]) -> crate::Result<()> {
    let mut count = 0;
    for argument in arguments {
        let flag = argument.as_str().ok_or("Invalid server argument.")?;
        if !flag.starts_with("--") {
            continue;
        }
        count += 1;
        let word =
            |character: char| character.is_alphanumeric() || character == '_' || character == '-';
        if !help.match_indices(flag).any(|(index, _)| {
            help[..index]
                .chars()
                .next_back()
                .is_none_or(|character| !word(character))
                && help[index + flag.len()..]
                    .chars()
                    .next()
                    .is_none_or(|character| !word(character))
        }) {
            return Err(format!(
                "Pinned server help does not advertise required switch: {flag}"
            ));
        }
    }
    if count == 0 {
        return Err("No server switches to verify.".into());
    }
    Ok(())
}

#[cfg(windows)]
mod windows {
    use serde::Deserialize;
    use serde_json::{Value, json};
    use std::{
        path::PathBuf,
        time::{Duration, Instant},
    };

    #[derive(Default, Deserialize, PartialEq, Eq)]
    #[serde(rename_all = "lowercase")]
    enum Verification {
        #[default]
        None,
        Runtime,
        Artifacts,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Request {
        data_directory: PathBuf,
        #[serde(default = "port")]
        port: u16,
        #[serde(default)]
        verification: Verification,
        parent_process_id: Option<u32>,
        parent_creation_time_file_time: Option<u64>,
    }
    const fn port() -> u16 {
        8080
    }

    pub(super) fn plan(value: Value) -> crate::Result<Value> {
        let started = Instant::now();
        let request: Request = serde_json::from_value(value).map_err(|error| error.to_string())?;
        bonsai_windows_process::require_local_data_drive(&request.data_directory)
            .map_err(|error| error.to_string())?;
        bonsai_artifacts::validate_external_path(&request.data_directory)
            .map_err(|error| error.to_string())?;
        let cpu = bonsai_cpu_policy::run(
            json!({"operation": "plan", "dataDirectory": request.data_directory,
            "profile": "reference-cpu", "useAbliterateLora": false, "useRepackedPtq1": false}),
        )?;
        let lock = &cpu["Lock"];
        let profile: Value =
            serde_json::from_str(include_str!("../../../config/profiles/reference-cpu.json"))
                .map_err(|error| error.to_string())?;
        let key = request.data_directory.join("private/server.key");
        bonsai_artifacts::validate_external_path(&key).map_err(|error| error.to_string())?;
        let mut result = super::run(json!({"operation": "plan", "profile": profile,
            "modelPath": cpu["ModelPath"], "keyFile": key, "port": request.port,
            "runtimeCommit": lock["runtime"]["commit"], "modelRevision": lock["model"]["revision"],
            "modelBytes": lock["model"]["bytes"],
            "environmentNames": std::env::vars_os().map(|(name, _)| name.to_string_lossy().into_owned()).collect::<Vec<_>>() }))?;
        if request.verification == Verification::None {
            return Ok(result);
        }
        if !cfg!(target_arch = "aarch64") {
            return Err("Runtime probes require native ARM64 Windows.".into());
        }
        let (id, creation) = crate::cpu_check::parent_identity(
            request.parent_process_id,
            request.parent_creation_time_file_time,
        )?;
        let runtime = crate::runtime::run(json!({"schemaVersion": 1,
            "runtime": crate::cpu_check::runtime_spec(&request.data_directory, lock)?,
            "commit": lock["runtime"]["commit"], "includeHelp": true,
            "parentProcessId": id, "parentCreationTimeFileTime": creation}))?;
        if request.verification == Verification::Artifacts {
            let parent = bonsai_windows_process::ParentProcess::open(id, creation)
                .map_err(|error| error.to_string())?;
            let model = PathBuf::from(cpu["ModelPath"].as_str().ok_or("Missing model path.")?);
            let _guard = bonsai_artifacts::VerifiedFile::verify_with(
                &model,
                lock["model"]["bytes"]
                    .as_u64()
                    .ok_or("Missing model length.")?,
                lock["model"]["sha256"]
                    .as_str()
                    .ok_or("Missing model hash.")?,
                || {
                    if started.elapsed() >= Duration::from_mins(4) || parent.ensure_alive().is_err()
                    {
                        return Err(bonsai_artifacts::ArtifactError::Cancelled);
                    }
                    Ok(())
                },
            )
            .map_err(|error| error.to_string())?;
        }
        super::verify_help(
            runtime["RuntimeHelp"]
                .as_str()
                .ok_or("Missing runtime help.")?,
            result["Arguments"]
                .as_array()
                .ok_or("Missing server arguments.")?,
        )?;
        result["ArtifactVerification"] =
            json!(if request.verification == Verification::Artifacts {
                "existing-model-and-runtime-size-sha256-and-arm64-version-verified"
            } else {
                "runtime-size-sha256-and-arm64-version-verified; model-not-checked"
            });
        result["RuntimeHelpVerification"] =
            json!("all-planned-switches-advertised; values/enforcement-not-qualified");
        result["ServerPath"] = runtime["ServerPath"].clone();
        result["MemoryAdmission"] = crate::cpu_check::run(
            json!({"dataDirectory": request.data_directory, "checkOnly": true}),
        )?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn help_requires_complete_switch_tokens() -> crate::Result<()> {
        let arguments = [json!("--no-agent"), json!("--host"), json!("127.0.0.1")];
        super::verify_help("--host HOST --no-agent", &arguments)?;
        for help in [
            "--no-agent-unsafe --host",
            "--no-agent --hostname",
            "--host",
        ] {
            assert!(super::verify_help(help, &arguments).is_err());
        }
        assert!(super::verify_help("anything", &[json!("value")]).is_err());
        Ok(())
    }
}
