//! Explicit source selection adds reviewed excerpts to a draft without sending it.

use super::{INPUT_LIMIT, RichText, State, audio_files, egui};
use crate::scene_packet::{MAX_SOURCE_BYTES, Packet, Source};
use std::{ops::Range, thread::JoinHandle, time::Duration};

#[derive(Default)]
pub(super) struct View {
    dialog: Option<Dialog>,
    task: Option<Task>,
}

struct Dialog {
    conversation: u64,
    title: String,
    version: String,
    source: SourceText,
    query: String,
    packet: Option<Packet>,
    selected: Vec<bool>,
    notice: Option<&'static str>,
    focus: bool,
    editor_ids: [Option<egui::Id>; 4],
}

struct Task {
    conversation: u64,
    worker: JoinHandle<Result<Packet, &'static str>>,
}

// Reject an oversized insertion in full instead of silently truncating a source.
// egui's ordinary character limit alone cannot enforce a UTF-8 byte limit.
#[derive(Default)]
struct SourceText {
    text: String,
    rejected: bool,
    deleted: Option<DeletedText>,
}

struct DeletedText {
    index: egui::text::CharIndex,
    text: String,
}

impl egui::TextBuffer for SourceText {
    fn type_id(&self) -> std::any::TypeId {
        std::any::TypeId::of::<Self>()
    }

    fn is_mutable(&self) -> bool {
        true
    }

    fn as_str(&self) -> &str {
        &self.text
    }

    fn insert_text(&mut self, text: &str, char_index: egui::text::CharIndex) -> usize {
        if text.len() > MAX_SOURCE_BYTES.saturating_sub(self.text.len()) {
            self.rejected = true;
            // TextEdit deletes a selection before inserting replacement text.
            // Roll that deletion back when rejecting an oversized replacement.
            if let Some(deleted) = self.deleted.take() {
                let _restored =
                    egui::TextBuffer::insert_text(&mut self.text, &deleted.text, deleted.index);
            }
            return 0;
        }
        self.deleted = None;
        egui::TextBuffer::insert_text(&mut self.text, text, char_index)
    }

    fn delete_char_range(&mut self, char_range: Range<egui::text::CharIndex>) {
        self.deleted = Some(DeletedText {
            index: char_range.start,
            text: egui::TextBuffer::char_range(&self.text, char_range.clone()).to_owned(),
        });
        egui::TextBuffer::delete_char_range(&mut self.text, char_range);
    }

    fn replace_with(&mut self, text: &str) {
        if text.len() > MAX_SOURCE_BYTES {
            self.rejected = true;
        } else {
            self.deleted = None;
            text.clone_into(&mut self.text);
        }
    }
}

impl Dialog {
    fn new(conversation: u64) -> Self {
        Self {
            conversation,
            title: String::new(),
            version: String::new(),
            source: SourceText::default(),
            query: String::new(),
            packet: None,
            selected: Vec::new(),
            notice: None,
            focus: true,
            editor_ids: [None; 4],
        }
    }

    fn source_fields(&mut self, ui: &mut egui::Ui, enabled: bool) {
        let changed = ui
            .add_enabled_ui(enabled, |ui| {
                let title = ui.add(
                    egui::TextEdit::singleline(&mut self.title)
                        .id_salt(("scene-title", self.conversation))
                        .hint_text("Source title")
                        .char_limit(160)
                        .desired_width(f32::INFINITY),
                )
                .accessible_name("Source title");
                self.editor_ids[0] = Some(title.id);
                if self.focus {
                    title.request_focus();
                    self.focus = false;
                }
                let version = ui.add(
                    egui::TextEdit::singleline(&mut self.version)
                        .id_salt(("scene-version", self.conversation))
                        .hint_text("Version or revision, for example chapter 3 draft 2")
                        .char_limit(120)
                        .desired_width(f32::INFINITY),
                )
                .accessible_name("Source version");
                self.editor_ids[1] = Some(version.id);
                ui.label(RichText::new("Paste a manuscript or project-bible excerpt (up to 1 MiB). Nothing is read from disk or sent while searching.").small().weak());
                let source = egui::ScrollArea::vertical()
                    .id_salt("scene-source-scroll")
                    .max_height(140.0)
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.source)
                                .id_salt(("scene-source", self.conversation))
                                .hint_text("Paste source text")
                                .desired_rows(5)
                                .desired_width(f32::INFINITY),
                        )
                        .accessible_name("Source text")
                    })
                    .inner;
                self.editor_ids[2] = Some(source.id);
                // A plain delete is complete after this editor frame. Its bytes
                // are retained only long enough to roll back a paired insertion.
                self.source.deleted = None;
                let query = ui.add(
                    egui::TextEdit::singleline(&mut self.query)
                        .id_salt(("scene-query", self.conversation))
                        .hint_text("Find names, places, events or constraints")
                        .char_limit(1_024)
                        .desired_width(f32::INFINITY),
                )
                .accessible_name("Passage search");
                self.editor_ids[3] = Some(query.id);
                title.changed() || version.changed() || source.changed() || query.changed()
            })
            .inner;
        if changed {
            self.packet = None;
            self.selected.clear();
            self.notice = None;
        }
        if std::mem::take(&mut self.source.rejected) {
            self.notice =
                Some("That insertion exceeds the 1 MiB source limit. Paste a smaller passage.");
        }
    }

    fn results(&mut self, ui: &mut egui::Ui) {
        let Some(packet) = &self.packet else {
            return;
        };
        if packet.excerpts.is_empty() {
            ui.label("No matching passages. Try different words from the source.");
            return;
        }
        ui.separator();
        ui.label(RichText::new("Choose exact passages to include").strong());
        ui.label(format!("{} · {}", packet.title, packet.version));
        ui.label(
            RichText::new(format!("Source SHA-256: {}", packet.sha256))
                .small()
                .weak(),
        );
        for (index, excerpt) in packet.excerpts.iter().enumerate() {
            if let Some(selected) = self.selected.get_mut(index) {
                ui.checkbox(
                    selected,
                    format!(
                        "Passage {} · lines {}–{}",
                        index + 1,
                        excerpt.start_line,
                        excerpt.end_line
                    ),
                );
            }
            ui.add(egui::Label::new(&excerpt.text).wrap().selectable(true));
            ui.add_space(8.0);
        }
    }

    fn render(&self) -> Result<String, &'static str> {
        let selected: Vec<_> = self
            .selected
            .iter()
            .enumerate()
            .filter_map(|(index, selected)| selected.then_some(index))
            .collect();
        self.packet
            .as_ref()
            .ok_or("Find passages and choose excerpts first.")?
            .render(&selected)
    }

    fn set_packet(&mut self, packet: Packet) {
        self.selected = vec![false; packet.excerpts.len()];
        self.packet = Some(packet);
    }

    fn clear_editor_history(&self, ctx: &egui::Context) {
        for id in self.editor_ids.into_iter().flatten() {
            if let Some(mut state) = egui::TextEdit::load_state(ctx, id) {
                state.clear_undoer();
                state.store(ctx, id);
            }
            ctx.data_mut(|data| data.remove::<egui::text_edit::TextEditState>(id));
        }
    }
}

impl View {
    pub(super) const fn is_open(&self) -> bool {
        self.dialog.is_some()
    }

    fn poll(&mut self, ctx: &egui::Context) {
        if self
            .task
            .as_ref()
            .is_some_and(|task| task.worker.is_finished())
            && let Some(task) = self.task.take()
        {
            ctx.request_repaint();
            let result = task
                .worker
                .join()
                .unwrap_or(Err("The source search could not finish."));
            if let Some(dialog) = &mut self.dialog
                && dialog.conversation == task.conversation
            {
                match result {
                    Ok(packet) => dialog.set_packet(packet),
                    Err(message) => dialog.notice = Some(message),
                }
            }
        } else if self.task.is_some() {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }

    fn search(&mut self, dialog: &mut Dialog) {
        if self.task.is_some() {
            return;
        }
        dialog.packet = None;
        dialog.selected.clear();
        dialog.notice = None;
        let title = dialog.title.clone();
        let version = dialog.version.clone();
        let text = dialog.source.text.clone();
        let query = dialog.query.clone();
        match std::thread::Builder::new()
            .name("swift-source-search".to_owned())
            .spawn(move || Source::new(&title, &version, &text)?.search(&query))
        {
            Ok(worker) => {
                self.task = Some(Task {
                    conversation: dialog.conversation,
                    worker,
                })
            }
            Err(_) => dialog.notice = Some("The source search could not start. Try again."),
        }
    }
}

impl Drop for View {
    fn drop(&mut self) {
        self.dialog.take();
        // One pure, byte-bounded task has no network, disk or model calls. Join it
        // at teardown so private source bytes cannot outlive the native window.
        if let Some(task) = self.task.take() {
            let _joined = task.worker.join();
        }
    }
}

impl State {
    pub(super) fn scene_packet_button(&mut self, ui: &mut egui::Ui) {
        if ui
            .add_enabled(
                !self.model.busy()
                    && !self.dictation_requested
                    && self.scene_packets.task.is_none(),
                audio_files::action_button("Sources"),
            )
            .on_hover_text(
                "Find passages in pasted source text and review them before adding to your draft.",
            )
            .clicked()
        {
            self.scene_packets.dialog = Some(Dialog::new(self.active_conversation));
            self.focus_composer = false;
        }
    }

    fn add_scene_packet(&mut self, dialog: &Dialog) -> Result<(), &'static str> {
        if dialog.conversation != self.active_conversation || self.model.busy() {
            return Err(
                "Return to the original conversation and wait until its response finishes.",
            );
        }
        let packet = dialog.render()?;
        let separator = if self.input.is_empty() { "" } else { "\n\n" };
        let length = self.input.chars().count()
            .checked_add(separator.len())
            .and_then(|length| length.checked_add(packet.chars().count()))
            .filter(|length| *length <= INPUT_LIMIT)
            .ok_or("These excerpts and your draft exceed the message limit. Select fewer passages or shorten the draft.")?;
        self.input.push_str(separator);
        self.input.push_str(&packet);
        self.place_composer_cursor(length);
        self.recovery_dirty = true;
        self.focus_composer = true;
        Ok(())
    }

    pub(super) fn scene_packet_dialog(&mut self, ctx: &egui::Context) {
        self.scene_packets.poll(ctx);
        let Some(mut dialog) = self.scene_packets.dialog.take() else {
            return;
        };
        let mut close = false;
        let mut add = false;
        let mut search = false;
        let busy = self.scene_packets.task.is_some();
        let enabled = dialog.conversation == self.active_conversation && !self.model.busy();
        let ime = self.composing
            || ctx.input(|input| {
                input
                    .events
                    .iter()
                    .any(|event| matches!(event, egui::Event::Ime(_)))
            });
        let modal = egui::Modal::new(egui::Id::unique("scene-packet-dialog"))
            .frame(audio_files::dialog_frame(ctx))
            .show(ctx, |ui| {
                ui.set_width((ctx.content_rect().width() - 80.0).clamp(260.0, 640.0));
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                ui.heading("Sources for this message");
                ui.label("Select passages from your source. Review the added text before sending; no summary is generated.");
                egui::ScrollArea::vertical()
                    .id_salt("scene-packet-fields")
                    .max_height((ctx.content_rect().height() - 230.0).max(80.0))
                    .show(ui, |ui| {
                        dialog.source_fields(ui, enabled && !busy);
                        search = ui.add_enabled(
                            enabled && !busy && !ime && !dialog.source.text.is_empty(),
                            egui::Button::new("Find passages"),
                        ).clicked();
                        if busy {
                            ui.label("Finding passages on this computer…");
                        }
                        ui.add_enabled_ui(enabled && !busy, |ui| dialog.results(ui));
                        if let Some(notice) = dialog.notice {
                            ui.colored_label(ui.visuals().warn_fg_color, notice);
                        }
                        ui.label(RichText::new("Closing clears the pasted source and search results. Excerpts you add become part of your draft and follow your conversation-saving settings.").small().weak());
                        if !enabled {
                            ui.label("Return to the original conversation and wait until its response finishes.");
                        }
                    });
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    add = ui.add_enabled(
                        enabled && !busy && !ime && dialog.selected.iter().any(|selected| *selected),
                        egui::Button::new("Add selected excerpts"),
                    ).clicked();
                    close = ui.button("Cancel").clicked();
                });
            });
        if add {
            match self.add_scene_packet(&dialog) {
                Ok(()) => close = true,
                Err(message) => dialog.notice = Some(message),
            }
        }
        if close || (modal.should_close() && !ime) {
            dialog.clear_editor_history(ctx);
            self.focus_composer = true;
            // An already-running bounded worker is joined by poll or teardown;
            // its result is discarded and no second task can start meanwhile.
        } else {
            if search {
                self.scene_packets.search(&mut dialog);
                ctx.request_repaint();
            }
            self.scene_packets.dialog = Some(dialog);
        }
    }
}

#[cfg(test)]
mod tests;
