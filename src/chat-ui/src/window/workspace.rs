//! Bounded conversation workspace. Locally identified interrupted replies are
//! retained as context without replaying their original requests.
use super::{Command, State, Transcript, client, history_search};

mod dialogs;
mod list;

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "Independent response choices and transcript status are preserved for each conversation."
)]
pub(super) struct ConversationSnapshot {
    pub(super) transcript: Transcript,
    #[serde(deserialize_with = "super::persistence::deserialize_history")]
    pub(super) committed: Vec<client::Message>,
    #[serde(skip)]
    pub(super) context_history: Option<usize>,
    pub(super) input: String,
    pub(super) last_answer: String,
    pub(super) retained_draft: String,
    pub(super) answer_complete: bool,
    pub(super) extra_effort: bool,
    #[serde(default)]
    pub(super) enable_thinking: bool,
    #[serde(default)]
    pub(super) streaming_read_aloud: bool,
    pub(super) output_budget: usize,
    #[serde(default = "client::default_abliteration_scale")]
    pub(super) abliteration_scale: f32,
    pub(super) status: String,
    #[serde(skip)]
    pub(super) status_detail: String,
    pub(super) warning: bool,
    #[serde(skip)]
    pub(super) stage: Option<&'static str>,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Conversation {
    pub(super) id: u64,
    pub(super) title: String,
    pub(super) renamed: bool,
    pub(super) saved: Option<ConversationSnapshot>,
    #[serde(skip)]
    pub(super) search: history_search::MatchCache,
}

impl Conversation {
    pub(super) fn new(id: u64) -> Self {
        Self {
            id,
            title: "New conversation".to_owned(),
            renamed: false,
            saved: None,
            search: history_search::MatchCache::default(),
        }
    }
}

impl State {
    pub(super) fn update_worker_conversation(&mut self, command: Command) {
        // Startup and generation consume history commands without applying them.
        // Coalesce navigation until the worker can accept the selected history.
        self.restore_history = true;
        if self.preview || self.model != super::ModelLifecycle::Ready {
            return;
        }
        if self.commands.send(command).is_ok() {
            self.restore_history = false;
        } else {
            // A lost worker must not prevent local conversation management.
            self.fail("The model worker stopped. Close and restart the app.");
        }
    }

    pub(super) fn take_conversation(&mut self) -> ConversationSnapshot {
        ConversationSnapshot {
            transcript: std::mem::take(&mut self.transcript),
            committed: std::mem::take(&mut self.committed),
            context_history: self.context_history.take(),
            input: std::mem::take(&mut self.input),
            last_answer: std::mem::take(&mut self.last_answer),
            retained_draft: std::mem::take(&mut self.retained_draft),
            answer_complete: self.answer_complete,
            extra_effort: self.extra_effort,
            enable_thinking: self.enable_thinking,
            streaming_read_aloud: self.streaming_read_aloud,
            output_budget: self.output_budget,
            abliteration_scale: self.abliteration_scale,
            status: if self.model.ready() {
                self.status.clone()
            } else {
                "Ready when you are.".to_owned()
            },
            warning: self.model.ready() && self.warning,
            status_detail: if self.model.ready() {
                self.status_detail.clone()
            } else {
                String::new()
            },
            stage: self.stage,
        }
    }

    pub(super) fn save_conversation(&mut self) {
        let snapshot = self.take_conversation();
        if let Some(conversation) = self
            .conversations
            .iter_mut()
            .find(|conversation| conversation.id == self.active_conversation)
        {
            conversation.saved = Some(snapshot);
        }
    }

    pub(super) fn clear_conversation(&mut self) {
        self.transcript.clear();
        self.committed.clear();
        self.context_history = None;
        self.input.clear();
        self.last_answer.clear();
        self.retained_draft.clear();
        self.stage = None;
        self.answer_complete = false;
        self.extra_effort = false;
        self.enable_thinking = false;
        self.streaming_read_aloud = false;
        self.output_budget = 3;
        self.abliteration_scale = client::DEFAULT_ABLITERATION_SCALE;
        self.place_composer_cursor(0);
        self.focus_composer = true;
    }

    pub(super) fn load_conversation(&mut self, snapshot: ConversationSnapshot) {
        self.transcript = snapshot.transcript;
        self.committed = snapshot.committed;
        self.context_history = snapshot.context_history;
        self.input = snapshot.input;
        self.last_answer = snapshot.last_answer;
        self.retained_draft = snapshot.retained_draft;
        self.answer_complete = snapshot.answer_complete;
        self.extra_effort = snapshot.extra_effort;
        self.enable_thinking = snapshot.enable_thinking;
        self.streaming_read_aloud = snapshot.streaming_read_aloud;
        self.output_budget = snapshot.output_budget;
        self.abliteration_scale = snapshot.abliteration_scale;
        // Loading/failure status belongs to the model, not a saved transcript.
        if self.model.ready() {
            self.status = snapshot.status;
            self.status_detail = snapshot.status_detail;
            self.warning = snapshot.warning;
        }
        self.stage = snapshot.stage;
        self.place_composer_cursor(self.input.chars().count());
        self.focus_composer = true;
    }

    // Event routing swaps only conversation content. The actual selection, Find
    // state in its transcript, and live composer cursor/focus stay with the user.
    pub(super) fn with_request_conversation(&mut self, action: impl FnOnce(&mut Self)) {
        if self.routing_request {
            action(self);
            return;
        }
        let Some(owner) = self
            .request_conversation
            .filter(|id| *id != self.active_conversation)
        else {
            action(self);
            return;
        };
        let Some(index) = self
            .conversations
            .iter()
            .position(|value| value.id == owner)
        else {
            action(self);
            return;
        };
        let Some(mut saved) = self.conversations[index].saved.take() else {
            return;
        };
        let cursor = (
            self.composer_cursor,
            self.pending_cursor,
            self.focus_composer,
        );
        self.swap_conversation(&mut saved);
        let previous = (
            self.model,
            self.status.clone(),
            self.warning,
            self.status_detail.clone(),
        );
        self.routing_request = true;
        action(self);
        self.routing_request = false;
        // Late rejections and completion races can update a hidden transcript
        // without changing the global stop/restart status currently on screen.
        let changed = previous.0 != self.model
            || previous.1 != self.status
            || previous.2 != self.warning
            || previous.3 != self.status_detail;
        let global_status = (!self.model.ready() && changed).then(|| {
            (
                self.status.clone(),
                self.warning,
                self.status_detail.clone(),
            )
        });
        self.swap_conversation(&mut saved);
        self.conversations[index].saved = Some(saved);
        (
            self.composer_cursor,
            self.pending_cursor,
            self.focus_composer,
        ) = cursor;
        if let Some((status, warning, detail)) = global_status {
            self.status = status;
            self.status_detail = detail;
            self.warning = warning;
        }
    }

    const fn swap_conversation(&mut self, saved: &mut ConversationSnapshot) {
        std::mem::swap(&mut self.transcript, &mut saved.transcript);
        std::mem::swap(&mut self.committed, &mut saved.committed);
        std::mem::swap(&mut self.context_history, &mut saved.context_history);
        std::mem::swap(&mut self.input, &mut saved.input);
        std::mem::swap(&mut self.last_answer, &mut saved.last_answer);
        std::mem::swap(&mut self.retained_draft, &mut saved.retained_draft);
        std::mem::swap(&mut self.answer_complete, &mut saved.answer_complete);
        std::mem::swap(&mut self.extra_effort, &mut saved.extra_effort);
        std::mem::swap(&mut self.enable_thinking, &mut saved.enable_thinking);
        std::mem::swap(
            &mut self.streaming_read_aloud,
            &mut saved.streaming_read_aloud,
        );
        std::mem::swap(&mut self.output_budget, &mut saved.output_budget);
        std::mem::swap(&mut self.abliteration_scale, &mut saved.abliteration_scale);
        std::mem::swap(&mut self.status, &mut saved.status);
        std::mem::swap(&mut self.status_detail, &mut saved.status_detail);
        std::mem::swap(&mut self.warning, &mut saved.warning);
        std::mem::swap(&mut self.stage, &mut saved.stage);
    }

    pub(super) fn restore_selected_history(&mut self) {
        if self.restore_history && self.model == super::ModelLifecycle::Ready {
            self.update_worker_conversation(Command::RestoreChat(self.committed.clone()));
        }
    }

    pub(super) fn generation_status(&self) -> String {
        if let Some(owner) = self.request_conversation
            && owner != self.active_conversation
            && let Some(conversation) = self.conversations.iter().find(|value| value.id == owner)
        {
            return format!(
                "Responding in ‘{}’. You can draft here while you wait.",
                conversation.title
            );
        }
        format!(
            "{} You can draft your next message while you wait.",
            self.status
        )
    }

    fn can_remove_conversation(&self, id: u64) -> bool {
        self.model.workspace_available()
            && !(self.model.busy() && self.request_conversation == Some(id))
    }

    pub(super) fn switch_conversation(&mut self, id: u64) {
        if !self.model.workspace_available() || id == self.active_conversation {
            return;
        }
        let Some(index) = self
            .conversations
            .iter()
            .position(|conversation| conversation.id == id)
        else {
            return;
        };
        let Some(saved) = self.conversations[index].saved.as_ref() else {
            return;
        };
        self.update_worker_conversation(Command::RestoreChat(saved.committed.clone()));
        self.stop_speech();
        self.save_conversation();
        self.conversation_notice = None;
        if let Some(snapshot) = self.conversations[index].saved.take() {
            self.active_conversation = id;
            self.load_conversation(snapshot);
            self.recovery_dirty = true;
        }
    }

    pub(super) fn remove_conversation(&mut self, id: u64) {
        if !self.can_remove_conversation(id) {
            return;
        }
        let Some(index) = self
            .conversations
            .iter()
            .position(|conversation| conversation.id == id)
        else {
            return;
        };
        if id == self.active_conversation {
            if let Some(next) = self
                .conversations
                .iter()
                .find(|conversation| conversation.id != id)
                .map(|conversation| conversation.id)
            {
                self.switch_conversation(next);
                if self.active_conversation == id {
                    return;
                }
            } else {
                self.update_worker_conversation(Command::NewChat);
                self.stop_speech();
                self.clear_conversation();
                self.active_conversation = self.next_conversation;
                self.next_conversation = self.next_conversation.saturating_add(1);
                self.conversations
                    .push(Conversation::new(self.active_conversation));
                if self.model.ready() {
                    self.set_status("Ready for a new conversation.", false);
                }
            }
        }
        self.conversations.remove(index);
        self.recovery_dirty = true;
        self.conversation_notice = Some("Conversation deleted.");
    }
}
