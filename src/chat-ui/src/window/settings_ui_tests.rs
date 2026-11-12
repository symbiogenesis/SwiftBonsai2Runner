//! Native Settings keeps scope, focus and modal ownership explicit.
use super::*;
use egui_kittest::{
    Harness,
    kittest::{NodeT as _, Queryable as _},
};

struct Fixture {
    app: State,
    commands: Receiver<Command>,
    _events: client::EventSender,
}

fn harness(size: [f32; 2], render: bool) -> Harness<'static, Fixture> {
    let (commands, incoming) = mpsc::channel();
    let (events, incoming_events) = client::event_channel();
    let mut app = State::create(commands, incoming_events, None, None);
    app.preview = false;
    app.model = ModelLifecycle::Ready;
    app.input = "Keep my next draft 🌱".into();
    app.transcript.push(Role::User, "A question to find".into());
    app.transcript
        .push(Role::Assistant, "A thoughtful answer to find".into());
    app.transcript.remove_empty_response();
    let builder = Harness::builder()
        .with_size(size)
        .with_context_setup(|context| {
            configure(context);
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
            fixture.app.flush_voice_transcript(ui.ctx());
        },
        Fixture {
            app,
            commands: incoming,
            _events: events,
        },
    )
}

fn open_settings(chat: &mut Harness<'_, Fixture>) {
    let mut clicked_header = false;
    for button in chat.query_all_by_role_and_label(egui::Role::Button, "Settings") {
        if button.rect().top() < 80.0 {
            button.click();
            clicked_header = true;
            break;
        }
    }
    assert!(clicked_header);
    chat.run();
    assert!(chat.state().app.show_settings);
}

fn select_page(chat: &mut Harness<'_, Fixture>, page: &str) {
    chat.get_by_role_and_label(egui::Role::Button, page).click();
    chat.run();
}

fn expand_advanced_response(chat: &mut Harness<'_, Fixture>) {
    chat.get_by_role_and_label(egui::Role::DisclosureTriangle, "Advanced response controls")
        .scroll_to_me();
    chat.run();
    chat.get_by_role_and_label(egui::Role::DisclosureTriangle, "Advanced response controls")
        .click();
    chat.run();
}

fn open_thinking(chat: &mut Harness<'_, Fixture>) {
    chat.get_by_role_and_label(egui::Role::Button, "Thinking")
        .click();
    chat.run();
    assert!(chat.query_by_label("Thinking options").is_some());
    assert!(!chat.state().app.show_settings);
}

#[test]
fn thinking_popup_has_only_two_controls_inside_minimum_and_wide_windows() {
    for size in [[480.0, 480.0], [620.0, 640.0], [1180.0, 860.0]] {
        let mut chat = harness(size, false);
        open_thinking(&mut chat);
        let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, size.into());
        let popup = chat.get_by_label("Thinking options");
        assert_eq!(popup.query_all_by_role(egui::Role::CheckBox).count(), 2);
        assert_eq!(popup.query_all_by_role(egui::Role::ComboBox).count(), 0);
        assert_eq!(popup.query_all_by_role(egui::Role::Slider).count(), 0);
        assert!(chat.query_by_label("Reply length").is_none());
        assert!(chat.query_by_label("Adapter strength").is_none());
        for label in ["Thinking", "Extra effort"] {
            let rect = popup
                .get_by_role_and_label(egui::Role::CheckBox, label)
                .rect();
            assert!(rect.height() >= 28.0 && bounds.contains_rect(rect));
        }
        assert_eq!(chat.state().app.input, "Keep my next draft 🌱");
        assert!(chat.state().commands.try_recv().is_err());
    }
}

#[test]
fn thinking_popup_changes_are_saved_only_for_the_selected_conversation() {
    let mut chat = harness([620.0, 640.0], false);
    let original = chat.state().app.active_conversation;
    let output_budget = chat.state().app.output_budget;
    let adapter = chat.state().app.abliteration_scale;
    chat.state_mut().app.recovery_dirty = false;
    open_thinking(&mut chat);
    assert!(!chat.state().app.recovery_dirty);
    for label in ["Thinking", "Extra effort"] {
        chat.get_by_role_and_label(egui::Role::CheckBox, label)
            .click();
        chat.run();
        assert!(chat.query_by_label("Thinking options").is_some());
    }
    assert!(chat.state().app.enable_thinking && chat.state().app.extra_effort);
    assert!(chat.state().app.recovery_dirty);
    assert_eq!(chat.state().app.output_budget, output_budget);
    assert!((chat.state().app.abliteration_scale - adapter).abs() < f32::EPSILON);
    chat.key_press(egui::Key::Escape);
    chat.run();
    chat.state_mut().app.new_chat();
    assert!(!chat.state().app.enable_thinking && !chat.state().app.extra_effort);
    chat.state_mut().app.switch_conversation(original);
    chat.run();
    assert!(chat.state().app.enable_thinking && chat.state().app.extra_effort);
    assert_eq!(chat.state().app.input, "Keep my next draft 🌱");
    assert!(
        chat.state()
            .commands
            .try_iter()
            .all(|command| !matches!(command, Command::Send { .. }))
    );
}

#[test]
fn thinking_popup_locks_both_choices_during_generation() {
    let mut chat = harness([620.0, 640.0], false);
    chat.state_mut().app.model = ModelLifecycle::Generating;
    chat.state_mut().app.recovery_dirty = false;
    open_thinking(&mut chat);
    for label in ["Thinking", "Extra effort"] {
        assert!(
            chat.get_by_role_and_label(egui::Role::CheckBox, label)
                .accesskit_node()
                .is_disabled()
        );
    }
    assert!(!chat.state().app.enable_thinking && !chat.state().app.extra_effort);
    assert!(!chat.state().app.recovery_dirty);
    assert!(chat.state().commands.try_recv().is_err());
}

#[test]
fn thinking_popup_keyboard_dismissal_keeps_the_draft_and_active_response() {
    let mut chat = harness([620.0, 640.0], false);
    let owner = chat.state().app.active_conversation;
    chat.state_mut().app.model = ModelLifecycle::Generating;
    chat.state_mut().app.request_conversation = Some(owner);
    chat.get_by_role_and_label(egui::Role::Button, "Thinking")
        .focus();
    chat.key_press(egui::Key::Space);
    chat.run();
    assert!(chat.query_by_label("Thinking options").is_some());
    chat.key_press(egui::Key::Enter);
    chat.run();
    assert!(chat.state().app.model.busy());
    assert!(chat.state().commands.try_recv().is_err());
    chat.key_press(egui::Key::Escape);
    chat.run();
    assert!(chat.query_by_label("Thinking options").is_none());
    assert!(chat.state().app.model.busy());
    assert_eq!(chat.state().app.input, "Keep my next draft 🌱");
    assert!(chat.state().commands.try_recv().is_err());
    chat.key_press(egui::Key::Escape);
    chat.run();
    assert!(matches!(
        chat.state().commands.try_recv(),
        Ok(Command::Stop)
    ));
}

#[test]
fn thinking_and_context_popups_replace_each_other() {
    let mut chat = harness([620.0, 640.0], false);
    chat.get_by_role_and_label(egui::Role::Button, "Context")
        .click();
    chat.run();
    assert!(chat.query_by_label("Space for this draft").is_some());
    open_thinking(&mut chat);
    assert!(chat.query_by_label("Space for this draft").is_none());
    chat.get_by_role_and_label(egui::Role::Button, "Context")
        .click();
    chat.run();
    assert!(chat.query_by_label("Thinking options").is_none());
    assert!(chat.query_by_label("Space for this draft").is_some());
    assert!(chat.state().commands.try_recv().is_err());
}

#[test]
fn thinking_popup_capture_when_requested() -> io::Result<()> {
    let Some(value) = std::env::var_os("BONSAI_UI_CAPTURE_DIRECTORY") else {
        return Ok(());
    };
    let directory = std::path::PathBuf::from(value);
    bonsai_windows_credentials::validate_external_path(&directory)?;
    std::fs::create_dir_all(&directory)?;
    for (name, size, theme) in [
        (
            "thinking-popup-wide.png",
            [1180.0, 860.0],
            appearance::Theme::Dark,
        ),
        (
            "thinking-popup-minimum.png",
            [480.0, 480.0],
            appearance::Theme::Dark,
        ),
        (
            "thinking-popup-light.png",
            [620.0, 640.0],
            appearance::Theme::Light,
        ),
    ] {
        let mut chat = harness(size, true);
        chat.state_mut().app.appearance.theme = theme;
        chat.state().app.appearance.apply(&chat.ctx);
        chat.state_mut().app.enable_thinking = true;
        chat.run();
        open_thinking(&mut chat);
        chat.render()
            .map_err(io::Error::other)?
            .save(directory.join(name))
            .map_err(io::Error::other)?;
        assert!(chat.state().app.worker.is_none());
        assert!(chat.state().app.speech.is_none());
    }
    Ok(())
}

#[test]
fn settings_modal_keeps_navigation_and_dismissal_inside_small_and_large_windows() {
    for size in [[1180.0, 860.0], [620.0, 640.0]] {
        let mut chat = harness(size, false);
        open_settings(&mut chat);
        for page in ["Response", "Voice", "Privacy", "Application"] {
            select_page(&mut chat, page);
            let scope = if page == "Response" {
                "This conversation"
            } else {
                "All conversations · This device"
            };
            assert!(chat.get_by_label(scope).rect().is_positive());
            for label in [
                "Response",
                "Voice",
                "Privacy",
                "Application",
                "Close settings",
                "Done",
            ] {
                let rect = chat.get_by_role_and_label(egui::Role::Button, label).rect();
                assert!(
                    rect.is_positive()
                        && rect.left() >= 0.0
                        && rect.top() >= 0.0
                        && rect.right() <= size[0]
                        && rect.bottom() <= size[1],
                    "Settings control {label} on {page} escaped {size:?}: {rect:?}"
                );
            }
        }
        assert_eq!(chat.state().app.input, "Keep my next draft 🌱");
        assert!(chat.state().commands.try_recv().is_err());
        chat.get_by_role_and_label(egui::Role::Button, "Done")
            .click();
        chat.run();
        assert!(!chat.state().app.show_settings);
        assert_eq!(
            chat.ctx.memory(egui::Memory::focused),
            chat.state().app.composer_id
        );
    }
}

#[test]
fn settings_modal_escape_preserves_generation_speech_find_and_the_unsent_draft() {
    for size in [[1180.0, 860.0], [620.0, 640.0]] {
        let mut chat = harness(size, false);
        chat.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::F);
        chat.run();
        chat.get_by_role_and_label(egui::Role::TextInput, "Find in conversation")
            .type_text("find");
        chat.run();
        chat.state_mut().app.model = ModelLifecycle::Generating;
        chat.state_mut().app.speaking = true;
        let original = chat.state().app.active_conversation;
        open_settings(&mut chat);
        chat.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::N);
        chat.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::F);
        chat.key_press(egui::Key::F3);
        chat.key_press(egui::Key::Enter);
        chat.run();
        assert_eq!(chat.state().app.active_conversation, original);
        assert_eq!(chat.state().app.conversations.len(), 1);
        assert!(chat.state().app.show_settings);
        assert!(chat.state().commands.try_recv().is_err());
        chat.key_press(egui::Key::Escape);
        chat.run();
        assert!(!chat.state().app.show_settings);
        assert!(chat.state().app.model.busy());
        assert!(chat.state().app.speaking);
        assert!(chat.query_by_label("Find in conversation").is_some());
        assert_eq!(chat.state().app.input, "Keep my next draft 🌱");
        assert!(chat.state().commands.try_recv().is_err());
    }
}

#[test]
fn settings_modal_owns_shortcuts_on_the_first_frame_it_opens() {
    let mut chat = harness([1180.0, 860.0], false);
    chat.get_by_role_and_label(egui::Role::MultilineTextInput, "Message")
        .focus();
    chat.run();
    chat.state_mut().app.show_settings = true;
    chat.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::N);
    chat.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::F);
    chat.key_press(egui::Key::Enter);
    chat.run();
    assert!(chat.state().app.show_settings);
    assert_eq!(chat.state().app.conversations.len(), 1);
    assert_eq!(chat.state().app.input, "Keep my next draft 🌱");
    assert!(chat.query_by_label("Find in conversation").is_none());
    assert!(chat.state().commands.try_recv().is_err());
}

#[test]
fn settings_response_scope_stays_with_its_conversation_and_voice_is_shared() {
    let mut chat = harness([1180.0, 860.0], false);
    let original = chat.state().app.active_conversation;
    chat.state_mut().app.recovery_dirty = false;
    open_settings(&mut chat);
    assert!(!chat.state().app.recovery_dirty);
    chat.get_by_role_and_label(egui::Role::CheckBox, "Thinking")
        .click();
    chat.run();
    assert!(chat.state().app.enable_thinking);
    assert!(chat.state().app.recovery_dirty);
    select_page(&mut chat, "Voice");
    chat.get_by_role_and_label(egui::Role::MultilineTextInput, "Voice profile")
        .focus();
    chat.run();
    chat.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    chat.run();
    chat.get_by_role_and_label(egui::Role::MultilineTextInput, "Voice profile")
        .type_text("A clear, measured storytelling voice.");
    chat.run();
    let voice = chat.state().app.voice_profile.clone();
    assert_eq!(voice, "A clear, measured storytelling voice.");
    chat.get_by_role_and_label(egui::Role::Button, "Done")
        .click();
    chat.run();
    chat.state_mut().app.new_chat();
    let other = chat.state().app.active_conversation;
    assert_ne!(other, original);
    assert!(!chat.state().app.enable_thinking);
    assert_eq!(chat.state().app.voice_profile, voice);
    chat.state_mut().app.switch_conversation(original);
    assert!(chat.state().app.enable_thinking);
    assert_eq!(chat.state().app.voice_profile, voice);
    assert_eq!(chat.state().app.input, "Keep my next draft 🌱");
}

#[test]
fn settings_response_controls_stay_locked_while_a_response_is_generating() {
    let mut chat = harness([1180.0, 860.0], false);
    chat.state_mut().app.model = ModelLifecycle::Generating;
    open_settings(&mut chat);
    assert!(
        chat.get_by_role_and_label(egui::Role::CheckBox, "Thinking")
            .accesskit_node()
            .is_disabled()
    );
    expand_advanced_response(&mut chat);
    chat.get_by_role_and_label(egui::Role::ComboBox, "Output cap")
        .scroll_to_me();
    chat.run();
    assert!(
        chat.get_by_role_and_label(egui::Role::ComboBox, "Output cap")
            .accesskit_node()
            .is_disabled()
    );
    chat.get_by_role_and_label(egui::Role::Slider, "Abliteration scale")
        .scroll_to_me();
    chat.run();
    assert!(
        chat.get_by_role_and_label(egui::Role::Slider, "Abliteration scale")
            .accesskit_node()
            .is_disabled()
    );
    assert!(!chat.state().app.enable_thinking);
    assert!(chat.state().commands.try_recv().is_err());
}

#[test]
fn settings_popup_escape_closes_the_combo_before_the_modal() {
    for size in [[1180.0, 860.0], [620.0, 640.0]] {
        let mut chat = harness(size, false);
        open_settings(&mut chat);
        expand_advanced_response(&mut chat);
        chat.get_by_role_and_label(egui::Role::ComboBox, "Output cap")
            .scroll_to_me();
        chat.run();
        chat.get_by_role_and_label(egui::Role::ComboBox, "Output cap")
            .click();
        chat.run();
        assert!(egui::Popup::is_any_open(&chat.ctx));
        chat.key_press(egui::Key::Escape);
        chat.run();
        assert!(!egui::Popup::is_any_open(&chat.ctx));
        assert!(chat.state().app.show_settings);
        chat.key_press(egui::Key::Escape);
        chat.run();
        assert!(!chat.state().app.show_settings);
        assert_eq!(chat.state().app.input, "Keep my next draft 🌱");
        assert!(chat.state().commands.try_recv().is_err());
    }
}

#[test]
fn settings_reply_limit_is_advanced_and_keeps_each_conversations_choice() {
    for size in [[1180.0, 860.0], [620.0, 640.0]] {
        let mut chat = harness(size, false);
        let original = chat.state().app.active_conversation;
        chat.state_mut().app.recovery_dirty = false;
        assert!(
            chat.query_by_role_and_label(egui::Role::ComboBox, "Output cap")
                .is_none()
        );
        open_settings(&mut chat);
        assert!(
            chat.query_by_role_and_label(egui::Role::ComboBox, "Output cap")
                .is_none()
        );
        expand_advanced_response(&mut chat);
        chat.get_by_role_and_label(egui::Role::ComboBox, "Output cap")
            .scroll_to_me();
        chat.run();
        chat.get_by_role_and_label(egui::Role::ComboBox, "Output cap")
            .click();
        chat.run();
        chat.get_by_label("1024 tokens").click();
        chat.run();
        assert_eq!(chat.state().app.output_budget, 1);
        assert!(chat.state().app.recovery_dirty);
        assert!(chat.state().commands.try_recv().is_err());
        chat.get_by_role_and_label(egui::Role::Button, "Done")
            .click();
        chat.run();
        chat.state_mut().app.new_chat();
        assert_eq!(chat.state().app.output_budget, 3);
        chat.state_mut().app.switch_conversation(original);
        chat.run();
        assert_eq!(chat.state().app.output_budget, 1);
        assert_eq!(chat.state().app.input, "Keep my next draft 🌱");
        assert!(
            chat.state()
                .commands
                .try_iter()
                .all(|command| !matches!(command, Command::Send { .. }))
        );
    }
}

#[test]
fn settings_modal_capture_when_requested() -> io::Result<()> {
    let Some(value) = std::env::var_os("BONSAI_UI_CAPTURE_DIRECTORY") else {
        return Ok(());
    };
    let directory = std::path::PathBuf::from(value);
    bonsai_windows_credentials::validate_external_path(&directory)?;
    std::fs::create_dir_all(&directory)?;
    for (name, size) in [
        ("settings-response-wide.png", [1180.0, 860.0]),
        ("settings-response-narrow.png", [620.0, 640.0]),
    ] {
        let mut chat = harness(size, true);
        open_settings(&mut chat);
        chat.render()
            .map_err(io::Error::other)?
            .save(directory.join(name))
            .map_err(io::Error::other)?;
        assert!(chat.state().app.worker.is_none());
        assert!(chat.state().app.speech.is_none());
    }
    Ok(())
}
