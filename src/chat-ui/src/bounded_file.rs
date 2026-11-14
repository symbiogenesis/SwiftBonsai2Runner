//! Bounded file reads for native worker inputs and diagnostics.

#![expect(
    clippy::redundant_pub_crate,
    reason = "Native workers share bounded reads while retaining their own failure messages."
)]

use std::{
    fs::File,
    io::{self, Read},
    path::Path,
};

/// Read at most one byte past the limit; `None` means the input exceeded it.
pub(crate) fn read(reader: impl Read, limit: u64) -> io::Result<Option<Vec<u8>>> {
    let mut bytes = Vec::new();
    reader
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)?;
    Ok((u64::try_from(bytes.len()).map_err(io::Error::other)? <= limit).then_some(bytes))
}

/// Decode bounded UTF-8 output with the owning worker's overflow diagnostic.
pub(crate) fn text(path: &Path, limit: u64, exceeded: &'static str) -> io::Result<String> {
    let bytes = read(File::open(path)?, limit)?.ok_or_else(|| io::Error::other(exceeded))?;
    String::from_utf8(bytes).map_err(io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_reads_keep_exact_bytes_and_probe_only_one_excess_byte() -> io::Result<()> {
        let input = b"\0\xffRain \xe9\x9b\xa8";
        for limit in 0..=input.len() + 1 {
            let mut reader = io::Cursor::new(input);
            let limit = u64::try_from(limit).map_err(io::Error::other)?;
            let bytes = read(&mut reader, limit)?;
            if limit < u64::try_from(input.len()).map_err(io::Error::other)? {
                assert!(bytes.is_none());
                assert_eq!(reader.position(), limit + 1);
            } else {
                assert_eq!(bytes.as_deref(), Some(input.as_slice()));
            }
        }
        assert_eq!(
            read(input.as_slice(), u64::MAX)?.as_deref(),
            Some(input.as_slice())
        );
        Ok(())
    }

    #[test]
    fn bounded_reads_never_publish_partial_bytes_after_an_io_failure() -> io::Result<()> {
        struct Failed;

        impl Read for Failed {
            fn read(&mut self, _bytes: &mut [u8]) -> io::Result<usize> {
                Err(io::ErrorKind::PermissionDenied.into())
            }
        }

        for prefix in [b"".as_slice(), b"partial".as_slice()] {
            let limit = u64::try_from(prefix.len()).map_err(io::Error::other)?;
            let error = read(prefix.chain(Failed), limit)
                .err()
                .ok_or_else(|| io::Error::other("Read failure was accepted as complete input"))?;
            assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        }
        // Once overflow has been established, the reader must not continue to
        // the later failure (or accept the retained prefix as bounded success).
        assert_eq!(read(b"partial".as_slice().chain(Failed), 2)?, None);
        Ok(())
    }

    #[test]
    fn bounded_text_preserves_worker_overflow_utf8_and_open_failures() -> io::Result<()> {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let directory =
            std::env::temp_dir().join(format!("BonsaiBoundedText-{}-{stamp}", std::process::id()));
        std::fs::create_dir(&directory)?;
        let path = directory.join("status.txt");
        let exceeded = "Worker output exceeds its limit.";
        std::fs::write(&path, b"ready")?;
        assert_eq!(text(&path, 5, exceeded)?, "ready");
        std::fs::write(&path, [0xff; 6])?;
        let overflow = text(&path, 5, exceeded)
            .err()
            .ok_or_else(|| io::Error::other("Oversized text was accepted"))?;
        // Bound admission precedes UTF-8 decoding, preserving the owning
        // worker's diagnostic even when the oversized bytes are also invalid.
        assert_eq!(overflow.to_string(), exceeded);
        let invalid = text(&path, 6, exceeded)
            .err()
            .ok_or_else(|| io::Error::other("Invalid UTF-8 was accepted"))?;
        assert!(
            invalid.get_ref().is_some_and(
                <dyn std::error::Error + Send + Sync>::is::<std::string::FromUtf8Error>
            )
        );
        std::fs::remove_file(&path)?;
        let missing = text(&path, 5, exceeded)
            .err()
            .ok_or_else(|| io::Error::other("Missing input was accepted"))?;
        assert_eq!(missing.kind(), io::ErrorKind::NotFound);
        std::fs::remove_dir(directory)
    }
}
