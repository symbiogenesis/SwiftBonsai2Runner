//! Conversation response controls shared by Settings and the composer popup.
use super::super::OUTPUT_TOKENS;
use super::{RichText, State, controls, egui, settings_card};

impl State {
    pub(in crate::window) fn thinking_button(&mut self, ui: &mut egui::Ui) {
        let help = match (self.enable_thinking, self.extra_effort) {
            (true, true) => {
                "Thinking and extra effort are on. Change how Swift thinks before replying."
            }
            (true, false) => "Thinking is on. Change how Swift thinks before replying.",
            (false, true) => "Extra effort is on. Change how Swift thinks before replying.",
            (false, false) => {
                "Choose whether Swift thinks before replying or takes extra time to review."
            }
        };
        let response =
            controls::toolbar_button(ui, "Thinking", self.enable_thinking || self.extra_effort)
                .on_hover_text(help);
        let opening = response.clicked()
            && !egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response));
        if opening {
            // Keyboard activation belongs to the choices after opening. If
            // generation disables both, Enter must not toggle the still-focused
            // opener closed and expose Stop to the following Escape.
            response.surrender_focus();
        }
        egui::Popup::menu(&response)
            .accessible_name("Thinking options")
            .align(egui::RectAlign::TOP_START)
            .width(280.0)
            .gap(6.0)
            .frame(
                egui::Frame::popup(ui.style())
                    .corner_radius(14)
                    .inner_margin(14),
            )
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                ui.label(RichText::new("Thinking").strong().size(16.0));
                ui.label(RichText::new("For this conversation").small().weak());
                ui.add_space(10.0);
                ui.separator();
                ui.add_space(10.0);
                self.thinking_preferences(ui, opening);
                if self.model.busy() {
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new("Available when this response finishes.")
                            .small()
                            .weak(),
                    );
                }
            });
    }

    fn thinking_preferences(&mut self, ui: &mut egui::Ui, focus: bool) {
        self.recovery_dirty |= ThinkingPreferences {
            thinking: &mut self.enable_thinking,
            extra_effort: &mut self.extra_effort,
        }
        .show(ui, !self.model.busy(), focus);
    }

    pub(super) fn response_preferences(&mut self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new("Changes affect the next response and every Extra effort pass.")
                .small()
                .weak(),
        );
        if self.model.busy() {
            ui.add_space(8.0);
            ui.label("Response controls unlock when the current response finishes.");
        }
        ui.add_space(8.0);
        settings_card(ui, |ui| {
            self.thinking_preferences(ui, false);
        });
        ui.add_space(8.0);
        settings_card(ui, |ui| {
            egui::CollapsingHeader::new("Advanced response controls")
                .id_salt("settings-advanced-response")
                .show(ui, |ui| {
                    ui.add_space(8.0);
                    self.recovery_dirty |=
                        reply_preferences(ui, &mut self.output_budget, !self.model.busy());
                    ui.add_space(12.0);
                    ui.separator();
                    self.recovery_dirty |= adapter_preferences(
                        ui,
                        &mut self.abliteration_scale,
                        self.abliteration_enabled,
                        !self.model.busy(),
                    );
                });
        });
    }
}

/// Borrow only the values edited by the shared Thinking control.
struct ThinkingPreferences<'a> {
    thinking: &'a mut bool,
    extra_effort: &'a mut bool,
}

impl ThinkingPreferences<'_> {
    fn show(self, ui: &mut egui::Ui, editable: bool, focus: bool) -> bool {
        ui.add_enabled_ui(editable, |ui| {
            ui.spacing_mut().interact_size.y = 28.0;
            let thinking = ui.checkbox(self.thinking, "Thinking");
            if focus && editable {
                thinking.request_focus();
            }
            let mut changed = thinking.changed();
            ui.label(
                RichText::new("Reason before answering. Uses part of the reply limit.")
                    .small()
                    .weak(),
            );
            ui.add_space(12.0);
            changed |= ui.checkbox(self.extra_effort, "Extra effort").changed();
            ui.label(
                RichText::new("Plan, draft and review. Experimental; takes longer and may not improve answers.")
                    .small()
                    .weak(),
            );
            changed
        })
        .inner
    }
}

fn reply_preferences(ui: &mut egui::Ui, output_budget: &mut usize, editable: bool) -> bool {
    ui.label(RichText::new("Reply length").strong());
    ui.add_space(8.0);
    let before = *output_budget;
    ui.add_enabled_ui(editable, |ui| {
        ui.horizontal(|ui| {
            let label = ui.label("Output cap");
            egui::ComboBox::from_id_salt("output-cap")
                .selected_text(format!(
                    "{} tokens",
                    OUTPUT_TOKENS.get(*output_budget).copied().unwrap_or(4096)
                ))
                .show_ui(ui, |ui| {
                    for (index, tokens) in OUTPUT_TOKENS.iter().enumerate() {
                        ui.selectable_value(output_budget, index, format!("{tokens} tokens"));
                    }
                })
                .response
                .labelled_by(label.id);
        });
    });
    ui.add_space(8.0);
    ui.label(RichText::new("The output cap includes thinking. A smaller cap leaves more room for your conversation. Open Context for the capacity selected for this model.").small().weak());
    before != *output_budget
}

fn adapter_preferences(ui: &mut egui::Ui, scale: &mut f32, enabled: bool, editable: bool) -> bool {
    ui.add_space(8.0);
    ui.label(RichText::new("Adapter strength").strong());
    ui.label(RichText::new("Abliteration changes the pinned adapter’s contribution to the response. It is an experimental model adjustment; higher values do not establish better quality.").small().weak());
    ui.add_space(8.0);
    let changed = ui
        .add_enabled_ui(editable && enabled, |ui| {
            ui.add(
                egui::Slider::new(scale, 0.0..=2.0)
                    .step_by(0.1)
                    .fixed_decimals(1)
                    .text("Abliteration scale"),
            )
            .changed()
        })
        .inner;
    ui.label(
        RichText::new(if enabled {
            "0 disables the correction. Default: 1.3. Applies to your next response."
        } else {
            "Disabled by the selected launch options."
        })
        .small()
        .weak(),
    );
    changed
}
