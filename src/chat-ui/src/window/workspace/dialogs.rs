//! Conversation dialog views emit decisions; the controller applies mutations.
use super::State;
use crate::window::{RichText, egui};

enum Decision {
    Pending,
    Confirm,
    Cancel,
}

impl State {
    pub(in crate::window) fn conversation_dialogs(&mut self, ctx: &egui::Context) {
        self.rename_dialog(ctx);
        self.delete_dialog(ctx);
    }

    fn rename_dialog(&mut self, ctx: &egui::Context) {
        let Some((id, mut title)) = self.rename_conversation.take() else {
            return;
        };
        match rename_prompt(ctx, &mut title) {
            Decision::Confirm => {
                if let Some(conversation) =
                    self.conversations.iter_mut().find(|value| value.id == id)
                {
                    title.trim().clone_into(&mut conversation.title);
                    conversation.renamed = true;
                    self.recovery_dirty = true;
                }
            }
            Decision::Cancel => {}
            Decision::Pending => self.rename_conversation = Some((id, title)),
        }
    }

    fn delete_dialog(&mut self, ctx: &egui::Context) {
        let Some(id) = self.delete_conversation else {
            return;
        };
        let Some(conversation) = self.conversations.iter().find(|value| value.id == id) else {
            self.delete_conversation = None;
            return;
        };
        match delete_prompt(ctx, &conversation.title, self.can_remove_conversation(id)) {
            Decision::Confirm => {
                self.remove_conversation(id);
                self.delete_conversation = None;
            }
            Decision::Cancel => self.delete_conversation = None,
            Decision::Pending => {}
        }
    }
}

fn rename_prompt(ctx: &egui::Context, title: &mut String) -> Decision {
    let mut decision = Decision::Pending;
    let modal = egui::Modal::new(egui::Id::unique("rename-conversation")).show(ctx, |ui| {
        ui.set_width(340.0);
        ui.heading("Rename conversation");
        ui.add_space(12.0);
        let label = ui.label("Conversation name");
        ui.add(
            egui::TextEdit::singleline(title)
                .id(egui::Id::unique("conversation-name"))
                .char_limit(64)
                .desired_width(f32::INFINITY),
        )
        .labelled_by(label.id);
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!title.trim().is_empty(), egui::Button::new("Save name"))
                .clicked()
            {
                decision = Decision::Confirm;
            }
            if ui.button("Cancel").clicked() {
                decision = Decision::Cancel;
            }
        });
    });
    if matches!(decision, Decision::Pending) && modal.should_close() {
        Decision::Cancel
    } else {
        decision
    }
}

fn delete_prompt(ctx: &egui::Context, title: &str, can_remove: bool) -> Decision {
    let mut decision = Decision::Pending;
    let modal = egui::Modal::new(egui::Id::unique("delete-conversation")).show(ctx, |ui| {
        ui.set_width(340.0);
        ui.heading("Delete conversation?");
        ui.add_space(12.0);
        ui.label(RichText::new(title).strong());
        ui.label("This conversation and its unsent draft will be removed. This cannot be undone.");
        ui.add_space(16.0);
        ui.horizontal(|ui| {
            if ui.button("Keep conversation").clicked() {
                decision = Decision::Cancel;
            }
            if ui
                .add_enabled(
                    can_remove,
                    egui::Button::new(
                        RichText::new("Delete conversation").color(ui.visuals().error_fg_color),
                    ),
                )
                .clicked()
            {
                decision = Decision::Confirm;
            }
        });
    });
    if matches!(decision, Decision::Pending) && modal.should_close() {
        Decision::Cancel
    } else {
        decision
    }
}
