//! Shared Unicode clip bounds for complete and incremental read aloud.

use super::{MAX_SEGMENT, MAX_TEXT, VecDeque};

pub(super) fn segments(text: &str) -> Option<VecDeque<String>> {
    Some(segments_iter(text)?.map(str::to_owned).collect())
}

pub(super) fn segments_iter(text: &str) -> Option<impl Iterator<Item = &str>> {
    if text.trim().is_empty()
        || text.contains('\0')
        || text.encode_utf16().take(MAX_TEXT + 1).count() > MAX_TEXT
    {
        return None;
    }
    let mut remaining = text.trim();
    Some(std::iter::from_fn(move || {
        while !remaining.is_empty() {
            let boundary = clip_boundary(remaining);
            let piece = remaining[..boundary].trim_matches(horizontal_whitespace);
            remaining = remaining[boundary..].trim_start_matches(horizontal_whitespace);
            if !piece.trim().is_empty() {
                return Some(piece);
            }
        }
        None
    }))
}

/// Keep available sentences together, preferring the final sentence in a full clip.
pub(super) fn clip_boundary(text: &str) -> usize {
    let Some(limit) = maximum(text) else {
        return text.len();
    };
    let mut boundary = 0;
    while let Some(next) = sentence_boundary(&text[boundary..], limit - boundary, false) {
        boundary += next;
        if boundary == limit {
            break;
        }
    }
    if boundary == 0 {
        phrase_boundary(text, limit)
    } else {
        boundary
    }
}

/// Return the byte offset only when another character exceeds the clip bound.
pub(super) fn maximum(text: &str) -> Option<usize> {
    text.char_indices().nth(MAX_SEGMENT).map(|(index, _)| index)
}

/// Prefer a clause, then a word, in the latter half of the bounded prefix.
pub(super) fn phrase_boundary(text: &str, maximum: usize) -> usize {
    let bounded = &text[..maximum];
    let minimum = bounded.chars().count() / 2;
    let mut clause = None;
    let mut word = None;
    let mut previous = None;
    for (position, (index, character)) in bounded.char_indices().enumerate() {
        if position >= minimum {
            if character.is_whitespace() {
                word = Some(index);
            }
            if matches!(character, ',' | ';' | ':' | '，' | '；' | '：' | '、') {
                let mut end = index + character.len_utf8();
                let next = text[end..].chars().next();
                // Thousands separators and times belong to the number. ASCII
                // punctuation inside a token (URLs, paths, etc.) is not a pause.
                let numeric =
                    previous.is_some_and(char::is_numeric) && next.is_some_and(char::is_numeric);
                while let Some(next) = bounded[end..].chars().next() {
                    if !is_closing_delimiter(next) {
                        break;
                    }
                    end += next.len_utf8();
                }
                if !numeric
                    && (matches!(character, '，' | '；' | '：' | '、')
                        || text[end..].chars().next().is_none_or(char::is_whitespace))
                {
                    clause = Some(end);
                }
            }
        }
        previous = Some(character);
    }
    clause.or(word).unwrap_or(maximum)
}

/// Return the byte offset after the first complete sentence within `limit`.
pub(super) fn sentence_boundary(text: &str, limit: usize, complete: bool) -> Option<usize> {
    let bounded = &text[..limit];
    let mut speakable = false;
    let mut non_digit = false;
    let mut indices = bounded.char_indices().peekable();
    while let Some((index, character)) = indices.next() {
        if character.is_alphanumeric() {
            speakable = true;
            if !character.is_ascii_digit() {
                non_digit = true;
            }
            continue;
        }
        if line_break(character) {
            if speakable {
                let mut end = index + character.len_utf8();
                // CRLF is one line ending when both scalars fit the clip.
                if character == '\r' && end < limit && text.as_bytes()[end] == b'\n' {
                    end += 1;
                }
                return Some(include_non_speakable_tail(text, limit, end, complete));
            }
            continue;
        }
        if is_sentence_punctuation(character) && speakable && (character != '.' || non_digit) {
            let mut end = index + character.len_utf8();
            let mut repeated = false;
            while let Some(&(next_index, next_char)) = indices.peek() {
                if is_sentence_punctuation(next_char) || is_closing_delimiter(next_char) {
                    repeated |= is_sentence_punctuation(next_char);
                    end = next_index + next_char.len_utf8();
                    indices.next();
                } else {
                    break;
                }
            }
            // Titles, initials and dotted abbreviations belong to their following
            // words. Splitting here restarts synthesis in the middle of a phrase.
            if character == '.'
                && !repeated
                && abbreviation(&text[..index])
                && !(complete && text[end..].trim().is_empty())
            {
                continue;
            }
            if (matches!(character, '。' | '！' | '？') && end < text.len())
                || text[end..].chars().next().is_some_and(char::is_whitespace)
                || (complete && end == text.len())
            {
                return Some(include_non_speakable_tail(text, limit, end, complete));
            }
        }
    }
    None
}

/// Unicode mandatory line breaks (UAX #14 classes BK, CR, LF and NL).
const fn line_break(character: char) -> bool {
    matches!(
        character,
        '\n' | '\r' | '\u{b}' | '\u{c}' | '\u{85}' | '\u{2028}' | '\u{2029}'
    )
}

/// Whitespace that can be trimmed without removing a mandatory line break.
pub(super) const fn horizontal_whitespace(character: char) -> bool {
    character.is_whitespace() && !line_break(character)
}

fn abbreviation(prefix: &str) -> bool {
    let word = prefix
        .rsplit(|character: char| !character.is_alphabetic() && character != '.')
        .next()
        .unwrap_or_default();
    let initials = word.split('.').all(|part| {
        let mut characters = part.chars();
        characters.next().is_some_and(char::is_alphabetic) && characters.next().is_none()
    });
    initials
        || ["Mr", "Mrs", "Ms", "Dr", "Prof", "Sr", "Jr", "St", "vs"]
            .iter()
            .any(|title| word.eq_ignore_ascii_case(title))
}

fn include_non_speakable_tail(text: &str, limit: usize, end: usize, complete: bool) -> usize {
    if complete
        && text.len() <= limit
        && !text[end..].trim().is_empty()
        && !text[end..].chars().any(char::is_alphanumeric)
    {
        text.len()
    } else {
        end
    }
}

const fn is_sentence_punctuation(character: char) -> bool {
    matches!(character, '.' | '!' | '?' | '…' | '。' | '！' | '？')
}

const fn is_closing_delimiter(character: char) -> bool {
    matches!(
        character,
        '"' | '\''
            | '”'
            | '’'
            | ')'
            | ']'
            | '}'
            | '»'
            | '*'
            | '_'
            | '`'
            | '」'
            | '』'
            | '）'
            | '】'
    )
}

#[cfg(test)]
mod tests {
    use super::{clip_boundary, maximum, phrase_boundary};

    #[test]
    fn unicode_line_breaks_release_sentences_and_keep_crlf_within_the_clip() {
        for separator in [
            "\r", "\n", "\r\n", "\u{b}", "\u{c}", "\u{85}", "\u{2028}", "\u{2029}",
        ] {
            let text = format!("First{separator}Second");
            assert_eq!(
                super::sentence_boundary(&text, text.len(), false),
                Some("First".len() + separator.len())
            );
        }
        assert_eq!(
            super::sentence_boundary("First\r\nSecond", 6, false),
            Some(6)
        );
    }

    #[test]
    fn hard_clip_prefers_clauses_and_keeps_their_closing_quotes() {
        for separator in [',', ';', ':', '，', '；', '：', '、'] {
            let head = format!(
                "{}\"the first clause{separator}\"",
                "A measured introduction ".repeat(6)
            );
            let input = format!(
                "{head} {}",
                "and the following clause continues with more words ".repeat(4)
            );
            assert_eq!(clip_boundary(&input), head.len());
        }
    }

    #[test]
    fn hard_clip_does_not_treat_numeric_or_token_separators_as_clauses() -> Result<(), &'static str>
    {
        let input = format!(
            "{}12,345 08:30 path:part item;part {}",
            "A measured introduction ".repeat(6),
            "remaining words ".repeat(15)
        );
        let limit = maximum(&input).ok_or("Expected bounded input")?;
        let expected = input[..limit]
            .char_indices()
            .rev()
            .find(|(_, character)| character.is_whitespace())
            .map(|(index, _)| index)
            .ok_or("Expected final word boundary")?;
        assert_eq!(phrase_boundary(&input, limit), expected);
        Ok(())
    }

    #[test]
    fn hard_clip_clause_window_counts_unicode_characters() {
        let head = format!("{}，", "读".repeat(140));
        let input = format!("{head}{}", "word ".repeat(40));
        assert_eq!(clip_boundary(&input), head.len());

        let input = format!("{}，{}", "读".repeat(100), "word ".repeat(40));
        assert!(clip_boundary(&input) > "读".repeat(100).len() + '，'.len_utf8());
    }

    #[test]
    fn prosody_complete_sentence_precedes_a_later_clause_cut() {
        let sentence = "The introduction ends naturally here.";
        let input = format!(
            "{sentence} {}; {}",
            "the following clause continues calmly ".repeat(4),
            "more words without a sentence ending ".repeat(8)
        );
        assert_eq!(clip_boundary(&input), sentence.len());
    }

    #[test]
    fn prosody_hard_clip_chooses_the_last_complete_clause_in_the_window() {
        let preferred = format!("{}, {};", "a".repeat(130), "b".repeat(50));
        let input = format!("{preferred} {}", "continuing words ".repeat(20));
        assert_eq!(clip_boundary(&input), preferred.len());
    }

    #[test]
    fn prosody_unicode_clause_and_closing_quote_fit_the_exact_character_cap() {
        let preferred = format!("{}，」", "读".repeat(super::super::MAX_SEGMENT - 2));
        let input = format!("{preferred}{}", "下一段".repeat(20));
        assert_eq!(clip_boundary(&input), preferred.len());
        assert_eq!(preferred.chars().count(), super::super::MAX_SEGMENT);
    }
}
