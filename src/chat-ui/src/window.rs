//! Native egui chat presentation. Backend work and audio remain off the UI loop.
#![expect(
    clippy::redundant_pub_crate,
    reason = "Keep presentation internals parent-only."
)]

use crate::{
    client::{self, Command, Completion, Config, Event},
    speech,
};
use eframe::egui::{self, RichText};
use std::{
    collections::VecDeque,
    io,
    sync::mpsc::{self, Receiver, Sender},
    thread::JoinHandle,
    time::{Duration, Instant},
};

mod appearance;
mod audio_files;
#[cfg(test)]
mod background_tests;
mod branding;
mod context_capacity;
#[cfg(test)]
mod ergonomics_tests;
mod graphics;
mod history_search;
#[cfg(test)]
mod history_search_tests;
#[cfg(test)]
mod interaction_tests;
mod iteration;
#[cfg(test)]
mod iteration_ui_tests;
#[cfg(test)]
mod keyboard_tests;
#[allow(
    unsafe_code,
    reason = "Audited UI-thread WinRT microphone initialization and teardown."
)]
mod microphone;
mod model_lifecycle;
mod navigation;
mod persistence;
#[allow(
    unsafe_code,
    reason = "Audited UI-thread native window placement with a borrowed eframe window handle."
)]
mod placement;
#[allow(
    unsafe_code,
    reason = "Audited Windows high-contrast and fatal-dialog boundary."
)]
mod platform;
mod profile;
mod read_aloud;
mod settings;
#[cfg(test)]
mod settings_ui_tests;
mod sidebar_status;
#[cfg(test)]
mod tests;
mod transcript;
mod updates;
mod workspace;

use model_lifecycle::ModelLifecycle;
use transcript::{Role, Transcript};
use workspace::Conversation;

const OUTPUT_TOKENS: [u32; 4] = [512, 1024, 2048, 4096];
const DEFAULT_VOICE: &str = "A warm, clear, natural adult voice with a calm conversational pace.";
const INPUT_LIMIT: usize = 32_768;
const VOICE_LIMIT: usize = 1_024;
const MAX_DIAGNOSTICS: usize = 128;
const MAX_DIAGNOSTIC_CHARS: usize = 4_096;

fn runtime_description(config: &Config) -> String {
    let engine = if config.native_engine {
        "Rust"
    } else {
        "Publisher"
    };
    let weights = if config.use_repacked_ptq1 {
        "PTQ1"
    } else {
        "PQ2"
    };
    if config.native_engine {
        format!(
            "{engine} CPU · {weights} · {} row workers",
            config.native_row_workers
        )
    } else {
        format!("{engine} CPU · {weights}")
    }
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "Independent broker, audio and presentation state is explicit."
)]
struct State {
    commands: Sender<Command>,
    events: client::EventReceiver,
    worker: Option<JoinHandle<()>>,
    startup_request: Option<crate::startup::Request>,
    startup: Option<crate::startup::Session>,
    startup_failure: Option<std::sync::Arc<std::sync::Mutex<Option<String>>>>,
    updates: Option<updates::Updates>,
    update_exit: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    model: ModelLifecycle,
    preview: bool,
    input: String,
    transcript: Transcript,
    committed: Vec<client::Message>,
    context_history: Option<usize>,
    context_draft: context_capacity::DraftCache,
    conversations: Vec<Conversation>,
    conversation_search: history_search::Search,
    active_conversation: u64,
    request_conversation: Option<u64>,
    routing_request: bool,
    next_conversation: u64,
    rename_conversation: Option<(u64, String)>,
    delete_conversation: Option<u64>,
    edit_message: Option<iteration::EditMessage>,
    conversation_notice: Option<&'static str>,
    recovery: Option<crate::recovery::Recovery>,
    placement: Option<placement::Placement>,
    save_conversations: bool,
    recovery_dirty: bool,
    recovery_erase_pending: bool,
    recovery_notice: String,
    last_recovery_save: Instant,
    restore_history: bool,
    status: String,
    runtime_description: String,
    warning: bool,
    diagnostics: VecDeque<String>,
    show_diagnostics: bool,
    show_settings: bool,
    settings_view: settings::View,
    appearance: appearance::Preferences,
    appearance_store: Option<appearance::Store>,
    profile_store: Option<profile::Store>,
    sidebar_open: bool,
    sidebar_focus: bool,
    help: Option<(&'static str, &'static str)>,
    extra_effort: bool,
    enable_thinking: bool,
    output_budget: usize,
    abliteration_scale: f32,
    abliteration_enabled: bool,
    focus_composer: bool,
    composing: bool,
    composer_id: Option<egui::Id>,
    composer_cursor: Option<egui::text::CCursorRange>,
    pending_cursor: Option<egui::text::CCursorRange>,
    speech: Option<speech::Session>,
    speech_checked: bool,
    speech_input: bool,
    audio_file_input: bool,
    audio_import: Option<crate::audio_import::Session>,
    audio_file_generation: u64,
    audio_file_active: bool,
    native_owner: Option<isize>,
    audio_paste: Option<crate::audio_import::PasteHook>,
    speech_output: bool,
    canary_configured: bool,
    tts_configured: bool,
    speech_status: String,
    speech_notice: bool,
    voice_profile: String,
    listening: bool,
    dictation_requested: bool,
    dictation_finishing: bool,
    pending_voice_transcript: Option<(u64, String)>,
    microphone: Option<microphone::Microphone>,
    microphone_generation: Option<u64>,
    speaking: bool,
    speaking_message: Option<u64>,
    streaming_read_aloud: bool,
    streaming_active: bool,
    streaming_suppressed: bool,
    answer_complete: bool,
    last_answer: String,
    retained_draft: String,
    stage: Option<&'static str>,
    last_theme_check: Instant,
}

impl State {
    fn create(
        commands: Sender<Command>,
        events: client::EventReceiver,
        config: Option<&Config>,
        microphone: Option<microphone::Microphone>,
    ) -> Self {
        let speech = config
            .and_then(|config| speech::start(config.canary.clone(), config.qwen_tts.clone()).ok());
        let speech_checked = speech.is_none();
        Self {
            commands,
            events,
            worker: None,
            startup_request: None,
            startup: None,
            startup_failure: None,
            updates: None,
            update_exit: None,
            model: if config.is_some() {
                ModelLifecycle::Preparing
            } else {
                ModelLifecycle::Unavailable
            },
            preview: config.is_none(),
            input: String::new(),
            transcript: Transcript::default(),
            committed: Vec::new(),
            context_history: None,
            context_draft: context_capacity::DraftCache::default(),
            conversations: vec![Conversation::new(1)],
            conversation_search: history_search::Search::default(),
            active_conversation: 1,
            request_conversation: None,
            routing_request: false,
            next_conversation: 2,
            rename_conversation: None,
            delete_conversation: None,
            edit_message: None,
            conversation_notice: None,
            recovery: None,
            placement: None,
            save_conversations: false,
            recovery_dirty: false,
            recovery_erase_pending: false,
            recovery_notice: String::new(),
            last_recovery_save: Instant::now(),
            restore_history: false,
            status: "Preparing your local model…".to_owned(),
            runtime_description: config
                .map_or_else(|| "Model-free preview".to_owned(), runtime_description),
            warning: false,
            diagnostics: VecDeque::new(),
            show_diagnostics: false,
            show_settings: false,
            settings_view: settings::View::default(),
            appearance: appearance::Preferences::default(),
            appearance_store: None,
            profile_store: None,
            sidebar_open: false,
            sidebar_focus: false,
            help: None,
            extra_effort: false,
            enable_thinking: false,
            output_budget: 3,
            abliteration_scale: client::DEFAULT_ABLITERATION_SCALE,
            abliteration_enabled: config.is_none_or(|config| config.use_abliterate_lora),
            focus_composer: false,
            composing: false,
            composer_id: None,
            composer_cursor: None,
            pending_cursor: None,
            speech,
            speech_checked,
            speech_input: false,
            audio_file_input: false,
            audio_import: None,
            audio_file_generation: 0,
            audio_file_active: false,
            native_owner: None,
            audio_paste: None,
            speech_output: false,
            canary_configured: config.is_some_and(|config| config.canary.is_some()),
            tts_configured: config.is_some_and(|config| config.qwen_tts.is_some()),
            speech_status: "Select Microphone or a response's speaker button for setup.".to_owned(),
            speech_notice: false,
            voice_profile: DEFAULT_VOICE.to_owned(),
            listening: false,
            dictation_requested: false,
            dictation_finishing: false,
            pending_voice_transcript: None,
            microphone,
            microphone_generation: None,
            speaking: false,
            speaking_message: None,
            streaming_read_aloud: false,
            streaming_active: false,
            streaming_suppressed: false,
            answer_complete: false,
            last_answer: String::new(),
            retained_draft: String::new(),
            stage: None,
            last_theme_check: Instant::now()
                .checked_sub(Duration::from_secs(2))
                .unwrap_or_else(Instant::now),
        }
    }

    fn pending_startup(
        commands: Sender<Command>,
        events: client::EventReceiver,
        request: crate::startup::Request,
    ) -> Self {
        let mut state = Self::create(commands, events, None, None);
        state.updates = updates::Updates::for_startup(&request);
        state.preview = false;
        state.canary_configured = request.canary_configured;
        state.tts_configured = request.tts_configured;
        state.speech_checked = !state.canary_configured && !state.tts_configured;
        if !state.speech_checked {
            "Preparing speech services…".clone_into(&mut state.speech_status);
        }
        "Preparing runtime selection…".clone_into(&mut state.runtime_description);
        state.model = ModelLifecycle::Preparing;
        state.abliteration_enabled = false;
        state.startup_request = Some(request);
        state.set_status(
            "Preparing your local model… You can draft a message now.",
            false,
        );
        state
    }

    fn poll_startup(&mut self) {
        if let Some(request) = self.startup_request.take() {
            match crate::startup::Session::start(request) {
                Ok(session) => self.startup = Some(session),
                Err(error) => self.fail_startup(&error),
            }
        }
        let Some(result) = self
            .startup
            .as_ref()
            .and_then(crate::startup::Session::poll)
        else {
            return;
        };
        match result {
            Ok(crate::startup::Progress::Model {
                config,
                speech_pending,
            }) => {
                if !speech_pending {
                    self.startup.take();
                }
                if let Err(error) = self.attach_configuration(config, speech_pending) {
                    self.startup.take();
                    self.fail_startup(&error);
                }
            }
            Ok(crate::startup::Progress::Speech(result)) => {
                self.startup.take();
                match result {
                    Ok(config) => self.attach_speech_configuration(&config),
                    Err(error) => self.fail_speech_preparation(&error),
                }
            }
            Err(error) => {
                self.startup.take();
                if self.worker.is_some() {
                    self.fail_speech_preparation(&error);
                } else {
                    self.fail_startup(&error);
                }
            }
        }
    }

    fn fail_speech_preparation(&mut self, error: &io::Error) {
        self.speech_checked = true;
        "Speech preparation failed. Close and relaunch to retry speech."
            .clone_into(&mut self.speech_status);
        self.record(&self.speech_status.clone());
        self.record(&error.to_string());
    }

    fn fail_startup(&mut self, error: &io::Error) {
        self.speech_checked = true;
        if let Some(failure) = &self.startup_failure
            && let Ok(mut failure) = failure.lock()
        {
            *failure = Some(error.to_string());
        }
        self.fail("Preparation failed. Close this window and launch again. Your draft is still available to copy.");
        self.record(&error.to_string());
    }

    fn attach_configuration(&mut self, config: Config, speech_pending: bool) -> io::Result<()> {
        self.attach_configuration_with(config, speech_pending, |config, incoming, events| {
            std::thread::Builder::new()
                .name("swift-chat-client".to_owned())
                .spawn(move || client::run(config, incoming, events))
        })
    }

    fn attach_configuration_with(
        &mut self,
        config: Config,
        speech_pending: bool,
        start: impl FnOnce(Config, Receiver<Command>, client::EventSender) -> io::Result<JoinHandle<()>>,
    ) -> io::Result<()> {
        let (commands, incoming) = mpsc::channel();
        let (events, incoming_events) = client::event_channel();
        self.abliteration_enabled = config.use_abliterate_lora;
        self.runtime_description = runtime_description(&config);
        if !speech_pending {
            self.attach_speech_configuration(&config);
        }
        self.worker = Some(start(config, incoming, events)?);
        self.commands = commands;
        self.events = incoming_events;
        self.set_status("Preparing your local model…", false);
        Ok(())
    }

    fn attach_speech_configuration(&mut self, config: &Config) {
        self.canary_configured = config.canary.is_some();
        self.tts_configured = config.qwen_tts.is_some();
        self.speech = speech::start(config.canary.clone(), config.qwen_tts.clone()).ok();
        self.speech_checked = self.speech.is_none();
        if self.speech_checked && self.speaking {
            self.speaking = false;
            self.speaking_message = None;
        }
        self.microphone = microphone::Microphone::new().ok();
    }
    fn set_status(&mut self, message: &str, warning: bool) {
        self.record(message);
        message
            .split("Protected credentials and technical evidence:")
            .next()
            .unwrap_or(message)
            .trim()
            .clone_into(&mut self.status);
        self.warning = warning;
    }

    fn record(&mut self, message: &str) {
        if self
            .diagnostics
            .back()
            .is_none_or(|previous| previous != message)
        {
            self.diagnostics
                .push_back(message.chars().take(MAX_DIAGNOSTIC_CHARS).collect());
            self.diagnostics.retain_back(MAX_DIAGNOSTICS);
        }
    }

    fn submit(&mut self) {
        if self.preview {
            self.set_status(
                "Preview only. Launch the app normally to chat with your local model.",
                false,
            );
            return;
        }
        if self.model != ModelLifecycle::Ready || self.input.trim().is_empty() {
            return;
        }
        let text = self.input.trim().to_owned();
        self.stop_speech();
        self.last_answer.clear();
        self.retained_draft.clear();
        self.stage = None;
        self.answer_complete = false;
        let max_completion_tokens = OUTPUT_TOKENS
            .get(self.output_budget)
            .copied()
            .unwrap_or(4096);
        if self
            .commands
            .send(Command::Send {
                text: text.clone(),
                max_completion_tokens,
                extra_effort: self.extra_effort,
                enable_thinking: self.enable_thinking,
                abliteration_scale: self.abliteration_scale,
            })
            .is_err()
        {
            self.fail("The model worker stopped. Close and restart the app.");
            return;
        }
        if let Some(conversation) = self
            .conversations
            .iter_mut()
            .find(|conversation| conversation.id == self.active_conversation)
            && !conversation.renamed
            && self.transcript.messages.is_empty()
        {
            conversation.title = text
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(48)
                .collect();
        }
        self.transcript.start_turn(text, self.committed.len());
        self.request_conversation = Some(self.active_conversation);
        self.recovery_dirty = true;
        self.input.clear();
        self.place_composer_cursor(0);
        self.model = ModelLifecycle::Generating;
        // A turn begun without speech stays silent even if preparation finishes
        // mid-answer. Only an explicit read or a later turn may start playback.
        self.streaming_suppressed = !self.speech_output;
        self.set_status("Responding…", false);
    }

    fn new_chat(&mut self) {
        if !self.model.workspace_available() {
            return;
        }
        let Some(next) = self.next_conversation.checked_add(1) else {
            self.conversation_notice =
                Some("Cannot create another conversation in this workspace.");
            return;
        };
        self.update_worker_conversation(Command::NewChat);
        self.sidebar_open = false;
        self.stop_speech();
        self.save_conversation();
        self.conversation_notice = None;
        self.conversation_search.clear();
        let id = self.next_conversation;
        self.next_conversation = next;
        self.conversations.push(Conversation::new(id));
        self.active_conversation = id;
        self.clear_conversation();
        self.recovery_dirty = true;
        if self.model == ModelLifecycle::Ready {
            self.set_status("Ready for a new conversation.", false);
        }
    }

    fn retry(&mut self) {
        if !self.model.retryable()
            || self
                .updates
                .as_ref()
                .is_some_and(updates::Updates::installing)
        {
            return;
        }
        if self.commands.send(Command::Retry).is_err() {
            self.fail("The model worker stopped. Close and restart the app.");
            return;
        }
        self.stop_speech();
        self.model = ModelLifecycle::Restarting;
        self.set_status("Restarting the local model…", false);
    }

    fn stop_generation(&mut self) {
        if !self.model.busy() {
            return;
        }
        if self.commands.send(Command::Stop).is_err() {
            self.fail("The model worker stopped. Close and restart the app.");
            return;
        }
        self.stop_speech();
        self.with_request_conversation(|state| {
            state.model = ModelLifecycle::Stopping;
            state.transcript.remove_empty_response();
            state.set_status("Stopping the response…", false);
        });
    }

    fn handle_escape(&mut self, ctx: &egui::Context) {
        // Voice cancellation takes effect before overlays consume Escape. Keep
        // the key available so the same action also dismisses Find or a modal.
        if self.dictation_requested
            && ctx.input(|input| {
                input.events.iter().any(|event| {
                    matches!(event, egui::Event::Key {
                        key: egui::Key::Escape,
                        pressed: true,
                        modifiers,
                        ..
                    } if *modifiers == egui::Modifiers::NONE)
                })
            })
        {
            self.cancel_microphone();
            "Voice message cancelled.".clone_into(&mut self.speech_status);
        }
        if self.rename_conversation.is_some()
            || self.delete_conversation.is_some()
            || self.edit_message.is_some()
            || self.show_settings
            || egui::Popup::is_any_open(ctx)
        {
            return;
        }
        if self.conversation_search.clear_on_escape(ctx) {
            return;
        }
        if self.sidebar_open && ctx.content_rect().width() < navigation::DOCK_THRESHOLD {
            return;
        }
        if self.transcript.close_find_on_escape(ctx) {
            self.focus_composer = true;
        } else if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            if self.model.busy() {
                self.stop_generation();
            } else if self.speaking || self.dictation_requested {
                self.stop_speech();
            }
        }
    }

    fn handle_input_shortcuts(&mut self, ctx: &egui::Context) {
        if self.dictation_requested && ctx.input(|input| input.viewport().close_requested()) {
            self.cancel_microphone();
        }
        ctx.input(|input| {
            for event in &input.events {
                match event {
                    egui::Event::Ime(egui::ImeEvent::Preedit { text, .. }) => {
                        self.composing = !text.is_empty();
                    }
                    egui::Event::Ime(egui::ImeEvent::Commit(_)) => self.composing = false,
                    _ => {}
                }
            }
        });
        if self.rename_conversation.is_none()
            && self.delete_conversation.is_none()
            && self.edit_message.is_none()
            && !self.show_settings
            && ctx.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::N))
        {
            self.new_chat();
        }
        if !self.show_settings
            && self.rename_conversation.is_none()
            && self.delete_conversation.is_none()
            && self.edit_message.is_none()
            && !egui::Popup::is_any_open(ctx)
        {
            // Run before the sidebar and composer text editors consume Ctrl+F.
            if ctx.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::F)) {
                self.sidebar_open = false;
                self.sidebar_focus = false;
                self.focus_composer = false;
                self.transcript.focus_find();
                ctx.request_repaint();
            }
            if ctx.input_mut(|input| {
                input.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::B)
            }) {
                self.toggle_navigation(ctx);
            }
        }
        self.handle_escape(ctx);
    }

    fn retain_draft(&mut self) {
        if !self.retained_draft.is_empty() {
            self.last_answer.clone_from(&self.retained_draft);
            self.answer_complete = true;
        }
    }

    fn fail(&mut self, message: &str) {
        if matches!(
            self.model,
            ModelLifecycle::Generating
                | ModelLifecycle::Stopping
                | ModelLifecycle::AwaitingTeardown
        ) {
            self.with_request_conversation(|state| state.fail_current_conversation(message));
        } else {
            self.fail_current_conversation(message);
        }
    }

    fn fail_current_conversation(&mut self, message: &str) {
        if !self.routing_request {
            self.stop_speech();
        }
        let adopted = matches!(
            self.model,
            ModelLifecycle::Generating
                | ModelLifecycle::Stopping
                | ModelLifecycle::AwaitingTeardown
        ) && self.adopt_interrupted();
        self.model = ModelLifecycle::Unavailable;
        self.transcript.remove_empty_response();
        if !adopted && !self.transcript.latest_interrupted(&self.committed) {
            self.retain_draft();
        }
        self.set_status(message, true);
    }

    fn reject(&mut self, message: &str, text: &str, retry_required: bool) {
        if !self.routing_request
            && let Some(id) = self.request_conversation
            && id != self.active_conversation
        {
            // A worker can reject a queued Send after announcing teardown.
            // Restore its prompt only to the conversation that sent it.
            if let Some(saved) = self
                .conversations
                .iter_mut()
                .find(|conversation| conversation.id == id)
                .and_then(|conversation| conversation.saved.as_mut())
                && saved.input.is_empty()
            {
                saved.input = text.chars().take(INPUT_LIMIT).collect();
            }
            return;
        }
        let adopted = retry_required && self.adopt_interrupted();
        if !retry_required {
            self.transcript.clear_turn(self.committed.len());
        }
        if self.model.stopping() || self.model == ModelLifecycle::Restarting {
            // A rejection can queue just before Stop or behind the first
            // teardown notice. Preserve explicit Stop/retry ownership until
            // teardown ends or the worker acknowledges the new attempt.
            if !adopted && self.input.is_empty() {
                self.input = text.chars().take(INPUT_LIMIT).collect();
                self.place_composer_cursor(self.input.chars().count());
            }
            if !adopted {
                self.retain_draft();
            }
            return;
        }
        if !self.routing_request {
            self.stop_speech();
        }
        self.model = if retry_required {
            if self.model.retryable() {
                ModelLifecycle::Retryable
            } else {
                ModelLifecycle::AwaitingTeardown
            }
        } else {
            ModelLifecycle::Ready
        };
        self.transcript.remove_empty_response();
        if !adopted && self.input.is_empty() {
            self.input = text.chars().take(INPUT_LIMIT).collect();
            self.place_composer_cursor(self.input.chars().count());
        }
        if !adopted {
            self.retain_draft();
        }
        self.set_status(if adopted { "Response interrupted. The partial reply is included in this conversation; retry the model to continue." } else { message }, true);
        self.focus_composer = !retry_required;
    }

    fn drain_events(&mut self) {
        if self.worker.is_none() && (self.startup_request.is_some() || self.startup.is_some()) {
            return;
        }
        // Bound per-frame work even when the worker refills the bounded queue.
        for _ in 0..64 {
            match self.events.try_recv() {
                Ok(event) => self.model_event(event),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    if !self.preview
                        && (self.model.ready() || self.model.busy() || self.model.waiting_model())
                    {
                        self.fail("The model worker stopped. Close and restart the app.");
                    }
                    break;
                }
            }
        }
    }

    fn model_event(&mut self, event: Event) {
        if !self.model.accepts(&event) {
            return;
        }
        if matches!(
            self.model,
            ModelLifecycle::Generating
                | ModelLifecycle::Stopping
                | ModelLifecycle::AwaitingTeardown
        ) || matches!(event, Event::Rejected { .. })
        {
            self.with_request_conversation(|state| state.apply_model_event(event));
        } else {
            self.apply_model_event(event);
        }
        self.restore_selected_history();
    }

    fn apply_model_event(&mut self, event: Event) {
        self.recovery_dirty = true;
        match event {
            Event::Starting => {
                self.model = ModelLifecycle::Preparing;
                self.set_status("Preparing your local model…", false);
            }
            Event::Retryable(message) => {
                self.fail(&message);
                self.model = ModelLifecycle::Retryable;
            }
            Event::Stopping(text) => self.stopping_response(&text),
            Event::Stopped => self.stopped_response(),
            Event::Ready => {
                self.model = ModelLifecycle::Ready;
                self.set_status("Ready when you are.", false);
            }
            Event::Rejected {
                message,
                text,
                retry_required,
            } => self.reject(&message, &text, retry_required),
            Event::Stage(label) => {
                self.replace_streaming_stage();
                self.stage = Some(label);
                self.last_answer.clear();
                self.answer_complete = false;
                self.transcript.stage();
                self.set_status(label, false);
            }
            Event::Draft(text) => self.retained_draft = text,
            Event::Text(text) | Event::RetainedAnswer(text) => {
                self.last_answer.push_str(&text);
                self.transcript.append(&text, false);
                self.stream_answer(&text);
                self.stage
                    .unwrap_or("Writing…")
                    .clone_into(&mut self.status);
            }
            Event::Reasoning(text) => self.transcript.append(&text, true),
            Event::Committed(history) => self.commit_history(history),
            Event::Complete(completion) => {
                if self.streaming_active {
                    self.speech_command(speech::Command::FinishStream);
                }
                self.transcript.remove_empty_response();
                self.model = ModelLifecycle::Ready;
                self.answer_complete = true;
                let message = match completion {
                    Completion::Finished => "Ready when you are.",
                    Completion::TokenLimit
                        if !self.retained_draft.is_empty()
                            && self.last_answer == self.retained_draft =>
                    {
                        "Review did not finish. Your complete draft is retained."
                    }
                    Completion::TokenLimit => {
                        "Response reached the selected output cap, including thinking. You can ask Swift to continue."
                    }
                };
                self.set_status(message, completion == Completion::TokenLimit);
            }
            Event::Failed(message) => self.fail(&message),
        }
    }

    fn commit_history(&mut self, history: Vec<client::Message>) {
        if self.model.stopping()
            && let Some(answer) = history.last().filter(|answer| answer.role == "assistant")
        {
            // A fully completed turn can win just before the stop click. Keep
            // its visible answer consistent with the authoritative history,
            // without reopening Send or overwriting the cleanup status.
            self.transcript
                .replace_response(&answer.content, answer.reasoning_content.as_deref());
            answer.content.as_ref().clone_into(&mut self.last_answer);
            self.answer_complete = true;
        }
        self.transcript.bind_committed(&self.committed, &history);
        self.committed = history;
        self.context_history = None;
    }

    fn stopping_response(&mut self, text: &str) {
        if !self.routing_request {
            self.stop_speech();
        }
        self.model = ModelLifecycle::Stopping;
        let adopted = self.adopt_interrupted();
        if !adopted && self.input.is_empty() {
            self.input = text.chars().take(INPUT_LIMIT).collect();
            self.place_composer_cursor(self.input.chars().count());
        }
        self.transcript.remove_empty_response();
        if !adopted {
            self.retain_draft();
        }
        self.set_status("Response stopped. Releasing the local model…", false);
    }

    fn stopped_response(&mut self) {
        self.model = ModelLifecycle::Retryable;
        self.set_status(client::STOPPED_MESSAGE, false);
        self.focus_composer = true;
    }

    fn adopt_interrupted(&mut self) -> bool {
        if self.transcript.adopt_interrupted(&mut self.committed) {
            if let Some(answer) = self.committed.last() {
                self.last_answer = answer.content.to_string();
                self.answer_complete = false;
            }
            self.restore_history = true;
            self.context_history = None;
            self.recovery_dirty = true;
            true
        } else {
            self.transcript.clear_turn(self.committed.len());
            false
        }
    }

    fn speech_command(&mut self, command: speech::Command) -> bool {
        if self
            .speech
            .as_ref()
            .is_some_and(|session| session.commands.send(command).is_ok())
        {
            return true;
        }
        self.speech_input = false;
        self.audio_file_input = false;
        self.speech_output = false;
        self.cancel_microphone();
        self.speaking = false;
        self.speaking_message = None;
        self.streaming_active = false;
        self.streaming_suppressed = true;
        "Speech worker stopped. Restart the app to retry.".clone_into(&mut self.speech_status);
        false
    }

    fn stop_speech(&mut self) {
        let was_active = self.speaking || self.dictation_requested || self.listening;
        self.cancel_microphone();
        if let Some(session) = &self.speech {
            let _result = session.commands.send(speech::Command::StopSpeaking);
        }
        self.listening = false;
        self.dictation_requested = false;
        self.speaking = false;
        self.speaking_message = None;
        self.streaming_active = false;
        self.streaming_suppressed = true;
        if was_active {
            "Speech stopped. Microphone off.".clone_into(&mut self.speech_status);
            self.speech_notice = true;
        }
    }

    fn cancel_microphone(&mut self) {
        if let Some(import) = &self.audio_import {
            import.cancel();
        }
        self.audio_file_active = false;
        if let Some(session) = &self.speech {
            session.set_input_generation(None);
            let _result = session.commands.send(speech::Command::StopListening);
        }
        if let Some(microphone) = &mut self.microphone {
            microphone.stop();
        }
        self.microphone_generation = None;
        self.dictation_requested = false;
        self.dictation_finishing = false;
        self.pending_voice_transcript = None;
        self.listening = false;
    }

    fn toggle_dictation(&mut self) {
        self.speech_notice = true;
        if self.dictation_requested {
            self.finish_dictation();
        } else if !self.speech_input && (self.speech_checked || !self.canary_configured) {
            let message = self.microphone_help();
            message.clone_into(&mut self.speech_status);
            self.help = Some(("Microphone setup", message));
        } else if !self.speech_input {
            "Enable the microphone after speech preparation completes."
                .clone_into(&mut self.speech_status);
        } else if !self.model.busy() && !self.speaking {
            let Some(microphone) = &mut self.microphone else {
                return;
            };
            match microphone.start() {
                Ok(generation) => {
                    if let Some(session) = &self.speech {
                        session.set_input_generation(Some(generation));
                    }
                    self.microphone_generation = Some(generation);
                    self.dictation_requested = true;
                    "Requesting microphone permission…".clone_into(&mut self.speech_status);
                    self.focus_composer = true;
                }
                Err(_) => {
                    "Microphone permission is unavailable. Check Windows privacy settings."
                        .clone_into(&mut self.speech_status);
                }
            }
        }
        self.record(&self.speech_status.clone());
    }

    fn finish_dictation(&mut self) {
        if self.dictation_finishing {
            self.cancel_microphone();
            "Voice message cancelled.".clone_into(&mut self.speech_status);
            return;
        }
        match self.microphone.as_mut().map(microphone::Microphone::finish) {
            Some(Ok(true)) => {
                self.dictation_finishing = true;
                "Finishing voice message… Escape cancels.".clone_into(&mut self.speech_status);
            }
            Some(Err(_)) => {
                self.cancel_microphone();
                "Could not finish microphone capture. Try again."
                    .clone_into(&mut self.speech_status);
            }
            Some(Ok(false)) | None => {
                self.cancel_microphone();
                "Microphone off. No voice message sent.".clone_into(&mut self.speech_status);
            }
        }
    }

    fn toggle_read_aloud(&mut self, message_id: u64) {
        if self.speaking && self.speaking_message == Some(message_id) {
            self.stop_speech();
            return;
        }
        if self.transcript.speech_text(message_id).is_none() {
            return;
        }
        self.speech_notice = true;
        if self.speech_output {
            let voice_description = self.voice_profile.trim().to_owned();
            if voice_description.is_empty() {
                "Describe the voice in Settings before reading aloud."
                    .clone_into(&mut self.speech_status);
                self.open_settings(settings::Page::Voice);
                self.record(&self.speech_status.clone());
                return;
            }
            let Some(text) = self.transcript.speech_text(message_id).map(str::to_owned) else {
                return;
            };
            self.stop_speech();
            if self.speech_command(speech::Command::Speak {
                text,
                voice_description,
            }) {
                self.speaking = true;
                self.speaking_message = Some(message_id);
                "Reading the selected response aloud.".clone_into(&mut self.speech_status);
            }
        } else if !self.speech_checked && self.tts_configured {
            "Read aloud will be available when speech preparation completes."
                .clone_into(&mut self.speech_status);
        } else {
            let message = self.read_aloud_help();
            message.clone_into(&mut self.speech_status);
            self.help = Some(("Read aloud setup", message));
        }
        self.record(&self.speech_status.clone());
    }

    const fn microphone_help(&self) -> &'static str {
        if !self.canary_configured {
            "Launch bonsai-tools chat with its default speech settings to prepare Qwen3-ASR 1.7B automatically. Click the microphone to record, then click again to transcribe and send your message. Windows microphone permission is required. See tools/canary-asr/README.md. Nothing is recording now."
        } else if self.microphone.is_none() {
            "Windows microphone access could not initialize. Check Windows microphone privacy settings and restart the app. Nothing is recording."
        } else {
            "Qwen3-ASR 1.7B is not ready. Start its service on the configured port, check its protected key file, then restart the app. See tools/canary-asr/README.md. Nothing is recording."
        }
    }

    const fn read_aloud_help(&self) -> &'static str {
        if self.tts_configured {
            "Qwen3-TTS VoiceDesign is not ready. Start its service on the configured port, check its protected key file, then restart the app. See tools/qwen-tts/README.md. Your voice description will be used when the service is ready."
        } else {
            "Launch the app with bonsai-tools chat to prepare Qwen3-TTS 1.7B VoiceDesign automatically. Use its default settings without --no-tts or --no-speech-models. Selecting a response's speaker button starts the local service. Describe your desired voice in Voice profile; no preset or reference recording is required. See tools/qwen-tts/README.md."
        }
    }

    fn drain_speech_events(&mut self) {
        for _ in 0..32 {
            let Some(session) = &self.speech else { break };
            match session.events.try_recv() {
                Ok(event) => self.speech_event(event),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.speech_checked = true;
                    self.speech_input = false;
                    self.audio_file_input = false;
                    self.speech_output = false;
                    self.cancel_microphone();
                    self.speaking = false;
                    self.speaking_message = None;
                    self.streaming_active = false;
                    self.streaming_suppressed = true;
                    "Speech unavailable. Text chat remains available."
                        .clone_into(&mut self.speech_status);
                    break;
                }
            }
        }
    }

    fn speech_event(&mut self, event: speech::Event) {
        let previous = self.speech_status.clone();
        match event {
            speech::Event::Capabilities { input, output } => {
                self.speech_checked = true;
                self.speech_input = input && self.microphone.is_some();
                self.audio_file_input = input;
                self.speech_output = output;
                match (self.speech_input, output) {
                    (true, true) => "Microphone and read aloud are ready.",
                    (true, false) => "Microphone ready. Open Settings for read aloud setup.",
                    (false, true) => "Response speakers ready. Select Microphone for setup.",
                    (false, false) => {
                        "Speech setup required. Select Microphone or open Settings for instructions."
                    }
                }
                .clone_into(&mut self.speech_status);
                if !output && self.speaking {
                    self.speaking = false;
                    self.speaking_message = None;
                }
            }
            speech::Event::Listening(listening) => {
                self.listening = listening && self.dictation_requested;
                if self.listening {
                    "Recording. Click the microphone to transcribe and send; Escape cancels."
                } else if self.dictation_requested {
                    "Microphone enabled."
                } else {
                    "Microphone off."
                }
                .clone_into(&mut self.speech_status);
            }
            speech::Event::Recognized { generation, text } => {
                if self.dictation_requested
                    && self.dictation_finishing
                    && self.microphone_generation == Some(generation)
                {
                    // User input in this frame must settle before this can send.
                    self.pending_voice_transcript = Some((generation, text));
                }
            }
            speech::Event::Speaking {
                generation,
                speaking,
            } => self.output_status(
                generation,
                speaking,
                if speaking {
                    if self.streaming_active {
                        "Reading the response as it arrives."
                    } else {
                        "Reading the selected response aloud."
                    }
                } else {
                    "Read aloud stopped."
                },
            ),
            speech::Event::Failed {
                generation,
                message,
            } => self.output_status(generation, false, message),
            speech::Event::OutputStatus {
                generation,
                message,
            } => self.output_status(generation, true, message),
            speech::Event::InputFailed {
                generation,
                message,
            } => {
                if self.dictation_requested && self.microphone_generation == Some(generation) {
                    self.cancel_microphone();
                    message.clone_into(&mut self.speech_status);
                    self.speech_notice = true;
                }
            }
            speech::Event::Status(message) if self.dictation_requested => {
                message.clone_into(&mut self.speech_status);
            }
            speech::Event::Status(_) => {}
        }
        if self.speech_status != previous {
            self.record(&self.speech_status.clone());
        }
    }

    fn output_status(&mut self, generation: u64, speaking: bool, message: &str) {
        if self
            .speech
            .as_ref()
            .is_some_and(|session| session.is_current_output(generation))
        {
            self.speaking = speaking;
            if !speaking {
                self.speaking_message = None;
                self.streaming_active = false;
                self.streaming_suppressed = true;
            }
            message.clone_into(&mut self.speech_status);
        }
    }

    fn accept_transcript(&mut self, generation: u64, text: &str) {
        if !self.dictation_requested
            || self.microphone_generation != Some(generation)
            || !self.dictation_finishing
        {
            return;
        }
        let from_file = self.audio_file_active;
        self.cancel_microphone();
        let text = text.trim();
        if text.is_empty() {
            "No speech recognized. Your draft was kept.".clone_into(&mut self.speech_status);
            return;
        }
        let complete = self.insert_dictation(text);
        if !complete {
            "Voice message exceeds the prompt limit. Review the draft before sending."
                .clone_into(&mut self.speech_status);
        } else if self.model.ready()
            && !self.composing
            && self.rename_conversation.is_none()
            && self.delete_conversation.is_none()
            && self.edit_message.is_none()
        {
            self.submit();
            if self.model.busy() {
                if from_file {
                    "Audio file transcribed and sent."
                } else {
                    "Voice message transcribed and sent."
                }
                .clone_into(&mut self.speech_status);
            }
        } else {
            if from_file {
                "Audio file transcribed. Select Send when the model is ready."
            } else {
                "Voice message transcribed. Select Send when the model is ready."
            }
            .clone_into(&mut self.speech_status);
        }
        self.focus_composer = true;
    }

    fn flush_voice_transcript(&mut self, ctx: &egui::Context) {
        // A press can become a Cancel or conversation switch on release. Leave
        // the transcript staged until the native widgets have handled that click.
        if ctx.input(|input| input.pointer.any_down()) {
            return;
        }
        if let Some((generation, text)) = self.pending_voice_transcript.take() {
            self.accept_transcript(generation, &text);
            ctx.request_repaint();
        }
    }

    fn place_composer_cursor(&mut self, index: usize) {
        self.recovery_dirty = true;
        let cursor = egui::text::CCursorRange::one(egui::text::CCursor::new(index));
        self.composer_cursor = Some(cursor);
        self.pending_cursor = Some(cursor);
    }

    fn insert_dictation(&mut self, text: &str) -> bool {
        let length = self.input.chars().count();
        let range = self.composer_cursor.map_or(length..length, |cursor| {
            let range = cursor.as_sorted_char_range();
            usize::from(range.start).min(length)..usize::from(range.end).min(length)
        });
        let available = INPUT_LIMIT.saturating_sub(length - (range.end - range.start));
        let byte_index = |index| {
            self.input
                .char_indices()
                .nth(index)
                .map_or(self.input.len(), |(byte, _)| byte)
        };
        let bytes = byte_index(range.start)..byte_index(range.end);
        let prefix = if self.input[..bytes.start]
            .chars()
            .next_back()
            .is_some_and(|character| !character.is_whitespace())
        {
            " "
        } else {
            ""
        };
        let suffix = if self.input[bytes.end..]
            .chars()
            .next()
            .is_some_and(|character| !character.is_whitespace())
        {
            " "
        } else {
            ""
        };
        let phrase = format!("{prefix}{text}{suffix}");
        let complete = phrase.chars().count() <= available;
        let phrase: String = phrase.chars().take(available).collect();
        self.input.replace_range(bytes, &phrase);
        self.place_composer_cursor(range.start + phrase.chars().count());
        complete
    }

    fn poll_microphone(&mut self) {
        if self.audio_file_active {
            return;
        }
        let previous = self.speech_status.clone();
        let Some(microphone) = &mut self.microphone else {
            return;
        };
        for event in microphone.poll() {
            match event {
                microphone::Event::Status(message) if self.dictation_requested => {
                    message.clone_into(&mut self.speech_status);
                }
                microphone::Event::Status(_) => {}
                microphone::Event::Failed(message) => {
                    self.cancel_microphone();
                    message.clone_into(&mut self.speech_status);
                }
                microphone::Event::Clip { generation, wav } => {
                    if self.dictation_requested && self.microphone_generation == Some(generation) {
                        self.listening = false;
                        self.dictation_finishing = true;
                        if self.speech_command(speech::Command::Transcribe { generation, wav }) {
                            "Transcribing with Qwen3-ASR 1.7B… Escape cancels."
                                .clone_into(&mut self.speech_status);
                        }
                    }
                }
            }
        }
        if self.speech_status != previous {
            self.record(&self.speech_status.clone());
        }
    }
}

impl Drop for State {
    fn drop(&mut self) {
        if let Some(store) = &mut self.profile_store {
            let _queued = store.queue(&self.voice_profile);
        }
        self.profile_store.take();
        if let Some(store) = &mut self.appearance_store {
            let _queued = store.queue(self.appearance);
        }
        self.appearance_store.take();
        self.audio_paste.take();
        self.updates.take();
        self.startup_request.take();
        self.startup.take();
        self.stop_speech();
        if let Some(recovery) = &self.recovery {
            if self.recovery_erase_pending {
                recovery.finish_erase();
            } else if self.save_conversations {
                recovery.finish_with(|| self.workspace_bytes().ok());
            }
        }
        self.microphone.take();
        self.speech.take();
        let _sent = self.commands.send(Command::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _joined = worker.join();
        }
    }
}

fn configure(ctx: &egui::Context) {
    appearance::configure(ctx);
}

impl State {
    fn sidebar(&mut self, ui: &mut egui::Ui) {
        let previous_height = egui::containers::panel::PanelState::load(
            ui.ctx(),
            ui.make_persistent_id("sidebar-footer"),
        )
        .map(|state| state.size().y);
        let footer = egui::Panel::bottom("sidebar-footer")
            .frame(egui::Frame::new().inner_margin(egui::Margin {
                left: 0,
                right: 0,
                top: 8,
                bottom: 0,
            }))
            .show(ui, |ui| self.sidebar_footer(ui));
        // Settle the measured bottom anchor after first layout or a notice change.
        if previous_height.is_none_or(|height| (height - footer.response.rect.height()).abs() > 0.5)
        {
            ui.ctx().request_repaint();
        }
        ui.add_space(4.0);
        branding::logo(ui);
        ui.add_space(8.0);
        if ui
            .add_enabled(
                self.model.workspace_available(),
                egui::Button::new("+ New conversation")
                    .min_size(egui::vec2(ui.available_width(), 44.0))
                    .corner_radius(10),
            )
            .on_hover_text("Start fresh · Ctrl+N")
            .clicked()
        {
            self.new_chat();
        }
        ui.add_space(8.0);
        self.conversation_list(ui);
    }

    fn sidebar_footer(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.ctx().content_rect().width() >= navigation::DOCK_THRESHOLD {
                self.navigation_button(ui, false);
            }
            if ui
                .add_sized([ui.available_width(), 36.0], egui::Button::new("Settings"))
                .clicked()
            {
                self.open_settings(settings::Page::Response);
            }
        });
        ui.add_space(4.0);
        if self
            .updates
            .as_ref()
            .is_some_and(updates::Updates::available)
        {
            ui.label(RichText::new("Application update available in Settings").small());
        }
        self.sidebar_status(ui);
    }

    fn runtime_controls(&self, ui: &mut egui::Ui) {
        ui.heading("Runtime");
        ui.label(&self.runtime_description);
        ui.label(
            RichText::new("Development build. Full-context, sustained-performance and output-quality qualification remain pending.")
                .small()
                .weak(),
        );
        ui.add_space(16.0);
    }

    const fn can_install_update(&self) -> bool {
        self.startup.is_none()
            && self.startup_request.is_none()
            && matches!(
                self.model,
                ModelLifecycle::Ready | ModelLifecycle::Retryable
            )
    }

    fn update_controls(&mut self, ui: &mut egui::Ui) {
        let can_install = self.can_install_update();
        let Some(updates) = &mut self.updates else {
            ui.heading("Application updates");
            ui.label("Automatic updates are available in the installed Windows application.");
            ui.add_space(16.0);
            return;
        };
        let requested = updates.controls(ui, can_install);
        if self.model == ModelLifecycle::Unavailable {
            ui.label("If model preparation failed, close Swift and install the MSI from GitHub releases.");
        }
        if !requested {
            return;
        }
        updates.wait_for_stop();
        self.stop_speech();
        if self.model == ModelLifecycle::Ready {
            if self.commands.send(Command::Stop).is_err() {
                self.fail("The model worker stopped. Close and restart the app before updating.");
            } else {
                self.model = ModelLifecycle::Stopping;
                self.set_status("Stopping the local model before updating...", false);
            }
        }
    }

    fn poll_updates(&mut self, ctx: &egui::Context) {
        let Some(updates) = &mut self.updates else {
            return;
        };
        if self.model == ModelLifecycle::Retryable {
            updates.finish_stop(true);
        } else if self.model == ModelLifecycle::Unavailable {
            updates.finish_stop(false);
        }
        let diagnostic = updates.poll();
        if updates.should_close() {
            if let Some(approved) = &self.update_exit {
                approved.store(true, std::sync::atomic::Ordering::Release);
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if let Some(message) = diagnostic {
            self.record(&message);
        }
    }

    fn diagnostics(&mut self, ctx: &egui::Context) {
        if !self.show_diagnostics {
            return;
        }
        let mut open = true;
        egui::Window::new("Diagnostics")
            .open(&mut open)
            .default_width(640.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(400.0)
                    .show(ui, |ui| self.diagnostics_contents(ui));
            });
        self.show_diagnostics = open;
    }

    fn conversation_notices(&mut self, ui: &mut egui::Ui) {
        if self.model.busy()
            && let Some(owner) = self.request_conversation
            && owner != self.active_conversation
        {
            ui.horizontal_wrapped(|ui| {
                ui.label(self.generation_status());
                if ui.small_button("Show response").clicked() {
                    self.switch_conversation(owner);
                }
            });
        }
        if let Some(notice) = self.conversation_notice {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(notice).small());
                if ui.small_button("Dismiss").clicked() {
                    self.conversation_notice = None;
                }
            });
        }
        if self.warning || self.model.retryable() {
            ui.horizontal_wrapped(|ui| {
                let color = if self.warning {
                    ui.visuals().warn_fg_color
                } else {
                    ui.visuals().text_color()
                };
                ui.label(RichText::new(&self.status).color(color));
                if self.model.retryable() && ui.button("Retry model").clicked() {
                    self.retry();
                }
                if ui.small_button("Details").clicked() {
                    self.show_diagnostics = true;
                }
            });
        }
        if self.preview {
            ui.label(
                RichText::new("PREVIEW  ·  Launch normally to use your local model.")
                    .small()
                    .weak(),
            );
        }
        if self.recovery_notice.starts_with("Could not") {
            ui.label(RichText::new(&self.recovery_notice).color(ui.visuals().warn_fg_color));
        }
    }

    fn empty_state(&mut self, ui: &mut egui::Ui) {
        ui.add_space((ui.available_height() * 0.18).max(20.0));
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new("B")
                    .size(44.0)
                    .color(ui.visuals().selection.bg_fill),
            );
            ui.add_space(12.0);
            ui.label(RichText::new("What’s on your mind?").strong().size(30.0));
            ui.add_space(8.0);
            ui.label(RichText::new("Write, explore, or work through an idea together.").weak());
            ui.add_space(28.0);
            for (title, prompt) in [
                (
                    "Find the right words",
                    "Help me write a clear, thoughtful message about ",
                ),
                ("Explore an idea", "Help me think through an idea: "),
                ("Make a plan", "Help me create a practical plan for "),
            ] {
                if ui
                    .add(egui::Button::new(title).min_size(egui::vec2(280.0, 42.0)))
                    .clicked()
                {
                    prompt.clone_into(&mut self.input);
                    self.place_composer_cursor(self.input.chars().count());
                    self.focus_composer = true;
                }
            }
        });
    }

    fn microphone_button(&mut self, ui: &mut egui::Ui) {
        let response = ui
            .add_enabled(
                self.dictation_requested || (!self.model.busy() && !self.speaking),
                egui::Button::new("")
                    .min_size(egui::Vec2::splat(32.0))
                    .selected(self.dictation_requested),
            )
            .accessible_name("Microphone")
            .on_hover_text(if self.dictation_finishing {
                "Cancel voice message · Escape"
            } else if self.dictation_requested {
                "Finish recording, transcribe and send · Escape cancels"
            } else {
                "Record a voice message · Qwen3-ASR 1.7B"
            });
        let center = response.rect.center();
        let stroke = ui.style().interact(&response).fg_stroke;
        let painter = ui.painter_at(response.rect);
        if self.dictation_finishing {
            painter.line_segment(
                [center - egui::vec2(5.0, 5.0), center + egui::vec2(5.0, 5.0)],
                stroke,
            );
            painter.line_segment(
                [
                    center + egui::vec2(-5.0, 5.0),
                    center + egui::vec2(5.0, -5.0),
                ],
                stroke,
            );
        } else if self.dictation_requested {
            painter.rect_filled(
                egui::Rect::from_center_size(center, egui::Vec2::splat(12.0)),
                2.0,
                stroke.color,
            );
        } else {
            painter.rect_stroke(
                egui::Rect::from_center_size(center - egui::vec2(0.0, 3.0), egui::vec2(7.0, 12.0)),
                4.0,
                stroke,
                egui::StrokeKind::Middle,
            );
            let cradle = [
                (-7.0, -2.0),
                (-7.0, 0.0),
                (-6.5, 2.7),
                (-5.0, 5.0),
                (-2.7, 6.5),
                (0.0, 7.0),
                (2.7, 6.5),
                (5.0, 5.0),
                (6.5, 2.7),
                (7.0, 0.0),
                (7.0, -2.0),
            ]
            .map(|(x, y)| center + egui::vec2(x, y));
            painter.add(egui::Shape::line(cradle.to_vec(), stroke));
            painter.line_segment(
                [
                    center + egui::vec2(0.0, 7.0),
                    center + egui::vec2(0.0, 10.0),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    center + egui::vec2(-4.0, 10.0),
                    center + egui::vec2(4.0, 10.0),
                ],
                stroke,
            );
        }
        if response.clicked() {
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
                    egui::Button::new(
                        RichText::new("Send  ↑").color(ui.visuals().selection.stroke.color),
                    )
                    .fill(ui.visuals().selection.bg_fill),
                )
                .on_hover_text("Send message · Enter")
                .clicked()
            {
                self.submit();
            }
            self.microphone_button(ui);
        });
    }

    #[expect(
        clippy::too_many_lines,
        reason = "Keep composer focus, IME and action dispatch together."
    )]
    fn composer(&mut self, ui: &mut egui::Ui) {
        let width = ui.available_width().min(self.appearance.column_width());
        let margin = ((ui.available_width() - width) * 0.5).max(0.0);
        ui.horizontal(|ui| {
            ui.add_space(margin);
            ui.vertical(|ui| {
                ui.set_width(width);
                egui::Frame::new()
                    .fill(ui.visuals().faint_bg_color)
                    .corner_radius(14)
                    .inner_margin(14)
                    .show(ui, |ui| {
                        let label = self.context_toolbar(ui);
                        // A pasted manuscript must not push the transcript and
                        // Send/Stop controls outside the window. TextEdit keeps
                        // the caret visible within this native scroll area.
                        let editor = egui::ScrollArea::vertical()
                            .id_salt(("composer-scroll", self.active_conversation))
                            .max_height(
                                (ui.ctx().content_rect().height() * 0.24).clamp(64.0, 160.0),
                            )
                            .show(ui, |ui| {
                                let output = ui
                                    .scope(|ui| {
                                        egui::TextEdit::multiline(&mut self.input)
                                            .font(egui::FontId::proportional(
                                                appearance::text_size(ui.ctx()),
                                            ))
                                            .id_salt("composer")
                                            .hint_text(if self.model.busy() {
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
                                            })
                                            .desired_rows(3)
                                            .desired_width(f32::INFINITY)
                                            .frame(egui::Frame::NONE)
                                            .char_limit(INPUT_LIMIT)
                                            .return_key(Some(egui::KeyboardShortcut::new(
                                                egui::Modifiers::SHIFT,
                                                egui::Key::Enter,
                                            )))
                                            .show(ui)
                                    })
                                    .inner;
                                let response = output.response.response.labelled_by(label.id);
                                self.recovery_dirty |= response.changed();
                                self.composer_id = Some(response.id);
                                let pending_cursor = self.pending_cursor.take();
                                let cursor = pending_cursor.or(output.cursor_range);
                                if let Some(mut state) =
                                    egui::TextEdit::load_state(ui.ctx(), response.id)
                                {
                                    if let Some(cursor) = pending_cursor {
                                        state.cursor.set_char_range(Some(cursor));
                                        state.store(ui.ctx(), response.id);
                                        ui.ctx().request_repaint();
                                    } else {
                                        self.composer_cursor = state.cursor.char_range();
                                    }
                                }
                                let focus_requested = self.focus_composer && !self.show_settings;
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
                                // Focusing a recovered draft can leave the cursor
                                // already at the end, so egui sees no cursor change
                                // and does not request its usual automatic scroll.
                                if (response.has_focus() || focus_requested)
                                    && (response.gained_focus()
                                        || focus_requested
                                        || pending_cursor.is_some()
                                        || ui.input(|input| {
                                            input.key_pressed(egui::Key::End)
                                                || input.key_pressed(egui::Key::Home)
                                        }))
                                    && let Some(caret) = caret
                                {
                                    ui.scroll_to_rect(caret, None);
                                }
                                (response, caret)
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
                        let response = editor.inner.0;
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
                                plain_enter
                                    && input.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                            })
                        {
                            self.submit();
                        }
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
                    });
                // Text-only status should grow with its contents, without
                // reserving the horizontal helper's button-height row.
                let status = if self.model.busy() {
                    self.generation_status()
                } else if !self.model.ready() {
                    self.status.clone()
                } else {
                    "Enter to send · Shift+Enter for a new line".to_owned()
                };
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), 0.0),
                    egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true),
                    |ui| {
                        ui.label(RichText::new(&status).small().weak());
                        if self.help.is_none()
                            && (self.speech_notice || self.dictation_requested || self.speaking)
                        {
                            ui.label(RichText::new(&self.speech_status).small());
                        }
                    },
                );
            });
        });
    }

    fn show_transcript(&mut self, ui: &mut egui::Ui, width: f32) {
        let available = !self
            .updates
            .as_ref()
            .is_some_and(updates::Updates::installing);
        let controls = transcript::MessageControls {
            history: &self.committed,
            can_send: available && !self.preview && self.model == ModelLifecycle::Ready,
            can_branch: available && self.model.workspace_available(),
        };
        match self.transcript.show_with_actions(
            ui,
            width,
            self.speaking_message,
            self.speech_output,
            Some(controls),
        ) {
            Some(transcript::TranscriptAction::ReadAloud(id)) => self.toggle_read_aloud(id),
            Some(transcript::TranscriptAction::Message(action)) => self.message_action(action),
            None => {}
        }
    }

    fn draw_workspace(&mut self, ui: &mut egui::Ui) {
        let narrow = ui.available_width() < navigation::DOCK_THRESHOLD;
        self.docked_navigation(ui, narrow);
        let horizontal_margin = if narrow { 12 } else { 24 };
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(horizontal_margin, 12)))
            .show(ui, |ui| {
                self.conversation_notices(ui);
                egui::Panel::bottom("composer-panel")
                    .frame(egui::Frame::new().inner_margin(egui::Margin {
                        left: 0,
                        right: 0,
                        top: 12,
                        bottom: 4,
                    }))
                    .show(ui, |ui| self.composer(ui));
                let empty = self.transcript.messages.is_empty();
                let closed_find = ui
                    .push_id(("conversation-find", self.active_conversation), |ui| {
                        self.transcript.search_bar(ui)
                    })
                    .inner;
                if closed_find {
                    self.focus_composer = true;
                    ui.ctx().request_repaint();
                }
                let scroll = egui::ScrollArea::vertical()
                    .id_salt(("conversation-scroll", self.active_conversation))
                    .stick_to_bottom(true)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if empty {
                            self.empty_state(ui);
                        } else {
                            ui.add_space(20.0);
                            let width = ui.available_width().min(self.appearance.column_width());
                            let margin = ((ui.available_width() - width) * 0.5).max(0.0);
                            ui.horizontal(|ui| {
                                ui.add_space(margin);
                                ui.vertical(|ui| {
                                    ui.set_width(width);
                                    ui.push_id(
                                        ("conversation-text", self.active_conversation),
                                        |ui| self.show_transcript(ui, width),
                                    );
                                });
                            });
                        }
                    });
                if !empty
                    && scroll.content_size.y
                        - scroll.inner_rect.height()
                        - scroll.state.clamped_offset().y
                        > 2.0
                {
                    self.transcript.latest_button(ui, scroll.inner_rect);
                }
            });
    }

    fn draw(&mut self, ui: &mut egui::Ui) {
        // Disable the workspace on the first modal frame too: egui's modal
        // focus layer is otherwise inherited only from the preceding pass.
        let drawer = self.sidebar_open && ui.available_width() < navigation::DOCK_THRESHOLD;
        ui.add_enabled_ui(!self.show_settings && !drawer, |ui| self.draw_workspace(ui));
        let ctx = ui.ctx().clone();
        if self.show_settings {
            self.settings(&ctx);
            return;
        }
        self.navigation_drawer(&ctx);
        self.diagnostics(&ctx);
        self.conversation_dialogs(&ctx);
        self.iteration_dialog(&ctx);
        if let Some((title, message)) = self.help {
            let mut open = true;
            egui::Window::new(title)
                .open(&mut open)
                .default_width(460.0)
                .show(&ctx, |ui| {
                    ui.label(message);
                    if ui.button("Close").clicked() {
                        self.help = None;
                        self.speech_notice = false;
                    }
                });
            if !open {
                self.help = None;
                self.speech_notice = false;
            }
        }
    }
}

impl eframe::App for State {
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.poll_placement(frame, ctx.input(|input| input.viewport().close_requested()));
        let was_ready = self.model.ready();
        let was_busy = self.model.busy();
        self.poll_startup();
        self.drain_events();
        self.poll_updates(ctx);
        if ((!was_ready && self.model.ready())
            || (was_busy && !self.model.busy() && self.model.ready()))
            && !self.show_settings
            && ctx.memory(|memory| {
                memory
                    .focused()
                    .is_none_or(|id| Some(id) == self.composer_id)
            })
            && !ctx
                .plugin::<egui::text_selection::LabelSelectionState>()
                .lock()
                .has_selection()
            && !ctx.input(|input| input.pointer.any_down())
        {
            self.focus_composer = true;
        }
        self.prepare_audio_paste(frame);
        self.handle_input_shortcuts(ctx);
        self.handle_audio_paste(ctx);
        self.poll_audio_import();
        self.poll_microphone();
        self.drain_speech_events();
        self.poll_recovery();
        self.poll_appearance(ctx);
        self.poll_profile();
        if self.last_theme_check.elapsed() >= Duration::from_secs(1) {
            self.appearance.apply(ctx);
            self.last_theme_check = Instant::now();
        }
        // No frame-rate polling: wait between events when idle. Microphone's
        // WinRT state machine is polled only during an explicit capture session.
        let active_generation = !self.preview && self.model.busy();
        let waiting = active_generation
            || (!self.preview
                && (self.model.waiting_model()
                    || self.speaking
                    || self.dictation_requested
                    || self.audio_import.is_some()
                    || !self.speech_checked));
        ctx.request_repaint_after(if active_generation {
            Duration::from_millis(16)
        } else if waiting {
            Duration::from_millis(50)
        } else {
            Duration::from_secs(1)
        });
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.draw(ui);
        self.flush_voice_transcript(ui.ctx());
    }
}

fn native_options() -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1080.0, 780.0])
            .with_min_inner_size([480.0, 480.0]),
        renderer: eframe::Renderer::Wgpu,
        ..eframe::NativeOptions::default()
    }
}

pub(super) fn run(config: Config) -> io::Result<()> {
    let recovery = crate::recovery::Recovery::open(&config.data);
    let placement = placement::Placement::open(&config.data);
    let appearance = appearance::Store::open(&config.data);
    let profile = profile::Store::open(&config.data);
    eframe::run_native(
        "Swift Bonsai",
        native_options(),
        Box::new(move |cc| {
            graphics::install(cc);
            configure(&cc.egui_ctx);
            let (commands, incoming) = mpsc::channel();
            let (events, incoming_events) = client::event_channel();
            let microphone = microphone::Microphone::new().ok();
            let mut state = State::create(commands, incoming_events, Some(&config), microphone);
            state.attach_recovery(recovery);
            state.attach_placement(placement, cc);
            state.attach_appearance(appearance, &cc.egui_ctx);
            state.attach_profile(profile);
            state.worker = Some(
                std::thread::Builder::new()
                    .name("swift-chat-client".to_owned())
                    .spawn(move || client::run(config, incoming, events))?,
            );
            Ok(Box::new(state))
        }),
    )
    .map_err(|error| io::Error::other(error.to_string()))
}

pub(super) fn run_startup(request: crate::startup::Request) -> io::Result<bool> {
    // Retain the established recovery ordering: restore before accepting a draft.
    let recovery = crate::recovery::Recovery::open(&request.data);
    let placement = placement::Placement::open(&request.data);
    let appearance = appearance::Store::open(&request.data);
    let profile = profile::Store::open(&request.data);
    let failure = std::sync::Arc::new(std::sync::Mutex::new(None));
    let reported_failure = std::sync::Arc::clone(&failure);
    let update_exit = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let approved_update = std::sync::Arc::clone(&update_exit);
    eframe::run_native(
        "Swift Bonsai",
        native_options(),
        Box::new(move |cc| {
            graphics::install(cc);
            configure(&cc.egui_ctx);
            let (commands, _incoming) = mpsc::channel();
            let (_events, incoming_events) = client::event_channel();
            let mut state = State::pending_startup(commands, incoming_events, request);
            state.startup_failure = Some(reported_failure);
            state.update_exit = Some(approved_update);
            state.attach_recovery(recovery);
            state.attach_placement(placement, cc);
            state.attach_appearance(appearance, &cc.egui_ctx);
            state.attach_profile(profile);
            Ok(Box::new(state))
        }),
    )
    .map_err(|error| io::Error::other(error.to_string()))?;
    let error = failure
        .lock()
        .map_err(|_| io::Error::other("Cannot read native preparation result."))?
        .take();
    error.map_or_else(
        || Ok(update_exit.load(std::sync::atomic::Ordering::Acquire)),
        |error| Err(io::Error::other(error)),
    )
}
/// Opens only a seeded interface; no backend, credential or audio operation runs.
pub(super) fn run_preview() -> io::Result<()> {
    eframe::run_native("Swift Bonsai · Preview", native_options(), Box::new(|cc| {
        graphics::install(cc);
        configure(&cc.egui_ctx);
        let (commands, _incoming) = mpsc::channel();
        let (_events, incoming_events) = client::event_channel();
        let mut state = State::create(commands, incoming_events, None, None);
        state.model = ModelLifecycle::Ready;
        "Interface preview".clone_into(&mut state.status);
        state.transcript.push(Role::User, "I have a few ideas for a short story, but I’m not sure where to start.".to_owned());
        state.transcript.push(Role::Assistant, "Start with the moment something changes.\n\nA quiet bookshop receives a letter addressed to someone who has never existed. Its owner recognizes the handwriting as their own.\n\nWhat draws you in more: the mystery of the letter, or the person who finds it?".to_owned());
        state.last_answer = state.transcript.messages.last().map_or(String::new(), |message| message.body.clone());
        state.answer_complete = true;
        Ok(Box::new(state))
    })).map_err(|error| io::Error::other(error.to_string()))
}

pub(super) fn show_error(message: &str) {
    platform::show_error(message);
}

#[cfg(test)]
mod startup_tests {
    use super::*;
    use std::{ffi::OsString, fs, path::PathBuf};

    fn fixture() -> io::Result<(crate::startup::Request, PathBuf)> {
        let directory = std::env::temp_dir().join(format!(
            "Bonsai-UI-Startup-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(io::Error::other)?
                .as_nanos()
        ));
        fs::create_dir(&directory)?;
        let request = directory.join("request.json");
        let tool = directory.join("tool.exe");
        fs::write(&request, "{}")?;
        fs::write(&tool, [])?;
        let arguments = vec![
            OsString::from("--startup-tool"),
            tool.into_os_string(),
            OsString::from("--startup-request"),
            request.into_os_string(),
            OsString::from("--startup-data-directory"),
            directory.clone().into_os_string(),
        ];
        Ok((crate::startup::Request::parse(&arguments)?, directory))
    }

    #[test]
    fn model_events_and_text_chat_continue_while_speech_prepares_or_fails() -> io::Result<()> {
        let (request, directory) = fixture()?;
        let (commands, incoming) = mpsc::channel();
        let (events, incoming_events) = client::event_channel();
        let mut state = State::pending_startup(commands, incoming_events, request);
        state.startup_request.take();
        let (session, progress) = crate::startup::Session::fixture();
        state.startup = Some(session);
        state.worker = Some(std::thread::spawn(|| {}));
        events
            .send(Event::Ready)
            .map_err(|()| io::Error::other("Model fixture disconnected"))?;
        state.drain_events();
        assert_eq!(state.model, ModelLifecycle::Ready);
        state.input = "Chat before speech is ready".into();
        state.submit();
        assert!(matches!(incoming.try_recv(), Ok(Command::Send { .. })));
        assert_eq!(state.model, ModelLifecycle::Generating);
        assert!(state.streaming_suppressed);
        let conversation = state.request_conversation;
        state.input = "A newer draft".into();
        let failure = std::sync::Arc::new(std::sync::Mutex::new(None));
        state.startup_failure = Some(std::sync::Arc::clone(&failure));
        progress
            .send(Ok(crate::startup::Progress::Speech(Err(io::Error::other(
                "Synthetic speech preparation failure",
            )))))
            .map_err(|_| io::Error::other("Preparation fixture disconnected"))?;
        state.poll_startup();
        assert!(state.startup.is_none() && state.speech_checked);
        assert_eq!(state.model, ModelLifecycle::Generating);
        assert_eq!(state.request_conversation, conversation);
        assert_eq!(state.input, "A newer draft");
        assert!(state.speech_status.contains("Speech preparation failed"));
        assert!(
            failure
                .lock()
                .map_err(|_| io::Error::other("Poisoned fixture"))?
                .is_none()
        );
        drop(state);
        fs::remove_dir_all(directory)?;
        Ok(())
    }

    #[test]
    fn pending_native_startup_preserves_draft_through_failure_and_configuration() -> io::Result<()>
    {
        let (request, directory) = fixture()?;
        let (commands, incoming) = mpsc::channel();
        let (_events, incoming_events) = client::event_channel();
        let mut state = State::pending_startup(commands, incoming_events, request);
        state.input = "A message drafted while applications prepare".into();
        state.enable_thinking = true;
        state.output_budget = 1;
        state.voice_profile = "My chosen voice".into();
        assert!(!state.preview && !state.model.ready() && state.model.waiting_model());
        assert!(state.worker.is_none() && state.speech.is_none() && state.microphone.is_none());
        state.submit();
        state.drain_events();
        assert!(incoming.try_recv().is_err());
        assert!(state.model.waiting_model() && !state.warning);
        let failure = std::sync::Arc::new(std::sync::Mutex::new(None));
        state.startup_failure = Some(std::sync::Arc::clone(&failure));
        assert!(
            failure
                .lock()
                .map_err(|_| io::Error::other("Poisoned fixture."))?
                .is_none()
        );
        state.fail_startup(&io::Error::other("Synthetic setup failure"));
        assert_eq!(
            failure
                .lock()
                .map_err(|_| io::Error::other("Poisoned fixture."))?
                .as_deref(),
            Some("Synthetic setup failure")
        );
        assert!(!state.model.waiting_model() && state.warning && !state.model.retryable());
        assert_eq!(state.input, "A message drafted while applications prepare");
        assert!(state.transcript.messages.is_empty());
        state.startup_request.take();
        state.model = ModelLifecycle::Preparing;
        let config = Config {
            broker: directory.join("broker.exe"),
            data: directory.clone(),
            evidence: directory.clone(),
            frontend_port: 5173,
            backend_port: 8081,
            use_abliterate_lora: false,
            use_repacked_ptq1: false,
            native_engine: false,
            native_row_workers: 1,
            latest_prompt_reuse: false,
            canary: None,
            qwen_tts: None,
        };
        state.attach_configuration_with(config, false, |_, incoming, events| {
            thread_for_fixture(incoming, events)
        })?;
        assert_eq!(state.input, "A message drafted while applications prepare");
        assert!(state.enable_thinking && state.output_budget == 1);
        assert_eq!(state.voice_profile, "My chosen voice");
        assert!(!state.abliteration_enabled);
        assert!(!state.preview && !state.model.ready());
        drop(state);
        fs::remove_dir_all(&directory)?;
        Ok(())
    }

    #[test]
    fn pending_native_startup_blocks_read_aloud_until_speech_is_available() -> io::Result<()> {
        let (request, directory) = fixture()?;
        let (commands, _incoming) = mpsc::channel();
        let (_events, incoming_events) = client::event_channel();
        let mut state = State::pending_startup(commands, incoming_events, request);
        assert!(state.canary_configured && state.tts_configured && !state.speech_checked);
        state
            .transcript
            .push(Role::Assistant, "Recovered answer to read aloud.".into());
        state.transcript.remove_empty_response();
        let message_id = state.transcript.messages[0].id;
        state.toggle_dictation();
        assert!(state.help.is_none());
        assert_eq!(
            state.speech_status,
            "Enable the microphone after speech preparation completes."
        );
        state.toggle_streaming_read_aloud();
        assert!(state.help.is_none() && !state.streaming_read_aloud);
        state.toggle_read_aloud(message_id);
        assert!(state.help.is_none());
        assert!(!state.speaking && state.speaking_message.is_none());
        let (session, speech_commands) = speech::Session::fixture()?;
        state.speech = Some(session);
        state.speech_event(speech::Event::Capabilities {
            input: true,
            output: true,
        });
        assert!(state.speech_checked && state.speech_output);
        assert!(speech_commands.try_recv().is_err());
        assert!(!state.model.ready());
        state.toggle_streaming_read_aloud();
        assert!(state.streaming_read_aloud);
        state.toggle_read_aloud(message_id);
        assert!(matches!(
            speech_commands
                .recv_timeout(Duration::from_secs(2))
                .map_err(io::Error::other)?,
            speech::Command::StopListening
        ));
        assert!(matches!(
            speech_commands
                .recv_timeout(Duration::from_secs(2))
                .map_err(io::Error::other)?,
            speech::Command::StopSpeaking
        ));
        assert!(matches!(
            speech_commands.recv_timeout(Duration::from_secs(2)).map_err(io::Error::other)?,
            speech::Command::Speak { text, voice_description }
                if text == "Recovered answer to read aloud." && voice_description == state.voice_profile
        ));
        drop(state);
        fs::remove_dir_all(&directory)?;
        Ok(())
    }

    fn thread_for_fixture(
        incoming: Receiver<Command>,
        events: client::EventSender,
    ) -> io::Result<JoinHandle<()>> {
        std::thread::Builder::new()
            .name("startup-regression".into())
            .spawn(move || {
                let _retained_events = events;
                while let Ok(command) = incoming.recv() {
                    if matches!(command, Command::Shutdown) {
                        break;
                    }
                }
            })
    }
}
