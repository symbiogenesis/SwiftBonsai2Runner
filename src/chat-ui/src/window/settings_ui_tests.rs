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
    chat.get_by_role_and_label(egui::Role::Button, "Response settings")
        .click();
    chat.run();
    assert!(chat.state().app.show_settings);
}

fn select_page(chat: &mut Harness<'_, Fixture>, page: &str) {
    chat.get_by_role_and_label(egui::Role::Button, page).click();
    chat.run();
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
    chat.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::A);
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
    chat.get_by_role_and_label(egui::Role::Button, "Advanced response controls")
        .scroll_to_me();
    chat.run();
    chat.get_by_role_and_label(egui::Role::Button, "Advanced response controls")
        .click();
    chat.run();
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
