//! Bind visible messages to exact committed positions before allowing iteration.
use super::{Message, Role, Transcript};
use crate::{client, window::iteration::MessageOptions};

impl Message {
    pub(in crate::window) fn retry_prefix(&self, history: &[client::Message]) -> Option<usize> {
        if self.role == Role::User {
            self.retry_prefix.filter(|prefix| {
                *prefix == history.len()
                    && prefix.is_multiple_of(2)
                    && self.history_index.is_none()
                    && !self.body.trim().is_empty()
                    && crate::window::draft_buffer::fits(&self.body)
            })
        } else {
            self.committed_index(history)
                .filter(|index| self.interrupted && *index > 0 && history[index - 1].role == "user")
                .map(|index| index - 1)
        }
    }

    fn matches_history(&self, message: &client::Message) -> bool {
        message.role
            == match self.role {
                Role::User => "user",
                Role::Assistant => "assistant",
            }
            && self.body == message.content.as_ref()
            && self.reasoning == message.reasoning_content.as_deref().unwrap_or_default()
    }

    pub(in crate::window) fn committed_index(&self, history: &[client::Message]) -> Option<usize> {
        self.history_index.filter(|index| {
            history
                .get(*index)
                .is_some_and(|entry| self.matches_history(entry))
        })
    }

    pub(in crate::window) fn iteration_options(
        &self,
        history: &[client::Message],
        can_send: bool,
        can_branch: bool,
    ) -> MessageOptions {
        let Some(index) = self.committed_index(history) else {
            return MessageOptions::default();
        };
        MessageOptions {
            edit: can_send && can_branch && self.role == Role::User,
            regenerate: can_send
                && can_branch
                && self.role == Role::Assistant
                && index > 0
                && history[index - 1].role == "user",
            branch: can_branch,
            continue_response: can_send
                && self.role == Role::Assistant
                && index + 1 == history.len(),
        }
    }
}

impl Transcript {
    pub(in crate::window) fn start_turn(&mut self, prompt: String, prefix: usize) {
        for message in &mut self.messages {
            message.retry_prefix = None;
        }
        self.push(Role::User, prompt);
        if let Some(message) = self.messages.last_mut() {
            message.turn_prefix = Some(prefix);
        }
        self.push(Role::Assistant, String::new());
        if let Some(message) = self.messages.last_mut() {
            message.turn_prefix = Some(prefix);
        }
    }

    pub(in crate::window) fn restart_turn(&mut self, id: u64, prefix: usize) {
        let Some(index) = self.messages.iter().position(|message| message.id == id) else {
            return;
        };
        for message in self.messages.drain(index + 1..) {
            self.text_bytes = self.text_bytes.saturating_sub(message.text_bytes());
        }
        self.history_revision = self.history_revision.wrapping_add(1);
        self.search.invalidate();
        self.messages[index].retry_prefix = None;
        self.messages[index].turn_prefix = Some(prefix);
        self.push(Role::Assistant, String::new());
        if let Some(message) = self.messages.last_mut() {
            message.turn_prefix = Some(prefix);
        }
    }

    pub(in crate::window) fn interrupt_turn(&mut self, prefix: usize) {
        if let Some(prompt) = self
            .messages
            .iter_mut()
            .rfind(|message| message.role == Role::User && message.turn_prefix == Some(prefix))
        {
            prompt.retry_prefix = Some(prefix);
        }
        self.clear_turn(prefix);
    }

    /// Adopt only a locally identified turn, never a text-matched legacy turn.
    /// No request is sent: the next explicit send restores this history first.
    pub(in crate::window) fn adopt_interrupted(
        &mut self,
        history: &mut Vec<client::Message>,
    ) -> bool {
        let prefix = history.len();
        if prefix > 126 || !prefix.is_multiple_of(2) {
            return false;
        }
        let Some(prompt_index) = self
            .messages
            .iter()
            .rposition(|message| message.role == Role::User && message.turn_prefix == Some(prefix))
        else {
            return false;
        };
        let Some(answer_offset) = self.messages[prompt_index + 1..]
            .iter()
            .rposition(|message| {
                message.role == Role::Assistant
                    && message.turn_prefix == Some(prefix)
                    && !message.body.trim().is_empty()
            })
        else {
            return false;
        };
        let answer_index = prompt_index + 1 + answer_offset;
        let prompt = &self.messages[prompt_index];
        let answer = &self.messages[answer_index];
        if prompt.body.is_empty()
            || prompt.body.len() > 1024 * 1024
            || answer.body.len() > 1024 * 1024
            || answer.reasoning.len() > 1024 * 1024
        {
            return false;
        }
        history.push(client::Message {
            role: "user",
            content: prompt.body.as_str().into(),
            reasoning_content: None,
        });
        history.push(client::Message {
            role: "assistant",
            content: answer.body.as_str().into(),
            reasoning_content: (!answer.reasoning.is_empty())
                .then(|| answer.reasoning.as_str().into()),
        });
        for message in &mut self.messages[prompt_index..] {
            if message.turn_prefix == Some(prefix) {
                message.turn_prefix = None;
                message.pending = false;
            }
        }
        self.messages[prompt_index].history_index = Some(prefix);
        self.messages[answer_index].history_index = Some(prefix + 1);
        self.messages[answer_index].interrupted = true;
        true
    }

    /// Older snapshots saved the active answer and completion state, but had no
    /// turn marker. Use that state only for the last unbound prompt/answer pair.
    pub(in crate::window) fn recover_interrupted(
        &mut self,
        history: &mut Vec<client::Message>,
        last_answer: &str,
        answer_complete: bool,
    ) -> bool {
        if self.adopt_interrupted(history) {
            return true;
        }
        if answer_complete || last_answer.trim().is_empty() || history.len() > 126 {
            return false;
        }
        let Some(answer_index) = self.messages.len().checked_sub(1) else {
            return false;
        };
        let answer = &self.messages[answer_index];
        if answer.role != Role::Assistant
            || answer.history_index.is_some()
            || answer.turn_prefix.is_some()
            || answer.body != last_answer
        {
            return false;
        }
        let Some(prompt_index) = self.messages[..answer_index]
            .iter()
            .rposition(|message| message.role == Role::User)
        else {
            return false;
        };
        if self.messages[prompt_index].history_index.is_some()
            || self.messages[prompt_index].turn_prefix.is_some()
        {
            return false;
        }
        let prefix = history.len();
        self.messages[prompt_index].turn_prefix = Some(prefix);
        self.messages[answer_index].turn_prefix = Some(prefix);
        let adopted = self.adopt_interrupted(history);
        if !adopted {
            self.clear_turn(prefix);
        }
        adopted
    }

    pub(in crate::window) fn clear_turn(&mut self, prefix: usize) {
        for message in &mut self.messages {
            if message.turn_prefix == Some(prefix) {
                message.turn_prefix = None;
            }
        }
    }

    pub(in crate::window) fn latest_interrupted(&self, history: &[client::Message]) -> bool {
        history.len().checked_sub(1).is_some_and(|last| {
            self.messages.iter().any(|message| {
                message.interrupted && message.committed_index(history) == Some(last)
            })
        })
    }

    pub(in crate::window) fn copy_interrupted_marks(
        &mut self,
        source: &Self,
        history: &[client::Message],
    ) {
        for message in &mut self.messages {
            let Some(index) = message.committed_index(history) else {
                continue;
            };
            message.interrupted = source.messages.iter().any(|original| {
                original.interrupted && original.committed_index(history) == Some(index)
            });
        }
    }

    pub(in crate::window) fn committed_index(
        &self,
        id: u64,
        history: &[client::Message],
    ) -> Option<usize> {
        self.messages
            .iter()
            .find(|message| message.id == id)
            .and_then(|message| message.committed_index(history))
    }

    pub(in crate::window) fn bind_committed(
        &mut self,
        previous: &[client::Message],
        history: &[client::Message],
    ) {
        if history.len() != previous.len().saturating_add(2)
            || !history.iter().zip(previous).all(|(left, right)| {
                left.role == right.role
                    && left.content == right.content
                    && left.reasoning_content == right.reasoning_content
            })
        {
            return;
        }
        let prompt_index = previous.len();
        let answer_index = prompt_index + 1;
        if history[prompt_index].role != "user" || history[answer_index].role != "assistant" {
            return;
        }
        if let Some(prompt) = self
            .messages
            .iter_mut()
            .rfind(|message| message.role == Role::User)
            && prompt.matches_history(&history[prompt_index])
        {
            prompt.history_index = Some(prompt_index);
        }
        if let Some(answer) = self.messages.last_mut()
            && answer.matches_history(&history[answer_index])
        {
            answer.history_index = Some(answer_index);
        }
        self.clear_turn(prompt_index);
    }

    pub(in crate::window) fn from_committed(history: &[client::Message]) -> Self {
        let mut transcript = Self::default();
        for (index, message) in history.iter().enumerate() {
            let role = if message.role == "user" {
                Role::User
            } else {
                Role::Assistant
            };
            transcript.push(role, message.content.to_string());
            if let Some(visible) = transcript.messages.last_mut() {
                visible.history_index = Some(index);
                visible.pending = false;
                if let Some(reasoning) = &message.reasoning_content {
                    visible.reasoning = reasoning.to_string();
                    transcript.text_bytes = transcript.text_bytes.saturating_add(reasoning.len());
                }
            }
            transcript.trim();
        }
        transcript
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(role: &'static str, text: &str) -> client::Message {
        client::Message {
            role,
            content: text.into(),
            reasoning_content: None,
        }
    }

    #[test]
    fn interruption_uses_last_identified_effort_stage_and_rejects_unidentified_text() {
        let mut history = vec![
            entry("user", "Same prompt"),
            entry("assistant", "Old answer"),
        ];
        let mut transcript = Transcript::from_committed(&history);
        transcript.start_turn("Same prompt".into(), history.len());
        transcript.append("First draft", false);
        transcript.stage();
        transcript.append("Partial revision", false);
        assert!(transcript.adopt_interrupted(&mut history));
        assert_eq!(history.len(), 4);
        assert_eq!(history[3].content.as_ref(), "Partial revision");
        assert_eq!(transcript.messages[2].committed_index(&history), Some(2));
        assert_eq!(transcript.messages[3].committed_index(&history), None);
        assert_eq!(transcript.messages[4].committed_index(&history), Some(3));
        assert!(transcript.messages[4].interrupted);
        assert!(!transcript.adopt_interrupted(&mut history));

        let mut legacy = Transcript::from_committed(&history);
        legacy.push(Role::User, "Same prompt".into());
        legacy.push(Role::Assistant, "Partial revision".into());
        assert!(!legacy.adopt_interrupted(&mut history));
        assert_eq!(history.len(), 4);
    }

    #[test]
    fn interruption_legacy_recovery_requires_unfinished_snapshot_and_exact_tail() {
        let mut history = vec![entry("user", "Same"), entry("assistant", "Old")];
        let mut transcript = Transcript::from_committed(&history);
        transcript.push(Role::User, "Same".into());
        transcript.push(Role::Assistant, "Partial".into());
        assert!(!transcript.recover_interrupted(&mut history, "Partial", true));
        assert!(!transcript.recover_interrupted(&mut history, "Different", false));
        assert!(transcript.recover_interrupted(&mut history, "Partial", false));
        assert_eq!(history[2].content.as_ref(), "Same");
        assert_eq!(history[3].content.as_ref(), "Partial");
        assert!(!transcript.recover_interrupted(&mut history, "Partial", false));
    }

    #[test]
    fn iteration_binding_distinguishes_duplicate_committed_and_interrupted_turns() {
        let history = vec![
            entry("user", "Again"),
            entry("assistant", "Answer"),
            entry("user", "Again"),
            entry("assistant", "Answer"),
        ];
        let mut transcript = Transcript::from_committed(&history[..2]);
        transcript.push(Role::User, "Again".into());
        transcript.push(Role::Assistant, "Answer".into());
        let interrupted = transcript.messages[2].id;
        assert_eq!(transcript.committed_index(interrupted, &history[..2]), None);
        transcript.bind_committed(&history[..2], &history);
        assert_eq!(transcript.committed_index(interrupted, &history), Some(2));
        assert_eq!(
            transcript.committed_index(transcript.messages[0].id, &history),
            Some(0)
        );
        transcript.push(Role::User, "Again".into());
        transcript.push(Role::Assistant, "Answer".into());
        transcript.remove_empty_response();
        let uncommitted = transcript.messages[4].id;
        assert_eq!(transcript.committed_index(uncommitted, &history), None);
    }

    #[test]
    fn iteration_binding_survives_eviction_recovery_and_rejects_mismatched_identity()
    -> std::io::Result<()> {
        let history = (0..520)
            .map(|index: usize| {
                entry(
                    if index.is_multiple_of(2) {
                        "user"
                    } else {
                        "assistant"
                    },
                    &index.to_string(),
                )
            })
            .collect::<Vec<_>>();
        let transcript = Transcript::from_committed(&history);
        assert_eq!(transcript.messages.len(), super::super::MAX_MESSAGES);
        assert_eq!(transcript.messages[0].history_index, Some(8));
        let bytes = serde_json::to_vec(&transcript)?;
        let restored: Transcript = serde_json::from_slice(&bytes)?;
        assert_eq!(
            restored.committed_index(restored.messages[0].id, &history),
            Some(8)
        );
        let mut changed = history;
        changed[8].content = "Changed".into();
        assert_eq!(
            restored.committed_index(restored.messages[0].id, &changed),
            None
        );
        Ok(())
    }

    #[test]
    fn iteration_legacy_duplicate_text_never_establishes_committed_positions() {
        let history = vec![entry("user", "Question"), entry("assistant", "Answer")];
        // An interrupted duplicate after display eviction can have exactly the
        // same text as committed history. Only a saved index or a fresh commit
        // event establishes identity; legacy text is never linked by equality.
        let mut transcript = Transcript::from_committed(&history);
        for message in &mut transcript.messages {
            message.history_index = None;
        }
        for message in &transcript.messages {
            assert_eq!(message.committed_index(&history), None);
        }
        transcript.push(Role::User, "Question".into());
        transcript.push(Role::Assistant, "Answer".into());
        let mut extended = history.clone();
        extended.extend(history.iter().cloned());
        transcript.bind_committed(&history, &extended);
        assert_eq!(transcript.messages[0].history_index, None);
        assert_eq!(transcript.messages[1].history_index, None);
        assert_eq!(transcript.messages[2].history_index, Some(2));
        assert_eq!(transcript.messages[3].history_index, Some(3));
    }

    #[test]
    fn iteration_commit_links_only_final_effort_answer_and_validated_original_prefix() {
        let history = vec![entry("user", "Question"), entry("assistant", "Final")];
        let mut transcript = Transcript::default();
        transcript.push(Role::User, "Question".into());
        transcript.push(Role::Assistant, "Draft".into());
        transcript.stage();
        transcript.append("Final", false);
        transcript.bind_committed(&[], &history);
        assert_eq!(transcript.messages[0].history_index, Some(0));
        assert_eq!(transcript.messages[1].history_index, None);
        assert_eq!(transcript.messages[2].history_index, Some(1));

        transcript.push(Role::User, "Later".into());
        transcript.push(Role::Assistant, "Answer".into());
        let mut changed = history.clone();
        changed[1].content = "Different prefix".into();
        changed.extend([entry("user", "Later"), entry("assistant", "Answer")]);
        transcript.bind_committed(&history, &changed);
        assert_eq!(transcript.messages[3].history_index, None);
        assert_eq!(transcript.messages[4].history_index, None);
    }
}
