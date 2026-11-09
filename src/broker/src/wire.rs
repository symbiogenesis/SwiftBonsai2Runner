//! Shared HTTP wire mechanics; request and response policy stays with callers.

#![expect(
    clippy::redundant_pub_crate,
    reason = "Keep shared HTTP helpers explicitly crate scoped"
)]

use crate::Rejection;

pub(super) enum LengthError {
    Syntax,
    Overflow,
}

pub(super) fn content_length(bytes: &[u8]) -> Result<usize, LengthError> {
    // Validate the entire field before reporting overflow, preserving the
    // request policy's distinction between invalid framing and excessive size.
    if bytes.is_empty() || !bytes.iter().all(u8::is_ascii_digit) {
        return Err(LengthError::Syntax);
    }
    bytes
        .iter()
        .try_fold(0usize, |value, byte| {
            value.checked_mul(10)?.checked_add(usize::from(byte - b'0'))
        })
        .ok_or(LengthError::Overflow)
}

pub(super) fn head_end(raw: &[u8], previous: usize) -> Option<usize> {
    // A terminator can straddle the previous read by at most three bytes.
    // Each byte is scanned at most four times even with one-byte fragments.
    let start = previous.saturating_sub(3);
    memchr::memmem::find(&raw[start..], b"\r\n\r\n").map(|offset| start + offset + 4)
}

pub(super) fn valid_line_endings(raw: &[u8]) -> bool {
    !raw.iter().enumerate().any(|(i, byte)| match byte {
        b'\r' => raw.get(i + 1) != Some(&b'\n'),
        b'\n' => i == 0 || raw[i - 1] != b'\r',
        _ => false,
    })
}

pub(super) fn valid_header_syntax(headers: &[httparse::Header<'_>]) -> bool {
    headers.iter().all(|header| {
        !header.name.is_empty()
            && header.name.bytes().all(is_header_name_byte)
            && header
                .value
                .iter()
                .all(|&byte| byte == b'\t' || (byte >= b' ' && byte != 0x7f))
    })
}

const fn is_header_name_byte(byte: u8) -> bool {
    matches!(
        byte,
        b'0'..=b'9'
            | b'a'..=b'z'
            | b'A'..=b'Z'
            | b'!'
            | b'#'
            | b'$'
            | b'%'
            | b'&'
            | b'\''
            | b'*'
            | b'+'
            | b'-'
            | b'.'
            | b'^'
            | b'_'
            | b'`'
            | b'|'
            | b'~'
    )
}

pub(super) fn has_header(headers: &[httparse::Header<'_>], name: &str) -> bool {
    headers
        .iter()
        .any(|header| header.name.eq_ignore_ascii_case(name))
}

pub(super) fn singleton_slice<'a>(
    headers: &'a [httparse::Header<'a>],
    name: &str,
) -> Result<Option<&'a [u8]>, Rejection> {
    let mut values = headers
        .iter()
        .filter(|header| header.name.eq_ignore_ascii_case(name))
        .map(|header| header.value);
    let first = values.next();
    if values.next().is_some() {
        return Err(Rejection::Headers);
    }
    Ok(first)
}
