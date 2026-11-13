//! Cancellable installed-app updates through the trusted native maintenance tool.

use bonsai_windows_credentials::PrivateDirectory;
use bonsai_windows_process::{OwnedProcess, ToolProcessSpec};
use eframe::egui;
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{self, Read as _},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const CHECK_INTERVAL: Duration = Duration::from_hours(6);
const MAX_OUTPUT: u64 = 64 * 1024;

#[derive(Clone)]
struct Installation {
    tool: PathBuf,
    root: PathBuf,
    handoff: PathBuf,
    portable: bool,
}

impl Installation {
    fn for_launch(
        tool: &Path,
        handoff: PathBuf,
        executable: &Path,
        portable: bool,
    ) -> Option<Self> {
        let bin = executable.parent()?;
        let versions = bin.parent()?.parent()?;
        let root = versions.parent()?;
        let installation = if portable {
            if !tool.is_absolute()
                || !handoff.is_absolute()
                || root.parent()?.file_name()? != "application-bundles"
            {
                return None;
            }
            Self {
                tool: tool.into(),
                root: root.into(),
                handoff,
                portable: true,
            }
        } else {
            Self::from_startup(tool, handoff)?
        };
        if executable.file_name()? != "bonsai-chat.exe"
            || bin.file_name()? != "bin"
            || versions.file_name()? != "versions"
            || root != installation.root
        {
            return None;
        }
        Some(installation)
    }

    fn from_startup(tool: &Path, handoff: PathBuf) -> Option<Self> {
        let maintenance = tool.parent()?;
        if !tool.is_absolute()
            || tool.file_name()? != "bonsai-tools.exe"
            || maintenance.file_name()? != "maintenance"
            || !handoff.is_absolute()
        {
            return None;
        }
        Some(Self {
            tool: tool.into(),
            root: maintenance.parent()?.into(),
            handoff,
            portable: false,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Operation {
    Check,
    Download,
    Install,
}

impl Operation {
    const fn name(self) -> &'static str {
        match self {
            Self::Check => "check",
            Self::Download => "download",
            Self::Install => "install",
        }
    }

    const fn deadline(self) -> Duration {
        match self {
            Self::Check | Self::Install => Duration::from_secs(90),
            Self::Download => Duration::from_secs(300),
        }
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Response {
    status: String,
    #[serde(default)]
    version: Option<String>,
}

fn parse_version(text: &str) -> Option<(u8, u8, u16)> {
    if text.len() > 13
        || text.split('.').any(|part| {
            part.is_empty()
                || (part.len() > 1 && part.starts_with('0'))
                || !part.bytes().all(|byte| byte.is_ascii_digit())
        })
    {
        return None;
    }
    let mut parts = text.split('.');
    let parsed = (
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    );
    parts.next().is_none().then_some(parsed)
}

fn newer_version(text: &str) -> bool {
    matches!(
        (parse_version(text), parse_version(env!("CARGO_PKG_VERSION"))),
        (Some(candidate), Some(current)) if candidate > current
    )
}

fn response(text: &str, operation: Operation, expected: Option<&str>) -> io::Result<Response> {
    let response: Response = serde_json::from_str(text).map_err(io::Error::other)?;
    let valid = match operation {
        Operation::Check => match response.status.as_str() {
            "available" => response.version.as_deref().is_some_and(newer_version),
            "current" | "no-release" => true,
            _ => false,
        },
        Operation::Download | Operation::Install => {
            response.status
                == if operation == Operation::Download {
                    "downloaded"
                } else {
                    "queued"
                }
                && expected.is_some_and(newer_version)
                && response.version.as_deref() == expected
        }
    };
    if !valid {
        return Err(io::Error::other(
            "The update tool returned an unexpected result.",
        ));
    }
    Ok(response)
}

fn bounded_text(path: &Path) -> io::Result<String> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_OUTPUT + 1)
        .read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).map_err(io::Error::other)? > MAX_OUTPUT {
        return Err(io::Error::other("Update tool output exceeded its limit."));
    }
    String::from_utf8(bytes).map_err(io::Error::other)
}

struct Process(OwnedProcess);

impl Drop for Process {
    fn drop(&mut self) {
        let _stopped = self.0.stop(Duration::from_secs(3));
    }
}

fn update_request(
    installation: &Installation,
    operation: Operation,
    version: Option<&str>,
) -> serde_json::Value {
    let mut request = if installation.portable {
        serde_json::json!({"operation": format!("portable-{}", operation.name())})
    } else {
        serde_json::json!({"operation": operation.name(), "installationDirectory": installation.root})
    };
    if let Some(version) = version.filter(|_| operation != Operation::Check) {
        request["version"] = version.into();
    }
    if operation == Operation::Install {
        request["handoffPath"] = serde_json::json!(installation.handoff);
    }
    request
}

fn execute(
    installation: &Installation,
    operation: Operation,
    version: Option<&str>,
    cancelled: &AtomicBool,
) -> io::Result<Response> {
    let directory = PrivateDirectory::create(&std::env::temp_dir())?;
    let request = update_request(installation, operation, version);
    let path = directory.path().join("request.json");
    fs::write(
        &path,
        serde_json::to_vec(&request).map_err(io::Error::other)?,
    )?;
    let spec = ToolProcessSpec {
        executable: installation.tool.clone(),
        arguments: vec![
            "app-update".into(),
            "--request".into(),
            path.into_os_string(),
        ],
        working_directory: directory.path().into(),
        stdout: directory.path().join("stdout.json"),
        stderr: directory.path().join("stderr.txt"),
        environment: BTreeMap::new(),
    };
    if cancelled.load(Ordering::Acquire) {
        return Err(io::Error::other("Update operation cancelled."));
    }
    let child = Process(OwnedProcess::start_tool(&spec)?);
    let started = Instant::now();
    let code = loop {
        if cancelled.load(Ordering::Acquire) {
            return Err(io::Error::other("Update operation cancelled."));
        }
        if started.elapsed() >= operation.deadline() {
            return Err(io::Error::other("The update operation timed out."));
        }
        if fs::metadata(&spec.stdout)?.len() > MAX_OUTPUT
            || fs::metadata(&spec.stderr)?.len() > MAX_OUTPUT
        {
            return Err(io::Error::other("Update tool output exceeded its limit."));
        }
        if let Some(code) = child.0.try_wait()? {
            break code;
        }
        thread::sleep(Duration::from_millis(20));
    };
    child.0.stop(Duration::from_secs(3))?;
    if code != 0 {
        return Err(io::Error::other(bounded_text(&spec.stderr)?));
    }
    response(&bounded_text(&spec.stdout)?, operation, version)
}

struct Task {
    operation: Operation,
    incoming: Receiver<io::Result<Response>>,
    cancelled: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Task {
    fn start(
        installation: Installation,
        operation: Operation,
        version: Option<String>,
    ) -> io::Result<Self> {
        let (outgoing, incoming) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancellation = Arc::clone(&cancelled);
        let worker = thread::Builder::new()
            .name("swift-app-update".into())
            .spawn(move || {
                let result = execute(&installation, operation, version.as_deref(), &cancellation);
                let _sent = outgoing.send(result);
            })?;
        Ok(Self {
            operation,
            incoming,
            cancelled,
            worker: Some(worker),
        })
    }
}

impl Drop for Task {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _joined = worker.join();
        }
    }
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "Downloaded bytes, confirmation, model teardown and the queued installer retain separate state."
)]
pub(super) struct Updates {
    installation: Installation,
    task: Option<Task>,
    version: Option<String>,
    downloaded: bool,
    queued: bool,
    confirming: bool,
    waiting_for_stop: bool,
    next_check: Instant,
    status: String,
}

impl Updates {
    pub(super) fn for_startup(request: &crate::startup::Request) -> Option<Self> {
        let executable = std::env::current_exe().ok()?;
        let installation = Installation::for_launch(
            request.update_tool(),
            request.update_handoff_path(),
            &executable,
            request.portable_update,
        )?;
        Some(Self {
            installation,
            task: None,
            version: None,
            downloaded: false,
            queued: false,
            confirming: false,
            waiting_for_stop: false,
            next_check: Instant::now(),
            status: "Checking for application updates shortly.".into(),
        })
    }

    pub(super) const fn available(&self) -> bool {
        self.version.is_some()
    }

    pub(super) fn installing(&self) -> bool {
        self.waiting_for_stop
            || self.queued
            || self
                .task
                .as_ref()
                .is_some_and(|task| task.operation == Operation::Install)
    }

    fn start(&mut self, operation: Operation) {
        if self.task.is_some() || self.queued {
            return;
        }
        self.confirming = false;
        if operation == Operation::Check {
            self.next_check = Instant::now() + CHECK_INTERVAL;
        }
        match Task::start(self.installation.clone(), operation, self.version.clone()) {
            Ok(task) => {
                self.task = Some(task);
                self.status = match operation {
                    Operation::Check => "Checking GitHub for updates...",
                    Operation::Download => "Downloading and verifying the application update...",
                    Operation::Install => "Preparing the update. The app will close when ready...",
                }
                .into();
            }
            Err(_) => self.status = "Could not start the update check. Try again.".into(),
        }
    }

    /// Polls one finite worker result and schedules at most one periodic check.
    pub(super) fn poll(&mut self) -> Option<String> {
        if self.task.is_none() && !self.installing() && Instant::now() >= self.next_check {
            self.start(Operation::Check);
        }
        let task = self.task.as_ref()?;
        let result = match task.incoming.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err(io::Error::other("The update worker stopped unexpectedly."))
            }
        };
        let operation = task.operation;
        self.task.take();
        match result {
            Ok(result) => {
                match result.status.as_str() {
                    "available" => {
                        self.downloaded = self.downloaded && self.version == result.version;
                        self.version = result.version;
                        self.status = self.version.as_ref().map_or_else(String::new, |version| {
                            format!("Version {version} is available.")
                        });
                        if self.installation.portable && !self.downloaded {
                            self.start(Operation::Download);
                        }
                    }
                    "current" => {
                        self.version = None;
                        self.downloaded = false;
                        self.status = "You are using the latest published version.".into();
                    }
                    "no-release" => {
                        self.version = None;
                        self.downloaded = false;
                        self.status = "No published releases yet.".into();
                    }
                    "downloaded" => {
                        self.downloaded = true;
                        self.status = "Update verified and ready to install.".into();
                    }
                    "queued" => {
                        self.queued = true;
                        self.status = if self.installation.portable {
                            "Closing Swift to replace the executable and reopen..."
                        } else {
                            "Closing Swift to open the Windows installer..."
                        }
                        .into();
                    }
                    _ => {}
                }
                None
            }
            Err(error) => {
                self.status = if operation == Operation::Check {
                    "Could not check for updates. Try again or open diagnostics for details."
                } else {
                    "The update did not complete. Try again or open diagnostics for details."
                }
                .into();
                Some(format!("Application update: {error}"))
            }
        }
    }

    pub(super) const fn should_close(&self) -> bool {
        self.queued
    }

    pub(super) fn wait_for_stop(&mut self) {
        self.waiting_for_stop = true;
        self.status = "Stopping the local model before installation...".into();
    }

    pub(super) fn finish_stop(&mut self, stopped: bool) {
        if !self.waiting_for_stop {
            return;
        }
        self.waiting_for_stop = false;
        if stopped {
            self.start(Operation::Install);
        } else {
            self.status =
                "The model could not stop safely. Close Swift before updating manually.".into();
        }
    }

    pub(super) fn controls(&mut self, ui: &mut egui::Ui, can_install: bool) -> bool {
        ui.heading("Application updates");
        ui.label(format!("Application version {}", env!("CARGO_PKG_VERSION")));
        ui.label(&self.status);
        ui.label(egui::RichText::new("Checks GitHub automatically at startup and every six hours, including published prereleases.").small().weak());
        if self.installation.portable {
            ui.label("New releases download automatically. Replacing the executable requires your confirmation.");
        }
        let idle = self.task.is_none() && !self.installing();
        if ui
            .add_enabled(idle, egui::Button::new("Check for updates"))
            .clicked()
        {
            self.start(Operation::Check);
        }
        if self.version.is_some()
            && !self.downloaded
            && ui
                .add_enabled(idle, egui::Button::new("Download update"))
                .clicked()
        {
            self.start(Operation::Download);
        }
        if self.downloaded {
            if ui
                .add_enabled(
                    idle && can_install,
                    egui::Button::new("Install update and close"),
                )
                .clicked()
            {
                self.confirming = true;
            }
            if !can_install && !self.installing() {
                ui.label(egui::RichText::new("Finish the response and wait for model preparation or shutdown before installing.").small().weak());
            }
        }
        let mut install = false;
        if self.confirming {
            ui.label(if self.installation.portable {
                "Swift will close, replace this executable and reopen with the same options and data directory. Unsaved conversations follow your saving setting."
            } else { "The app will close. Unsaved conversations follow your saving setting. Windows may ask for administrator approval. Reopen Swift after installation." });
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(can_install && idle, egui::Button::new("Close and install"))
                    .clicked()
                {
                    self.confirming = false;
                    install = true;
                }
                if ui.button("Cancel update").clicked() {
                    self.confirming = false;
                }
            });
        }
        ui.add_space(16.0);
        install
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn next_version(text: &str) -> io::Result<String> {
        let (major, minor, patch) =
            parse_version(text).ok_or_else(|| io::Error::other("Invalid fixture version."))?;
        let next = if let Some(patch) = patch.checked_add(1) {
            (major, minor, patch)
        } else if let Some(minor) = minor.checked_add(1) {
            (major, minor, 0)
        } else if let Some(major) = major.checked_add(1) {
            (major, 0, 0)
        } else {
            return Err(io::Error::other("No newer fixture version is available."));
        };
        Ok(format!("{}.{}.{}", next.0, next.1, next.2))
    }

    #[test]
    fn update_results_reject_noncanonical_out_of_bounds_and_nonnewer_versions() {
        assert_eq!(parse_version("255.255.65535"), Some((255, 255, 65535)));
        for invalid in [
            "",
            "0.01.1",
            "00.2.0",
            "0.2",
            "0.2.0.0",
            "0.2.0-beta",
            "256.0.0",
            "0.256.0",
            "0.0.65536",
            "0.2.+1",
            "0.0.0",
            env!("CARGO_PKG_VERSION"),
        ] {
            for (status, operation) in [
                ("available", Operation::Check),
                ("downloaded", Operation::Download),
                ("queued", Operation::Install),
            ] {
                let text = serde_json::json!({"Status": status, "Version": invalid}).to_string();
                assert!(response(&text, operation, Some(invalid)).is_err());
            }
        }
    }

    #[test]
    fn update_results_bind_download_and_install_to_the_selected_version() -> io::Result<()> {
        let candidate = next_version(env!("CARGO_PKG_VERSION"))?;
        let mismatched = next_version(&candidate)?;
        assert!(response(r#"{"Status":"no-release"}"#, Operation::Check, None).is_ok());
        assert!(
            response(
                &serde_json::json!({"Status": "available", "Version": &candidate}).to_string(),
                Operation::Check,
                None
            )
            .is_ok()
        );
        assert!(
            response(
                r#"{"Status":"available","Version":"../other"}"#,
                Operation::Check,
                None
            )
            .is_err()
        );
        assert!(
            response(
                &serde_json::json!({"Status": "downloaded", "Version": &candidate}).to_string(),
                Operation::Download,
                Some(&candidate)
            )
            .is_ok()
        );
        assert!(
            response(
                &serde_json::json!({"Status": "queued", "Version": mismatched}).to_string(),
                Operation::Install,
                Some(&candidate)
            )
            .is_err()
        );
        let selected = response(
            &serde_json::json!({"Status": "queued", "Version": &candidate}).to_string(),
            Operation::Install,
            Some(&candidate),
        )?;
        assert_eq!(selected.status, "queued");
        assert_eq!(selected.version.as_deref(), Some(candidate.as_str()));
        Ok(())
    }

    #[test]
    fn repeat_checks_omit_selected_version_and_portable_requests_never_select_msi() {
        let root = std::env::temp_dir().join("Bonsai portable request fixture");
        let mut installation = Installation {
            tool: root.join("Swift.exe"),
            root: root.clone(),
            handoff: root.join("chat-startup-fixture.update.json"),
            portable: true,
        };
        for portable in [true, false] {
            installation.portable = portable;
            let check = update_request(&installation, Operation::Check, Some("0.9.0"));
            assert!(check.get("version").is_none());
            assert_eq!(
                check["operation"],
                if portable { "portable-check" } else { "check" }
            );
            let download = update_request(&installation, Operation::Download, Some("0.9.0"));
            assert_eq!(download["version"], "0.9.0");
            assert_eq!(
                download["operation"],
                if portable {
                    "portable-download"
                } else {
                    "download"
                }
            );
            assert_eq!(download.get("installationDirectory").is_none(), portable);
        }
    }

    #[test]
    fn only_maintenance_layout_enables_legacy_installer_updates() {
        let root = std::env::temp_dir().join("Bonsai update fixture");
        let handoff = root.join("session.update.json");
        assert!(
            Installation::from_startup(&root.join("bin/bonsai-tools.exe"), handoff.clone())
                .is_none()
        );
        assert!(
            Installation::from_startup(&root.join("maintenance/bonsai-tools.exe"), handoff)
                .is_some()
        );
    }

    #[test]
    fn portable_updates_require_explicit_bundle_launch_and_accept_renamed_executables()
    -> io::Result<()> {
        let root = std::env::temp_dir().join("Bonsai portable update fixture");
        let tool = root.join("My Swift.exe");
        let handoff = root.join("chat-startup-fixture.update.json");
        let chat = root.join("application-bundles/digest/versions/package/bin/bonsai-chat.exe");
        assert!(Installation::for_launch(&tool, handoff.clone(), &chat, false).is_none());
        let selected = Installation::for_launch(&tool, handoff.clone(), &chat, true)
            .ok_or_else(|| io::Error::other("Rejected a renamed portable launch."))?;
        assert!(selected.portable);
        assert_eq!(selected.tool, tool);
        assert!(
            Installation::for_launch(&tool, handoff, &root.join("bin/bonsai-chat.exe"), true)
                .is_none()
        );
        Ok(())
    }
}
