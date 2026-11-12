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

const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
const MAX_OUTPUT: u64 = 64 * 1024;

#[derive(Clone)]
struct Installation {
    tool: PathBuf,
    root: PathBuf,
    handoff: PathBuf,
}

impl Installation {
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
            Self::Check | Self::Install => Duration::from_secs(60),
            Self::Download => Duration::from_secs(240),
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

fn valid_version(version: &str) -> bool {
    version.len() <= 32
        && version.split('.').count() == 3
        && version
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

fn response(text: &str, operation: Operation, expected: Option<&str>) -> io::Result<Response> {
    let response: Response = serde_json::from_str(text).map_err(io::Error::other)?;
    let valid = match operation {
        Operation::Check => match response.status.as_str() {
            "available" => response.version.as_deref().is_some_and(valid_version),
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
                && expected.is_some_and(valid_version)
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

fn execute(
    installation: &Installation,
    operation: Operation,
    version: Option<&str>,
    cancelled: &AtomicBool,
) -> io::Result<Response> {
    let directory = PrivateDirectory::create(&std::env::temp_dir())?;
    let mut request = serde_json::json!({
        "operation": operation.name(), "installationDirectory": installation.root,
    });
    if let Some(version) = version {
        request["version"] = version.into();
    }
    if operation == Operation::Install {
        request["handoffPath"] = serde_json::json!(installation.handoff);
    }
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
        let installation =
            Installation::from_startup(request.update_tool(), request.update_handoff_path())?;
        let executable = std::env::current_exe().ok()?;
        let bin = executable.parent()?;
        let versions = bin.parent()?.parent()?;
        if executable.file_name()? != "bonsai-chat.exe"
            || bin.file_name()? != "bin"
            || versions.file_name()? != "versions"
            || versions.parent()? != installation.root.as_path()
        {
            return None;
        }
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

    pub(super) fn available(&self) -> bool {
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
                    Operation::Download => "Downloading and verifying the Windows installer...",
                    Operation::Install => {
                        "Preparing the installer. The app will close when ready..."
                    }
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
                        self.status = "Closing Swift to open the Windows installer...".into();
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

    pub(super) fn should_close(&self) -> bool {
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
        ui.label(format!("Installed version {}", env!("CARGO_PKG_VERSION")));
        ui.label(&self.status);
        ui.label(egui::RichText::new("Checks GitHub automatically at startup and every six hours, including published prereleases.").small().weak());
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
            ui.label("The app will close. Unsaved conversations follow your saving setting. Windows may ask for administrator approval. Reopen Swift after installation.");
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

    #[test]
    fn update_results_bind_download_and_install_to_the_selected_version() -> io::Result<()> {
        assert!(response(r#"{"Status":"no-release"}"#, Operation::Check, None).is_ok());
        assert!(
            response(
                r#"{"Status":"available","Version":"0.2.0"}"#,
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
                r#"{"Status":"downloaded","Version":"0.2.0"}"#,
                Operation::Download,
                Some("0.2.0")
            )
            .is_ok()
        );
        assert!(
            response(
                r#"{"Status":"queued","Version":"0.1.9"}"#,
                Operation::Install,
                Some("0.2.0")
            )
            .is_err()
        );
        let selected = response(
            r#"{"Status":"queued","Version":"0.2.0"}"#,
            Operation::Install,
            Some("0.2.0"),
        )?;
        assert_eq!(selected.status, "queued");
        Ok(())
    }

    #[test]
    fn source_and_portable_tools_do_not_enable_automatic_updates() {
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
}
