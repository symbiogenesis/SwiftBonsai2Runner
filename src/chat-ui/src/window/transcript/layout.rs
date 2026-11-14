//! Lazy variable-height measurement with one shared budget per egui frame.
//!
//! Galleys remain indivisible so native selection and source offsets survive.
//! The byte allowance admits one oversized message alone; it is an amortization
//! target, not a hard glyph-work bound for a single existing bounded message.
use super::{
    HighlightKey, MESSAGE_ACTION_SIZE, MESSAGE_CONTENT_GAP, MESSAGE_GAP, MESSAGE_INSET,
    MessageControls, Transcript, egui,
};
use std::ops::Range;

const MAX_LAYOUTS_PER_FRAME: usize = 8;
const LAYOUT_BYTES_PER_FRAME: usize = 256 * 1024;

#[derive(Default)]
struct Budget {
    frame: Option<u64>,
    layouts: usize,
    bytes: usize,
}

impl Budget {
    fn begin(&mut self, frame: u64) {
        if self.frame != Some(frame) {
            self.frame = Some(frame);
            self.layouts = 0;
            self.bytes = 0;
        }
    }

    fn admit(&mut self, bytes: usize) -> bool {
        if self.layouts >= MAX_LAYOUTS_PER_FRAME
            || (self.layouts != 0 && self.bytes.saturating_add(bytes) > LAYOUT_BYTES_PER_FRAME)
        {
            return false;
        }
        self.layouts += 1;
        self.bytes = self.bytes.saturating_add(bytes);
        true
    }
}

#[derive(Clone)]
struct Anchor {
    message: u64,
    // A retained active Find result anchors its exact source position. Ordinary
    // reading anchors the first visible message top, including a partial row.
    matched: Option<Range<usize>>,
    top: f32,
}

struct Observation {
    offset: f32,
    anchor: Anchor,
    bottom: bool,
}

#[derive(Default)]
pub(super) struct Viewport {
    observation: Option<Observation>,
    budget: Budget,
}

pub(super) struct Rows {
    pub(super) messages: Vec<Row>,
    pub(super) correction: f32,
}

pub(super) struct Row {
    matches: Range<usize>,
    highlights: HighlightKey,
    selected: Option<Range<usize>>,
    extra_height: f32,
    pub(super) height: f32,
    pub(super) ready: bool,
}

fn position(rows: &[Row], index: usize, gap: f32) -> f32 {
    rows[..index].iter().map(|row| row.height + gap).sum()
}

fn first_visible(rows: &[Row], offset: f32, gap: f32) -> usize {
    let mut top = 0.0;
    rows.iter()
        .position(|row| {
            let visible = top + row.height > offset;
            top += row.height + gap;
            visible
        })
        .unwrap_or_else(|| rows.len().saturating_sub(1))
}

impl Transcript {
    pub(super) fn prepare_layout(
        &mut self,
        ui: &egui::Ui,
        width: f32,
        controls: Option<&MessageControls<'_>>,
    ) -> Rows {
        let body_width = 2.0_f32.mul_add(-MESSAGE_INSET.x, width);
        let mut rows = self.layout_rows(controls);
        if rows.is_empty() {
            self.viewport.observation = None;
            return Rows {
                messages: rows,
                correction: 0.0,
            };
        }
        self.viewport.budget.begin(ui.ctx().cumulative_frame_nr());
        let gap = MESSAGE_GAP.max(ui.spacing().item_spacing.y);
        let viewport_height = ui.clip_rect().height();
        let offset = (ui.clip_rect().top() - ui.next_widget_position().y).max(0.0);
        let old_total = position(&rows, rows.len(), gap);
        let anchor = self.layout_anchor(&rows, offset, viewport_height, gap);
        let follow_bottom = offset > 0.0
            && self
                .viewport
                .observation
                .as_ref()
                .filter(|previous| (previous.offset - offset).abs() < 0.5)
                .map_or(old_total - offset <= viewport_height + 2.0, |previous| {
                    previous.bottom
                });
        let navigating = self.search.target.is_some() || self.search.latest;
        let priorities = self.layout_priorities(&rows, offset, viewport_height, gap, anchor.0);
        self.measure_rows(ui, body_width, &mut rows, priorities);
        let total = position(&rows, rows.len(), gap);
        let anchor_position = position(&rows, anchor.0, gap)
            + self
                .find_anchor_offset(anchor.0, anchor.1.matched.as_ref())
                .unwrap_or(0.0);
        let desired = if follow_bottom {
            total - viewport_height
        } else {
            anchor_position - anchor.1.top
        };
        let correction = if navigating {
            0.0
        } else {
            desired.clamp(0.0, (total - viewport_height).max(0.0)) - offset
        };
        let corrected_offset = offset + correction;
        // Earlier measurements can shrink and expose rows that were offscreen
        // when priorities were chosen. Refine those estimates on the next frame
        // even when none of the old visible rows exhausted the job budget.
        if visible_pending(&rows, corrected_offset, viewport_height, gap) {
            ui.ctx().request_repaint();
        }
        self.viewport.observation = (!navigating).then_some(Observation {
            offset: corrected_offset,
            anchor: Anchor {
                top: anchor_position - corrected_offset,
                ..anchor.1
            },
            bottom: total - corrected_offset <= viewport_height + 2.0,
        });
        Rows {
            messages: rows,
            correction,
        }
    }

    fn layout_anchor(
        &self,
        rows: &[Row],
        offset: f32,
        viewport_height: f32,
        gap: f32,
    ) -> (usize, Anchor) {
        let previous = self.viewport.observation.as_ref().filter(|previous| {
            // Actual wheel/drag/navigation movement chooses a new anchor.
            (previous.offset - offset).abs() < 0.5
        });
        previous
            .and_then(|previous| {
                self.messages
                    .iter()
                    .position(|message| message.id == previous.anchor.message)
                    .filter(|&index| {
                        previous.anchor.matched.is_none()
                            || previous.anchor.matched == rows[index].selected
                    })
                    .map(|index| (index, previous.anchor.clone()))
            })
            .unwrap_or_else(|| {
                let active = rows
                    .iter()
                    .position(|row| row.selected.is_some())
                    .filter(|&index| {
                        let row = &rows[index];
                        let top = position(rows, index, gap);
                        self.find_anchor_offset(index, row.selected.as_ref())
                            .is_some_and(|local| {
                                let point = top + local;
                                point >= offset && point <= offset + viewport_height
                            })
                    });
                let index = active.unwrap_or_else(|| first_visible(rows, offset, gap));
                let matched = active.and_then(|index| rows[index].selected.clone());
                let local = self
                    .find_anchor_offset(index, matched.as_ref())
                    .unwrap_or(0.0);
                (
                    index,
                    Anchor {
                        message: self.messages[index].id,
                        matched,
                        top: position(rows, index, gap) + local - offset,
                    },
                )
            })
    }

    fn measure_rows(
        &mut self,
        ui: &egui::Ui,
        body_width: f32,
        rows: &mut [Row],
        priorities: Vec<(usize, bool)>,
    ) {
        let mut pending = false;
        for (index, required) in priorities {
            let message = &mut self.messages[index];
            let row = &mut rows[index];
            row.ready =
                message.layout_current(ui, body_width, row.highlights, row.selected.as_ref());
            if row.ready {
                continue;
            }
            let bytes = message
                .body
                .len()
                .saturating_add(if message.show_reasoning {
                    message.reasoning.len()
                } else {
                    0
                });
            if !self.viewport.budget.admit(bytes) {
                pending |= required;
                continue;
            }
            message.layout(
                ui,
                body_width,
                &self.search.matches[row.matches.clone()],
                row.highlights,
                row.selected.as_ref(),
            );
            let height = message.height();
            message.measured_height = Some(height);
            row.height = height + row.extra_height;
            row.ready = true;
        }
        if pending {
            ui.ctx().request_repaint();
        }
    }

    fn find_anchor_offset(&self, index: usize, matched: Option<&Range<usize>>) -> Option<f32> {
        let matched = matched?;
        let message = &self.messages[index];
        let cache = message.cache.as_ref()?;
        if cache.body_revision != message.body_revision {
            return None;
        }
        let characters = message.body.get(..matched.start)?.chars().count();
        Some(
            MESSAGE_INSET.y
                + MESSAGE_ACTION_SIZE
                + MESSAGE_CONTENT_GAP
                + cache
                    .body
                    .pos_from_cursor(egui::text::CCursor::new(characters))
                    .center()
                    .y,
        )
    }

    fn layout_rows(&self, controls: Option<&MessageControls<'_>>) -> Vec<Row> {
        let mut consumed = 0;
        self.messages
            .iter()
            .map(|message| {
                let count = self.search.matches[consumed..]
                    .partition_point(|found| found.message == message.id);
                let matches = consumed..consumed + count;
                consumed += count;
                let selected = self
                    .search
                    .matches
                    .get(self.search.active)
                    .filter(|found| found.message == message.id)
                    .map(|found| found.range.clone());
                let extra_height = if controls
                    .is_some_and(|controls| message.retry_prefix(controls.history).is_some())
                {
                    MESSAGE_CONTENT_GAP + MESSAGE_ACTION_SIZE
                } else {
                    0.0
                };
                Row {
                    matches,
                    highlights: HighlightKey {
                        query_revision: if count == 0 {
                            0
                        } else {
                            self.search.query_revision
                        },
                        count,
                    },
                    selected,
                    extra_height,
                    height: message.height() + extra_height,
                    ready: false,
                }
            })
            .collect()
    }

    fn layout_priorities(
        &self,
        rows: &[Row],
        offset: f32,
        viewport_height: f32,
        gap: f32,
        anchor: usize,
    ) -> Vec<(usize, bool)> {
        let mut priorities = Vec::with_capacity(rows.len());
        let mut queued = vec![false; rows.len()];
        let mut push = |index: usize, required| {
            if !queued[index] {
                queued[index] = true;
                priorities.push((index, required));
            }
        };
        // Find has one exact target. It shares the budget and cannot trigger
        // eager remeasurement of the entire estimated prefix before scrolling.
        if let Some(target) = &self.search.target
            && let Some(index) = self
                .messages
                .iter()
                .position(|message| message.id == target.message)
        {
            push(index, true);
        }
        if self.search.latest {
            push(rows.len() - 1, true);
        }
        push(anchor, true);
        let mut top = 0.0;
        for (index, row) in rows.iter().enumerate() {
            if top + row.height >= offset && top <= offset + viewport_height {
                push(index, true);
            }
            top += row.height + gap;
        }
        // Spend spare capacity on offscreen rows without keeping an idle window
        // awake solely to measure them. Their existing sizes remain estimates.
        for index in 0..rows.len() {
            push(index, false);
        }
        priorities
    }
}

fn visible_pending(rows: &[Row], offset: f32, height: f32, gap: f32) -> bool {
    let mut top = 0.0;
    rows.iter().any(|row| {
        let pending = !row.ready && top + row.height >= offset && top <= offset + height;
        top += row.height + gap;
        pending
    })
}

#[cfg(test)]
mod tests {
    use super::super::Role;
    use super::*;
    use egui_kittest::{Harness, kittest::Queryable as _};

    struct Scene {
        transcript: Transcript,
        requested_offset: Option<f32>,
        offset: f32,
    }

    fn harness() -> Harness<'static, Scene> {
        let mut transcript = Transcript::default();
        for index in 0..24 {
            transcript.push(
                Role::User,
                format!("Message {index}: {}", "wrapping words 界 🌱 ".repeat(6)),
            );
        }
        Harness::builder()
            .with_size([640.0, 320.0])
            .with_theme(egui::Theme::Dark)
            .with_context_setup(|context| {
                context.add_font_provider(crate::window::test_font_provider());
                context.style_mut_of(egui::Theme::Dark, |style| {
                    style.scroll_animation = egui::style::ScrollAnimation::none()
                });
            })
            .build_ui_state(
                |ui, scene: &mut Scene| {
                    let mut scroll = egui::ScrollArea::vertical().auto_shrink([false, false]);
                    if let Some(offset) = scene.requested_offset.take() {
                        scroll = scroll.vertical_scroll_offset(offset);
                    }
                    let output = scroll.show(ui, |ui| {
                        scene
                            .transcript
                            .show(ui, ui.available_width().min(600.0), None, false);
                    });
                    scene.offset = output.state.clamped_offset().y;
                },
                Scene {
                    transcript,
                    requested_offset: None,
                    offset: 0.0,
                },
            )
    }

    fn count(scene: &Scene) -> usize {
        scene
            .transcript
            .messages
            .iter()
            .map(|message| message.body_layouts)
            .sum()
    }

    fn top(scene: &Scene, index: usize) -> f32 {
        scene.transcript.messages[..index]
            .iter()
            .map(|message| message.height() + MESSAGE_GAP)
            .sum::<f32>()
            - scene.offset
    }

    fn settle(chat: &mut Harness<'static, Scene>, frames: usize) {
        for _ in 0..frames {
            let previous = count(chat.state());
            chat.step();
            assert!(count(chat.state()) - previous <= MAX_LAYOUTS_PER_FRAME);
        }
    }

    #[test]
    fn layout_budget_caps_jobs_and_keeps_oversized_find_within_the_same_frame_budget() {
        let mut budget = Budget::default();
        budget.begin(10);
        for _ in 0..MAX_LAYOUTS_PER_FRAME {
            assert!(budget.admit(1));
        }
        assert!(!budget.admit(1));
        budget.begin(10);
        assert!(
            !budget.admit(1),
            "Another egui pass must not renew the budget"
        );
        budget.begin(11);
        assert!(budget.admit(LAYOUT_BYTES_PER_FRAME + 1));
        assert!(!budget.admit(1));
        assert!(!budget.admit(LAYOUT_BYTES_PER_FRAME + 1));
        budget.begin(12);
        assert!(budget.admit(LAYOUT_BYTES_PER_FRAME / 2));
        assert!(budget.admit(LAYOUT_BYTES_PER_FRAME / 2));
        assert!(!budget.admit(1));
    }

    #[test]
    fn lazy_transcript_reflow_caps_each_frame_and_prioritizes_exact_unicode_find()
    -> Result<(), &'static str> {
        let mut chat = harness();
        settle(&mut chat, 5);
        assert!(
            chat.state()
                .transcript
                .messages
                .iter()
                .all(|message| message.cache.is_some())
        );
        let previous = count(chat.state());
        chat.set_size(egui::vec2(360.0, 320.0));
        chat.step();
        assert!(count(chat.state()) - previous <= MAX_LAYOUTS_PER_FRAME);
        assert!(
            chat.state()
                .transcript
                .messages
                .iter()
                .any(|message| message.body_layouts == 1)
        );
        chat.state_mut()
            .transcript
            .replace_response("A Unicode 界 needle 🌱 target", None);
        chat.state_mut().transcript.find_query("needle");
        let target = chat.state().transcript.search.target.clone();
        settle(&mut chat, 1);
        let transcript = &chat.state().transcript;
        assert!(transcript.search.target.is_none());
        let target = target.ok_or("Expected exact Find target")?;
        let message = transcript
            .messages
            .last()
            .ok_or("Expected target message")?;
        assert_eq!(message.id, target.message);
        assert_eq!(message.body.get(target.range.clone()), Some("needle"));
        assert!(message.cache.as_ref().is_some_and(|cache| {
            cache.selected.as_ref() == Some(&target.range)
                && cache.body.job.text == message.body
                && !cache.underline.is_empty()
        }));
        let bytes = serde_json::to_value(transcript);
        assert!(bytes.is_ok_and(|value| value.as_object().is_some_and(|object| object.len() == 1)));
        Ok(())
    }

    #[test]
    fn viewport_anchor_survives_reflow_prefix_eviction_and_preserves_user_scrolling() {
        let mut chat = harness();
        settle(&mut chat, 5);
        let requested = top(chat.state(), 10) + 20.0;
        chat.state_mut().requested_offset = Some(requested);
        settle(&mut chat, 2);
        let reading_id = chat.state().transcript.messages[10].id;
        let reading_top = top(chat.state(), 10);
        assert!((reading_top + 20.0).abs() < 0.5);
        chat.set_size(egui::vec2(360.0, 320.0));
        chat.ctx.set_pixels_per_point(1.5);
        chat.ctx
            .data_mut(|data| data.insert_temp(egui::Id::unique("reading-text-size"), 24.0_f32));
        settle(&mut chat, 6);
        assert_eq!(chat.state().transcript.messages[10].id, reading_id);
        assert!((top(chat.state(), 10) - reading_top).abs() < 0.5);
        drop(chat.state_mut().transcript.messages.drain(..5));
        chat.state_mut().transcript.search.invalidate();
        settle(&mut chat, 2);
        assert_eq!(chat.state().transcript.messages[5].id, reading_id);
        assert!((top(chat.state(), 5) - reading_top).abs() < 0.5);
        let requested = chat.state().offset + 100.0;
        chat.state_mut().requested_offset = Some(requested);
        settle(&mut chat, 2);
        assert!((chat.state().offset - requested).abs() < 0.5);
    }

    #[test]
    fn active_find_keeps_its_source_position_during_lazy_prefix_reflow() -> Result<(), &'static str>
    {
        let mut chat = harness();
        settle(&mut chat, 5);
        let message = &mut chat.state_mut().transcript.messages[10];
        message.body.push_str(" Unicode 界 needle 🌱");
        message.body_revision += 1;
        chat.state_mut().transcript.search.invalidate();
        chat.state_mut().transcript.find_query("needle");
        settle(&mut chat, 3);
        let target = chat
            .state()
            .transcript
            .search
            .matches
            .first()
            .ok_or("Expected Find match")?
            .clone();
        let cursor_y = |scene: &Scene| -> Result<f32, &'static str> {
            let message = &scene.transcript.messages[10];
            let cache = message.cache.as_ref().ok_or("Expected Find galley")?;
            let characters = message.body[..target.range.start].chars().count();
            Ok(top(scene, 10)
                + MESSAGE_INSET.y
                + MESSAGE_ACTION_SIZE
                + MESSAGE_CONTENT_GAP
                + cache
                    .body
                    .pos_from_cursor(egui::text::CCursor::new(characters))
                    .center()
                    .y)
        };
        let before = cursor_y(chat.state())?;
        assert!((0.0..320.0).contains(&before));
        chat.set_size(egui::vec2(360.0, 320.0));
        settle(&mut chat, 6);
        assert!((cursor_y(chat.state())? - before).abs() < 0.5);
        assert!(chat.state().transcript.search.matches.first() == Some(&target));
        Ok(())
    }

    #[test]
    fn deferred_geometry_reflow_keeps_real_native_text_selection() {
        let mut chat = harness();
        chat.set_size(egui::vec2(640.0, 8000.0));
        settle(&mut chat, 5);
        let body = chat.state().transcript.messages[10].body.clone();
        let label = chat.get_by_label(&body).rect();
        let start = label.left_top() + egui::vec2(3.0, 8.0);
        let end = label.left_top() + egui::vec2(130.0, 8.0);
        chat.hover_at(start);
        chat.drag_at(start);
        chat.run_steps(1);
        chat.hover_at(end);
        chat.run_steps(1);
        chat.drop_at(end);
        chat.run();
        assert!(
            chat.ctx
                .plugin::<egui::text_selection::LabelSelectionState>()
                .lock()
                .has_selection()
        );
        let previous = chat.state().transcript.messages[10].body_layouts;
        chat.set_size(egui::vec2(360.0, 8000.0));
        settle(&mut chat, 1);
        assert_eq!(chat.state().transcript.messages[10].body_layouts, previous);
        assert!(
            chat.ctx
                .plugin::<egui::text_selection::LabelSelectionState>()
                .lock()
                .has_selection()
        );
        settle(&mut chat, 3);
        assert!(chat.state().transcript.messages[10].body_layouts > previous);
        assert!(
            chat.ctx
                .plugin::<egui::text_selection::LabelSelectionState>()
                .lock()
                .has_selection()
        );
    }

    #[test]
    fn shrunk_rows_schedule_measurement_for_newly_exposed_viewport_rows() {
        let mut rows = (0..12)
            .map(|_| Row {
                matches: 0..0,
                highlights: HighlightKey {
                    query_revision: 0,
                    count: 0,
                },
                selected: None,
                extra_height: 0.0,
                height: 200.0,
                ready: false,
            })
            .collect::<Vec<_>>();
        // Two old visible cards plus six spare background jobs consume the
        // frame budget. Their shorter measurements expose four untouched cards.
        for row in rows.iter_mut().take(MAX_LAYOUTS_PER_FRAME) {
            row.height = 20.0;
            row.ready = true;
        }
        assert!(visible_pending(&rows, 0.0, 400.0, MESSAGE_GAP));
        rows[8].ready = true;
        assert!(!visible_pending(&rows, 0.0, 400.0, MESSAGE_GAP));
        assert!(!visible_pending(&rows, 2000.0, 100.0, MESSAGE_GAP));
    }

    #[test]
    fn offscreen_find_match_does_not_replace_the_visible_message_anchor() {
        let mut chat = harness();
        settle(&mut chat, 5);
        let message = &mut chat.state_mut().transcript.messages[10];
        message.body = format!(
            "{}needle",
            "Long Unicode 界 paragraph with wrapping words.\n".repeat(80)
        );
        message.body_revision += 1;
        chat.state_mut().transcript.search.invalidate();
        chat.state_mut().transcript.find_query("needle");
        settle(&mut chat, 3);
        let prefix = top(chat.state(), 10) + chat.state().offset;
        chat.state_mut().requested_offset = Some(prefix + 20.0);
        settle(&mut chat, 2);
        assert!(
            chat.state()
                .transcript
                .viewport
                .observation
                .as_ref()
                .is_some_and(|observation| {
                    observation.anchor.message == chat.state().transcript.messages[10].id
                        && observation.anchor.matched.is_none()
                })
        );
        let before = top(chat.state(), 10);
        chat.set_size(egui::vec2(360.0, 320.0));
        settle(&mut chat, 6);
        assert!((top(chat.state(), 10) - before).abs() < 0.5);
    }
}
