//! Native text styling without removing source characters or interpreting HTML.
//! Raw offsets therefore remain valid for copying, selection and Find. Only the
//! message whose layout cache changed is scanned, with one forward highlight merge.
use super::{egui, search};

// Adversarial alternating markers cannot create an unbounded section inventory.
// After this many input spans, retain the remaining source as ordinary text;
// Find still applies its separately bounded highlights across that remainder.
const MAX_STYLE_SPANS: usize = 16_384;

#[derive(Clone, Copy)]
struct Fence {
    marker: u8,
    width: usize,
}

struct Builder<'a> {
    job: egui::text::LayoutJob,
    matches: &'a [search::Match],
    next_match: usize,
    visuals: &'a egui::Visuals,
    fallback: egui::TextFormat,
    styled_spans: usize,
}

impl Builder<'_> {
    fn append(&mut self, text: &str, format: &egui::TextFormat) {
        let offset = self.job.text.len();
        let end = offset + text.len();
        let mut cursor = offset;
        let base = if self.styled_spans < MAX_STYLE_SPANS {
            format.clone()
        } else {
            self.fallback.clone()
        };
        self.styled_spans = self.styled_spans.saturating_add(1);
        while cursor < end {
            while self
                .matches
                .get(self.next_match)
                .is_some_and(|found| found.range.end <= cursor)
            {
                self.next_match += 1;
            }
            let found = self
                .matches
                .get(self.next_match)
                .filter(|found| found.range.start < end);
            let (until, highlighted) = match found {
                Some(found) if found.range.start <= cursor => (found.range.end.min(end), true),
                Some(found) => (found.range.start.min(end), false),
                None => (end, false),
            };
            let mut effective = base.clone();
            if highlighted {
                effective.background = self.visuals.selection.bg_fill;
                effective.color = self.visuals.selection.stroke.color;
            }
            // The pinned LayoutJob::append coalesces adjacent equal formats.
            self.job
                .append(&text[cursor - offset..until - offset], 0.0, effective);
            cursor = until;
        }
    }

    fn inline(&mut self, text: &str, normal: &egui::TextFormat, code: &egui::TextFormat) {
        let bytes = text.as_bytes();
        let mut cursor = 0;
        let mut previous = 0;
        let mut delimiter = None;
        while cursor < bytes.len() {
            if bytes[cursor] == b'\\' {
                cursor = (cursor + 2).min(bytes.len());
            } else if bytes[cursor] == b'`' {
                let start = cursor;
                while bytes.get(cursor) == Some(&b'`') {
                    cursor += 1;
                }
                let width = cursor - start;
                if delimiter.is_none() {
                    self.append(&text[previous..start], normal);
                    previous = start;
                    delimiter = Some(width);
                } else if delimiter == Some(width) {
                    self.append(&text[previous..cursor], code);
                    previous = cursor;
                    delimiter = None;
                }
            } else {
                cursor += 1;
            }
        }
        // An unfinished inline span remains readable while a response streams.
        self.append(
            &text[previous..],
            if delimiter.is_some() { code } else { normal },
        );
    }
}

fn fence_start(line: &str) -> Option<(Fence, &str)> {
    let trimmed = line.trim_start_matches(' ');
    if line.len() - trimmed.len() > 3 {
        return None;
    }
    let marker = *trimmed.as_bytes().first()?;
    if !matches!(marker, b'`' | b'~') {
        return None;
    }
    let width = trimmed.bytes().take_while(|byte| *byte == marker).count();
    (width >= 3).then_some((Fence { marker, width }, &trimmed[width..]))
}

fn heading_size(line: &str) -> Option<f32> {
    let level = line.bytes().take_while(|byte| *byte == b'#').count();
    if !(1..=6).contains(&level)
        || !line
            .as_bytes()
            .get(level)
            .is_none_or(u8::is_ascii_whitespace)
    {
        return None;
    }
    Some(match level {
        1 => 22.0,
        2 => 20.0,
        _ => 18.0,
    })
}

fn list_prefix(line: &str) -> Option<usize> {
    let trimmed = line.trim_start_matches(' ');
    let indent = line.len() - trimmed.len();
    let bytes = trimmed.as_bytes();
    let marker = if matches!(bytes.first(), Some(b'-' | b'*' | b'+')) {
        1
    } else {
        let digits = bytes
            .iter()
            .take_while(|byte| byte.is_ascii_digit())
            .count();
        if digits == 0 || !matches!(bytes.get(digits), Some(b'.' | b')')) {
            return None;
        }
        digits + 1
    };
    bytes
        .get(marker)
        .is_some_and(u8::is_ascii_whitespace)
        .then_some(indent + marker)
}

pub(super) fn layout_job(
    body: &str,
    structured: bool,
    width: f32,
    visuals: &egui::Visuals,
    matches: &[search::Match],
) -> egui::text::LayoutJob {
    let normal = egui::TextFormat::simple(egui::FontId::proportional(16.0), visuals.text_color());
    let mut builder = Builder {
        job: egui::text::LayoutJob::default(),
        matches,
        next_match: 0,
        visuals,
        fallback: normal.clone(),
        styled_spans: 0,
    };
    builder.job.wrap.max_width = width;
    if !structured {
        builder.append(body, &normal);
        return builder.job;
    }
    let mut code = egui::TextFormat::simple(egui::FontId::monospace(16.0), visuals.text_color());
    code.background = visuals.code_bg_color;
    let mut fence: Option<Fence> = None;
    for line in body.split_inclusive('\n') {
        let delimiter = fence_start(line);
        if let Some(open) = fence {
            builder.append(line, &code);
            if delimiter.is_some_and(|(close, rest)| {
                close.marker == open.marker && close.width >= open.width && rest.trim().is_empty()
            }) {
                fence = None;
            }
        } else if let Some((open, _)) = delimiter {
            fence = Some(open);
            builder.append(line, &code);
        } else {
            let trimmed = line.trim_start_matches(' ');
            let mut format = normal.clone();
            if let Some(size) = heading_size(trimmed) {
                format.font_id.size = size;
            } else if trimmed.starts_with("> ") {
                format.italics = true;
            }
            if let Some(prefix) = list_prefix(line) {
                let mut marker = format.clone();
                marker.font_id = egui::FontId::monospace(16.0);
                builder.append(&line[..prefix], &marker);
                builder.inline(&line[prefix..], &format, &code);
            } else {
                builder.inline(line, &format, &code);
            }
        }
    }
    builder.job
}

#[cfg(test)]
mod tests {
    use super::*;

    fn format_at(job: &egui::text::LayoutJob, offset: usize) -> Option<&egui::TextFormat> {
        job.sections
            .iter()
            .find(|section| section.byte_range.contains(&egui::text::ByteIndex(offset)))
            .map(|section| &section.format)
    }

    #[test]
    fn structured_text_retains_raw_unicode_and_styles_native_code_and_headings() {
        let source = "# Heading 界\n\nUse `let é = 1;` here.\n- First\n2. Second\n> A quote\n```rust\nfn main() { /* 🌱 */ }\n```\n<html>literal</html>";
        let visuals = egui::Visuals::dark();
        let job = layout_job(source, true, 600.0, &visuals, &[]);
        assert_eq!(job.text, source);
        assert_eq!(job.wrap.max_width.to_bits(), 600.0_f32.to_bits());
        assert_eq!(
            format_at(&job, 0).map(|format| format.font_id.size.to_bits()),
            Some(22.0_f32.to_bits())
        );
        for needle in ["let é", "fn main", "```rust"] {
            let offset = source.find(needle);
            assert!(
                offset
                    .and_then(|offset| format_at(&job, offset))
                    .is_some_and(|format| {
                        format.font_id.family == egui::FontFamily::Monospace
                            && format.background == visuals.code_bg_color
                    })
            );
        }
        assert!(
            source
                .find("A quote")
                .and_then(|offset| format_at(&job, offset))
                .is_some_and(|format| format.italics)
        );
        assert!(
            source
                .find("<html>")
                .and_then(|offset| format_at(&job, offset))
                .is_some_and(|format| format.font_id.family == egui::FontFamily::Proportional)
        );
    }

    #[test]
    fn unfinished_fences_stream_as_code_until_a_matching_close() {
        let visuals = egui::Visuals::dark();
        for source in [
            "~~~rust\n界",
            "````rust\n```\nstill code",
            "An unfinished `界",
        ] {
            let job = layout_job(source, true, 600.0, &visuals, &[]);
            assert_eq!(job.text, source);
            assert!(
                format_at(&job, source.len() - 1)
                    .is_some_and(|format| format.font_id.family == egui::FontFamily::Monospace)
            );
        }
        let source = "~~~rust\ncode\n~~~\nProse";
        let job = layout_job(source, true, 600.0, &visuals, &[]);
        assert_eq!(job.text, source);
        assert!(
            format_at(&job, source.len() - 1)
                .is_some_and(|format| format.font_id.family == egui::FontFamily::Proportional)
        );
    }

    #[test]
    fn find_highlights_cross_native_style_boundaries_without_changing_offsets() {
        let source = "# Heading `界` tail\n```\n🌱 code\n```\n";
        let visuals = egui::Visuals::dark();
        let start = source.find("Heading").unwrap_or_default();
        let end = source.find("code").unwrap_or_default() + "code".len();
        let matches = [search::Match {
            message: 1,
            range: start..end,
        }];
        let job = layout_job(source, true, 420.0, &visuals, &matches);
        assert_eq!(job.text, source);
        for section in &job.sections {
            assert!(source.is_char_boundary(section.byte_range.start.0));
            assert!(source.is_char_boundary(section.byte_range.end.0));
            if section.byte_range.start.0 >= start && section.byte_range.end.0 <= end {
                assert_eq!(section.format.background, visuals.selection.bg_fill);
                assert_eq!(section.format.color, visuals.selection.stroke.color);
            }
        }
        assert!(
            source
                .find('界')
                .and_then(|offset| format_at(&job, offset))
                .is_some_and(|format| format.font_id.family == egui::FontFamily::Monospace)
        );
    }

    #[test]
    fn user_markup_and_escaped_inline_markers_stay_literal() {
        let source = "# Heading\n```rust\n界\n```\n`inline`";
        let visuals = egui::Visuals::dark();
        let job = layout_job(source, false, 600.0, &visuals, &[]);
        assert_eq!(job.text, source);
        assert!(
            job.sections
                .iter()
                .all(|section| section.format.font_id == egui::FontId::proportional(16.0))
        );
        let escaped = "Escaped \\`normal";
        let job = layout_job(escaped, true, 600.0, &visuals, &[]);
        assert_eq!(job.text, escaped);
        assert!(
            job.sections
                .iter()
                .all(|section| section.format.font_id.family == egui::FontFamily::Proportional)
        );
    }

    #[test]
    fn alternating_markers_bound_sections_and_plain_lines_coalesce() {
        let visuals = egui::Visuals::dark();
        let source = "`x` ".repeat(MAX_STYLE_SPANS);
        let job = layout_job(&source, true, 600.0, &visuals, &[]);
        assert_eq!(job.text, source);
        assert!(job.sections.len() <= MAX_STYLE_SPANS + 1);
        let source = "ordinary line\n".repeat(100);
        let job = layout_job(&source, true, 600.0, &visuals, &[]);
        assert_eq!(job.text, source);
        assert_eq!(job.sections.len(), 1);
    }

    #[test]
    fn code_background_changes_invalidate_only_assistant_layouts() {
        use super::super::{HighlightKey, Role, Transcript};
        use egui_kittest::Harness;

        let mut transcript = Transcript::default();
        transcript.push(Role::User, "`literal`".to_owned());
        transcript.push(Role::Assistant, "```rust\nlet x = 1;\n```".to_owned());
        let mut chat = Harness::builder()
            .with_theme(egui::Theme::Dark)
            .build_ui_state(
                |ui, transcript: &mut Transcript| {
                    for message in &mut transcript.messages {
                        message.layout(
                            ui,
                            600.0,
                            &[],
                            HighlightKey {
                                query_revision: 0,
                                count: 0,
                            },
                            None,
                        );
                    }
                },
                transcript,
            );
        chat.run();
        let before: Vec<_> = chat
            .state()
            .messages
            .iter()
            .map(|message| message.body_layouts)
            .collect();
        let mut visuals = chat.ctx.style_of(egui::Theme::Dark).visuals.clone();
        visuals.code_bg_color = egui::Color32::RED;
        chat.ctx.set_visuals(visuals);
        chat.run();
        assert_eq!(chat.state().messages[0].body_layouts, before[0]);
        assert_eq!(chat.state().messages[1].body_layouts, before[1] + 1);
    }
}
