//! Exercise adversarial error text, copy payloads and the combined notice budget.
use super::*;
use egui_kittest::{Harness, kittest::Queryable as _};

fn configure_error_fixture(context: &egui::Context) {
    configure(context);
    context.add_font_provider(std::sync::Arc::new(
        egui_system_font_provider::SystemFontProvider::new(),
    ));
}

struct CardFixture {
    message: String,
    rect: egui::Rect,
    copied: Option<String>,
}

fn long_error() -> String {
    format!(
        "The operation could not finish.\r\n{}\nFinal cause: permission denied. 日本語 🌱",
        "C:\\a-very-long-path-without-spaces\\日本語\\artifact: nested cause\n".repeat(1_000)
    )
}

#[test]
fn error_reporting_expanded_cards_stay_bounded_and_copy_every_character() {
    for (width, theme) in [(240.0, egui::Theme::Light), (460.0, egui::Theme::Dark)] {
        let message = long_error();
        let mut chat = Harness::builder()
            .with_size(egui::vec2(width, 480.0))
            .with_theme(theme)
            .with_context_setup(|ctx| {
                configure_error_fixture(ctx);
                ctx.global_style_mut(|style| {
                    style
                        .text_styles
                        .insert(egui::TextStyle::Body, egui::FontId::proportional(24.0));
                });
            })
            .build_ui_state(
                |ui, fixture: &mut CardFixture| {
                    fixture.rect = ui
                        .scope(|ui| {
                            error_ui::card(
                                ui,
                                "test-error",
                                "Setup needs attention",
                                &fixture.message,
                                &fixture.message,
                                |ui| {
                                    let _retry = ui.button("Retry setup");
                                },
                            );
                        })
                        .response
                        .rect;
                    ui.ctx().output(|output| {
                        for command in &output.commands {
                            if let egui::OutputCommand::CopyText(text) = command {
                                fixture.copied = Some(text.clone());
                            }
                        }
                    });
                },
                CardFixture {
                    message: message.clone(),
                    rect: egui::Rect::NOTHING,
                    copied: None,
                },
            );
        chat.run();
        assert!(chat.state().rect.width() <= width);
        assert!(chat.state().rect.height() <= 190.0);
        chat.get_by_label("Details").click();
        chat.run();
        assert!(chat.state().rect.width() <= width);
        assert!(chat.state().rect.height() <= 190.0);
        chat.get_by_role_and_label(egui::Role::Button, "Copy error")
            .click();
        chat.run();
        assert_eq!(chat.state().copied.as_deref(), Some(message.as_str()));
    }
}

#[test]
fn error_reporting_current_status_keeps_details_without_breaking_recovery() -> io::Result<()> {
    let (commands, _incoming) = mpsc::channel();
    let (_events, incoming) = client::event_channel();
    let mut state = State::create(commands, incoming, None, None);
    let message = format!(
        "{}\nProtected credentials and technical evidence: final detail",
        long_error()
    );
    state.set_status(&message, true);
    assert_eq!(state.status_detail, message);
    assert!(state.status.chars().count() <= MAX_DIAGNOSTIC_CHARS);
    assert!(
        state
            .diagnostics
            .back()
            .is_some_and(|entry| entry.chars().count() <= MAX_DIAGNOSTIC_CHARS)
    );
    let saved: serde_json::Value =
        serde_json::from_slice(&state.workspace_bytes()?).map_err(io::Error::other)?;
    assert!(saved["active"].get("status_detail").is_none());
    Ok(())
}

#[test]
fn error_reporting_stacked_failures_leave_the_composer_available() {
    let (commands, _incoming) = mpsc::channel();
    let (_events, incoming) = client::event_channel();
    let mut app = State::create(commands, incoming, None, None);
    app.preview = false;
    app.model = ModelLifecycle::Unavailable;
    app.fail_startup(&io::Error::other(long_error()));
    app.setup.speech_error = Some(long_error());
    app.speech_notice = true;
    app.speech_status = long_error();
    app.audio_file_error = true;
    app.audio_file_notice = Some(format!("Audio import failed: {}", long_error()));
    app.recovery_notice = format!("Could not save conversations: {}", long_error());
    app.input = "Keep this draft".to_owned();
    let mut chat = Harness::builder()
        .with_size(egui::vec2(480.0, 480.0))
        .with_context_setup(configure_error_fixture)
        .build_ui_state(|ui, state: &mut State| state.draw(ui), app);
    chat.run();
    let composer = chat
        .get_by_role_and_label(egui::Role::MultilineTextInput, "Message")
        .rect();
    assert!(composer.min.y >= 0.0 && composer.max.y <= 480.0);
    assert!(composer.width() > 200.0 && composer.height() > 20.0);
    assert_eq!(chat.state().input, "Keep this draft");
}

#[test]
fn error_reporting_capture_when_requested() -> io::Result<()> {
    let Some(directory) = std::env::var_os("BONSAI_ERROR_CAPTURE_DIRECTORY") else {
        return Ok(());
    };
    let directory = std::path::PathBuf::from(directory);
    bonsai_windows_credentials::validate_external_path(&directory)?;
    std::fs::create_dir_all(&directory)?;
    for (name, size, theme) in [
        ("error-minimum.png", [480.0, 480.0], appearance::Theme::Dark),
        ("error-wide.png", [1080.0, 780.0], appearance::Theme::Light),
    ] {
        let (commands, _incoming) = mpsc::channel();
        let (_events, incoming) = client::event_channel();
        let mut app = State::create(commands, incoming, None, None);
        app.preview = false;
        app.fail_startup(&io::Error::other(long_error()));
        app.input = "My draft stays available while I resolve this error.".to_owned();
        app.appearance.theme = theme;
        let mut setup = eframe::egui_wgpu::WgpuSetupCreateNew::without_display_handle();
        setup.instance_descriptor.backends = wgpu::Backends::DX12;
        let mut chat = Harness::builder()
            .with_size(size)
            .with_context_setup(configure_error_fixture)
            .wgpu_setup(eframe::egui_wgpu::WgpuSetup::CreateNew(setup))
            .build_ui_state(|ui, state: &mut State| state.draw(ui), app);
        chat.state().appearance.apply(&chat.ctx);
        chat.run();
        chat.get_by_label("Details").click();
        chat.run();
        chat.render()
            .map_err(io::Error::other)?
            .save(directory.join(name))
            .map_err(io::Error::other)?;
    }
    Ok(())
}
