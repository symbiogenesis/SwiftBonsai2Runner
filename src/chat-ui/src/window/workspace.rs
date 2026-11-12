//! Bounded conversation workspace. Locally identified interrupted replies are
//! retained as context without replaying their original requests.
use super::{Command, RichText, State, Transcript, client, egui, history_search};

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "Independent response choices and transcript status are preserved for each conversation."
)]
pub(super) struct ConversationSnapshot {
    pub(super) transcript: Transcript,
    #[serde(deserialize_with = "super::persistence::deserialize_history")]
    pub(super) committed: Vec<client::Message>,
    #[serde(skip)]
    pub(super) context_history: Option<usize>,
    pub(super) input: String,
    pub(super) last_answer: String,
    pub(super) retained_draft: String,
    pub(super) answer_complete: bool,
    pub(super) extra_effort: bool,
    #[serde(default)]
    pub(super) enable_thinking: bool,
    #[serde(default)]
    pub(super) streaming_read_aloud: bool,
    pub(super) output_budget: usize,
    #[serde(default = "client::default_abliteration_scale")]
    pub(super) abliteration_scale: f32,
    pub(super) status: String,
    pub(super) warning: bool,
    #[serde(skip)]
    pub(super) stage: Option<&'static str>,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Conversation {
    pub(super) id: u64,
    pub(super) title: String,
    pub(super) renamed: bool,
    pub(super) saved: Option<ConversationSnapshot>,
    #[serde(skip)]
    pub(super) search: history_search::MatchCache,
}

impl Conversation {
    pub(super) fn new(id: u64) -> Self {
        Self {
            id,
            title: "New conversation".to_owned(),
            renamed: false,
            saved: None,
            search: history_search::MatchCache::default(),
        }
    }
}

enum ConversationAction {
    Select(u64),
    Rename(u64, String),
    Delete(u64),
}

impl State {
    pub(super) fn update_worker_conversation(&mut self, command: Command) {
        // Startup and generation consume history commands without applying them.
        // Coalesce navigation until the worker can accept the selected history.
        self.restore_history = true;
        if self.preview || self.model != super::ModelLifecycle::Ready {
            return;
        }
        if self.commands.send(command).is_ok() {
            self.restore_history = false;
        } else {
            // A lost worker must not prevent local conversation management.
            self.fail("The model worker stopped. Close and restart the app.");
        }
    }

    pub(super) fn take_conversation(&mut self) -> ConversationSnapshot {
        ConversationSnapshot {
            transcript: std::mem::take(&mut self.transcript),
            committed: std::mem::take(&mut self.committed),
            context_history: self.context_history.take(),
            input: std::mem::take(&mut self.input),
            last_answer: std::mem::take(&mut self.last_answer),
            retained_draft: std::mem::take(&mut self.retained_draft),
            answer_complete: self.answer_complete,
            extra_effort: self.extra_effort,
            enable_thinking: self.enable_thinking,
            streaming_read_aloud: self.streaming_read_aloud,
            output_budget: self.output_budget,
            abliteration_scale: self.abliteration_scale,
            status: if self.model.ready() {
                self.status.clone()
            } else {
                "Ready when you are.".to_owned()
            },
            warning: self.model.ready() && self.warning,
            stage: self.stage,
        }
    }

    pub(super) fn save_conversation(&mut self) {
        let snapshot = self.take_conversation();
        if let Some(conversation) = self
            .conversations
            .iter_mut()
            .find(|conversation| conversation.id == self.active_conversation)
        {
            conversation.saved = Some(snapshot);
        }
    }

    pub(super) fn clear_conversation(&mut self) {
        self.transcript.clear();
        self.committed.clear();
        self.context_history = None;
        self.input.clear();
        self.last_answer.clear();
        self.retained_draft.clear();
        self.stage = None;
        self.answer_complete = false;
        self.extra_effort = false;
        self.enable_thinking = false;
        self.streaming_read_aloud = false;
        self.output_budget = 3;
        self.abliteration_scale = client::DEFAULT_ABLITERATION_SCALE;
        self.place_composer_cursor(0);
        self.focus_composer = true;
    }

    pub(super) fn load_conversation(&mut self, snapshot: ConversationSnapshot) {
        self.transcript = snapshot.transcript;
        self.committed = snapshot.committed;
        self.context_history = snapshot.context_history;
        self.input = snapshot.input;
        self.last_answer = snapshot.last_answer;
        self.retained_draft = snapshot.retained_draft;
        self.answer_complete = snapshot.answer_complete;
        self.extra_effort = snapshot.extra_effort;
        self.enable_thinking = snapshot.enable_thinking;
        self.streaming_read_aloud = snapshot.streaming_read_aloud;
        self.output_budget = snapshot.output_budget;
        self.abliteration_scale = snapshot.abliteration_scale;
        // Loading/failure status belongs to the model, not a saved transcript.
        if self.model.ready() {
            self.status = snapshot.status;
            self.warning = snapshot.warning;
        }
        self.stage = snapshot.stage;
        self.place_composer_cursor(self.input.chars().count());
        self.focus_composer = true;
    }

    // Event routing swaps only conversation content. The actual selection, Find
    // state in its transcript, and live composer cursor/focus stay with the user.
    pub(super) fn with_request_conversation(&mut self, action: impl FnOnce(&mut Self)) {
        if self.routing_request {
            action(self);
            return;
        }
        let Some(owner) = self
            .request_conversation
            .filter(|id| *id != self.active_conversation)
        else {
            action(self);
            return;
        };
        let Some(index) = self
            .conversations
            .iter()
            .position(|value| value.id == owner)
        else {
            action(self);
            return;
        };
        let Some(mut saved) = self.conversations[index].saved.take() else {
            return;
        };
        let cursor = (
            self.composer_cursor,
            self.pending_cursor,
            self.focus_composer,
        );
        self.swap_conversation(&mut saved);
        let previous = (self.model, self.status.clone(), self.warning);
        self.routing_request = true;
        action(self);
        self.routing_request = false;
        // Late rejections and completion races can update a hidden transcript
        // without changing the global stop/restart status currently on screen.
        let changed =
            previous.0 != self.model || previous.1 != self.status || previous.2 != self.warning;
        let global_status =
            (!self.model.ready() && changed).then(|| (self.status.clone(), self.warning));
        self.swap_conversation(&mut saved);
        self.conversations[index].saved = Some(saved);
        (
            self.composer_cursor,
            self.pending_cursor,
            self.focus_composer,
        ) = cursor;
        if let Some((status, warning)) = global_status {
            self.status = status;
            self.warning = warning;
        }
    }

    const fn swap_conversation(&mut self, saved: &mut ConversationSnapshot) {
        std::mem::swap(&mut self.transcript, &mut saved.transcript);
        std::mem::swap(&mut self.committed, &mut saved.committed);
        std::mem::swap(&mut self.context_history, &mut saved.context_history);
        std::mem::swap(&mut self.input, &mut saved.input);
        std::mem::swap(&mut self.last_answer, &mut saved.last_answer);
        std::mem::swap(&mut self.retained_draft, &mut saved.retained_draft);
        std::mem::swap(&mut self.answer_complete, &mut saved.answer_complete);
        std::mem::swap(&mut self.extra_effort, &mut saved.extra_effort);
        std::mem::swap(&mut self.enable_thinking, &mut saved.enable_thinking);
        std::mem::swap(
            &mut self.streaming_read_aloud,
            &mut saved.streaming_read_aloud,
        );
        std::mem::swap(&mut self.output_budget, &mut saved.output_budget);
        std::mem::swap(&mut self.abliteration_scale, &mut saved.abliteration_scale);
        std::mem::swap(&mut self.status, &mut saved.status);
        std::mem::swap(&mut self.warning, &mut saved.warning);
        std::mem::swap(&mut self.stage, &mut saved.stage);
    }

    pub(super) fn restore_selected_history(&mut self) {
        if self.restore_history && self.model == super::ModelLifecycle::Ready {
            self.update_worker_conversation(Command::RestoreChat(self.committed.clone()));
        }
    }

    pub(super) fn generation_status(&self) -> String {
        if let Some(owner) = self.request_conversation
            && owner != self.active_conversation
            && let Some(conversation) = self.conversations.iter().find(|value| value.id == owner)
        {
            return format!(
                "Responding in ‘{}’. You can draft here while you wait.",
                conversation.title
            );
        }
        format!(
            "{} You can draft your next message while you wait.",
            self.status
        )
    }

    fn can_remove_conversation(&self, id: u64) -> bool {
        self.model.workspace_available()
            && !(self.model.busy() && self.request_conversation == Some(id))
    }

    pub(super) fn switch_conversation(&mut self, id: u64) {
        if !self.model.workspace_available() || id == self.active_conversation {
            return;
        }
        let Some(index) = self
            .conversations
            .iter()
            .position(|conversation| conversation.id == id)
        else {
            return;
        };
        let Some(saved) = self.conversations[index].saved.as_ref() else {
            return;
        };
        self.update_worker_conversation(Command::RestoreChat(saved.committed.clone()));
        self.stop_speech();
        self.save_conversation();
        self.conversation_notice = None;
        if let Some(snapshot) = self.conversations[index].saved.take() {
            self.active_conversation = id;
            self.load_conversation(snapshot);
            self.recovery_dirty = true;
        }
    }

    pub(super) fn remove_conversation(&mut self, id: u64) {
        if !self.can_remove_conversation(id) {
            return;
        }
        let Some(index) = self
            .conversations
            .iter()
            .position(|conversation| conversation.id == id)
        else {
            return;
        };
        if id == self.active_conversation {
            if let Some(next) = self
                .conversations
                .iter()
                .find(|conversation| conversation.id != id)
                .map(|conversation| conversation.id)
            {
                self.switch_conversation(next);
                if self.active_conversation == id {
                    return;
                }
            } else {
                self.update_worker_conversation(Command::NewChat);
                self.stop_speech();
                self.clear_conversation();
                self.active_conversation = self.next_conversation;
                self.next_conversation = self.next_conversation.saturating_add(1);
                self.conversations
                    .push(Conversation::new(self.active_conversation));
                if self.model.ready() {
                    self.set_status("Ready for a new conversation.", false);
                }
            }
        }
        self.conversations.remove(index);
        self.recovery_dirty = true;
        self.conversation_notice = Some("Conversation deleted.");
    }

    pub(super) fn conversation_list(&mut self, ui: &mut egui::Ui) {
        self.conversation_search_field(ui);
        self.conversation_search.update(
            &mut self.conversations,
            self.active_conversation,
            &self.transcript,
        );
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("CONVERSATIONS").small().strong().weak());
            ui.label(
                RichText::new(if self.conversation_search.filtered() {
                    format!(
                        "{} / {}",
                        self.conversation_search.rows.len(),
                        self.conversations.len()
                    )
                } else {
                    self.conversations.len().to_string()
                })
                .small()
                .weak(),
            );
        });
        ui.add_space(8.0);
        if self.conversation_search.rows.is_empty() {
            ui.add_space(12.0);
            ui.label(RichText::new("No conversations found").strong());
            ui.label(
                RichText::new("Try another word from a title or message.")
                    .small()
                    .weak(),
            );
            return;
        }
        let mut action = None;
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt("conversation-list")
            .auto_shrink([false, false])
            .min_scrolled_height(0.0);
        // A changed query starts at its first result. Creation/selection outside
        // search must also reveal rows that were previously outside the viewport.
        let reset = self.conversation_search.take_scroll_reset();
        if reset && !self.focus_composer {
            scroll = scroll.vertical_scroll_offset(0.0);
        } else if self.focus_composer
            && let Some(row) = self
                .conversation_search
                .rows
                .iter()
                .position(|&index| self.conversations[index].id == self.active_conversation)
        {
            let offset = history_search::row_offset(row, 40.0 + ui.spacing().item_spacing.y);
            scroll =
                scroll.vertical_scroll_offset(ui.available_height().mul_add(-0.5, offset).max(0.0));
        } else if reset {
            scroll = scroll.vertical_scroll_offset(0.0);
        }
        scroll.show_rows(ui, 40.0, self.conversation_search.rows.len(), |ui, rows| {
            let row_width = ui.available_width();
            for row in rows {
                let conversation = &self.conversations[self.conversation_search.rows[row]];
                // Stable identities survive row removal and duplicate titles.
                ui.push_id(conversation.id, |ui| {
                    if let Some(selected) = self.conversation_row(ui, conversation, row_width) {
                        action = Some(selected);
                    }
                });
            }
        });
        match action {
            Some(ConversationAction::Select(id)) => {
                self.switch_conversation(id);
                self.sidebar_open = false;
                self.focus_composer = true;
            }
            Some(ConversationAction::Rename(id, title)) => {
                self.rename_conversation = Some((id, title));
                ui.ctx().memory_mut(|memory| {
                    memory.request_focus(egui::Id::unique("conversation-name"));
                });
            }
            Some(ConversationAction::Delete(id)) => self.delete_conversation = Some(id),
            None => {}
        }
    }

    fn conversation_search_field(&mut self, ui: &mut egui::Ui) {
        egui::Frame::new()
            .fill(ui.visuals().extreme_bg_color)
            .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
            .corner_radius(8)
            .inner_margin(egui::Margin::symmetric(10, 6))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                ui.horizontal(|ui| {
                    let (icon, _) =
                        ui.allocate_exact_size(egui::vec2(16.0, 24.0), egui::Sense::hover());
                    let center = icon.center();
                    let stroke = egui::Stroke::new(1.5, ui.visuals().weak_text_color());
                    ui.painter()
                        .circle_stroke(center - egui::vec2(2.0, 2.0), 4.5, stroke);
                    ui.painter().line_segment(
                        [center + egui::vec2(1.5, 1.5), center + egui::vec2(6.0, 6.0)],
                        stroke,
                    );
                    let clear_width = if self.conversation_search.text.is_empty() {
                        0.0
                    } else {
                        30.0
                    };
                    let field = ui
                        .add(
                            egui::TextEdit::singleline(&mut self.conversation_search.text)
                                .id(history_search::field_id())
                                .desired_width((ui.available_width() - clear_width).max(24.0))
                                .frame(egui::Frame::NONE)
                                .return_key(None),
                        )
                        .accessible_name("Search conversations");
                    if field.clicked() || field.changed() {
                        self.focus_composer = false;
                    }
                    if !self.conversation_search.text.is_empty()
                        && ui
                            .add(
                                egui::Button::new("×")
                                    .min_size(egui::Vec2::splat(24.0))
                                    .frame_when_inactive(false),
                            )
                            .accessible_name("Clear conversation search")
                            .on_hover_text("Clear search · Escape")
                            .clicked()
                    {
                        self.conversation_search.clear();
                        field.request_focus();
                    }
                });
            });
    }

    fn conversation_row(
        &self,
        ui: &mut egui::Ui,
        conversation: &Conversation,
        row_width: f32,
    ) -> Option<ConversationAction> {
        let mut action = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let width = (row_width - 38.0).max(40.0);
            let active = conversation.id == self.active_conversation;
            let title = RichText::new(&conversation.title).color(if active {
                ui.visuals().selection.stroke.color
            } else {
                ui.visuals().text_color()
            });
            let response = ui
                .add_enabled_ui(self.model.workspace_available(), |ui| {
                    ui.add_sized(
                        [width, 40.0],
                        egui::Button::selectable(active, title)
                            .right_text(())
                            .truncate()
                            .corner_radius(8),
                    )
                })
                .inner;
            // Only scroll when the composer can consume this focus request in
            // the same frame; pending focus during generation must not repaint forever.
            if active && self.focus_composer {
                response.scroll_to_me(Some(egui::Align::Center));
            }
            if response.on_hover_text(&conversation.title).clicked() {
                action = Some(ConversationAction::Select(conversation.id));
            }
            let (response, _) = egui::containers::menu::MenuButton::from_button(
                egui::Button::new("⋯")
                    .min_size(egui::vec2(34.0, 40.0))
                    .frame_when_inactive(false),
            )
            .ui(ui, |ui| {
                action = self.conversation_menu(ui, conversation);
            });
            let label = format!("Actions for {}", conversation.title);
            response.accessible_name(&label).on_hover_text(label);
        });
        action
    }

    fn conversation_menu(
        &self,
        ui: &mut egui::Ui,
        conversation: &Conversation,
    ) -> Option<ConversationAction> {
        ui.set_min_width(156.0);
        if ui
            .add_enabled(!self.model.busy(), egui::Button::new("Rename"))
            .clicked()
        {
            ui.close();
            return Some(ConversationAction::Rename(
                conversation.id,
                conversation.title.clone(),
            ));
        }
        let transcript = if conversation.id == self.active_conversation {
            Some(&self.transcript)
        } else {
            conversation.saved.as_ref().map(|saved| &saved.transcript)
        };
        if ui
            .add_enabled(
                transcript.is_some_and(|value| !value.messages.is_empty()),
                egui::Button::new("Copy conversation"),
            )
            .clicked()
            && let Some(transcript) = transcript
        {
            ui.ctx().copy_text(transcript.plain_text());
            ui.close();
        }
        ui.separator();
        if ui
            .add_enabled(
                self.can_remove_conversation(conversation.id),
                egui::Button::new(RichText::new("Delete").color(ui.visuals().error_fg_color)),
            )
            .clicked()
        {
            ui.close();
            return Some(ConversationAction::Delete(conversation.id));
        }
        None
    }

    pub(super) fn conversation_dialogs(&mut self, ctx: &egui::Context) {
        self.rename_dialog(ctx);
        self.delete_dialog(ctx);
    }

    fn rename_dialog(&mut self, ctx: &egui::Context) {
        if let Some((id, mut title)) = self.rename_conversation.take() {
            let mut done = false;
            let modal = egui::Modal::new(egui::Id::unique("rename-conversation")).show(ctx, |ui| {
                ui.set_width(340.0);
                ui.heading("Rename conversation");
                ui.add_space(12.0);
                let label = ui.label("Conversation name");
                ui.add(
                    egui::TextEdit::singleline(&mut title)
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
                        if let Some(conversation) = self
                            .conversations
                            .iter_mut()
                            .find(|conversation| conversation.id == id)
                        {
                            title.trim().clone_into(&mut conversation.title);
                            conversation.renamed = true;
                            self.recovery_dirty = true;
                        }
                        done = true;
                    }
                    if ui.button("Cancel").clicked() {
                        done = true;
                    }
                });
            });
            if !modal.should_close() && !done {
                self.rename_conversation = Some((id, title));
            }
        }
    }

    fn delete_dialog(&mut self, ctx: &egui::Context) {
        if let Some(id) = self.delete_conversation {
            let Some(conversation) = self.conversations.iter().find(|value| value.id == id) else {
                self.delete_conversation = None;
                return;
            };
            let title = conversation.title.clone();
            let mut confirmed = false;
            let mut cancelled = false;
            let modal = egui::Modal::new(egui::Id::unique("delete-conversation"))
                .show(ctx, |ui| {
                    ui.set_width(340.0);
                    ui.heading("Delete conversation?");
                    ui.add_space(12.0);
                    ui.label(RichText::new(&title).strong());
                    ui.label("This conversation and its unsent draft will be removed. This cannot be undone.");
                    ui.add_space(16.0);
                    ui.horizontal(|ui| {
                        cancelled = ui.button("Keep conversation").clicked();
                        confirmed = ui
                            .add_enabled(
                                self.can_remove_conversation(id),
                                egui::Button::new(RichText::new("Delete conversation").color(ui.visuals().error_fg_color)),
                            )
                            .clicked();
                    });
                });
            if confirmed {
                self.remove_conversation(id);
            }
            if modal.should_close() || confirmed || cancelled {
                self.delete_conversation = None;
            }
        }
    }
}
