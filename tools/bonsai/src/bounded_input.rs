//! Byte-limited reads; callers retain file admission, decoding and handle ownership.
#![expect(
    clippy::redundant_pub_crate,
    reason = "Private maintenance modules expose only a crate-local implementation API."
)]

use std::io::{self, Read};

#[derive(Debug)]
pub(super) enum Error {
    Bound,
    Io(io::Error),
}

impl Error {
    pub(super) fn into_message(self, exceeded: &str) -> String {
        match self {
            Self::Bound => exceeded.into(),
            Self::Io(error) => error.to_string(),
        }
    }
}

pub(super) fn read(reader: impl Read, limit: u64) -> Result<Vec<u8>, Error> {
    // The extra byte detects growth after the caller's metadata admission. Reject
    // an unrepresentable probe instead of wrapping or accepting a prefix.
    let probe = limit.checked_add(1).ok_or(Error::Bound)?;
    let mut bytes = Vec::new();
    reader
        .take(probe)
        .read_to_end(&mut bytes)
        .map_err(Error::Io)?;
    if u64::try_from(bytes.len()).map_err(|_| Error::Bound)? > limit {
        return Err(Error::Bound);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use std::io::{self, Cursor, Read as _};

    use super::{Error, read};

    #[test]
    fn bounded_input_accepts_exact_and_empty_inputs_and_keeps_the_reader() -> io::Result<()> {
        assert_eq!(read(&b""[..], 0).ok(), Some(Vec::new()));
        let mut reader = Cursor::new(b"abc");
        assert_eq!(read(&mut reader, 3).ok(), Some(b"abc".to_vec()));
        assert_eq!(reader.position(), 3);
        assert_eq!(reader.read(&mut [0; 1])?, 0);
        Ok(())
    }

    #[test]
    fn bounded_input_rejects_growth_after_one_probe_byte() {
        let mut reader = Cursor::new(b"abcde");
        assert!(matches!(read(&mut reader, 2), Err(Error::Bound)));
        assert_eq!(reader.position(), 3);
        assert!(matches!(read(&b"x"[..], 0), Err(Error::Bound)));
    }

    #[test]
    fn bounded_input_rejects_limit_overflow_before_reading() {
        let mut reader = Cursor::new(b"abc");
        assert!(matches!(read(&mut reader, u64::MAX), Err(Error::Bound)));
        assert_eq!(reader.position(), 0);
    }

    #[test]
    fn bounded_input_preserves_io_failures_and_caller_bound_messages() {
        struct Failed;
        impl io::Read for Failed {
            fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::other("read failed"))
            }
        }
        assert!(
            matches!(read(Failed, 3), Err(Error::Io(error)) if error.to_string() == "read failed")
        );
        assert_eq!(Error::Bound.into_message("caller bound"), "caller bound");
        assert_eq!(
            Error::Io(io::Error::other("read failed")).into_message("caller bound"),
            "read failed"
        );
    }
}
