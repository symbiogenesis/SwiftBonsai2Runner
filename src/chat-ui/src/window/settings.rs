//! Scoped preferences in one responsive, keyboard-contained native surface.
use super::{OUTPUT_TOKENS, RichText, State, VOICE_LIMIT, appearance, egui};

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

    pub(super) fn thinking_button(&mut self, ui: &mut egui::Ui) {
        let help = match (self.enable_thinking, self.extra_effort) {
            (true, true) => {
                "Thinking and extra effort are on. Change how Swift thinks before replying."
            }
            (true, false) => "Thinking is on. Change how Swift thinks before replying.",
            (false, true) => "Extra effort is on. Change how Swift thinks before replying.",
            (false, false) => {
                "Choose whether Swift thinks before replying or takes extra time to review."
            }
        };
        let response = super::context_capacity::toolbar_button(
            ui,
            "Thinking",
            self.enable_thinking || self.extra_effort,
        )
        .on_hover_text(help);
        let opening = response.clicked()
            && !egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response));
        if opening {
            // Keyboard activation belongs to the choices after opening. If
            // generation disables both, Enter must not toggle the still-focused
            // opener closed and expose Stop to the following Escape.
            response.surrender_focus();
        }
        egui::Popup::menu(&response)
            .accessible_name("Thinking options")
            .align(egui::RectAlign::TOP_START)
            .width(280.0)
            .gap(6.0)
            .frame(
                egui::Frame::popup(ui.style())
                    .corner_radius(14)
                    .inner_margin(14),
            )
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                ui.label(RichText::new("Thinking").strong().size(16.0));
                ui.label(RichText::new("For this conversation").small().weak());
                ui.add_space(10.0);
                ui.separator();
                ui.add_space(10.0);
                self.thinking_preferences(ui, opening);
                if self.model.busy() {
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new("Available when this response finishes.")
                            .small()
                            .weak(),
                    );
                }
            });
    }

    fn thinking_preferences(&mut self, ui: &mut egui::Ui, focus: bool) {
        let changed = ui
            .add_enabled_ui(!self.model.busy(), |ui| {
                ui.spacing_mut().interact_size.y = 28.0;
                let thinking = ui.checkbox(&mut self.enable_thinking, "Thinking");
                if focus && !self.model.busy() {
                    thinking.request_focus();
                }
                let mut changed = thinking.changed();
                ui.label(
                    RichText::new("Reason before answering. Uses part of the reply limit.")
                        .small()
                        .weak(),
                );
                ui.add_space(12.0);
                changed |= ui.checkbox(&mut self.extra_effort, "Extra effort").changed();
                ui.label(
                    RichText::new("Plan, draft and review. Experimental; takes longer and may not improve answers.")
                        .small()
                        .weak(),
                );
                changed
            })
            .inner;
        self.recovery_dirty |= changed;
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
                            self.settings_navigation(ui, false);
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
                                egui::Button::new(
                                    RichText::new("Done")
                                        .color(ui.visuals().selection.stroke.color),
                                )
                                .fill(ui.visuals().selection.bg_fill)
                                .min_size(egui::vec2(72.0, 34.0))
                                .corner_radius(10),
                            )
                            .clicked();
                    });
                });
            });
        if done || modal.should_close() {
            self.close_settings(ctx);
        }
    }

    fn settings_navigation(&mut self, ui: &mut egui::Ui, horizontal: bool) {
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
                let selected = self.settings_view.page == page;
                let text = RichText::new(page.label()).color(if selected {
                    ui.visuals().selection.stroke.color
                } else {
                    ui.visuals().text_color()
                });
                let response = ui.add(
                    egui::Button::selectable(selected, text)
                        .min_size(egui::vec2(button_width, 34.0))
                        .wrap_mode(egui::TextWrapMode::Extend)
                        .corner_radius(10),
                );
                if self.settings_view.focus && selected {
                    response.request_focus();
                    self.settings_view.focus = false;
                }
                if response.clicked() {
                    self.settings_view.page = page;
                }
            }
        });
    }

    fn settings_page(&mut self, ui: &mut egui::Ui, height: f32, horizontal_nav: bool) -> bool {
        let close = settings_header(ui, self.settings_view.page.label());
        if horizontal_nav {
            self.settings_navigation(ui, true);
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

    fn appearance_preferences(&mut self, ui: &mut egui::Ui) {
        let before = self.appearance;
        settings_card(ui, |ui| {
            ui.label(RichText::new("Theme").strong());
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                for theme in appearance::Theme::ALL {
                    let selected = self.appearance.theme == theme;
                    let text = RichText::new(theme.label()).color(if selected {
                        ui.visuals().selection.stroke.color
                    } else {
                        ui.visuals().text_color()
                    });
                    if ui
                        .add(
                            egui::Button::selectable(selected, text)
                                .min_size(egui::vec2(78.0, 36.0))
                                .corner_radius(10),
                        )
                        .clicked()
                    {
                        self.appearance.theme = theme;
                    }
                }
            });
            ui.add_space(8.0);
            ui.label(
                RichText::new("System follows Windows. High contrast always takes priority.")
                    .small()
                    .weak(),
            );
        });
        ui.add_space(8.0);
        settings_card(ui, |ui| {
            ui.label(RichText::new("Reading comfort").strong());
            ui.add_space(12.0);
            ui.label("Text size");
            ui.add(
                egui::Slider::new(&mut self.appearance.text_size, 14..=24)
                    .step_by(1.0)
                    .suffix(" pt")
                    .text("Reading text size"),
            );
            ui.add_space(12.0);
            ui.label("Reading width");
            ui.add(
                egui::Slider::new(&mut self.appearance.reading_width, 640..=1_040)
                    .step_by(40.0)
                    .suffix(" pt")
                    .text("Reading column width"),
            );
            ui.label(RichText::new("The conversation and composer fit the space available. Controls keep their usual size.").small().weak());
            ui.add_space(16.0);
            egui::Frame::new()
                .fill(ui.visuals().extreme_bg_color)
                .corner_radius(10)
                .inner_margin(14)
                .show(ui, |ui| {
                    ui.label(RichText::new("A little space to think.").strong().size(f32::from(self.appearance.text_size)));
                    ui.add_space(8.0);
                    ui.label(RichText::new("Follow an idea at your own pace. Find the words, take a breath, and leave room for what comes next.").size(f32::from(self.appearance.text_size)));
                });
        });
        ui.add_space(12.0);
        ui.label(
            RichText::new(
                "Appearance is saved on this device, including when conversation saving is off.",
            )
            .small()
            .weak(),
        );
        if ui.button("Reset appearance").clicked() {
            let defaults = appearance::Preferences::default();
            self.appearance.theme = defaults.theme;
            self.appearance.text_size = defaults.text_size;
            self.appearance.reading_width = defaults.reading_width;
        }
        if self.appearance != before {
            self.appearance_changed(ui.ctx());
        }
    }

    fn response_preferences(&mut self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new("Changes affect the next response and every Extra effort pass.")
                .small()
                .weak(),
        );
        if self.model.busy() {
            ui.add_space(8.0);
            ui.label("Response controls unlock when the current response finishes.");
        }
        ui.add_space(8.0);
        settings_card(ui, |ui| {
            self.thinking_preferences(ui, false);
        });
        ui.add_space(8.0);
        settings_card(ui, |ui| {
            egui::CollapsingHeader::new("Advanced response controls")
                .id_salt("settings-advanced-response")
                .show(ui, |ui| {
                    ui.add_space(8.0);
                    self.reply_preferences(ui);
                    ui.add_space(12.0);
                    ui.separator();
                    self.adapter_preferences(ui);
                });
        });
    }

    fn reply_preferences(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Reply length").strong());
        ui.add_space(8.0);
        let before = self.output_budget;
        ui.add_enabled_ui(!self.model.busy(), |ui| {
            ui.horizontal(|ui| {
                let label = ui.label("Output cap");
                egui::ComboBox::from_id_salt("output-cap")
                    .selected_text(format!(
                        "{} tokens",
                        OUTPUT_TOKENS
                            .get(self.output_budget)
                            .copied()
                            .unwrap_or(4096)
                    ))
                    .show_ui(ui, |ui| {
                        for (index, tokens) in OUTPUT_TOKENS.iter().enumerate() {
                            ui.selectable_value(
                                &mut self.output_budget,
                                index,
                                format!("{tokens} tokens"),
                            );
                        }
                    })
                    .response
                    .labelled_by(label.id);
            });
        });
        self.recovery_dirty |= before != self.output_budget;
        ui.add_space(8.0);
        ui.label(RichText::new("8,192-token context. The output cap includes thinking. A smaller cap leaves more room for your conversation.").small().weak());
    }

    fn adapter_preferences(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.label(RichText::new("Adapter strength").strong());
        ui.label(RichText::new("Abliteration changes the pinned adapter’s contribution to the response. It is an experimental model adjustment; higher values do not establish better quality.").small().weak());
        ui.add_space(8.0);
        let changed = ui
            .add_enabled_ui(!self.model.busy() && self.abliteration_enabled, |ui| {
                ui.add(
                    egui::Slider::new(&mut self.abliteration_scale, 0.0..=2.0)
                        .step_by(0.1)
                        .fixed_decimals(1)
                        .text("Abliteration scale"),
                )
                .changed()
            })
            .inner;
        self.recovery_dirty |= changed;
        ui.label(
            RichText::new(if self.abliteration_enabled {
                "0 disables the correction. Default: 1.3. Applies to your next response."
            } else {
                "Disabled by the selected launch options."
            })
            .small()
            .weak(),
        );
    }

    fn voice_preferences(&mut self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new("One voice profile for read aloud across your conversations. Saved on this device with Windows user encryption, even when conversation saving is off.")
                .small()
                .weak(),
        );
        ui.add_space(12.0);
        settings_card(ui, |ui| {
            let label = ui.label(RichText::new("Voice profile").strong());
            ui.add_space(8.0);
            self.recovery_dirty |= ui
                .add_enabled(
                    !self.speaking,
                    egui::TextEdit::multiline(&mut self.voice_profile)
                        .desired_rows(4)
                        .desired_width(f32::INFINITY)
                        .char_limit(VOICE_LIMIT),
                )
                .labelled_by(label.id)
                .changed();
            ui.label(
                RichText::new(
                    "Describe the voice you want to hear. Applies to your next read aloud.",
                )
                .small()
                .weak(),
            );
            if self.speaking {
                ui.label("Finish or stop speech before changing the voice profile.");
            }
        });
        ui.add_space(8.0);
        settings_card(ui, |ui| {
            ui.label(
                RichText::new(if self.speech_output {
                    "Read aloud is available"
                } else {
                    "Read aloud becomes available after speech preparation."
                })
                .strong(),
            );
            if ui.button("Read aloud setup").clicked() {
                self.settings_view.voice_help = !self.settings_view.voice_help;
            }
            if self.settings_view.voice_help {
                ui.add_space(8.0);
                ui.label(self.read_aloud_help());
            }
        });
        if let Some(error) = self.setup.speech_error.clone() {
            ui.add_space(8.0);
            settings_card(ui, |ui| {
                ui.label(RichText::new("Speech setup needs attention").strong());
                super::error_ui::summary(ui, &error);
                ui.horizontal_wrapped(|ui| {
                    super::error_ui::copy_button(ui, "Copy error", &error);
                    self.speech_setup_error_actions(ui, &error);
                });
                ui.collapsing("Details", |ui| super::error_ui::details(ui, &error));
            });
        }
    }

    fn application_preferences(&mut self, ui: &mut egui::Ui) {
        settings_card(ui, |ui| self.update_controls(ui));
        ui.add_space(8.0);
        settings_card(ui, |ui| self.runtime_controls(ui));
        ui.add_space(8.0);
        settings_card(ui, |ui| {
            egui::CollapsingHeader::new("Diagnostics")
                .id_salt("settings-diagnostics")
                .show(ui, |ui| self.diagnostics_contents(ui));
        });
    }

    pub(super) fn diagnostics_contents(&self, ui: &mut egui::Ui) {
        ui.label("Recent application messages");
        ui.label(
            RichText::new("Kept in memory for this session. Review paths before sharing.")
                .small()
                .weak(),
        );
        super::error_ui::copy_button(
            ui,
            "Copy diagnostics",
            &self
                .diagnostics
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join("\n\n"),
        );
        ui.label(
            RichText::new("Recent previews only. Use Copy error on a notice for its full message.")
                .small()
                .weak(),
        );
        egui::ScrollArea::vertical()
            .id_salt("diagnostic-messages")
            .max_height(240.0)
            .min_scrolled_height(0.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                super::navigation::bounded_width(ui, |ui| {
                    for message in &self.diagnostics {
                        ui.add(egui::Label::new(message).selectable(true).wrap());
                        ui.separator();
                    }
                });
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
                    egui::Button::new(RichText::new("×").size(22.0))
                        .min_size(egui::vec2(34.0, 34.0))
                        .frame_when_inactive(false)
                        .corner_radius(10),
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
