//! Exercise shared alert selection, exact copy payloads and modal isolation.
use super::*;
use egui_kittest::{Harness, kittest::Queryable as _};

struct AlertFixture {
    app: State,
    commands: Receiver<Command>,
    copied: Option<String>,
}

fn harness(size: [f32; 2], font_size: f32) -> Harness<'static, AlertFixture> {
    let (commands, incoming) = mpsc::channel();
    let (_events, incoming_events) = client::event_channel();
    let mut app = State::create(commands, incoming_events, None, None);
    app.preview = false;
    app.model = ModelLifecycle::Ready;
    app.input = "Keep this draft".to_owned();
    Harness::builder()
        .with_size(size)
        .with_context_setup(move |ctx| {
            configure_test_context(ctx);
            ctx.global_style_mut(|style| {
                style
                    .text_styles
                    .insert(egui::TextStyle::Body, egui::FontId::proportional(font_size));
            });
        })
        .build_ui_state(
            |ui, fixture: &mut AlertFixture| {
                fixture.app.handle_input_shortcuts(ui.ctx());
                fixture.app.draw(ui);
                ui.ctx().output(|output| {
                    for command in &output.commands {
                        if let egui::OutputCommand::CopyText(text) = command {
                            fixture.copied = Some(text.clone());
                        }
                    }
                });
            },
            AlertFixture {
                app,
                commands: incoming,
                copied: None,
            },
        )
}

fn long_error() -> String {
    format!(
        "The operation could not finish.\r\n{}\nFinal cause: permission denied. 日本語 🌱",
        "C:\\a-very-long-path-without-spaces\\日本語\\artifact: nested cause\n".repeat(1_000)
    )
}

fn collect_paint_shapes<'a>(shape: &'a egui::Shape, shapes: &mut Vec<&'a egui::Shape>) {
    if let egui::Shape::Vec(children) = shape {
        for child in children {
            collect_paint_shapes(child, shapes);
        }
    } else {
        shapes.push(shape);
    }
}

fn luminance(color: egui::Color32) -> f32 {
    let linear = egui::Rgba::from(color);
    linear
        .r()
        .mul_add(0.2126, linear.g().mul_add(0.7152, 0.0722 * linear.b()))
}

fn assert_popup_text_contrast(chat: &Harness<'_, AlertFixture>, title: &str) -> io::Result<()> {
    let row = chat.get_by_role_and_label(egui::Role::Button, title).rect();
    let mut shapes = Vec::new();
    for clipped in &chat.output().shapes {
        collect_paint_shapes(&clipped.shape, &mut shapes);
    }
    let mut background = None;
    let mut checked = 0;
    for shape in shapes {
        match shape {
            egui::Shape::Rect(rect)
                if rect.fill.is_opaque() && rect.rect.contains(row.center()) =>
            {
                background = Some(rect.fill);
            }
            egui::Shape::Text(text) if row.contains(text.pos) && text.galley.job.text == title => {
                let background = luminance(background.ok_or_else(|| {
                    io::Error::other("Popup row does not have a painted background")
                })?);
                for section in &text.galley.job.sections {
                    let foreground = text.override_text_color.unwrap_or_else(|| {
                        if section.format.color == egui::Color32::PLACEHOLDER {
                            text.fallback_color
                        } else {
                            section.format.color
                        }
                    });
                    let foreground = luminance(foreground);
                    let contrast =
                        (foreground.max(background) + 0.05) / (foreground.min(background) + 0.05);
                    assert!(contrast >= 4.5, "{title}: rendered contrast was {contrast}");
                    checked += 1;
                }
            }
            _ => {}
        }
    }
    assert!(checked > 0, "Popup row text was actually painted");
    Ok(())
}

#[test]
fn error_reporting_selected_popup_text_has_contrast_when_idle_and_hovered() -> io::Result<()> {
    for theme in [egui::ThemePreference::Light, egui::ThemePreference::Dark] {
        let mut chat = harness([480.0, 480.0], 24.0);
        chat.ctx.set_theme(theme);
        chat.state_mut().app.set_status("Model failed", true);
        chat.state_mut().app.speech_notice = true;
        chat.state_mut().app.speech_status = "Speech failed".into();
        chat.run();
        chat.get_by_role_and_label(egui::Role::ComboBox, "Current alert")
            .click();
        chat.run();
        assert_popup_text_contrast(&chat, "Model needs attention")?;
        chat.get_by_role_and_label(egui::Role::Button, "Model needs attention")
            .hover();
        chat.run();
        assert_popup_text_contrast(&chat, "Model needs attention")?;
    }
    Ok(())
}

#[test]
fn error_reporting_single_alert_omits_selector_and_prioritizes_recovery_in_tab_order() {
    let mut chat = harness([1080.0, 780.0], 16.0);
    chat.state_mut().app.set_status("Model failed", true);
    chat.state_mut().app.model = ModelLifecycle::Retryable;
    chat.run();
    assert!(chat.get_by_label("1 alert").rect().is_positive());
    assert!(
        chat.get_by_label("Model needs attention")
            .rect()
            .is_positive()
    );
    assert!(
        chat.query_by_role_and_label(egui::Role::ComboBox, "Current alert")
            .is_none()
    );
    chat.get_by_role_and_label(egui::Role::Button, "Retry model")
        .focus();
    chat.run();
    chat.key_press(egui::Key::Tab);
    chat.run();
    assert!(
        chat.get_by_role_and_label(egui::Role::Button, "View details")
            .is_focused()
    );
    chat.key_press(egui::Key::Tab);
    chat.run();
    assert!(
        chat.get_by_role_and_label(egui::Role::Button, "Copy error")
            .is_focused()
    );
    assert!(chat.state().commands.try_recv().is_err());
    assert_eq!(chat.state().app.input, "Keep this draft");
}

#[test]
fn error_reporting_popup_rows_wrap_and_fill_the_width_at_minimum_size() {
    let mut chat = harness([480.0, 480.0], 24.0);
    chat.state_mut().app.set_status("Model failed", true);
    chat.state_mut().app.recovery_notice = "Could not save conversations".into();
    chat.run();
    chat.get_by_role_and_label(egui::Role::ComboBox, "Current alert")
        .click();
    chat.run();
    let model = chat
        .get_by_role_and_label(egui::Role::Button, "Model needs attention")
        .rect();
    let recovery = chat
        .get_by_role_and_label(egui::Role::Button, "Conversation saving needs attention")
        .rect();
    let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(480.0, 480.0));
    assert!(viewport.contains_rect(model) && viewport.contains_rect(recovery));
    assert!((model.width() - recovery.width()).abs() < 1.0);
    assert!(recovery.height() > model.height());
}

#[test]
fn error_reporting_shared_panel_selects_errors_and_copies_every_character() {
    for (size, theme) in [
        ([480.0, 480.0], egui::Theme::Light),
        ([1080.0, 780.0], egui::Theme::Dark),
    ] {
        let model = long_error();
        let speech = format!("Speech failed: {}", long_error());
        let mut chat = harness(size, 24.0);
        chat.ctx.set_theme(match theme {
            egui::Theme::Light => egui::ThemePreference::Light,
            egui::Theme::Dark => egui::ThemePreference::Dark,
        });
        chat.state_mut().app.set_status(&model, true);
        chat.state_mut().app.speech_notice = true;
        chat.state_mut().app.speech_status = speech.clone();
        chat.run();
        assert!(chat.get_by_label("2 alerts").rect().is_positive());
        chat.get_by_role_and_label(egui::Role::Button, "Copy error")
            .click();
        chat.run();
        assert_eq!(chat.state().copied.as_deref(), Some(model.as_str()));
        chat.get_by_role_and_label(egui::Role::ComboBox, "Current alert")
            .click();
        chat.run();
        chat.get_by_label("Speech needs attention").click();
        chat.run();
        assert_eq!(
            chat.state().app.alerts.selection,
            Some(error_ui::AlertKind::Speech)
        );
        chat.get_by_role_and_label(egui::Role::Button, "Copy error")
            .click();
        chat.run();
        assert_eq!(chat.state().copied.as_deref(), Some(speech.as_str()));
        chat.get_by_role_and_label(egui::Role::Button, "View details")
            .click();
        chat.run();
        assert!(chat.get_by_label("Error details").rect().is_positive());
        chat.get_by_role_and_label(egui::Role::Button, "Copy full error")
            .click();
        chat.run();
        assert_eq!(chat.state().copied.as_deref(), Some(speech.as_str()));
        chat.get_by_role_and_label(egui::Role::Button, "Close")
            .click();
        chat.run();
        assert_eq!(chat.state().app.alerts.details, None);
        let composer = chat
            .get_by_role_and_label(egui::Role::MultilineTextInput, "Message")
            .rect();
        assert!(egui::Rect::from_min_size(egui::Pos2::ZERO, size.into()).contains_rect(composer));
        assert_eq!(chat.state().app.input, "Keep this draft");
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
fn error_reporting_multiple_failures_preserve_selection_and_audio_actions() {
    let mut chat = harness([480.0, 480.0], 24.0);
    let app = &mut chat.state_mut().app;
    app.fail_startup(&io::Error::other(long_error()));
    app.setup.speech_error = Some(long_error());
    app.speech_notice = true;
    app.speech_status = long_error();
    app.audio_file_error = true;
    app.audio_file_notice = Some(format!("Audio import failed: {}", long_error()));
    app.audio_file_transcript = "Retained partial transcript 日本語 🌱".to_owned();
    app.recovery_notice = format!("Could not save conversations: {}", long_error());
    app.alerts.selection = Some(error_ui::AlertKind::AudioFile);
    chat.run();
    assert!(chat.get_by_label("4 alerts").rect().is_positive());
    assert!(
        chat.get_by_role_and_label(egui::Role::Button, "View transcript")
            .rect()
            .is_positive()
    );
    chat.get_by_role_and_label(egui::Role::Button, "Copy transcript")
        .click();
    chat.run();
    assert_eq!(
        chat.state().copied.as_deref(),
        Some("Retained partial transcript 日本語 🌱")
    );
    // Removing an earlier source must not select a different surviving alert.
    chat.state_mut().app.recovery_notice.clear();
    chat.run();
    assert_eq!(
        chat.state().app.alerts.selection,
        Some(error_ui::AlertKind::AudioFile)
    );
    assert!(chat.get_by_label("3 alerts").rect().is_positive());
    chat.get_by_role_and_label(egui::Role::Button, "Dismiss audio status")
        .click();
    chat.run();
    assert_eq!(
        chat.state().app.alerts.selection,
        Some(error_ui::AlertKind::Setup)
    );
    assert!(chat.state().app.audio_file_notice.is_none());
    assert_eq!(chat.state().app.audio_file_transcript, "");
    assert!(!chat.state().app.audio_file_error);
    assert!(chat.get_by_label("2 alerts").rect().is_positive());
    let composer = chat
        .get_by_role_and_label(egui::Role::MultilineTextInput, "Message")
        .rect();
    assert!(composer.min.y >= 0.0 && composer.max.y <= 480.0);
    assert!(composer.width() > 200.0 && composer.height() > 20.0);
    assert_eq!(chat.state().app.input, "Keep this draft");
}

#[test]
fn error_reporting_sources_suppress_duplicates_and_resolve_to_empty() {
    use error_ui::AlertKind;
    let mut chat = harness([480.0, 480.0], 16.0);
    let app = &mut chat.state_mut().app;
    app.set_status("Model failed", true);
    app.setup.error = Some("Setup failed".into());
    app.setup.speech_error = Some("Speech setup failed".into());
    app.speech_notice = true;
    app.speech_status = "Speech failed".into();
    assert_eq!(
        app.active_alerts(),
        vec![AlertKind::Setup, AlertKind::SpeechSetup]
    );
    app.setup.speech_error = None;
    app.audio_file_error = true;
    app.audio_file_notice = Some(app.speech_status.clone());
    assert_eq!(
        app.active_alerts(),
        vec![AlertKind::Setup, AlertKind::AudioFile]
    );
    app.audio_file_notice = None;
    app.audio_file_error = false;
    app.help = Some(("Speech help", "Check speech setup"));
    assert_eq!(app.active_alerts(), vec![AlertKind::Setup]);
    app.setup.error = None;
    assert_eq!(app.active_alerts(), vec![AlertKind::Model]);
    app.set_status("Ready", false);
    chat.run();
    assert_eq!(chat.state().app.alerts.selection, None);
    assert!(
        chat.query_by_role_and_label(egui::Role::Button, "Copy error")
            .is_none()
    );
}

#[test]
fn error_reporting_details_own_shortcuts_without_stopping_generation() {
    let mut chat = harness([1080.0, 780.0], 16.0);
    chat.state_mut().app.model = ModelLifecycle::Generating;
    chat.state_mut().app.speech_notice = true;
    chat.state_mut().app.speech_status = long_error();
    chat.run();
    chat.get_by_role_and_label(egui::Role::Button, "View details")
        .click();
    chat.run();
    let count = chat.state().app.conversations.len();
    chat.key_press(egui::Key::Enter);
    chat.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::N);
    chat.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::F);
    chat.run();
    assert_eq!(chat.state().app.conversations.len(), count);
    assert!(!chat.state().app.transcript.find_open());
    assert_eq!(chat.state().app.input, "Keep this draft");
    assert!(chat.state().commands.try_recv().is_err());
    chat.key_press(egui::Key::Escape);
    chat.run();
    assert_eq!(chat.state().app.alerts.details, None);
    assert_eq!(chat.state().app.model, ModelLifecycle::Generating);
    assert!(chat.state().commands.try_recv().is_err());
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
        app.setup.speech_error = Some(long_error());
        app.input = "My draft stays available while I resolve this error.".to_owned();
        app.appearance.theme = theme;
        let mut setup = eframe::egui_wgpu::WgpuSetupCreateNew::without_display_handle();
        setup.instance_descriptor.backends = wgpu::Backends::DX12;
        let mut chat = Harness::builder()
            .with_size(size)
            .with_context_setup(configure_test_context)
            .wgpu_setup(eframe::egui_wgpu::WgpuSetup::CreateNew(setup))
            .build_ui_state(|ui, state: &mut State| state.draw(ui), app);
        chat.state().appearance.apply(&chat.ctx);
        chat.run();
        chat.render()
            .map_err(io::Error::other)?
            .save(directory.join(name))
            .map_err(io::Error::other)?;
    }
    Ok(())
}
