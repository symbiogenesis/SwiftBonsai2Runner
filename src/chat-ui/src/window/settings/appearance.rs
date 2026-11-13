//! Device appearance editing; persistence remains with the window.
use super::super::appearance;
use super::{RichText, State, controls, egui, settings_card};

impl State {
    pub(super) fn appearance_preferences(&mut self, ui: &mut egui::Ui) {
        if appearance_preferences(ui, &mut self.appearance) {
            self.appearance_changed(ui.ctx());
        }
    }
}

fn appearance_preferences(ui: &mut egui::Ui, preferences: &mut appearance::Preferences) -> bool {
    let before = *preferences;
    settings_card(ui, |ui| {
        ui.label(RichText::new("Theme").strong());
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            for theme in appearance::Theme::ALL {
                let selected = preferences.theme == theme;
                if ui
                    .add(
                        controls::choice_button(ui, theme.label(), selected)
                            .min_size(egui::vec2(78.0, 36.0)),
                    )
                    .clicked()
                {
                    preferences.theme = theme;
                }
            }
        });
        ui.add_space(8.0);
        ui.label(
            RichText::new("System follows Windows. High contrast always takes priority.")
                .small()
                .weak(),
        );
    });
    ui.add_space(8.0);
    settings_card(ui, |ui| {
        ui.label(RichText::new("Reading comfort").strong());
        ui.add_space(12.0);
        ui.label("Text size");
        ui.add(
            egui::Slider::new(&mut preferences.text_size, 14..=24)
                .step_by(1.0)
                .suffix(" pt")
                .text("Reading text size"),
        );
        ui.add_space(12.0);
        ui.label("Reading width");
        ui.add(
            egui::Slider::new(&mut preferences.reading_width, 640..=1_040)
                .step_by(40.0)
                .suffix(" pt")
                .text("Reading column width"),
        );
        ui.label(RichText::new("The conversation and composer fit the space available. Controls keep their usual size.").small().weak());
        ui.add_space(16.0);
        egui::Frame::new()
                .fill(ui.visuals().extreme_bg_color)
                .corner_radius(10)
                .inner_margin(14)
                .show(ui, |ui| {
                    ui.label(RichText::new("A little space to think.").strong().size(f32::from(preferences.text_size)));
                    ui.add_space(8.0);
                    ui.label(RichText::new("Follow an idea at your own pace. Find the words, take a breath, and leave room for what comes next.").size(f32::from(preferences.text_size)));
                });
    });
    ui.add_space(12.0);
    ui.label(
        RichText::new(
            "Appearance is saved on this device, including when conversation saving is off.",
        )
        .small()
        .weak(),
    );
    if ui.button("Reset appearance").clicked() {
        let defaults = appearance::Preferences::default();
        preferences.theme = defaults.theme;
        preferences.text_size = defaults.text_size;
        preferences.reading_width = defaults.reading_width;
    }
    *preferences != before
}
