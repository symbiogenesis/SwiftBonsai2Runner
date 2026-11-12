//! Capacity guidance follows the same histories and controls as actual requests.
use super::*;
use crate::{client, window::transcript::Role};
use egui_kittest::{
    Harness,
    kittest::{NodeT as _, Queryable as _},
};
use std::sync::mpsc;

fn state() -> (State, mpsc::Receiver<client::Command>) {
    let (commands, incoming) = mpsc::channel();
    let (_events, events) = client::event_channel();
    let mut state = State::create(commands, events, None, None);
    state.preview = false;
    state.model = ModelLifecycle::Ready;
    (state, incoming)
}

fn history(text: &str) -> Vec<client::Message> {
    vec![
        client::Message {
            role: "user",
            content: text.into(),
            reasoning_content: None,
        },
        client::Message {
            role: "assistant",
            content: "Answer".into(),
            reasoning_content: Some("Retained reasoning".into()),
        },
    ]
}

struct Fixture {
    app: State,
    commands: mpsc::Receiver<client::Command>,
}

fn harness(size: [f32; 2], text: &str) -> Harness<'static, Fixture> {
    harness_with_render(size, text, false)
}

fn harness_with_render(size: [f32; 2], text: &str, render: bool) -> Harness<'static, Fixture> {
    let (mut app, commands) = state();
    app.input = "Keep this draft".into();
    app.commit_history(history(text));
    let builder = Harness::builder()
        .with_size(size)
        .with_context_setup(|context| {
            crate::window::configure(context);
            context.add_font_provider(std::sync::Arc::new(
                egui_system_font_provider::SystemFontProvider::new(),
            ));
        });
    let builder = if render {
        let mut setup = eframe::egui_wgpu::WgpuSetupCreateNew::without_display_handle();
        setup.instance_descriptor.backends = wgpu::Backends::DX12;
        builder.wgpu_setup(eframe::egui_wgpu::WgpuSetup::CreateNew(setup))
    } else {
        builder
    };
    builder.build_ui_state(
        |ui, fixture: &mut Fixture| {
            fixture.app.handle_input_shortcuts(ui.ctx());
            fixture.app.draw(ui);
        },
        Fixture { app, commands },
    )
}

#[test]
fn context_capacity_capture_when_requested() -> std::io::Result<()> {
    let Some(value) = std::env::var_os("BONSAI_UI_CAPTURE_DIRECTORY") else {
        return Ok(());
    };
    let directory = std::path::PathBuf::from(value);
    bonsai_windows_credentials::validate_external_path(&directory)?;
    std::fs::create_dir_all(&directory)?;
    for (name, size, details) in [
        ("context-details-wide.png", [1180.0, 860.0], true),
        ("context-warning-narrow.png", [620.0, 640.0], false),
    ] {
        let mut chat = harness_with_render(size, "Help me refine this idea.", true);
        let mut messages = history("Help me refine this idea.");
        messages[1].content =
            "A clear question is a good place to begin. What would you like to explore?".into();
        messages[1].reasoning_content = Some("a".repeat(20_000).into());
        chat.state_mut().app.commit_history(messages);
        let transcript = crate::window::Transcript::from_committed(&chat.state().app.committed);
        chat.state_mut().app.transcript = transcript;
        chat.run();
        if details {
            chat.get_by_role_and_label(egui::Role::Button, "Context capacity")
                .click();
            chat.run();
        }
        chat.render()
            .map_err(std::io::Error::other)?
            .save(directory.join(name))
            .map_err(std::io::Error::other)?;
        assert!(chat.state().app.worker.is_none());
        assert!(chat.state().app.speech.is_none());
    }
    Ok(())
}

#[test]
fn context_capacity_cache_tracks_equal_length_edits_and_committed_replacement() {
    let (mut app, _commands) = state();
    app.commit_history(history("aaaaaaaa"));
    app.input = "aaaaaaaa".into();
    let before = app.context_estimate();
    app.input = "{}[]();=".into();
    assert!(app.context_estimate().draft > before.draft);
    app.commit_history(history("{}[]();="));
    assert!(app.context_estimate().history > before.history);
    let committed = app.context_estimate();
    app.transcript
        .push(Role::Assistant, "uncommitted".repeat(1_000));
    assert_eq!(app.context_estimate(), committed);
    // No copy or fresh estimate is needed for an unchanged draft/history.
    let allocation = app.context_draft.text.as_ptr();
    assert_eq!(app.context_estimate(), committed);
    assert_eq!(app.context_draft.text.as_ptr(), allocation);
}

#[test]
fn context_capacity_background_commit_and_switch_preserve_selected_estimates() {
    let (mut app, _commands) = state();
    app.commit_history(history("short"));
    app.input = "Original draft".into();
    let original = app.active_conversation;
    app.context_estimate();
    app.new_chat();
    app.input = "Other draft".into();
    let selected = app.active_conversation;
    let visible = app.context_estimate();
    app.request_conversation = Some(original);
    app.model = ModelLifecycle::Generating;
    app.with_request_conversation(|owner| owner.commit_history(history(&"long".repeat(1_000))));
    assert_eq!(app.active_conversation, selected);
    assert_eq!(app.context_estimate(), visible);
    assert!(!app.context_pending());
    app.switch_conversation(original);
    assert!(app.context_pending());
    assert!(app.context_estimate().history > visible.history);
    assert_eq!(app.input, "Original draft");
}

#[test]
fn context_capacity_recovery_and_branch_rebuild_without_saving_estimates() -> std::io::Result<()> {
    let (mut app, _commands) = state();
    app.commit_history(history("Original question"));
    app.transcript = crate::window::Transcript::from_committed(&app.committed);
    app.input = "A saved draft".into();
    let expected = app.context_estimate();
    let bytes = app.workspace_bytes()?;
    assert!(!String::from_utf8_lossy(&bytes).contains("context_history"));
    // Exercise the production deserialization path without a DPAPI/disk workload.
    let mut snapshot: crate::window::workspace::ConversationSnapshot = serde_json::from_value(
        serde_json::from_slice::<serde_json::Value>(&bytes)?["active"].clone(),
    )?;
    assert!(snapshot.context_history.is_none());
    snapshot.warning = false;
    let (mut restored, _commands) = state();
    restored.load_conversation(snapshot);
    assert_eq!(restored.context_estimate(), expected);
    let message = app.transcript.messages[0].id;
    app.message_action(crate::window::iteration::MessageAction::Branch(message));
    assert_eq!(app.context_estimate().history, 0);
    assert_eq!(app.input, "Original question");
    Ok(())
}

#[test]
fn context_capacity_warning_keeps_send_available_and_reply_choice_reaches_worker() {
    for size in [[1180.0, 860.0], [620.0, 640.0]] {
        let mut chat = harness(size, &"a".repeat(20_000));
        assert_eq!(
            chat.state_mut().app.context_estimate().pressure(),
            Pressure::Over
        );
        assert!(
            !chat
                .get_by_role_and_label(egui::Role::Button, "Send  ↑")
                .is_disabled()
        );
        let send = chat
            .get_by_role_and_label(egui::Role::Button, "Send  ↑")
            .rect();
        assert!(egui::Rect::from_min_size(egui::Pos2::ZERO, size.into()).contains_rect(send));
        chat.get_by_role_and_label(egui::Role::Button, "Use 2,048-token reply")
            .click();
        chat.run();
        assert_eq!(chat.state().app.input, "Keep this draft");
        assert_eq!(chat.state().app.output_budget, 2);
        assert!(chat.state().app.recovery_dirty);
        chat.get_by_role_and_label(egui::Role::Button, "Send  ↑")
            .click();
        chat.run();
        assert!(chat.state().commands.try_iter().any(|command| matches!(
            command,
            client::Command::Send { max_completion_tokens: 2048, text, .. } if text == "Keep this draft"
        )));
    }
}

#[test]
fn context_capacity_popover_is_keyboard_accessible_and_scopes_extra_effort() {
    let mut chat = harness([620.0, 640.0], "Question");
    chat.state_mut().app.extra_effort = true;
    chat.run();
    chat.get_by_role_and_label(egui::Role::Button, "Context capacity")
        .focus();
    chat.key_press(egui::Key::Space);
    chat.run();
    assert!(chat.query_by_label("Space for this draft").is_some());
    assert!(chat.query_by_label("First pass only. Extra effort's later passes add generated notes and drafts and need more room.").is_some());
    chat.key_press(egui::Key::Enter);
    chat.run();
    assert!(chat.state().commands.try_recv().is_err());
    chat.key_press(egui::Key::Escape);
    chat.run();
    assert!(chat.query_by_label("Space for this draft").is_none());
    assert_eq!(chat.state().app.input, "Keep this draft");
    assert!(chat.state().commands.try_recv().is_err());
}

#[test]
fn context_capacity_start_fresh_preserves_original_draft_and_full_history() {
    let mut chat = harness([620.0, 640.0], &"a".repeat(40_000));
    let original = chat.state().app.active_conversation;
    chat.get_by_role_and_label(egui::Role::Button, "Start fresh")
        .click();
    chat.run();
    assert_ne!(chat.state().app.active_conversation, original);
    assert_eq!(chat.state_mut().app.context_estimate().history, 0);
    chat.state_mut().app.switch_conversation(original);
    chat.run();
    assert_eq!(chat.state().app.input, "Keep this draft");
    assert_eq!(chat.state().app.committed.len(), 2);
    assert!(
        chat.state()
            .commands
            .try_iter()
            .all(|command| !matches!(command, client::Command::Send { .. }))
    );
}

#[test]
fn context_capacity_escape_closes_details_before_stopping_generation() {
    let mut chat = harness([620.0, 640.0], "Question");
    let owner = chat.state().app.active_conversation;
    chat.state_mut().app.model = ModelLifecycle::Generating;
    chat.state_mut().app.request_conversation = Some(owner);
    chat.run();
    chat.get_by_role_and_label(egui::Role::Button, "Context capacity")
        .click();
    chat.run();
    assert!(chat.query_by_label("Space for this draft").is_some());
    assert!(
        chat.query_by_label(
            "Updates when this response finishes. The unfinished turn is not included below."
        )
        .is_some()
    );
    chat.key_press(egui::Key::Escape);
    chat.run();
    assert!(chat.query_by_label("Space for this draft").is_none());
    assert!(chat.state().app.model.busy());
    assert!(chat.state().commands.try_recv().is_err());
    chat.key_press(egui::Key::Escape);
    chat.run();
    assert!(matches!(
        chat.state().commands.try_recv(),
        Ok(client::Command::Stop)
    ));
}
