//! Shared header scanning and media types for the local HTTP adapters.

#![forbid(unsafe_code)]
#![expect(
    clippy::redundant_pub_crate,
    reason = "The native HTTP adapters share this crate-private parser."
)]

/// Locates the first HTTP header ending without rescanning an unchanged prefix.
#[derive(Default)]
pub(super) struct HeaderEnd {
    next: usize,
    #[cfg(test)]
    comparisons: usize,
}

impl HeaderEnd {
    /// Receives the same growing buffer until its first complete header ending.
    pub(super) fn find(&mut self, bytes: &[u8]) -> Option<usize> {
        for (offset, window) in bytes[self.next..].array_windows().enumerate() {
            #[cfg(test)]
            {
                self.comparisons += 1;
            }
            if window == b"\r\n\r\n" {
                return Some(self.next + offset + 4);
            }
        }
        // Only these trailing bytes can begin a delimiter spanning the next read.
        self.next = bytes.len().saturating_sub(3);
        None
    }
}

/// Accept the expected media type with at most one explicit UTF-8 charset.
pub(super) fn utf8_media_type(value: &[u8], expected: &str) -> bool {
    let Ok(value) = std::str::from_utf8(value) else {
        return false;
    };
    let mut parts = value.split(';');
    if !parts.next().is_some_and(|media| {
        media
            .trim_matches([' ', '\t'])
            .eq_ignore_ascii_case(expected)
    }) {
        return false;
    }
    let Some(parameter) = parts.next() else {
        return true;
    };
    let Some((name, charset)) = parameter.split_once('=') else {
        return false;
    };
    // HTTP optional whitespace is SP / HTAB, not Unicode White_Space.
    let charset = charset.trim_matches([' ', '\t']);
    name.trim_matches([' ', '\t'])
        .eq_ignore_ascii_case("charset")
        && (charset.eq_ignore_ascii_case("utf-8") || charset.eq_ignore_ascii_case("\"utf-8\""))
        && parts.next().is_none()
}

#[cfg(test)]
mod tests {
    use super::{HeaderEnd, utf8_media_type};

    #[test]
    fn fragmented_headers_scan_each_window_once_and_preserve_coalesced_payload() {
        let mut head = vec![b'x'; 16_384 - 4];
        head.extend_from_slice(b"\r\n\r\n");
        for fragment in [1, 2, 3, 7, 4096, 16_384] {
            let mut scan = HeaderEnd::default();
            let mut received = Vec::new();
            for chunk in head.chunks(fragment) {
                received.extend_from_slice(chunk);
                let expected = (received.len() == head.len()).then_some(head.len());
                assert_eq!(scan.find(&received), expected);
            }
            assert_eq!(scan.comparisons, head.len() - 3);
        }
        for split in 1..=4 {
            let mut scan = HeaderEnd::default();
            assert_eq!(scan.find(&head[..head.len() - split]), None);
            let mut wire = head.clone();
            wire.extend_from_slice(b"body\r\n\r\n");
            assert_eq!(scan.find(&wire), Some(head.len()));
            assert_eq!(&wire[head.len()..], b"body\r\n\r\n");
            assert_eq!(scan.comparisons, head.len() - 3);
        }
    }

    #[test]
    fn local_media_types_share_the_same_utf8_parameter_subset() {
        for media in ["application/json", "text/event-stream"] {
            for suffix in [
                "",
                "; CHARSET=UTF-8",
                " ; charset = \"utf-8\"",
                "\t;\tcharset\t=\tutf-8\t",
            ] {
                assert!(utf8_media_type(
                    format!("{media}{suffix}").as_bytes(),
                    media
                ));
            }
            for suffix in [
                "-extra",
                ";",
                "; charset=latin1",
                "; charset=utf-8; charset=utf-8",
                "; charset=\"utf-8",
                "; other=utf-8",
            ] {
                assert!(!utf8_media_type(
                    format!("{media}{suffix}").as_bytes(),
                    media
                ));
            }
            assert!(!utf8_media_type(&[0xff], media));
            for whitespace in [
                '\u{00a0}', '\u{2003}', '\u{0085}', '\u{000b}', '\u{000c}', '\r', '\n',
            ] {
                for value in [
                    format!("{whitespace}{media}"),
                    format!("{media}{whitespace}"),
                    format!("{media};{whitespace}charset=utf-8"),
                    format!("{media};charset{whitespace}=utf-8"),
                    format!("{media};charset={whitespace}utf-8"),
                    format!("{media};charset=utf-8{whitespace}"),
                ] {
                    assert!(!utf8_media_type(value.as_bytes(), media), "{value:?}");
                }
            }
        }
        assert!(!utf8_media_type(b"application/json", "text/event-stream"));
        assert!(!utf8_media_type(b"text/event-stream", "application/json"));
    }
}
