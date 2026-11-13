//! Model-free history filtering, keyboard routing and large-list interaction.
use super::*;
use egui_kittest::{Harness, kittest::Queryable as _};

const DRAFT: &str = "Keep my next thought 🌱";

struct Fixture {
    app: State,
    commands: Receiver<Command>,
    _events: client::EventSender,
}

fn fixture() -> Fixture {
    let (commands, requests) = mpsc::channel();
    let (events, incoming) = client::event_channel();
    let mut app = State::create(commands, incoming, None, None);
    app.preview = false;
    app.model = ModelLifecycle::Ready;
    app.conversations[0].title = "Garden notes".into();
    app.transcript
        .push(Role::User, "An ÉLAN beneath the cedar trees.".into());
    app.transcript
        .push(Role::Assistant, "A quiet corner of the garden.".into());
    app.transcript.remove_empty_response();
    app.input = "An unfinished gardening draft".into();
    app.new_chat();
    app.conversations[1].title = "Voyage à Montréal".into();
    app.transcript
        .push(Role::User, "Walk beside the river.".into());
    app.transcript.push(Role::Assistant, String::new());
    app.transcript.append("PrivateCompass", true);
    app.transcript
        .append("A route through the old city.", false);
    app.transcript.remove_empty_response();
    app.input = "Unsent Cobalt itinerary".into();
    app.new_chat();
    app.conversations[2].title = "Notebook".into();
    app.transcript.push(Role::User, "Watch a seedling.".into());
    app.transcript.push(Role::Assistant, String::new());
    app.input = DRAFT.into();
    while requests.try_recv().is_ok() {}
    Fixture {
        app,
        commands: requests,
        _events: events,
    }
}

fn harness(size: [f32; 2]) -> Harness<'static, Fixture> {
    Harness::builder()
        .with_size(size)
        .with_context_setup(|ctx| {
            configure(ctx);
            ctx.add_font_provider(std::sync::Arc::new(
                egui_system_font_provider::SystemFontProvider::new(),
            ));
        })
        .build_ui_state(
            |ui, fixture: &mut Fixture| {
                fixture.app.handle_input_shortcuts(ui.ctx());
                fixture.app.draw(ui);
            },
            fixture(),
        )
}

fn type_query(chat: &mut Harness<'_, Fixture>, query: &str) {
    chat.get_by_role_and_label(egui::Role::TextInput, "Search conversations")
        .focus();
    chat.run();
    chat.get_by_role_and_label(egui::Role::TextInput, "Search conversations")
        .type_text(query);
    chat.run();
}

fn clear_query(chat: &mut Harness<'_, Fixture>) {
    chat.get_by_role_and_label(egui::Role::Button, "Clear conversation search")
        .click();
    chat.run();
}

fn open_sidebar(chat: &mut Harness<'_, Fixture>, width: f32) {
    if width < navigation::DOCK_THRESHOLD {
        chat.get_by_role_and_label(egui::Role::Button, "Conversation menu")
            .click();
        chat.run();
    }
}

#[test]
fn find_sync_sidebar_edits_update_open_result_without_reselecting() {
    for size in [[1180.0, 860.0], [480.0, 640.0]] {
        let mut chat = harness(size);
        open_sidebar(&mut chat, size[0]);
        type_query(&mut chat, "cedar");
        chat.get_by_role_and_label(egui::Role::Button, "Garden notes")
            .click();
        chat.run();
        let selected = chat.state().app.active_conversation;
        open_sidebar(&mut chat, size[0]);
        chat.get_by_role_and_label(egui::Role::TextInput, "Search conversations")
            .focus();
        chat.run();
        chat.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        chat.get_by_role_and_label(egui::Role::TextInput, "Search conversations")
            .type_text("garden");
        chat.run();
        assert!(
            chat.get_by_role_and_label(egui::Role::TextInput, "Search conversations")
                .is_focused()
        );
        if size[0] < navigation::DOCK_THRESHOLD {
            chat.get_by_role_and_label(egui::Role::Button, "Close conversations")
                .click();
            chat.run();
        }
        assert_eq!(chat.state().app.active_conversation, selected);
        assert_eq!(
            chat.get_by_role_and_label(egui::Role::TextInput, "Find in conversation")
                .value()
                .as_deref(),
            Some("garden")
        );
        assert!(chat.get_by_label("1 of 1").rect().is_positive());
        assert_eq!(chat.state().app.input, "An unfinished gardening draft");
        while let Ok(command) = chat.state().commands.try_recv() {
            assert!(!matches!(command, Command::Send { .. }));
        }
    }
}

#[test]
fn find_sync_sidebar_updates_selected_chat_and_clears_without_taking_focus() {
    let mut chat = harness([1180.0, 860.0]);
    let selected = chat.state().app.active_conversation;
    type_query(&mut chat, "seed");
    assert_eq!(
        chat.get_by_role_and_label(egui::Role::TextInput, "Find in conversation")
            .value()
            .as_deref(),
        Some("seed")
    );
    assert!(chat.get_by_label("1 of 1").rect().is_positive());
    type_query(&mut chat, " does not exist");
    assert_eq!(
        chat.state().app.conversation_search.rows,
        Vec::<usize>::new()
    );
    assert_eq!(
        chat.get_by_role_and_label(egui::Role::TextInput, "Find in conversation")
            .value()
            .as_deref(),
        Some("seed does not exist")
    );
    assert!(chat.get_by_label("No matches").rect().is_positive());
    clear_query(&mut chat);
    assert_eq!(
        chat.get_by_role_and_label(egui::Role::TextInput, "Find in conversation")
            .value()
            .as_deref(),
        Some("")
    );
    type_query(&mut chat, "seed");
    chat.key_press(egui::Key::Escape);
    chat.run();
    assert_eq!(
        chat.get_by_role_and_label(egui::Role::TextInput, "Find in conversation")
            .value()
            .as_deref(),
        Some("")
    );
    assert!(
        chat.get_by_role_and_label(egui::Role::TextInput, "Search conversations")
            .is_focused()
    );
    assert_eq!(chat.state().app.active_conversation, selected);
    assert_eq!(chat.state().app.input, DRAFT);
    assert!(chat.state().commands.try_recv().is_err());
}

#[test]
fn find_sync_sidebar_selection_and_reselection_open_the_same_find() {
    for size in [[1180.0, 860.0], [480.0, 640.0]] {
        let mut chat = harness(size);
        let original = chat.state().app.active_conversation;
        open_sidebar(&mut chat, size[0]);
        type_query(&mut chat, "  ÉLAN  ");
        chat.get_by_role_and_label(egui::Role::Button, "Garden notes")
            .click();
        chat.run();
        assert!(!chat.state().app.sidebar_open);
        let field = chat.get_by_role_and_label(egui::Role::TextInput, "Find in conversation");
        assert!(field.is_focused());
        assert_eq!(field.value().as_deref(), Some("ÉLAN"));
        assert!(chat.get_by_label("1 of 1").rect().is_positive());
        assert_eq!(chat.state().app.input, "An unfinished gardening draft");
        let saved = chat
            .state()
            .app
            .conversations
            .iter()
            .find(|conversation| conversation.id == original)
            .and_then(|conversation| conversation.saved.as_ref());
        assert_eq!(saved.map(|snapshot| snapshot.input.as_str()), Some(DRAFT));

        // Local edits and dismissal must not rewrite the cross-chat filter.
        field.type_text("!");
        chat.run();
        assert!(chat.get_by_label("No matches").rect().is_positive());
        assert_eq!(chat.state().app.conversation_search.text, "  ÉLAN  ");
        chat.key_press(egui::Key::Escape);
        chat.run();
        chat.run_steps(3);
        assert!(chat.query_by_label("Find in conversation").is_none());
        open_sidebar(&mut chat, size[0]);
        chat.get_by_role_and_label(egui::Role::Button, "Garden notes")
            .click();
        chat.run();
        assert!(chat.get_by_label("1 of 1").rect().is_positive());
        assert!(
            chat.get_by_role_and_label(egui::Role::TextInput, "Find in conversation")
                .is_focused()
        );
        while let Ok(command) = chat.state().commands.try_recv() {
            assert!(!matches!(command, Command::Send { .. }));
        }
    }
}

#[test]
fn find_sync_ctrl_f_leaves_the_drawer_and_preserves_its_filter() {
    let mut chat = harness([480.0, 640.0]);
    open_sidebar(&mut chat, 480.0);
    type_query(&mut chat, "garden");
    chat.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::F);
    chat.run();
    assert!(!chat.state().app.sidebar_open);
    assert!(
        chat.get_by_role_and_label(egui::Role::TextInput, "Find in conversation")
            .is_focused()
    );
    assert_eq!(chat.state().app.conversation_search.text, "garden");
    assert_eq!(chat.state().app.input, DRAFT);
    assert!(chat.state().commands.try_recv().is_err());
}

#[test]
fn find_sync_sidebar_and_find_share_unicode_and_long_phrase_matching() {
    for query in ["Σ".to_owned(), "cedar ".repeat(50)] {
        let mut chat = harness([1180.0, 860.0]);
        let query = query.trim();
        chat.state_mut()
            .app
            .transcript
            .replace_response(&format!("Ο{query} then Ο{query}"), None);
        chat.run();
        type_query(&mut chat, query);
        assert_eq!(chat.state().app.conversation_search.rows, [2]);
        chat.get_by_role_and_label(egui::Role::Button, "Notebook")
            .click();
        chat.run();
        assert_eq!(
            chat.get_by_role_and_label(egui::Role::TextInput, "Find in conversation")
                .value()
                .as_deref(),
            Some(query)
        );
        assert!(chat.get_by_label("1 of 2").rect().is_positive());
        chat.key_press(egui::Key::F3);
        chat.run();
        chat.run_steps(3);
        assert!(chat.get_by_label("2 of 2").rect().is_positive());
        chat.get_by_role_and_label(egui::Role::Button, "Notebook")
            .click();
        chat.run();
        assert!(chat.get_by_label("1 of 2").rect().is_positive());
        assert!(chat.state().commands.try_recv().is_err());
    }
}

#[test]
fn find_sync_title_only_result_shows_no_body_matches() {
    let mut chat = harness([1180.0, 860.0]);
    chat.state_mut().app.transcript.clear();
    chat.run();
    type_query(&mut chat, "Notebook");
    chat.get_by_role_and_label(egui::Role::Button, "Notebook")
        .click();
    chat.run();
    assert_eq!(
        chat.get_by_role_and_label(egui::Role::TextInput, "Find in conversation")
            .value()
            .as_deref(),
        Some("Notebook")
    );
    assert!(chat.get_by_label("No matches").rect().is_positive());
    assert!(chat.state().commands.try_recv().is_err());
}

#[test]
fn conversation_history_filters_unicode_titles_and_inactive_messages_as_you_type() {
    let mut chat = harness([1180.0, 860.0]);
    let active = chat.state().app.active_conversation;
    type_query(&mut chat, "éL");
    assert_eq!(chat.state().app.conversation_search.rows, [0]);
    assert!(
        chat.query_by_role_and_label(egui::Role::Button, "Garden notes")
            .is_some()
    );
    assert!(
        chat.query_by_role_and_label(egui::Role::Button, "Voyage à Montréal")
            .is_none()
    );
    type_query(&mut chat, "AN");
    assert_eq!(chat.state().app.conversation_search.rows, [0]);
    clear_query(&mut chat);
    type_query(&mut chat, "MONTRÉAL");
    assert_eq!(chat.state().app.conversation_search.rows, [1]);
    assert!(
        chat.query_by_role_and_label(egui::Role::Button, "Voyage à Montréal")
            .is_some()
    );
    assert_eq!(chat.state().app.active_conversation, active);
    assert_eq!(chat.state().app.input, DRAFT);
    assert!(chat.state().commands.try_recv().is_err());
}

#[test]
fn conversation_history_reuses_matches_and_refreshes_streams_renames_and_replacements() {
    let mut chat = harness([1180.0, 860.0]);
    type_query(&mut chat, "new leaf");
    assert_eq!(
        chat.state().app.conversation_search.rows,
        Vec::<usize>::new()
    );
    let scans: Vec<_> = chat
        .state()
        .app
        .conversations
        .iter()
        .map(|conversation| conversation.search.scans)
        .collect();
    chat.run_steps(3);
    assert_eq!(
        chat.state()
            .app
            .conversations
            .iter()
            .map(|conversation| conversation.search.scans)
            .collect::<Vec<_>>(),
        scans
    );
    chat.state_mut().app.transcript.append("A NEW LEAF", false);
    chat.run();
    assert_eq!(chat.state().app.conversation_search.rows, [2]);
    assert_eq!(chat.state().app.conversations[0].search.scans, scans[0]);
    assert_eq!(chat.state().app.conversations[1].search.scans, scans[1]);
    assert_eq!(chat.state().app.conversations[2].search.scans, scans[2] + 1);
    chat.state_mut().app.conversations[0].title = "New leaf archive".into();
    chat.run();
    assert_eq!(chat.state().app.conversation_search.rows, [0, 2]);
    assert_eq!(chat.state().app.conversations[0].search.scans, scans[0] + 1);
    chat.state_mut()
        .app
        .transcript
        .replace_response("Other words", None);
    chat.run();
    assert_eq!(chat.state().app.conversation_search.rows, [0]);
    assert_eq!(chat.state().app.conversations[2].search.scans, scans[2] + 2);
}

#[test]
fn conversation_history_excludes_reasoning_and_drafts_and_clears_without_switching() {
    let mut chat = harness([1180.0, 860.0]);
    let active = chat.state().app.active_conversation;
    for query in ["privatecompass", "cobalt"] {
        type_query(&mut chat, query);
        assert_eq!(
            chat.state().app.conversation_search.rows,
            Vec::<usize>::new()
        );
        assert!(chat.query_by_label("No conversations found").is_some());
        clear_query(&mut chat);
        assert_eq!(chat.state().app.conversation_search.rows, [0, 1, 2]);
        assert_eq!(chat.state().app.active_conversation, active);
        assert_eq!(chat.state().app.input, DRAFT);
    }
    assert!(chat.state().commands.try_recv().is_err());
}

#[test]
fn conversation_history_enter_never_sends_the_composer() {
    let mut chat = harness([1180.0, 860.0]);
    type_query(&mut chat, "garden");
    chat.key_press(egui::Key::Enter);
    chat.run();
    assert_eq!(chat.state().app.model, ModelLifecycle::Ready);
    assert_eq!(chat.state().app.input, DRAFT);
    assert_eq!(chat.state().app.conversation_search.text, "garden");
    assert!(chat.state().commands.try_recv().is_err());
}

#[test]
fn conversation_history_escape_clears_before_drawer_dismissal_during_generation() {
    let mut chat = harness([480.0, 640.0]);
    chat.state_mut().app.model = ModelLifecycle::Generating;
    let active = chat.state().app.active_conversation;
    chat.state_mut().app.request_conversation = Some(active);
    chat.get_by_role_and_label(egui::Role::Button, "Conversation menu")
        .click();
    chat.run();
    type_query(&mut chat, "élan");
    chat.key_press(egui::Key::Escape);
    chat.run();
    assert_eq!(chat.state().app.conversation_search.text, "");
    assert!(chat.state().app.sidebar_open);
    assert!(chat.state().app.model.busy());
    assert!(
        chat.get_by_role_and_label(egui::Role::TextInput, "Search conversations")
            .is_focused()
    );
    assert_eq!(chat.state().app.input, DRAFT);
    assert!(chat.state().commands.try_recv().is_err());
    chat.key_press(egui::Key::Escape);
    chat.run();
    assert!(!chat.state().app.sidebar_open);
    assert!(chat.state().app.model.busy());
    assert_eq!(chat.state().app.input, DRAFT);
    assert!(chat.state().commands.try_recv().is_err());
}

#[test]
fn conversation_history_escape_from_another_control_preserves_the_query() {
    let mut chat = harness([480.0, 640.0]);
    chat.state_mut().app.model = ModelLifecycle::Generating;
    chat.get_by_role_and_label(egui::Role::Button, "Conversation menu")
        .click();
    chat.run();
    type_query(&mut chat, "garden");
    chat.get_by_role_and_label(egui::Role::Button, "Close conversations")
        .focus();
    chat.run();
    chat.key_press(egui::Key::Escape);
    chat.run();
    assert!(!chat.state().app.sidebar_open);
    assert_eq!(chat.state().app.conversation_search.text, "garden");
    assert!(chat.state().app.model.busy());
    assert_eq!(chat.state().app.input, DRAFT);
    assert!(chat.state().commands.try_recv().is_err());
}

#[test]
fn conversation_history_large_history_selects_offscreen_results_by_stable_identity() {
    let mut chat = harness([1180.0, 860.0]);
    for index in 3..80 {
        let app = &mut chat.state_mut().app;
        app.new_chat();
        app.conversations[index].title = if index == 57 {
            "Needle far in history".into()
        } else {
            format!("Conversation {index}")
        };
        app.input = format!("Draft {index}");
    }
    let target = chat.state().app.conversations[57].id;
    let original = chat.state().app.conversations[0].id;
    let selected = chat.state().app.active_conversation;
    while chat.state().commands.try_recv().is_ok() {}
    chat.run();
    assert_eq!(chat.state().app.conversations.len(), 80);
    assert!(
        chat.query_by_role_and_label(egui::Role::Button, "Needle far in history")
            .is_none()
    );
    type_query(&mut chat, "needle");
    assert_eq!(chat.state().app.conversation_search.rows, [57]);
    assert_eq!(chat.state().app.active_conversation, selected);
    chat.state_mut().app.remove_conversation(original);
    chat.run();
    assert_eq!(chat.state().app.conversation_search.rows, [56]);
    chat.get_by_role_and_label(egui::Role::Button, "Needle far in history")
        .click();
    chat.run();
    assert_eq!(chat.state().app.active_conversation, target);
    assert_eq!(chat.state().app.input, "Draft 57");
    while let Ok(command) = chat.state().commands.try_recv() {
        assert!(!matches!(command, Command::Send { .. }));
    }
}

#[test]
fn conversation_history_new_conversation_clears_the_filter_and_preserves_the_draft() {
    let mut chat = harness([1180.0, 860.0]);
    let original = chat.state().app.active_conversation;
    type_query(&mut chat, "garden");
    chat.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::N);
    chat.run();
    assert_eq!(chat.state().app.conversations.len(), 4);
    assert_ne!(chat.state().app.active_conversation, original);
    assert_eq!(chat.state().app.conversation_search.text, "");
    assert_eq!(chat.state().app.conversation_search.rows, [0, 1, 2, 3]);
    assert!(chat.query_by_label("Find in conversation").is_none());
    assert!(
        chat.get_by_role_and_label(egui::Role::MultilineTextInput, "Message")
            .is_focused()
    );
    let saved = chat
        .state()
        .app
        .conversations
        .iter()
        .find(|conversation| conversation.id == original)
        .and_then(|conversation| conversation.saved.as_ref());
    assert_eq!(saved.map(|snapshot| snapshot.input.as_str()), Some(DRAFT));
    while let Ok(command) = chat.state().commands.try_recv() {
        assert!(!matches!(command, Command::Send { .. }));
    }
}
