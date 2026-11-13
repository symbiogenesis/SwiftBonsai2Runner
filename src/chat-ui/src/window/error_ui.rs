//! Compact failure presentation; preview limits never affect the copied error.
use super::{RichText, egui};

const PREVIEW_CHARS: usize = 320;
const DETAIL_CHARS: usize = 4_096;

/// Keep arbitrary newlines and long paths from growing a notice or its parent.
pub(super) fn summary(ui: &mut egui::Ui, message: &str) {
    let message = message
        .split("Protected credentials and technical evidence:")
        .next()
        .unwrap_or(message);
    let mut characters = message.chars();
    let mut text: String = characters
        .by_ref()
        .take(PREVIEW_CHARS)
        .map(|character| {
            if character.is_whitespace() {
                ' '
            } else {
                character
            }
        })
        .collect();
    if characters.next().is_some() {
        text.push('…');
    }
    let mut job = egui::text::LayoutJob::simple(
        text,
        egui::TextStyle::Body.resolve(ui.style()),
        ui.visuals().text_color(),
        ui.available_width().max(1.0),
    );
    job.wrap.max_rows = 2;
    job.wrap.break_anywhere = true;
    ui.add(egui::Label::new(job));
}

pub(super) fn copy_button(ui: &mut egui::Ui, label: &str, message: &str) {
    let id = ui.make_persistent_id(("error-copy", label));
    let now = ui.input(|input| input.time);
    let copied = ui
        .ctx()
        .data(|data| data.get_temp::<f64>(id))
        .is_some_and(|until| now < until);
    let text_width = ui
        .painter()
        .layout_no_wrap(
            label.to_owned(),
            egui::TextStyle::Button.resolve(ui.style()),
            ui.visuals().text_color(),
        )
        .size()
        .x;
    let width = ui.spacing().button_padding.x.mul_add(2.0, text_width);
    if ui
        .add(
            egui::Button::new(if copied { "Copied" } else { label })
                .min_size(egui::vec2(width, 0.0)),
        )
        .accessible_name(label)
        .on_hover_text("Copy the complete message, including details beyond the preview.")
        .clicked()
    {
        ui.ctx().copy_text(message.to_owned());
        ui.ctx().data_mut(|data| data.insert_temp(id, now + 1.5));
        ui.ctx().request_repaint();
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(1_500));
    }
}

pub(super) fn details(ui: &mut egui::Ui, message: &str) {
    let mut characters = message.chars();
    let preview: String = characters.by_ref().take(DETAIL_CHARS).collect();
    let shortened = characters.next().is_some();
    egui::ScrollArea::vertical()
        .id_salt("error-details-scroll")
        .max_height(80.0)
        .min_scrolled_height(0.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            super::navigation::bounded_width(ui, |ui| {
                ui.add(
                    egui::Label::new(RichText::new(preview).small().monospace())
                        .selectable(true)
                        .wrap(),
                );
            });
        });
    if shortened {
        ui.add(
            egui::Label::new(
                RichText::new("Preview shortened. Copy includes the full message.")
                    .small()
                    .weak(),
            )
            .truncate(),
        );
    }
}

pub(super) fn card(
    ui: &mut egui::Ui,
    id: &'static str,
    title: &str,
    message: &str,
    error: &str,
    actions: impl FnOnce(&mut egui::Ui),
) {
    ui.push_id(id, |ui| {
        super::navigation::bounded_width(ui, |ui| {
            egui::Frame::new()
                .fill(ui.visuals().faint_bg_color)
                .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
                .corner_radius(10)
                .inner_margin(10)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing = egui::vec2(8.0, 4.0);
                    ui.spacing_mut().button_padding = egui::vec2(8.0, 4.0);
                    ui.spacing_mut().interact_size.y = 28.0;
                    // The whole card has a hard height limit, even at large text
                    // sizes or when its action row needs to wrap.
                    egui::ScrollArea::vertical()
                        .id_salt("error-card-scroll")
                        .max_height(168.0)
                        .min_scrolled_height(0.0)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(title)
                                        .strong()
                                        .color(ui.visuals().warn_fg_color),
                                )
                                .truncate(),
                            );
                            summary(ui, message);
                            ui.horizontal_wrapped(|ui| {
                                copy_button(ui, "Copy error", error);
                                actions(ui);
                            });
                            egui::CollapsingHeader::new("Details")
                                .id_salt("error-details")
                                .show(ui, |ui| details(ui, error));
                        });
                });
        });
    });
}
