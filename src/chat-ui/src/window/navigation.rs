//! Responsive native navigation and device-local appearance integration.
use super::{RichText, State, appearance, egui};

pub(super) const DOCK_THRESHOLD: f32 = 900.0;
const SIDEBAR_MARGIN: egui::Margin = egui::Margin {
    left: 16,
    right: 16,
    top: 16,
    bottom: 8,
};

impl State {
    pub(super) fn attach_appearance(
        &mut self,
        opened: std::io::Result<(appearance::Store, appearance::Preferences)>,
        ctx: &egui::Context,
    ) {
        match opened {
            Ok((store, preferences)) => {
                self.appearance = preferences;
                self.appearance_store = Some(store);
            }
            Err(_) => self
                .record("Could not open appearance preferences. Using defaults for this window."),
        }
        self.appearance.apply(ctx);
    }

    pub(super) fn appearance_changed(&mut self, ctx: &egui::Context) {
        self.appearance.apply(ctx);
        self.poll_appearance(ctx);
        ctx.request_repaint();
    }

    pub(super) fn poll_appearance(&mut self, ctx: &egui::Context) {
        let Some(store) = &mut self.appearance_store else {
            return;
        };
        let failed = store.take_error();
        let queue_failed =
            !ctx.input(|input| input.pointer.any_down()) && store.queue(self.appearance).is_err();
        if failed || queue_failed {
            self.record(
                "Could not save appearance preferences. Your conversations are saved separately.",
            );
        }
    }

    pub(super) fn toggle_navigation(&mut self, ctx: &egui::Context) {
        if ctx.content_rect().width() < DOCK_THRESHOLD {
            self.sidebar_open = !self.sidebar_open;
            self.sidebar_focus = self.sidebar_open;
            self.focus_composer = !self.sidebar_open;
        } else {
            self.sidebar_open = false;
            self.appearance.sidebar_visible = !self.appearance.sidebar_visible;
            self.appearance_changed(ctx);
        }
        ctx.request_repaint();
    }

    pub(super) fn navigation_button(&mut self, ui: &mut egui::Ui, narrow: bool) {
        let label = if narrow {
            "Conversation menu"
        } else if self.appearance.sidebar_visible {
            "Hide conversations"
        } else {
            "Show conversations"
        };
        let response = ui
            .add(
                egui::Button::new("")
                    .min_size(egui::Vec2::splat(36.0))
                    .frame_when_inactive(false)
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
        if response.clicked() {
            self.toggle_navigation(ui.ctx());
        }
    }

    pub(super) fn docked_navigation(&mut self, ui: &mut egui::Ui, narrow: bool) {
        if narrow {
            return;
        }
        // A drawer never survives a transition back to the docked layout.
        self.sidebar_open = false;
        self.sidebar_focus = false;
        if !self.appearance.sidebar_visible {
            return;
        }
        let response = egui::Panel::left("navigation")
            .default_size(self.appearance.sidebar_width)
            .size_range(216.0..=340.0)
            .resizable(true)
            .frame(
                egui::Frame::new()
                    .fill(ui.visuals().window_fill)
                    .inner_margin(SIDEBAR_MARGIN),
            )
            .show(ui, |ui| self.sidebar(ui));
        self.appearance.sidebar_width = response.response.rect.width().clamp(216.0, 340.0);
    }

    pub(super) fn navigation_drawer(&mut self, ctx: &egui::Context) {
        if !self.sidebar_open || ctx.content_rect().width() >= DOCK_THRESHOLD {
            return;
        }
        if self.rename_conversation.is_some()
            || self.delete_conversation.is_some()
            || self.edit_message.is_some()
        {
            self.sidebar_open = false;
            return;
        }
        let content = ctx.content_rect();
        let width = (self.appearance.sidebar_width + 24.0).min(content.width() - 56.0);
        let visuals = ctx.global_style().visuals.clone();
        let frame = egui::Frame::new()
            .fill(visuals.window_fill)
            .stroke(visuals.window_stroke)
            .inner_margin(SIDEBAR_MARGIN)
            .corner_radius(egui::CornerRadius {
                nw: 0,
                ne: 16,
                sw: 0,
                se: 16,
            });
        let vertical_margin = frame.total_margin().sum().y;
        let mut close = false;
        let modal = egui::Modal::new(egui::Id::unique("conversation-drawer"))
            .area(
                egui::Area::new(egui::Id::unique("conversation-drawer"))
                    .kind(egui::UiKind::Modal)
                    .order(egui::Order::Foreground)
                    .sense(egui::Sense::hover())
                    .fixed_pos(content.left_top())
                    .movable(false),
            )
            .frame(frame)
            .backdrop_color(egui::Color32::from_black_alpha(100))
            .show(ctx, |ui| {
                ui.set_width(width - 32.0);
                ui.set_height((content.height() - vertical_margin).max(200.0));
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Conversations").strong().size(18.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let response = ui
                            .add(
                                egui::Button::new("×")
                                    .min_size(egui::Vec2::splat(36.0))
                                    .frame_when_inactive(false),
                            )
                            .accessible_name("Close conversations")
                            .on_hover_text("Close conversations · Escape");
                        if self.sidebar_focus {
                            response.request_focus();
                            self.sidebar_focus = false;
                        }
                        close = response.clicked();
                    });
                });
                ui.add_space(8.0);
                self.sidebar(ui);
            });
        if close || modal.should_close() {
            self.sidebar_open = false;
            self.focus_composer = true;
        }
        if self.rename_conversation.is_some() || self.delete_conversation.is_some() {
            self.sidebar_open = false;
        }
    }
}
