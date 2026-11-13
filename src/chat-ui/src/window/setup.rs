//! First-run preparation and recovery without shell commands.
use super::{ModelLifecycle, RichText, State, egui};
use crate::startup::{SetupActivity, SetupKind, SetupStatus};
use std::{io, path::PathBuf};

#[derive(Default)]
pub(super) struct Setup {
    pub(super) request: Option<crate::startup::Request>,
    pub(super) broker: Option<PathBuf>,
    pub(super) progress: Option<SetupStatus>,
    pub(super) error: Option<String>,
    pub(super) speech_error: Option<String>,
    pub(super) pausing: bool,
}

fn cleanup_known(error: &str) -> bool {
    !error.contains("Setup process cleanup could not be confirmed.")
}

fn preparation_title(progress: Option<&SetupStatus>, pausing: bool) -> &'static str {
    if pausing {
        "Pausing setup"
    } else {
        match progress.map(|progress| progress.activity) {
            Some(SetupActivity::Downloading) => "Downloading Swift",
            Some(SetupActivity::Ready) => "Starting Swift",
            _ if progress.is_some_and(|progress| progress.kind == Some(SetupKind::FirstRun)) => {
                "Setting up Swift"
            }
            _ => "Preparing Swift",
        }
    }
}

fn download_progress(progress: &SetupStatus) -> Option<(f32, String)> {
    if progress.activity != SetupActivity::Downloading {
        return None;
    }
    let (downloaded, expected) = (progress.downloaded?, progress.expected?);
    if expected == 0 {
        return None;
    }
    let downloaded = downloaded.min(expected);
    #[expect(
        clippy::cast_precision_loss,
        clippy::as_conversions,
        reason = "Approximate display only; provisioning verifies exact integer lengths and SHA-256."
    )]
    let fraction = downloaded as f32 / expected as f32;
    Some((
        fraction,
        format!("{} / {}", byte_count(downloaded), byte_count(expected)),
    ))
}

fn byte_count(bytes: u64) -> String {
    #[expect(
        clippy::cast_precision_loss,
        clippy::as_conversions,
        reason = "Rounded display units only; artifact verification uses exact byte counts."
    )]
    let bytes = bytes as f64;
    if bytes >= 1024.0 * 1024.0 * 1024.0 {
        format!("{:.2} GiB", bytes / (1024.0 * 1024.0 * 1024.0))
    } else {
        format!("{:.1} MiB", bytes / (1024.0 * 1024.0))
    }
}

pub(super) fn failure_message(error: &io::Error) -> &'static str {
    let text = error.to_string().to_ascii_lowercase();
    if !cleanup_known(&error.to_string()) {
        "Setup could not confirm that its processes stopped. Close Swift before trying again."
    } else if error.kind() == io::ErrorKind::Interrupted {
        "Setup paused. Select Retry setup to continue. Your draft is safe."
    } else if text.contains("not enough free disk space")
        || text.contains("not enough disk space")
        || text.contains("disk full")
        || text.contains("not enough space on the disk")
        || text.contains("os error 112")
        || text.contains("free storage") && text.contains("is available")
    {
        "Setup needs more free storage. Free some space, then select Retry setup. Your draft is safe."
    } else if text.contains("access is denied")
        || text.contains("permissiondenied")
        || text.contains("permission denied")
        || text.contains("os error 5")
        || text.contains("unsafe")
        || text.contains("artifact storage could not be opened safely")
    {
        "Setup cannot use its data folder. Check its permissions and use a local folder, then select Retry setup."
    } else if text.contains("download") || text.contains("connect") || text.contains("resolve host")
    {
        "A setup download could not finish. Check your internet connection and free storage, then select Retry setup."
    } else {
        "Setup could not finish. Select Retry setup to try again. Your draft is safe; details are available below."
    }
}

impl State {
    pub(super) fn setup_stopped_for_update(&self) -> bool {
        self.setup.request.is_some()
            && self.setup.error.as_deref().is_some_and(cleanup_known)
            && self.worker.is_none()
            && self.startup.is_none()
            && self.startup_request.is_none()
            && self.model == ModelLifecycle::Unavailable
    }

    fn can_retry_setup(&self) -> bool {
        self.setup_stopped_for_update()
            && !self
                .updates
                .as_ref()
                .is_some_and(super::updates::Updates::installing)
    }

    pub(super) fn retry_setup(&mut self) {
        if !self.can_retry_setup() {
            return;
        }
        self.startup_request = self.setup.request.clone();
        self.setup.error = None;
        self.setup.progress = None;
        self.setup.pausing = false;
        if let Some(failure) = &self.startup_failure
            && let Ok(mut failure) = failure.lock()
        {
            *failure = None;
        }
        self.model = ModelLifecycle::Preparing;
        self.speech_checked = !self.canary_configured && !self.tts_configured;
        self.set_status(
            "Continuing setup… Verified downloads are reused. You can keep drafting.",
            false,
        );
    }

    pub(super) fn retry_speech_setup(&mut self) {
        if self.startup.is_some()
            || self.startup_request.is_some()
            || self
                .updates
                .as_ref()
                .is_some_and(super::updates::Updates::installing)
        {
            return;
        }
        let Some(error) = self.setup.speech_error.as_deref() else {
            return;
        };
        if !cleanup_known(error) {
            return;
        }
        let (Some(request), Some(broker)) = (self.setup.request.clone(), self.setup.broker.clone())
        else {
            return;
        };
        match crate::startup::Session::start_speech(request, broker) {
            Ok(session) => {
                self.startup = Some(session);
                self.setup.speech_error = None;
                self.speech_checked = false;
                "Preparing speech services… Text chat stays available."
                    .clone_into(&mut self.speech_status);
            }
            Err(error) => self.fail_speech_preparation(&error),
        }
    }

    fn preparation_card(&mut self, ui: &mut egui::Ui) -> egui::Response {
        egui::Frame::new()
            .fill(ui.visuals().faint_bg_color)
            .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
            .corner_radius(12)
            .inner_margin(12)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing = egui::vec2(8.0, 6.0);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if let Some(startup) = &self.startup
                            && ui.add_enabled(!self.setup.pausing, egui::Button::new("Pause setup"))
                                .on_hover_text("Stop setup safely and keep partial downloads for later.")
                                .clicked()
                        {
                            startup.cancel();
                            self.setup.pausing = true;
                        }
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.add(egui::Spinner::new().size(16.0));
                            ui.add(egui::Label::new(RichText::new(preparation_title(
                                self.setup.progress.as_ref(), self.setup.pausing,
                            )).strong()).wrap());
                        });
                    });
                });
                if self.setup.pausing {
                    ui.label("Stopping setup safely…");
                } else if let Some(progress) = &self.setup.progress {
                    ui.add(egui::Label::new(&progress.message).wrap());
                    if let Some((fraction, bytes)) = download_progress(progress) {
                        let label = ui.label(RichText::new(format!("{:.0}% · {bytes}", fraction * 100.0)).small());
                        ui.add(egui::ProgressBar::new(fraction).desired_height(6.0).animate(false))
                            .labelled_by(label.id)
                            .on_hover_text("Downloaded bytes for the current file. Verification follows the download.");
                    }
                } else {
                    ui.label("Checking runtime and local files…");
                }
                let details = match self.setup.progress.as_ref().and_then(|progress| progress.kind) {
                    Some(SetupKind::FirstRun) => "First-time model setup",
                    Some(SetupKind::Existing) => "Saved model found",
                    None => "Setup details",
                };
                ui.collapsing(details, |ui| {
                    ui.label("You can keep drafting while Swift prepares.");
                    match self.setup.progress.as_ref().and_then(|progress| progress.kind) {
                        Some(SetupKind::FirstRun) => {
                            ui.label("This setup started without a saved model. Completed downloads are reused, and partial downloads resume where possible.");
                        }
                        Some(SetupKind::Existing) => {
                            ui.label("The selected model is already saved. Setup checks local files and downloads replacements only when needed.");
                        }
                        None => {}
                    }
                    ui.label("Pausing or closing Swift stops setup and retains partial downloads.");
                });
            })
            .response
    }

    pub(super) fn setup_controls(&mut self, ui: &mut egui::Ui) {
        let preparing =
            self.worker.is_none() && (self.startup_request.is_some() || self.startup.is_some());
        if preparing {
            let _ = self.preparation_card(ui);
            ui.add_space(8.0);
        } else if self.setup.error.is_some() {
            egui::Frame::group(ui.style())
                .inner_margin(12)
                .corner_radius(12)
                .show(ui, |ui| {
                    ui.label(RichText::new("Setup needs attention").strong());
                    ui.label(&self.status);
                    if ui
                        .add_enabled(self.can_retry_setup(), egui::Button::new("Retry setup"))
                        .clicked()
                    {
                        self.retry_setup();
                    }
                    ui.collapsing("Setup details", |ui| {
                        if let Some(error) = &self.setup.error {
                            ui.add(
                                egui::Label::new(
                                    error
                                        .chars()
                                        .take(super::MAX_DIAGNOSTIC_CHARS)
                                        .collect::<String>(),
                                )
                                .selectable(true)
                                .wrap(),
                            );
                        }
                        if ui.button("Copy diagnostics").clicked() {
                            ui.ctx().copy_text(
                                self.diagnostics
                                    .iter()
                                    .cloned()
                                    .collect::<Vec<_>>()
                                    .join("\n\n"),
                            );
                        }
                    });
                });
            ui.add_space(8.0);
        }
        if self.setup.speech_error.is_some() {
            egui::Frame::group(ui.style())
                .inner_margin(12)
                .show(ui, |ui| {
                    ui.label("Speech setup could not finish. You can continue using text chat.");
                    let retry = self.startup.is_none()
                        && self.startup_request.is_none()
                        && !self
                            .updates
                            .as_ref()
                            .is_some_and(super::updates::Updates::installing)
                        && self
                            .setup
                            .speech_error
                            .as_deref()
                            .is_some_and(cleanup_known);
                    if ui
                        .add_enabled(retry, egui::Button::new("Retry speech setup"))
                        .clicked()
                    {
                        self.retry_speech_setup();
                    }
                    ui.collapsing("Speech setup details", |ui| {
                        if let Some(error) = &self.setup.speech_error {
                            ui.add(
                                egui::Label::new(
                                    error
                                        .chars()
                                        .take(super::MAX_DIAGNOSTIC_CHARS)
                                        .collect::<String>(),
                                )
                                .selectable(true)
                                .wrap(),
                            );
                        }
                        ui.hyperlink_to(
                            "Microsoft ARM64 runtime download",
                            "https://aka.ms/vc14/vc_redist.arm64.exe",
                        );
                    });
                });
            ui.add_space(10.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "Keep native setup phase transitions, layout bounds and pause behavior in one model-free fixture."
    )]
    fn setup_card_separates_downloads_from_local_preparation_and_fits_narrow_layouts() {
        use egui_kittest::{Harness, kittest::Queryable as _};
        struct Fixture {
            app: State,
            card: egui::Rect,
        }
        for width in [320.0, 480.0, 800.0] {
            let (commands, incoming) = std::sync::mpsc::channel();
            let (_events, events) = crate::client::event_channel();
            let mut app = State::create(commands, events, None, None);
            app.input = "Keep my draft".into();
            let (session, _outgoing) = crate::startup::Session::fixture();
            app.startup = Some(session);
            let mut harness = Harness::builder()
                .with_size([width, 480.0])
                .with_context_setup(super::super::configure)
                .build_ui_state(
                    |ui, fixture: &mut Fixture| {
                        fixture.card = fixture.app.preparation_card(ui).rect;
                    },
                    Fixture {
                        app,
                        card: egui::Rect::NOTHING,
                    },
                );
            assert!(harness.query_by_label("Preparing Swift").is_some());
            assert!(harness.query_by_label("First-time model setup").is_none());
            for (kind, activity, title, message, details, downloading) in [
                (
                    SetupKind::FirstRun,
                    SetupActivity::Downloading,
                    "Downloading Swift",
                    "Downloading the Swift model…",
                    "First-time model setup",
                    true,
                ),
                (
                    SetupKind::Existing,
                    SetupActivity::Verifying,
                    "Preparing Swift",
                    "Verifying the saved Swift model…",
                    "Saved model found",
                    false,
                ),
                (
                    SetupKind::Existing,
                    SetupActivity::Preparing,
                    "Preparing Swift",
                    "Unpacking the saved runtime…",
                    "Saved model found",
                    false,
                ),
                (
                    SetupKind::Existing,
                    SetupActivity::Ready,
                    "Starting Swift",
                    "Starting your local model…",
                    "Saved model found",
                    false,
                ),
            ] {
                let app = &mut harness.state_mut().app;
                app.setup.progress = Some(SetupStatus {
                    kind: Some(kind),
                    activity,
                    message: message.into(),
                    // Stale observations must never become a verification bar.
                    downloaded: Some(1024 * 1024 * 1024),
                    expected: Some(2 * 1024 * 1024 * 1024),
                });
                harness.run_steps(2);
                assert!(harness.query_by_label(title).is_some());
                assert!(harness.query_by_label(message).is_some());
                assert!(
                    harness
                        .query_by_role_and_label(egui::Role::DisclosureTriangle, details)
                        .is_some()
                );
                assert_eq!(
                    harness
                        .query_by_role_and_label(
                            egui::Role::ProgressIndicator,
                            "50% · 1.00 GiB / 2.00 GiB"
                        )
                        .is_some(),
                    downloading
                );
                let card = harness.state().card;
                assert!(
                    card.max.x <= width && card.height() < 150.0,
                    "Card bounds: {card:?}"
                );
                assert!(
                    card.contains_rect(
                        harness
                            .get_by_role_and_label(egui::Role::Button, "Pause setup")
                            .rect()
                    )
                );
                assert_eq!(harness.state().app.input, "Keep my draft");
                assert!(incoming.try_recv().is_err());
            }
            harness
                .get_by_role_and_label(egui::Role::Button, "Pause setup")
                .click();
            harness.run_steps(2);
            assert!(harness.state().app.setup.pausing);
            assert!(harness.query_by_label("Pausing setup").is_some());
            assert!(harness.query_by_label("Stopping setup safely…").is_some());
            assert!(
                harness
                    .query_by_role_and_label(
                        egui::Role::ProgressIndicator,
                        "50% · 1.00 GiB / 2.00 GiB"
                    )
                    .is_none()
            );
        }
    }

    #[test]
    fn setup_failure_advice_keeps_cleanup_unknowns_closed_and_pauses_resumable() {
        assert!(!cleanup_known(
            "Setup process cleanup could not be confirmed. download failed"
        ));
        assert!(
            failure_message(&io::Error::other(
                "Setup process cleanup could not be confirmed. disk"
            ))
            .contains("Close Swift")
        );
        assert!(
            failure_message(&io::Error::new(io::ErrorKind::Interrupted, "paused"))
                .contains("Retry setup")
        );
        assert!(failure_message(&io::Error::other("Not enough disk space")).contains("storage"));
        assert!(failure_message(&io::Error::other("Access is denied")).contains("permissions"));
        assert!(failure_message(&io::Error::other("Pinned download failed")).contains("internet"));
        assert!(failure_message(&io::Error::other("Swift model setup failed: Artifact storage could not be opened safely. Check the internet connection, free storage and folder permissions.")).contains("permissions"));
        assert!(failure_message(&io::Error::other("Pinned download failed (6): Could not resolve host; check free disk space and folder permissions")).contains("internet"));
    }
}
