//! Source-bound engine and hosted capture builds, with native process ownership.

use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs::{self, OpenOptions},
    io::{Read as _, Seek as _, SeekFrom, Write as _},
    path::{Path, PathBuf},
    time::Duration,
};

use bonsai_artifacts::{is_redirect, validate_external_path};
use serde::Deserialize;
use serde_json::{Value, json};

mod capture;
mod engine;
mod headless;

const CHANNEL: &str = "nightly-2026-10-22-aarch64-pc-windows-msvc";
const RECEIPT_LIMIT: usize = 65_536;

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "kebab-case", deny_unknown_fields)]
enum Request {
    #[serde(rename_all = "camelCase")]
    Ptq1Capture {
        repository_root: PathBuf,
        output_directory: PathBuf,
    },
    #[serde(rename_all = "camelCase")]
    RowCapture {
        repository_root: PathBuf,
        output_directory: PathBuf,
    },
    #[serde(rename_all = "camelCase")]
    HeadlessPublisher {
        repository_root: PathBuf,
        output_directory: PathBuf,
    },
    #[serde(rename_all = "camelCase")]
    Engine {
        repository_root: PathBuf,
        data_directory: PathBuf,
        git_path: PathBuf,
        cmake_path: PathBuf,
        #[serde(default = "generator")]
        generator: String,
        #[serde(default)]
        parallel: u8,
        #[serde(default)]
        fetch_source: bool,
    },
}

fn generator() -> String {
    "Visual Studio 18 2026".into()
}

fn environment() -> BTreeMap<String, String> {
    std::env::vars_os()
        .filter_map(|(name, value)| Some((name.into_string().ok()?, value.into_string().ok()?)))
        .collect()
}

fn hosted_context(native: bool, environment: &BTreeMap<String, String>) -> crate::Result<()> {
    let value = |name| environment.get(name).map_or("", String::as_str);
    if !native
        || value("GITHUB_ACTIONS") != "true"
        || value("RUNNER_ENVIRONMENT") != "github-hosted"
        || value("RUNNER_ARCH") != "ARM64"
        || value("GITHUB_REF") != "refs/heads/main"
        || value("GITHUB_REPOSITORY") != "symbiogenesis/SwiftBonsai2Runner"
        || !crate::identity::is_lower_hex(value("GITHUB_SHA"), 40)
    {
        return Err(
            "Build preparation requires this repository main on hosted native Windows ARM64 CI."
                .into(),
        );
    }
    Ok(())
}

fn external(path: &Path) -> crate::Result<()> {
    validate_external_path(path).map_err(|error| error.to_string())
}

fn ordinary(path: &Path) -> crate::Result<()> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !metadata.is_file() || is_redirect(&metadata) {
        return Err("Build inputs and outputs must be ordinary files.".into());
    }
    Ok(())
}

fn read_json(path: &Path) -> crate::Result<Value> {
    ordinary(path)?;
    let file = fs::File::open(path).map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    file.take(u64::try_from(RECEIPT_LIMIT + 1).map_err(|error| error.to_string())?)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > RECEIPT_LIMIT {
        return Err("Build input exceeds its fixed byte bound.".into());
    }
    serde_json::from_slice(&bytes).map_err(|error| error.to_string())
}

fn text<'a>(value: &'a Value, name: &str) -> crate::Result<&'a str> {
    value[name]
        .as_str()
        .ok_or_else(|| format!("Missing build field: {name}"))
}

fn count(value: &Value, name: &str) -> crate::Result<u64> {
    value[name]
        .as_u64()
        .ok_or_else(|| format!("Missing build count: {name}"))
}

fn save(path: &Path, receipt: &Value, exclusive: bool) -> crate::Result<()> {
    external(path)?;
    let mut bytes = serde_json::to_vec_pretty(receipt).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    if bytes.len() > RECEIPT_LIMIT {
        return Err("Build receipt exceeds its fixed byte bound.".into());
    }
    let mut options = OpenOptions::new();
    options.write(true);
    if exclusive {
        options.create_new(true);
    } else {
        options.create(true).truncate(true);
    }
    let mut file = options.open(path).map_err(|error| error.to_string())?;
    file.write_all(&bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| error.to_string())
}

fn image(path: &Path) -> crate::Result<Value> {
    ordinary(path)?;
    Ok(
        json!({"Name":path.file_name().and_then(|name| name.to_str()).ok_or("Build output name must be Unicode.")?,
        "Bytes":fs::metadata(path).map_err(|error| error.to_string())?.len(),
        "Sha256":crate::startup_cache::digest_file(path)?}),
    )
}

fn text_arguments(arguments: &[OsString]) -> crate::Result<Vec<&str>> {
    arguments
        .iter()
        .map(|argument| {
            argument
                .to_str()
                .ok_or_else(|| "Build arguments must be Unicode.".to_owned())
        })
        .collect()
}

fn stage_output(path: &Path, empty_allowed: bool) -> crate::Result<()> {
    external(path)?;
    match fs::symlink_metadata(path) {
        Ok(metadata) if empty_allowed && metadata.is_dir() && !is_redirect(&metadata) => {
            if fs::read_dir(path)
                .map_err(|error| error.to_string())?
                .next()
                .is_some()
            {
                return Err("Build output must be a new or empty external directory.".into());
            }
        }
        Ok(_) => return Err("Build output must be a fresh external directory.".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(path).map_err(|error| error.to_string())?;
        }
        Err(error) => return Err(error.to_string()),
    }
    Ok(())
}

fn scratch(label: &str) -> crate::Result<PathBuf> {
    let path = std::env::temp_dir().join(format!(
        "Bonsai-{label}-{}",
        crate::startup_process::unique()
    ));
    external(&path)?;
    fs::create_dir(&path).map_err(|error| error.to_string())?;
    Ok(path)
}

fn execute(
    executable: &Path,
    arguments: &[OsString],
    working: &Path,
    environment: &BTreeMap<String, String>,
    name: &str,
    seconds: u64,
) -> crate::Result<crate::startup_process::ProcessOutput> {
    let log = working.join(name);
    let result = crate::startup_process::run(
        executable.as_os_str(),
        arguments,
        working,
        environment,
        Duration::from_secs(seconds),
        &log,
    )?;
    if result.code != 0 {
        return Err(format!(
            "Build tool failed ({}); inspect {}",
            result.code,
            log.display()
        ));
    }
    Ok(result)
}

fn download(pin: &Value, path: &Path, working: &Path, seconds: u64) -> crate::Result<()> {
    crate::startup_download::receive_with_timeout(
        text(pin, "url")?,
        path,
        count(pin, "bytes")?,
        text(pin, "sha256")?,
        working,
        Duration::from_secs(seconds),
    )
}

fn extract_zip(archive: &Path, destination: &Path) -> crate::Result<()> {
    external(destination)?;
    let input = bonsai_artifacts::open_locked_file(archive).map_err(|error| error.to_string())?;
    let mut zip = zip::ZipArchive::new(input).map_err(|error| error.to_string())?;
    if zip.len() > 100_000 {
        return Err("Source archive has too many entries.".into());
    }
    let mut total = 0_u64;
    let mut names = std::collections::BTreeSet::new();
    for index in 0..zip.len() {
        let entry = zip.by_index(index).map_err(|error| error.to_string())?;
        let name = entry.name().map_err(|error| error.to_string())?;
        total = total
            .checked_add(entry.size())
            .ok_or("Source archive size overflow.")?;
        let relative = entry.enclosed_name().ok_or("Unsafe source archive path.")?;
        if entry.is_symlink()
            || total > 2_147_483_648
            || name.contains(['\\', ':'])
            || !names.insert(name.to_uppercase())
            || relative
                .components()
                .any(|component| !matches!(component, std::path::Component::Normal(_)))
        {
            return Err(
                "Source archive has a linked, duplicate, unsafe or oversized entry.".into(),
            );
        }
    }
    fs::create_dir(destination).map_err(|error| error.to_string())?;
    for index in 0..zip.len() {
        crate::startup_cancel::checkpoint()?;
        let mut entry = zip.by_index(index).map_err(|error| error.to_string())?;
        let path = destination.join(entry.enclosed_name().ok_or("Unsafe source archive path.")?);
        external(&path)?;
        if entry.is_dir() {
            fs::create_dir_all(path).map_err(|error| error.to_string())?;
            continue;
        }
        fs::create_dir_all(path.parent().ok_or("Missing archive parent.")?)
            .map_err(|error| error.to_string())?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|error| error.to_string())?;
        let expected = entry.size();
        if std::io::copy(&mut entry.by_ref().take(expected + 1), &mut output)
            .map_err(|error| error.to_string())?
            != expected
        {
            return Err("Source archive entry differs from its declared length.".into());
        }
        output.sync_all().map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn save_evidence(
    working: &Path,
    evidence: &Path,
    receipt: &Value,
    names: &[&str],
) -> crate::Result<()> {
    for name in names {
        let path = working.join(name);
        if !path.exists() {
            continue;
        }
        ordinary(&path)?;
        let mut input = fs::File::open(path).map_err(|error| error.to_string())?;
        let length = input.metadata().map_err(|error| error.to_string())?.len();
        let count = length.min(65_536);
        input
            .seek(SeekFrom::Start(length - count))
            .map_err(|error| error.to_string())?;
        let mut tail = vec![0; usize::try_from(count).map_err(|error| error.to_string())?];
        input
            .read_exact(&mut tail)
            .map_err(|error| error.to_string())?;
        fs::write(evidence.join(name), tail).map_err(|error| error.to_string())?;
    }
    save(&evidence.join("build-receipt.json"), receipt, false)
}

/// Builds the fixed source-bound engine or hosted qualification tools without models.
///
/// # Errors
/// Rejects unsupported requests, non-hosted capture/candidate builds, unsafe paths,
/// changed source/compiler identities, unowned processes and incomplete output receipts.
pub fn run(request: Value) -> crate::Result<Value> {
    let request: Request = serde_json::from_value(request).map_err(|error| error.to_string())?;
    match request {
        Request::Ptq1Capture {
            repository_root,
            output_directory,
        } => capture::build(&repository_root, &output_directory, true),
        Request::RowCapture {
            repository_root,
            output_directory,
        } => capture::build(&repository_root, &output_directory, false),
        Request::HeadlessPublisher {
            repository_root,
            output_directory,
        } => headless::build(&repository_root, &output_directory),
        Request::Engine {
            repository_root,
            data_directory,
            git_path,
            cmake_path,
            generator,
            parallel,
            fetch_source,
        } => engine::build(
            &repository_root,
            &data_directory,
            &git_path,
            &cmake_path,
            &generator,
            parallel,
            fetch_source,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hosted() -> BTreeMap<String, String> {
        [
            ("GITHUB_ACTIONS", "true"),
            ("RUNNER_ENVIRONMENT", "github-hosted"),
            ("RUNNER_ARCH", "ARM64"),
            ("GITHUB_REF", "refs/heads/main"),
            ("GITHUB_REPOSITORY", "symbiogenesis/SwiftBonsai2Runner"),
            ("GITHUB_SHA", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        ]
        .into_iter()
        .map(|(key, value)| (key.into(), value.into()))
        .collect()
    }

    #[test]
    fn hosted_build_guard_rejects_each_changed_identity_before_writes() -> crate::Result<()> {
        let environment = hosted();
        hosted_context(true, &environment)?;
        assert!(hosted_context(false, &environment).is_err());
        for key in environment.keys() {
            let mut changed = environment.clone();
            changed.insert(key.clone(), "wrong".into());
            assert!(hosted_context(true, &changed).is_err(), "{key}");
        }
        Ok(())
    }

    #[test]
    fn compiler_evidence_retains_only_bounded_selected_tails_and_failure_scope() -> crate::Result<()>
    {
        let working = scratch("BuildEvidenceTest")?;
        let evidence = working.join("evidence");
        fs::create_dir(&evidence).map_err(|error| error.to_string())?;
        let mut bytes = vec![b'x'; 70_000];
        bytes.extend_from_slice(b"tail");
        fs::write(working.join("build.txt"), bytes).map_err(|error| error.to_string())?;
        fs::write(working.join("unselected.txt"), "private").map_err(|error| error.to_string())?;
        let receipt = json!({"Status":"failed", "Phase":"compile", "ModelProcessStarted":false});
        save_evidence(&working, &evidence, &receipt, &["build.txt"])?;
        let tail = fs::read(evidence.join("build.txt")).map_err(|error| error.to_string())?;
        assert_eq!(tail.len(), 65_536);
        assert!(tail.ends_with(b"tail"));
        assert!(!evidence.join("unselected.txt").exists());
        assert_eq!(read_json(&evidence.join("build-receipt.json"))?, receipt);
        assert!(
            save(
                &evidence.join("oversized.json"),
                &json!({"Failure":"x".repeat(65_537)}),
                false
            )
            .is_err()
        );
        fs::remove_dir_all(working).map_err(|error| error.to_string())
    }
}
