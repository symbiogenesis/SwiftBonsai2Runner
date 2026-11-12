//! Native audio attachments reuse the microphone transcription and prompt path.

use super::{State, egui, speech};
use crate::audio_import::{self, Source};
use raw_window_handle::{HasWindowHandle as _, RawWindowHandle};

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
                self.record("Audio file paste is unavailable. Use the plus button to select audio.")
            }
        }
    }

    pub(super) fn handle_audio_paste(&mut self, ctx: &egui::Context) {
        if !self.audio_paste.as_ref().is_some_and(|hook| hook.poll())
            || self.composing
            || self.rename_conversation.is_some()
            || self.delete_conversation.is_some()
            || self.show_settings
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
                .retain(|event| !matches!(event, egui::Event::Paste(_)))
        });
        self.start_audio_import(Source::Clipboard);
    }

    fn start_audio_import(&mut self, source: Source) {
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
        } else if self.model.busy()
            || self.speaking
            || self.dictation_requested
            || self.audio_import.is_some()
        {
            "Finish or cancel the current response or speech action before adding audio."
                .clone_into(&mut self.speech_status);
        } else {
            self.cancel_microphone();
            match audio_import::Session::start(source) {
                Ok(import) => {
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
                    self.microphone_generation = Some(generation);
                    self.dictation_requested = true;
                    self.dictation_finishing = true;
                    "Opening audio file. Escape cancels.".clone_into(&mut self.speech_status);
                    self.focus_composer = true;
                }
                Err(_) => {
                    "Could not open audio input. Your draft was kept."
                        .clone_into(&mut self.speech_status);
                }
            }
        }
        self.record(&self.speech_status.clone());
    }

    pub(super) fn poll_audio_import(&mut self) {
        let result = self
            .audio_import
            .as_mut()
            .and_then(audio_import::Session::poll);
        if let Some(result) = result {
            self.audio_import.take();
            self.audio_import_result(result);
        }
    }

    fn audio_import_result(&mut self, result: Result<Option<Vec<u8>>, &'static str>) {
        if !self.audio_file_active || !self.dictation_requested {
            return;
        }
        match result {
            Ok(Some(wav)) => {
                if let Some(generation) = self.microphone_generation
                    && self.speech_command(speech::Command::Transcribe { generation, wav })
                {
                    "Transcribing audio file with Qwen3-ASR 1.7B. Escape cancels."
                        .clone_into(&mut self.speech_status);
                }
            }
            Ok(None) => {
                self.cancel_microphone();
                "No audio file selected. Your draft was kept.".clone_into(&mut self.speech_status);
            }
            Err(message) => {
                self.cancel_microphone();
                message.clone_into(&mut self.speech_status);
            }
        }
        self.record(&self.speech_status.clone());
    }

    pub(super) fn audio_file_button(&mut self, ui: &mut egui::Ui) {
        let response = ui.add_enabled(
            self.audio_file_active || (!self.model.busy() && !self.speaking && !self.dictation_requested && self.audio_import.is_none()),
            egui::Button::new("").min_size(egui::Vec2::splat(32.0)).selected(self.audio_file_active),
        ).accessible_name("Add audio file").on_hover_text(if self.audio_file_active {
            "Cancel audio file transcription - Escape"
        } else {
            "Add audio file - MP3, WAV, M4A, AAC, FLAC or WMA; up to 30 seconds. You can also paste a file copied from Explorer."
        });
        let center = response.rect.center();
        let stroke = ui.style().interact(&response).fg_stroke;
        let painter = ui.painter_at(response.rect);
        let offset = if self.audio_file_active {
            egui::vec2(5.0, 5.0)
        } else {
            egui::vec2(7.0, 0.0)
        };
        painter.line_segment([center - offset, center + offset], stroke);
        let perpendicular = egui::vec2(-offset.y, offset.x);
        painter.line_segment([center - perpendicular, center + perpendicular], stroke);
        if response.clicked() {
            if self.audio_file_active {
                self.cancel_microphone();
                "Audio file cancelled. Your draft was kept.".clone_into(&mut self.speech_status);
                self.speech_notice = true;
            } else {
                self.start_audio_import(Source::Picker {
                    owner: self.native_owner.unwrap_or_default(),
                });
            }
        }
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

    #[test]
    fn audio_file_without_microphone_transcribes_and_submits_one_text_prompt() -> io::Result<()> {
        let (mut state, speech_commands, requests) = fixture()?;
        assert!(state.audio_file_input && !state.speech_input && state.microphone.is_none());
        state.model = ModelLifecycle::Ready;
        let wave = b"normalized test clip".to_vec();
        state.audio_import_result(Ok(Some(wave.clone())));
        assert!(
            matches!(speech_commands.recv_timeout(Duration::from_secs(2)), Ok(speech::Command::Transcribe { generation, wav }) if generation == 1_u64 << 63 && wav == wave)
        );
        state.speech_event(speech::Event::Recognized {
            generation: 1_u64 << 63,
            text: "Explain this recording".into(),
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
            state.audio_import_result(Ok(Some(vec![1, 2])));
            state.accept_transcript(1_u64 << 63, "Late audio transcript");
            assert!(!state.audio_file_active);
            if action < 2 {
                assert_eq!(state.input, "Existing draft");
                assert!(requests.try_recv().is_err());
            } else {
                assert!(state.input.is_empty());
                assert!(matches!(requests.try_recv(), Ok(client::Command::NewChat)));
                assert!(requests.try_recv().is_err());
            }
        }
        Ok(())
    }
}
