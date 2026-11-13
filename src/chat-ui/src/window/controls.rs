//! Shared native control styling. Callers own availability, focus and actions.
use super::{RichText, egui};

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
