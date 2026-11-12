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
            "Conversation saving is on for drafts and history on this device."
        } else {
            "Conversation saving is off. Conversations stay in this window."
        };
        let mut history = if self.save_conversations {
            "History on"
        } else {
            "History off"
        };
        let font = egui::TextStyle::Small.resolve(ui.style());
        let width = |text: &str| {
            ui.painter()
                .layout_no_wrap(text.to_owned(), font.clone(), ui.visuals().text_color())
                .size()
                .x
        };
        // Five four-point gaps and a ten-point lock. Measure using the active
        // font so narrow docks keep all three meanings at the normal Small size.
        let tail_width =
            |history: &str| width(history) + 2.0 * width("·") + width("Private") + 30.0;
        if width(model) + tail_width(history) > ui.available_width() {
            history = if self.save_conversations {
                "Save on"
            } else {
                "Save off"
            };
        }
        let model_width = (ui.available_width() - tail_width(history)).max(0.0);
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), 0.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.add_sized(
                    [model_width, 0.0],
                    egui::Label::new(RichText::new(model).small())
                        .halign(egui::Align::Min)
                        .truncate(),
                )
                .accessible_name(description)
                .on_hover_text(description);
                ui.label(RichText::new("·").small().weak());
                ui.add(egui::Label::new(RichText::new(history).small().weak()).extend())
                    .accessible_name(saving_description)
                    .on_hover_text(saving_description);
                ui.label(RichText::new("·").small().weak());
                // A vector lock avoids emoji substitution or missing icon fonts.
                let (lock, _) =
                    ui.allocate_exact_size(egui::vec2(10.0, 12.0), egui::Sense::hover());
                let painter = ui.painter();
                let stroke = egui::Stroke::new(1.0, ui.visuals().weak_text_color());
                painter.rect_stroke(
                    egui::Rect::from_min_max(
                        lock.min + egui::vec2(2.5, 1.0),
                        lock.min + egui::vec2(7.5, 8.0),
                    ),
                    2.5,
                    stroke,
                    egui::StrokeKind::Inside,
                );
                painter.rect(
                    egui::Rect::from_min_max(lock.min + egui::vec2(1.0, 5.0), lock.max),
                    1.5,
                    ui.visuals().window_fill,
                    stroke,
                    egui::StrokeKind::Inside,
                );
                ui.add(egui::Label::new(RichText::new("Private").small().weak()).extend())
                    .accessible_name("Private · On your device")
                    .on_hover_text("Private · On your device");
            },
        );
    }
}
