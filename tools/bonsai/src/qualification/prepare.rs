//! Native preparation for the full hosted qualification matrices.

use serde_json::{Map, Value};

fn hosted_policy(environment: &str) -> crate::Result<()> {
    if environment != "github-hosted" {
        return Err("Full qualification matrices require hosted CI; the five-minute owner gate remains pending. Use the nonlaunching plan operation.".into());
    }
    Ok(())
}

pub(super) fn require_hosted_capture() -> crate::Result<()> {
    hosted_policy(&std::env::var("RUNNER_ENVIRONMENT").unwrap_or_default())?;
    for name in [
        "ACTIONS_RUNTIME_TOKEN",
        "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
        "GITHUB_TOKEN",
        "GH_TOKEN",
        "ACTIONS_RUNNER_INPUT_TOKEN",
    ] {
        if std::env::var_os(name).is_some() {
            return Err("Qualification requires CI credentials omitted from the complete evaluation process tree.".into());
        }
    }
    Ok(())
}

pub(super) fn run(request: Map<String, Value>, screen: bool) -> crate::Result<Value> {
    require_hosted_capture()?;
    #[cfg(windows)]
    {
        windows::run(Value::Object(request), screen)
    }
    #[cfg(not(windows))]
    {
        let _ = (request, screen);
        Err("Qualification preparation requires native Windows ARM64.".into())
    }
}

#[cfg(windows)]
mod windows;

#[cfg(test)]
mod tests {
    use super::hosted_policy;

    #[test]
    fn full_matrices_cannot_consume_the_owner_allowance() {
        assert!(hosted_policy("github-hosted").is_ok());
        for environment in ["", "self-hosted", "owner", "GitHub-Hosted"] {
            assert!(hosted_policy(environment).is_err());
        }
    }
}
