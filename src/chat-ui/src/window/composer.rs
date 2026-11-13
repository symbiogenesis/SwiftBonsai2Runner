//! Composer layout, draft editing and action dispatch.
use super::{INPUT_LIMIT, RichText, State, appearance, controls, draft_buffer, egui, error_ui};

mod microphone;

impl State {
    fn microphone_button(&mut self, ui: &mut egui::Ui) {
        let phase = if self.dictation_finishing {
            microphone::Phase::Finishing
        } else if self.dictation_requested {
            microphone::Phase::Recording
        } else {
            microphone::Phase::Idle
        };
        let enabled = self.dictation_requested || (!self.model.busy() && !self.speaking);
        if microphone::button(ui, phase, enabled).clicked() {
            self.toggle_dictation();
        }
    }
    fn composer_send_controls(&mut self, ui: &mut egui::Ui) {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if self.model.busy() {
                if ui.button("Stop").clicked() {
                    self.stop_generation();
                }
            } else if ui
                .add_enabled(
                    self.model.ready() && !self.input.trim().is_empty(),
                    controls::primary_button(ui, "Send  ↑"),
                )
                .on_hover_text("Send message · Enter")
                .clicked()
            {
                self.submit();
            }
            self.microphone_button(ui);
        });
    }

    pub(super) fn composer(&mut self, ui: &mut egui::Ui) {
        let width = ui.available_width().min(self.appearance.column_width());
        let margin = ((ui.available_width() - width) * 0.5).max(0.0);
        ui.horizontal(|ui| {
            ui.add_space(margin);
            ui.vertical(|ui| {
                ui.set_width(width);
                if !self.audio_file_error {
                    self.audio_file_status(ui);
                }
                egui::Frame::new()
                    .fill(ui.visuals().faint_bg_color)
                    .corner_radius(14)
                    .inner_margin(14)
                    .show(ui, |ui| {
                        let label = self.context_toolbar(ui);
                        let response = self.composer_editor(ui, label.id);
                        self.composer_enter(ui, &response);
                        self.composer_actions(ui);
                    });
                self.composer_feedback(ui);
            });
        });
    }
    fn composer_editor(&mut self, ui: &mut egui::Ui, label: egui::Id) -> egui::Response {
        // A pasted manuscript must not push the transcript and
        // Send/Stop controls outside the window. TextEdit keeps
        // the caret visible within this native scroll area.
        let editor = egui::ScrollArea::vertical()
            .id_salt(("composer-scroll", self.active_conversation))
            .max_height((ui.ctx().content_rect().height() * 0.24).clamp(64.0, 160.0))
            .show(ui, |ui| {
                let output = ui.scope(|ui| self.composer_text_edit(ui)).inner;
                self.track_composer_editor(ui, output, label)
            });
        #[cfg(test)]
        ui.ctx().data_mut(|data| {
            data.insert_temp(
                egui::Id::unique("composer-viewport"),
                (
                    editor.inner.1,
                    editor.inner_rect,
                    editor.state.clamped_offset(),
                ),
            );
        });
        editor.inner.0
    }

    fn composer_text_edit(&mut self, ui: &mut egui::Ui) -> egui::text_edit::TextEditOutput {
        let hint = self.composer_hint();
        let id = ui.make_persistent_id("composer");
        let mut buffer = draft_buffer::Buffer::new(&mut self.input, ui.ctx(), id, INPUT_LIMIT);
        let mut output = egui::TextEdit::multiline(&mut buffer)
            .font(egui::FontId::proportional(appearance::text_size(ui.ctx())))
            .id(id)
            .hint_text(hint)
            .desired_rows(3)
            .desired_width(f32::INFINITY)
            .frame(egui::Frame::NONE)
            .return_key(Some(egui::KeyboardShortcut::new(
                egui::Modifiers::SHIFT,
                egui::Key::Enter,
            )))
            .show(ui);
        if buffer.finish(&mut output) {
            self.set_status(draft_buffer::LIMIT_MESSAGE, true);
        }
        output
    }

    const fn composer_hint(&self) -> &'static str {
        if self.model.busy() {
            "Draft your next message…"
        } else if self.model.ready() {
            "Ask anything…"
        } else if self.model.retryable() {
            "Retry the model to continue…"
        } else if self.model.stopping() {
            "Write while the model stops…"
        } else if self.model.waiting_model() {
            "Write while your local model prepares…"
        } else {
            "Model unavailable"
        }
    }

    fn track_composer_editor(
        &mut self,
        ui: &egui::Ui,
        output: egui::text_edit::TextEditOutput,
        label: egui::Id,
    ) -> (egui::Response, Option<egui::Rect>) {
        let response = output.response.response.labelled_by(label);
        self.recovery_dirty |= response.changed();
        self.composer_id = Some(response.id);
        let pending_cursor = self.pending_cursor.take();
        let cursor = pending_cursor.or(output.cursor_range);
        if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), response.id) {
            if let Some(cursor) = pending_cursor {
                state.cursor.set_char_range(Some(cursor));
                state.store(ui.ctx(), response.id);
                ui.ctx().request_repaint();
            } else {
                self.composer_cursor = state.cursor.char_range();
            }
        }
        let focus_requested = self.focus_composer
            && !self.show_settings
            && !self.show_audio_transcript
            && self.alerts.details.is_none();
        if focus_requested {
            response.request_focus();
            self.focus_composer = false;
        }
        let caret = cursor.map(|cursor| {
            output
                .galley
                .pos_from_cursor(cursor.primary)
                .translate(output.galley_pos.to_vec2())
        });
        // Recovered drafts can already have their cursor at the end, so egui
        // sees no cursor change and does not request its usual automatic scroll.
        if (response.has_focus() || focus_requested)
            && (response.gained_focus()
                || focus_requested
                || pending_cursor.is_some()
                || ui.input(|input| {
                    input.key_pressed(egui::Key::End) || input.key_pressed(egui::Key::Home)
                }))
            && let Some(caret) = caret
        {
            ui.scroll_to_rect(caret, None);
        }
        (response, caret)
    }

    fn composer_enter(&mut self, ui: &egui::Ui, response: &egui::Response) {
        let ime_event = ui.input(|input| {
            input
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::Ime(_)))
        });
        if self.rename_conversation.is_none()
            && self.delete_conversation.is_none()
            && self.edit_message.is_none()
            && !egui::Popup::is_any_open(ui.ctx())
            && !self.show_settings
            && !self.show_audio_transcript
            && self.alerts.details.is_none()
            && ui.is_enabled()
            && response.has_focus()
            && !self.composing
            && !ime_event
            && ui.input_mut(|input| {
                let plain_enter = input.events.iter().any(|event| {
                    matches!(event, egui::Event::Key {
                        key: egui::Key::Enter,
                        pressed: true,
                        modifiers,
                        ..
                    } if *modifiers == egui::Modifiers::NONE)
                });
                plain_enter && input.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
            })
        {
            self.submit();
        }
    }

    fn composer_actions(&mut self, ui: &mut egui::Ui) {
        // Keep the send group out of a narrow remainder beside
        // the audio controls, including with the collapsed rail.
        let separate_send_row = ui.available_width() < 420.0;
        ui.horizontal_wrapped(|ui| {
            self.audio_file_button(ui);
            self.read_aloud_controls(ui);
            if !separate_send_row {
                self.composer_send_controls(ui);
            }
        });
        if separate_send_row {
            self.composer_send_controls(ui);
        }
    }

    fn composer_feedback(&self, ui: &mut egui::Ui) {
        // Text-only status should grow with its contents, without
        // reserving the horizontal helper's button-height row.
        if (self.dictation_requested || self.speaking)
            && (self.audio_file_notice.is_none() || !self.audio_file_active)
            && !self.speech_notice
            && self.help.is_none()
        {
            error_ui::summary(ui, &self.speech_status);
        }
        // Keyboard guidance stays visually separate from audio feedback.
        if self.model.busy() || self.model.ready() || !self.warning && self.setup.error.is_none() {
            let status: std::borrow::Cow<'_, str> = if self.model.busy() {
                self.generation_status().into()
            } else if self.model.ready() {
                "Enter to send · Shift+Enter for a new line".into()
            } else {
                self.status.as_str().into()
            };
            let preview: String = status.chars().take(320).collect();
            ui.add(egui::Label::new(RichText::new(preview).small().weak()).truncate());
        }
    }
}
