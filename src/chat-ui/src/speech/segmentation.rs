//! Shared Unicode clip bounds for complete and incremental read aloud.

use super::{MAX_SEGMENT, MAX_TEXT, VecDeque};

pub(super) fn segments(text: &str) -> Option<VecDeque<String>> {
    if text.trim().is_empty()
        || text.contains('\0')
        || text.encode_utf16().take(MAX_TEXT + 1).count() > MAX_TEXT
    {
        return None;
    }
    let mut remaining = text.trim();
    let mut result = VecDeque::new();
    while !remaining.is_empty() {
        let maximum = maximum(remaining);
        let limit = maximum.unwrap_or(remaining.len());
        let boundary = sentence_boundary(remaining, limit, true)
            .or_else(|| maximum.map(|limit| word_boundary(remaining, limit)))
            .unwrap_or(remaining.len());
        let piece = remaining[..boundary].trim();
        if !piece.is_empty() {
            result.push_back(piece.to_owned());
        }
        remaining = remaining[boundary..].trim_start();
    }
    Some(result)
}

/// Return the byte offset only when another character exceeds the clip bound.
pub(super) fn maximum(text: &str) -> Option<usize> {
    text.char_indices().nth(MAX_SEGMENT).map(|(index, _)| index)
}

/// Prefer a word boundary in the latter half of the bounded UTF-8 prefix.
pub(super) fn word_boundary(text: &str, maximum: usize) -> usize {
    text[..maximum]
        .char_indices()
        .rev()
        .find(|(index, character)| *index >= maximum / 2 && character.is_whitespace())
        .map_or(maximum, |(index, _)| index)
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
        if character == '\n' {
            if speakable {
                let end = index + character.len_utf8();
                return Some(include_non_speakable_tail(text, limit, end, complete));
            }
            continue;
        }
        if is_sentence_punctuation(character) && speakable && (character != '.' || non_digit) {
            let mut end = index + character.len_utf8();
            while let Some(&(next_index, next_char)) = indices.peek() {
                if is_sentence_punctuation(next_char) || is_closing_delimiter(next_char) {
                    end = next_index + next_char.len_utf8();
                    indices.next();
                } else {
                    break;
                }
            }
            if text[end..].chars().next().is_some_and(char::is_whitespace)
                || (complete && end == text.len())
            {
                return Some(include_non_speakable_tail(text, limit, end, complete));
            }
        }
    }
    None
}

pub(super) fn ends_sentence(text: &str) -> bool {
    let trimmed = text.trim_end_matches(|character: char| {
        !character.is_alphanumeric() && !is_sentence_punctuation(character)
    });
    trimmed.chars().last().is_some_and(is_sentence_punctuation)
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
        '"' | '\'' | '”' | '’' | ')' | ']' | '}' | '»' | '*' | '_' | '`'
    )
}
