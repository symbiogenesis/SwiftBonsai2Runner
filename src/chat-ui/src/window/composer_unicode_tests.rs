//! Exact logical text and cursor round trips through the native composer widget.
//! These controls do not qualify bidirectional visual layout or a system IME.
use super::*;
use egui_kittest::{Harness, kittest::Queryable as _};

struct Fixture {
    app: State,
    commands: Receiver<Command>,
    _events: client::EventSender,
}

fn harness(draft: &str) -> Harness<'static, Fixture> {
    let (commands, incoming) = mpsc::channel();
    let (events, incoming_events) = client::event_channel();
    let mut app = State::create(commands, incoming_events, None, None);
    app.preview = false;
    app.model = ModelLifecycle::Ready;
    draft.clone_into(&mut app.input);
    let mut chat = Harness::builder()
        .with_size([1180.0, 860.0])
        .with_context_setup(configure_test_context)
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
        );
    chat.get_by_role_and_label(egui::Role::MultilineTextInput, "Message")
        .focus();
    chat.run();
    chat
}

fn cursor(chat: &Harness<'_, Fixture>) -> Option<egui::text::CCursorRange> {
    chat.state()
        .app
        .composer_id
        .and_then(|id| egui::TextEdit::load_state(&chat.ctx, id))
        .and_then(|state| state.cursor.char_range())
}

#[test]
fn composer_unicode_partial_replacement_preserves_logical_text_and_undo_redo() -> io::Result<()> {
    // Keep the decomposed accent as two scalars. The Hebrew selection follows
    // logical character order, independently of its rendered reading direction.
    let original = "左e\u{301}ימין🌱右";
    let replacement = "左e\u{301}日本語🌱右";
    let mut chat = harness(original);
    let id = chat
        .state()
        .app
        .composer_id
        .ok_or_else(|| io::Error::other("Composer editor missing"))?;
    let mut editor = egui::TextEdit::load_state(&chat.ctx, id)
        .ok_or_else(|| io::Error::other("Composer state missing"))?;
    let selection =
        egui::text::CCursorRange::two(egui::text::CCursor::new(3), egui::text::CCursor::new(7));
    editor.cursor.set_char_range(Some(selection));
    let mut undoer = editor.undoer();
    undoer.add_undo(&(selection, original.to_owned()));
    editor.set_undoer(undoer);
    editor.store(&chat.ctx, id);

    chat.event(egui::Event::Paste("日本語".to_owned()));
    chat.run();
    let inserted = egui::text::CCursorRange::one(egui::text::CCursor::new(6));
    assert_eq!(chat.state().app.input, replacement);
    assert_eq!(cursor(&chat), Some(inserted));
    assert!(chat.state().commands.try_recv().is_err());

    chat.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    chat.run();
    assert_eq!(chat.state().app.input, original);
    assert_eq!(cursor(&chat), Some(selection));
    chat.key_press_modifiers(
        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
        egui::Key::Z,
    );
    chat.run();
    assert_eq!(chat.state().app.input, replacement);
    assert_eq!(cursor(&chat), Some(inserted));
    assert!(chat.state().commands.try_recv().is_err());
    Ok(())
}

#[test]
fn composer_unicode_ime_commit_preserves_surrounding_text_and_requires_later_send() {
    let mut chat = harness("e\u{301} 🌱");
    chat.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::End);
    chat.run();
    for text in ["にほん", "にほんご"] {
        chat.event(egui::Event::Ime(egui::ImeEvent::Preedit {
            text: text.to_owned(),
            active_range_chars: None,
        }));
        chat.run();
        assert!(chat.state().app.composing);
        assert!(chat.state().commands.try_recv().is_err());
    }
    chat.input_mut().events.extend([
        egui::Event::Ime(egui::ImeEvent::Commit("日本語".to_owned())),
        egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        },
    ]);
    chat.run();
    let committed = "e\u{301} 🌱日本語";
    assert_eq!(chat.state().app.input, committed);
    assert_eq!(
        cursor(&chat),
        Some(egui::text::CCursorRange::one(egui::text::CCursor::new(7)))
    );
    assert!(!chat.state().app.composing);
    assert!(chat.state().app.model.ready());
    assert!(chat.state().commands.try_recv().is_err());

    chat.key_press(egui::Key::Enter);
    chat.run();
    assert!(matches!(
        chat.state().commands.try_recv(),
        Ok(Command::Send { text, .. }) if text == committed
    ));
    assert!(chat.state().commands.try_recv().is_err());
}
