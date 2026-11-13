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

const CHECK_INTERVAL: Duration = Duration::from_mins(15);
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
    #[serde(default)]
    release_identity: Option<String>,
}

fn release_identity(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
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

fn verified_response(
    text: &str,
    operation: Operation,
    expected: Option<&str>,
    portable: bool,
) -> io::Result<Response> {
    let result = response(text, operation, expected)?;
    if portable
        && matches!(result.status.as_str(), "available" | "downloaded")
        && !result
            .release_identity
            .as_deref()
            .is_some_and(release_identity)
    {
        return Err(io::Error::other(
            "The update tool did not identify the exact verified portable release. Check for updates again.",
        ));
    }
    Ok(result)
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
    identity: Option<&str>,
    restart: bool,
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
        if installation.portable
            && let Some(identity) = identity
        {
            request["releaseIdentity"] = identity.into();
            request["restart"] = restart.into();
        }
    }
    request
}

fn execute(
    installation: &Installation,
    operation: Operation,
    version: Option<&str>,
    identity: Option<&str>,
    restart: bool,
    cancelled: &AtomicBool,
) -> io::Result<Response> {
    let directory = PrivateDirectory::create(&std::env::temp_dir())?;
    let request = update_request(installation, operation, version, identity, restart);
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
    let outcome = (|| loop {
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
            break Ok(code);
        }
        thread::sleep(Duration::from_millis(250));
    })();
    child.0.stop(Duration::from_secs(3)).map_err(|error| {
        io::Error::other(format!(
            "Application update process cleanup could not be confirmed. Close Swift before trying another update. {error}"
        ))
    })?;
    let code = outcome?;
    if code != 0 {
        return Err(io::Error::other(bounded_text(&spec.stderr)?));
    }
    verified_response(
        &bounded_text(&spec.stdout)?,
        operation,
        version,
        installation.portable,
    )
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
        identity: Option<String>,
        restart: bool,
    ) -> io::Result<Self> {
        let (outgoing, incoming) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancellation = Arc::clone(&cancelled);
        let worker = thread::Builder::new()
            .name("swift-app-update".into())
            .spawn(move || {
                let result = execute(
                    &installation,
                    operation,
                    version.as_deref(),
                    identity.as_deref(),
                    restart,
                    &cancellation,
                );
                let _sent = outgoing.send(result);
            })?;
        Ok(Self {
            operation,
            incoming,
            cancelled,
            worker: Some(worker),
        })
    }

    fn cancel_for_close(&mut self) -> io::Result<()> {
        self.cancelled.store(true, Ordering::Release);
        if self
            .worker
            .take()
            .is_some_and(|worker| worker.join().is_err())
        {
            return Err(io::Error::other(
                "Application update process cleanup could not be confirmed. The update worker panicked during cancellation.",
            ));
        }
        match self.incoming.try_recv() {
            Ok(Err(error))
                if error
                    .to_string()
                    .contains("Application update process cleanup could not be confirmed.") =>
            {
                Err(error)
            }
            Ok(_) => Ok(()),
            Err(_) => Err(io::Error::other(
                "Application update process cleanup could not be confirmed. The cancelled update worker returned no result.",
            )),
        }
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
    release_identity: Option<String>,
    downloaded: bool,
    queued: bool,
    confirming: bool,
    confirmed: bool,
    waiting_for_stop: bool,
    cleanup_unconfirmed: bool,
    notified: bool,
    deferred: bool,
    close_requested: bool,
    next_check: Instant,
    status: String,
    error: Option<String>,
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
            release_identity: None,
            downloaded: false,
            queued: false,
            confirming: false,
            confirmed: false,
            waiting_for_stop: false,
            cleanup_unconfirmed: false,
            notified: false,
            deferred: false,
            close_requested: false,
            next_check: Instant::now(),
            status: "Checking for application updates shortly.".into(),
            error: None,
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

    fn verified_download(&self) -> bool {
        self.downloaded
            && self.version.as_deref().is_some_and(newer_version)
            && (!self.installation.portable
                || self
                    .release_identity
                    .as_deref()
                    .is_some_and(release_identity))
    }

    fn select_candidate(&mut self, result: Response) -> bool {
        let same = self.version == result.version
            && (!self.installation.portable || self.release_identity == result.release_identity);
        self.downloaded = self.downloaded && same;
        if !same {
            self.confirming = false;
            self.confirmed = false;
            self.notified = false;
            self.deferred = false;
        }
        if !self.downloaded {
            self.release_identity = None;
        }
        self.version = result.version;
        self.status = self.version.as_ref().map_or_else(String::new, |version| {
            format!("Version {version} is available.")
        });
        self.installation.portable && !self.downloaded
    }

    fn start(&mut self, operation: Operation) {
        if self.task.is_some() || self.queued || self.cleanup_unconfirmed {
            return;
        }
        self.confirming = false;
        self.error = None;
        if operation != Operation::Install {
            self.confirmed = false;
        }
        if operation == Operation::Check {
            self.next_check = Instant::now() + CHECK_INTERVAL;
        }
        match Task::start(
            self.installation.clone(),
            operation,
            self.version.clone(),
            self.release_identity.clone(),
            !self.close_requested,
        ) {
            Ok(task) => {
                self.task = Some(task);
                self.status = match operation {
                    Operation::Check => "Checking GitHub for updates...",
                    Operation::Download => "Downloading and verifying the application update...",
                    Operation::Install => "Preparing the update. The app will close when ready...",
                }
                .into();
            }
            Err(error) => {
                self.confirmed = false;
                self.status = "Could not start the update check. Try again.".into();
                self.error = Some(error.to_string());
            }
        }
    }

    /// Polls one finite worker result and schedules at most one periodic check.
    pub(super) fn poll(&mut self) -> Option<String> {
        if self.task.is_none()
            && !self.installing()
            && !self.cleanup_unconfirmed
            && !self.close_requested
            && Instant::now() >= self.next_check
        {
            self.start(Operation::Check);
        }
        let task = self.task.as_ref()?;
        let result = match task.incoming.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return None,
            Err(mpsc::TryRecvError::Disconnected) => Err(io::Error::other(
                "Application update process cleanup could not be confirmed. The update worker stopped unexpectedly. Close Swift before trying another update.",
            )),
        };
        let operation = task.operation;
        self.task.take();
        match result {
            Ok(result) => self.operation_finished(operation, result),
            Err(error) => Some(self.operation_failed(operation, &error)),
        }
    }

    fn operation_finished(&mut self, operation: Operation, result: Response) -> Option<String> {
        self.error = None;
        match result.status.as_str() {
            "available" => {
                if self.select_candidate(result) {
                    self.start(Operation::Download);
                }
            }
            "current" => {
                self.version = None;
                self.release_identity = None;
                self.downloaded = false;
                self.confirming = false;
                self.deferred = false;
                self.status = "You are using the latest published version.".into();
            }
            "no-release" => {
                self.version = None;
                self.release_identity = None;
                self.downloaded = false;
                self.confirming = false;
                self.deferred = false;
                self.status = "No published releases yet.".into();
            }
            "downloaded" => {
                self.release_identity = result.release_identity;
                self.downloaded = true;
                self.status = "Update verified and ready to install.".into();
            }
            "queued" => {
                if operation != Operation::Install || !self.confirmed || !self.verified_download() {
                    self.confirmed = false;
                    self.status = "The update was not approved for this verified download. Check for updates and confirm again.".into();
                    self.error = Some(self.status.clone());
                    return Some("Application update: rejected a queued result without current confirmation and a verified download.".into());
                }
                self.queued = true;
                self.status = if self.close_requested {
                    "Closing Swift to replace the executable..."
                } else if self.installation.portable {
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

    fn operation_failed(&mut self, operation: Operation, error: &io::Error) -> String {
        self.error = Some(error.to_string());
        self.cleanup_unconfirmed = error
            .to_string()
            .contains("Application update process cleanup could not be confirmed.");
        if operation == Operation::Install {
            // Failed queue preparation never carries an old approval or
            // a potentially changed publication into another attempt.
            self.confirming = false;
            self.confirmed = false;
            self.downloaded = false;
            self.release_identity = None;
            if !self.cleanup_unconfirmed {
                self.next_check = Instant::now();
            }
        }
        self.status = if self.cleanup_unconfirmed {
            "Update cleanup could not be confirmed. Close Swift before trying another update."
        } else if operation == Operation::Check {
            "Could not check for updates. Try again or open diagnostics for details."
        } else {
            "The update did not complete. Try again or open diagnostics for details."
        }
        .into();
        format!("Application update: {error}")
    }

    pub(super) const fn should_close(&self) -> bool {
        self.queued && self.confirmed
    }

    pub(super) const fn closing(&self) -> bool {
        self.close_requested
    }

    /// Normal close authorizes only the exact portable update previously
    /// deferred with Later. An unfinished download never delays app shutdown.
    pub(super) fn approve_on_close(&mut self) -> bool {
        if !self.installation.portable
            || !self.deferred
            || self.cleanup_unconfirmed
            || !self.verified_download()
            || self.queued
            || self
                .task
                .as_ref()
                .is_some_and(|task| task.operation != Operation::Check)
        {
            return false;
        }
        // A redundant metadata check may be cancelled just as on ordinary
        // shutdown. Queueing still freshly validates publication and staging.
        if let Some(mut task) = self.task.take()
            && let Err(error) = task.cancel_for_close()
        {
            self.operation_failed(Operation::Check, &error);
            return false;
        }
        self.confirming = false;
        self.confirmed = true;
        self.close_requested = true;
        self.wait_for_stop();
        self.waiting_for_stop
    }

    fn defer(&mut self) {
        self.notified = true;
        self.deferred = self.installation.portable && self.verified_download();
        self.confirming = false;
        self.confirmed = false;
    }

    pub(super) fn notification_pending(&self) -> bool {
        self.installation.portable
            && self.verified_download()
            && !self.notified
            && !self.installing()
            && self.task.is_none()
            && !self.cleanup_unconfirmed
    }

    pub(super) fn notification(&mut self, ctx: &egui::Context) -> bool {
        let mut install = false;
        let mut later = false;
        let width = (ctx.content_rect().width() - 64.0).clamp(240.0, 420.0);
        let modal = egui::Modal::new(egui::Id::new("application-update-prompt")).show(ctx, |ui| {
            ui.set_width(width);
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
            ui.heading("Update ready");
            ui.label(format!("Swift {} is downloaded and verified.", self.version.as_deref().unwrap_or_default()));
            ui.label("Update now to restart Swift, or choose Later to install when you close it. Your saving setting still applies.");
            ui.add_space(10.0);
            ui.horizontal_wrapped(|ui| {
                install = ui.add(update_button(ui, "Update and restart", true)).clicked();
                later = ui.add(update_button(ui, "Later", false)).clicked();
            });
        });
        if install {
            self.notified = true;
            self.confirmed = true;
        } else if later || modal.should_close() {
            self.defer();
        }
        install
    }

    pub(super) fn wait_for_stop(&mut self) {
        if self.task.is_some()
            || self.queued
            || self.cleanup_unconfirmed
            || !self.confirmed
            || !self.verified_download()
        {
            return;
        }
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
            self.confirmed = false;
            self.status =
                "The model could not stop safely. Close Swift before updating manually.".into();
            self.error = Some(self.status.clone());
        }
    }

    pub(super) fn error_notice(&self) -> Option<(&str, &str)> {
        self.error
            .as_deref()
            .map(|error| (self.status.as_str(), error))
    }

    fn status_controls(&self, ui: &mut egui::Ui) {
        status_frame(ui, |ui| {
            ui.horizontal_top(|ui| {
                if self.task.is_some() || self.installing() {
                    ui.spinner();
                }
                super::error_ui::summary(ui, &self.status);
            });
            if self.installation.portable {
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(
                        "New releases download automatically. Choose Update now, or Later to install when you close Swift.",
                    )
                    .small()
                    .weak(),
                );
            }
        });
    }

    pub(super) fn controls(&mut self, ui: &mut egui::Ui, can_install: bool) -> bool {
        update_header(ui);
        self.status_controls(ui);
        ui.add_space(10.0);
        let idle = self.task.is_none() && !self.installing() && !self.cleanup_unconfirmed;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
            if self.downloaded
                && !self.confirming
                && ui
                    .add_enabled(
                        idle && can_install && self.verified_download(),
                        update_button(ui, "Install update and close", true),
                    )
                    .clicked()
            {
                self.confirming = true;
            }
            if self.version.is_some()
                && !self.downloaded
                && ui
                    .add_enabled(idle, update_button(ui, "Download update", true))
                    .clicked()
            {
                self.start(Operation::Download);
            }
            if ui
                .add_enabled(
                    idle,
                    update_button(ui, "Check for updates", self.version.is_none()),
                )
                .clicked()
            {
                self.start(Operation::Check);
            }
        });
        if self.downloaded && !can_install && !self.installing() {
            ui.add_space(4.0);
            ui.label(egui::RichText::new("Finish the response and wait for model preparation or shutdown before installing.").small().weak());
        }
        let mut install = false;
        if self.confirming {
            ui.add_space(10.0);
            status_frame(ui, |ui| {
                ui.label(egui::RichText::new("Ready to close and install?").strong());
                ui.add_space(4.0);
                ui.label(if self.installation.portable {
                    "Swift will close, replace this executable and reopen with the same options and data directory. Unsaved conversations follow your saving setting."
                } else { "The app will close. Unsaved conversations follow your saving setting. Windows may ask for administrator approval. Reopen Swift after installation." });
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
                    if ui
                        .add_enabled(
                            can_install && idle && self.verified_download(),
                            update_button(ui, "Close and install", true),
                        )
                        .clicked()
                    {
                        self.confirming = false;
                        self.confirmed = true;
                        self.notified = true;
                        install = true;
                    }
                    let decline = if self.installation.portable {
                        "Later"
                    } else {
                        "Cancel update"
                    };
                    if ui.add(update_button(ui, decline, false)).clicked() {
                        self.defer();
                    }
                });
            });
        }
        ui.add_space(10.0);
        ui.label(
            egui::RichText::new(
                "Checks GitHub at startup and every 15 minutes. Published prereleases are included.",
            )
            .small()
            .weak(),
        );
        install
    }
}

fn update_header(ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(8.0, 6.0);
        ui.label(
            egui::RichText::new("Application updates")
                .strong()
                .size(16.0),
        );
        egui::Frame::new()
            .fill(ui.visuals().extreme_bg_color)
            .corner_radius(6)
            .inner_margin(egui::Margin::symmetric(8, 4))
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                        .small()
                        .monospace(),
                )
                .on_hover_text("Application version");
            });
    });
    ui.add_space(10.0);
}

fn status_frame(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(ui.visuals().faint_bg_color)
        .corner_radius(8)
        .inner_margin(12)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
            contents(ui);
        });
}

fn update_button<'a>(ui: &egui::Ui, label: &'a str, primary: bool) -> egui::Button<'a> {
    let button = egui::Button::new(egui::RichText::new(label).color(if primary {
        ui.visuals().selection.stroke.color
    } else {
        ui.visuals().text_color()
    }))
    .min_size(egui::vec2(0.0, 34.0))
    .corner_radius(8);
    if primary {
        button.fill(ui.visuals().selection.bg_fill)
    } else {
        button
    }
}

/// Presents manual update guidance when no installed-app updater is attached.
pub(super) fn manual_controls(ui: &mut egui::Ui) {
    update_header(ui);
    status_frame(ui, |ui| {
        ui.label(egui::RichText::new("Manual updates").strong());
        ui.add_space(4.0);
        ui.label("Download the latest Windows ARM64 release from GitHub.");
    });
    ui.add_space(10.0);
    ui.hyperlink_to(
        "Open GitHub releases",
        "https://github.com/symbiogenesis/SwiftBonsai2Runner/releases",
    );
    ui.add_space(10.0);
    ui.label(egui::RichText::new("Close Swift, verify the new executable against its published SHA-256, then replace your copy. Keep the same data directory to preserve models and conversations.").small().weak());
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

    fn update_fixture() -> io::Result<Updates> {
        let root = std::env::temp_dir().join("Bonsai model-free update controls");
        Ok(Updates {
            installation: Installation {
                tool: root.join("Swift.exe"),
                root: root.clone(),
                handoff: root.join("chat-startup-fixture.update.json"),
                portable: true,
            },
            task: None,
            version: Some(next_version(env!("CARGO_PKG_VERSION"))?),
            release_identity: Some("a".repeat(64)),
            downloaded: true,
            queued: false,
            confirming: false,
            confirmed: false,
            waiting_for_stop: false,
            cleanup_unconfirmed: false,
            notified: false,
            deferred: false,
            close_requested: false,
            next_check: Instant::now() + CHECK_INTERVAL,
            status: "Verified fixture update".into(),
            error: None,
        })
    }

    fn completed_task(operation: Operation, result: Option<io::Result<Response>>) -> Task {
        let (outgoing, incoming) = mpsc::sync_channel(1);
        if let Some(result) = result {
            let _sent = outgoing.send(result);
        }
        Task {
            operation,
            incoming,
            cancelled: Arc::new(AtomicBool::new(false)),
            worker: None,
        }
    }

    fn update_state() -> io::Result<(super::super::State, Receiver<crate::client::Command>)> {
        let (commands, incoming) = mpsc::channel();
        let (_events, events) = crate::client::event_channel();
        let mut state = super::super::State::create(commands, events, None, None);
        state.preview = false;
        state.model = super::super::ModelLifecycle::Ready;
        state.input = "Keep this draft".into();
        state.updates = Some(update_fixture()?);
        Ok((state, incoming))
    }

    #[test]
    fn portable_prompt_later_keeps_settings_badge_and_owns_keyboard_shortcuts() -> io::Result<()> {
        use egui_kittest::{Harness, kittest::Queryable as _};
        let (state, commands) = update_state()?;
        let original = state.active_conversation;
        let mut harness = Harness::builder()
            .with_size([1180.0, 860.0])
            .with_context_setup(super::super::configure)
            .build_ui_state(
                |ui, state: &mut super::super::State| {
                    state.handle_input_shortcuts(ui.ctx());
                    state.draw(ui);
                },
                state,
            );
        assert!(harness.query_by_label("Update ready").is_some());
        harness.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::N);
        harness.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::F);
        harness.key_press(egui::Key::Enter);
        harness.run();
        assert_eq!(harness.state().active_conversation, original);
        assert_eq!(harness.state().input, "Keep this draft");
        assert!(commands.try_recv().is_err());
        harness.key_press(egui::Key::Escape);
        harness.run();
        assert!(harness.query_by_label("Update ready").is_none());
        let updates = harness
            .state()
            .updates
            .as_ref()
            .ok_or_else(|| io::Error::other("Missing updates"))?;
        assert!(updates.deferred && updates.downloaded);
        assert!(!updates.notification_pending());
        assert!(!updates.confirmed && !updates.should_close());
        assert!(
            harness
                .query_by_role_and_label(egui::Role::Button, "Settings, update pending")
                .is_some()
        );
        assert!(commands.try_recv().is_err());
        harness.run_steps(2);
        assert!(harness.query_by_label("Update ready").is_none());
        Ok(())
    }

    #[test]
    fn later_is_bound_to_exact_publication_and_close_never_waits_for_download() -> io::Result<()> {
        let mut updates = update_fixture()?;
        updates.defer();
        let same = Response {
            status: "available".into(),
            version: updates.version.clone(),
            release_identity: updates.release_identity.clone(),
        };
        assert!(!updates.select_candidate(same));
        assert!(updates.deferred && !updates.notification_pending());
        updates.task = Some(completed_task(Operation::Download, None));
        assert!(!updates.approve_on_close());
        assert!(!updates.closing() && !updates.confirmed);
        updates.task.take();
        let changed = Response {
            status: "available".into(),
            version: updates.version.clone(),
            release_identity: Some("b".repeat(64)),
        };
        assert!(updates.select_candidate(changed));
        assert!(!updates.deferred && !updates.notified && !updates.downloaded);
        assert!(!updates.approve_on_close());
        updates.release_identity = Some("b".repeat(64));
        updates.downloaded = true;
        assert!(updates.notification_pending());
        updates.installation.portable = false;
        updates.defer();
        assert!(
            !updates.approve_on_close(),
            "MSI still requires explicit installation"
        );
        Ok(())
    }

    #[test]
    fn deferred_close_cancels_check_and_requires_acknowledged_generation_teardown() -> io::Result<()>
    {
        let (mut state, commands) = update_state()?;
        state.model = super::super::ModelLifecycle::Generating;
        let updates = state
            .updates
            .as_mut()
            .ok_or_else(|| io::Error::other("Missing updates"))?;
        updates.defer();
        updates.task = Some(completed_task(
            Operation::Check,
            Some(Err(io::Error::other("Update operation cancelled."))),
        ));
        assert!(state.prepare_update_on_close());
        assert!(matches!(
            commands.try_recv(),
            Ok(crate::client::Command::Stop)
        ));
        assert_eq!(state.model, super::super::ModelLifecycle::Stopping);
        assert!(state.prepare_update_on_close());
        assert!(
            commands.try_recv().is_err(),
            "Repeated close cannot send another Stop"
        );
        let updates = state
            .updates
            .as_ref()
            .ok_or_else(|| io::Error::other("Missing updates"))?;
        assert!(updates.waiting_for_stop && updates.confirmed && updates.closing());
        assert!(updates.task.is_none() && !updates.should_close());
        let request = update_request(
            &updates.installation,
            Operation::Install,
            updates.version.as_deref(),
            updates.release_identity.as_deref(),
            !updates.closing(),
        );
        assert_eq!(request["restart"], false);
        state.fail("Synthetic unknown owned-model teardown");
        let approved = Arc::new(AtomicBool::new(false));
        state.update_exit = Some(Arc::clone(&approved));
        state.poll_updates(&egui::Context::default());
        assert!(!approved.load(Ordering::Acquire));
        assert!(!state.prepare_update_on_close());
        assert!(!state.updates.as_ref().is_some_and(Updates::should_close));
        Ok(())
    }

    #[test]
    fn pending_close_requires_deferral_verified_bytes_and_known_ownership() -> io::Result<()> {
        let (mut state, commands) = update_state()?;
        assert!(!state.prepare_update_on_close());
        state
            .updates
            .as_mut()
            .ok_or_else(|| io::Error::other("Missing updates"))?
            .defer();
        state.model = super::super::ModelLifecycle::Unavailable;
        assert!(!state.prepare_update_on_close());
        state.model = super::super::ModelLifecycle::Ready;
        let updates = state
            .updates
            .as_mut()
            .ok_or_else(|| io::Error::other("Missing updates"))?;
        updates.cleanup_unconfirmed = true;
        assert!(!state.prepare_update_on_close());
        let updates = state
            .updates
            .as_mut()
            .ok_or_else(|| io::Error::other("Missing updates"))?;
        updates.cleanup_unconfirmed = false;
        updates.downloaded = false;
        assert!(!state.prepare_update_on_close());
        assert!(commands.try_recv().is_err());
        Ok(())
    }

    #[test]
    fn cancelled_check_with_unknown_cleanup_never_authorizes_close_update() -> io::Result<()> {
        for result in [
            None,
            Some(Err(io::Error::other(
                "Application update process cleanup could not be confirmed. Synthetic cancellation failure.",
            ))),
        ] {
            let mut updates = update_fixture()?;
            updates.defer();
            updates.task = Some(completed_task(Operation::Check, result));
            assert!(!updates.approve_on_close());
            assert!(updates.cleanup_unconfirmed);
            assert!(!updates.confirmed && !updates.waiting_for_stop && !updates.closing());
            assert!(updates.task.is_none());
        }
        Ok(())
    }

    #[test]
    fn failed_deferred_install_never_starts_another_check_while_closing() -> io::Result<()> {
        let mut updates = update_fixture()?;
        updates.close_requested = true;
        updates.confirmed = true;
        updates.task = Some(completed_task(
            Operation::Install,
            Some(Err(io::Error::other(
                "Publication changed before installation",
            ))),
        ));
        assert!(updates.poll().is_some());
        assert!(updates.closing() && !updates.installing() && !updates.should_close());
        assert!(updates.next_check <= Instant::now());
        assert!(updates.poll().is_none());
        assert!(
            updates.task.is_none(),
            "Failed closing must not start another metadata worker"
        );
        Ok(())
    }

    #[test]
    fn error_reporting_update_preserves_full_failure_and_clears_it_after_success() -> io::Result<()>
    {
        let mut updates = update_fixture()?;
        let message = format!(
            "{}Final cause: denied. 日本語",
            "nested cause\n".repeat(1_000)
        );
        let diagnostic =
            updates.operation_failed(Operation::Check, &io::Error::other(message.clone()));
        assert_eq!(updates.error.as_deref(), Some(message.as_str()));
        assert!(diagnostic.ends_with(&message));
        updates.operation_finished(
            Operation::Check,
            Response {
                status: "current".into(),
                version: None,
                release_identity: None,
            },
        );
        assert!(updates.error.is_none());
        Ok(())
    }

    #[test]
    fn update_confirmation_needs_both_clicks_and_current_install_admission() -> io::Result<()> {
        use egui_kittest::{
            Harness,
            kittest::{NodeT as _, Queryable as _},
        };
        let mut harness = Harness::builder().with_size([640.0, 640.0]).build_ui_state(
            |ui, state: &mut (Updates, bool, usize)| {
                let can_install = state.1;
                if state.0.controls(ui, can_install) {
                    state.2 += 1;
                }
            },
            (update_fixture()?, true, 0),
        );
        harness
            .get_by_role_and_label(egui::Role::Button, "Install update and close")
            .click();
        harness.run();
        assert_eq!(harness.state().2, 0);
        assert!(harness.state().0.confirming);
        assert!(!harness.state().0.confirmed);
        assert!(!harness.state().0.should_close());
        harness.state_mut().1 = false;
        harness.run();
        assert!(
            harness
                .get_by_role_and_label(egui::Role::Button, "Close and install")
                .accesskit_node()
                .is_disabled()
        );
        harness
            .get_by_role_and_label(egui::Role::Button, "Later")
            .click();
        harness.run();
        assert!(!harness.state().0.confirming);
        assert!(!harness.state().0.confirmed);
        assert_eq!(harness.state().2, 0);
        harness.state_mut().1 = true;
        harness.run();
        harness
            .get_by_role_and_label(egui::Role::Button, "Install update and close")
            .click();
        harness.run();
        harness
            .get_by_role_and_label(egui::Role::Button, "Close and install")
            .click();
        harness.run();
        assert_eq!(harness.state().2, 1);
        assert!(!harness.state().0.confirming);
        assert!(harness.state().0.confirmed);
        assert!(harness.state().0.task.is_none());
        assert!(!harness.state().0.should_close());
        Ok(())
    }

    #[test]
    fn unsafe_teardown_or_unverified_download_never_queues_an_update() -> io::Result<()> {
        let mut updates = update_fixture()?;
        updates.wait_for_stop();
        assert!(
            !updates.waiting_for_stop,
            "A direct call cannot replace user confirmation"
        );
        updates.confirmed = true;
        updates.wait_for_stop();
        assert!(updates.waiting_for_stop);
        updates.finish_stop(false);
        assert!(!updates.waiting_for_stop);
        assert!(updates.task.is_none());
        assert!(!updates.should_close());
        assert!(updates.status.contains("could not stop safely"));
        assert!(!updates.confirmed);
        updates.downloaded = false;
        updates.confirmed = true;
        updates.wait_for_stop();
        assert!(!updates.waiting_for_stop);
        assert!(updates.task.is_none());
        updates.downloaded = true;
        updates.version = Some(env!("CARGO_PKG_VERSION").into());
        updates.wait_for_stop();
        assert!(!updates.waiting_for_stop);
        assert!(updates.task.is_none());
        Ok(())
    }

    #[test]
    fn unknown_update_cleanup_blocks_further_tasks_and_never_closes() -> io::Result<()> {
        for result in [
            None,
            Some(Err(io::Error::other(
                "Application update process cleanup could not be confirmed. Synthetic native failure.",
            ))),
        ] {
            let mut updates = update_fixture()?;
            updates.task = Some(completed_task(Operation::Install, result));
            let diagnostic = updates
                .poll()
                .ok_or_else(|| io::Error::other("Missing cleanup diagnostic"))?;
            assert!(diagnostic.contains("cleanup could not be confirmed"));
            assert!(updates.cleanup_unconfirmed);
            assert!(updates.task.is_none());
            assert!(!updates.should_close());
            for operation in [Operation::Check, Operation::Download, Operation::Install] {
                updates.start(operation);
                assert!(
                    updates.task.is_none(),
                    "Unconfirmed cleanup must never overlap a new update task"
                );
            }
            updates.wait_for_stop();
            assert!(!updates.waiting_for_stop);
            updates.next_check = Instant::now();
            assert!(updates.poll().is_none());
            assert!(updates.task.is_none());
        }
        Ok(())
    }

    #[test]
    fn failed_queue_is_retryable_but_only_verified_queue_success_requests_close() -> io::Result<()>
    {
        let mut updates = update_fixture()?;
        updates.confirmed = true;
        updates.task = Some(completed_task(
            Operation::Install,
            Some(Err(io::Error::other("Publication changed; confirm again"))),
        ));
        assert!(updates.poll().is_some());
        assert!(!updates.cleanup_unconfirmed);
        assert!(!updates.confirming);
        assert!(!updates.confirmed);
        assert!(!updates.should_close());
        assert!(updates.task.is_none());
        assert!(!updates.downloaded);
        assert!(updates.release_identity.is_none());
        let selected = updates
            .version
            .clone()
            .ok_or_else(|| io::Error::other("Missing selected version"))?;
        let queued = response(
            &serde_json::json!({"Status":"queued","Version":selected}).to_string(),
            Operation::Install,
            Some(&selected),
        )?;
        updates.task = Some(completed_task(Operation::Install, Some(Ok(queued))));
        assert!(
            updates.poll().is_some(),
            "A stale queued result cannot replace a fresh approval"
        );
        assert!(!updates.should_close());
        updates.downloaded = true;
        updates.release_identity = Some("b".repeat(64));
        updates.confirmed = true;
        let queued = response(
            &serde_json::json!({"Status":"queued","Version":selected}).to_string(),
            Operation::Install,
            Some(&selected),
        )?;
        updates.task = Some(completed_task(Operation::Install, Some(Ok(queued))));
        assert!(updates.poll().is_none());
        assert!(updates.should_close());
        assert!(updates.installing());
        assert!(updates.task.is_none());
        Ok(())
    }

    #[test]
    fn confirmed_update_blocks_setup_and_speech_retries_through_queue_completion() -> io::Result<()>
    {
        use std::ffi::OsString;
        let directory = std::env::temp_dir().join(format!(
            "Bonsai-UpdateSetupOwnership-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(io::Error::other)?
                .as_nanos(),
        ));
        fs::create_dir(&directory)?;
        let tool = directory.join("fixture-tool.exe");
        let payload = directory.join("fixture-startup.json");
        fs::write(&tool, [])?;
        fs::write(&payload, r#"{"PreparationStage":"all"}"#)?;
        let request = crate::startup::Request::parse(&[
            OsString::from("--startup-tool"),
            tool.into_os_string(),
            OsString::from("--startup-request"),
            payload.into_os_string(),
            OsString::from("--startup-data-directory"),
            directory.clone().into_os_string(),
        ])?;
        for phase in 0..3 {
            let (commands, incoming) = mpsc::channel();
            let (_events, notifications) = crate::client::event_channel();
            let mut state =
                super::super::State::pending_startup(commands, notifications, request.clone());
            state.startup_request.take();
            state.fail_startup(&io::Error::other("Synthetic resumable setup failure"));
            state.setup.broker = Some(directory.join("broker.exe"));
            state.setup.speech_error = Some("Synthetic resumable speech failure".into());
            state.input = "Keep this draft through update confirmation".into();
            let mut updates = update_fixture()?;
            updates.confirmed = true;
            match phase {
                0 => updates.wait_for_stop(),
                1 => updates.task = Some(completed_task(Operation::Install, None)),
                _ => updates.queued = true,
            }
            assert!(updates.installing());
            state.updates = Some(updates);
            state.retry_setup();
            state.retry_speech_setup();
            assert!(
                state.startup_request.is_none(),
                "Text setup restarted during update phase {phase}"
            );
            assert!(
                state.startup.is_none(),
                "Speech setup restarted during update phase {phase}"
            );
            assert!(state.setup.error.is_some() && state.setup.speech_error.is_some());
            assert_eq!(state.model, super::super::ModelLifecycle::Unavailable);
            assert_eq!(state.input, "Keep this draft through update confirmation");
            assert!(
                incoming.try_recv().is_err(),
                "An update must never replay a draft or restart the model"
            );
            drop(state);
        }
        fs::remove_dir_all(directory)
    }

    #[test]
    fn portable_update_requires_exact_release_fingerprint_and_changed_publication_cancels_approval()
    -> io::Result<()> {
        let mut updates = update_fixture()?;
        let version = updates
            .version
            .clone()
            .ok_or_else(|| io::Error::other("Missing version"))?;
        for identity in [
            None,
            Some(String::new()),
            Some("a".repeat(63)),
            Some("A".repeat(64)),
            Some("z".repeat(64)),
        ] {
            for (status, operation) in [
                ("available", Operation::Check),
                ("downloaded", Operation::Download),
            ] {
                let payload = serde_json::json!({"Status":status,"Version":version,"ReleaseIdentity":identity}).to_string();
                assert!(verified_response(&payload, operation, Some(&version), true).is_err());
                assert!(
                    verified_response(&payload, operation, Some(&version), false).is_ok(),
                    "Legacy MSI responses retain their existing protocol"
                );
            }
        }
        updates.confirming = true;
        let same = verified_response(&serde_json::json!({"Status":"available","Version":version,"ReleaseIdentity":"a".repeat(64)}).to_string(), Operation::Check, None, true)?;
        assert!(!updates.select_candidate(same));
        assert!(updates.downloaded && updates.confirming);
        assert!(updates.verified_download());
        let changed = verified_response(&serde_json::json!({"Status":"available","Version":version,"ReleaseIdentity":"b".repeat(64)}).to_string(), Operation::Check, None, true)?;
        assert!(updates.select_candidate(changed));
        assert!(!updates.downloaded);
        assert!(!updates.confirming);
        assert!(updates.release_identity.is_none());
        assert!(!updates.verified_download());
        assert!(!updates.should_close());
        updates.wait_for_stop();
        assert!(!updates.waiting_for_stop);
        Ok(())
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
            let check = update_request(
                &installation,
                Operation::Check,
                Some("0.9.0"),
                Some(&"a".repeat(64)),
                true,
            );
            assert!(check.get("version").is_none());
            assert_eq!(
                check["operation"],
                if portable { "portable-check" } else { "check" }
            );
            let download = update_request(
                &installation,
                Operation::Download,
                Some("0.9.0"),
                Some(&"a".repeat(64)),
                true,
            );
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
            assert!(check.get("releaseIdentity").is_none());
            assert!(download.get("releaseIdentity").is_none());
            let install = update_request(
                &installation,
                Operation::Install,
                Some("0.9.0"),
                Some(&"a".repeat(64)),
                true,
            );
            assert_eq!(install.get("releaseIdentity").is_some(), portable);
            if portable {
                assert_eq!(install["releaseIdentity"], "a".repeat(64));
                assert_eq!(install["restart"], true);
            }
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
