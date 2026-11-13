//! One Unicode selection, spacing and capacity calculation for transcript input.

use std::ops::Range;

// A plan borrows only the inserted text, so its draft can be mutated immediately.
// Recompute it after asynchronous input completes; drafts and cursors can change.
pub(super) struct Plan<'a> {
    bytes: Range<usize>,
    text: &'a str,
    prefix: &'static str,
    suffix: &'static str,
    cursor: usize,
}

impl<'a> Plan<'a> {
    pub(super) fn new(
        input: &str,
        selection: Option<Range<usize>>,
        text: &'a str,
        limit: usize,
    ) -> Option<Self> {
        let length = input.chars().count();
        let range = selection.map_or(length..length, |range| {
            range.start.min(range.end).min(length)..range.start.max(range.end).min(length)
        });
        let byte_index = |index| {
            input
                .char_indices()
                .nth(index)
                .map_or(input.len(), |(byte, _)| byte)
        };
        let bytes = byte_index(range.start)..byte_index(range.end);
        let prefix = if input[..bytes.start]
            .chars()
            .next_back()
            .is_some_and(|character| !character.is_whitespace())
        {
            " "
        } else {
            ""
        };
        let suffix = if input[bytes.end..]
            .chars()
            .next()
            .is_some_and(|character| !character.is_whitespace())
        {
            " "
        } else {
            ""
        };
        let inserted = text
            .chars()
            .count()
            .saturating_add(prefix.len())
            .saturating_add(suffix.len());
        let available = limit.saturating_sub(length - (range.end - range.start));
        if inserted > available {
            return None;
        }
        Some(Self {
            bytes,
            text,
            prefix,
            suffix,
            cursor: range.start + inserted,
        })
    }

    pub(super) fn apply(self, input: &mut String) -> usize {
        let phrase = format!("{}{}{}", self.prefix, self.text, self.suffix);
        input.replace_range(self.bytes, &phrase);
        self.cursor
    }
}

#[cfg(test)]
mod tests {
    use super::Plan;

    #[test]
    fn transcript_plan_preserves_unicode_selection_spacing_and_cursor() -> Result<(), &'static str>
    {
        for (input, selection, expected, cursor) in [
            ("雨🌱雪", Some(1..2), "雨 語 雪", 4),
            ("雨 \t雪", Some(2..3), "雨 語 雪", 4),
            ("雪", Some(0..0), "語 雪", 2),
            ("雨", None, "雨 語", 3),
            ("雨", Some(4..20), "雨 語", 3),
            ("", None, "語", 1),
        ] {
            let mut draft = input.to_owned();
            let plan = Plan::new(&draft, selection, "語", 20).ok_or("insertion rejected")?;
            assert_eq!(plan.apply(&mut draft), cursor);
            assert_eq!(draft, expected);
        }
        Ok(())
    }

    #[test]
    fn transcript_plan_counts_padding_at_the_limit_and_releases_selection_capacity()
    -> Result<(), &'static str> {
        let mut draft = "ab".to_owned();
        assert!(Plan::new(&draft, Some(1..1), "雨", 4).is_none());
        assert_eq!(draft, "ab");
        let plan = Plan::new(&draft, Some(1..1), "雨", 5).ok_or("exact limit rejected")?;
        assert_eq!(plan.apply(&mut draft), 4);
        assert_eq!(draft, "a 雨 b");
        let plan = Plan::new(&draft, Some(0..5), "雪", 1).ok_or("selection capacity lost")?;
        assert_eq!(plan.apply(&mut draft), 1);
        assert_eq!(draft, "雪");
        Ok(())
    }
}
