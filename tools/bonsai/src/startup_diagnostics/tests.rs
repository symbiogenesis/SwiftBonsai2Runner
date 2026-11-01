use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> crate::Result<Self> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "Bonsai-Startup-Diagnostics-{}-{nonce}",
            std::process::id()
        ));
        validate_external_path(&root).map_err(|error| error.to_string())?;
        fs::create_dir(&root).map_err(|error| error.to_string())?;
        Ok(Self(root))
    }

    fn session(&self, digit: char) -> crate::Result<PathBuf> {
        let directory = self
            .0
            .join(format!("SwiftBonsai-Keys-{}", digit.to_string().repeat(32)));
        fs::create_dir(&directory).map_err(|error| error.to_string())?;
        for (name, key) in [("frontend.key", "a"), ("backend.key", "b")] {
            fs::write(directory.join(name), key.repeat(64)).map_err(|error| error.to_string())?;
        }
        Ok(directory)
    }

    fn collect(&self) -> crate::Result<Value> {
        run(json!({"evidenceDirectory": self.0}))
    }

    fn receipt(&self) -> crate::Result<Value> {
        serde_json::from_slice(
            &fs::read(self.0.join(PUBLIC_FILES[2])).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Only the unique, externally validated fixture tree is ever removed.
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn failed_acceptance_preserves_typed_workload_result_without_qualifying_logs() -> crate::Result<()>
{
    let fixture = Fixture::new()?;
    let path = fixture.0.join("acceptance.json");
    assert!(fixture.collect()?["AllRequestedWorkloadsPassed"].is_null());
    for passed in [false, true] {
        fs::write(
            &path,
            serde_json::to_vec(&json!({"schemaVersion":1,
            "allRequestedWorkloadsPassed":passed, "private":"not-for-public-output"}))
            .map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        let result = run(json!({"evidenceDirectory":fixture.0,
            "acceptanceExitCode":101, "budgetExpired":true}))?;
        assert_eq!(result["AllRequestedWorkloadsPassed"], passed);
        assert_eq!(result["Ready"], false);
        assert_eq!(result["Classification"], "no-protected-worker-directory");
        assert!(!result.to_string().contains("not-for-public-output"));
    }
    for text in [
        "{}",
        "[]",
        "[1,true]",
        "not-json",
        "{\"schemaVersion\":1}",
        "{\"schemaVersion\":2,\"allRequestedWorkloadsPassed\":true}",
        "{\"schemaVersion\":1,\"allRequestedWorkloadsPassed\":null}",
        "{\"schemaVersion\":1,\"allRequestedWorkloadsPassed\":\"false\"}",
        "{\"schemaVersion\":1,\"allRequestedWorkloadsPassed\":0}",
        "{\"schemaVersion\":1,\"allRequestedWorkloadsPassed\":false,\"allRequestedWorkloadsPassed\":true}",
    ] {
        fs::write(&path, text).map_err(|error| error.to_string())?;
        assert!(fixture.collect()?["AllRequestedWorkloadsPassed"].is_null());
    }
    fs::write(&path, vec![b' '; MAX_ACCEPTANCE_BYTES + 1]).map_err(|error| error.to_string())?;
    assert!(fixture.collect()?["AllRequestedWorkloadsPassed"].is_null());
    fs::remove_file(&path).map_err(|error| error.to_string())?;
    fs::create_dir(&path).map_err(|error| error.to_string())?;
    assert!(fixture.collect()?["AllRequestedWorkloadsPassed"].is_null());
    Ok(())
}

#[test]
fn logs_redact_every_session_key_and_unknown_hex_without_traversing_nested_files()
-> crate::Result<()> {
    let fixture = Fixture::new()?;
    let first = fixture.session('1')?;
    let second = fixture.session('2')?;
    fs::write(second.join("frontend.key"), "c".repeat(64)).map_err(|error| error.to_string())?;
    fs::write(second.join("backend.key"), "d".repeat(64)).map_err(|error| error.to_string())?;
    fs::write(
        first.join("backend.stdout"),
        format!(
            "ready {} {} {} {}\nUnicode: 森 🌲\n",
            "a".repeat(64),
            "B".repeat(64),
            "c".repeat(64),
            "e".repeat(64)
        ),
    )
    .map_err(|error| error.to_string())?;
    fs::write(
        first.join("backend.stderr"),
        "fixed startup failure\n\u{1b}control",
    )
    .map_err(|error| error.to_string())?;
    fs::write(second.join("backend.stdout"), []).map_err(|error| error.to_string())?;
    fs::write(second.join("backend.stderr"), []).map_err(|error| error.to_string())?;
    fs::create_dir(first.join("nested")).map_err(|error| error.to_string())?;
    fs::write(first.join("nested/backend.stderr"), "MUST-NOT-BE-COPIED")
        .map_err(|error| error.to_string())?;
    let result = run(json!({"evidenceDirectory": fixture.0, "acceptanceExitCode": 101}))?;
    assert_eq!(result["Ready"], true);
    assert_eq!(result["Classification"], "bounded-sanitized-logs-retained");
    let receipt = fixture.receipt()?;
    assert_eq!(receipt["AcceptanceProcessExitCode"], 101);
    assert!(receipt.get("BackendExitCode").is_none());
    assert_eq!(receipt["Sources"].as_array().map(Vec::len), Some(4));
    assert_eq!(receipt["PublishedFiles"].as_array().map(Vec::len), Some(2));
    let mut public = String::new();
    for name in PUBLIC_FILES {
        public.push_str(
            &fs::read_to_string(fixture.0.join(name)).map_err(|error| error.to_string())?,
        );
    }
    for secret in ["a", "b", "c", "d", "e"] {
        assert!(!public.to_ascii_lowercase().contains(&secret.repeat(64)));
    }
    assert!(!public.contains("MUST-NOT-BE-COPIED"));
    assert!(public.contains("ready [REDACTED]"));
    assert!(public.contains("fixed startup failure"));
    assert!(public.contains("Unicode: 森 🌲"));
    assert!(!public.contains('\u{1b}'));
    for file in receipt["PublishedFiles"]
        .as_array()
        .ok_or("Missing files.")?
    {
        let bytes = fs::read(
            fixture
                .0
                .join(file["Name"].as_str().ok_or("Missing name.")?),
        )
        .map_err(|error| error.to_string())?;
        assert_eq!(file["Bytes"], bytes.len());
        assert_eq!(file["Sha256"], crate::identity::sha256(&bytes));
    }
    Ok(())
}

#[test]
fn oversized_and_invalid_utf8_logs_never_enable_publication() -> crate::Result<()> {
    let fixture = Fixture::new()?;
    let session = fixture.session('1')?;
    fs::write(session.join("backend.stdout"), vec![0; MAX_LOG_BYTES + 1])
        .map_err(|error| error.to_string())?;
    fs::write(session.join("backend.stderr"), [0xc3, 0x28]).map_err(|error| error.to_string())?;
    let result = run(json!({"evidenceDirectory": fixture.0, "budgetExpired": true}))?;
    assert_eq!(result["Ready"], false);
    let receipt = fixture.receipt()?;
    assert_eq!(receipt["AggregateBudgetExpired"], true);
    assert_eq!(receipt["Classification"], "no-log-published");
    assert_eq!(
        receipt["Sources"][0]["Classification"],
        "read-bound-exceeded"
    );
    assert_eq!(receipt["Sources"][1]["Classification"], "invalid-utf8");
    assert!(!fixture.0.join(PUBLIC_FILES[0]).exists());
    assert!(!fixture.0.join(PUBLIC_FILES[1]).exists());
    Ok(())
}

#[test]
fn invalid_actual_keys_remove_stale_logs_and_close_the_complete_collection() -> crate::Result<()> {
    for invalid in [
        None,
        Some("B".repeat(64)),
        Some("b".repeat(65)),
        Some("short".into()),
    ] {
        let fixture = Fixture::new()?;
        let session = fixture.session('1')?;
        if let Some(invalid) = invalid {
            fs::write(session.join("frontend.key"), invalid).map_err(|error| error.to_string())?;
        } else {
            fs::remove_file(session.join("frontend.key")).map_err(|error| error.to_string())?;
        }
        fs::write(session.join("backend.stderr"), "b".repeat(64))
            .map_err(|error| error.to_string())?;
        fs::write(fixture.0.join(PUBLIC_FILES[1]), "stale").map_err(|error| error.to_string())?;
        let result = fixture.collect()?;
        assert_eq!(result["Ready"], false);
        assert_eq!(result["Classification"], "key-validation-failed");
        assert!(!fixture.0.join(PUBLIC_FILES[1]).exists());
        assert_eq!(fixture.receipt()?["PublishedFiles"], json!([]));
    }
    Ok(())
}

#[test]
fn fixed_session_bound_and_empty_inventory_suppress_logs() -> crate::Result<()> {
    let fixture = Fixture::new()?;
    assert_eq!(
        fixture.collect()?["Classification"],
        "no-protected-worker-directory"
    );
    for digit in '1'..='9' {
        fixture.session(digit)?;
    }
    let result = fixture.collect()?;
    assert_eq!(result["Ready"], false);
    assert_eq!(result["Classification"], "session-bound-exceeded");
    assert_eq!(fixture.receipt()?["PublishedFiles"], json!([]));
    Ok(())
}

#[test]
fn output_directories_remain_untouched_and_preflight_changes_nothing() -> crate::Result<()> {
    for name in PUBLIC_FILES {
        let fixture = Fixture::new()?;
        fixture.session('1')?;
        let unsafe_output = fixture.0.join(name);
        fs::create_dir(&unsafe_output).map_err(|error| error.to_string())?;
        fs::write(unsafe_output.join("private.key"), "b".repeat(64))
            .map_err(|error| error.to_string())?;
        let result = fixture.collect()?;
        assert_eq!(result["Ready"], false);
        assert_eq!(result["Classification"], "unsafe-output");
        assert_eq!(
            fs::read_to_string(unsafe_output.join("private.key"))
                .map_err(|error| error.to_string())?,
            "b".repeat(64)
        );
        for other in PUBLIC_FILES.into_iter().filter(|other| *other != name) {
            assert!(!fixture.0.join(other).exists());
        }
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn output_and_root_links_are_rejected_without_touching_their_target() -> crate::Result<()> {
    let target = Fixture::new()?;
    let sentinel = target.0.join("private.key");
    fs::write(&sentinel, "b".repeat(64)).map_err(|error| error.to_string())?;
    for name in PUBLIC_FILES {
        let fixture = Fixture::new()?;
        fixture.session('1')?;
        std::os::unix::fs::symlink(&target.0, fixture.0.join(name))
            .map_err(|error| error.to_string())?;
        assert_eq!(fixture.collect()?["Classification"], "unsafe-output");
        assert_eq!(
            run(json!({"evidenceDirectory": fixture.0.join(name)}))?["Ready"],
            false
        );
        assert_eq!(
            fs::read_to_string(&sentinel).map_err(|error| error.to_string())?,
            "b".repeat(64)
        );
    }
    Ok(())
}

#[cfg(windows)]
#[test]
fn output_and_root_junctions_are_rejected_without_touching_their_target() -> crate::Result<()> {
    use std::os::windows::process::CommandExt as _;

    let target = Fixture::new()?;
    let sentinel = target.0.join("private.key");
    fs::write(&sentinel, "b".repeat(64)).map_err(|error| error.to_string())?;
    for name in PUBLIC_FILES {
        let fixture = Fixture::new()?;
        fixture.session('1')?;
        let link = fixture.0.join(name);
        // mklink /J creates directory junctions without symlink privilege. Each
        // argument is a generated fixture path, never user-controlled content.
        let output = std::process::Command::new("cmd.exe")
            .creation_flags(0x0800_0000)
            .args(["/D", "/C", "mklink", "/J"])
            .arg(&link)
            .arg(&target.0)
            .output()
            .map_err(|error| error.to_string())?;
        if !output.status.success() {
            return Err("Unable to prepare the junction regression fixture.".into());
        }
        assert_eq!(fixture.collect()?["Classification"], "unsafe-output");
        assert_eq!(run(json!({"evidenceDirectory": link}))?["Ready"], false);
        assert_eq!(
            fs::read_to_string(&sentinel).map_err(|error| error.to_string())?,
            "b".repeat(64)
        );
        assert!(is_redirect(
            &fs::symlink_metadata(&link).map_err(|error| error.to_string())?
        ));
        fs::remove_dir(link).map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[test]
fn sanitization_preserves_utf8_whitespace_and_short_hex_with_bounded_long_runs() {
    let keys = vec!["a".repeat(64), "b".repeat(64)];
    assert_eq!(
        sanitize(
            &format!(
                "\u{feff}森\t\n\r\0\u{7f} {} {} Z",
                "B".repeat(64),
                "f".repeat(65)
            ),
            &keys
        ),
        "\u{feff}森\t\n\r?? [REDACTED] [REDACTED] Z"
    );
    assert_eq!(sanitize(&"f".repeat(63), &keys), "f".repeat(63));
}

#[test]
fn missing_log_stays_incomplete_and_invalid_requests_never_reflect_values() -> crate::Result<()> {
    let fixture = Fixture::new()?;
    let session = fixture.session('1')?;
    fs::write(session.join("backend.stdout"), "useful ready marker")
        .map_err(|error| error.to_string())?;
    let result = fixture.collect()?;
    assert_eq!(result["Ready"], false);
    assert_eq!(result["Classification"], "bounded-sanitized-logs-retained");
    assert_eq!(
        fixture.receipt()?["Sources"][1]["Classification"],
        "missing"
    );
    let error = run(json!({"evidenceDirectory": fixture.0, "private": "SECRET"}))
        .err()
        .ok_or("Invalid fields accepted.")?;
    assert!(!error.contains("SECRET"));
    assert_eq!(
        run(json!({"evidenceDirectory": "relative"}))?["Ready"],
        false
    );
    Ok(())
}
