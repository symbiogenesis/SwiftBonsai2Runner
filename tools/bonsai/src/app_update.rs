//! Bounded GitHub release discovery and verified Windows Installer update handoff.

use std::{
    fs,
    path::{Path, PathBuf},
};

use bonsai_artifacts::validate_external_path;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

mod policy;
mod transfer;

use policy::{Candidate, Version};

const REPOSITORY: &str = "symbiogenesis/SwiftBonsai2Runner";
const TARGET: &str = "aarch64-pc-windows-msvc";
const MAX_METADATA: u64 = 2 * 1024 * 1024;
const MAX_INSTALLER: u64 = 512 * 1024 * 1024;
pub(crate) const UPDATE_EXIT_CODE: u32 = 42;

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "kebab-case", deny_unknown_fields)]
enum Request {
    #[serde(rename_all = "camelCase")]
    Check { installation_directory: PathBuf },
    #[serde(rename_all = "camelCase")]
    Download {
        installation_directory: PathBuf,
        version: String,
    },
    #[serde(rename_all = "camelCase")]
    Install {
        installation_directory: PathBuf,
        version: String,
        handoff_path: PathBuf,
    },
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Handoff {
    schema_version: u32,
    installation_directory: PathBuf,
    version: String,
}

fn external(path: &Path) -> crate::Result<()> {
    validate_external_path(path).map_err(|error| error.to_string())
}

fn installation(root: &Path, require_current_exe: bool) -> crate::Result<Value> {
    external(root)?;
    let maintenance = root.join("maintenance/bonsai-tools.exe");
    external(&maintenance)?;
    if require_current_exe
        && std::env::current_exe().map_err(|error| error.to_string())? != maintenance
    {
        return Err("Automatic updates require the installed MSI maintenance executable.".into());
    }
    let selected =
        crate::chat_package::run(json!({"operation": "installed", "installationDirectory": root}))?;
    if selected["Manifest"]["schemaVersion"] != 2
        || selected["Manifest"]["version"] != env!("CARGO_PKG_VERSION")
        || selected["Manifest"]["target"] != TARGET
    {
        return Err("The active MSI package does not match this updater build.".into());
    }
    let package_directory = PathBuf::from(
        selected["Directory"]
            .as_str()
            .ok_or("Missing installed package directory.")?,
    );
    if crate::startup_cache::digest_file(&maintenance)?
        != crate::startup_cache::digest_file(&package_directory.join("bin/bonsai-tools.exe"))?
    {
        return Err("Installed maintenance and package identities disagree.".into());
    }
    Ok(selected)
}

fn cache_root() -> crate::Result<PathBuf> {
    let root = PathBuf::from(
        std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA is required for updates.")?,
    )
    .join("SwiftBonsai2/updates");
    external(&root)?;
    fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    external(&root)?;
    Ok(root)
}

fn staged(version: &str) -> crate::Result<PathBuf> {
    Version::parse(version)?;
    let directory = cache_root()?.join(version);
    external(&directory)?;
    Ok(directory)
}

fn discover(working: &Path) -> crate::Result<(bool, Option<Candidate>)> {
    let data = transfer::metadata(
        &format!("https://api.github.com/repos/{REPOSITORY}/releases?per_page=100"),
        working,
    )?;
    policy::select(&data, env!("CARGO_PKG_VERSION"))
}

fn verified_release(candidate: &Candidate, working: &Path) -> crate::Result<policy::Installer> {
    let manifest = transfer::asset_metadata(&candidate.manifest, working)?;
    let checksums = transfer::asset_metadata(&candidate.checksums, working)?;
    let installer = policy::installer(candidate, &manifest, &checksums)?;
    let commit = transfer::metadata(
        &format!(
            "https://api.github.com/repos/{REPOSITORY}/commits/v{}",
            candidate.version
        ),
        working,
    )?;
    installer.verify_source(&commit)?;
    Ok(installer)
}

fn selected_release(version: &str, working: &Path) -> crate::Result<Candidate> {
    Version::parse(version)?.newer_than_current()?;
    let (_, candidate) = discover(working)?;
    candidate
        .filter(|candidate| candidate.version == version)
        .ok_or_else(|| {
            "The selected update is no longer the newest published release; check again.".into()
        })
}

fn download(root: &Path, version: &str) -> crate::Result<Value> {
    installation(root, true)?;
    Version::parse(version)?.newer_than_current()?;
    let directory = staged(version)?;
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    external(&directory)?;
    let _lock = crate::startup_cache::lock_file(&directory.join("update.lock"))?;
    let candidate = selected_release(version, &directory)?;
    let installer = verified_release(&candidate, &directory)?;
    let path = directory.join(&installer.asset.name);
    crate::startup_download::receive_with_timeout(
        &installer.asset.url,
        &path,
        installer.asset.bytes,
        &installer.asset.sha256,
        &directory,
        std::time::Duration::from_secs(180),
    )?;
    transfer::write_new_or_identical(
        &directory.join("verified-release.json"),
        &serde_json::to_vec(&installer).map_err(|error| error.to_string())?,
    )?;
    Ok(json!({"Status": "downloaded", "Version": version, "ReleaseUrl": candidate.release_url}))
}

fn verify_stage(
    directory: &Path,
    version: &str,
    published: &policy::Installer,
) -> crate::Result<bonsai_artifacts::VerifiedFile> {
    Version::parse(version)?.newer_than_current()?;
    published.validate(version)?;
    let bytes = transfer::read_bounded(&directory.join("verified-release.json"), MAX_METADATA)?;
    let installer: policy::Installer =
        serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    installer.validate(version)?;
    if installer != *published {
        return Err(
            "The staged update differs from the published release; download and confirm it again."
                .into(),
        );
    }
    let path = directory.join(&installer.asset.name);
    bonsai_artifacts::VerifiedFile::verify_with(
        &path,
        installer.asset.bytes,
        &installer.asset.sha256,
        || {
            crate::startup_cancel::checkpoint()
                .map_err(|_| bonsai_artifacts::ArtifactError::Cancelled)
        },
    )
    .map_err(|error| error.to_string())
}

fn queue(root: &Path, version: &str, handoff: &Path) -> crate::Result<Value> {
    installation(root, true)?;
    let directory = staged(version)?;
    let _lock = crate::startup_cache::lock_file(&directory.join("update.lock"))?;
    let candidate = selected_release(version, &directory)?;
    let fresh = verified_release(&candidate, &directory)?;
    let _guard = verify_stage(&directory, version, &fresh)?;
    external(handoff)?;
    let name = handoff
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("Missing update handoff name.")?;
    if !name.starts_with("chat-startup-") || !name.ends_with(".update.json") || name.len() > 160 {
        return Err("Updates require the current native launcher's startup handoff.".into());
    }
    let startup = handoff.with_extension("").with_extension("json");
    // The exact path is also retained independently by the launcher. A stale
    // handoff or a request for a different package cannot change that selection.
    let startup_request: Value =
        serde_json::from_slice(&transfer::read_bounded(&startup, MAX_METADATA)?)
            .map_err(|error| error.to_string())?;
    let chat = PathBuf::from(
        startup_request["ChatPath"]
            .as_str()
            .ok_or("Missing startup chat identity.")?,
    );
    let selected = installation(root, true)?;
    let expected = PathBuf::from(
        selected["Directory"]
            .as_str()
            .ok_or("Missing package identity.")?,
    )
    .join("bin/bonsai-chat.exe");
    if chat != expected {
        return Err("Update handoff belongs to another application package.".into());
    }
    let payload = serde_json::to_vec(&Handoff {
        schema_version: 1,
        installation_directory: root.to_owned(),
        version: version.into(),
    })
    .map_err(|error| error.to_string())?;
    transfer::write_exclusive(handoff, &payload)?;
    Ok(json!({"Status": "queued", "Version": version}))
}

/// Checks for, downloads, or queues an explicit installed application update.
///
/// # Errors
/// Rejects unsupported hosts/installations, release identities, untrusted asset
/// paths, downloads exceeding bounds, changed bytes and invalid launch handoffs.
pub fn run(value: Value) -> crate::Result<Value> {
    if !cfg!(all(windows, target_arch = "aarch64")) {
        return Err("Application updates require native Windows ARM64.".into());
    }
    match serde_json::from_value(value).map_err(|error| error.to_string())? {
        Request::Check {
            installation_directory,
        } => {
            installation(&installation_directory, true)?;
            let working = cache_root()?;
            let (published, candidate) = discover(&working)?;
            if let Some(candidate) = candidate {
                verified_release(&candidate, &working)?;
                Ok(
                    json!({"Status": "available", "Version": candidate.version, "ReleaseUrl": candidate.release_url}),
                )
            } else {
                Ok(
                    json!({"Status": if published { "current" } else { "no-release" }, "Version": env!("CARGO_PKG_VERSION")}),
                )
            }
        }
        Request::Download {
            installation_directory,
            version,
        } => download(&installation_directory, &version),
        Request::Install {
            installation_directory,
            version,
            handoff_path,
        } => queue(&installation_directory, &version, &handoff_path),
    }
}

pub(crate) fn apply_after_exit(handoff: &Path) -> crate::Result<Value> {
    external(handoff)?;
    let request: Handoff = serde_json::from_slice(&transfer::read_bounded(handoff, 8192)?)
        .map_err(|error| error.to_string())?;
    if request.schema_version != 1 {
        return Err("Unsupported update handoff schema.".into());
    }
    installation(&request.installation_directory, true)?;
    let directory = staged(&request.version)?;
    let _lock = crate::startup_cache::lock_file(&directory.join("update.lock"))?;
    // The cache is user-writable. Never let a rewritten receipt authenticate
    // replacement bytes after the UI's earlier confirmation and verification.
    let candidate = selected_release(&request.version, &directory)?;
    let published = verified_release(&candidate, &directory)?;
    let immutable_installer = verify_stage(&directory, &request.version, &published)?;
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        use std::process::{Command, Stdio};
        let installer = crate::startup_download::system_tool("msiexec.exe")?;
        let install_folder = format!("INSTALLFOLDER={}", request.installation_directory.display());
        // Consume authorization before process creation. A failed spawn requires
        // a new confirmation; cleanup failure must never follow a live installer.
        fs::remove_file(handoff).map_err(|error| error.to_string())?;
        let process = Command::new(installer)
            .arg("/i")
            .arg(immutable_installer.path())
            .arg("/passive")
            .arg("/norestart")
            .arg(install_folder)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x0000_0200 | 0x0800_0000)
            .spawn()
            .map_err(|error| error.to_string())?;
        Ok(
            json!({"Status": "update-installer-started", "Version": request.version, "InstallerProcessId": process.id()}),
        )
    }
    #[cfg(not(windows))]
    {
        let _ = immutable_installer;
        Err("Application installation requires Windows.".into())
    }
}

#[cfg(all(test, windows))]
mod tests;
