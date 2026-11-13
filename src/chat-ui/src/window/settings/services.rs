//! Voice and application Settings pages.
use super::super::VOICE_LIMIT;
use super::{RichText, State, egui, settings_card};

impl State {
    pub(super) fn voice_preferences(&mut self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new("One voice profile for read aloud across your conversations. Saved on this device with Windows user encryption, even when conversation saving is off.")
                .small()
                .weak(),
        );
        ui.add_space(12.0);
        settings_card(ui, |ui| {
            self.recovery_dirty |= voice_profile(ui, &mut self.voice_profile, self.speaking);
        });
        ui.add_space(8.0);
        settings_card(ui, |ui| {
            ui.label(
                RichText::new(if self.speech_output {
                    "Read aloud is available"
                } else {
                    "Read aloud becomes available after speech preparation."
                })
                .strong(),
            );
            if ui.button("Read aloud setup").clicked() {
                self.settings_view.voice_help = !self.settings_view.voice_help;
            }
            if self.settings_view.voice_help {
                ui.add_space(8.0);
                ui.label(self.read_aloud_help());
            }
        });
        if let Some(error) = self.setup.speech_error.clone() {
            ui.add_space(8.0);
            settings_card(ui, |ui| {
                ui.label(RichText::new("Speech setup needs attention").strong());
                super::super::error_ui::summary(ui, &error);
                ui.horizontal_wrapped(|ui| {
                    super::super::error_ui::copy_button(ui, "Copy error", &error);
                    self.speech_setup_error_actions(ui, &error);
                });
                ui.collapsing("Details", |ui| super::super::error_ui::details(ui, &error));
            });
        }
    }

    pub(super) fn application_preferences(&mut self, ui: &mut egui::Ui) {
        settings_card(ui, |ui| self.update_controls(ui));
        ui.add_space(8.0);
        settings_card(ui, |ui| self.runtime_controls(ui));
        ui.add_space(8.0);
        settings_card(ui, |ui| {
            egui::CollapsingHeader::new("Diagnostics")
                .id_salt("settings-diagnostics")
                .show(ui, |ui| self.diagnostics_contents(ui));
        });
    }

    pub(in crate::window) fn diagnostics_contents(&self, ui: &mut egui::Ui) {
        ui.label("Recent application messages");
        ui.label(
            RichText::new("Kept in memory for this session. Review paths before sharing.")
                .small()
                .weak(),
        );
        let copied = ui.make_persistent_id("diagnostics-copy");
        super::super::controls::CopyButton::new("Copy diagnostics", copied)
            .source(egui::Id::new(self.diagnostics_revision))
            .show(ui, || {
                self.diagnostics
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    .join("\n\n")
            })
            .on_hover_text(
                "Copy recent application messages, including details beyond the preview.",
            );
        ui.label(
            RichText::new("Recent previews only. Use Copy error on a notice for its full message.")
                .small()
                .weak(),
        );
        egui::ScrollArea::vertical()
            .id_salt("diagnostic-messages")
            .max_height(240.0)
            .min_scrolled_height(0.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                super::super::navigation::bounded_width(ui, |ui| {
                    for message in &self.diagnostics {
                        ui.add(egui::Label::new(message).selectable(true).wrap());
                        ui.separator();
                    }
                });
            });
    }
}

fn voice_profile(ui: &mut egui::Ui, profile: &mut String, speaking: bool) -> bool {
    let label = ui.label(RichText::new("Voice profile").strong());
    ui.add_space(8.0);
    let changed = ui
        .add_enabled(
            !speaking,
            egui::TextEdit::multiline(profile)
                .desired_rows(4)
                .desired_width(f32::INFINITY)
                .char_limit(VOICE_LIMIT),
        )
        .labelled_by(label.id)
        .changed();
    ui.label(
        RichText::new("Describe the voice you want to hear. Applies to your next read aloud.")
            .small()
            .weak(),
    );
    if speaking {
        ui.label("Finish or stop speech before changing the voice profile.");
    }
    changed
}
