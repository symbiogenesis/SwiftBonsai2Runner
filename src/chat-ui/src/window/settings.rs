//! Scoped preferences in one responsive, keyboard-contained native surface.
use super::{RichText, State, controls, egui};

mod appearance;
mod response;
mod services;

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub(super) enum Page {
    #[default]
    Response,
    Appearance,
    Voice,
    Privacy,
    Application,
}

impl Page {
    const ALL: [Self; 5] = [
        Self::Response,
        Self::Appearance,
        Self::Voice,
        Self::Privacy,
        Self::Application,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::Response => "Response",
            Self::Appearance => "Appearance",
            Self::Voice => "Voice",
            Self::Privacy => "Privacy",
            Self::Application => "Application",
        }
    }
}

#[derive(Default)]
pub(super) struct View {
    page: Page,
    focus: bool,
    voice_help: bool,
}

impl View {
    fn navigation(&mut self, ui: &mut egui::Ui, horizontal: bool) {
        let button_width = if horizontal {
            0.0
        } else {
            ui.available_width()
        };
        let layout = if horizontal {
            egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true)
        } else {
            egui::Layout::top_down(egui::Align::Min)
        };
        // A horizontal navigation row needs a bounded cross-axis; with_layout
        // would center its controls in the modal's entire remaining height.
        ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 34.0), layout, |ui| {
            for page in Page::ALL {
                if !horizontal && matches!(page, Page::Response | Page::Appearance) {
                    if page == Page::Appearance {
                        ui.add_space(12.0);
                    }
                    ui.label(
                        RichText::new(if page == Page::Response {
                            "Conversation"
                        } else {
                            "On this device"
                        })
                        .small()
                        .weak(),
                    );
                }
                let selected = self.page == page;
                let response = ui.add(
                    controls::choice_button(ui, page.label(), selected)
                        .min_size(egui::vec2(button_width, 34.0))
                        .wrap_mode(egui::TextWrapMode::Extend),
                );
                if self.focus && selected {
                    response.request_focus();
                    self.focus = false;
                }
                if response.clicked() {
                    self.page = page;
                }
            }
        });
    }
}

impl State {
    pub(super) const fn open_settings(&mut self, page: Page) {
        self.settings_view.page = page;
        self.settings_view.focus = true;
        self.show_settings = true;
        self.sidebar_open = false;
        self.focus_composer = false;
    }

    pub(super) const fn open_voice_setup(&mut self) {
        self.open_settings(Page::Voice);
        self.settings_view.voice_help = true;
    }

    fn close_settings(&mut self, ctx: &egui::Context) {
        self.show_settings = false;
        self.focus_composer = true;
        ctx.request_repaint();
    }

    pub(super) fn settings(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }
        let content = ctx.content_rect();
        let margin: i8 = 16;
        let width = (f32::from(margin).mul_add(-2.0, content.width()) - 32.0).clamp(240.0, 800.0);
        let wide = width >= 700.0 && content.height() >= 560.0;
        // Reserve the outer inset and footer; the page subtracts its actual
        // heading, scope and (when narrow) wrapping navigation before scrolling.
        let page_height = f32::from(margin).mul_add(-2.0, content.height()) - 88.0;
        let visuals = ctx.global_style().visuals.clone();
        let frame = egui::Frame::new()
            .fill(visuals.window_fill)
            .stroke(visuals.window_stroke)
            .corner_radius(20)
            .inner_margin(margin)
            .shadow(visuals.window_shadow);
        let mut done = false;
        let modal = egui::Modal::new(egui::Id::unique("settings-modal"))
            .frame(frame)
            .backdrop_color(egui::Color32::from_black_alpha(150))
            .show(ctx, |ui| {
                ui.set_width(width);
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                ui.spacing_mut().item_spacing.y = 6.0;
                ui.spacing_mut().button_padding = egui::vec2(10.0, 6.0);
                if wide {
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 16.0;
                        ui.vertical(|ui| {
                            ui.set_width(144.0);
                            self.settings_view.navigation(ui, false);
                        });
                        ui.vertical(|ui| {
                            ui.set_width(ui.available_width());
                            done |= self.settings_page(ui, page_height, false);
                        });
                    });
                } else {
                    done |= self.settings_page(ui, page_height, true);
                }
                ui.add_space(8.0);
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Changes apply immediately.").small().weak());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        done |= ui
                            .add(
                                controls::primary_button(ui, "Done")
                                    .min_size(egui::vec2(72.0, 34.0)),
                            )
                            .clicked();
                    });
                });
            });
        if done || modal.should_close() {
            self.close_settings(ctx);
        }
    }

    fn settings_page(&mut self, ui: &mut egui::Ui, height: f32, horizontal_nav: bool) -> bool {
        let close = settings_header(ui, self.settings_view.page.label());
        if horizontal_nav {
            self.settings_view.navigation(ui, true);
        }
        self.settings_scope(ui);
        ui.add_space(4.0);
        let body_height = (height - ui.min_rect().height()).clamp(60.0, 380.0);
        self.settings_content(ui, body_height);
        close
    }

    fn settings_scope(&self, ui: &mut egui::Ui) {
        let page = self.settings_view.page;
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(if page == Page::Response {
                    "This conversation"
                } else {
                    "All conversations · This device"
                })
                .small()
                .weak(),
            );
            if page == Page::Response
                && let Some(conversation) = self
                    .conversations
                    .iter()
                    .find(|conversation| conversation.id == self.active_conversation)
            {
                ui.label(RichText::new("·").weak());
                ui.add(
                    egui::Label::new(RichText::new(&conversation.title).small().weak()).truncate(),
                )
                .on_hover_text(&conversation.title);
            }
        });
    }

    fn settings_content(&mut self, ui: &mut egui::Ui, height: f32) {
        let page = self.settings_view.page;
        egui::ScrollArea::vertical()
            .id_salt(("settings-content", page))
            .min_scrolled_height(height)
            .max_height(height)
            .auto_shrink([false, false])
            .show(ui, |ui| match page {
                Page::Response => self.response_preferences(ui),
                Page::Appearance => self.appearance_preferences(ui),
                Page::Voice => self.voice_preferences(ui),
                Page::Privacy => settings_card(ui, |ui| self.recovery_controls(ui)),
                Page::Application => self.application_preferences(ui),
            });
    }
}

fn settings_header(ui: &mut egui::Ui, title: &str) -> bool {
    let mut close = false;
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).strong().size(18.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            close = ui
                .add(
                    controls::quiet_button(RichText::new("×").size(22.0))
                        .min_size(egui::vec2(34.0, 34.0)),
                )
                .accessible_name("Close settings")
                .on_hover_text("Close settings · Escape")
                .clicked();
        });
    });
    close
}

fn settings_card(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(ui.visuals().panel_fill)
        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
        .corner_radius(12)
        .inner_margin(12)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            contents(ui);
        });
}
