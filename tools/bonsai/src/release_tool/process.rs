//! Contained release tools and bounded exact-private-directory cleanup.
use super::{Result, path, resolve, text};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    path::Path,
    time::{Duration, Instant},
};

fn scan(root: &Path, deadline: Option<Instant>, entries: &mut usize) -> Result<()> {
    resolve(root, true, false)?;
    for entry in fs::read_dir(root).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        *entries += 1;
        if *entries > 100_000 || deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Err("Release cleanup traversal exceeded its bound.".into());
        }
        let metadata = fs::symlink_metadata(entry.path()).map_err(|error| error.to_string())?;
        if bonsai_artifacts::is_redirect(&metadata) {
            return Err("Redirected release cleanup entry.".into());
        }
        if metadata.is_dir() {
            scan(&entry.path(), deadline, entries)?;
        }
        #[cfg(windows)]
        if metadata.permissions().readonly() {
            let mut permissions = metadata.permissions();
            #[expect(
                clippy::permissions_set_readonly_false,
                reason = "Windows-only private release cleanup clears the DOS readonly flag; this branch never changes Unix permissions."
            )]
            permissions.set_readonly(false);
            fs::set_permissions(entry.path(), permissions).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

pub(super) fn cleanup(value: &Value) -> Result<Value> {
    let root = resolve(&path(value, "path")?, true, true)?;
    let parent = resolve(&path(value, "parentDirectory")?, true, false)?;
    let leaf = text(value, "leafName")?;
    if leaf.is_empty()
        || leaf == "."
        || leaf == ".."
        || leaf.contains(['/', '\\', ':'])
        || root.file_name().and_then(|s| s.to_str()) != Some(leaf)
        || root.parent().is_none_or(|actual| {
            !actual
                .to_string_lossy()
                .eq_ignore_ascii_case(&parent.to_string_lossy())
        })
    {
        return Err("Refusing cleanup outside exact private release directory.".into());
    }
    let timeout = allowance(value, "timeoutMilliseconds", 5000)?;
    if timeout > 10_000 {
        return Err("Release cleanup allowance exceeds bound.".into());
    }
    let started = Instant::now();
    loop {
        match fs::symlink_metadata(&root) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Value::Null),
            Err(error) => return Err(error.to_string()),
            Ok(_) => {}
        }
        scan(
            &root,
            (timeout > 0).then(|| started + Duration::from_millis(timeout)),
            &mut 0,
        )?;
        match fs::remove_dir_all(&root) {
            Ok(()) => return Ok(Value::Null),
            Err(error)
                if matches!(error.raw_os_error(), Some(32 | 33))
                    && started.elapsed() < Duration::from_millis(timeout) =>
            {
                let remaining = Duration::from_millis(timeout).saturating_sub(started.elapsed());
                std::thread::sleep(remaining.min(Duration::from_millis(100)));
                if started.elapsed() >= Duration::from_millis(timeout) {
                    return Err(error.to_string());
                }
            }
            Err(error) => return Err(error.to_string()),
        }
    }
}

fn excerpt(text: &str) -> String {
    if text.trim().is_empty() {
        return "<empty>".into();
    }
    if text.chars().count() <= 4096 {
        return text.into();
    }
    let start: String = text.chars().take(2048).collect();
    let end: String = text
        .chars()
        .rev()
        .take(2048)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{start}\n[... output truncated ...]\n{end}")
}

fn executable_name(name: &str) -> OsString {
    let path = Path::new(name);
    let mut executable = OsString::from(name);
    if cfg!(windows)
        && !name.contains(['/', '\\'])
        && path.components().count() == 1
        && path.file_name().is_some()
        && path.extension().is_none()
    {
        // The native resolver searches exact PATH entries without shell/PATHEXT
        // expansion. Release adapters use bare cargo, gh and git names.
        executable.push(".exe");
    }
    executable
}

pub(super) fn invoke(value: &Value) -> Result<Value> {
    let executable = executable_name(text(value, "filePath")?);
    let args = value["arguments"]
        .as_array()
        .ok_or("Release arguments must be an array.")?
        .iter()
        .map(|arg| {
            arg.as_str()
                .map(OsString::from)
                .ok_or_else(|| "Release arguments must be strings.".into())
        })
        .collect::<Result<Vec<_>>>()?;
    let environment: BTreeMap<String, String> = serde_json::from_value(
        value
            .get("environment")
            .cloned()
            .unwrap_or_else(|| json!({})),
    )
    .map_err(|error| error.to_string())?;
    let seconds = allowance(value, "timeoutSeconds", 180)?;
    if !(1..=600).contains(&seconds) {
        return Err("Release command allowance exceeds bound.".into());
    }
    let timeout = if value["buildCommand"].as_bool() == Some(true) {
        crate::startup_process::build_timeout(Duration::from_secs(seconds))
    } else {
        Duration::from_secs(seconds)
    };
    let working = std::env::temp_dir().join(format!(
        "SwiftBonsai-Release-Command-{}",
        crate::startup_process::unique()
    ));
    fs::create_dir(&working).map_err(|error| error.to_string())?;
    let log = working.join("command.log");
    let result =
        crate::startup_process::run(&executable, &args, &working, &environment, timeout, &log);
    match result {
        Ok(output) => {
            // Captures are private to this native invocation. Preserve any
            // additional child-created diagnostics instead of erasing them.
            for entry in fs::read_dir(&working).map_err(|error| error.to_string())? {
                let entry = entry.map_err(|error| error.to_string())?;
                if entry.path() == log
                    || entry
                        .path()
                        .extension()
                        .is_some_and(|ext| ext == "stdout" || ext == "stderr")
                {
                    fs::remove_file(entry.path()).map_err(|error| error.to_string())?;
                }
            }
            if fs::read_dir(&working)
                .map_err(|error| error.to_string())?
                .next()
                .is_none()
            {
                fs::remove_dir(&working).map_err(|error| error.to_string())?;
            }
            if output.code != 0 {
                return Err(format!(
                    "Release command failed with exit code {}: {}\nstdout: {}\nstderr: {}",
                    output.code,
                    executable.to_string_lossy(),
                    excerpt(&output.stdout),
                    excerpt(&output.stderr)
                ));
            }
            Ok(json!({"stdout":output.stdout,"stderr":output.stderr}))
        }
        Err(error) => {
            if fs::read_dir(&working).is_ok_and(|mut entries| entries.next().is_none()) {
                fs::remove_dir(&working).map_err(|cleanup| {
                    format!("{error}; empty scratch cleanup failed: {cleanup}")
                })?;
            }
            Err(error)
        }
    }
}

fn allowance(value: &Value, field: &str, default: u64) -> Result<u64> {
    value.get(field).map_or(Ok(default), |value| {
        value
            .as_u64()
            .ok_or_else(|| format!("Release {field} must be an unsigned integer."))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn release_command_executable_names_normalize_only_bare_windows_tools() {
        for name in ["cargo", "gh", "git"] {
            let expected = if cfg!(windows) {
                format!("{name}.exe")
            } else {
                name.into()
            };
            assert_eq!(executable_name(name), OsString::from(expected));
        }
        for name in [
            "git.exe",
            "cargo.EXE",
            "helper.cmd",
            "tools/cargo",
            "cargo/",
            "./cargo",
            "/usr/bin/cargo",
            "",
            ".",
            "..",
        ] {
            assert_eq!(executable_name(name), OsString::from(name));
        }
    }

    #[cfg(windows)]
    #[test]
    fn release_command_executable_names_preserve_windows_paths() {
        for name in [
            r"C:\tools\cargo",
            r"\\server\share\cargo",
            r"tools\cargo",
            "cargo\\",
            r".\cargo",
            r"C:git",
        ] {
            assert_eq!(executable_name(name), OsString::from(name));
        }
    }

    #[test]
    fn release_command_diagnostics_preserve_unicode_head_tail_and_empty_output() {
        assert_eq!(excerpt(" \n"), "<empty>");
        assert_eq!(excerpt("output\n"), "output\n");
        let text = format!("first{}last", "\u{1f333}".repeat(6000));
        let result = excerpt(&text);
        assert!(result.starts_with("first"));
        assert!(result.ends_with("last"));
        assert!(result.contains("[... output truncated ...]"));
    }
    #[test]
    fn release_cleanup_rejects_wrong_parent_and_leaf_before_deleting() -> Result<()> {
        let root =
            std::env::temp_dir().join(format!("release-path-{}", crate::startup_process::unique()));
        fs::create_dir(&root).map_err(|e| e.to_string())?;
        let keep = root.join("keep");
        fs::create_dir(&keep).map_err(|e| e.to_string())?;
        assert!(
            cleanup(&json!({"path":keep,"parentDirectory":root,"leafName":"different"})).is_err()
        );
        assert!(keep.exists());
        fs::remove_dir(&keep)
            .and_then(|()| fs::remove_dir(&root))
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn release_cleanup_retries_only_sharing_and_preserves_a_persistent_locked_file() -> Result<()> {
        use std::os::windows::fs::OpenOptionsExt as _;
        let parent = std::env::temp_dir();
        let leaf = format!("release-lock-{}", crate::startup_process::unique());
        let root = parent.join(&leaf);
        fs::create_dir(&root).map_err(|error| error.to_string())?;
        let file = root.join("locked");
        fs::write(&file, b"inert").map_err(|error| error.to_string())?;
        let hold = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&file)
            .map_err(|error| error.to_string())?;
        let started = Instant::now();
        assert!(cleanup(&json!({"path":root,"parentDirectory":parent,"leafName":leaf,"timeoutMilliseconds":50})).is_err());
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(
            fs::read(&file).map_err(|error| error.to_string())?,
            b"inert"
        );
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            drop(hold);
        });
        cleanup(
            &json!({"path":root,"parentDirectory":parent,"leafName":leaf,"timeoutMilliseconds":2000}),
        )?;
        release
            .join()
            .map_err(|_| "Fixture unlock thread failed.")?;
        assert!(!root.exists());
        Ok(())
    }
}
