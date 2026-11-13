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
            let piece = remaining[..boundary].trim();
            remaining = remaining[boundary..].trim_start();
            if !piece.is_empty() {
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
        word_boundary(text, limit)
    } else {
        boundary
    }
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
