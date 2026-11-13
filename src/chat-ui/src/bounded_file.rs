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
}
