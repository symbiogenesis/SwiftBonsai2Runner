//! First-run preparation and recovery without shell commands.
use super::{ModelLifecycle, RichText, State, egui};
use std::{io, path::PathBuf};

#[derive(Default)]
pub(super) struct Setup {
    pub(super) request: Option<crate::startup::Request>,
    pub(super) broker: Option<PathBuf>,
    pub(super) progress: Option<crate::startup::SetupStatus>,
    pub(super) error: Option<String>,
    pub(super) speech_error: Option<String>,
    pub(super) pausing: bool,
}

fn cleanup_known(error: &str) -> bool {
    !error.contains("Setup process cleanup could not be confirmed.")
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

    fn retry_speech_setup(&mut self) {
        if self.startup.is_some() || self.startup_request.is_some() {
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

    pub(super) fn setup_controls(&mut self, ui: &mut egui::Ui) {
        let preparing =
            self.worker.is_none() && (self.startup_request.is_some() || self.startup.is_some());
        if preparing || self.setup.error.is_some() {
            egui::Frame::group(ui.style()).inner_margin(14).show(ui, |ui| {
                ui.label(RichText::new(if preparing { "Setting up Swift" } else { "Setup needs attention" }).strong());
                if preparing {
                    ui.label("First-time setup downloads the local model (6.71 GiB), its adapter and runtime. Verified files are reused.");
                    if let Some(progress) = &self.setup.progress {
                        ui.label(&progress.message);
                        if let (Some(downloaded), Some(expected)) = (progress.downloaded, progress.expected) {
                            #[expect(clippy::cast_precision_loss, reason = "Approximate UI progress; artifact acceptance uses exact integer lengths and SHA-256.")]
                            let fraction = downloaded as f32 / expected as f32;
                            ui.add(egui::ProgressBar::new(fraction).show_percentage());
                        } else {
                            ui.spinner();
                        }
                    } else {
                        ui.horizontal(|ui| { ui.spinner(); ui.label("Checking setup…"); });
                    }
                    ui.label("You can draft while setup runs. Closing Swift stops setup and retains partial downloads.");
                    if let Some(startup) = &self.startup
                        && ui.add_enabled(!self.setup.pausing, egui::Button::new("Pause setup")).clicked()
                    {
                        startup.cancel();
                        self.setup.pausing = true;
                    }
                    if self.setup.pausing { ui.label("Pausing setup safely…"); }
                } else {
                    ui.label(&self.status);
                    if ui.add_enabled(self.can_retry_setup(), egui::Button::new("Retry setup")).clicked() {
                        self.retry_setup();
                    }
                    ui.collapsing("Setup details", |ui| {
                        if let Some(error) = &self.setup.error {
                            ui.add(egui::Label::new(error.chars().take(super::MAX_DIAGNOSTIC_CHARS).collect::<String>()).selectable(true).wrap());
                        }
                        if ui.button("Copy diagnostics").clicked() {
                            ui.ctx().copy_text(self.diagnostics.iter().cloned().collect::<Vec<_>>().join("\n\n"));
                        }
                    });
                }
            });
            ui.add_space(10.0);
        }
        if self.setup.speech_error.is_some() {
            egui::Frame::group(ui.style())
                .inner_margin(12)
                .show(ui, |ui| {
                    ui.label("Speech setup could not finish. You can continue using text chat.");
                    let retry = self.startup.is_none()
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
        assert!(failure_message(&io::Error::other("Pinned download failed (6): Could not resolve host; check free disk space and folder permissions")).contains("internet"));
    }
}
