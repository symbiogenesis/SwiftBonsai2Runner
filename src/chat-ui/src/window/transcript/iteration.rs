//! Bind visible messages to exact committed positions before allowing iteration.
use super::{Message, Role, Transcript};
use crate::{client, window::iteration::MessageOptions};

impl Message {
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
