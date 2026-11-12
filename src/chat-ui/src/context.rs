//! Local, advisory context estimates. Only the broker's tokenizer can admit a request.
//!
//! These intentionally inexpensive heuristics are neither exact token counts nor
//! upper bounds. They must never gate Send, trim history or change response settings.
#![expect(
    clippy::redundant_pub_crate,
    reason = "The private context estimator exposes its policy and estimates to sibling UI modules."
)]

use crate::client::Message;

/// Pinned broker policy: prompt, requested response and reserve share this limit.
pub(crate) const CONTEXT_TOKENS: usize = 8_192;
/// Exact broker policy reserve, separate from the estimated rendered prompt.
pub(crate) const RESERVE_TOKENS: usize = 64;

const MESSAGE_FRAMING_TOKENS: usize = 8;
const NEXT_TURN_FRAMING_TOKENS: usize = 16;
// Approximate the planning instructions, workflow wrapper and null working JSON
// in client/effort.rs. Later passes also include generated notes/drafts and are
// deliberately outside this first-pass estimate.
const EFFORT_PLANNING_TOKENS: usize = 192;
const NEAR_TOKENS: usize = CONTEXT_TOKENS * 85 / 100;
const OUTPUT_CAPS: [u32; 4] = [512, 1_024, 2_048, 4_096];

/// Advisory presentation state, never an admission decision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Pressure {
    Comfortable,
    Near,
    Over,
}

/// Estimated first-request occupancy with an exact selected response reservation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Estimate {
    /// Committed content, retained reasoning and approximate message framing.
    pub(crate) history: usize,
    /// Trimmed draft plus approximate next-turn template framing.
    pub(crate) draft: usize,
    /// Approximate Extra effort planning wrapper; zero for Standard.
    pub(crate) workflow: usize,
    /// Selected response cap, including generated thinking.
    pub(crate) output: usize,
    /// Estimated prompt plus exact response cap and policy reserve.
    pub(crate) total: usize,
}

impl Estimate {
    /// Combine a cached committed-history estimate with the current draft/options.
    pub(crate) fn new(history: usize, draft: &str, output_cap: u32, extra_effort: bool) -> Self {
        let draft = text_tokens(draft.trim()).saturating_add(NEXT_TURN_FRAMING_TOKENS);
        let workflow = if extra_effort {
            EFFORT_PLANNING_TOKENS
        } else {
            0
        };
        let output = usize::try_from(output_cap).unwrap_or(usize::MAX);
        let total = history
            .saturating_add(draft)
            .saturating_add(workflow)
            .saturating_add(output)
            .saturating_add(RESERVE_TOKENS);
        Self {
            history,
            draft,
            workflow,
            output,
            total,
        }
    }

    /// Rendered input estimate before the response cap and policy reserve.
    pub(crate) const fn prompt_tokens(self) -> usize {
        self.history
            .saturating_add(self.draft)
            .saturating_add(self.workflow)
    }

    /// Estimated spare capacity after reserving the complete selected response.
    pub(crate) const fn remaining(self) -> usize {
        CONTEXT_TOKENS.saturating_sub(self.total)
    }

    /// Estimated excess; zero also includes exact equality with the policy limit.
    pub(crate) const fn overflow(self) -> usize {
        self.total.saturating_sub(CONTEXT_TOKENS)
    }

    /// Distinguish comfortable, near-capacity and estimated over-capacity states.
    pub(crate) const fn pressure(self) -> Pressure {
        if self.total > CONTEXT_TOKENS {
            Pressure::Over
        } else if self.total >= NEAR_TOKENS {
            Pressure::Near
        } else {
            Pressure::Comfortable
        }
    }

    /// Largest supported smaller response cap that fits this input estimate.
    ///
    /// This is an explicit user action suggestion, never a promise of admission.
    pub(crate) fn suggested_cap(self) -> Option<u32> {
        let available =
            CONTEXT_TOKENS.saturating_sub(self.prompt_tokens().saturating_add(RESERVE_TOKENS));
        OUTPUT_CAPS.into_iter().rev().find(|&cap| {
            usize::try_from(cap).is_ok_and(|cap| cap < self.output && cap <= available)
        })
    }
}

/// Estimate committed model history, including reasoning even with Thinking off.
///
/// Call only when conversation history changes; adopted partial replies are
/// included and unadopted effort drafts are excluded. No history text is retained here.
pub(crate) fn history_tokens(history: &[Message]) -> usize {
    history.iter().fold(0_usize, |total, message| {
        total
            .saturating_add(MESSAGE_FRAMING_TOKENS)
            .saturating_add(text_tokens(&message.content))
            .saturating_add(message.reasoning_content.as_deref().map_or(0, text_tokens))
    })
}

// Eighth-token weights avoid per-character floating point and rounding drift.
// Prose has cheaper spaces; punctuation/digits make code and numeric material
// denser. Non-ASCII scripts and emoji must not inherit English chars/4 behavior.
fn text_tokens(text: &str) -> usize {
    let units = text.chars().fold(0_usize, |total, character| {
        let weight = if character.is_ascii_alphabetic() {
            2
        } else if character.is_ascii_digit() {
            3
        } else if character.is_whitespace() {
            1
        } else if character.is_ascii() {
            8
        } else if matches!(
            character,
            '\u{2e80}'..='\u{9fff}' | '\u{ac00}'..='\u{d7af}' | '\u{f900}'..='\u{faff}'
        ) {
            12
        } else if matches!(character, '\u{2600}'..='\u{27bf}' | '\u{1f000}'..='\u{1faff}') {
            24
        } else {
            character.len_utf8() * 4
        };
        total.saturating_add(weight)
    });
    units.div_ceil(8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_reservation_and_exact_boundary_remain_distinct_from_estimation() {
        let history = CONTEXT_TOKENS - RESERVE_TOKENS - NEXT_TURN_FRAMING_TOKENS - 512;
        let fits = Estimate::new(history, "", 512, false);
        assert_eq!(fits.total, CONTEXT_TOKENS);
        assert_eq!(fits.pressure(), Pressure::Near);
        assert_eq!(fits.remaining(), 0);
        assert_eq!(fits.overflow(), 0);
        let over = Estimate::new(history + 1, "", 512, false);
        assert_eq!(over.pressure(), Pressure::Over);
        assert_eq!(over.overflow(), 1);
        assert_eq!(over.suggested_cap(), None);
    }

    #[test]
    fn lower_cap_suggestion_preserves_input_and_reserve() {
        let large = Estimate::new(5_000, "hello", 4_096, false);
        assert_eq!(large.pressure(), Pressure::Over);
        assert_eq!(large.suggested_cap(), Some(2_048));
        let small = Estimate::new(5_000, "hello", 2_048, false);
        assert_eq!(large.prompt_tokens(), small.prompt_tokens());
        assert_eq!(large.total - small.total, 2_048);
        assert_eq!(small.overflow(), 0);
        assert_eq!(
            Estimate::new(8_000, "hello", 4_096, false).suggested_cap(),
            None
        );
    }

    #[test]
    fn committed_reasoning_counts_independently_of_visible_answer() {
        let mut messages = vec![Message {
            role: "assistant",
            content: "Visible answer".into(),
            reasoning_content: None,
        }];
        let visible = history_tokens(&messages);
        messages[0].reasoning_content = Some("Retained reasoning 雨".into());
        assert_eq!(
            history_tokens(&messages),
            visible + text_tokens("Retained reasoning 雨")
        );
        assert_eq!(history_tokens(&[]), 0);
    }

    #[test]
    fn unicode_and_code_are_not_english_character_counts() {
        assert!(text_tokens("雨雨雨雨") > text_tokens("aaaa"));
        assert!(text_tokens("🌱🌱🌱🌱") > text_tokens("雨雨雨雨"));
        assert!(text_tokens("{}[]();=") > text_tokens("abcdefgh"));
        assert!(text_tokens("مرحبا") > 0);
        assert!(text_tokens("नमस्ते") > 0);
        assert_eq!(text_tokens(""), 0);
    }

    #[test]
    fn draft_trimming_and_extra_effort_match_their_estimated_scope() {
        let standard = Estimate::new(0, "question", 512, false);
        assert_eq!(standard, Estimate::new(0, " \nquestion\t ", 512, false));
        let effort = Estimate::new(0, "question", 512, true);
        assert_eq!(effort.workflow, EFFORT_PLANNING_TOKENS);
        assert_eq!(effort.total - standard.total, EFFORT_PLANNING_TOKENS);
        assert_eq!(standard.pressure(), Pressure::Comfortable);
    }

    #[test]
    fn saturated_totals_cannot_wrap_into_available_capacity() {
        let estimate = Estimate::new(usize::MAX, "question", u32::MAX, true);
        assert_eq!(estimate.total, usize::MAX);
        assert_eq!(estimate.pressure(), Pressure::Over);
        assert_eq!(estimate.remaining(), 0);
        assert_eq!(estimate.suggested_cap(), None);
    }
}
