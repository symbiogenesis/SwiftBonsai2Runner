//! Ordered Qwen2 split rules over the pinned Unicode category table.

use super::unicode::flags;

fn letter(ch: char) -> bool {
    flags(ch) & 4 != 0
}
fn number(ch: char) -> bool {
    flags(ch) & 2 != 0
}
fn whitespace(ch: char) -> bool {
    flags(ch) & 0x100 != 0
}

pub(super) fn pretokens(text: &str) -> impl Iterator<Item = &str> {
    let mut remaining = text;
    std::iter::from_fn(move || {
        if remaining.is_empty() {
            return None;
        }
        let count = next_length(remaining);
        let (word, rest) = remaining.split_at(count);
        remaining = rest;
        Some(word)
    })
}

fn next_length(text: &str) -> usize {
    let mut chars = text.char_indices();
    let Some((_, first)) = chars.next() else {
        return 0;
    };
    if first == '\'' {
        for contraction in ["s", "t", "m", "d", "re", "ve", "ll"] {
            if text
                .as_bytes()
                .get(1..1 + contraction.len())
                .is_some_and(|bytes| bytes.eq_ignore_ascii_case(contraction.as_bytes()))
            {
                return 1 + contraction.len();
            }
        }
    }
    if !matches!(first, '\r' | '\n')
        && !number(first)
        && (letter(first) || chars.clone().next().is_some_and(|(_, next)| letter(next)))
    {
        return chars
            .find_map(|(offset, ch)| (!letter(ch)).then_some(offset))
            .unwrap_or(text.len());
    }
    if number(first) {
        return first.len_utf8();
    }
    let punctuation_start = usize::from(first == ' ');
    if let Some(next) = text[punctuation_start..].chars().next()
        && !whitespace(next)
        && !letter(next)
        && !number(next)
        && flags(first) & 0xff != 0
    {
        let mut end = punctuation_start;
        for ch in text[punctuation_start..].chars() {
            if whitespace(ch) || letter(ch) || number(ch) || flags(ch).trailing_zeros() >= 8 {
                break;
            }
            end += ch.len_utf8();
        }
        for ch in text[end..].chars() {
            if !matches!(ch, '\r' | '\n') {
                break;
            }
            end += ch.len_utf8();
        }
        return end;
    }
    let mut after_newline = 0;
    let mut whitespace_end = 0;
    let mut last_start = 0;
    let mut count = 0;
    for (offset, ch) in text.char_indices() {
        if !whitespace(ch) {
            break;
        }
        last_start = offset;
        whitespace_end = offset + ch.len_utf8();
        count += 1;
        if matches!(ch, '\r' | '\n') {
            after_newline = whitespace_end;
        }
    }
    if after_newline > 0 {
        return after_newline;
    }
    if count > 1 && whitespace_end < text.len() {
        return last_start;
    }
    if count > 0 {
        return whitespace_end;
    }
    first.len_utf8()
}
