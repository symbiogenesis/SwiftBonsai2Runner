//! Shared native control styling. Callers own availability, focus and actions.
use super::{RichText, egui};
use std::time::Duration;

/// A width-stable copy action whose caller owns content identity and production.
#[must_use]
pub(super) struct CopyButton<'a> {
    label: &'a str,
    feedback: egui::Id,
    source: egui::Id,
    min_size: egui::Vec2,
    corner_radius: Option<u8>,
    primary: bool,
    confirmation_duration: Duration,
}

impl<'a> CopyButton<'a> {
    /// Reuse one stable feedback slot for this control across source changes.
    pub(super) const fn new(label: &'a str, feedback: egui::Id) -> Self {
        Self {
            label,
            feedback,
            source: feedback,
            min_size: egui::Vec2::ZERO,
            corner_radius: None,
            primary: false,
            confirmation_duration: Duration::from_millis(1_500),
        }
    }

    /// Invalidate confirmation when the caller's copyable content changes.
    pub(super) const fn source(mut self, source: egui::Id) -> Self {
        self.source = source;
        self
    }

    /// Preserve the surrounding control row's geometry during confirmation.
    pub(super) const fn min_size(mut self, size: egui::Vec2) -> Self {
        self.min_size = size;
        self
    }

    /// Match the caller's control shape rather than changing its native default.
    pub(super) const fn corner_radius(mut self, radius: u8) -> Self {
        self.corner_radius = Some(radius);
        self
    }

    /// Use the theme's paired selection colors for a primary action.
    pub(super) const fn primary(mut self, primary: bool) -> Self {
        self.primary = primary;
        self
    }

    /// Set the duration of the caller's transient copy confirmation.
    pub(super) const fn confirmation_duration(mut self, duration: Duration) -> Self {
        self.confirmation_duration = duration;
        self
    }

    /// Produce the complete clipboard payload only after activation.
    pub(super) fn show(
        self,
        ui: &mut egui::Ui,
        contents: impl FnOnce() -> String,
    ) -> egui::Response {
        let now = ui.input(|input| input.time);
        let until = ui.ctx().data_mut(|data| {
            let feedback = data.get_temp::<CopyFeedback>(self.feedback)?;
            if feedback.source == self.source && now < feedback.until {
                Some(feedback.until)
            } else {
                data.remove::<CopyFeedback>(self.feedback);
                None
            }
        });
        let copied = until.is_some();
        // Match egui's Body-font button theme in both measurement and paint.
        let font = egui::TextStyle::Body.resolve(ui.style());
        let text_width = ui
            .painter()
            .layout_no_wrap(
                self.label.to_owned(),
                font.clone(),
                ui.visuals().text_color(),
            )
            .size()
            .x;
        let confirmation_width = ui
            .painter()
            .layout_no_wrap("Copied".to_owned(), font.clone(), ui.visuals().text_color())
            .size()
            .x;
        let text_width = text_width.max(confirmation_width);
        let width = ui.spacing().button_padding.x.mul_add(2.0, text_width);
        let mut text = RichText::new(if copied { "Copied" } else { self.label }).font(font);
        if self.primary {
            text = text.color(ui.visuals().selection.stroke.color);
        }
        let mut button = egui::Button::new(text)
            .min_size(egui::vec2(width.max(self.min_size.x), self.min_size.y));
        if let Some(radius) = self.corner_radius {
            button = button.corner_radius(radius);
        }
        if self.primary {
            button = button.fill(ui.visuals().selection.bg_fill);
        }
        let response = ui.add(button).accessible_name(self.label);
        if response.clicked() {
            ui.ctx().copy_text(contents());
            ui.ctx().data_mut(|data| {
                data.insert_temp(
                    self.feedback,
                    CopyFeedback {
                        source: self.source,
                        until: now + self.confirmation_duration.as_secs_f64(),
                    },
                );
            });
            ui.ctx().request_repaint();
            ui.ctx().request_repaint_after(self.confirmation_duration);
        } else if let Some(until) = until {
            let remaining =
                Duration::try_from_secs_f64(until - now).unwrap_or(self.confirmation_duration);
            ui.ctx().request_repaint_after(remaining);
        }
        response
    }
}

/// One transient confirmation per control, regardless of content revisions.
#[derive(Clone, Copy)]
struct CopyFeedback {
    source: egui::Id,
    until: f64,
}

/// A standard-sized secondary action, with its label and behavior at the call site.
pub(super) fn action_button(label: &str) -> egui::Button<'_> {
    egui::Button::new(label)
        .min_size(egui::vec2(0.0, 36.0))
        .corner_radius(8)
}

/// A quiet inline notice that follows native theme and high-contrast colors.
pub(super) fn notice_frame(ui: &egui::Ui) -> egui::Frame {
    egui::Frame::new()
        .fill(ui.visuals().faint_bg_color)
        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
        .corner_radius(12)
        .inner_margin(egui::Margin::symmetric(14, 12))
}

/// Shared dialog chrome; modality and focus ownership belong to the caller.
pub(super) fn dialog_frame(ctx: &egui::Context) -> egui::Frame {
    let visuals = &ctx.global_style().visuals;
    egui::Frame::new()
        .fill(visuals.window_fill)
        .stroke(visuals.window_stroke)
        .shadow(visuals.window_shadow)
        .corner_radius(16)
        .inner_margin(20)
}

/// Use the current theme's paired selection colors for a primary action.
pub(super) fn primary_button(ui: &egui::Ui, text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text).color(ui.visuals().selection.stroke.color))
        .fill(ui.visuals().selection.bg_fill)
        .corner_radius(10)
}

/// A choice announces its selected state, unlike a popup opener.
pub(super) fn choice_button(ui: &egui::Ui, text: &str, selected: bool) -> egui::Button<'static> {
    let color = if selected {
        ui.visuals().selection.stroke.color
    } else {
        ui.visuals().text_color()
    };
    egui::Button::selectable(selected, RichText::new(text).color(color)).corner_radius(10)
}

/// De-emphasize secondary actions while retaining hover and keyboard feedback.
pub(super) fn quiet_button<'a>(contents: impl egui::IntoAtoms<'a>) -> egui::Button<'a> {
    egui::Button::new(contents).frame_when_inactive(false)
}

/// Keep popup openers consistent across native themes without toggle semantics.
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

/// Draw arbitrary toolbar contents with the same scoped geometry and feedback.
pub(super) fn toolbar_action<'a>(
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
                // Highlight the setting represented by the opener. Its action
                // still opens a popup rather than directly toggling a setting.
                .with_class_if(egui::Button::CLASS_SELECTED, active),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand)
    })
    .inner
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::{Harness, kittest::Queryable as _};

    #[test]
    fn copy_button_builds_complete_payload_only_on_activation_and_preserves_geometry() {
        struct Fixture {
            payload: String,
            productions: usize,
            copied: Option<String>,
        }

        let mut control = Harness::builder()
            .with_size([420.0, 100.0])
            .with_context_setup(super::super::configure)
            .build_ui_state(
                |ui, fixture: &mut Fixture| {
                    let id = ui.make_persistent_id("lazy-copy");
                    CopyButton::new("Copy complete message", id)
                        .min_size(egui::vec2(0.0, 36.0))
                        .corner_radius(8)
                        .primary(true)
                        .confirmation_duration(Duration::from_secs(2))
                        .show(ui, || {
                            fixture.productions += 1;
                            fixture.payload.clone()
                        });
                    ui.ctx().output(|output| {
                        for command in &output.commands {
                            if let egui::OutputCommand::CopyText(text) = command {
                                fixture.copied = Some(text.clone());
                            }
                        }
                    });
                },
                Fixture {
                    payload: "Complete Unicode details: 日本語 🌱\r\n".repeat(1_000),
                    productions: 0,
                    copied: None,
                },
            );
        control.run();
        assert_eq!(control.state().productions, 0);
        assert!(control.state().copied.is_none());
        let before = control
            .get_by_role_and_label(egui::Role::Button, "Copy complete message")
            .rect();
        control
            .get_by_role_and_label(egui::Role::Button, "Copy complete message")
            .click();
        control.run();
        assert_eq!(control.state().productions, 1);
        assert_eq!(
            control.state().copied.as_deref(),
            Some(control.state().payload.as_str())
        );
        let after = control
            .get_by_role_and_label(egui::Role::Button, "Copy complete message")
            .rect();
        assert!((before.width() - after.width()).abs() < 0.5);
        assert!((before.height() - after.height()).abs() < 0.5);
        control.run();
        assert_eq!(control.state().productions, 1);
    }

    #[test]
    fn copy_button_reuses_one_feedback_slot_across_sources_and_removes_expired_confirmation() {
        struct Fixture {
            source: u64,
        }

        let mut control = Harness::builder()
            .with_size([200.0, 100.0])
            .with_context_setup(super::super::configure)
            .build_ui_state(
                |ui, fixture: &mut Fixture| {
                    let slot = ui.make_persistent_id("bounded-copy-feedback");
                    CopyButton::new("Copy", slot)
                        .source(egui::Id::new(fixture.source))
                        .show(ui, || format!("Payload {}", fixture.source));
                },
                Fixture { source: 0 },
            );
        for source in 0..16 {
            control.state_mut().source = source;
            control.step();
            assert_eq!(control.ctx.data(|data| data.count::<CopyFeedback>()), 0);
            let before = control
                .get_by_role_and_label(egui::Role::Button, "Copy")
                .rect();
            control
                .get_by_role_and_label(egui::Role::Button, "Copy")
                .click();
            control.step();
            control.step();
            assert_eq!(control.ctx.data(|data| data.count::<CopyFeedback>()), 1);
            let after = control
                .get_by_role_and_label(egui::Role::Button, "Copy")
                .rect();
            assert!((before.width() - after.width()).abs() < 0.5);
            assert!((before.height() - after.height()).abs() < 0.5);
        }
        let expired = control.ctx.input(|input| input.time) + 2.0;
        control.input_mut().time = Some(expired);
        control.step();
        assert_eq!(control.ctx.data(|data| data.count::<CopyFeedback>()), 0);
    }
}
