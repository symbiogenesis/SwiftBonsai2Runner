//! A single, native status line beneath Settings, with full accessible descriptions.
use super::{RichText, State, egui};

impl State {
    pub(super) fn sidebar_status(&self, ui: &mut egui::Ui) {
        let (model, description) = if self.preview {
            ("◌ Preview", "Interface preview")
        } else if self.model.busy() {
            ("● Replying", "Model responding")
        } else if self.model.ready() {
            ("● Ready", "Model ready")
        } else if self.model.retryable() {
            ("● Paused", "Model paused")
        } else if self.model.stopping() {
            ("◌ Stopping", "Releasing model")
        } else if self.model.waiting_model() {
            ("◌ Loading", "Preparing model")
        } else {
            ("● Offline", "Model unavailable")
        };
        let saving_description = if self.save_conversations {
            "Conversation saving is on for drafts and history on this device. Change it in Settings → Privacy."
        } else {
            "Conversation saving is off. Conversations stay in this window. Change it in Settings → Privacy."
        };
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), 0.0),
            egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(false),
            |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                // Natural label widths keep text un-justified and the three
                // groups together, in the same order at every sidebar width.
                ui.add(egui::Label::new(RichText::new(model).small()).extend())
                    .accessible_name(description)
                    .on_hover_text(description);
                ui.label(RichText::new("·").small().weak());
                ui.add(egui::Label::new(RichText::new("History").small().weak()).extend())
                    .accessible_name(saving_description)
                    .on_hover_text(saving_description);
                history_indicator(ui, self.save_conversations).on_hover_text(saving_description);
                ui.label(RichText::new("·").small().weak());
                ui.add(egui::Label::new(RichText::new("Private").small().weak()).extend())
                    .accessible_name("Private · On your device")
                    .on_hover_text("Private · On your device");
            },
        );
    }
}

fn history_indicator(ui: &mut egui::Ui, enabled: bool) -> egui::Response {
    // This is a status glyph; the saving preference stays in Settings → Privacy.
    let (track, response) = ui.allocate_exact_size(egui::vec2(18.0, 10.0), egui::Sense::hover());
    let visuals = ui.visuals();
    let (fill, ink) = if enabled {
        (visuals.selection.bg_fill, visuals.selection.stroke.color)
    } else {
        (visuals.extreme_bg_color, visuals.weak_text_color())
    };
    ui.painter().rect(
        track,
        5.0,
        fill,
        egui::Stroke::new(1.0, visuals.weak_text_color()),
        egui::StrokeKind::Inside,
    );
    ui.painter().circle_filled(
        egui::pos2(
            if enabled {
                track.right() - 4.0
            } else {
                track.left() + 4.0
            },
            track.center().y,
        ),
        3.0,
        ink,
    );
    response
}
