//! Navigation chrome returns responses without changing application state.
use crate::window::{RichText, controls, egui};

pub(super) fn navigation_toggle(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let response = ui
        .add(
            controls::quiet_button("")
                .min_size(egui::Vec2::splat(36.0))
                .corner_radius(8),
        )
        .accessible_name(label)
        .on_hover_text(format!("{label} · Ctrl+Shift+B"));
    let rect = egui::Rect::from_center_size(response.rect.center(), egui::vec2(18.0, 16.0));
    let stroke = ui.style().interact(&response).fg_stroke;
    let painter = ui.painter_at(response.rect);
    painter.rect_stroke(rect, 3.0, stroke, egui::StrokeKind::Middle);
    let divider = rect.left() + 6.0;
    painter.line_segment(
        [
            egui::pos2(divider, rect.top()),
            egui::pos2(divider, rect.bottom()),
        ],
        stroke,
    );
    response
}

pub(super) fn drawer_header(ui: &mut egui::Ui) -> egui::Response {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Conversations").strong().size(18.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add(controls::quiet_button("×").min_size(egui::Vec2::splat(36.0)))
                .accessible_name("Close conversations")
                .on_hover_text("Close conversations · Escape")
        })
        .inner
    })
    .inner
}
