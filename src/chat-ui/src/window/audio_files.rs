//! Native audio attachments reuse the microphone transcription and prompt path.

use super::{State, egui, speech};
use crate::audio_import::{self, Source};
use raw_window_handle::{HasWindowHandle as _, RawWindowHandle};
use std::time::Duration;

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
            self.cancel_microphone();
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
            self.cancel_microphone();
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

    pub(super) fn audio_file_status(&mut self, ui: &mut egui::Ui) {
        let Some(message) = self.audio_file_notice.clone() else {
            return;
        };
        egui::Frame::new()
            .fill(ui.visuals().faint_bg_color)
            .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
            .corner_radius(12)
            .inner_margin(12)
            .show(ui, |ui| {
                // Frame margins belong inside the available composer column.
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    if self.audio_file_active {
                        ui.spinner();
                    }
                    let title = if self.audio_file_error {
                        egui::RichText::new("Audio transcription failed")
                            .color(ui.visuals().error_fg_color)
                    } else if self.audio_file_active {
                        egui::RichText::new("Audio transcription in progress")
                    } else {
                        egui::RichText::new("Audio transcription")
                    };
                    ui.label(title.strong());
                    if self.audio_file_active {
                        if ui.button("Cancel transcription").clicked() {
                            self.cancel_audio_file();
                        }
                    } else if ui.button("Dismiss audio status").clicked() {
                        self.audio_file_notice = None;
                        self.audio_file_transcript.clear();
                        self.audio_file_progress = None;
                        self.audio_file_error = false;
                        self.show_audio_transcript = false;
                    }
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
                        ui.label(format!(
                            "{range} | estimated length {}",
                            audio_time(progress.total),
                        ));
                        // Only earlier segments have completed. Container duration
                        // is an estimate, so never present a percentage or an ETA.
                        ui.add(
                            egui::ProgressBar::new(
                                progress.start.as_secs_f32() / progress.total.as_secs_f32(),
                            )
                            .animate(false),
                        )
                        .on_hover_text("Completed audio before the current segment. The recording length is estimated from file metadata.");
                    } else {
                        ui.label(format!("{range} | recording length unknown"));
                    }
                }
                egui::ScrollArea::vertical()
                    .id_salt("audio-status-message")
                    .max_height(64.0)
                    .show(ui, |ui| {
                        ui.add(egui::Label::new(message).wrap());
                    });
                if !self.audio_file_transcript.is_empty() {
                    if self.audio_import.is_some() {
                        ui.weak("Your previous transcript is kept until another file is accepted.");
                    }
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("View transcript").clicked() {
                            self.show_audio_transcript = true;
                            self.focus_composer = false;
                        }
                        self.copy_audio_transcript(ui, "Copy transcript");
                    });
                }
            });
        ui.add_space(6.0);
    }

    fn copy_audio_transcript(&self, ui: &mut egui::Ui, label: &str) {
        let id = egui::Id::unique("audio-transcript-copied");
        if ui.button(label).clicked() {
            ui.ctx().copy_text(self.audio_file_transcript.clone());
            let now = ui.input(|input| input.time);
            ui.ctx().data_mut(|data| {
                data.insert_temp(id, (self.audio_file_generation, now));
            });
        }
        let copied = ui.ctx().data(|data| data.get_temp::<(u64, f64)>(id));
        if copied.is_some_and(|(generation, time)| {
            generation == self.audio_file_generation && ui.input(|input| input.time) - time < 2.0
        }) {
            ui.label("Copied");
            ui.ctx().request_repaint_after(Duration::from_secs(2));
        }
    }

    pub(super) fn audio_transcript_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_audio_transcript {
            return;
        }
        let mut close = false;
        let modal = egui::Modal::new(egui::Id::unique("audio-transcript-modal")).show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 64.0).clamp(260.0, 680.0));
            let content_top = ui.cursor().top();
            ui.heading("Audio transcript");
            ui.label("Session only. Copy the text to keep it.");
            if self.audio_file_active {
                ui.label(
                    "Still transcribing. If it finishes here, review the draft before sending.",
                );
            }
            ui.weak("Escape closes this view.");
            ui.add_space(8.0);
            let label = ui.label("Transcript text (read only)");
            // Header wrapping and text scaling consume real space. Reserve
            // footer/frame/screen margins before admitting the text viewport.
            let header_height = ui.cursor().top() - content_top;
            let viewport_height =
                (ctx.content_rect().height() - header_height - 112.0).clamp(40.0, 480.0);
            let scroll = egui::ScrollArea::vertical()
                .id_salt("audio-transcript-scroll")
                .auto_shrink([false, false])
                .max_height(viewport_height)
                .show(ui, |ui| {
                    // Borrowed &str keeps native selection/copy without any
                    // route for editing the retained transcript.
                    ui.add(
                        egui::TextEdit::multiline(&mut self.audio_file_transcript.as_str())
                            .id_salt("audio-transcript-text")
                            .desired_width(ui.available_width())
                            .desired_rows(4),
                    )
                    .labelled_by(label.id);
                });
            #[cfg(test)]
            ctx.data_mut(|data| {
                data.insert_temp(
                    egui::Id::unique("audio-transcript-viewport"),
                    scroll.inner_rect,
                );
            });
            #[cfg(not(test))]
            let _scroll = scroll;
            ui.add_space(10.0);
            ui.horizontal_wrapped(|ui| {
                self.copy_audio_transcript(ui, "Copy full transcript");
                close = ui.button("Close transcript").clicked();
            });
        });
        #[cfg(test)]
        ctx.data_mut(|data| {
            data.insert_temp(
                egui::Id::unique("audio-transcript-bounds"),
                modal.response.rect,
            );
        });
        if close || modal.should_close() {
            self.show_audio_transcript = false;
            self.focus_composer = true;
        }
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
                egui::Button::new(if active { "Cancel audio" } else { "+ Audio file" })
                    .min_size(egui::vec2(32.0, 32.0))
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
    use std::{io, sync::mpsc, time::Duration};

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
            speech_commands.try_recv(),
            Ok(speech::Command::TranscribeFile { .. })
        ));
        assert!(state.audio_file_transcript.is_empty());
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
