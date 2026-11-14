//! Native hosted compiler supervision with one absolute allowance and bounded files.
use crate::Result;
#[cfg(windows)]
use bonsai_windows_process::{OwnedProcess, ToolProcessSpec};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    path::Path,
    thread,
    time::{Duration, Instant},
};
#[cfg(unix)]
use std::{
    os::unix::process::CommandExt as _,
    process::{Child, Command, Stdio},
};

pub(super) struct Spec<'a> {
    pub(super) executable: &'a Path,
    pub(super) arguments: &'a [OsString],
    pub(super) directory: &'a Path,
    pub(super) environment: &'a BTreeMap<String, String>,
    pub(super) removed: &'a [String],
    pub(super) stdout: &'a Path,
    pub(super) stderr: &'a Path,
    pub(super) deadline: Instant,
}
struct Owned {
    #[cfg(windows)]
    child: OwnedProcess,
    #[cfg(unix)]
    child: Child,
    #[cfg(unix)]
    group: nix::unistd::Pid,
    finished: bool,
}
impl Owned {
    fn start(spec: &Spec<'_>) -> Result<Self> {
        #[cfg(windows)]
        let child = OwnedProcess::start_tool_with_removed(
            &ToolProcessSpec {
                executable: spec.executable.to_owned(),
                arguments: spec.arguments.to_vec(),
                working_directory: spec.directory.to_owned(),
                stdout: spec.stdout.to_owned(),
                stderr: spec.stderr.to_owned(),
                environment: spec.environment.clone(),
            },
            spec.removed,
        )
        .map_err(|error| error.to_string())?;
        #[cfg(unix)]
        let child = {
            let _ = spec.removed;
            let capture = |path: &Path| {
                fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create_new(true)
                    .open(path)
                    .map_err(|error| error.to_string())
            };
            Command::new(spec.executable)
                .args(spec.arguments)
                .current_dir(spec.directory)
                .env_clear()
                .envs(spec.environment)
                .stdin(Stdio::null())
                .stdout(capture(spec.stdout)?)
                .stderr(capture(spec.stderr)?)
                .process_group(0)
                .spawn()
                .map_err(|error| error.to_string())?
        };
        #[cfg(unix)]
        let group = nix::unistd::Pid::from_raw(
            i32::try_from(child.id()).map_err(|error| error.to_string())?,
        );
        Ok(Self {
            child,
            #[cfg(unix)]
            group,
            finished: false,
        })
    }
    #[cfg_attr(
        windows,
        expect(
            clippy::needless_pass_by_ref_mut,
            reason = "Unix Child::try_wait requires a mutable process handle."
        )
    )]
    fn wait(&mut self) -> Result<Option<i64>> {
        #[cfg(windows)]
        {
            self.child
                .try_wait()
                .map(|status| status.map(i64::from))
                .map_err(|error| error.to_string())
        }
        #[cfg(unix)]
        {
            self.child
                .try_wait()
                .map(|status| status.map(|status| i64::from(status.code().unwrap_or(-1))))
                .map_err(|error| error.to_string())
        }
    }
    fn stop(&mut self) -> Result<()> {
        #[cfg(windows)]
        self.child
            .stop(Duration::from_secs(5))
            .map_err(|error| error.to_string())?;
        #[cfg(unix)]
        {
            use nix::{
                errno::Errno,
                sys::signal::{Signal, killpg},
            };
            if let Err(error) = killpg(self.group, Signal::SIGKILL) {
                if error != Errno::ESRCH {
                    return Err(error.to_string());
                }
            }
            let started = Instant::now();
            while self
                .child
                .try_wait()
                .map_err(|error| error.to_string())?
                .is_none()
            {
                if started.elapsed() >= Duration::from_secs(5) {
                    return Err("Hosted compiler root cleanup is unconfirmed.".into());
                }
                thread::sleep(Duration::from_millis(10));
            }
            loop {
                match killpg(self.group, None) {
                    Err(Errno::ESRCH) => break,
                    Err(error) => return Err(error.to_string()),
                    Ok(()) => {}
                }
                if started.elapsed() >= Duration::from_secs(5) {
                    return Err("Hosted compiler process group cleanup is unconfirmed.".into());
                }
                thread::sleep(Duration::from_millis(10));
            }
        }
        self.finished = true;
        Ok(())
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.stop();
        }
    }
}

pub(super) fn run(spec: &Spec<'_>, record: &mut Value) -> Result<String> {
    let started = Instant::now();
    let mut child = Owned::start(spec)?;
    let result: Result<()> = (|| {
        loop {
            let extent = fs::metadata(spec.stdout)
                .map_err(|error| error.to_string())?
                .len()
                .checked_add(
                    fs::metadata(spec.stderr)
                        .map_err(|error| error.to_string())?
                        .len(),
                )
                .ok_or("Hosted output extent overflow.")?;
            if extent > 4 * 1024 * 1024 || Instant::now() >= spec.deadline {
                return Err(
                    "Hosted command exceeded its original aggregate/output allowance.".into(),
                );
            }
            if let Some(code) = child.wait()? {
                record["completed"] = json!(true);
                record["exitCode"] = json!(code);
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        Ok(())
    })();
    let cleanup = child.stop();
    record["cleanupConfirmed"] = json!(cleanup.is_ok());
    record["elapsedMilliseconds"] = json!(started.elapsed().as_millis());
    let mut stdout = String::new();
    let mut captured = 0_u64;
    let retain = record["exitCode"] != 0
        || [
            "wire-capture",
            "wire-rust-encoding",
            "queue-abi-capture",
            "queue-abi-rust-capture",
            "start-stop-wire-capture",
            "start-stop-rust-capture",
        ]
        .iter()
        .any(|name| record["name"] == *name);
    for (field, path) in [("log", spec.stdout), ("stderr", spec.stderr)] {
        if let Ok(metadata) = fs::metadata(path) {
            captured = captured.saturating_add(metadata.len());
            if metadata.len() == 0 {
                fs::remove_file(path).map_err(|error| error.to_string())?;
                record[format!("{field}Empty")] = json!(true);
            } else {
                record[field] = super::identity(path)?;
                if field == "log" && metadata.len() <= 4 * 1024 * 1024 {
                    stdout = String::from_utf8_lossy(
                        &fs::read(path).map_err(|error| error.to_string())?,
                    )
                    .into_owned();
                }
                if !retain {
                    record[format!("{field}Observation")] = record[field].take();
                    record
                        .as_object_mut()
                        .ok_or("Hosted command record must remain an object.")?
                        .remove(field);
                    record[format!("{field}Retained")] = json!(false);
                    fs::remove_file(path).map_err(|error| error.to_string())?;
                }
            }
        } else {
            record[format!("{field}Unavailable")] = json!(true);
        }
    }
    result?;
    cleanup?;
    super::require(
        captured <= 4 * 1024 * 1024,
        "Hosted command output exceeded its original combined bound.",
    )?;
    Ok(stdout)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hosted_control_expired_deadline_stops_owned_command() -> Result<()> {
        let root = std::env::temp_dir().join(format!(
            "Bonsai-Hexagon-Control-{}",
            crate::startup_process::unique()
        ));
        fs::create_dir(&root).map_err(|error| error.to_string())?;
        #[cfg(windows)]
        let executable = crate::startup_process::resolve(std::ffi::OsStr::new("cmd.exe"))?;
        #[cfg(windows)]
        let arguments = vec!["/d".into(), "/c".into(), "exit 0".into()];
        #[cfg(unix)]
        let executable = std::path::PathBuf::from("/bin/true");
        #[cfg(unix)]
        let arguments = Vec::new();
        let spec = Spec {
            executable: &executable,
            arguments: &arguments,
            directory: &root,
            environment: &BTreeMap::new(),
            removed: &[],
            stdout: &root.join("stdout.log"),
            stderr: &root.join("stderr.log"),
            deadline: Instant::now(),
        };
        let mut record = json!({"completed":false});
        let result = run(&spec, &mut record);
        assert!(result.is_err());
        assert_eq!(record["cleanupConfirmed"], true);
        fs::remove_dir_all(root).map_err(|error| error.to_string())?;
        Ok(())
    }
}
