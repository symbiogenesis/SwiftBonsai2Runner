//! Compact failure presentation; preview limits never affect the copied error.
use super::{RichText, State, egui};

const PREVIEW_CHARS: usize = 320;
const DETAIL_CHARS: usize = 4_096;

/// Keep arbitrary newlines and long paths from growing a notice or its parent.
pub(super) fn summary(ui: &mut egui::Ui, message: &str) {
    let message = message
        .split("Protected credentials and technical evidence:")
        .next()
        .unwrap_or(message);
    let mut characters = message.chars();
    let mut text: String = characters
        .by_ref()
        .take(PREVIEW_CHARS)
        .map(|character| {
            if character.is_whitespace() {
                ' '
            } else {
                character
            }
        })
        .collect();
    if characters.next().is_some() {
        text.push('…');
    }
    let mut job = egui::text::LayoutJob::simple(
        text,
        egui::TextStyle::Body.resolve(ui.style()),
        ui.visuals().text_color(),
        ui.available_width().max(1.0),
    );
    job.wrap.max_rows = 2;
    job.wrap.break_anywhere = true;
    ui.add(egui::Label::new(job));
}

pub(super) fn copy_button(ui: &mut egui::Ui, label: &str, message: &str) {
    let id = ui.make_persistent_id(("error-copy", label, message));
    super::controls::CopyButton::new(label, id)
        .show(ui, || message.to_owned())
        .on_hover_text("Copy the complete message, including details beyond the preview.");
}

pub(super) fn details(ui: &mut egui::Ui, message: &str) {
    let mut characters = message.chars();
    let preview: String = characters.by_ref().take(DETAIL_CHARS).collect();
    let shortened = characters.next().is_some();
    egui::ScrollArea::vertical()
        .id_salt("error-details-scroll")
        .max_height((ui.ctx().content_rect().height() * 0.4).clamp(80.0, 320.0))
        .min_scrolled_height(0.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            super::navigation::bounded_width(ui, |ui| {
                ui.add(
                    egui::Label::new(RichText::new(preview).small().monospace())
                        .selectable(true)
                        .wrap(),
                );
            });
        });
    if shortened {
        ui.add(
            egui::Label::new(
                RichText::new("Preview shortened. Copy includes the full message.")
                    .small()
                    .weak(),
            )
            .truncate(),
        );
    }
}

/// Stable source identities keep selection and copy feedback independent.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) enum AlertKind {
    Setup,
    Model,
    Recovery,
    SpeechSetup,
    AudioFile,
    Speech,
    Update,
}

/// Alert presentation is transient and never enters conversation recovery.
#[derive(Default)]
pub(super) struct View {
    pub(super) selection: Option<AlertKind>,
    pub(super) details: Option<AlertKind>,
}

impl AlertKind {
    const fn title(self) -> &'static str {
        match self {
            Self::Setup => "Setup needs attention",
            Self::Model => "Model needs attention",
            Self::Recovery => "Conversation saving needs attention",
            Self::SpeechSetup => "Speech setup needs attention",
            Self::AudioFile => "Audio transcription failed",
            Self::Speech => "Speech needs attention",
            Self::Update => "Update needs attention",
        }
    }
}

impl State {
    pub(super) fn active_alerts(&self) -> Vec<AlertKind> {
        let preparing =
            self.worker.is_none() && (self.startup_request.is_some() || self.startup.is_some());
        let sources = [
            (AlertKind::Setup, self.setup.error.is_some() && !preparing),
            (
                AlertKind::Model,
                (self.warning || self.model.retryable()) && self.setup.error.is_none(),
            ),
            (
                AlertKind::Recovery,
                self.recovery_notice.starts_with("Could not"),
            ),
            (AlertKind::SpeechSetup, self.setup.speech_error.is_some()),
            (
                AlertKind::AudioFile,
                self.audio_file_error && self.audio_file_notice.is_some(),
            ),
            (
                AlertKind::Speech,
                self.speech_notice
                    && self.help.is_none()
                    && self.setup.speech_error.is_none()
                    && self.audio_file_notice.as_deref() != Some(self.speech_status.as_str()),
            ),
            (
                AlertKind::Update,
                self.updates
                    .as_ref()
                    .is_some_and(|updates| updates.error_notice().is_some()),
            ),
        ];
        sources
            .into_iter()
            .filter_map(|(kind, active)| active.then_some(kind))
            .collect()
    }

    fn alert_text(&self, kind: AlertKind) -> (&str, &str) {
        match kind {
            AlertKind::Setup => (
                &self.status,
                self.setup.error.as_deref().unwrap_or_default(),
            ),
            AlertKind::Model => (
                &self.status,
                if self.status_detail.is_empty() {
                    &self.status
                } else {
                    &self.status_detail
                },
            ),
            AlertKind::Recovery => (&self.recovery_notice, &self.recovery_notice),
            AlertKind::SpeechSetup => (
                "Speech setup could not finish. You can continue using text chat.",
                self.setup.speech_error.as_deref().unwrap_or_default(),
            ),
            AlertKind::AudioFile => {
                let message = self.audio_file_notice.as_deref().unwrap_or_default();
                (message, message)
            }
            AlertKind::Speech => (&self.speech_status, &self.speech_status),
            AlertKind::Update => self
                .updates
                .as_ref()
                .and_then(super::updates::Updates::error_notice)
                .unwrap_or_default(),
        }
    }

    pub(super) fn alert_panel(&mut self, ui: &mut egui::Ui) {
        let alerts = self.active_alerts();
        let Some(first) = alerts.first().copied() else {
            self.alerts.selection = None;
            return;
        };
        let mut selected = self
            .alerts
            .selection
            .filter(|kind| alerts.contains(kind))
            .unwrap_or(first);
        super::navigation::bounded_width(ui, |ui| {
            egui::Frame::new()
                .fill(ui.visuals().faint_bg_color)
                .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
                .corner_radius(10)
                .inner_margin(10)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing = egui::vec2(8.0, 4.0);
                    ui.spacing_mut().button_padding = egui::vec2(8.0, 4.0);
                    ui.spacing_mut().interact_size.y = 28.0;
                    ui.horizontal(|ui| {
                        let count = if alerts.len() == 1 {
                            "1 alert".to_owned()
                        } else {
                            format!("{} alerts", alerts.len())
                        };
                        ui.label(
                            RichText::new(count)
                                .small()
                                .color(ui.visuals().warn_fg_color),
                        );
                        if alerts.len() == 1 {
                            ui.add(
                                egui::Label::new(RichText::new(selected.title()).strong())
                                    .truncate(),
                            )
                            .on_hover_text(selected.title());
                        } else {
                            egui::ComboBox::from_id_salt("alert-selector")
                                .selected_text(RichText::new(selected.title()).strong())
                                .width(ui.available_width())
                                .truncate()
                                .show_ui(ui, |ui| {
                                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                                    for kind in &alerts {
                                        if ui
                                            .add_sized(
                                                [ui.available_width(), 0.0],
                                                egui::Button::selectable(
                                                    selected == *kind,
                                                    kind.title(),
                                                )
                                                .wrap(),
                                            )
                                            .clicked()
                                        {
                                            selected = *kind;
                                        }
                                    }
                                })
                                .response
                                .accessible_name("Current alert")
                                .on_hover_text(format!(
                                    "{}\nChoose an alert to see its message and actions.",
                                    selected.title()
                                ));
                        }
                    });
                    self.alerts.selection = Some(selected);
                    let (message, error) = self.alert_text(selected);
                    summary(ui, message);
                    let error = error.to_owned();
                    ui.push_id(selected, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            self.alert_actions(ui, selected, &error);
                            if ui.button("View details").clicked() {
                                self.alerts.details = Some(selected);
                                self.focus_composer = false;
                            }
                            copy_button(ui, "Copy error", &error);
                        });
                    });
                });
        });
        ui.add_space(8.0);
    }

    fn alert_actions(&mut self, ui: &mut egui::Ui, kind: AlertKind, error: &str) {
        match kind {
            AlertKind::Setup => self.setup_error_actions(ui),
            AlertKind::Model => {
                if self.model.retryable() && ui.button("Retry model").clicked() {
                    self.retry();
                }
            }
            AlertKind::SpeechSetup => self.speech_setup_error_actions(ui, error),
            AlertKind::AudioFile => self.audio_file_error_actions(ui),
            AlertKind::Update => {
                if ui.button("Application settings").clicked() {
                    self.open_settings(super::settings::Page::Application);
                }
            }
            AlertKind::Recovery | AlertKind::Speech => {}
        }
    }

    pub(super) fn alert_details_dialog(&mut self, ctx: &egui::Context) {
        let Some(kind) = self.alerts.details else {
            return;
        };
        if !self.active_alerts().contains(&kind) {
            self.alerts.details = None;
            self.focus_composer = true;
            return;
        }
        let (_, error) = self.alert_text(kind);
        let modal = egui::Modal::new(egui::Id::unique("alert-details"))
            .frame(super::controls::dialog_frame(ctx))
            .show(ctx, |ui| {
                ui.set_width((ctx.content_rect().width() - 80.0).clamp(240.0, 680.0));
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                ui.label(RichText::new("Error details").strong().size(22.0));
                ui.label(kind.title());
                ui.push_id(kind, |ui| details(ui, error));
                ui.separator();
                let mut close = false;
                ui.horizontal_wrapped(|ui| {
                    copy_button(ui, "Copy full error", error);
                    close = ui.button("Close").clicked();
                });
                close
            });
        if modal.inner || modal.should_close() {
            self.alerts.details = None;
            self.focus_composer = true;
        }
    }
}
