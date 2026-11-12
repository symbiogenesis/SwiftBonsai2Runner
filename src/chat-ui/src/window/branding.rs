//! Compact seedling drawn as native vectors, inspired by the README's sprout.
use super::egui;
use egui::{Color32, Pos2, Stroke, epaint::CubicBezierShape};

pub(super) fn mark(ui: &mut egui::Ui) {
    let (rect, response) = ui.allocate_exact_size(egui::Vec2::splat(26.0), egui::Sense::hover());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::Role::Image, ui.is_enabled(), "Swift Bonsai")
    });
    if !ui.is_rect_visible(rect) {
        return;
    }

    let painter = ui.painter_at(rect);
    // Theme ink keeps the small silhouette crisp, including Windows contrast themes.
    let ink = ui.visuals().text_color();
    let point = |x, y| rect.min + egui::vec2(x, y) * (rect.width() / 24.0);
    painter.add(CubicBezierShape::from_points_stroke(
        [
            point(12.0, 21.0),
            point(12.0, 16.5),
            point(11.0, 13.0),
            point(15.5, 8.5),
        ],
        false,
        Color32::TRANSPARENT,
        Stroke::new(1.8, ink),
    ));
    leaf(
        &painter,
        [
            point(11.8, 13.5),
            point(6.5, 14.0),
            point(3.0, 11.5),
            point(2.5, 6.0),
            point(8.0, 5.5),
            point(11.8, 8.0),
        ],
        ink,
    );
    leaf(
        &painter,
        [
            point(12.0, 12.0),
            point(11.5, 6.5),
            point(15.5, 2.5),
            point(21.5, 2.5),
            point(22.0, 8.5),
            point(18.0, 12.5),
        ],
        ink,
    );
}

fn leaf(painter: &egui::Painter, points: [Pos2; 6], ink: Color32) {
    let [base, first, second, tip, third, fourth] = points;
    let curve = |points| {
        CubicBezierShape::from_points_stroke(points, false, Color32::TRANSPARENT, Stroke::NONE)
            .flatten(Some(0.1))
    };
    let mut outline = curve([base, first, second, tip]);
    outline.extend(curve([tip, third, fourth, base]).into_iter().skip(1));
    // The polygon closes itself; duplicate endpoints interrupt edge feathering.
    let _closing_point = outline.pop();
    painter.add(egui::Shape::convex_polygon(outline, ink, Stroke::NONE));
}
