//! Quiet composer guidance; estimates never participate in request admission.
use super::{ModelLifecycle, OUTPUT_TOKENS, RichText, State, egui};
use crate::context::{self, Estimate, Pressure};

#[derive(Default)]
pub(super) struct DraftCache {
    text: String,
    estimate: Option<Estimate>,
    extra_effort: bool,
}

impl State {
    pub(super) fn context_estimate(&mut self) -> Estimate {
        let history = *self
            .context_history
            .get_or_insert_with(|| context::history_tokens(&self.committed));
        let output = self.selected_output_cap();
        if let Some(estimate) = self.context_draft.estimate
            && estimate.history == history
            && estimate.output == usize::try_from(output).unwrap_or(usize::MAX)
            && self.context_draft.extra_effort == self.extra_effort
            && self.context_draft.text == self.input
        {
            return estimate;
        }
        let estimate = Estimate::new(history, &self.input, output, self.extra_effort);
        self.input.clone_into(&mut self.context_draft.text);
        self.context_draft.extra_effort = self.extra_effort;
        self.context_draft.estimate = Some(estimate);
        estimate
    }

    fn selected_output_cap(&self) -> u32 {
        OUTPUT_TOKENS
            .get(self.output_budget)
            .copied()
            .unwrap_or(4096)
    }

    fn select_output_cap(&mut self, cap: u32) {
        if !self.model.busy()
            && let Some(index) = OUTPUT_TOKENS.iter().position(|&value| value == cap)
            && self.output_budget != index
        {
            self.output_budget = index;
            self.recovery_dirty = true;
        }
    }

    fn context_pending(&self) -> bool {
        self.request_conversation == Some(self.active_conversation)
            && matches!(
                self.model,
                ModelLifecycle::Generating
                    | ModelLifecycle::Stopping
                    | ModelLifecycle::AwaitingTeardown
            )
    }

    pub(super) fn context_toolbar(&mut self, ui: &mut egui::Ui) -> egui::Response {
        let estimate = self.context_estimate();
        let pending = self.context_pending();
        let label = ui
            .push_id(("context-capacity", self.active_conversation), |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    let label = ui.label(RichText::new("Message").small().weak());
                    let summary = if pending {
                        "Context · updating".to_owned()
                    } else {
                        let percent = estimate.total.saturating_mul(100) / context::CONTEXT_TOKENS;
                        format!("Context · ~{percent}%")
                    };
                    let color = if pending || estimate.pressure() == Pressure::Comfortable {
                        ui.visuals().weak_text_color()
                    } else {
                        ui.visuals().warn_fg_color
                    };
                    let response = ui
                        .add(
                            egui::Button::new(RichText::new(summary).small().color(color))
                                .frame_when_inactive(false)
                                .corner_radius(8),
                        )
                        .accessible_name("Context capacity")
                        .on_hover_text("Estimated space for this draft, completed history and the reply limit. Open for details.");
                    egui::Popup::menu(&response)
                        .align(egui::RectAlign::TOP_START)
                        .width(320.0)
                        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                        .show(|ui| self.context_details(ui, estimate, pending));
                    ui.add_enabled_ui(!self.model.busy(), |ui| {
                        egui::ComboBox::from_id_salt("composer-output-cap")
                            .selected_text(format!("Reply · {}", tokens(estimate.output)))
                            .width(122.0)
                            .show_ui(ui, |ui| {
                                for cap in OUTPUT_TOKENS {
                                    if ui
                                        .selectable_label(
                                            self.selected_output_cap() == cap,
                                            format!("{} tokens", tokens(usize::try_from(cap).unwrap_or(4096))),
                                        )
                                        .clicked()
                                    {
                                        self.select_output_cap(cap);
                                    }
                                }
                            })
                            .response
                            .accessible_name("Reply token limit")
                            .on_hover_text("Maximum tokens for the next reply, including thinking. Saved for this conversation.");
                    });
                    self.response_summary(ui);
                    label
                })
                .inner
            })
            .inner;
        // Re-evaluate after a reply-limit selection so the guidance updates in
        // the same frame. Ordinary messages carry no extra warning row.
        let estimate = self.context_estimate();
        if !pending && estimate.pressure() != Pressure::Comfortable {
            self.context_guidance(ui, estimate);
        }
        label
    }

    fn context_details(&mut self, ui: &mut egui::Ui, estimate: Estimate, pending: bool) {
        ui.heading("Space for this draft");
        ui.label(
            RichText::new("Estimate · exact token count checked when you send")
                .small()
                .weak(),
        );
        ui.add_space(8.0);
        if pending {
            ui.label(
                "Updates when this response finishes. The unfinished turn is not included below.",
            );
        } else {
            ui.label(format!(
                "~{} of {} tokens reserved",
                tokens(estimate.total),
                tokens(context::CONTEXT_TOKENS)
            ));
        }
        capacity_meter(ui, estimate);
        ui.add_space(8.0);
        egui::Grid::new("context-breakdown")
            .num_columns(2)
            .spacing([18.0, 6.0])
            .show(ui, |ui| {
                for (label, amount, approximate) in [
                    ("Completed history & reasoning", estimate.history, true),
                    ("This draft & message framing", estimate.draft, true),
                    ("Extra effort planning", estimate.workflow, true),
                    ("Reply limit (includes thinking)", estimate.output, false),
                    ("Safety reserve", context::RESERVE_TOKENS, false),
                ] {
                    if amount == 0 && label == "Extra effort planning" {
                        continue;
                    }
                    ui.label(RichText::new(label).small());
                    ui.label(
                        RichText::new(format!(
                            "{}{}",
                            if approximate { "~" } else { "" },
                            tokens(amount)
                        ))
                        .small(),
                    );
                    ui.end_row();
                }
            });
        ui.add_space(8.0);
        if !pending && estimate.overflow() == 0 {
            ui.label(format!(
                "About {} tokens of headroom after the reply reservation.",
                tokens(estimate.remaining())
            ));
        }
        ui.label(RichText::new("Language and formatting affect tokenization. This estimate can be high or low; it does not guarantee that a request fits.").small().weak());
        if self.extra_effort {
            ui.label("First pass only. Extra effort's later passes add generated notes and drafts and need more room.");
        }
        ui.label(RichText::new("Changing the reply limit keeps every completed turn and your draft. Message actions use their own prompts.").small().weak());
        if !pending && estimate.pressure() != Pressure::Comfortable {
            ui.add_space(8.0);
            self.context_actions(ui, estimate);
        }
    }

    fn context_guidance(&mut self, ui: &mut egui::Ui, estimate: Estimate) {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(if estimate.pressure() == Pressure::Over {
                    "This draft may exceed the context limit."
                } else {
                    "This conversation may be near its context limit."
                })
                .small()
                .color(ui.visuals().warn_fg_color),
            );
            self.context_actions(ui, estimate);
        });
    }

    fn context_actions(&mut self, ui: &mut egui::Ui, estimate: Estimate) {
        if let Some(cap) = estimate.suggested_cap()
            && ui
                .add_enabled(!self.model.busy(), egui::Button::new(format!("Use {}-token reply", tokens(usize::try_from(cap).unwrap_or(4096)))).small())
                .on_hover_text("Change only the reply limit for this conversation; fit is still checked exactly when sending.")
                .clicked()
        {
            self.select_output_cap(cap);
            egui::Popup::close_all(ui.ctx());
        }
        if ui
            .add_enabled(
                self.model.workspace_available(),
                egui::Button::new("Start fresh").small(),
            )
            .on_hover_text("Open a new conversation. This conversation and its draft are kept.")
            .clicked()
        {
            self.new_chat();
            egui::Popup::close_all(ui.ctx());
        }
    }
}

fn capacity_meter(ui: &mut egui::Ui, estimate: Estimate) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 6.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, 3.0, ui.visuals().faint_bg_color);
    let accent = ui.visuals().selection.bg_fill;
    let mut used = 0_usize;
    for (amount, color) in [
        (estimate.history, ui.visuals().weak_text_color()),
        (estimate.draft.saturating_add(estimate.workflow), accent),
        (estimate.output, accent.gamma_multiply(0.55)),
        (context::RESERVE_TOKENS, ui.visuals().text_color()),
    ] {
        let start = used.min(context::CONTEXT_TOKENS);
        used = used.saturating_add(amount);
        let end = used.min(context::CONTEXT_TOKENS);
        // Both positions are clamped to 8192, well inside exact f32 integers.
        let fraction = |value| f32::from(u16::try_from(value).unwrap_or(8192)) / 8192.0;
        let left = rect.width().mul_add(fraction(start), rect.left());
        let right = rect.width().mul_add(fraction(end), rect.left());
        if right > left {
            ui.painter().rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(left, rect.top()),
                    egui::pos2(right, rect.bottom()),
                ),
                1.0,
                color,
            );
        }
    }
}

fn tokens(amount: usize) -> String {
    let raw = amount.to_string();
    let mut formatted = String::with_capacity(raw.len() + raw.len() / 3);
    for (index, character) in raw.chars().enumerate() {
        if index != 0 && (raw.len() - index).is_multiple_of(3) {
            formatted.push(',');
        }
        formatted.push(character);
    }
    formatted
}

#[cfg(test)]
mod tests;
