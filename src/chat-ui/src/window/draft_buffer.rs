//! Atomic native editor transactions: a rejected insertion never replaces a selection.
use super::egui;
use egui::{
    TextBuffer,
    text::CharIndex,
    text_edit::{TextEditOutput, TextEditState},
};
use std::{any::TypeId, ops::Range};

type TextEditUndoer = egui::util::undoer::Undoer<(egui::text::CCursorRange, String)>;

// egui retains complete strings in each undo point. Eight points bound the
// undo points to 32 MiB of UTF-8 payload, plus current, flux and transaction copies.
const MAX_UNDO_POINTS: usize = 8;

pub(super) const LIMIT_MESSAGE: &str = "This edit exceeds the 1,048,576-character draft limit. Your draft was kept. Paste a smaller passage.";

/// A `TextEdit` may delete the selection before inserting pasted or IME text.
/// Retain its pre-edit bytes on mutation and snapshot undo state only on frames
/// with potential edits, so an oversized replacement can reject the entire
/// frame without losing the draft. Idle frames copy neither text nor history.
pub(super) struct Buffer<'a> {
    text: &'a mut String,
    limit: usize,
    original: Option<String>,
    previous: TextEditState,
    undoer: Option<TextEditUndoer>,
    rejected: bool,
}

impl<'a> Buffer<'a> {
    pub(super) fn new(
        text: &'a mut String,
        ctx: &egui::Context,
        id: egui::Id,
        limit: usize,
    ) -> Self {
        let previous = TextEditState::load(ctx, id).unwrap_or_else(|| {
            let mut state = TextEditState::default();
            state.set_undoer(TextEditUndoer::with_settings(
                egui::util::undoer::Settings {
                    max_undos: MAX_UNDO_POINTS,
                    ..Default::default()
                },
            ));
            state.clone().store(ctx, id);
            state
        });
        // Undo/redo holds egui's undo mutex while calling TextBuffer methods.
        // Snapshot before TextEdit handles events; never lock it in a mutation.
        // Pointer motion, idle frames and navigation keys require no history
        // copy. Keep other keys conservative: egui also supports editing through
        // shortcuts such as Ctrl+H/K/U/W, not just Backspace/Delete and undo.
        let undoer = ctx
            .input(|input| {
                input.events.iter().any(|event| match event {
                    egui::Event::Text(_)
                    | egui::Event::Paste(_)
                    | egui::Event::Cut
                    | egui::Event::Ime(_)
                    | egui::Event::AccessKitActionRequest(_) => true,
                    egui::Event::Key {
                        key, pressed: true, ..
                    } => !matches!(
                        key,
                        egui::Key::ArrowLeft
                            | egui::Key::ArrowRight
                            | egui::Key::ArrowUp
                            | egui::Key::ArrowDown
                            | egui::Key::Home
                            | egui::Key::End
                            | egui::Key::PageUp
                            | egui::Key::PageDown
                            | egui::Key::Escape
                    ),
                    _ => false,
                })
            })
            .then(|| previous.undoer());
        Self {
            text,
            limit,
            original: None,
            previous,
            undoer,
            rejected: false,
        }
    }

    fn checkpoint(&mut self) {
        if self.original.is_none() {
            self.original = Some(self.text.clone());
        }
    }

    pub(super) fn finish(mut self, output: &mut TextEditOutput) -> bool {
        if self.rejected {
            if let Some(original) = self.original.take() {
                *self.text = original;
            }
            if let Some(undoer) = self.undoer.take() {
                self.previous.set_undoer(undoer);
            }
            output.cursor_range = self.previous.cursor.char_range();
            output.state = self.previous;
            output
                .state
                .clone()
                .store(&output.response.response.ctx, output.response.response.id);
            output.response.response.ctx.request_repaint();
        }
        self.rejected
    }
}

pub(super) fn fits(text: &str) -> bool {
    text.len() <= super::INPUT_LIMIT * 4
        && text.chars().take(super::INPUT_LIMIT + 1).count() <= super::INPUT_LIMIT
}

impl TextBuffer for Buffer<'_> {
    fn is_mutable(&self) -> bool {
        true
    }

    fn as_str(&self) -> &str {
        self.text
    }

    fn insert_text(&mut self, text: &str, index: CharIndex) -> usize {
        if self.rejected {
            return 0;
        }
        let remaining = self.limit.saturating_sub(self.text.chars().count());
        // The byte guard avoids scanning an arbitrarily large clipboard payload.
        let count = if text.len() > remaining.saturating_mul(4) {
            remaining.saturating_add(1)
        } else {
            text.chars().take(remaining.saturating_add(1)).count()
        };
        if count > remaining {
            self.rejected = true;
            return 0;
        }
        if count != 0 {
            self.checkpoint();
            self.text.insert_text(text, index);
        }
        count
    }

    fn delete_char_range(&mut self, range: Range<CharIndex>) {
        if !self.rejected && !range.is_empty() {
            self.checkpoint();
            self.text.delete_char_range(range);
        }
    }

    fn type_id(&self) -> TypeId {
        TypeId::of::<Buffer<'static>>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draft_navigation_preserves_history_without_copying_undo_snapshots() -> std::io::Result<()> {
        let ctx = egui::Context::default();
        let id = egui::Id::unique("draft-navigation");
        let cursor = egui::text::CCursorRange::one(egui::text::CCursor::new(0));
        let older = (cursor, "Earlier café 🌱 draft".to_owned());
        let current = (cursor, "Current 界 draft".to_owned());
        let mut undoer = TextEditUndoer::default();
        undoer.add_undo(&older);
        undoer.add_undo(&current);
        let mut state = TextEditState::default();
        state.set_undoer(undoer);
        state.store(&ctx, id);

        for key in [
            egui::Key::ArrowLeft,
            egui::Key::ArrowRight,
            egui::Key::ArrowUp,
            egui::Key::ArrowDown,
            egui::Key::Home,
            egui::Key::End,
            egui::Key::PageUp,
            egui::Key::PageDown,
            egui::Key::Escape,
        ] {
            let mut text = current.1.clone();
            let _ = ctx.run_pass(
                egui::RawInput {
                    events: vec![egui::Event::Key {
                        key,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::SHIFT | egui::Modifiers::CTRL,
                    }],
                    ..Default::default()
                },
                |ctx| {
                    let buffer = Buffer::new(&mut text, ctx, id, 100);
                    assert!(buffer.undoer.is_none());
                },
            );
            assert_eq!(text, current.1);
        }
        let mut history = TextEditState::load(&ctx, id)
            .ok_or_else(|| std::io::Error::other("Draft undo state missing"))?
            .undoer();
        assert_eq!(history.undo(&current), Some(&older));
        assert_eq!(history.redo(&older), Some(&current));

        // An editing event in the same frame must still take a rollback
        // snapshot, even when navigation precedes it.
        let mut text = current.1.clone();
        let _ = ctx.run_pass(
            egui::RawInput {
                events: vec![
                    egui::Event::Key {
                        key: egui::Key::ArrowLeft,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    },
                    egui::Event::Paste("Replacement".to_owned()),
                ],
                ..Default::default()
            },
            |ctx| {
                let buffer = Buffer::new(&mut text, ctx, id, 100);
                assert!(buffer.undoer.is_some());
            },
        );
        Ok(())
    }

    #[test]
    fn draft_buffer_keeps_complete_unicode_at_limit_and_rejects_next_character() {
        let ctx = egui::Context::default();
        let mut text = "🌱".repeat(super::super::INPUT_LIMIT - 1);
        let mut buffer = Buffer::new(
            &mut text,
            &ctx,
            egui::Id::unique("draft"),
            super::super::INPUT_LIMIT,
        );
        assert_eq!(
            buffer.insert_text("界", CharIndex(super::super::INPUT_LIMIT - 1)),
            1
        );
        assert_eq!(
            buffer.insert_text("x", CharIndex(super::super::INPUT_LIMIT)),
            0
        );
        assert!(buffer.rejected);
        assert_eq!(buffer.text.chars().count(), super::super::INPUT_LIMIT);
    }

    #[test]
    fn draft_buffer_selection_overflow_retains_original_and_blocks_later_mutations() {
        let ctx = egui::Context::default();
        let mut text = "café 🌱 end".to_owned();
        let original = text.clone();
        let mut buffer = Buffer::new(&mut text, &ctx, egui::Id::unique("draft"), 12);
        buffer.delete_char_range(CharIndex(5)..CharIndex(6));
        assert_eq!(buffer.insert_text("oversized replacement", CharIndex(5)), 0);
        buffer.delete_char_range(CharIndex(0)..CharIndex(4));
        assert_eq!(buffer.original.as_deref(), Some(original.as_str()));
        assert_eq!(buffer.text.as_str(), "café  end");
    }
}
