//! Model-free interaction coverage for navigation and reading ergonomics.
use super::*;
use egui_kittest::{Harness, kittest::Queryable as _};

struct Fixture {
    app: State,
    commands: Receiver<Command>,
    _events: client::EventSender,
}

fn harness(size: [f32; 2], render: bool) -> Harness<'static, Fixture> {
    let (commands, requests) = mpsc::channel();
    let (events, incoming) = client::event_channel();
    let mut app = State::create(commands, incoming, None, None);
    app.preview = false;
    app.model = ModelLifecycle::Ready;
    app.input = "Keep my next thought 🌱".into();
    app.transcript
        .push(Role::User, "Help me make a little room to think.".into());
    app.transcript.push(Role::Assistant, "# A fresh perspective\n\nLeave a little space between ideas.\n\n- Start with what matters.\n- Let the details follow.\n\n`Small steps add up.`".into());
    app.transcript.remove_empty_response();
    let builder = Harness::builder()
        .with_size(size)
        .with_context_setup(|ctx| {
            configure(ctx);
            ctx.add_font_provider(std::sync::Arc::new(
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
        Fixture {
            app,
            commands: requests,
            _events: events,
        },
    )
}

#[test]
fn ergonomics_sidebar_resizes_collapses_and_reopens_with_the_draft_intact() {
    let mut chat = harness([1180.0, 860.0], false);
    let start = egui::pos2(chat.state().app.appearance.sidebar_width, 260.0);
    let end = start + egui::vec2(48.0, 0.0);
    chat.hover_at(start);
    chat.drag_at(start);
    chat.run_steps(1);
    chat.hover_at(end);
    chat.run_steps(1);
    chat.drop_at(end);
    chat.run();
    assert!(chat.state().app.appearance.sidebar_width > 280.0);
    chat.get_by_role_and_label(egui::Role::Button, "Hide conversations")
        .click();
    chat.run();
    assert!(!chat.state().app.appearance.sidebar_visible);
    assert!(
        chat.query_by_role_and_label(egui::Role::Button, "+ New conversation")
            .is_none()
    );
    chat.get_by_role_and_label(egui::Role::Button, "Show conversations")
        .click();
    chat.run();
    assert!(chat.state().app.appearance.sidebar_visible);
    assert_eq!(chat.state().app.input, "Keep my next thought 🌱");
    assert!(chat.state().commands.try_recv().is_err());
}

#[test]
fn ergonomics_drawer_escape_preserves_generation_and_returns_to_the_draft() {
    let mut chat = harness([480.0, 480.0], false);
    chat.state_mut().app.model = ModelLifecycle::Generating;
    chat.get_by_role_and_label(egui::Role::Button, "Conversation menu")
        .click();
    chat.run();
    assert!(chat.state().app.sidebar_open);
    let close = chat
        .get_by_role_and_label(egui::Role::Button, "Close conversations")
        .rect();
    assert!(close.right() < 400.0 && close.top() < 50.0);
    chat.key_press(egui::Key::Escape);
    chat.run();
    assert!(!chat.state().app.sidebar_open);
    assert!(chat.state().app.model.busy());
    assert_eq!(chat.state().app.input, "Keep my next thought 🌱");
    assert!(chat.state().commands.try_recv().is_err());
    let composer = chat.get_by_role_and_label(egui::Role::MultilineTextInput, "Message");
    assert!(composer.is_focused());
}

#[test]
fn ergonomics_hidden_dock_dismisses_the_drawer_after_resizing_wide() {
    let mut chat = harness([480.0, 640.0], false);
    chat.state_mut().app.appearance.sidebar_visible = false;
    chat.get_by_role_and_label(egui::Role::Button, "Conversation menu")
        .click();
    chat.run();
    assert!(chat.state().app.sidebar_open);
    chat.set_size(egui::vec2(1180.0, 860.0));
    chat.run();
    assert!(!chat.state().app.sidebar_open);
    assert!(!chat.state().app.appearance.sidebar_visible);
    assert!(
        chat.query_by_role_and_label(egui::Role::Button, "Close conversations")
            .is_none()
    );
    chat.state_mut().app.model = ModelLifecycle::Generating;
    chat.key_press(egui::Key::Escape);
    chat.run();
    assert!(matches!(
        chat.state().commands.try_recv(),
        Ok(Command::Stop)
    ));
}

#[test]
fn ergonomics_largest_reader_and_appearance_settings_fit_the_minimum_window() {
    let mut chat = harness([480.0, 480.0], false);
    chat.state_mut().app.appearance.text_size = 24;
    chat.state().app.appearance.apply(&chat.ctx);
    chat.run();
    let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(480.0, 480.0));
    assert!(
        bounds.contains_rect(
            chat.get_by_role_and_label(egui::Role::Button, "Send  ↑")
                .rect()
        )
    );
    chat.get_by_role_and_label(egui::Role::Button, "Settings")
        .click();
    chat.run();
    chat.get_by_role_and_label(egui::Role::Button, "Appearance")
        .click();
    chat.run();
    assert!(
        bounds.contains_rect(
            chat.get_by_role_and_label(egui::Role::Button, "Done")
                .rect()
        )
    );
    chat.key_press(egui::Key::Escape);
    chat.run();
    assert!(!chat.state().app.show_settings);
    assert_eq!(chat.state().app.input, "Keep my next thought 🌱");
    assert!(chat.state().commands.try_recv().is_err());
}

#[test]
fn ergonomics_capture_when_requested() -> io::Result<()> {
    let Some(directory) = std::env::var_os("BONSAI_ERGONOMICS_CAPTURE_DIRECTORY") else {
        return Ok(());
    };
    let directory = std::path::PathBuf::from(directory);
    bonsai_windows_credentials::validate_external_path(&directory)?;
    std::fs::create_dir_all(&directory)?;
    for (name, size, theme) in [
        ("light-wide.png", [1080.0, 780.0], appearance::Theme::Light),
        ("dark-drawer.png", [480.0, 640.0], appearance::Theme::Dark),
        (
            "appearance-minimum.png",
            [480.0, 480.0],
            appearance::Theme::Light,
        ),
    ] {
        let mut chat = harness(size, true);
        chat.state_mut().app.appearance.theme = theme;
        chat.state().app.appearance.apply(&chat.ctx);
        if name == "dark-drawer.png" {
            chat.state_mut().app.sidebar_open = true;
        } else if name == "appearance-minimum.png" {
            chat.state_mut()
                .app
                .open_settings(settings::Page::Appearance);
        }
        chat.run();
        chat.render()
            .map_err(io::Error::other)?
            .save(directory.join(name))
            .map_err(io::Error::other)?;
    }
    Ok(())
}
