//! Explicit iteration actions stay separate from selectable model text.
use super::{
    MESSAGE_ACTION_SIZE, Message, MessageControls, RichText, Role, TranscriptAction, egui,
};
use crate::window::{controls, iteration::MessageAction};

impl Message {
    pub(super) fn header(
        &self,
        ui: &mut egui::Ui,
        controls: Option<&MessageControls<'_>>,
        speaking_message: Option<u64>,
        speech_enabled: bool,
    ) -> Option<TranscriptAction> {
        let mut action = None;
        ui.horizontal(|ui| {
            ui.set_min_height(MESSAGE_ACTION_SIZE);
            let (avatar, name) = if self.role == Role::User {
                ("●", "You")
            } else {
                ("B", "Swift Bonsai")
            };
            ui.label(
                RichText::new(avatar)
                    .color(ui.visuals().selection.bg_fill)
                    .size(18.0),
            );
            ui.label(RichText::new(name).strong().size(14.0));
            if self.interrupted {
                ui.label(RichText::new("Interrupted").small().weak())
                    .on_hover_text(
                        "This partial response is included in the conversation context.",
                    );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(controls) = controls {
                    action = self
                        .message_controls(ui, controls)
                        .map(TranscriptAction::Message);
                }
                if self.role == Role::Assistant
                    && self.speaker(
                        ui,
                        speaking_message,
                        speech_enabled,
                        controls.is_some_and(|controls| {
                            (controls.speech_loading == Some(self.id))
                                || (!speech_enabled
                                    && self.can_speak()
                                    && controls.speech_preparing)
                        }),
                    )
                {
                    action = Some(TranscriptAction::ReadAloud(self.id));
                }
                if ui
                    .add(
                        controls::quiet_button("Copy")
                            .min_size(egui::vec2(0.0, MESSAGE_ACTION_SIZE)),
                    )
                    .on_hover_text("Copy this message")
                    .clicked()
                {
                    ui.ctx().copy_text(self.body.clone());
                }
            });
        });
        action
    }

    pub(super) fn message_controls(
        &self,
        ui: &mut egui::Ui,
        controls: &MessageControls<'_>,
    ) -> Option<MessageAction> {
        let (response, menu) = egui::containers::menu::MenuButton::from_button(
            controls::quiet_button("⋯").min_size(egui::Vec2::splat(MESSAGE_ACTION_SIZE)),
        )
        .ui(ui, |ui| self.iteration_menu(ui, controls));
        let name = format!("Actions for message {}", self.id);
        response
            .accessible_name(&name)
            .on_hover_text("Message actions");
        menu.and_then(|response| response.inner)
    }

    fn iteration_menu(
        &self,
        ui: &mut egui::Ui,
        controls: &MessageControls<'_>,
    ) -> Option<MessageAction> {
        let mut action = None;
        let options =
            self.iteration_options(controls.history, controls.can_send, controls.can_branch);
        let mut item = |enabled, label, help, selected| {
            if ui
                .add_enabled(enabled, egui::Button::new(label))
                .on_hover_text(help)
                .clicked()
            {
                action = Some(selected);
                ui.close();
            }
        };
        if self.role == Role::User {
            item(
                options.edit,
                "Edit and resend",
                "Edit this prompt in a new branch. Keeps the original conversation and draft.",
                MessageAction::Edit(self.id),
            );
        } else {
            item(
                options.regenerate,
                "Regenerate",
                "Generate another answer in a new branch using the original prompt.",
                MessageAction::Regenerate(self.id),
            );
            item(
                options.continue_response,
                "Continue response",
                "Ask Swift to continue the latest answer. Keeps your unsent draft.",
                MessageAction::Continue(self.id),
            );
        }
        item(
            options.branch,
            "Branch from here",
            "Create a conversation from this point without sending a message.",
            MessageAction::Branch(self.id),
        );
        if !controls.can_branch {
            ui.separator();
            ui.label("Branching needs a free conversation slot and an available workspace.");
        } else if !controls.can_send {
            ui.separator();
            ui.label("Sending is available when the model is ready.");
        } else if !options.edit
            && !options.regenerate
            && !options.branch
            && !options.continue_response
        {
            ui.separator();
            ui.label("These actions require a message included in Swift's conversation context.");
        }
        action
    }
}
