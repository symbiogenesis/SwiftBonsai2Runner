//! Native audio attachments reuse the microphone transcription and prompt path.

use super::{State, egui, speech};
use crate::audio_import::{self, Source};
use raw_window_handle::{HasWindowHandle as _, RawWindowHandle};
use std::time::Duration;

pub(super) fn action_button(label: &str) -> egui::Button<'_> {
    egui::Button::new(label)
        .min_size(egui::vec2(0.0, 36.0))
        .corner_radius(8)
}

pub(super) fn notice_frame(ui: &egui::Ui) -> egui::Frame {
    egui::Frame::new()
        .fill(ui.visuals().faint_bg_color)
        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
        .corner_radius(12)
        .inner_margin(egui::Margin::symmetric(14, 12))
}

pub(super) fn dialog_frame(ctx: &egui::Context) -> egui::Frame {
    let visuals = &ctx.global_style().visuals;
    egui::Frame::new()
        .fill(visuals.window_fill)
        .stroke(visuals.window_stroke)
        .shadow(visuals.window_shadow)
        .corner_radius(16)
        .inner_margin(20)
}

impl State {
    pub(super) fn prepare_audio_paste(&mut self, frame: &eframe::Frame) {
        if self.preview || self.native_owner.is_some() {
            return;
        }
        let Ok(handle) = frame.window_handle() else {
            return;
        };
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return;
        };
        let owner = handle.hwnd.get();
        self.native_owner = Some(owner);
        match audio_import::PasteHook::new(owner) {
            Ok(hook) => self.audio_paste = Some(hook),
            Err(_) => {
                self.record(
                    "Audio file paste is unavailable. Use the plus button to select audio.",
                );
            }
        }
    }

    pub(super) fn handle_audio_paste(&mut self, ctx: &egui::Context) {
        if !self
            .audio_paste
            .as_ref()
            .is_some_and(audio_import::PasteHook::poll)
            || self.composing
            || self.rename_conversation.is_some()
            || self.delete_conversation.is_some()
            || self.show_settings
            || self.show_audio_transcript
            || self.alerts.details.is_some()
            || self.scene_packets.is_open()
            || self.edit_message.is_some()
            || self.help.is_some()
            || !ctx.memory(|memory| {
                memory
                    .focused()
                    .is_some_and(|id| Some(id) == self.composer_id)
            })
        {
            return;
        }
        // Explorer can advertise text alongside its file list. Only an explicit
        // native file-paste gesture suppresses that text; ordinary paste is egui's.
        ctx.input_mut(|input| {
            input
                .events
                .retain(|event| !matches!(event, egui::Event::Paste(_)));
        });
        self.start_audio_import(Source::Clipboard);
    }

    fn start_audio_import(&mut self, source: Source) {
        // A repeated paste must never replace live progress or a retained result.
        if self.model.busy()
            || self.speaking
            || self.dictation_requested
            || self.audio_import.is_some()
        {
            return;
        }
        self.speech_notice = true;
        if self.preview {
            "Audio files are unavailable in the model-free preview."
                .clone_into(&mut self.speech_status);
        } else if !self.audio_file_input {
            if self.speech_checked || !self.canary_configured {
                let message = "Launch bonsai-tools chat with its default speech settings to prepare Qwen3-ASR 1.7B. Then select an audio file with the plus button or paste a file copied from Explorer. Microphone permission is not required.";
                message.clone_into(&mut self.speech_status);
                self.help = Some(("Audio file setup", message));
            } else {
                "Audio files will be available after speech preparation completes."
                    .clone_into(&mut self.speech_status);
            }
        } else {
            if self.dictation_requested || self.audio_import.is_some() {
                self.cancel_microphone();
            }
            if let Ok(import) = audio_import::Session::start(source) {
                // Disjoint from microphone generations and speech's NO_INPUT
                // sentinel, even after the counter wraps.
                self.audio_file_generation =
                    self.audio_file_generation.wrapping_add(1) & ((1_u64 << 62) - 1);
                let generation = (1_u64 << 63) | self.audio_file_generation;
                if let Some(session) = &self.speech {
                    session.set_input_generation(Some(generation));
                }
                self.audio_import = Some(import);
                self.audio_file_active = true;
                self.audio_file_error = false;
                self.audio_file_progress = None;
                self.microphone_generation = Some(generation);
                self.dictation_requested = true;
                self.dictation_finishing = true;
                "Choose an audio file. Its transcript is inserted at the cursor and sent with your draft when the model is ready; otherwise it stays in the draft. Cancel or Escape stops import."
                    .clone_into(&mut self.speech_status);
                self.focus_composer = true;
            } else {
                self.audio_file_error = true;
                "Could not open audio input. Your draft was kept."
                    .clone_into(&mut self.speech_status);
            }
        }
        if self.audio_file_active
            || (self.audio_file_input && self.audio_file_error)
            || self.audio_file_notice.is_none()
        {
            self.audio_file_notice = Some(self.speech_status.clone());
        }
        self.record(&self.speech_status.clone());
    }

    pub(super) fn poll_audio_import(&mut self) {
        let result = self
            .audio_import
            .as_mut()
            .and_then(audio_import::Session::poll);
        if let Some(result) = result {
            self.audio_import_result(result);
        }
    }

    fn audio_import_result(&mut self, result: Result<Option<audio_import::Chunk>, &'static str>) {
        if !self.audio_file_active || !self.dictation_requested {
            self.audio_import.take();
            return;
        }
        match result {
            Ok(Some(first)) => {
                let Some(generation) = self.microphone_generation else {
                    self.fail_audio_file(
                        "The audio request is no longer available. Try selecting the file again.",
                    );
                    return;
                };
                let Some(import) = self.audio_import.take() else {
                    self.fail_audio_file(
                        "The audio reader is no longer available. Try selecting the file again.",
                    );
                    return;
                };
                if !self.speech_command(speech::Command::TranscribeFile {
                    generation,
                    first,
                    import,
                }) {
                    // speech_command already published the actionable failure.
                    return;
                }
                // Keep the previous result through picker cancellation,
                // decoding rejection and failed speech-worker handoff.
                self.audio_file_transcript.clear();
                self.show_audio_transcript = false;
                self.audio_file_review_required = false;
                "Loading the speech model and transcribing audio. When finished, the transcript is inserted at the cursor and sent with your draft if the model is ready. Cancel or Escape stops transcription."
                    .clone_into(&mut self.speech_status);
            }
            Ok(None) => {
                self.cancel_microphone();
                self.audio_import.take();
                "No audio file selected. Your draft was kept.".clone_into(&mut self.speech_status);
            }
            Err(message) => {
                self.cancel_microphone();
                self.audio_import.take();
                message.clone_into(&mut self.speech_status);
                self.audio_file_error = true;
            }
        }
        self.audio_file_notice = Some(self.speech_status.clone());
        self.record(&self.speech_status.clone());
    }

    pub(super) fn cancel_audio_file(&mut self) {
        self.cancel_microphone();
        self.audio_file_notice = Some(if self.audio_file_transcript.is_empty() {
            "Audio transcription cancelled. No message was sent. Your draft was kept.".to_owned()
        } else {
            "Audio transcription cancelled. No message was sent. Your draft was kept. The retained transcript is available to view or copy.".to_owned()
        });
    }

    pub(super) fn fail_audio_file(&mut self, message: &str) {
        self.cancel_microphone();
        self.audio_file_error = true;
        self.audio_file_notice = Some(format!(
            "{message} No message was sent. Your draft was kept.{}",
            if self.audio_file_transcript.is_empty() {
                ""
            } else {
                " The retained full or partial transcript is available to view or copy."
            },
        ));
    }

    pub(super) fn audio_file_part(&mut self, generation: u64, text: &str) {
        if !self.audio_file_active || self.microphone_generation != Some(generation) {
            return;
        }
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        if self
            .audio_file_transcript
            .try_reserve_exact(text.len().saturating_add(1))
            .is_err()
        {
            self.cancel_microphone();
            self.audio_file_error = true;
            self.audio_file_notice = Some("Not enough memory to retain more transcript. Close other apps and try again. Your draft was kept; any partial transcript is available to view or copy.".to_owned());
            return;
        }
        if !self.audio_file_transcript.is_empty() {
            self.audio_file_transcript.push('\n');
        }
        self.audio_file_transcript.push_str(text);
    }

    pub(super) fn finish_audio_file(&mut self, generation: u64) {
        if !self.audio_file_active || self.microphone_generation != Some(generation) {
            return;
        }
        if self.audio_file_fits_composer(&self.audio_file_transcript) {
            // Use the existing deferred insertion path so same-frame Escape,
            // conversation changes and composer input settle before submission.
            self.pending_voice_transcript = Some((generation, self.audio_file_transcript.clone()));
        } else {
            self.release_completed_input();
            self.audio_file_notice = Some("Transcription complete. View or copy the full transcript below. It exceeds the chat prompt size, so no message was sent. Your draft was kept.".to_owned());
        }
    }

    pub(super) fn audio_file_fits_composer(&self, text: &str) -> bool {
        let length = self.input.chars().count();
        let range = self.composer_cursor.map_or(length..length, |cursor| {
            let range = cursor.as_sorted_char_range();
            usize::from(range.start).min(length)..usize::from(range.end).min(length)
        });
        let prefix = range.start > 0
            && self
                .input
                .chars()
                .nth(range.start - 1)
                .is_some_and(|character| !character.is_whitespace());
        let suffix = self
            .input
            .chars()
            .nth(range.end)
            .is_some_and(|character| !character.is_whitespace());
        text.trim()
            .chars()
            .count()
            .saturating_add(usize::from(prefix))
            .saturating_add(usize::from(suffix))
            <= super::INPUT_LIMIT.saturating_sub(length - (range.end - range.start))
    }

    pub(super) fn audio_file_error_actions(&mut self, ui: &mut egui::Ui) {
        if !self.audio_file_transcript.is_empty() {
            if ui.button("View transcript").clicked() {
                self.show_audio_transcript = true;
                self.focus_composer = false;
            }
            self.copy_audio_transcript(ui, "Copy transcript", false);
        }
        if ui
            .button("Dismiss")
            .accessible_name("Dismiss audio status")
            .clicked()
        {
            self.audio_file_notice = None;
            self.audio_file_transcript.clear();
            self.audio_file_progress = None;
            self.audio_file_error = false;
            self.show_audio_transcript = false;
        }
    }

    pub(super) fn audio_file_status(&mut self, ui: &mut egui::Ui) {
        let Some(message) = self.audio_file_notice.clone() else {
            return;
        };
        if self.audio_file_error {
            return;
        }
        notice_frame(ui).show(ui, |ui| {
                // Frame margins belong inside the available composer column.
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
                ui.horizontal_wrapped(|ui| {
                    if self.audio_file_active {
                        ui.spinner();
                    }
                    let title = if self.audio_file_active {
                        egui::RichText::new("Audio transcription in progress")
                    } else {
                        egui::RichText::new("Audio transcription")
                    };
                    ui.label(title.strong());
                });
                if self.audio_file_active
                    && let Some(progress) = self.audio_file_progress
                {
                    let range = format!(
                        "Processing {}-{}",
                        audio_time(progress.start),
                        audio_time(progress.end),
                    );
                    if !progress.total.is_zero() && progress.end <= progress.total {
                        ui.label(egui::RichText::new(format!(
                            "{range} · estimated length {}",
                            audio_time(progress.total),
                        )).small().weak());
                        // Only earlier segments have completed. Container duration
                        // is an estimate, so never present a percentage or an ETA.
                        ui.add(
                            egui::ProgressBar::new(
                                progress.start.as_secs_f32() / progress.total.as_secs_f32(),
                            )
                            .desired_height(6.0)
                            .animate(false),
                        )
                        .on_hover_text("Completed audio before the current segment. The recording length is estimated from file metadata.");
                    } else {
                        ui.label(egui::RichText::new(format!("{range} · recording length unknown")).small().weak());
                    }
                }
                egui::ScrollArea::vertical()
                    .id_salt("audio-status-message")
                    .max_height(64.0)
                    .show(ui, |ui| {
                        ui.add(egui::Label::new(message).wrap());
                    });
                let has_transcript = !self.audio_file_transcript.is_empty();
                if has_transcript && self.audio_import.is_some() {
                    ui.weak("Your previous transcript is kept until another file is accepted.");
                }
                ui.horizontal_wrapped(|ui| {
                    if has_transcript {
                        if ui.add(action_button("View transcript")).clicked() {
                            self.show_audio_transcript = true;
                            self.focus_composer = false;
                        }
                        self.copy_audio_transcript(ui, "Copy transcript", false);
                    }
                    if self.audio_file_active {
                        if ui.add(action_button("Cancel"))
                            .accessible_name("Cancel transcription")
                            .on_hover_text("Cancel transcription · Escape")
                            .clicked()
                        {
                            self.cancel_audio_file();
                        }
                    } else if ui.add(action_button("Dismiss").frame_when_inactive(false))
                        .accessible_name("Dismiss audio status")
                        .clicked()
                    {
                        self.audio_file_notice = None;
                        self.audio_file_transcript.clear();
                        self.audio_file_progress = None;
                        self.audio_file_error = false;
                        self.show_audio_transcript = false;
                    }
                });
            });
        ui.add_space(8.0);
    }

    fn copy_audio_transcript(&self, ui: &mut egui::Ui, label: &str, primary: bool) {
        let id = egui::Id::unique("audio-transcript-copied");
        let copied = ui.ctx().data(|data| data.get_temp::<(u64, f64)>(id));
        let copied = copied.is_some_and(|(generation, time)| {
            generation == self.audio_file_generation && ui.input(|input| input.time) - time < 2.0
        });
        // Reserve the ordinary label width so copy confirmation never moves
        // adjacent actions or changes a wrapping footer's height.
        // egui 0.37's button theme uses Body. Bind the measured and displayed
        // font explicitly so the shorter confirmation keeps the same bounds.
        let font = egui::TextStyle::Body.resolve(ui.style());
        let text_width = ui
            .painter()
            .layout_no_wrap(label.to_owned(), font.clone(), ui.visuals().text_color())
            .size()
            .x;
        let width = ui.spacing().button_padding.x.mul_add(2.0, text_width);
        let mut text = egui::RichText::new(if copied { "Copied" } else { label }).font(font);
        if primary {
            text = text.color(ui.visuals().selection.stroke.color);
        }
        let mut button = egui::Button::new(text)
            .min_size(egui::vec2(width, 36.0))
            .corner_radius(8);
        if primary {
            button = button.fill(ui.visuals().selection.bg_fill);
        }
        let response = ui.add(button);
        if response.accessible_name(label).clicked() {
            ui.ctx().copy_text(self.audio_file_transcript.clone());
            let now = ui.input(|input| input.time);
            ui.ctx().data_mut(|data| {
                data.insert_temp(id, (self.audio_file_generation, now));
            });
        }
        if copied {
            ui.ctx().request_repaint_after(Duration::from_secs(2));
        }
    }

    pub(super) fn audio_transcript_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_audio_transcript {
            return;
        }
        let modal = egui::Modal::new(egui::Id::unique("audio-transcript-modal"))
            .frame(dialog_frame(ctx))
            .backdrop_color(egui::Color32::from_black_alpha(150))
            .show(ctx, |ui| self.audio_transcript_contents(ui));
        #[cfg(test)]
        ctx.data_mut(|data| {
            data.insert_temp(
                egui::Id::unique("audio-transcript-bounds"),
                modal.response.rect,
            );
        });
        if modal.inner || modal.should_close() {
            self.show_audio_transcript = false;
            self.focus_composer = true;
        }
    }

    fn audio_transcript_contents(&mut self, ui: &mut egui::Ui) -> bool {
        ui.set_width((ui.ctx().content_rect().width() - 80.0).clamp(240.0, 680.0));
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
        ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
        let content_top = ui.cursor().top();
        ui.label(egui::RichText::new("Audio transcript").strong().size(22.0));
        ui.label(egui::RichText::new("Session only. Copy the text to keep it.").weak());
        if self.audio_file_active {
            notice_frame(ui).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    "Still transcribing. If it finishes here, review the draft before sending.",
                );
            });
        }
        ui.add_space(4.0);
        let label = ui.label(
            egui::RichText::new("Transcript text (read only)")
                .small()
                .strong(),
        );
        // Header wrapping and text scaling consume real space. Reserve both
        // possible footer rows, document/frame insets and screen margins.
        let header_height = ui.cursor().top() - content_top;
        let height = (ui.ctx().content_rect().height() - header_height - 184.0).clamp(40.0, 480.0);
        self.audio_transcript_text(ui, label.id, height);
        ui.add_space(4.0);
        ui.separator();
        ui.add_space(4.0);
        let mut close = false;
        ui.horizontal_wrapped(|ui| {
            self.copy_audio_transcript(ui, "Copy full transcript", true);
            if self.audio_file_active
                && ui
                    .add(action_button("Cancel"))
                    .accessible_name("Cancel transcription in viewer")
                    .on_hover_text("Cancel transcription · Escape closes the viewer")
                    .clicked()
            {
                self.cancel_audio_file();
            }
            close = ui
                .add(action_button("Close").frame_when_inactive(false))
                .accessible_name("Close transcript")
                .on_hover_text("Close transcript · Escape")
                .clicked();
        });
        close
    }

    fn audio_transcript_text(&self, ui: &mut egui::Ui, label: egui::Id, height: f32) {
        let scroll = egui::Frame::new()
            .fill(ui.visuals().extreme_bg_color)
            .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
            .corner_radius(10)
            .inner_margin(12)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::ScrollArea::vertical()
                    .id_salt("audio-transcript-scroll")
                    .auto_shrink([false, false])
                    .stick_to_bottom(self.audio_file_active)
                    // Reserve the admitted height so transcript growth cannot
                    // resize the modal or move its footer.
                    .min_scrolled_height(height)
                    .max_height(height)
                    .show(ui, |ui| {
                        // Borrowed &str keeps selection/copy without editing.
                        ui.add(
                            egui::TextEdit::multiline(&mut self.audio_file_transcript.as_str())
                                .id_salt("audio-transcript-text")
                                .frame(egui::Frame::NONE)
                                .font(egui::FontId::proportional(super::appearance::text_size(
                                    ui.ctx(),
                                )))
                                .hint_text("Recognized speech will appear here.")
                                .desired_width(ui.available_width())
                                .desired_rows(4),
                        )
                        .labelled_by(label);
                    })
            })
            .inner;
        #[cfg(test)]
        ui.ctx().data_mut(|data| {
            data.insert_temp(
                egui::Id::unique("audio-transcript-viewport"),
                scroll.inner_rect,
            );
        });
        #[cfg(not(test))]
        let _ = scroll;
    }

    pub(super) fn audio_file_button(&mut self, ui: &mut egui::Ui) {
        let active = self.audio_file_active;
        let response = ui
            .add_enabled(
                active
                    || (!self.model.busy()
                        && !self.speaking
                        && !self.dictation_requested
                        && self.audio_import.is_none()),
                action_button(if active { "Cancel audio" } else { "+ Audio file" })
                    .selected(active),
            )
            .accessible_name(if active { "Cancel audio file transcription" } else { "Add audio file" })
            .on_hover_text(if active {
                "Cancel audio file transcription - Escape. Your draft and any retained transcript are kept."
            } else {
                "Transcribe an MP3, WAV, M4A, AAC, FLAC or WMA file, or paste a file copied from Explorer. The transcript is inserted at the cursor and sent with your draft if the model is ready; otherwise it stays in the draft. Microphone permission is not required. Long recordings are processed in segments."
            });
        if response.clicked() {
            if active {
                self.cancel_audio_file();
                self.speech_notice = true;
            } else {
                self.start_audio_import(Source::Picker {
                    owner: self.native_owner.unwrap_or_default(),
                });
            }
        }
    }
}

fn audio_time(duration: Duration) -> String {
    let seconds = duration.as_secs();
    if seconds >= 3600 {
        format!(
            "{}:{:02}:{:02}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        )
    } else {
        format!("{}:{:02}", seconds / 60, seconds % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{client, window::ModelLifecycle};
    use std::{io, sync::mpsc};

    type Fixture = (
        State,
        mpsc::Receiver<speech::Command>,
        mpsc::Receiver<client::Command>,
    );

    fn fixture() -> io::Result<Fixture> {
        let (commands, requests) = mpsc::channel();
        let (_events, incoming) = client::event_channel();
        let mut state = State::create(commands, incoming, None, None);
        let (session, speech_commands) = speech::Session::fixture()?;
        state.speech = Some(session);
        state.preview = false;
        state.model = ModelLifecycle::Preparing;
        state.speech_event(speech::Event::Capabilities {
            input: true,
            output: false,
        });
        state.audio_file_active = true;
        state.dictation_requested = true;
        state.dictation_finishing = true;
        state.microphone_generation = Some(1_u64 << 63);
        Ok((state, speech_commands, requests))
    }

    fn chunk(wav: Vec<u8>) -> audio_import::Chunk {
        audio_import::Chunk {
            wav,
            start: Duration::ZERO,
            end: Duration::from_secs(1),
            total: Duration::from_secs(1),
            last: true,
        }
    }

    #[test]
    fn audio_file_without_microphone_transcribes_and_submits_one_text_prompt() -> io::Result<()> {
        let (mut state, speech_commands, requests) = fixture()?;
        assert!(state.audio_file_input && !state.speech_input && state.microphone.is_none());
        state.model = ModelLifecycle::Ready;
        let wave = b"normalized test clip".to_vec();
        state.audio_import = Some(audio_import::Session::fixture(vec![])?);
        state.audio_import_result(Ok(Some(chunk(wave.clone()))));
        assert!(
            matches!(speech_commands.recv_timeout(Duration::from_secs(2)), Ok(speech::Command::TranscribeFile { generation, first, .. }) if generation == 1_u64 << 63 && first.wav == wave)
        );
        state.speech_event(speech::Event::FilePart {
            generation: 1_u64 << 63,
            text: "Explain this recording".into(),
        });
        state.speech_event(speech::Event::FileFinished {
            generation: 1_u64 << 63,
        });
        state.flush_voice_transcript(&egui::Context::default());
        assert!(
            matches!(requests.try_recv(), Ok(client::Command::Send { text, .. }) if text == "Explain this recording")
        );
        assert!(!state.audio_file_active);
        assert_eq!(state.speech_status, "Audio file transcribed and sent.");
        state.accept_transcript(1_u64 << 63, "Late duplicate");
        assert!(requests.try_recv().is_err());
        Ok(())
    }

    #[test]
    fn audio_file_loading_draft_survives_readiness_without_deferred_send() -> io::Result<()> {
        let (mut state, _speech_commands, requests) = fixture()?;
        state.accept_transcript(1_u64 << 63, "Keep this transcript");
        assert_eq!(state.input, "Keep this transcript");
        assert!(!state.audio_file_active);
        assert!(state.speech_status.starts_with("Audio file transcribed."));
        state.model_event(client::Event::Ready);
        assert!(requests.try_recv().is_err());
        Ok(())
    }

    #[test]
    fn completed_input_submission_keeps_asr_until_explicit_cancellation() -> io::Result<()> {
        for from_file in [false, true] {
            let (mut state, commands, requests) = fixture()?;
            state.audio_file_active = from_file;
            state.model = ModelLifecycle::Ready;
            state.accept_transcript(1_u64 << 63, "Next question");
            assert!(matches!(
                requests.try_recv(),
                Ok(client::Command::Send { text, .. }) if text == "Next question"
            ));
            assert!(!state.dictation_requested && state.microphone_generation.is_none());
            assert!(matches!(
                commands.recv_timeout(Duration::from_secs(2)),
                Ok(speech::Command::StopSpeaking)
            ));
            state.cancel_microphone();
            assert!(matches!(
                commands.recv_timeout(Duration::from_secs(2)),
                Ok(speech::Command::StopListening)
            ));
        }
        Ok(())
    }

    #[test]
    fn completed_input_draft_and_oversized_file_keep_asr_until_switch() -> io::Result<()> {
        for oversized in [false, true] {
            let (mut state, commands, requests) = fixture()?;
            if oversized {
                state.audio_file_transcript = "x".repeat(super::super::INPUT_LIMIT + 1);
                state.finish_audio_file(1_u64 << 63);
            } else {
                state.accept_transcript(1_u64 << 63, "Keep as draft");
                assert_eq!(state.input, "Keep as draft");
            }
            assert!(!state.dictation_requested && !state.audio_file_active);
            assert!(requests.try_recv().is_err());
            state.new_chat();
            assert!(matches!(
                commands.recv_timeout(Duration::from_secs(2)),
                Ok(speech::Command::StopListening)
            ));
            assert!(matches!(
                commands.recv_timeout(Duration::from_secs(2)),
                Ok(speech::Command::StopSpeaking)
            ));
        }
        Ok(())
    }

    #[test]
    fn audio_file_failure_cancellation_and_switch_preserve_drafts_and_ignore_late_results()
    -> io::Result<()> {
        for action in 0..3 {
            let (mut state, _speech_commands, requests) = fixture()?;
            state.model = ModelLifecycle::Ready;
            state.input = "Existing draft".into();
            match action {
                0 => state
                    .audio_import_result(Err("Unsupported or corrupt audio. Your draft was kept.")),
                1 => state.cancel_microphone(),
                _ => state.new_chat(),
            }
            state.audio_import_result(Ok(Some(chunk(vec![1, 2]))));
            state.accept_transcript(1_u64 << 63, "Late audio transcript");
            assert!(!state.audio_file_active);
            if action < 2 {
                assert_eq!(state.input, "Existing draft");
            } else {
                assert_eq!(state.input, "");
                assert!(matches!(requests.try_recv(), Ok(client::Command::NewChat)));
            }
            assert!(requests.try_recv().is_err());
        }
        Ok(())
    }

    #[test]
    fn audio_file_errors_and_progress_survive_microphone_cleanup_events() -> io::Result<()> {
        let (mut state, _speech_commands, requests) = fixture()?;
        state.audio_file_notice = Some("Preparing audio".to_owned());
        state.speech_status = "Preparing audio".to_owned();
        state.speech_event(speech::Event::Listening(false));
        assert_eq!(state.audio_file_notice.as_deref(), Some("Preparing audio"));
        assert_eq!(state.speech_status, "Preparing audio");
        state.input = "Keep draft".to_owned();
        let error = "The audio file is damaged or uses an unsupported codec. Try exporting it as WAV or MP3. Your draft was kept.";
        state.audio_import_result(Err(error));
        state.speech_event(speech::Event::Listening(false));
        state.speech_event(speech::Event::Capabilities {
            input: true,
            output: false,
        });
        assert_eq!(state.audio_file_notice.as_deref(), Some(error));
        assert!(state.audio_file_error && !state.audio_file_active);
        assert_eq!(state.input, "Keep draft");
        assert!(requests.try_recv().is_err());
        Ok(())
    }

    #[test]
    fn audio_file_long_transcript_is_complete_without_prompt_truncation_or_submission()
    -> io::Result<()> {
        let (mut state, _speech_commands, requests) = fixture()?;
        state.model = ModelLifecycle::Ready;
        state.input = "Keep draft".to_owned();
        let first = "界".repeat(super::super::INPUT_LIMIT);
        let generation = 1_u64 << 63;
        state.audio_file_part(generation, &first);
        state.audio_file_part(generation, "Final segment");
        state.finish_audio_file(generation);
        state.flush_voice_transcript(&egui::Context::default());
        assert_eq!(
            state.audio_file_transcript,
            format!("{first}\nFinal segment")
        );
        assert_eq!(state.input, "Keep draft");
        assert!(!state.audio_file_active);
        assert!(
            state
                .audio_file_notice
                .as_deref()
                .is_some_and(|message| message.contains("full transcript"))
        );
        assert!(requests.try_recv().is_err());
        Ok(())
    }

    #[test]
    fn audio_file_failure_preserves_partial_transcript_and_ignores_late_parts() -> io::Result<()> {
        let (mut state, _speech_commands, requests) = fixture()?;
        let generation = 1_u64 << 63;
        state.audio_file_part(generation, "First segment");
        state.speech_event(speech::Event::InputFailed {
            generation,
            message: "Not enough available memory.",
        });
        state.audio_file_part(generation, "Late segment");
        state.finish_audio_file(generation);
        state.speech_event(speech::Event::Listening(false));
        assert_eq!(state.audio_file_transcript, "First segment");
        assert!(
            state
                .audio_file_notice
                .as_deref()
                .is_some_and(|message| message.contains("partial transcript"))
        );
        assert!(state.audio_file_error && !state.audio_file_active);
        assert!(requests.try_recv().is_err());
        Ok(())
    }

    #[test]
    fn audio_file_notice_does_not_replace_later_microphone_recording_feedback() -> io::Result<()> {
        let (mut state, _speech_commands, _requests) = fixture()?;
        state.cancel_audio_file();
        state.dictation_requested = true;
        state.speech_event(speech::Event::Listening(true));
        assert!(state.speech_status.starts_with("Recording."));
        assert!(
            state
                .audio_file_notice
                .as_deref()
                .is_some_and(|message| message.contains("cancelled"))
        );
        Ok(())
    }

    #[test]
    fn audio_file_replaces_a_full_draft_selection_without_truncating_the_transcript()
    -> io::Result<()> {
        let (mut state, _speech_commands, requests) = fixture()?;
        state.input = "界".repeat(super::super::INPUT_LIMIT);
        state.composer_cursor = Some(egui::text::CCursorRange::two(
            egui::text::CCursor::new(0),
            egui::text::CCursor::new(super::super::INPUT_LIMIT),
        ));
        let generation = 1_u64 << 63;
        state.audio_file_part(generation, "Replacement transcript");
        state.finish_audio_file(generation);
        state.flush_voice_transcript(&egui::Context::default());
        assert_eq!(state.input, "Replacement transcript");
        assert_eq!(state.audio_file_transcript, "Replacement transcript");
        assert!(requests.try_recv().is_err());
        Ok(())
    }

    #[test]
    fn audio_file_replacement_preserves_retained_text_until_successful_handoff() -> io::Result<()> {
        for rejected in [false, true] {
            let (mut state, _speech_commands, requests) = fixture()?;
            state.audio_file_transcript = "Previous transcript".to_owned();
            state.input = "Keep draft".to_owned();
            state.audio_import_result(if rejected {
                Err("Unsupported audio. Choose a WAV or MP3 file.")
            } else {
                Ok(None)
            });
            assert_eq!(state.audio_file_transcript, "Previous transcript");
            assert_eq!(state.input, "Keep draft");
            assert!(!state.audio_file_active);
            assert!(requests.try_recv().is_err());
        }
        let (mut state, speech_commands, requests) = fixture()?;
        state.audio_file_transcript = "Previous transcript".to_owned();
        state.audio_file_notice = Some("Current progress".to_owned());
        state.start_audio_import(Source::Clipboard);
        assert_eq!(state.audio_file_notice.as_deref(), Some("Current progress"));
        assert_eq!(state.audio_file_transcript, "Previous transcript");
        state.audio_import = Some(audio_import::Session::fixture(vec![])?);
        state.audio_import_result(Ok(Some(chunk(vec![1, 2]))));
        assert!(matches!(
            speech_commands.recv_timeout(Duration::from_secs(2)),
            Ok(speech::Command::TranscribeFile { .. })
        ));
        assert_eq!(state.audio_file_transcript, "");
        state.audio_file_part(1_u64 << 63, "New transcript");
        assert_eq!(state.audio_file_transcript, "New transcript");
        assert!(requests.try_recv().is_err());
        Ok(())
    }

    #[test]
    fn audio_file_failed_handoff_keeps_previous_transcript_and_actionable_notice() -> io::Result<()>
    {
        let (mut state, speech_commands, requests) = fixture()?;
        drop(speech_commands);
        // Dropping the observation channel alone leaves the fixture command
        // worker alive. Join it before exercising an unavailable-worker handoff.
        drop(state.speech.take());
        state.audio_file_transcript = "Previous transcript".to_owned();
        state.audio_import = Some(audio_import::Session::fixture(vec![])?);
        state.audio_import_result(Ok(Some(chunk(vec![1, 2]))));
        assert_eq!(state.audio_file_transcript, "Previous transcript");
        assert!(state.audio_file_error && !state.audio_file_active);
        assert!(
            state
                .audio_file_notice
                .as_deref()
                .is_some_and(|notice| notice.contains("transcript"))
        );
        assert!(requests.try_recv().is_err());
        Ok(())
    }
}
