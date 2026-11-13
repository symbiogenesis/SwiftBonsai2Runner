//! Explicit message iteration preserves the source conversation and exact history.
use super::{Command, Conversation, INPUT_LIMIT, ModelLifecycle, State, Transcript, egui};

const CONTINUE_PROMPT: &str = "Continue your previous response from where it stopped, without repeating what you already wrote.";

#[derive(Clone, Copy, Debug)]
pub(super) enum MessageAction {
    Edit(u64),
    Regenerate(u64),
    Branch(u64),
    Continue(u64),
}

#[derive(Clone, Copy, Default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "Each explicit message action has an independent eligibility rule."
)]
pub(super) struct MessageOptions {
    pub(super) edit: bool,
    pub(super) regenerate: bool,
    pub(super) branch: bool,
    pub(super) continue_response: bool,
}

pub(super) struct EditMessage {
    pub(super) source_conversation: u64,
    pub(super) message: u64,
    pub(super) text: String,
    focus: bool,
    limit_notice: bool,
}

impl State {
    pub(super) fn message_options(&self, id: u64) -> MessageOptions {
        let available = !self
            .updates
            .as_ref()
            .is_some_and(super::updates::Updates::installing);
        let can_send = available && !self.preview && self.model == ModelLifecycle::Ready;
        let can_branch = available && self.model.workspace_available();
        self.transcript
            .messages
            .iter()
            .find(|message| message.id == id)
            .map_or_else(MessageOptions::default, |message| {
                message.iteration_options(&self.committed, can_send, can_branch)
            })
    }

    pub(super) fn message_action(&mut self, action: MessageAction) {
        match action {
            MessageAction::Edit(id) if self.message_options(id).edit => {
                let Some(index) = self.transcript.committed_index(id, &self.committed) else {
                    return;
                };
                self.stop_speech();
                self.help = None;
                self.edit_message = Some(EditMessage {
                    source_conversation: self.active_conversation,
                    message: id,
                    text: self.committed[index].content.to_string(),
                    focus: true,
                    limit_notice: false,
                });
                self.focus_composer = false;
            }
            MessageAction::Regenerate(id) if self.message_options(id).regenerate => {
                let Some(index) = self.transcript.committed_index(id, &self.committed) else {
                    return;
                };
                let prompt_index = index - 1;
                let prompt = self.committed[prompt_index].content.to_string();
                if self.branch_history(prompt_index, "Regenerated") {
                    self.input = prompt;
                    self.submit();
                }
            }
            MessageAction::Branch(id) if self.message_options(id).branch => {
                let Some(index) = self.transcript.committed_index(id, &self.committed) else {
                    return;
                };
                // A committed history always contains complete user/assistant
                // pairs. A user-message branch places that prompt in the draft.
                let (end, prompt) = if self.committed[index].role == "user" {
                    (index, self.committed[index].content.to_string())
                } else {
                    (index + 1, String::new())
                };
                if self.branch_history(end, "Branch") {
                    self.input = prompt;
                    self.place_composer_cursor(self.input.chars().count());
                }
            }
            MessageAction::Continue(id) if self.message_options(id).continue_response => {
                // Continue is an explicit ordinary followup, not an automatic
                // replay or modification of an already committed answer.
                let draft = std::mem::replace(&mut self.input, CONTINUE_PROMPT.to_owned());
                let cursor = (
                    self.composer_cursor,
                    self.pending_cursor,
                    self.focus_composer,
                );
                self.restore_selected_history();
                self.submit();
                self.input = draft;
                (
                    self.composer_cursor,
                    self.pending_cursor,
                    self.focus_composer,
                ) = cursor;
                self.recovery_dirty = true;
            }
            _ => {}
        }
    }

    fn branch_history(&mut self, end: usize, label: &str) -> bool {
        if !self.model.workspace_available() || end > self.committed.len() || !end.is_multiple_of(2)
        {
            return false;
        }
        let Some(next) = self.next_conversation.checked_add(1) else {
            self.conversation_notice =
                Some("Cannot create another conversation in this workspace.");
            return false;
        };
        let history = self.committed[..end].to_vec();
        let mut transcript = Transcript::from_committed(&history);
        transcript.copy_interrupted_marks(&self.transcript, &history);
        let title = self
            .conversations
            .iter()
            .find(|conversation| conversation.id == self.active_conversation)
            .map_or_else(
                || "Conversation".to_owned(),
                |conversation| conversation.title.chars().take(44).collect::<String>(),
            );
        let settings = (
            self.extra_effort,
            self.enable_thinking,
            self.streaming_read_aloud,
            self.output_budget,
            self.abliteration_scale,
        );
        self.stop_speech();
        self.save_conversation();
        self.clear_conversation();
        self.active_conversation = self.next_conversation;
        self.next_conversation = next;
        let mut branch = Conversation::new(self.active_conversation);
        branch.title = format!("{title} · {label}");
        branch.renamed = true;
        self.conversations.push(branch);
        self.conversation_search.clear();
        self.transcript = transcript;
        self.committed = history;
        (
            self.extra_effort,
            self.enable_thinking,
            self.streaming_read_aloud,
            self.output_budget,
            self.abliteration_scale,
        ) = settings;
        if let Some(answer) = self.committed.last() {
            self.last_answer = answer.content.to_string();
            self.answer_complete = true;
        }
        self.update_worker_conversation(Command::RestoreChat(self.committed.clone()));
        if self.model == ModelLifecycle::Ready {
            self.set_status("Ready in your new branch.", false);
        }
        self.conversation_notice =
            Some("Created a branch. The original conversation is unchanged.");
        self.recovery_dirty = true;
        true
    }

    fn resend_edited_message(&mut self, edit: EditMessage) {
        if edit.source_conversation != self.active_conversation
            || !self.message_options(edit.message).edit
            || edit.text.trim().is_empty()
            || edit.text.chars().count() > INPUT_LIMIT
        {
            return;
        }
        let Some(index) = self
            .transcript
            .committed_index(edit.message, &self.committed)
        else {
            return;
        };
        if self.branch_history(index, "Edited") {
            self.input = edit.text;
            self.submit();
        }
    }

    pub(super) fn iteration_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut edit) = self.edit_message.take() else {
            return;
        };
        let mut confirmed = false;
        let mut cancelled = false;
        let enabled = edit.source_conversation == self.active_conversation
            && self.message_options(edit.message).edit;
        let modal = egui::Modal::new(egui::Id::unique("edit-and-resend")).show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 80.0).clamp(280.0, 560.0));
            ui.heading("Edit and resend");
            ui.label("Creates a new conversation from this prompt. The original conversation and its draft stay unchanged.");
            ui.add_space(8.0);
            let label = ui.label("Edited message");
            egui::ScrollArea::vertical()
                .max_height((ctx.content_rect().height() - 240.0).max(100.0))
                .show(ui, |ui| {
                    let id = ui.make_persistent_id("edited-message");
                    let mut buffer = super::draft_buffer::Buffer::new(
                        &mut edit.text, ui.ctx(), id, INPUT_LIMIT,
                    );
                    let mut output = egui::TextEdit::multiline(&mut buffer)
                            .id(id)
                            .desired_width(f32::INFINITY)
                            .desired_rows(6)
                            .show(ui);
                    if buffer.finish(&mut output) {
                        edit.limit_notice = true;
                    } else if output.response.response.changed() {
                        edit.limit_notice = false;
                    }
                    let response = output.response.response.labelled_by(label.id);
                    if edit.focus {
                        response.request_focus();
                        edit.focus = false;
                    }
                });
            if edit.limit_notice {
                ui.label(super::draft_buffer::LIMIT_MESSAGE);
            }
            if !enabled {
                ui.label("Return to the source conversation and wait until the model is ready to resend.");
            }
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                confirmed = ui
                    .add_enabled(
                        enabled && !edit.text.trim().is_empty(),
                        egui::Button::new("Create branch and send"),
                    )
                    .clicked();
                cancelled = ui.button("Cancel").clicked();
            });
        });
        if confirmed {
            self.resend_edited_message(edit);
        } else if !cancelled && !modal.should_close() {
            self.edit_message = Some(edit);
        } else {
            self.focus_composer = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{client, window::mpsc};

    fn fixture() -> (State, mpsc::Receiver<Command>) {
        let (commands, incoming) = mpsc::channel();
        let (_events, incoming_events) = client::event_channel();
        let mut state = State::create(commands, incoming_events, None, None);
        state.preview = false;
        state.model = ModelLifecycle::Ready;
        state.committed = [
            ("user", "Same question"),
            ("assistant", "First answer"),
            ("user", "Same question"),
            ("assistant", "Second answer"),
        ]
        .into_iter()
        .map(|(role, content)| client::Message {
            role,
            content: content.into(),
            reasoning_content: None,
        })
        .collect();
        state.transcript = Transcript::from_committed(&state.committed);
        state.input = "Unsent source draft 🌱".into();
        state.enable_thinking = true;
        state.extra_effort = true;
        state.output_budget = 1;
        state.abliteration_scale = 1.4;
        (state, incoming)
    }

    #[test]
    fn iteration_regenerates_selected_duplicate_turn_in_a_branch_with_exact_prefix() {
        let (mut state, commands) = fixture();
        let source = state.active_conversation;
        let answer = state.transcript.messages[3].id;
        state.message_action(MessageAction::Regenerate(answer));
        assert_ne!(state.active_conversation, source);
        assert_eq!(state.committed.len(), 2);
        assert_eq!(state.committed[1].content.as_ref(), "First answer");
        assert!(
            matches!(commands.try_recv(), Ok(Command::RestoreChat(history)) if history.len() == 2)
        );
        assert!(
            matches!(commands.try_recv(), Ok(Command::Send { text, enable_thinking: true, extra_effort: true, max_completion_tokens: 1024, abliteration_scale, .. }) if text == "Same question" && (abliteration_scale - 1.4).abs() < f32::EPSILON)
        );
        let original = state.conversations[0].saved.as_ref();
        assert!(original.is_some_and(
            |saved| saved.committed.len() == 4 && saved.input == "Unsent source draft 🌱"
        ));
        assert!(state.model.busy());
    }

    #[test]
    fn iteration_edit_cancellation_is_inert_and_confirmation_preserves_original()
    -> std::io::Result<()> {
        let (mut state, commands) = fixture();
        let prompt = state.transcript.messages[0].id;
        state.message_action(MessageAction::Edit(prompt));
        let mut edit = state
            .edit_message
            .take()
            .ok_or_else(|| std::io::Error::other("Edit must open a review dialog"))?;
        assert_eq!(edit.text, "Same question");
        assert!(commands.try_recv().is_err());
        assert_eq!(state.conversations.len(), 1);
        assert_eq!(state.input, "Unsent source draft 🌱");
        edit.text = "Revised question".into();
        state.resend_edited_message(edit);
        assert!(
            matches!(commands.try_recv(), Ok(Command::RestoreChat(history)) if history.is_empty())
        );
        assert!(
            matches!(commands.try_recv(), Ok(Command::Send { text, .. }) if text == "Revised question")
        );
        assert!(state.committed.is_empty());
        assert!(
            state.conversations[0]
                .saved
                .as_ref()
                .is_some_and(|saved| saved.committed.len() == 4)
        );
        Ok(())
    }

    #[test]
    fn iteration_branches_a_user_prompt_without_committing_or_sending_it() {
        let (mut state, commands) = fixture();
        let prompt = state.transcript.messages[2].id;
        state.message_action(MessageAction::Branch(prompt));
        assert_eq!(state.committed.len(), 2);
        assert_eq!(state.input, "Same question");
        assert_eq!(state.transcript.messages.len(), 2);
        assert!(
            matches!(commands.try_recv(), Ok(Command::RestoreChat(history)) if history.len() == 2)
        );
        assert!(commands.try_recv().is_err());
        assert_eq!(state.model, ModelLifecycle::Ready);
    }

    #[test]
    fn iteration_continue_preserves_draft_and_only_targets_the_last_committed_answer() {
        let (mut state, commands) = fixture();
        state.place_composer_cursor(7);
        let cursor = (state.composer_cursor, state.pending_cursor);
        let earlier = state.transcript.messages[1].id;
        let last = state.transcript.messages[3].id;
        state.message_action(MessageAction::Continue(earlier));
        assert!(commands.try_recv().is_err());
        state.message_action(MessageAction::Continue(last));
        assert!(
            matches!(commands.try_recv(), Ok(Command::Send { text, .. }) if text == CONTINUE_PROMPT)
        );
        assert_eq!(state.input, "Unsent source draft 🌱");
        assert_eq!((state.composer_cursor, state.pending_cursor), cursor);
        assert_eq!(state.committed.len(), 4);
        assert_eq!(state.transcript.messages[4].body, CONTINUE_PROMPT);
    }

    #[test]
    fn iteration_respects_busy_request_ownership() {
        let (mut state, commands) = fixture();
        let answer = state.transcript.messages[3].id;
        state.model = ModelLifecycle::Generating;
        state.request_conversation = Some(state.active_conversation);
        let owner = state.active_conversation;
        state.message_action(MessageAction::Regenerate(answer));
        state.message_action(MessageAction::Continue(answer));
        assert!(commands.try_recv().is_err());
        state.message_action(MessageAction::Branch(answer));
        assert_eq!(state.request_conversation, Some(owner));
        assert_ne!(state.active_conversation, owner);
        assert!(state.restore_history);
        assert!(commands.try_recv().is_err());
    }

    #[test]
    fn conversation_history_iteration_has_no_count_cap_and_reveals_new_branches()
    -> std::io::Result<()> {
        for action in 0..3 {
            let (mut state, commands) = fixture();
            let source = state.active_conversation;
            for _ in 1..32 {
                state.new_chat();
            }
            state.switch_conversation(source);
            while commands.try_recv().is_ok() {}
            state.conversation_search.text = "An unrelated search".into();
            let answer = state.transcript.messages[3].id;
            let prompt = state.transcript.messages[2].id;
            match action {
                0 => state.message_action(MessageAction::Branch(answer)),
                1 => state.message_action(MessageAction::Regenerate(answer)),
                _ => {
                    state.message_action(MessageAction::Edit(prompt));
                    let mut edit = state.edit_message.take().ok_or_else(|| {
                        std::io::Error::other("Edit must remain available after the former limit.")
                    })?;
                    edit.text = "A revised question".into();
                    state.resend_edited_message(edit);
                }
            }
            assert_eq!(state.conversations.len(), 33);
            assert_ne!(state.active_conversation, source);
            assert_eq!(state.conversation_search.text, "");
            let original = state
                .conversations
                .iter()
                .find(|item| item.id == source)
                .and_then(|item| item.saved.as_ref());
            assert_eq!(
                original.map(|saved| saved.input.as_str()),
                Some("Unsent source draft 🌱")
            );
        }
        Ok(())
    }

    #[test]
    fn interruption_continue_and_branch_use_partial_reply_and_preserve_draft() -> std::io::Result<()>
    {
        let (mut state, commands) = fixture();
        state.input = "Interrupted prompt".into();
        state.submit();
        assert!(matches!(commands.try_recv(), Ok(Command::Send { .. })));
        state.model_event(client::Event::Text("Partial answer".into()));
        state.input = "New draft".into();
        state.model_event(client::Event::Stopping("Interrupted prompt".into()));
        state.model_event(client::Event::Stopped);
        let answer = state
            .transcript
            .messages
            .last()
            .ok_or_else(|| std::io::Error::other("partial answer missing"))?
            .id;
        assert!(
            state
                .committed
                .last()
                .is_some_and(|message| message.content.as_ref() == "Partial answer")
        );
        assert!(state.message_options(answer).branch);
        assert!(!state.message_options(answer).continue_response);
        state.model = ModelLifecycle::Ready;
        state.restore_selected_history();
        assert!(
            matches!(commands.try_recv(), Ok(Command::RestoreChat(history)) if history.last().is_some_and(|message| message.content.as_ref() == "Partial answer"))
        );
        assert!(state.message_options(answer).continue_response);
        state.message_action(MessageAction::Branch(answer));
        assert!(
            matches!(commands.try_recv(), Ok(Command::RestoreChat(history)) if history.last().is_some_and(|message| message.content.as_ref() == "Partial answer"))
        );
        assert!(
            state.conversations[0]
                .saved
                .as_ref()
                .is_some_and(|saved| saved.input == "New draft")
        );
        let branch_answer = state
            .transcript
            .messages
            .last()
            .ok_or_else(|| std::io::Error::other("branch answer missing"))?
            .id;
        state.message_action(MessageAction::Continue(branch_answer));
        assert!(
            matches!(commands.try_recv(), Ok(Command::Send { text, .. }) if text == CONTINUE_PROMPT)
        );
        assert!(
            state
                .committed
                .last()
                .is_some_and(|message| message.content.as_ref() == "Partial answer")
        );
        Ok(())
    }
}
