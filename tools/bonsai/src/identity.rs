//! Canonical lowercase identities for maintenance receipts and reviewed pins.
#![expect(
    clippy::redundant_pub_crate,
    reason = "Private maintenance modules expose only a crate-local implementation API."
)]

use sha2::{Digest as _, Sha256};

pub(crate) struct Hasher {
    #[cfg(windows)]
    inner: bonsai_windows_credentials::Sha256,
    #[cfg(not(windows))]
    inner: Sha256,
}

impl Hasher {
    pub(crate) fn new() -> crate::Result<Self> {
        #[cfg(windows)]
        {
            Ok(Self {
                inner: bonsai_windows_credentials::Sha256::new()
                    .map_err(|error| error.to_string())?,
            })
        }
        #[cfg(not(windows))]
        {
            Ok(Self {
                inner: Sha256::new(),
            })
        }
    }

    pub(crate) fn update(&mut self, bytes: &[u8]) -> crate::Result<()> {
        #[cfg(windows)]
        {
            self.inner.update(bytes).map_err(|error| error.to_string())
        }
        #[cfg(not(windows))]
        {
            self.inner.update(bytes);
            Ok(())
        }
    }

    pub(crate) fn finalize_bytes(self) -> crate::Result<[u8; 32]> {
        #[cfg(windows)]
        {
            self.inner.finish().map_err(|error| error.to_string())
        }
        #[cfg(not(windows))]
        {
            Ok(self.inner.finalize().into())
        }
    }

    pub(crate) fn finalize_hex(self) -> crate::Result<String> {
        Ok(hex(&self.finalize_bytes()?))
    }
}

pub(super) fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(char::from(DIGITS[usize::from(byte >> 4)]));
        text.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    text
}

pub(super) fn sha256(bytes: &[u8]) -> String {
    #[cfg(windows)]
    if let Ok(digest) = bonsai_windows_credentials::Sha256::digest_bytes(bytes) {
        return hex(&digest);
    }
    hex(&Sha256::digest(bytes))
}

pub(super) fn is_lower_hex(text: &str, length: usize) -> bool {
    text.len() == length
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
