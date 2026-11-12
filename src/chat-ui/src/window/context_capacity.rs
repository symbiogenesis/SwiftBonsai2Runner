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
                    let response = context_button(ui, estimate, pending);
                    let frame = egui::Frame::popup(ui.style())
                        .corner_radius(14)
                        .inner_margin(14);
                    // Keep details above both toolbar actions. An unconstrained
                    // tall popup can be moved down by viewport containment and
                    // intercept the neighboring Thinking button's pointer input.
                    let detail_height = (response.rect.top()
                        - ui.ctx().content_rect().top()
                        - frame.total_margin().sum().y
                        - 6.0)
                        .max(0.0);
                    egui::Popup::menu(&response)
                        .accessible_name("Context details")
                        .align(egui::RectAlign::TOP_START)
                        .align_alternatives(&[egui::RectAlign::TOP_END])
                        .width(320.0)
                        .gap(6.0)
                        .frame(frame)
                        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                        .show(|ui| {
                            egui::ScrollArea::vertical()
                                .id_salt("context-details")
                                .max_height(detail_height)
                                .min_scrolled_height(0.0)
                                .auto_shrink([false, true])
                                .show(ui, |ui| self.context_details(ui, estimate, pending));
                        });
                    self.thinking_button(ui);
                    label
                })
                .inner
            })
            .inner;
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
        ui.label(RichText::new("Reply limits are in Settings → Response → Advanced response controls. Changing them keeps your conversation and draft. Message actions use their own prompts.").small().weak());
        if !pending && estimate.pressure() != Pressure::Comfortable {
            if estimate.suggested_cap().is_some() {
                ui.label("A smaller reply limit could leave more room for this message.");
            }
            ui.add_space(8.0);
            self.context_actions(ui);
        }
    }

    fn context_guidance(&mut self, ui: &mut egui::Ui, estimate: Estimate) {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(if estimate.pressure() == Pressure::Over {
                    "This message may be too long for this conversation."
                } else {
                    "This conversation is getting full."
                })
                .small()
                .color(ui.visuals().warn_fg_color),
            );
            self.context_actions(ui);
        });
    }

    fn context_actions(&mut self, ui: &mut egui::Ui) {
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

fn context_button(ui: &mut egui::Ui, estimate: Estimate, pending: bool) -> egui::Response {
    let used = u16::try_from(estimate.total.min(context::CONTEXT_TOKENS)).unwrap_or(8192);
    let fraction = f32::from(used) / 8192.0;
    let ring = egui::Atom::paint(egui::Vec2::splat(16.0), move |ui, args| {
        context_ring(
            ui,
            args.rect.center(),
            fraction,
            estimate.pressure(),
            pending,
        );
    });
    let response = toolbar_action(ui, (RichText::new("Context:").size(13.0), ring), false)
        .accessible_name("Context");
    let description = if pending {
        "Context updates when this response finishes. Open for details.".to_owned()
    } else {
        let percent = estimate.total.saturating_mul(100) / context::CONTEXT_TOKENS;
        format!("About {percent}% of context reserved, including the next reply. Open for details.")
    };
    let _ = response.ctx.accesskit_node_builder(response.id, |node| {
        node.set_description(description.clone());
    });
    response.on_hover_text(description)
}

fn context_ring(
    ui: &egui::Ui,
    center: egui::Pos2,
    fraction: f32,
    pressure: Pressure,
    pending: bool,
) {
    const RADIUS: f32 = 6.5;
    ui.painter().circle_stroke(
        center,
        RADIUS,
        egui::Stroke::new(1.0, ui.visuals().weak_text_color()),
    );
    if pending {
        // An unfilled ring with a dash means the unfinished turn is not yet
        // counted. Do not display a stale fraction or animate an idle widget.
        ui.painter().line_segment(
            [center - egui::vec2(2.0, 0.0), center + egui::vec2(2.0, 0.0)],
            egui::Stroke::new(1.5, ui.visuals().weak_text_color()),
        );
        return;
    }
    let color = if pressure == Pressure::Comfortable {
        ui.visuals().text_color()
    } else {
        ui.visuals().warn_fg_color
    };
    if fraction > 0.0 {
        let points = (0..=48_u16)
            .map(|step| {
                let angle = std::f32::consts::TAU.mul_add(
                    fraction * f32::from(step) / 48.0,
                    -std::f32::consts::FRAC_PI_2,
                );
                let (sin, cos) = angle.sin_cos();
                center + egui::vec2(cos, sin) * RADIUS
            })
            .collect();
        ui.painter()
            .add(egui::Shape::line(points, egui::Stroke::new(2.5, color)));
    }
}

/// Keep the composer's popup actions consistent across native themes.
pub(super) fn toolbar_button(ui: &mut egui::Ui, text: &str, active: bool) -> egui::Response {
    let color = if active {
        ui.visuals().selection.stroke.color
    } else {
        ui.visuals().text_color()
    };
    let chevron = egui::Atom::paint(egui::Vec2::splat(8.0), move |ui, args| {
        let center = args.rect.center();
        ui.painter().add(egui::Shape::line(
            vec![
                center + egui::vec2(-2.5, -1.5),
                center + egui::vec2(0.0, 1.0),
                center + egui::vec2(2.5, -1.5),
            ],
            egui::Stroke::new(1.25, color),
        ));
    });
    toolbar_action(
        ui,
        (RichText::new(text).size(13.0).color(color), chevron),
        active,
    )
}

fn toolbar_action<'a>(
    ui: &mut egui::Ui,
    contents: impl egui::IntoAtoms<'a>,
    active: bool,
) -> egui::Response {
    use egui::class::HasClasses as _;

    ui.scope(|ui| {
        ui.spacing_mut().button_padding = egui::vec2(10.0, 6.0);
        ui.add(
            egui::Button::new(contents)
                .gap(7.0)
                .corner_radius(16)
                .min_size(egui::vec2(0.0, 32.0))
                // Highlight enabled thinking without announcing a toggle:
                // activating the button opens its popup.
                .with_class_if(egui::Button::CLASS_SELECTED, active),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand)
    })
    .inner
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
