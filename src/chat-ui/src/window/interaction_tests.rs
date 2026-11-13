//! Real egui input/accessibility regressions with inert backend channels.
//! GPU rendering is opt-in and writes captures only to validated external storage.
use super::*;
use egui_kittest::{
    Harness,
    kittest::{NodeT as _, Queryable as _},
};
use std::{fs, path::PathBuf};

const QUESTION: &str = "I have a few ideas for a short story, but I am not sure where to start.";
const ANSWER: &str = "Start with the moment something changes.\n\nA quiet bookshop receives a letter addressed to someone who has never existed. Its owner recognizes the handwriting as their own.\n\nWhat draws you in more: the mystery of the letter, or the person who finds it?";

struct Fixture {
    app: State,
    commands: Receiver<Command>,
    _events: client::EventSender,
}

impl Fixture {
    fn new(seeded: bool) -> Self {
        let (commands, incoming) = mpsc::channel();
        let (events, incoming_events) = client::event_channel();
        let mut app = State::create(commands, incoming_events, None, None);
        app.preview = false;
        app.model = ModelLifecycle::Ready;
        app.status = "Ready when you are.".to_owned();
        if seeded {
            app.transcript.push(Role::User, QUESTION.to_owned());
            app.transcript.push(Role::Assistant, ANSWER.to_owned());
            app.transcript.remove_empty_response();
            app.last_answer = ANSWER.to_owned();
            app.answer_complete = true;
        }
        Self {
            app,
            commands: incoming,
            _events: events,
        }
    }
}

fn harness(size: [f32; 2], seeded: bool) -> Harness<'static, Fixture> {
    Harness::builder()
        .with_size(size)
        .with_theme(egui::Theme::Dark)
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
            Fixture::new(seeded),
        )
}

fn open_sidebar(chat: &mut Harness<'_, Fixture>, size: [f32; 2]) {
    if size[0] < 900.0 {
        chat.get_by_role_and_label(egui::Role::Button, "Conversation menu")
            .click();
        chat.run();
    }
}

fn request_conversation_deletion(chat: &mut Harness<'_, Fixture>, title: &str) {
    chat.get_by_role_and_label(egui::Role::Button, &format!("Actions for {title}"))
        .click();
    chat.run();
    chat.get_by_role_and_label(egui::Role::Button, "Delete")
        .click();
    chat.run();
    assert!(
        chat.get_by_label("Delete conversation?")
            .rect()
            .is_positive()
    );
}

#[test]
fn conversation_deletion_confirms_with_one_pointer_click_after_repeated_reopening() {
    for size in [[1180.0, 860.0], [620.0, 640.0]] {
        let mut chat = harness(size, false);
        for cycle in 0..3 {
            let id = chat.state().app.active_conversation;
            let title = format!("Conversation {cycle}");
            chat.state_mut().app.conversations[0].title = title.clone();
            chat.state_mut().app.input = format!("Unsent draft {cycle}");
            chat.run();
            // The narrow drawer closes when the confirmation dialog opens.
            // Each repeated deletion must enter through navigation again.
            open_sidebar(&mut chat, size);
            request_conversation_deletion(&mut chat, &title);
            assert_eq!(chat.state().app.delete_conversation, Some(id));
            // Node::click sends pointer movement, press and release; AccessKit
            // activation would bypass the hit testing this regression covers.
            chat.get_by_role_and_label(egui::Role::Button, "Delete conversation")
                .click();
            chat.run();
            assert_eq!(chat.state().app.delete_conversation, None);
            assert_ne!(chat.state().app.active_conversation, id);
            assert_eq!(chat.state().app.conversations.len(), 1);
            assert!(
                chat.state()
                    .app
                    .conversations
                    .iter()
                    .all(|item| item.id != id)
            );
            assert_eq!(chat.state().app.input, "");
            assert!(matches!(
                chat.state().commands.try_recv(),
                Ok(Command::NewChat)
            ));
            assert!(chat.state().commands.try_recv().is_err());
        }
    }
}

#[test]
fn conversation_deletion_cancel_and_escape_preserve_the_target_and_unsent_draft() {
    for size in [[1180.0, 860.0], [620.0, 640.0]] {
        let mut chat = harness(size, false);
        let id = chat.state().app.active_conversation;
        chat.state_mut().app.conversations[0].title = "A draft to keep".to_owned();
        chat.state_mut().app.input = "Keep this unsent text.\nAnd this line.".to_owned();
        chat.run();
        for escape in [false, true] {
            open_sidebar(&mut chat, size);
            request_conversation_deletion(&mut chat, "A draft to keep");
            if escape {
                chat.key_press(egui::Key::Escape);
            } else {
                chat.get_by_role_and_label(egui::Role::Button, "Keep conversation")
                    .click();
            }
            chat.run();
            assert_eq!(chat.state().app.delete_conversation, None);
            assert_eq!(chat.state().app.active_conversation, id);
            assert_eq!(chat.state().app.conversations.len(), 1);
            assert_eq!(
                chat.state().app.input,
                "Keep this unsent text.\nAnd this line."
            );
            assert!(chat.state().commands.try_recv().is_err());
        }
    }
}

#[test]
fn conversation_actions_delete_an_inactive_row_without_switching_the_active_draft() {
    for size in [[1180.0, 860.0], [620.0, 640.0]] {
        let mut chat = harness(size, false);
        let inactive = chat.state().app.active_conversation;
        chat.state_mut().app.conversations[0].title = "Saved conversation".to_owned();
        chat.state_mut().app.new_chat();
        assert!(matches!(
            chat.state().commands.try_recv(),
            Ok(Command::NewChat)
        ));
        let active = chat.state().app.active_conversation;
        chat.state_mut().app.conversations[1].title = "Current conversation".to_owned();
        chat.state_mut().app.input = "Continue this draft".to_owned();
        open_sidebar(&mut chat, size);
        chat.run();
        request_conversation_deletion(&mut chat, "Saved conversation");
        assert_eq!(chat.state().app.delete_conversation, Some(inactive));
        chat.get_by_role_and_label(egui::Role::Button, "Delete conversation")
            .click();
        chat.run();
        assert_eq!(chat.state().app.active_conversation, active);
        assert_eq!(chat.state().app.input, "Continue this draft");
        assert_eq!(chat.state().app.conversations.len(), 1);
        assert_eq!(chat.state().app.conversations[0].id, active);
        assert!(chat.state().commands.try_recv().is_err());
    }
}

#[test]
fn streaming_read_aloud_control_is_available_during_generation_and_stops_audio() -> io::Result<()> {
    for size in [[1180.0, 960.0], [620.0, 800.0]] {
        let mut chat = harness(size, true);
        let (session, _commands) = speech::Session::fixture()?;
        chat.state_mut().app.speech = Some(session);
        chat.state_mut().app.speech_output = true;
        "Older answer.".clone_into(&mut chat.state_mut().app.transcript.messages[1].body);
        // AccessKit focus requests use the previous frame's focusable widgets.
        // Publish the newly enabled checkbox before asking it to take focus.
        chat.run();
        chat.get_by_role_and_label(egui::Role::CheckBox, "Read aloud as responses arrive")
            .focus();
        chat.run();
        assert!(
            chat.get_by_role_and_label(egui::Role::CheckBox, "Read aloud as responses arrive")
                .is_focused()
        );
        chat.key_press(egui::Key::Space);
        chat.run();
        assert!(chat.state().app.streaming_read_aloud);
        chat.state_mut().app.input = "Next answer".to_owned();
        chat.state_mut().app.submit();
        chat.state_mut()
            .app
            .model_event(Event::Text("Live answer.".to_owned()));
        chat.run();
        assert!(chat.state().app.model.busy() && chat.state().app.streaming_active);
        chat.get_by_role_and_label(egui::Role::Button, "Stop speech")
            .click();
        chat.run();
        assert!(!chat.state().app.speaking);
        assert!(chat.state().app.model.busy() && chat.state().app.streaming_read_aloud);
        chat.get_all_by_role_and_label(egui::Role::Button, "Read this response aloud")
            .next()
            .ok_or_else(|| io::Error::other("Missing older response speaker"))?
            .click();
        chat.run();
        assert_eq!(
            chat.state().app.speaking_message,
            Some(chat.state().app.transcript.messages[1].id)
        );
        chat.get_by_role_and_label(egui::Role::CheckBox, "Read aloud as responses arrive")
            .click();
        chat.run();
        assert!(!chat.state().app.streaming_read_aloud);
        assert!(chat.state().app.speaking);
    }
    Ok(())
}

#[test]
fn compact_message_headers_keep_short_exchanges_and_actions_visible() -> io::Result<()> {
    for size in [[1180.0, 860.0], [620.0, 640.0]] {
        let mut chat = harness(size, false);
        chat.state_mut()
            .app
            .transcript
            .push(Role::User, "A short prompt.".to_owned());
        chat.state_mut()
            .app
            .transcript
            .push(Role::Assistant, "A short response.".to_owned());
        chat.state_mut().app.transcript.remove_empty_response();
        chat.state_mut().app.transcript.messages[1].interrupted = true;
        chat.state_mut()
            .app
            .transcript
            .push(Role::User, "A follow-up.".to_owned());
        chat.run();
        let prompt = chat.get_by_label("A short prompt.").rect();
        let answer = chat.get_by_label("A short response.").rect();
        let followup = chat.get_by_label("A follow-up.").rect();
        // A short exchange should leave room to read and write at minimum size,
        // without an empty speech footer separating the answer from its followup.
        assert!(followup.bottom() - prompt.top() < 230.0);
        assert!(followup.top() - answer.bottom() < 100.0);
        assert!(
            followup.bottom()
                < chat
                    .get_by_role_and_label(egui::Role::MultilineTextInput, "Message")
                    .rect()
                    .top()
        );
        let speaker = chat
            .get_by_role_and_label(egui::Role::Button, "Read this response aloud")
            .rect();
        let interrupted = chat.get_by_label("Interrupted").rect();
        assert!(interrupted.right() < speaker.left());
        assert!(speaker.bottom() <= answer.top());
        for button in chat.get_all_by_role_and_label(egui::Role::Button, "Copy") {
            let rect = button.rect();
            assert!(rect.height() >= 32.0);
            assert!(rect.left() >= 0.0 && rect.right() <= size[0]);
            assert!(!rect.intersects(speaker));
        }
        let action = chat
            .get_by_role_and_label(egui::Role::Button, "Actions for message 2")
            .rect();
        assert!(action.left() >= speaker.right());
        assert!(action.right() <= size[0]);
        if let Some(value) = std::env::var_os("BONSAI_UI_CAPTURE_DIRECTORY") {
            let directory = capture_directory(value)?;
            fs::create_dir_all(&directory)?;
            let name = if size[0] < 900.0 {
                "messages-narrow.png"
            } else {
                "messages-wide.png"
            };
            chat.render()
                .map_err(io::Error::other)?
                .save(directory.join(name))
                .map_err(io::Error::other)?;
        }
    }
    Ok(())
}

#[test]
fn response_speakers_stay_in_bubbles_and_support_keyboard_play_and_stop() -> io::Result<()> {
    for size in [[1180.0, 960.0], [620.0, 800.0]] {
        let mut chat = harness(size, true);
        chat.state_mut()
            .app
            .transcript
            .append("Private reasoning", true);
        chat.run();
        assert!(
            chat.query_by_role_and_label(egui::Role::Button, "Read aloud")
                .is_none()
        );
        let body = chat.get_by_label(ANSWER).rect();
        let speaker = chat
            .get_by_role_and_label(egui::Role::Button, "Read this response aloud")
            .rect();
        assert!(speaker.bottom() <= body.top());
        assert!(speaker.center().x > body.center().x);
        assert!(
            speaker.bottom()
                < chat
                    .get_by_role_and_label(egui::Role::MultilineTextInput, "Message")
                    .rect()
                    .top()
        );
        chat.get_by_role_and_label(egui::Role::Button, "Show thinking")
            .click();
        chat.run();
        let speaker = chat
            .get_by_role_and_label(egui::Role::Button, "Read this response aloud")
            .rect();
        let reasoning = chat.get_by_label("Private reasoning").rect();
        assert!(
            speaker.bottom() <= body.top() && reasoning.top() > body.bottom(),
            "Speaker {speaker:?} overlaps reasoning {reasoning:?} at {size:?}"
        );
        assert!(
            chat.get_by_role_and_label(egui::Role::Button, "Read this response aloud")
                .accesskit_node()
                .is_disabled()
        );
        assert!(!chat.state().app.dictation_requested);
        let (session, _commands) = speech::Session::fixture()?;
        chat.state_mut().app.speech = Some(session);
        chat.state_mut().app.speech_output = true;
        chat.run();
        chat.get_by_role_and_label(egui::Role::Button, "Read this response aloud")
            .focus();
        chat.run();
        chat.key_press(egui::Key::Space);
        chat.run();
        assert!(chat.state().app.speaking);
        assert_eq!(
            chat.state().app.speaking_message,
            Some(chat.state().app.transcript.messages[1].id)
        );
        chat.get_by_role_and_label(egui::Role::Button, "Stop reading this response")
            .click();
        chat.run();
        assert!(!chat.state().app.speaking);
        assert_eq!(chat.state().app.speaking_message, None);
    }
    Ok(())
}

#[test]
fn speech_controls_follow_speech_readiness_independently_of_chat_readiness() -> io::Result<()> {
    let mut chat = harness([1180.0, 960.0], true);
    let (session, _commands) = speech::Session::fixture()?;
    chat.state_mut().app.speech = Some(session);
    for model in [ModelLifecycle::Preparing, ModelLifecycle::Ready] {
        chat.state_mut().app.model = model;
        for output in [false, true] {
            chat.state_mut()
                .app
                .speech_event(speech::Event::Capabilities {
                    input: false,
                    output,
                });
            chat.run();
            for (role, label) in [
                (egui::Role::Button, "Read this response aloud"),
                (egui::Role::CheckBox, "Read aloud as responses arrive"),
            ] {
                assert_eq!(
                    chat.get_by_role_and_label(role, label)
                        .accesskit_node()
                        .is_disabled(),
                    !output
                );
            }
        }
    }
    Ok(())
}

#[test]
fn response_speakers_play_the_clicked_bubble_in_a_multi_answer_conversation() -> io::Result<()> {
    let mut chat = harness([1180.0, 960.0], false);
    let (session, _commands) = speech::Session::fixture()?;
    chat.state_mut().app.speech = Some(session);
    chat.state_mut().app.speech_output = true;
    chat.state_mut()
        .app
        .transcript
        .push(Role::User, "A question".to_owned());
    chat.state_mut()
        .app
        .transcript
        .push(Role::Assistant, "An older answer".to_owned());
    chat.state_mut().app.transcript.remove_empty_response();
    chat.state_mut()
        .app
        .transcript
        .push(Role::Assistant, "A newer answer".to_owned());
    chat.state_mut().app.transcript.remove_empty_response();
    chat.run();
    assert_eq!(
        chat.get_all_by_role_and_label(egui::Role::Button, "Read this response aloud")
            .count(),
        2
    );
    if let Some(speaker) = chat
        .get_all_by_role_and_label(egui::Role::Button, "Read this response aloud")
        .next()
    {
        speaker.click();
    }
    chat.run();
    assert_eq!(
        chat.state().app.speaking_message,
        Some(chat.state().app.transcript.messages[1].id)
    );
    chat.get_by_role_and_label(egui::Role::Button, "Read this response aloud")
        .click();
    chat.run();
    assert_eq!(
        chat.state().app.speaking_message,
        Some(chat.state().app.transcript.messages[2].id)
    );
    assert_eq!(
        chat.get_all_by_role_and_label(egui::Role::Button, "Stop reading this response")
            .count(),
        1
    );
    Ok(())
}

#[test]
fn thinking_setting_defaults_off_reaches_next_turn_and_locks_while_busy() {
    let mut chat = harness([1180.0, 960.0], false);
    assert!(!chat.state().app.enable_thinking);
    chat.state_mut().app.show_settings = true;
    chat.run();
    chat.get_by_role_and_label(egui::Role::CheckBox, "Thinking")
        .click();
    chat.run();
    assert!(chat.state().app.enable_thinking);
    assert!(chat.state().app.recovery_dirty);
    chat.state_mut().app.show_settings = false;
    chat.state_mut().app.input = "A story about a garden".to_owned();
    chat.state_mut().app.submit();
    assert!(matches!(
        chat.state().commands.try_recv(),
        Ok(Command::Send {
            enable_thinking: true,
            ..
        })
    ));
    chat.state_mut().app.show_settings = true;
    chat.run();
    chat.get_by_role_and_label(egui::Role::CheckBox, "Thinking")
        .click();
    chat.run();
    assert!(chat.state().app.enable_thinking);
    chat.state_mut().app.model = ModelLifecycle::Ready;
    chat.run();
    chat.get_by_role_and_label(egui::Role::CheckBox, "Thinking")
        .click();
    chat.run();
    assert!(!chat.state().app.enable_thinking);
    chat.state_mut().app.show_settings = false;
    chat.state_mut().app.input = "Another garden story".to_owned();
    chat.state_mut().app.submit();
    assert!(matches!(
        chat.state().commands.try_recv(),
        Ok(Command::Send {
            enable_thinking: false,
            ..
        })
    ));
}

#[test]
fn abliteration_settings_keyboard_selection_reaches_next_turn_and_locks_while_busy() {
    let mut chat = harness([1180.0, 960.0], false);
    assert!((chat.state().app.abliteration_scale - 1.3).abs() < f32::EPSILON);
    chat.state_mut().app.show_settings = true;
    chat.run();
    chat.get_by_role_and_label(egui::Role::DisclosureTriangle, "Advanced response controls")
        .scroll_to_me();
    chat.run();
    chat.get_by_role_and_label(egui::Role::DisclosureTriangle, "Advanced response controls")
        .click();
    chat.run();
    chat.get_by_role_and_label(egui::Role::Slider, "Abliteration scale")
        .scroll_to_me();
    chat.run();
    chat.get_by_role_and_label(egui::Role::Slider, "Abliteration scale")
        .focus();
    chat.run();
    chat.key_press(egui::Key::ArrowRight);
    chat.run();
    let selected = chat.state().app.abliteration_scale;
    assert!((selected - 1.4).abs() < 0.001);
    chat.state_mut().app.show_settings = false;
    chat.state_mut().app.input = "A story about a garden".to_owned();
    chat.state_mut().app.submit();
    assert!(matches!(
        chat.state().commands.try_recv(),
        Ok(Command::Send { abliteration_scale, .. })
            if (abliteration_scale - selected).abs() < f32::EPSILON
    ));
    chat.state_mut().app.show_settings = true;
    chat.run();
    chat.key_press(egui::Key::ArrowRight);
    chat.run();
    assert!((chat.state().app.abliteration_scale - selected).abs() < f32::EPSILON);
}

#[test]
fn abliteration_startup_opt_out_disables_settings_control() {
    let mut chat = harness([1180.0, 960.0], false);
    chat.state_mut().app.abliteration_enabled = false;
    chat.state_mut().app.show_settings = true;
    chat.run();
    chat.get_by_role_and_label(egui::Role::DisclosureTriangle, "Advanced response controls")
        .scroll_to_me();
    chat.run();
    chat.get_by_role_and_label(egui::Role::DisclosureTriangle, "Advanced response controls")
        .click();
    chat.run();
    chat.get_by_role_and_label(egui::Role::Slider, "Abliteration scale")
        .scroll_to_me();
    chat.run();
    assert!(
        chat.get_by_label("Disabled by the selected launch options.")
            .rect()
            .is_positive()
    );
    let initial = chat.state().app.abliteration_scale;
    chat.get_by_role_and_label(egui::Role::Slider, "Abliteration scale")
        .focus();
    chat.run();
    chat.key_press(egui::Key::ArrowRight);
    chat.run();
    assert!((chat.state().app.abliteration_scale - initial).abs() < f32::EPSILON);
}

#[test]
fn accessible_chat_controls_handle_enter_and_shift_enter() {
    let mut menu = harness([760.0, 800.0], false);
    menu.get_by_role_and_label(egui::Role::Button, "Conversation menu")
        .click();
    menu.run();
    assert!(menu.state().app.sidebar_open);
    assert!(
        menu.get_by_role_and_label(egui::Role::Button, "+ New conversation")
            .rect()
            .is_positive()
    );
    drop(menu);

    let mut chat = harness([1180.0, 860.0], false);
    chat.get_by_role_and_label(egui::Role::MultilineTextInput, "Message")
        .focus();
    chat.run();
    chat.get_by_role_and_label(egui::Role::MultilineTextInput, "Message")
        .type_text("First line");
    chat.run();
    assert!(
        chat.get_by_role_and_label(egui::Role::Button, "Send  ↑")
            .rect()
            .is_positive()
    );

    chat.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::Enter);
    chat.run();
    assert_eq!(chat.state().app.input, "First line\n");
    assert!(chat.state().app.transcript.messages.is_empty());
    assert!(matches!(
        chat.state().commands.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));

    chat.get_by_role_and_label(egui::Role::MultilineTextInput, "Message")
        .type_text("Second line");
    chat.run();
    chat.key_press(egui::Key::Enter);
    chat.run();
    assert!(
        matches!(chat.state().commands.try_recv(), Ok(Command::Send { text, .. })
        if text == "First line\nSecond line")
    );
    assert!(chat.state().app.model.busy());
    assert_eq!(chat.state().app.transcript.messages.len(), 2);
    assert_eq!(chat.state().app.input, "");
    assert!(
        chat.get_by_role_and_label(egui::Role::Button, "Stop")
            .rect()
            .is_positive()
    );
    assert!(chat.state().app.worker.is_none());
    assert!(chat.state().app.speech.is_none());
    assert!(chat.state().app.microphone.is_none());
}

#[test]
fn background_generation_keeps_composer_and_conversation_navigation_available() {
    for size in [[1180.0, 860.0], [620.0, 760.0]] {
        let mut chat = harness(size, false);
        chat.state_mut().app.input = "Original question".into();
        chat.state_mut().app.submit();
        let owner = chat.state().app.active_conversation;
        assert!(matches!(
            chat.state().commands.try_recv(),
            Ok(Command::Send { .. })
        ));
        chat.run();
        chat.get_by_role_and_label(egui::Role::MultilineTextInput, "Message")
            .focus();
        chat.run();
        chat.get_by_role_and_label(egui::Role::MultilineTextInput, "Message")
            .type_text("Owner's next draft");
        chat.run();
        chat.key_press(egui::Key::Enter);
        chat.run();
        assert_eq!(chat.state().app.input, "Owner's next draft");
        assert!(chat.state().commands.try_recv().is_err());
        open_sidebar(&mut chat, size);
        chat.get_by_role_and_label(egui::Role::Button, "+ New conversation")
            .click();
        chat.run();
        let selected = chat.state().app.active_conversation;
        assert_ne!(selected, owner);
        chat.state_mut().app.sidebar_open = false;
        chat.run();
        chat.get_by_role_and_label(egui::Role::MultilineTextInput, "Message")
            .type_text("Draft in another chat");
        chat.run();
        chat.key_press(egui::Key::Enter);
        chat.run();
        chat.state_mut()
            .app
            .model_event(Event::Text("Owner answer".into()));
        chat.run();
        assert_eq!(chat.state().app.input, "Draft in another chat");
        assert!(chat.state().app.transcript.messages.is_empty());
        assert!(chat.state().commands.try_recv().is_err());
        chat.get_by_role_and_label(egui::Role::Button, "Show response")
            .click();
        chat.run();
        assert_eq!(chat.state().app.active_conversation, owner);
        assert_eq!(chat.state().app.input, "Owner's next draft");
        assert!(
            chat.state()
                .app
                .transcript
                .plain_text()
                .contains("Owner answer")
        );
        chat.get_by_role_and_label(egui::Role::Button, "Stop")
            .click();
        chat.run();
        assert!(matches!(
            chat.state().commands.try_recv(),
            Ok(Command::Stop)
        ));
    }
}

#[test]
fn conversation_controls_work_while_loading_without_sending() {
    let mut chat = harness([1180.0, 860.0], true);
    chat.state_mut().app.model = ModelLifecycle::Preparing;
    chat.state_mut().app.conversations[0].title = "Original conversation".into();
    chat.state_mut().app.input = "Original draft".into();
    let original = chat.state().app.active_conversation;
    chat.run();
    chat.get_by_role_and_label(egui::Role::Button, "+ New conversation")
        .click();
    chat.run();
    let second = chat.state().app.active_conversation;
    assert_ne!(second, original);
    chat.get_by_role_and_label(egui::Role::MultilineTextInput, "Message")
        .type_text("Draft while loading");
    chat.run();
    chat.key_press(egui::Key::Enter);
    chat.run();
    assert_eq!(chat.state().app.input, "Draft while loading");
    chat.get_by_role_and_label(egui::Role::Button, "Send  ↑")
        .click();
    chat.run();
    assert!(chat.state().commands.try_recv().is_err());
    chat.get_by_role_and_label(egui::Role::Button, "Original conversation")
        .click();
    chat.run();
    assert_eq!(chat.state().app.active_conversation, original);
    assert_eq!(chat.state().app.input, "Original draft");
    assert_eq!(chat.state().app.transcript.messages.len(), 2);
    request_conversation_deletion(&mut chat, "Original conversation");
    chat.get_by_role_and_label(egui::Role::Button, "Delete conversation")
        .click();
    chat.run();
    assert_eq!(chat.state().app.active_conversation, second);
    assert_eq!(chat.state().app.input, "Draft while loading");
    assert_eq!(chat.state().app.conversations.len(), 1);
    assert!(chat.state().commands.try_recv().is_err());
    assert_eq!(chat.state().app.model, ModelLifecycle::Preparing);
}

#[test]
fn composer_keeps_a_draft_while_the_model_prepares() {
    let mut chat = harness([620.0, 760.0], false);
    chat.state_mut().app.model = ModelLifecycle::Preparing;
    chat.run();
    chat.get_by_role_and_label(egui::Role::MultilineTextInput, "Message")
        .focus();
    chat.run();
    chat.get_by_role_and_label(egui::Role::MultilineTextInput, "Message")
        .type_text("Keep this draft while loading 🌱");
    chat.run();
    chat.key_press(egui::Key::Enter);
    chat.run();
    assert_eq!(chat.state().app.input, "Keep this draft while loading 🌱");
    assert!(chat.state().app.transcript.messages.is_empty());
    assert!(matches!(
        chat.state().commands.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
}

#[test]
fn maximum_drafts_keep_transcript_and_actions_visible_and_editable() {
    for size in [[1180.0, 860.0], [620.0, 640.0]] {
        for line in [
            "A manuscript line with a thought.\n",
            "A wrapped manuscript passage. ",
        ] {
            let mut chat = harness(size, true);
            let draft: String = line.chars().cycle().take(INPUT_LIMIT - 2).collect();
            chat.state_mut().app.input.clone_from(&draft);
            chat.run();
            for label in ["Microphone", "Read this response aloud", "Send  ↑"] {
                let action = chat.get_by_role_and_label(egui::Role::Button, label).rect();
                assert!(action.top() >= 0.0 && action.bottom() <= size[1]);
                assert!(action.left() >= 0.0 && action.right() <= size[0]);
            }
            let transcript = chat.get_by_label(ANSWER).rect();
            let send = chat
                .get_by_role_and_label(egui::Role::Button, "Send  ↑")
                .rect();
            assert!(transcript.top() >= 0.0 && transcript.bottom() < send.top());
            chat.get_by_role_and_label(egui::Role::MultilineTextInput, "Message")
                .focus();
            chat.run();
            chat.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::End);
            chat.run();
            let geometry = chat.ctx.data(|data| {
                data.get_temp::<(Option<egui::Rect>, egui::Rect, egui::Vec2)>(egui::Id::unique(
                    "composer-viewport",
                ))
            });
            assert!(
                geometry.is_some_and(|(caret, viewport, offset)| {
                    offset.y > 0.0
                        && viewport.height() <= 160.0
                        && caret.is_some_and(|caret| viewport.expand(1.0).contains_rect(caret))
                }),
                "Composer caret must scroll into view: {geometry:?}"
            );
            chat.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::Enter);
            chat.run();
            assert_eq!(chat.state().app.input, format!("{draft}\n"));
            assert!(matches!(
                chat.state().commands.try_recv(),
                Err(mpsc::TryRecvError::Empty)
            ));
            chat.key_press(egui::Key::Enter);
            chat.run();
            assert!(
                matches!(chat.state().commands.try_recv(), Ok(Command::Send { text, .. }) if text == draft.trim())
            );
            assert!(chat.state().app.model.busy());
            let stop = chat
                .get_by_role_and_label(egui::Role::Button, "Stop")
                .rect();
            assert!(stop.top() >= 0.0 && stop.bottom() <= size[1]);
        }
    }
}

#[test]
fn transcript_export_preserves_roles_spacing_and_visible_unicode_only() {
    let mut transcript = Transcript::default();
    assert_eq!(transcript.plain_text(), "");
    transcript.push(Role::User, "First line\n\n界".to_owned());
    transcript.push(Role::Assistant, String::new());
    transcript.append("Private thinking", true);
    assert_eq!(transcript.plain_text(), "You\nFirst line\n\n界");
    transcript.stage();
    transcript.append("Answer 🌱\n", false);
    transcript.append("More private thinking", true);
    transcript.push(Role::User, String::new());
    assert_eq!(
        transcript.plain_text(),
        "You\nFirst line\n\n界\n\nSwift Bonsai\nAnswer 🌱\n"
    );
    transcript.clear();
    assert_eq!(transcript.plain_text(), "");
}

#[test]
fn thinking_toggle_reveals_current_streamed_reasoning_after_each_expansion() {
    let mut chat = harness([1180.0, 860.0], true);
    chat.state_mut().app.model = ModelLifecycle::Generating;
    chat.state_mut()
        .app
        .model_event(Event::Reasoning("First thought.".to_owned()));
    chat.run();
    assert!(chat.query_by_label("First thought.").is_none());
    chat.get_by_role_and_label(egui::Role::Button, "Show thinking")
        .click();
    chat.run();
    assert!(chat.get_by_label("First thought.").rect().is_positive());

    chat.state_mut()
        .app
        .model_event(Event::Reasoning(" A second thought.".to_owned()));
    chat.run();
    assert!(
        chat.get_by_label("First thought. A second thought.")
            .rect()
            .is_positive()
    );
    chat.get_by_role_and_label(egui::Role::Button, "Hide thinking")
        .click();
    chat.run();
    assert!(
        chat.query_by_label("First thought. A second thought.")
            .is_none()
    );

    chat.state_mut()
        .app
        .model_event(Event::Reasoning(" Latest thought.".to_owned()));
    chat.run();
    assert!(
        chat.query_by_label("First thought. A second thought. Latest thought.")
            .is_none()
    );
    chat.get_by_role_and_label(egui::Role::Button, "Show thinking")
        .click();
    chat.run();
    assert!(
        chat.get_by_label("First thought. A second thought. Latest thought.")
            .rect()
            .is_positive()
    );
    assert!(chat.get_by_label(ANSWER).rect().is_positive());
    assert!(!chat.state().app.transcript.plain_text().contains("thought"));
    assert!(chat.state().app.worker.is_none());
    assert!(chat.state().app.speech.is_none());
    assert!(chat.state().app.microphone.is_none());
}

#[test]
fn streaming_append_preserves_a_real_transcript_text_selection() {
    let mut chat = harness([1180.0, 860.0], true);
    chat.state_mut().app.model = ModelLifecycle::Generating;
    chat.run();
    let answer = chat.get_by_label(ANSWER).rect();
    let start = answer.left_top() + egui::vec2(3.0, 8.0);
    let end = answer.left_top() + egui::vec2(130.0, 8.0);
    chat.hover_at(start);
    chat.drag_at(start);
    chat.run_steps(1);
    chat.hover_at(end);
    chat.run_steps(1);
    chat.drop_at(end);
    chat.run();
    assert!(
        chat.ctx
            .plugin::<egui::text_selection::LabelSelectionState>()
            .lock()
            .has_selection()
    );

    chat.state_mut()
        .app
        .model_event(Event::Text("\n\nWe can build from there.".to_owned()));
    chat.run();
    assert!(
        chat.ctx
            .plugin::<egui::text_selection::LabelSelectionState>()
            .lock()
            .has_selection()
    );
    assert!(
        chat.state()
            .app
            .transcript
            .plain_text()
            .ends_with("We can build from there.")
    );
    assert!(chat.state().app.worker.is_none());
    assert!(chat.state().app.speech.is_none());
    assert!(chat.state().app.microphone.is_none());
}

fn capture_directory(value: std::ffi::OsString) -> io::Result<PathBuf> {
    let directory = PathBuf::from(value);
    // This existing boundary rejects relative/unexpanded paths, reparse points,
    // and every Git ancestor before any directory or image can be written.
    bonsai_windows_credentials::validate_external_path(&directory)?;
    Ok(directory)
}

#[test]
fn capture_destination_rejects_relative_and_unexpanded_paths() {
    assert!(capture_directory("captures".into()).is_err());
    assert!(capture_directory("%TEMP%\\Bonsai-ui".into()).is_err());
    assert!(capture_directory(env!("CARGO_MANIFEST_DIR").into()).is_err());
}

#[test]
fn capture_chat_layouts_when_requested() -> io::Result<()> {
    let Some(value) = std::env::var_os("BONSAI_UI_CAPTURE_DIRECTORY") else {
        // Ordinary local/hosted regressions do not initialize a GPU.
        return Ok(());
    };
    let directory = capture_directory(value)?;
    fs::create_dir_all(&directory)?;
    for (name, size, seeded) in [
        ("conversation-wide.png", [1180.0, 860.0], true),
        ("conversation-narrow.png", [620.0, 760.0], true),
        ("navigation-wide.png", [1180.0, 860.0], true),
        ("navigation-narrow.png", [620.0, 640.0], true),
        ("empty-state.png", [1180.0, 860.0], false),
        ("settings-narrow.png", [620.0, 640.0], true),
        ("draft-wide.png", [1180.0, 860.0], true),
        ("draft-narrow.png", [620.0, 640.0], true),
        ("sidebar-narrow.png", [620.0, 640.0], true),
        ("sidebar-many.png", [1180.0, 860.0], true),
        ("delete-conversation.png", [620.0, 640.0], true),
        ("rename-conversation.png", [620.0, 640.0], true),
    ] {
        let mut chat = harness(size, seeded);
        if name == "settings-narrow.png" {
            chat.state_mut().app.show_settings = true;
        } else if name == "sidebar-many.png" {
            for index in 1..128 {
                chat.state_mut().app.new_chat();
                chat.state_mut().app.conversations[index].title =
                    format!("Conversation {index}: a longer title to browse");
            }
        } else if matches!(
            name,
            "sidebar-narrow.png" | "delete-conversation.png" | "rename-conversation.png"
        ) {
            chat.state_mut().app.sidebar_open = true;
            chat.state_mut().app.conversations[0].title =
                "A quiet bookshop and a mysterious letter".to_owned();
            let id = chat.state().app.active_conversation;
            if name == "delete-conversation.png" {
                chat.state_mut().app.delete_conversation = Some(id);
            } else if name == "rename-conversation.png" {
                chat.state_mut().app.rename_conversation =
                    Some((id, "A quiet bookshop and a mysterious letter".to_owned()));
            }
        } else if name.starts_with("draft-") {
            chat.state_mut().app.input = "A manuscript line with a thought.\n"
                .chars()
                .cycle()
                .take(INPUT_LIMIT - "\nEnd of draft.".len())
                .collect();
            chat.state_mut().app.input.push_str("\nEnd of draft.");
        }
        chat.run();
        if name.starts_with("navigation-") {
            for index in 0..20 {
                chat.state_mut().app.transcript.push(
                    Role::Assistant,
                    format!("A detail to revisit {index}.\n\n{ANSWER}"),
                );
            }
            chat.run();
            chat.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::F);
            chat.run();
            chat.get_by_role_and_label(egui::Role::TextInput, "Find in conversation")
                .type_text("quiet bookshop");
            chat.run();
        }
        if name.starts_with("draft-") {
            chat.get_by_role_and_label(egui::Role::MultilineTextInput, "Message")
                .focus();
            chat.run();
            chat.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::End);
            chat.run();
        }
        chat.render()
            .map_err(io::Error::other)?
            .save(directory.join(name))
            .map_err(io::Error::other)?;
        assert!(chat.state().app.worker.is_none());
        assert!(chat.state().app.speech.is_none());
        assert!(chat.state().app.microphone.is_none());
    }
    Ok(())
}

#[test]
fn find_controls_navigate_unicode_matches_and_refresh_streamed_text() {
    let mut chat = Harness::builder()
        .with_size([900.0, 760.0])
        .with_context_setup(|context| {
            configure(context);
            context.add_font_provider(std::sync::Arc::new(
                egui_system_font_provider::SystemFontProvider::new(),
            ));
        })
        .build_ui_state(
            |ui, transcript: &mut Transcript| {
                transcript.close_find_on_escape(ui.ctx());
                transcript.search_bar(ui);
                egui::ScrollArea::vertical().show(ui, |ui| transcript.show(ui, 700.0, None, true));
            },
            {
                let mut transcript = Transcript::default();
                transcript.push(Role::User, "Élan and an idea".to_owned());
                transcript.push(Role::Assistant, "An éLAN, then another Élan.".to_owned());
                transcript.append("Reasoning with Élan stays private.", true);
                transcript
            },
        );
    chat.state_mut().focus_find();
    chat.run();
    chat.get_by_role_and_label(egui::Role::TextInput, "Find in conversation")
        .type_text("ÉLAN");
    chat.run();
    assert!(chat.get_by_label("1 of 3").rect().is_positive());
    chat.get_by_role_and_label(egui::Role::Button, "Next match")
        .click();
    chat.run();
    assert!(chat.get_by_label("2 of 3").rect().is_positive());
    chat.get_by_role_and_label(egui::Role::Button, "Previous match")
        .click();
    chat.run();
    assert!(chat.get_by_label("1 of 3").rect().is_positive());
    chat.state_mut().append(" Élan", false);
    chat.run();
    assert!(chat.get_by_label("1 of 4").rect().is_positive());
    assert!(!chat.state().plain_text().contains("Reasoning"));
    chat.key_press(egui::Key::Escape);
    chat.run();
    assert!(chat.query_by_label("Find in conversation").is_none());
}

#[test]
fn long_conversation_find_scrolls_to_offscreen_matches_and_keeps_latest_reachable() {
    const FIRST: &str = "Needle at the beginning of the conversation.";
    const LAST: &str = "Needle at the end of the conversation.";
    for size in [[1180.0, 860.0], [620.0, 640.0]] {
        let mut chat = harness(size, false);
        chat.state_mut()
            .app
            .transcript
            .push(Role::User, FIRST.to_owned());
        for index in 0..30 {
            chat.state_mut().app.transcript.push(
                Role::Assistant,
                format!("Middle passage {index}.\nA detail about a character.\nAnother detail about the setting.\nA final detail about the scene."),
            );
        }
        chat.state_mut()
            .app
            .transcript
            .push(Role::Assistant, LAST.to_owned());
        chat.state_mut().app.transcript.remove_empty_response();
        chat.run();
        assert!(chat.query_by_label(FIRST).is_none());
        assert!(chat.get_by_label(LAST).rect().is_positive());
        assert!(
            chat.query_by_role_and_label(egui::Role::Button, "Latest")
                .is_none()
        );
        chat.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::F);
        chat.run();
        chat.get_by_role_and_label(egui::Role::TextInput, "Find in conversation")
            .type_text("Needle");
        chat.run();
        let controls = chat
            .get_by_role_and_label(egui::Role::Button, "Next match")
            .rect();
        let first = chat.get_by_label(FIRST).rect();
        let composer = chat
            .get_by_role_and_label(egui::Role::MultilineTextInput, "Message")
            .rect();
        assert!(first.top() >= controls.bottom());
        assert!(first.bottom() < composer.top());
        assert!(chat.query_by_label(LAST).is_none());
        assert!(chat.get_by_label("1 of 2").rect().is_positive());

        chat.get_by_role_and_label(egui::Role::Button, "Next match")
            .click();
        chat.run();
        let last = chat.get_by_label(LAST).rect();
        assert!(last.top() >= controls.bottom());
        assert!(last.bottom() < composer.top());
        assert!(chat.query_by_label(FIRST).is_none());
        assert!(chat.get_by_label("2 of 2").rect().is_positive());
        let next = chat
            .get_by_role_and_label(egui::Role::Button, "Next match")
            .rect();
        assert!((next.top() - controls.top()).abs() < 0.5);

        chat.get_by_role_and_label(egui::Role::Button, "Previous match")
            .click();
        chat.run();
        assert!(chat.get_by_label(FIRST).rect().is_positive());
        assert!(chat.query_by_label(LAST).is_none());
        let latest = chat
            .get_by_role_and_label(egui::Role::Button, "Latest")
            .rect();
        let toolbar = chat
            .get_by_role_and_label(egui::Role::Button, "Context")
            .rect();
        assert!((latest.width() - latest.height()).abs() < 0.5);
        assert!((latest.center().x - composer.center().x).abs() < 12.0);
        // Anchor the jump button to the top of the composer, including its
        // toolbar; the editor can start lower when narrow controls wrap.
        assert!(latest.top() > toolbar.top() - 100.0);
        assert!(latest.bottom() < toolbar.top());
        chat.get_by_role_and_label(egui::Role::Button, "Latest")
            .click();
        // Let scrolling and the disappearing button's hover animation settle.
        chat.run_steps(8);
        chat.run();
        assert!(chat.get_by_label(LAST).rect().is_positive());
        assert!(chat.query_by_label(FIRST).is_none());
        assert!(
            chat.query_by_role_and_label(egui::Role::Button, "Latest")
                .is_none()
        );
        chat.state_mut().app.transcript.append(
            "\nNew details arriving in the latest response.\nA second detail.\nA third detail.",
            false,
        );
        chat.run();
        assert!(
            chat.query_by_role_and_label(egui::Role::Button, "Latest")
                .is_none()
        );
        assert!(chat.query_by_label(FIRST).is_none());
        let next = chat
            .get_by_role_and_label(egui::Role::Button, "Next match")
            .rect();
        assert!((next.top() - controls.top()).abs() < 0.5);
    }
}

#[test]
fn real_text_selection_does_not_leak_when_conversation_message_ids_repeat() {
    let mut chat = harness([1180.0, 860.0], true);
    chat.run();
    let original = chat.state().app.active_conversation;
    let answer = chat.get_by_label(ANSWER).rect();
    let start = answer.left_top() + egui::vec2(3.0, 8.0);
    let end = answer.left_top() + egui::vec2(130.0, 8.0);
    chat.hover_at(start);
    chat.drag_at(start);
    chat.run_steps(1);
    chat.hover_at(end);
    chat.run_steps(1);
    chat.drop_at(end);
    chat.run();
    assert!(
        chat.ctx
            .plugin::<egui::text_selection::LabelSelectionState>()
            .lock()
            .has_selection()
    );

    // Switch without a pointer click, which would independently clear selection.
    // Both transcripts assign their first two messages ids 1 and 2.
    chat.state_mut().app.new_chat();
    chat.state_mut()
        .app
        .transcript
        .push(Role::User, "A different question.".to_owned());
    chat.state_mut().app.transcript.push(
        Role::Assistant,
        "An unrelated answer that must not inherit another conversation's text selection."
            .to_owned(),
    );
    chat.state_mut().app.transcript.remove_empty_response();
    chat.run();
    assert!(
        !chat
            .ctx
            .plugin::<egui::text_selection::LabelSelectionState>()
            .lock()
            .has_selection()
    );
    chat.state_mut().app.switch_conversation(original);
    chat.run();
    assert!(chat.get_by_label(ANSWER).rect().is_positive());
    assert!(
        !chat
            .ctx
            .plugin::<egui::text_selection::LabelSelectionState>()
            .lock()
            .has_selection()
    );
}

#[test]
fn audio_file_plus_is_accessible_and_remains_inside_the_composer() {
    for size in [[1180.0, 860.0], [620.0, 640.0]] {
        let mut chat = harness(size, false);
        let add = chat.get_by_role_and_label(egui::Role::Button, "Add audio file");
        let microphone = chat.get_by_role_and_label(egui::Role::Button, "Microphone");
        assert!(add.rect().is_positive());
        assert!(add.rect().left() >= 0.0 && add.rect().bottom() <= size[1]);
        assert!(add.rect().center().x < microphone.rect().center().x);
        add.click();
        chat.run();
        assert!(chat.get_by_label("Audio file setup").rect().is_positive());
        assert!(chat.state().app.microphone.is_none());
        assert!(chat.state().commands.try_recv().is_err());
    }
}

#[test]
fn audio_file_progress_cancel_and_rejection_are_visible_in_wide_and_narrow_windows() {
    for size in [[1180.0, 860.0], [620.0, 640.0]] {
        let mut chat = harness(size, false);
        let app = &mut chat.state_mut().app;
        app.audio_file_notice = Some("Transcribing audio: 30–60 of 3600 seconds.".to_owned());
        app.audio_file_active = true;
        app.dictation_requested = true;
        app.dictation_finishing = true;
        app.microphone_generation = Some(7);
        app.input = "Keep draft".to_owned();
        // The active transcription spinner deliberately keeps requesting frames.
        chat.run_steps(2);
        let status = chat.get_by_label("Audio transcription in progress").rect();
        assert!(status.is_positive() && status.bottom() <= size[1]);
        chat.get_by_role_and_label(egui::Role::Button, "Cancel transcription")
            .click();
        chat.run();
        assert!(!chat.state().app.audio_file_active);
        assert_eq!(chat.state().app.input, "Keep draft");
        assert!(chat.state().commands.try_recv().is_err());
        let app = &mut chat.state_mut().app;
        app.audio_file_notice = Some("Not enough available memory to decode this audio. Close other apps and try again. Your draft was kept.".to_owned());
        app.audio_file_error = true;
        app.speech_event(speech::Event::Listening(false));
        chat.run();
        let error = chat.get_by_label("Audio transcription failed").rect();
        assert!(error.is_positive() && error.bottom() <= size[1]);
        assert!(chat.get_by_label("Not enough available memory to decode this audio. Close other apps and try again. Your draft was kept.").rect().is_positive());
        chat.get_by_role_and_label(egui::Role::Button, "Dismiss audio status")
            .click();
        chat.run();
        assert!(chat.state().app.audio_file_notice.is_none());
    }
}

#[test]
fn audio_transcript_modal_stays_bounded_as_a_long_recording_arrives() -> io::Result<()> {
    for size in [[480.0, 480.0], [620.0, 640.0], [1180.0, 860.0]] {
        let mut chat = harness(size, false);
        let app = &mut chat.state_mut().app;
        app.audio_file_notice = Some("Recognizing speech in this segment.".into());
        app.audio_file_active = true;
        app.dictation_requested = true;
        app.dictation_finishing = true;
        app.microphone_generation = Some(7);
        app.show_audio_transcript = true;
        app.input = "Keep draft".into();
        let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, size.into());
        let mut initial = None;
        // Include long unbroken text as well as paragraphs: neither may expand
        // the viewport or move the action row as recognition appends more text.
        for part in [
            "First segment.\n".to_owned(),
            "word ".repeat(3000),
            "界".repeat(16000),
        ] {
            chat.state_mut().app.audio_file_part(7, &part);
            chat.run_steps(2);
            let (modal, viewport) = chat.ctx.data(|data| {
                (
                    data.get_temp::<egui::Rect>(egui::Id::unique("audio-transcript-bounds")),
                    data.get_temp::<egui::Rect>(egui::Id::unique("audio-transcript-viewport")),
                )
            });
            let modal = modal.ok_or_else(|| io::Error::other("Missing modal bounds"))?;
            let viewport =
                viewport.ok_or_else(|| io::Error::other("Missing transcript viewport"))?;
            assert!(bounds.contains_rect(modal), "{size:?}: {modal:?}");
            assert!(modal.contains_rect(viewport));
            let close = chat
                .get_by_role_and_label(egui::Role::Button, "Close transcript")
                .rect();
            let copy = chat
                .get_by_role_and_label(egui::Role::Button, "Copy full transcript")
                .rect();
            let cancel = chat
                .get_by_role_and_label(egui::Role::Button, "Cancel transcription in viewer")
                .rect();
            assert!(bounds.contains_rect(close) && bounds.contains_rect(copy));
            assert!(bounds.contains_rect(cancel));
            assert!(close.top() >= viewport.bottom());
            if let Some(first) = initial {
                assert_eq!(modal, first, "transcript growth changed the modal bounds");
            } else {
                initial = Some(modal);
            }
        }
        let retained = chat.state().app.audio_file_transcript.clone();
        chat.get_by_role_and_label(egui::Role::Button, "Cancel transcription in viewer")
            .click();
        chat.run();
        assert!(!chat.state().app.audio_file_active);
        assert!(chat.state().app.show_audio_transcript);
        assert_eq!(chat.state().app.audio_file_transcript, retained);
        assert_eq!(chat.state().app.input, "Keep draft");
        assert!(chat.state().commands.try_recv().is_err());
    }
    Ok(())
}

#[test]
fn audio_transcript_modal_owns_shortcuts_and_completion_requires_review() {
    for size in [[480.0, 480.0], [1180.0, 860.0]] {
        let mut chat = harness(size, false);
        chat.get_by_role_and_label(egui::Role::MultilineTextInput, "Message")
            .focus();
        chat.run();
        let app = &mut chat.state_mut().app;
        app.input = "Keep draft".into();
        app.audio_file_active = true;
        app.dictation_requested = true;
        app.dictation_finishing = true;
        app.microphone_generation = Some(7);
        app.audio_file_transcript = "Recognized speech".into();
        app.show_audio_transcript = true;
        let original = app.active_conversation;
        chat.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::N);
        chat.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::F);
        chat.key_press(egui::Key::Enter);
        chat.run_steps(2);
        assert_eq!(chat.state().app.active_conversation, original);
        assert_eq!(chat.state().app.conversations.len(), 1);
        assert_eq!(chat.state().app.input, "Keep draft");
        assert!(chat.query_by_label("Find in conversation").is_none());
        assert!(chat.state().commands.try_recv().is_err());
        chat.state_mut()
            .app
            .speech_event(speech::Event::FileFinished { generation: 7 });
        // A completion racing the Close/Escape action must also stay a draft.
        chat.key_press(egui::Key::Escape);
        chat.run_steps(2);
        assert!(!chat.state().app.show_audio_transcript);
        let ctx = chat.ctx.clone();
        chat.state_mut().app.flush_voice_transcript(&ctx);
        assert!(!chat.state().app.audio_file_active);
        assert!(chat.state().app.input.contains("Keep draft"));
        assert!(chat.state().app.input.contains("Recognized speech"));
        assert!(chat.state().commands.try_recv().is_err());
        assert!(chat.state().app.speech_status.contains("Review the draft"));
    }
}

#[test]
fn audio_file_progress_is_generation_bound_and_metadata_never_finishes_import() {
    let mut chat = harness([620.0, 640.0], false);
    let app = &mut chat.state_mut().app;
    app.audio_file_active = true;
    app.dictation_requested = true;
    app.dictation_finishing = true;
    app.microphone_generation = Some(7);
    let progress = speech::FileProgress {
        start: Duration::from_secs(30),
        end: Duration::from_secs(60),
        total: Duration::from_secs(45),
    };
    app.speech_event(speech::Event::FileProgress {
        generation: 7,
        progress: Some(progress),
    });
    assert_eq!(app.audio_file_progress, Some(progress));
    app.speech_event(speech::Event::FileProgress {
        generation: 7,
        progress: None,
    });
    assert_eq!(app.audio_file_progress, Some(progress));
    assert!(app.audio_file_active);
    app.cancel_audio_file();
    let cancelled = app.audio_file_notice.clone();
    app.speech_event(speech::Event::FileProgress {
        generation: 7,
        progress: Some(progress),
    });
    app.speech_event(speech::Event::FileFinished { generation: 7 });
    assert_eq!(app.audio_file_notice, cancelled);
    assert!(app.pending_voice_transcript.is_none());
    assert!(chat.state().commands.try_recv().is_err());
}
