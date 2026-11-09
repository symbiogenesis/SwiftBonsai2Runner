//! Windows Job ownership and retained Cargo check receipts.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bonsai_artifacts::{hash_reader, validate_external_path};
use bonsai_windows_process::{OwnedProcess, ToolProcessSpec};
use serde_json::{Value, json};

use super::{Check, Options, Package};
use crate::Result;

const MAX_LOG_BYTES: u64 = 16 * 1024 * 1024;

fn external(path: &Path) -> Result<()> {
    validate_external_path(path).map_err(|error| error.to_string())
}

fn distinct(target: &Path, evidence: &Path) -> Result<()> {
    external(target)?;
    external(evidence)?;
    let normalized = |path: &Path| {
        path.to_string_lossy()
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_uppercase()
    };
    let target = normalized(target);
    let evidence = normalized(evidence);
    if target == evidence
        || target.starts_with(&(evidence.clone() + "\\"))
        || evidence.starts_with(&(target + "\\"))
    {
        return Err("Cargo targets must be separate from uploaded evidence.".into());
    }
    Ok(())
}

fn executable(name: &str) -> Result<PathBuf> {
    for directory in std::env::split_paths(&std::env::var_os("PATH").ok_or("PATH is unavailable.")?)
    {
        if directory.is_absolute() {
            let path = directory.join(name);
            if path.is_file() {
                return Ok(path);
            }
        }
    }
    Err(format!("Required executable is missing from PATH: {name}"))
}

fn digest(path: &Path) -> Result<String> {
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let length = file.metadata().map_err(|error| error.to_string())?.len();
    hash_reader(&mut file, length, &mut vec![0; 65536], &mut || Ok(()))
        .map_err(|error| error.to_string())
}

fn read_log(path: &Path) -> Result<Vec<u8>> {
    let file = File::open(path).map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    file.take(MAX_LOG_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if u64::try_from(bytes.len()).map_err(|error| error.to_string())? > MAX_LOG_BYTES {
        return Err("Rust command output exceeded its byte limit.".into());
    }
    Ok(bytes)
}

fn command(spec: &ToolProcessSpec, allowance: Duration, log: &Path) -> Result<(u32, String)> {
    if allowance.is_zero() {
        return Err("Rust check aggregate deadline expired.".into());
    }
    let child = OwnedProcess::start_tool(spec).map_err(|error| error.to_string())?;
    let started = Instant::now();
    let code = loop {
        if let Some(code) = child.try_wait().map_err(|error| error.to_string())? {
            break code;
        }
        if started.elapsed() >= allowance
            || [&spec.stdout, &spec.stderr].into_iter().any(|path| {
                !fs::metadata(path).is_ok_and(|metadata| metadata.len() <= MAX_LOG_BYTES)
            })
        {
            child
                .stop(Duration::from_secs(5))
                .map_err(|error| error.to_string())?;
            fs::write(log, "Rust command exceeded its deadline or output limit; partial stdout/stderr are retained beside this log.\n").map_err(|error| error.to_string())?;
            return Err("Rust command exceeded its deadline or output limit; owned descendants were stopped.".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    child
        .stop(Duration::from_secs(5))
        .map_err(|error| error.to_string())?;
    let stdout = read_log(&spec.stdout)?;
    let stderr = read_log(&spec.stderr)?;
    let mut combined = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(log)
        .map_err(|error| error.to_string())?;
    combined
        .write_all(&stdout)
        .and_then(|()| combined.write_all(&stderr))
        .map_err(|error| error.to_string())?;
    Ok((code, String::from_utf8_lossy(&stdout).trim().into()))
}

fn receipt(path: &Path, value: &Value) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    fs::write(path, bytes).map_err(|error| error.to_string())
}

struct Execution<'a> {
    options: &'a Options,
    plan: &'a [Check],
    toolchain: &'a str,
    target: &'a Path,
    evidence: &'a Path,
    work: &'a Path,
    started: Instant,
    owner: bool,
}

impl Execution<'_> {
    fn allowance(&self, seconds: u64) -> Duration {
        let per_command = Duration::from_secs(seconds);
        if self.owner {
            per_command.min(Duration::from_secs(290).saturating_sub(self.started.elapsed()))
        } else {
            per_command
        }
    }

    fn spec(
        &self,
        executable: PathBuf,
        arguments: Vec<OsString>,
        name: &str,
    ) -> Result<ToolProcessSpec> {
        let text = |path: &Path| -> Result<String> {
            path.to_str()
                .map(String::from)
                .ok_or_else(|| "Rust check paths must be UTF-8.".into())
        };
        Ok(ToolProcessSpec {
            executable,
            arguments,
            working_directory: self.work.into(),
            stdout: self.evidence.join(format!("{name}.stdout")),
            stderr: self.evidence.join(format!("{name}.stderr")),
            environment: BTreeMap::from([
                ("CARGO_TARGET_DIR".into(), text(self.target)?),
                ("TEMP".into(), text(self.work)?),
                ("TMP".into(), text(self.work)?),
            ]),
        })
    }

    fn run(&self, report: &mut Value, receipt_path: &Path) -> Result<()> {
        let cargo = executable("cargo.exe")?;
        let rustc = executable("rustc.exe")?;
        let identity = command(
            &self.spec(
                rustc,
                vec![format!("+{}", self.toolchain).into(), "-vV".into()],
                "rustc",
            )?,
            self.allowance(60),
            &self.evidence.join("rustc.log"),
        )?;
        if identity.0 != 0 {
            return Err("Rust compiler identification failed.".into());
        }
        report["CompilerIdentity"] = identity.1.into();
        for check in self.plan {
            let log = self.evidence.join(format!("{}.log", check.name));
            let started = Instant::now();
            let mut record = json!({"Name": check.name, "Phase": check.phase, "Packages": check.packages,
                "Executable": cargo, "Arguments": check.arguments, "Log": log,
                "ExitCode": null, "Passed": false, "ElapsedMilliseconds": null});
            let result = command(
                &self.spec(
                    cargo.clone(),
                    check.arguments.iter().map(Into::into).collect(),
                    &check.name,
                )?,
                self.allowance(self.options.timeout_seconds),
                &log,
            );
            record["ElapsedMilliseconds"] = u64::try_from(started.elapsed().as_millis())
                .map_err(|error| error.to_string())?
                .into();
            if let Ok((code, _)) = &result {
                record["ExitCode"] = (*code).into();
                record["Passed"] = (*code == 0).into();
            }
            report["Checks"]
                .as_array_mut()
                .ok_or("Invalid check receipt.")?
                .push(record);
            receipt(receipt_path, report)?;
            result?;
        }
        if report["Checks"]
            .as_array()
            .ok_or("Invalid check receipt.")?
            .iter()
            .any(|record| record["Passed"] != true)
        {
            return Err("Rust checks failed; inspect the retained command logs.".into());
        }
        Ok(())
    }
}

pub(super) fn run(
    options: &Options,
    packages: &[&Package],
    toolchain: &str,
    plan: &[Check],
) -> Result<String> {
    let started = Instant::now();
    let target = options
        .target
        .as_deref()
        .ok_or("--target is required for rust-check.")?;
    let evidence = options
        .evidence
        .as_deref()
        .ok_or("--evidence is required for rust-check.")?;
    distinct(target, evidence)?;
    if evidence.exists() {
        return Err("Use a new Rust check evidence directory.".into());
    }
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let work = std::env::temp_dir().join(format!(
        "SwiftBonsai-RustCheck-{}-{nonce}",
        std::process::id()
    ));
    external(&work)?;
    let workspace_hash = digest(&options.root.join("Cargo.toml"))?;
    let lock_hash = digest(&options.root.join("Cargo.lock"))?;
    let registry_hash = digest(&options.root.join("config/rust-packages.json"))?;
    fs::create_dir_all(target).map_err(|error| error.to_string())?;
    if let Some(parent) = evidence.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::create_dir(evidence).map_err(|error| error.to_string())?;
    fs::create_dir(&work).map_err(|error| error.to_string())?;
    let mut report = json!({
        "SchemaVersion": 2, "Revision": std::env::var("GITHUB_SHA").ok(), "Toolchain": toolchain,
        "CheckProfile": options.profile, "FingerprintMode": std::env::var("CARGO_BUILD_FINGERPRINT").unwrap_or_else(|_| "mtime".into()),
        "Packages": packages.iter().map(|package| &package.name).collect::<Vec<_>>(), "TargetDirectory": target,
        "WorkingDirectory": work, "CompilerIdentity": null, "Checks": [], "Success": false, "Error": null,
        "PerformanceMeasured": false, "PlannedCargoInvocations": plan.len(), "UnbatchedCargoInvocations": packages.len() * options.phases.len(),
        "WorkspaceSha256": workspace_hash, "LockSha256": lock_hash, "RegistrySha256": registry_hash
    });
    let path = evidence.join("rust-checks.json");
    receipt(&path, &report)?;
    let execution = Execution {
        options,
        plan,
        toolchain,
        target,
        evidence,
        work: &work,
        started,
        owner: std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted"),
    };
    let result = execution.run(&mut report, &path);
    match &result {
        Ok(()) => report["Success"] = true.into(),
        Err(error) => report["Error"] = error.clone().into(),
    }
    receipt(&path, &report)?;
    result?;
    serde_json::to_string(&report).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_and_uploaded_evidence_cannot_overlap() {
        let root = std::env::temp_dir().join("SwiftBonsai-CheckPaths");
        assert!(distinct(&root.join("target"), &root.join("evidence")).is_ok());
        assert!(distinct(&root, &root).is_err());
        assert!(distinct(&root, &root.join("nested")).is_err());
        assert!(distinct(&root.join("nested"), &root).is_err());
        assert!(distinct(Path::new("relative"), &root).is_err());
    }
}
