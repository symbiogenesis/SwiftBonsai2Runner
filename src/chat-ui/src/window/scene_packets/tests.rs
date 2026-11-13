//! Source excerpts remain reviewed drafts with bounded, private modal state.

use super::*;
use crate::{client, window::ModelLifecycle};
use egui::TextBuffer as _;
use egui_kittest::{
    Harness,
    kittest::{NodeT as _, Queryable as _},
};
use std::sync::mpsc;

fn fixture() -> (State, mpsc::Receiver<client::Command>) {
    let (commands, incoming) = mpsc::channel();
    let (_events, events) = client::event_channel();
    let mut state = State::create(commands, events, None, None);
    state.preview = false;
    state.model = ModelLifecycle::Ready;
    state.input = "Keep this draft 🌱".into();
    (state, incoming)
}

fn reviewed(conversation: u64) -> Result<Dialog, &'static str> {
    let mut dialog = Dialog::new(conversation);
    dialog.title = "Project bible".into();
    dialog.version = "Revision 7".into();
    dialog.source.text = "Mira kept the blue key.\nThe door stayed locked.".into();
    dialog.query = "Mira".into();
    dialog.set_packet(
        Source::new(&dialog.title, &dialog.version, &dialog.source.text)?.search(&dialog.query)?,
    );
    Ok(dialog)
}

#[test]
fn scene_packet_selection_is_explicit_and_appending_preserves_draft_and_history()
-> Result<(), &'static str> {
    let (mut app, incoming) = fixture();
    app.committed.push(client::Message {
        role: "user",
        content: "Keep this history".into(),
        reasoning_content: None,
    });
    let mut dialog = reviewed(app.active_conversation)?;
    assert!(!dialog.selected.is_empty());
    assert!(dialog.selected.iter().all(|selected| !selected));
    assert!(app.add_scene_packet(&dialog).is_err());
    assert_eq!(app.input, "Keep this draft 🌱");
    dialog.selected[0] = true;
    app.add_scene_packet(&dialog)?;
    assert!(app.input.starts_with("Keep this draft 🌱\n\n"));
    assert!(app.input.contains("Mira kept the blue key."));
    assert!(app.input.contains("Revision 7"));
    assert_eq!(app.committed.len(), 1);
    assert_eq!(app.committed[0].content.as_ref(), "Keep this history");
    assert!(app.recovery_dirty);
    assert!(incoming.try_recv().is_err());
    Ok(())
}

#[test]
fn scene_packet_overflow_busy_and_changed_conversation_leave_drafts_untouched()
-> Result<(), &'static str> {
    let (mut app, incoming) = fixture();
    let mut dialog = reviewed(app.active_conversation)?;
    dialog.selected[0] = true;
    let packet = dialog.render()?;
    let fitting = INPUT_LIMIT - 2 - packet.chars().count();
    app.input = "🌱".repeat(fitting + 1);
    assert!(app.add_scene_packet(&dialog).is_err());
    assert_eq!(app.input.chars().count(), fitting + 1);
    app.input.pop();
    app.model = ModelLifecycle::Generating;
    assert!(app.add_scene_packet(&dialog).is_err());
    app.model = ModelLifecycle::Ready;
    dialog.conversation += 1;
    assert!(app.add_scene_packet(&dialog).is_err());
    assert_eq!(app.input.chars().count(), fitting);
    dialog.conversation = app.active_conversation;
    app.add_scene_packet(&dialog)?;
    assert_eq!(app.input.chars().count(), INPUT_LIMIT);
    assert!(incoming.try_recv().is_err());
    Ok(())
}

#[test]
fn scene_packet_source_buffer_rejects_oversized_unicode_without_truncation() {
    let mut source = SourceText {
        text: "🌱".repeat(MAX_SOURCE_BYTES / 4),
        rejected: false,
        deleted: None,
    };
    assert_eq!(source.text.len(), MAX_SOURCE_BYTES);
    assert_eq!(source.insert_text("x", egui::text::CharIndex(0)), 0);
    assert!(source.rejected);
    assert_eq!(source.text.len(), MAX_SOURCE_BYTES);
    source.replace_with(&"x".repeat(MAX_SOURCE_BYTES + 1));
    assert!(source.text.starts_with('🌱'));
    source.delete_char_range(egui::text::CharIndex(0)..egui::text::CharIndex(1));
    assert_eq!(source.insert_text("🌱", egui::text::CharIndex(0)), 1);
    assert_eq!(source.text.len(), MAX_SOURCE_BYTES);
}

#[test]
fn scene_packet_oversized_selection_replacement_restores_the_exact_source() {
    let mut source = SourceText {
        text: "Keep 🌱 this exact source.".into(),
        ..SourceText::default()
    };
    source.delete_char_range(egui::text::CharIndex(5)..egui::text::CharIndex(11));
    assert_eq!(
        source.insert_text(&"x".repeat(MAX_SOURCE_BYTES), egui::text::CharIndex(5)),
        0
    );
    assert!(source.rejected);
    assert_eq!(source.text, "Keep 🌱 this exact source.");
    assert!(source.deleted.is_none());
}

#[test]
fn scene_packet_close_discards_private_source_and_joins_its_single_worker() {
    let (completed, incoming) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let _sent = completed.send(());
        Err("Discard this closed search result.")
    });
    let mut view = View {
        dialog: Some(Dialog::new(7)),
        task: Some(Task {
            conversation: 7,
            worker,
        }),
    };
    view.dialog.take();
    assert!(!view.is_open());
    assert!(view.task.is_some());
    drop(view);
    assert!(incoming.try_recv().is_ok());
}

struct Fixture {
    app: State,
    commands: mpsc::Receiver<client::Command>,
}

fn harness(size: [f32; 2]) -> Harness<'static, Fixture> {
    let (app, commands) = fixture();
    Harness::builder().with_size(size).build_ui_state(
        |ui, fixture: &mut Fixture| {
            fixture.app.handle_input_shortcuts(ui.ctx());
            fixture.app.draw(ui);
        },
        Fixture { app, commands },
    )
}

#[test]
fn scene_packet_modal_blocks_send_and_global_shortcuts_and_clears_on_cancel() {
    let mut chat = harness([620.0, 640.0]);
    let original = chat.state().app.active_conversation;
    chat.get_by_role_and_label(egui::Role::Button, "Sources")
        .click();
    chat.run();
    assert!(chat.query_by_label("Sources for this message").is_some());
    chat.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::N);
    chat.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::F);
    chat.key_press_modifiers(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::B);
    chat.key_press(egui::Key::Enter);
    chat.run();
    assert_eq!(chat.state().app.active_conversation, original);
    assert!(!chat.state().app.sidebar_open);
    assert!(chat.state().commands.try_recv().is_err());
    assert_eq!(chat.state().app.input, "Keep this draft 🌱");
    chat.key_press(egui::Key::Escape);
    chat.run();
    assert!(!chat.state().app.scene_packets.is_open());
    assert!(chat.state().commands.try_recv().is_err());
    chat.get_by_role_and_label(egui::Role::Button, "Sources")
        .click();
    chat.run();
    let dialog = chat.state().app.scene_packets.dialog.as_ref();
    assert!(dialog.is_some_and(|dialog| dialog.source.text.is_empty()
        && dialog.title.is_empty()
        && dialog.packet.is_none()));
}

#[test]
fn scene_packet_review_adds_only_after_explicit_action_at_narrow_and_wide_sizes()
-> Result<(), &'static str> {
    for size in [[480.0, 480.0], [1180.0, 860.0]] {
        let mut chat = harness(size);
        let mut dialog = reviewed(chat.state().app.active_conversation)?;
        dialog.selected[0] = true;
        chat.state_mut().app.scene_packets.dialog = Some(dialog);
        chat.run();
        let add = chat.get_by_role_and_label(egui::Role::Button, "Add selected excerpts");
        assert!(egui::Rect::from_min_size(egui::Pos2::ZERO, size.into()).contains_rect(add.rect()));
        add.click();
        chat.run();
        assert!(!chat.state().app.scene_packets.is_open());
        assert!(chat.state().app.input.contains("Mira kept the blue key."));
        assert!(chat.state().commands.try_recv().is_err());
    }
    Ok(())
}

#[test]
fn scene_packet_modal_escape_during_composition_never_stops_or_sends() {
    let mut chat = harness([620.0, 640.0]);
    chat.state_mut().app.scene_packets.dialog = Some(Dialog::new(1));
    chat.state_mut().app.composing = true;
    chat.run();
    chat.key_press(egui::Key::Escape);
    chat.run();
    assert!(chat.state().app.scene_packets.is_open());
    assert!(chat.state().commands.try_recv().is_err());
}

#[test]
fn scene_packet_source_paste_replacement_is_transactional() {
    let mut chat = harness([620.0, 640.0]);
    let mut dialog = Dialog::new(1);
    dialog.source.text = "Keep 🌱 this exact source.".into();
    chat.state_mut().app.scene_packets.dialog = Some(dialog);
    chat.run();
    chat.get_by_role_and_label(egui::Role::TextInput, "Source text")
        .focus();
    chat.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    chat.event(egui::Event::Paste("x".repeat(MAX_SOURCE_BYTES + 1)));
    chat.run();
    assert!(
        chat.state()
            .app
            .scene_packets
            .dialog
            .as_ref()
            .is_some_and(|dialog| { dialog.source.text == "Keep 🌱 this exact source." })
    );
    assert!(
        chat.query_by_label(
            "That insertion exceeds the 1 MiB source limit. Paste a smaller passage."
        )
        .is_some()
    );
    assert!(chat.state().commands.try_recv().is_err());
}
