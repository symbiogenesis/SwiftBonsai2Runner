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
        let font = egui::TextStyle::Small.resolve(ui.style());
        let text_width = |text: &str| {
            ui.painter()
                .layout_no_wrap(text.to_owned(), font.clone(), ui.visuals().text_color())
                .size()
                .x
        };
        // Include the two icons and their four-point label gaps. Only the gaps
        // between groups tighten on a narrow dock; text keeps its natural width.
        let content_width =
            text_width(model) + text_width("History") + text_width("Private") + 32.0;
        let available = ui.available_width();
        let group_gap = ((available - content_width) * 0.5).clamp(0.0, 14.0);
        let inset = ((available - content_width - 2.0 * group_gap) * 0.5).max(0.0);
        ui.allocate_ui_with_layout(
            egui::vec2(available, 0.0),
            egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(false),
            |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.add_space(inset);
                ui.add(egui::Label::new(RichText::new(model).small()).extend())
                    .accessible_name(description)
                    .on_hover_text(description);
                ui.add_space(group_gap);
                ui.add(egui::Label::new(RichText::new("History").small().weak()).extend())
                    .accessible_name(saving_description)
                    .on_hover_text(saving_description);
                ui.add_space(4.0);
                history_indicator(ui, self.save_conversations).on_hover_text(saving_description);
                ui.add_space(group_gap);
                privacy_icon(ui).on_hover_text("Private · On your device");
                ui.add_space(4.0);
                ui.add(egui::Label::new(RichText::new("Private").small().weak()).extend())
                    .accessible_name("Private · On your device")
                    .on_hover_text("Private · On your device");
            },
        );
    }
}

fn history_indicator(ui: &mut egui::Ui, enabled: bool) -> egui::Response {
    // A small monochrome rail reads as status, without a switch control's
    // filled capsule, accent or hover treatment. Thumb position carries state.
    let (track, response) = ui.allocate_exact_size(egui::vec2(14.0, 8.0), egui::Sense::hover());
    let ink = ui.visuals().weak_text_color();
    ui.painter().line_segment(
        [
            egui::pos2(track.left() + 3.0, track.center().y),
            egui::pos2(track.right() - 3.0, track.center().y),
        ],
        egui::Stroke::new(1.0, ink),
    );
    ui.painter().circle_filled(
        egui::pos2(
            if enabled {
                track.right() - 3.0
            } else {
                track.left() + 3.0
            },
            track.center().y,
        ),
        2.0,
        ink,
    );
    response
}

fn privacy_icon(ui: &mut egui::Ui) -> egui::Response {
    let (lock, response) = ui.allocate_exact_size(egui::vec2(10.0, 12.0), egui::Sense::hover());
    let stroke = egui::Stroke::new(1.0, ui.visuals().weak_text_color());
    ui.painter().rect_stroke(
        egui::Rect::from_min_max(
            lock.min + egui::vec2(2.5, 1.0),
            lock.min + egui::vec2(7.5, 8.0),
        ),
        2.5,
        stroke,
        egui::StrokeKind::Inside,
    );
    ui.painter().rect(
        egui::Rect::from_min_max(lock.min + egui::vec2(1.0, 5.0), lock.max),
        1.5,
        ui.visuals().window_fill,
        stroke,
        egui::StrokeKind::Inside,
    );
    response
}
