//! Stateless conversation controls render borrowed values and emit intent.
use super::Conversation;
use crate::window::{RichText, controls, egui, history_search};

pub(super) const ROW_HEIGHT: f32 = 40.0;

pub(super) enum Action {
    Select(u64),
    Rename(u64),
    Copy(u64),
    Delete(u64),
}

pub(super) struct Availability {
    pub(super) select: bool,
    pub(super) rename: bool,
    pub(super) delete: bool,
}

pub(super) struct Row<'a> {
    pub(super) conversation: &'a Conversation,
    pub(super) active: bool,
    pub(super) reveal: bool,
    pub(super) copy_available: bool,
    pub(super) availability: Availability,
}

impl Row<'_> {
    pub(super) fn show(&self, ui: &mut egui::Ui, width: f32) -> Option<Action> {
        // Stable identities survive row removal and duplicate titles.
        ui.push_id(self.conversation.id, |ui| self.contents(ui, width))
            .inner
    }

    fn contents(&self, ui: &mut egui::Ui, width: f32) -> Option<Action> {
        let mut action = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let title = RichText::new(&self.conversation.title).color(if self.active {
                ui.visuals().selection.stroke.color
            } else {
                ui.visuals().text_color()
            });
            let response = ui
                .add_enabled_ui(self.availability.select, |ui| {
                    ui.add_sized(
                        [(width - 38.0).max(40.0), ROW_HEIGHT],
                        egui::Button::selectable(self.active, title)
                            .right_text(())
                            .truncate()
                            .corner_radius(8),
                    )
                })
                .inner;
            // Pending composer focus during generation must not repaint forever.
            if self.active && self.reveal {
                response.scroll_to_me(Some(egui::Align::Center));
            }
            if response.on_hover_text(&self.conversation.title).clicked() {
                action = Some(Action::Select(self.conversation.id));
            }
            let (response, _) = egui::containers::menu::MenuButton::from_button(
                controls::quiet_button("⋯").min_size(egui::vec2(34.0, ROW_HEIGHT)),
            )
            .ui(ui, |ui| {
                action = self.menu(ui);
            });
            let label = format!("Actions for {}", self.conversation.title);
            response.accessible_name(&label).on_hover_text(label);
        });
        action
    }

    fn menu(&self, ui: &mut egui::Ui) -> Option<Action> {
        ui.set_min_width(156.0);
        let action = if ui
            .add_enabled(self.availability.rename, egui::Button::new("Rename"))
            .clicked()
        {
            Some(Action::Rename(self.conversation.id))
        } else if ui
            .add_enabled(self.copy_available, egui::Button::new("Copy conversation"))
            .clicked()
        {
            Some(Action::Copy(self.conversation.id))
        } else {
            ui.separator();
            ui.add_enabled(
                self.availability.delete,
                egui::Button::new(RichText::new("Delete").color(ui.visuals().error_fg_color)),
            )
            .clicked()
            .then_some(Action::Delete(self.conversation.id))
        };
        if action.is_some() {
            ui.close();
        }
        action
    }
}

pub(super) fn search_field(
    ui: &mut egui::Ui,
    search: &mut history_search::Search,
) -> egui::Response {
    egui::Frame::new()
        .fill(ui.visuals().extreme_bg_color)
        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
        .corner_radius(8)
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.horizontal(|ui| {
                search_icon(ui);
                let clear_width = if search.text.is_empty() { 0.0 } else { 30.0 };
                let field = ui
                    .add(
                        egui::TextEdit::singleline(&mut search.text)
                            .id(history_search::field_id())
                            .event_filter(egui::EventFilter {
                                horizontal_arrows: true,
                                vertical_arrows: true,
                                escape: true,
                                ..Default::default()
                            })
                            .align(egui::Align2::LEFT_CENTER)
                            .desired_width((ui.available_width() - clear_width).max(24.0))
                            .frame(egui::Frame::NONE)
                            .return_key(None),
                    )
                    .accessible_name("Search conversations");
                if !search.text.is_empty()
                    && ui
                        .add(controls::quiet_button("×").min_size(egui::Vec2::splat(24.0)))
                        .accessible_name("Clear conversation search")
                        .on_hover_text("Clear search · Escape")
                        .clicked()
                {
                    search.clear();
                    field.request_focus();
                }
                field
            })
            .inner
        })
        .inner
}

fn search_icon(ui: &mut egui::Ui) {
    let (icon, _) = ui.allocate_exact_size(egui::vec2(16.0, 24.0), egui::Sense::hover());
    let center = icon.center();
    let stroke = egui::Stroke::new(1.5, ui.visuals().weak_text_color());
    ui.painter()
        .circle_stroke(center - egui::vec2(2.0, 2.0), 4.5, stroke);
    ui.painter().line_segment(
        [center + egui::vec2(1.5, 1.5), center + egui::vec2(6.0, 6.0)],
        stroke,
    );
}

pub(super) fn list_heading(ui: &mut egui::Ui, shown: usize, total: usize, filtered: bool) {
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("CONVERSATIONS").small().strong().weak());
        ui.label(
            RichText::new(if filtered {
                format!("{shown} / {total}")
            } else {
                total.to_string()
            })
            .small()
            .weak(),
        );
    });
    ui.add_space(2.0);
}

pub(super) fn empty_list(ui: &mut egui::Ui) {
    ui.add_space(12.0);
    ui.label(RichText::new("No conversations found").strong());
    ui.label(
        RichText::new("Try another word from a title or message.")
            .small()
            .weak(),
    );
}
