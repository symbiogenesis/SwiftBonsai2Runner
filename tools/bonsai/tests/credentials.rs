//! Public command shape checks before any credential filesystem operation.
#![forbid(unsafe_code)]

use bonsai_tools::credentials;
use serde_json::json;

#[test]
fn credential_command_rejects_positional_and_unknown_fields() {
    assert!(credentials::run(json!(["C:\\private"])).is_err());
    assert!(credentials::run(json!({"parentDirectory": ".", "reuse": true})).is_err());
    assert!(credentials::run(json!({})).is_err());
}

#[cfg(windows)]
#[test]
#[expect(
    clippy::manual_assert_eq,
    reason = "Credential bytes and environment secrets must not enter assertion diagnostics."
)]
fn native_credentials_reject_unsafe_parents_and_preserve_path_only_receipts()
-> Result<(), Box<dyn std::error::Error>> {
    use std::{
        fs,
        path::Path,
        time::{SystemTime, UNIX_EPOCH},
    };

    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let root =
        std::env::temp_dir().join(format!("Bonsai-Credentials-{}-{nonce}", std::process::id()));
    fs::create_dir(&root)?;
    fs::create_dir(root.join("repository"))?;
    fs::write(root.join("repository/.git"), "gitdir: elsewhere")?;
    for parent in [
        Path::new("."),
        Path::new("%TEMP%/keys"),
        Path::new("\\\\localhost\\share"),
        &root.join("absent"),
        &root.join("repository"),
    ] {
        assert!(credentials::run(json!({"parentDirectory":parent})).is_err());
    }
    let environment: std::collections::BTreeMap<_, _> = std::env::vars_os().collect();
    let result = credentials::run(json!({"parentDirectory":root}))?;
    assert_eq!(result["Status"], "development-keys-created");
    let frontend = fs::read_to_string(
        result["FrontendKeyFile"]
            .as_str()
            .ok_or("Missing frontend path.")?,
    )?;
    let backend = fs::read_to_string(
        result["BackendKeyFile"]
            .as_str()
            .ok_or("Missing backend path.")?,
    )?;
    // Never expose credential bytes through an assertion's debug representation.
    assert!(frontend.len() == 64 && backend.len() == 64 && frontend != backend);
    assert!(!result.to_string().contains(&frontend) && !result.to_string().contains(&backend));
    let second = credentials::run(json!({"parentDirectory":root}))?;
    assert_ne!(result["Directory"], second["Directory"]);
    assert!(
        fs::read_to_string(
            result["FrontendKeyFile"]
                .as_str()
                .ok_or("Missing frontend path.")?
        )? == frontend
    );
    assert!(std::env::vars_os().collect::<std::collections::BTreeMap<_, _>>() == environment);
    fs::remove_dir_all(root)?;
    Ok(())
}
