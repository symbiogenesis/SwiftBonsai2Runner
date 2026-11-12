//! Background response ownership with inert model and speech channels.
use super::*;

fn background() -> (State, Receiver<Command>, client::EventSender, u64) {
    let (commands, incoming) = mpsc::channel();
    let (events, incoming_events) = client::event_channel();
    let mut state = State::create(commands, incoming_events, None, None);
    state.preview = false;
    state.model = ModelLifecycle::Ready;
    state.input = "Original question".into();
    state.submit();
    assert!(matches!(incoming.try_recv(), Ok(Command::Send { .. })));
    let owner = state.active_conversation;
    state.input = "Owner's next draft".into();
    state.new_chat();
    assert_ne!(state.active_conversation, owner);
    state.input = "Browsing draft 🌱".into();
    assert!(incoming.try_recv().is_err());
    (state, incoming, events, owner)
}

fn completed_history() -> Vec<client::Message> {
    vec![
        client::Message {
            role: "user",
            content: "Original question".into(),
            reasoning_content: None,
        },
        client::Message {
            role: "assistant",
            content: "Finished answer".into(),
            reasoning_content: Some("Reasoning".into()),
        },
    ]
}

#[test]
fn background_completion_preserves_browsing_drafts_and_restores_selected_history() {
    let (mut state, commands, _events, owner) = background();
    let selected = state.active_conversation;
    state.extra_effort = true;
    state.output_budget = 1;
    state.focus_composer = false;
    state.place_composer_cursor(3);
    let cursor = (state.composer_cursor, state.pending_cursor);
    state.submit();
    assert!(commands.try_recv().is_err());

    state.model_event(Event::Stage("Draft"));
    state.model_event(Event::Reasoning("Reasoning".into()));
    state.model_event(Event::Text("Finished answer".into()));
    state.model_event(Event::Draft("Finished answer".into()));
    state.model_event(Event::Committed(completed_history()));
    assert!(state.model.busy());
    assert!(state.transcript.messages.is_empty() && state.committed.is_empty());
    assert_eq!(state.active_conversation, selected);
    assert_eq!(state.input, "Browsing draft 🌱");
    assert_eq!((state.composer_cursor, state.pending_cursor), cursor);
    assert!(!state.focus_composer);
    assert!(commands.try_recv().is_err());
    state.remove_conversation(owner);
    assert_eq!(state.conversations.len(), 2);

    state.switch_conversation(owner);
    assert_eq!(state.stage, Some("Draft"));
    assert_eq!(state.input, "Owner's next draft");
    state.switch_conversation(selected);
    state.model_event(Event::Complete(Completion::Finished));
    assert_eq!(state.model, ModelLifecycle::Ready);
    assert_eq!(state.input, "Browsing draft 🌱");
    assert!(state.extra_effort && state.output_budget == 1);
    assert!(matches!(commands.try_recv(), Ok(Command::RestoreChat(history)) if history.is_empty()));
    state.submit();
    assert!(
        matches!(commands.try_recv(), Ok(Command::Send { text, .. }) if text == "Browsing draft 🌱")
    );
    assert!(commands.try_recv().is_err());
    state.switch_conversation(owner);
    assert_eq!(state.last_answer, "Finished answer");
    assert_eq!(state.committed.len(), 2);
    assert_eq!(
        state.committed[1].reasoning_content.as_deref(),
        Some("Reasoning")
    );
    assert!(state.answer_complete);
}

#[test]
fn background_stop_completion_race_preserves_both_drafts_and_exact_owner_history() {
    let (mut state, commands, _events, owner) = background();
    state.model_event(Event::Text("Partial".into()));
    state.stop_generation();
    assert!(matches!(commands.try_recv(), Ok(Command::Stop)));
    state.model_event(Event::Text("Ignored after Stop".into()));
    state.model_event(Event::Committed(completed_history()));
    state.model_event(Event::Stopping("Original question".into()));
    state.model_event(Event::Stopped);
    assert_eq!(state.model, ModelLifecycle::Retryable);
    assert_eq!(state.input, "Browsing draft 🌱");
    assert!(state.transcript.messages.is_empty());
    state.switch_conversation(owner);
    assert_eq!(state.input, "Owner's next draft");
    assert_eq!(state.last_answer, "Finished answer");
    assert_eq!(state.committed.len(), 2);
    assert!(state.transcript.plain_text().contains("Finished answer"));
    assert!(!state.transcript.plain_text().contains("Ignored after Stop"));
    assert!(commands.try_recv().is_err());
}

#[test]
fn background_rejection_restores_only_empty_owner_drafts_and_preserves_lifecycle() {
    for retry_required in [false, true] {
        for owner_draft in ["", "Keep newer draft"] {
            let (mut state, commands, _events, owner) = background();
            let selected = state.active_conversation;
            state.switch_conversation(owner);
            owner_draft.clone_into(&mut state.input);
            state.switch_conversation(selected);
            state.model_event(Event::Rejected {
                message: "Rejected response".into(),
                text: "Original question".into(),
                retry_required,
            });
            assert_eq!(state.input, "Browsing draft 🌱");
            assert_eq!(
                state.model,
                if retry_required {
                    ModelLifecycle::AwaitingTeardown
                } else {
                    ModelLifecycle::Ready
                }
            );
            if retry_required {
                assert!(commands.try_recv().is_err());
                state.model_event(Event::Retryable("Cleanup complete".into()));
            } else {
                assert!(
                    matches!(commands.try_recv(), Ok(Command::RestoreChat(history)) if history.is_empty())
                );
            }
            state.switch_conversation(owner);
            assert_eq!(
                state.input,
                if owner_draft.is_empty() {
                    "Original question"
                } else {
                    owner_draft
                }
            );
            assert!(state.committed.is_empty());
        }
    }
}

#[test]
fn background_restart_late_rejection_preserves_global_status_and_visible_draft() {
    let (mut state, commands, _events, owner) = background();
    state.stop_generation();
    assert!(matches!(commands.try_recv(), Ok(Command::Stop)));
    state.model_event(Event::Stopping("Original question".into()));
    state.model_event(Event::Stopped);
    state.retry();
    assert!(matches!(commands.try_recv(), Ok(Command::Retry)));
    let status = state.status.clone();
    state.model_event(Event::Rejected {
        message: "Old request could not run".into(),
        text: "Original question".into(),
        retry_required: true,
    });
    assert_eq!(state.model, ModelLifecycle::Restarting);
    assert_eq!(state.status, status);
    assert_eq!(state.input, "Browsing draft 🌱");
    state.model_event(Event::Starting);
    state.model_event(Event::Ready);
    assert!(matches!(commands.try_recv(), Ok(Command::RestoreChat(history)) if history.is_empty()));
    state.switch_conversation(owner);
    assert_eq!(state.input, "Owner's next draft");
}

#[test]
fn background_disconnect_cleans_up_the_owner_and_preserves_browsed_content() {
    let (mut state, commands, events, owner) = background();
    state
        .transcript
        .push(Role::Assistant, "Existing browsed answer".into());
    drop(events);
    state.drain_events();
    assert_eq!(state.model, ModelLifecycle::Unavailable);
    assert_eq!(state.input, "Browsing draft 🌱");
    assert!(state.warning);
    assert_eq!(state.transcript.messages[0].body, "Existing browsed answer");
    state.switch_conversation(owner);
    assert_eq!(state.transcript.messages.len(), 1);
    assert_eq!(state.input, "Owner's next draft");
    assert!(commands.try_recv().is_err());
}

#[test]
fn background_recovery_keeps_partial_owner_text_separate_from_committed_history() -> io::Result<()>
{
    let (mut state, _commands, _events, owner) = background();
    state.model_event(Event::Text("Partial owner answer".into()));
    let saved: serde_json::Value = serde_json::from_slice(&state.workspace_bytes()?)?;
    assert_eq!(saved["active"]["input"], "Browsing draft 🌱");
    assert_eq!(
        saved["active"]["transcript"]["messages"],
        serde_json::json!([])
    );
    let owner = saved["conversations"]
        .as_array()
        .and_then(|conversations| conversations.iter().find(|value| value["id"] == owner))
        .ok_or_else(|| io::Error::other("Owner conversation was not saved"))?;
    assert_eq!(owner["saved"]["input"], "Owner's next draft");
    assert_eq!(
        owner["saved"]["transcript"]["messages"][1]["body"],
        "Partial owner answer"
    );
    assert_eq!(owner["saved"]["committed"], serde_json::json!([]));
    assert!(owner["saved"].get("stage").is_none());
    Ok(())
}

#[test]
fn background_speech_never_starts_hidden_playback_or_finishes_selected_playback() -> io::Result<()>
{
    let (mut state, _commands, _events, owner) = background();
    let selected = state.active_conversation;
    let (session, speech_commands) = speech::Session::fixture()?;
    state.speech = Some(session);
    state.speech_output = true;
    state
        .transcript
        .push(Role::Assistant, "Selected answer".into());
    state.transcript.remove_empty_response();
    state.last_answer = "Selected answer".into();
    state.toggle_streaming_read_aloud();
    assert!(speech_commands.try_recv().is_err());
    state.switch_conversation(owner);
    state.streaming_read_aloud = true;
    state.switch_conversation(selected);
    while speech_commands.try_recv().is_ok() {}
    state.streaming_suppressed = false;
    let message = state.transcript.messages[0].id;
    state.toggle_read_aloud(message);
    while speech_commands.try_recv().is_ok() {}
    assert!(state.speaking);
    state.model_event(Event::Stage("Draft"));
    state.model_event(Event::Text("Hidden answer".into()));
    state.model_event(Event::Complete(Completion::Finished));
    assert!(state.speaking);
    assert!(speech_commands.try_recv().is_err());
    Ok(())
}
