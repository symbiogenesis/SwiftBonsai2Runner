//! Conversation-list orchestration keeps workspace actions outside row rendering.
use super::{Conversation, State};
use crate::window::{Transcript, egui, history_search};

mod controls;
use controls::{Action, Availability, Row};

impl State {
    pub(in crate::window) fn conversation_list(&mut self, ui: &mut egui::Ui) {
        let field = controls::search_field(ui, &mut self.conversation_search);
        if field.clicked() || field.changed() {
            self.focus_composer = false;
        }
        if self.conversation_search.update(
            &mut self.conversations,
            self.active_conversation,
            &self.transcript,
        ) {
            self.transcript
                .sync_find_query(self.conversation_search.text.trim());
            // Drawer input is drawn after the disabled workspace. Refresh Find
            // next frame without taking focus from the sidebar.
            ui.ctx().request_repaint();
        }
        controls::list_heading(
            ui,
            self.conversation_search.rows.len(),
            self.conversations.len(),
            self.conversation_search.filtered(),
        );
        if self.conversation_search.rows.is_empty() {
            controls::empty_list(ui);
            return;
        }
        let mut action = None;
        self.conversation_scroll(ui).show_rows(
            ui,
            controls::ROW_HEIGHT,
            self.conversation_search.rows.len(),
            |ui, rows| {
                let width = ui.available_width();
                for row in rows {
                    let conversation = &self.conversations[self.conversation_search.rows[row]];
                    let transcript = if conversation.id == self.active_conversation {
                        Some(&self.transcript)
                    } else {
                        conversation
                            .saved
                            .as_ref()
                            .map(|snapshot| &snapshot.transcript)
                    };
                    let view = Row {
                        conversation,
                        active: conversation.id == self.active_conversation,
                        reveal: self.focus_composer,
                        copy_available: transcript
                            .is_some_and(|transcript| !transcript.messages.is_empty()),
                        availability: Availability {
                            select: self.model.workspace_available(),
                            rename: !self.model.busy(),
                            delete: self.can_remove_conversation(conversation.id),
                        },
                    };
                    if let Some(selected) = view.show(ui, width) {
                        action = Some(selected);
                    }
                }
            },
        );
        if let Some(action) = action {
            self.apply_conversation_action(ui.ctx(), action);
        }
    }

    fn conversation_scroll(&mut self, ui: &egui::Ui) -> egui::ScrollArea {
        let scroll = egui::ScrollArea::vertical()
            .id_salt("conversation-list")
            .auto_shrink([false, false])
            .min_scrolled_height(0.0);
        // A changed query starts at its first result. Creation/selection outside
        // search must also reveal rows previously outside the viewport.
        let reset = self.conversation_search.take_scroll_reset();
        if reset && !self.focus_composer {
            return scroll.vertical_scroll_offset(0.0);
        }
        if self.focus_composer
            && let Some(row) = self
                .conversation_search
                .rows
                .iter()
                .position(|&index| self.conversations[index].id == self.active_conversation)
        {
            let offset =
                history_search::row_offset(row, controls::ROW_HEIGHT + ui.spacing().item_spacing.y);
            return scroll
                .vertical_scroll_offset(ui.available_height().mul_add(-0.5, offset).max(0.0));
        }
        if reset {
            return scroll.vertical_scroll_offset(0.0);
        }
        scroll
    }

    fn conversation_transcript(&self, id: u64) -> Option<&Transcript> {
        if id == self.active_conversation {
            Some(&self.transcript)
        } else {
            self.conversations
                .iter()
                .find(|conversation| conversation.id == id)
                .and_then(|conversation| conversation.saved.as_ref())
                .map(|snapshot| &snapshot.transcript)
        }
    }

    fn apply_conversation_action(&mut self, ctx: &egui::Context, action: Action) {
        match action {
            Action::Select(id) => {
                self.switch_conversation(id);
                self.sidebar_open = false;
                self.focus_composer = true;
                // Reselecting a result also resets Find to the sidebar filter.
                // Local Find edits never rewrite that filter.
                let query = self.conversation_search.text.trim();
                if self.active_conversation == id && !query.is_empty() {
                    self.transcript.find_query(query);
                    self.focus_composer = false;
                }
            }
            Action::Rename(id) => {
                if let Some(conversation) = self.conversations.iter().find(|value| value.id == id) {
                    self.rename_conversation = Some((id, conversation.title.clone()));
                    ctx.memory_mut(|memory| {
                        memory.request_focus(egui::Id::unique("conversation-name"));
                    });
                }
            }
            Action::Copy(id) => {
                if let Some(transcript) = self.conversation_transcript(id) {
                    ctx.copy_text(transcript.plain_text());
                }
            }
            Action::Delete(id) => self.delete_conversation = Some(id),
        }
    }
}
