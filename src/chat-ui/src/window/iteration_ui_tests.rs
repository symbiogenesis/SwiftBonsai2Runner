//! Message menus dispatch explicit actions through the real native widgets.
use super::*;
use egui_kittest::{Harness, kittest::Queryable as _};

struct Fixture {
    app: State,
    commands: Receiver<Command>,
    _events: client::EventSender,
}

fn harness(size: [f32; 2]) -> Harness<'static, Fixture> {
    let (commands, incoming) = mpsc::channel();
    let (events, incoming_events) = client::event_channel();
    let mut app = State::create(commands, incoming_events, None, None);
    app.preview = false;
    app.model = ModelLifecycle::Ready;
    app.input = "Keep my next draft 🌱".into();
    app.transcript.push(Role::User, "Original question".into());
    app.transcript
        .push(Role::Assistant, "Original answer".into());
    app.transcript.remove_empty_response();
    app.committed = vec![
        client::Message {
            role: "user",
            content: "Original question".into(),
            reasoning_content: None,
        },
        client::Message {
            role: "assistant",
            content: "Original answer".into(),
            reasoning_content: None,
        },
    ];
    app.transcript.link_legacy_history(&app.committed);
    Harness::builder()
        .with_size(size)
        .with_context_setup(|context| {
            configure(context);
            context.add_font_provider(std::sync::Arc::new(
                egui_system_font_provider::SystemFontProvider::new(),
            ));
        })
        .build_ui_state(
            |ui, fixture: &mut Fixture| {
                fixture.app.handle_input_shortcuts(ui.ctx());
                fixture.app.draw(ui);
            },
            Fixture {
                app,
                commands: incoming,
                _events: events,
            },
        )
}

fn open_actions(chat: &mut Harness<'_, Fixture>, id: u64) {
    chat.get_by_role_and_label(egui::Role::Button, &format!("Actions for message {id}"))
        .click();
    chat.run();
}

#[test]
fn iteration_menu_edit_escape_preserves_original_draft_and_blocks_global_shortcuts() {
    for size in [[1180.0, 860.0], [620.0, 800.0]] {
        let mut chat = harness(size);
        let original = chat.state().app.active_conversation;
        open_actions(&mut chat, 1);
        chat.get_by_role_and_label(egui::Role::Button, "Edit and resend")
            .click();
        chat.run();
        assert!(chat.state().app.edit_message.is_some());
        chat.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::N);
        chat.key_press(egui::Key::Enter);
        chat.run();
        assert_eq!(chat.state().app.conversations.len(), 1);
        assert!(chat.state().commands.try_recv().is_err());
        chat.key_press(egui::Key::Escape);
        chat.run();
        assert!(chat.state().app.edit_message.is_none());
        assert_eq!(chat.state().app.active_conversation, original);
        assert_eq!(chat.state().app.input, "Keep my next draft 🌱");
        assert_eq!(chat.state().app.committed.len(), 2);
        assert!(chat.state().commands.try_recv().is_err());
    }
}

#[test]
fn iteration_menu_branches_without_sending_and_continue_keeps_a_draft() {
    let mut chat = harness([1180.0, 860.0]);
    let original = chat.state().app.active_conversation;
    open_actions(&mut chat, 2);
    chat.get_by_role_and_label(egui::Role::Button, "Branch from here")
        .click();
    chat.run();
    assert_ne!(chat.state().app.active_conversation, original);
    assert_eq!(chat.state().app.conversations.len(), 2);
    assert_eq!(chat.state().app.committed.len(), 2);
    assert!(matches!(
        chat.state().commands.try_recv(),
        Ok(Command::RestoreChat(history)) if history.len() == 2
    ));
    assert!(chat.state().commands.try_recv().is_err());
    chat.state_mut().app.input = "Branch draft".into();
    open_actions(&mut chat, 2);
    chat.get_by_role_and_label(egui::Role::Button, "Continue response")
        .click();
    chat.run();
    assert!(chat.state().app.model.busy());
    assert_eq!(chat.state().app.input, "Branch draft");
    assert!(matches!(
        chat.state().commands.try_recv(),
        Ok(Command::Send { text, .. }) if text.contains("Continue")
    ));
    assert!(chat.state().commands.try_recv().is_err());
    let saved = chat
        .state()
        .app
        .conversations
        .iter()
        .find(|conversation| conversation.id == original)
        .and_then(|conversation| conversation.saved.as_ref());
    assert!(saved.is_some_and(|snapshot| snapshot.input == "Keep my next draft 🌱"));
}
