//! Local conversation filtering. Cached matches never retain a second transcript.
use super::{Transcript, egui, workspace::Conversation};
use std::sync::Arc;

pub(super) fn field_id() -> egui::Id {
    egui::Id::unique("conversation-history-query")
}

#[derive(Default)]
pub(super) struct Search {
    pub(super) text: String,
    previous: String,
    query: Arc<str>,
    pub(super) rows: Vec<usize>,
    reset_scroll: bool,
}

impl Search {
    pub(super) fn clear(&mut self) {
        self.text.clear();
    }

    pub(super) fn clear_on_escape(&mut self, ctx: &egui::Context) -> bool {
        // egui's input prepass can release focus before the application sees
        // Escape. Only reclaim that release, never another control's focus.
        let owns_escape = ctx.memory(|memory| {
            memory.has_focus(field_id())
                || (memory.focused().is_none() && memory.had_focus_last_frame(field_id()))
        });
        if !self.text.is_empty()
            && owns_escape
            && ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            self.clear();
            ctx.memory_mut(|memory| memory.request_focus(field_id()));
            return true;
        }
        false
    }

    pub(super) fn filtered(&self) -> bool {
        !self.query.is_empty()
    }

    pub(super) fn take_scroll_reset(&mut self) -> bool {
        std::mem::take(&mut self.reset_scroll)
    }

    pub(super) fn update(
        &mut self,
        conversations: &mut [Conversation],
        active: u64,
        transcript: &Transcript,
    ) -> bool {
        let changed = self.previous != self.text;
        if changed {
            self.text.clone_into(&mut self.previous);
            self.query = lowercase(self.text.trim()).into();
            self.reset_scroll = true;
        }
        self.rows.clear();
        for (index, conversation) in conversations.iter_mut().enumerate() {
            let transcript = if conversation.id == active {
                Some(transcript)
            } else {
                conversation.saved.as_ref().map(|saved| &saved.transcript)
            };
            if self.query.is_empty()
                || conversation
                    .search
                    .matches(&self.query, &conversation.title, transcript)
            {
                self.rows.push(index);
            }
        }
        changed
    }
}

#[derive(Default)]
pub(super) struct MatchCache {
    query: Option<Arc<str>>,
    title: String,
    revision: Option<u64>,
    result: Option<bool>,
    #[cfg(test)]
    pub(super) scans: usize,
}

impl MatchCache {
    fn matches(&mut self, query: &Arc<str>, title: &str, transcript: Option<&Transcript>) -> bool {
        let revision = transcript.map(Transcript::history_revision);
        if self
            .query
            .as_ref()
            .is_some_and(|cached| Arc::ptr_eq(cached, query))
            && self.title == title
            && self.revision == revision
            && let Some(result) = self.result
        {
            return result;
        }
        #[cfg(test)]
        {
            self.scans += 1;
        }
        let result = lowercase(title).contains(query.as_ref())
            || transcript.is_some_and(|transcript| {
                transcript
                    .messages
                    .iter()
                    .any(|message| lowercase(&message.body).contains(query.as_ref()))
            });
        self.query = Some(Arc::clone(query));
        title.clone_into(&mut self.title);
        self.revision = revision;
        self.result = Some(result);
        result
    }
}

// Use the same scalar folding as transcript Find, including Greek sigma and
// lowercase expansions, so sidebar body results have identical highlights.
fn lowercase(text: &str) -> String {
    text.chars().flat_map(char::to_lowercase).collect()
}

// egui itself uses f32 for scroll offsets. Precision loss far beyond the visible
// row range can affect positioning, but never conversation identity or selection.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "Convert the virtual row position to egui's floating-point scroll coordinates."
)]
pub(super) fn row_offset(index: usize, stride: f32) -> f32 {
    index as f32 * stride
}
