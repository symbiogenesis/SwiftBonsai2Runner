//! Explicit iteration actions stay separate from selectable model text.
use super::{Message, MessageControls, Role, egui};
use crate::window::iteration::MessageAction;

impl Message {
    pub(super) fn message_controls(
        &self,
        ui: &mut egui::Ui,
        controls: &MessageControls<'_>,
    ) -> Option<MessageAction> {
        let mut action = None;
        let (response, _) = egui::containers::menu::MenuButton::from_button(
            egui::Button::new("⋯").frame_when_inactive(false),
        )
        .ui(ui, |ui| {
            let options = self.iteration_options(
                controls.history,
                controls.can_send,
                controls.can_branch,
            );
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
                ui.label("These actions require a completed message included in Swift's conversation context.");
            }
        });
        let name = format!("Actions for message {}", self.id);
        response
            .accessible_name(&name)
            .on_hover_text("Message actions");
        action
    }
}
