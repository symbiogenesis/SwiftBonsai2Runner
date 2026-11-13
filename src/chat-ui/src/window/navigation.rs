//! Responsive native navigation and device-local appearance integration.
use super::{State, appearance, egui};

mod controls;

pub(super) const DOCK_THRESHOLD: f32 = 900.0;
const SIDEBAR_MARGIN: egui::Margin = egui::Margin {
    left: 16,
    right: 16,
    top: 16,
    bottom: 8,
};

pub(super) fn bounded_width(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui)) {
    // Keep unwrapped labels from increasing their parent's measured width.
    let rect = ui.available_rect_before_wrap();
    let mut content = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    content.shrink_clip_rect(rect);
    contents(&mut content);
    let used = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), content.min_size().y));
    ui.allocate_rect(used, egui::Sense::hover());
}

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
        let response = controls::navigation_toggle(ui, label);
        if response.clicked() {
            self.toggle_navigation(ui.ctx());
        }
    }

    pub(super) fn docked_navigation(&mut self, ui: &mut egui::Ui, narrow: bool) {
        // A drawer never survives a transition back to the docked layout.
        if !narrow {
            self.sidebar_open = false;
            self.sidebar_focus = false;
        }
        if narrow || !self.appearance.sidebar_visible {
            egui::Panel::left("navigation-rail")
                .exact_size(44.0)
                .resizable(false)
                .frame(
                    egui::Frame::new()
                        .fill(ui.visuals().window_fill)
                        .inner_margin(egui::Margin::symmetric(4, 12)),
                )
                .show(ui, |ui| self.navigation_button(ui, narrow));
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
            .show(ui, |ui| bounded_width(ui, |ui| self.sidebar(ui)));
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
                let response = controls::drawer_header(ui);
                if self.sidebar_focus {
                    response.request_focus();
                    self.sidebar_focus = false;
                }
                close = response.clicked();
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
