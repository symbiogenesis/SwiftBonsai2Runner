//! Bounded release discovery, verified portable replacement and legacy MSI handoff.

use std::{
    fs,
    path::{Path, PathBuf},
};

use bonsai_artifacts::validate_external_path;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

mod policy;
mod portable;
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
    PortableCheck,
    #[serde(rename_all = "camelCase")]
    PortableDownload {
        version: String,
    },
    #[serde(rename_all = "camelCase")]
    PortableInstall {
        version: String,
        handoff_path: PathBuf,
        release_identity: String,
        #[serde(default = "restart_by_default")]
        restart: bool,
    },
    ReplacePortable(portable::Replacement),
    #[serde(rename_all = "camelCase")]
    Check {
        installation_directory: PathBuf,
    },
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

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PortableHandoff {
    schema_version: u32,
    distribution: String,
    version: String,
    release_identity: String,
    #[serde(default = "restart_by_default")]
    restart: bool,
}

const fn restart_by_default() -> bool {
    true
}

impl PortableHandoff {
    fn verify(&self, published: &policy::Installer) -> crate::Result<()> {
        if self.schema_version != 2
            || self.distribution != "portable-executable"
            || published.distribution() != policy::Distribution::Portable
        {
            return Err("Unsupported or changed portable update handoff.".into());
        }
        published.validate(&self.version)?;
        published.verify_release_identity(&self.release_identity)
    }
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
    let data = cached_release_catalog(working)?;
    policy::select(&data, env!("CARGO_PKG_VERSION"))
}

fn collect_releases(
    mut fetch: impl FnMut(u32) -> crate::Result<Vec<u8>>,
) -> crate::Result<Vec<u8>> {
    let mut releases = Vec::new();
    for page in 1..=3 {
        let bytes = fetch(page)?;
        let entries: Vec<Value> =
            serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
        if entries.len() > 100 {
            return Err("GitHub returned an oversized release page.".into());
        }
        let complete = entries.len() < 100;
        releases.extend(entries);
        if complete {
            return serde_json::to_vec(&releases).map_err(|error| error.to_string());
        }
    }
    Err(
        "The release catalog exceeds this check's limit; cannot determine the newest release."
            .into(),
    )
}

fn release_catalog(working: &Path) -> crate::Result<Vec<u8>> {
    collect_releases(|page| {
        transfer::metadata(
            &format!("https://api.github.com/repos/{REPOSITORY}/releases?per_page=100&page={page}"),
            working,
        )
    })
}

fn cached_release_catalog(working: &Path) -> crate::Result<Vec<u8>> {
    collect_releases(|page| {
        transfer::cached_metadata(
            &format!("https://api.github.com/repos/{REPOSITORY}/releases?per_page=100&page={page}"),
            working,
        )
    })
}

fn portable_installation() -> crate::Result<Value> {
    let (root, digest) = crate::chat_package::bundle::update_identity()?;
    let selected =
        crate::chat_package::run(json!({"operation": "installed", "installationDirectory": root}))?;
    if selected["ManifestSha256"] != digest
        || selected["Manifest"]["schemaVersion"] != 2
        || selected["Manifest"]["version"] != env!("CARGO_PKG_VERSION")
        || selected["Manifest"]["target"] != TARGET
    {
        return Err("Portable update requires the current verified bundle package.".into());
    }
    Ok(selected)
}

fn published_portable_release(version: &str, working: &Path) -> crate::Result<Candidate> {
    Version::parse(version)?;
    let (_, candidate) = policy::select_portable(&release_catalog(working)?, "0.0.0")?;
    candidate
        .filter(|candidate| candidate.version == version)
        .ok_or_else(|| {
            "The selected update is no longer the newest published release; check again.".into()
        })
}

fn verified_portable_release(
    candidate: &Candidate,
    working: &Path,
) -> crate::Result<policy::Installer> {
    portable_release_metadata(candidate, working, false)
}

type AssetMetadata = fn(&policy::Asset, &Path) -> crate::Result<Vec<u8>>;
type Metadata = fn(&str, &Path) -> crate::Result<Vec<u8>>;

fn metadata_functions(conditional: bool) -> (AssetMetadata, Metadata) {
    if conditional {
        (transfer::cached_asset_metadata, transfer::cached_metadata)
    } else {
        (transfer::asset_metadata, transfer::metadata)
    }
}

fn portable_release_metadata(
    candidate: &Candidate,
    working: &Path,
    conditional: bool,
) -> crate::Result<policy::Installer> {
    let (asset, metadata) = metadata_functions(conditional);
    let manifest = asset(&candidate.manifest, working)?;
    let checksums = asset(&candidate.checksums, working)?;
    let executable = policy::portable(candidate, &manifest, &checksums)?;
    let commit = metadata(
        &format!(
            "https://api.github.com/repos/{REPOSITORY}/commits/v{}",
            candidate.version
        ),
        working,
    )?;
    executable.verify_source(&commit)?;
    Ok(executable)
}

fn portable_download(version: &str) -> crate::Result<Value> {
    portable_installation()?;
    Version::parse(version)?.newer_than_current()?;
    let directory = staged(version)?;
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    external(&directory)?;
    let _lock = crate::startup_cache::lock_file(&directory.join("update.lock"))?;
    let candidate = published_portable_release(version, &directory)?;
    let published = verified_portable_release(&candidate, &directory)?;
    let identity = stage_portable_with(&directory, version, &published, |path| {
        crate::startup_download::receive_repair_with_timeout(
            &published.asset.url,
            path,
            published.asset.bytes,
            &published.asset.sha256,
            &directory,
            std::time::Duration::from_secs(180),
        )
    })?;
    Ok(json!({"Status": "downloaded", "Version": version, "ReleaseIdentity": identity}))
}

fn stage_portable_with(
    directory: &Path,
    version: &str,
    published: &policy::Installer,
    receive: impl FnOnce(&Path) -> crate::Result<()>,
) -> crate::Result<String> {
    published.validate(version)?;
    if published.distribution() != policy::Distribution::Portable {
        return Err("Portable staging requires the selected executable release.".into());
    }
    crate::startup_cancel::checkpoint()?;
    receive(&directory.join(&published.asset.name))?;
    let _verified = verify_asset(directory, published)?;
    // A poisoned local receipt must not make every later download fail. Fresh
    // publication plus immutable verified bytes authenticate this replacement;
    // an earlier confirmation remains bound to its own release fingerprint.
    transfer::write_current(
        &directory.join("verified-release.json"),
        &serde_json::to_vec(published).map_err(|error| error.to_string())?,
    )?;
    published.release_identity()
}

fn portable_queue(
    version: &str,
    handoff: &Path,
    release_identity: &str,
    restart: bool,
) -> crate::Result<Value> {
    Version::parse(version)?.newer_than_current()?;
    let selected = portable_installation()?;
    let directory = staged(version)?;
    let _lock = crate::startup_cache::lock_file(&directory.join("update.lock"))?;
    let candidate = published_portable_release(version, &directory)?;
    let fresh = verified_portable_release(&candidate, &directory)?;
    fresh.verify_release_identity(release_identity)?;
    let _guard = verify_stage(&directory, version, &fresh)?;
    let startup_request = startup_handoff(handoff)?;
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let chat = PathBuf::from(
        selected["Directory"]
            .as_str()
            .ok_or("Missing package directory.")?,
    )
    .join("bin/bonsai-chat.exe");
    if startup_request["PortableExecutable"] != json!(executable)
        || startup_request["ChatPath"] != json!(chat)
    {
        return Err("Portable update handoff belongs to another application launch.".into());
    }
    // This command follows a new explicit confirmation and fresh verification.
    // A leftover file from a lost response is replaced, never accepted as prior
    // authorization; only the UI's successful dedicated exit activates it.
    write_portable_handoff(handoff, version, release_identity, &fresh, restart)?;
    Ok(json!({"Status": "queued", "Version": version}))
}

fn write_portable_handoff(
    path: &Path,
    version: &str,
    release_identity: &str,
    published: &policy::Installer,
    restart: bool,
) -> crate::Result<()> {
    let request = PortableHandoff {
        schema_version: 2,
        distribution: "portable-executable".into(),
        version: version.into(),
        release_identity: release_identity.into(),
        restart,
    };
    request.verify(published)?;
    transfer::write_current(
        path,
        &serde_json::to_vec(&request).map_err(|error| error.to_string())?,
    )
}

fn startup_handoff(handoff: &Path) -> crate::Result<Value> {
    external(handoff)?;
    let name = handoff
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("Missing update handoff name.")?;
    if !name.starts_with("chat-startup-") || !name.ends_with(".update.json") || name.len() > 160 {
        return Err("Updates require the current native launcher's startup handoff.".into());
    }
    let startup = handoff.with_extension("").with_extension("json");
    serde_json::from_slice(&transfer::read_bounded(&startup, MAX_METADATA)?)
        .map_err(|error| error.to_string())
}

fn verified_release(candidate: &Candidate, working: &Path) -> crate::Result<policy::Installer> {
    installer_release_metadata(candidate, working, false)
}

fn installer_release_metadata(
    candidate: &Candidate,
    working: &Path,
    conditional: bool,
) -> crate::Result<policy::Installer> {
    let (asset, metadata) = metadata_functions(conditional);
    let manifest = asset(&candidate.manifest, working)?;
    let checksums = asset(&candidate.checksums, working)?;
    let installer = policy::installer(candidate, &manifest, &checksums)?;
    let commit = metadata(
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
    let (_, candidate) = policy::select(&release_catalog(working)?, env!("CARGO_PKG_VERSION"))?;
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
    verify_asset(directory, &installer)
}

fn verify_asset(
    directory: &Path,
    installer: &policy::Installer,
) -> crate::Result<bonsai_artifacts::VerifiedFile> {
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
    transfer::write_current(handoff, &payload)?;
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
        Request::PortableCheck => {
            // Discovery is read-only. Admit the running bundle without hashing
            // every extracted application executable on each timer tick. The
            // download and installation gates still verify the full package.
            crate::chat_package::bundle::update_identity()?;
            let working = cache_root()?;
            let (published, candidate) = policy::select_portable(
                &cached_release_catalog(&working)?,
                env!("CARGO_PKG_VERSION"),
            )?;
            if let Some(candidate) = candidate {
                let published = portable_release_metadata(&candidate, &working, true)?;
                Ok(json!({"Status": "available", "Version": candidate.version,
                    "ReleaseIdentity": published.release_identity()?}))
            } else {
                Ok(json!({"Status": if published { "current" } else { "no-release" }}))
            }
        }
        Request::PortableDownload { version } => portable_download(&version),
        Request::PortableInstall {
            version,
            handoff_path,
            release_identity,
            restart,
        } => portable_queue(&version, &handoff_path, &release_identity, restart),
        Request::ReplacePortable(request) => {
            let result = portable::replace(&request);
            #[cfg(windows)]
            if let Err(error) = &result {
                bonsai_windows_process::show_startup_error(error);
            }
            result
        }
        Request::Check {
            installation_directory,
        } => {
            installation(&installation_directory, true)?;
            let working = cache_root()?;
            let (published, candidate) = discover(&working)?;
            if let Some(candidate) = candidate {
                installer_release_metadata(&candidate, &working, true)?;
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
    let bytes = transfer::read_bounded(handoff, 8192)?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    if value["distribution"] == "portable-executable" {
        return apply_portable_after_exit(handoff, value);
    }
    let request: Handoff = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
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

fn apply_portable_after_exit(handoff: &Path, value: Value) -> crate::Result<Value> {
    let queued: PortableHandoff =
        serde_json::from_value(value).map_err(|error| error.to_string())?;
    Version::parse(&queued.version)?.newer_than_current()?;
    portable_installation()?;
    let directory = staged(&queued.version)?;
    let stage_lock = crate::startup_cache::lock_file(&directory.join("update.lock"))?;
    let candidate = published_portable_release(&queued.version, &directory)?;
    let published = verified_portable_release(&candidate, &directory)?;
    queued.verify(&published)?;
    let immutable = verify_stage(&directory, &queued.version, &published)?;
    let target = std::env::current_exe().map_err(|error| error.to_string())?;
    let previous_sha256 = crate::startup_cache::digest_file(&target)?;
    let arguments = std::env::args_os()
        .skip(1)
        .map(|arg| {
            arg.into_string()
                .map_err(|_| "Portable restart arguments must be Unicode.".to_owned())
        })
        .collect::<crate::Result<Vec<_>>>()?;
    let request = json!({"operation": "replace-portable", "target": target,
        "previousSha256": previous_sha256, "version": queued.version, "arguments": arguments,
        "releaseIdentity": queued.release_identity, "restart": queued.restart});
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        use std::process::{Command, Stdio};
        // Owned UI, model and speech jobs have stopped. Only this fixed helper
        // outlives the launcher, briefly, to replace its locked executable.
        fs::remove_file(handoff).map_err(|error| error.to_string())?;
        // The verified image stays immutable through process creation. Let the
        // helper acquire staging coordination during its independent validation.
        drop(stage_lock);
        let child = Command::new(immutable.path())
            .arg("app-update")
            .arg("--json")
            .arg(request.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x0800_0000)
            .spawn()
            .map_err(|error| error.to_string())?;
        Ok(
            json!({"Status": "portable-update-started", "Version": queued.version, "HelperProcessId": child.id()}),
        )
    }
    #[cfg(not(windows))]
    {
        let _ = (immutable, request, handoff, stage_lock);
        Err("Portable replacement requires Windows.".into())
    }
}

#[cfg(all(test, windows))]
mod tests;
