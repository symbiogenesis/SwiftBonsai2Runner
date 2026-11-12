//! Bounded context observations from the authenticated local broker response.

use super::{Failure, Result};
use crate::context::{CONTEXT_TOKENS, Capacity, MAX_CONTEXT_TOKENS};

const INVALID: Failure = Failure::Message("The local model returned invalid context metadata.");

#[derive(Default)]
pub(super) struct Headers<'a> {
    policy: Option<&'a [u8]>,
    observed: Option<&'a [u8]>,
    maximum: Option<&'a [u8]>,
}

impl<'a> Headers<'a> {
    pub(super) const fn capture(&mut self, name: &str, value: &'a [u8]) -> Result<()> {
        let target = if name.eq_ignore_ascii_case("x-bonsai-context-policy") {
            &mut self.policy
        } else if name.eq_ignore_ascii_case("x-bonsai-context-tokens") {
            &mut self.observed
        } else if name.eq_ignore_ascii_case("x-bonsai-max-context-tokens") {
            &mut self.maximum
        } else {
            return Ok(());
        };
        if target.replace(value).is_some() {
            return Err(INVALID);
        }
        Ok(())
    }

    pub(super) fn finish(self, context_rejection: bool) -> Result<Option<Capacity>> {
        let (policy, observed, maximum) = match (self.policy, self.observed, self.maximum) {
            (None, None, None) => return Ok(None),
            (Some(policy), Some(observed), Some(maximum)) => {
                (policy, number(observed)?, number(maximum)?)
            }
            _ => return Err(INVALID),
        };
        match policy {
            b"fixed" if observed == CONTEXT_TOKENS && maximum == CONTEXT_TOKENS => {
                Ok(Some(Capacity::Fixed))
            }
            b"available-memory-v1"
                if maximum == MAX_CONTEXT_TOKENS && (1..=maximum).contains(&observed) =>
            {
                Ok(Some(Capacity::Adaptive { observed }))
            }
            b"available-memory-v1"
                if context_rejection && maximum == MAX_CONTEXT_TOKENS && observed == 0 =>
            {
                // A pressure rejection may leave no allocatable context. It is
                // not a usable denominator or a new successful observation.
                Ok(Some(Capacity::Unknown))
            }
            _ => Err(INVALID),
        }
    }
}

fn number(bytes: &[u8]) -> Result<usize> {
    if bytes.is_empty() || !bytes.iter().all(u8::is_ascii_digit) {
        return Err(INVALID);
    }
    std::str::from_utf8(bytes)
        .ok()
        .and_then(|text| text.parse().ok())
        .ok_or(INVALID)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_metadata_accepts_complete_fixed_and_adaptive_observations() -> Result<()> {
        for (policy, observed, maximum, expected) in [
            ("fixed", 8192, 8192, Capacity::Fixed),
            (
                "available-memory-v1",
                131_072,
                262_144,
                Capacity::Adaptive { observed: 131_072 },
            ),
        ] {
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 0\r\nX-Bonsai-Context-Policy: {policy}\r\nx-bonsai-context-tokens: {observed}\r\nX-Bonsai-Max-Context-Tokens: {maximum}\r\n\r\n"
            );
            let (_, _, capacity) = super::super::parse_head(head.as_bytes())?;
            assert_eq!(capacity, Some(expected));
        }
        let (_, _, capacity) = super::super::parse_head(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 0\r\n\r\n",
        )?;
        assert_eq!(capacity, None);
        Ok(())
    }

    #[test]
    fn context_metadata_rejects_partial_duplicate_overflow_and_foreign_limits() {
        for headers in [
            "X-Bonsai-Context-Policy: available-memory-v1\r\n",
            "X-Bonsai-Context-Policy: fixed\r\nX-Bonsai-Context-Tokens: 8192\r\nX-Bonsai-Max-Context-Tokens: 8192\r\nx-bonsai-context-tokens: 8192\r\n",
            "X-Bonsai-Context-Policy: fixed\r\nX-Bonsai-Context-Tokens: 262144\r\nX-Bonsai-Max-Context-Tokens: 262144\r\n",
            "X-Bonsai-Context-Policy: available-memory-v1\r\nX-Bonsai-Context-Tokens: 262145\r\nX-Bonsai-Max-Context-Tokens: 262144\r\n",
            "X-Bonsai-Context-Policy: available-memory-v1\r\nX-Bonsai-Context-Tokens: 0\r\nX-Bonsai-Max-Context-Tokens: 262144\r\n",
            "X-Bonsai-Context-Policy: available-memory-v1\r\nX-Bonsai-Context-Tokens: 999999999999999999999999999999999\r\nX-Bonsai-Max-Context-Tokens: 262144\r\n",
            "X-Bonsai-Context-Policy: available-memory-v1\r\nX-Bonsai-Context-Tokens: +8192\r\nX-Bonsai-Max-Context-Tokens: 262144\r\n",
        ] {
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 0\r\n{headers}\r\n"
            );
            assert!(super::super::parse_head(head.as_bytes()).is_err());
        }
    }

    #[test]
    fn context_metadata_zero_pressure_capacity_preserves_typed_context_rejection() {
        let payload = r#"{"error":{"code":"context_length_exceeded"}}"#;
        let response = format!(
            "HTTP/1.1 413 Payload Too Large\r\nContent-Type: application/json\r\nContent-Length: {}\r\nX-Bonsai-Context-Policy: available-memory-v1\r\nX-Bonsai-Context-Tokens: 0\r\nX-Bonsai-Max-Context-Tokens: 262144\r\n\r\n{payload}",
            payload.len()
        );
        assert!(matches!(
            super::super::HttpBody::new(response.as_bytes()),
            Err(Failure::Rejected(super::super::CONTEXT_LIMIT_MESSAGE))
        ));
    }
}
