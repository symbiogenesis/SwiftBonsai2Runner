//! Pure-Rust GGUF metadata access and transactional tokenizer ownership.

use std::{
    ffi::{CStr, c_char, c_int, c_void},
    io,
    path::Path,
};

use crate::gguf::{Gguf, invalid};

use super::{Owner, guard, new_owner, owner_mut, reject_stage, replace_owner};

const TOKENS: &CStr = c"tokenizer.ggml.tokens";
const MERGES: &CStr = c"tokenizer.ggml.merges";
const MAX_METADATA_ENTRIES: usize = 1_000_000;

pub(super) trait Metadata {
    fn find(&self, name: &CStr) -> Option<usize>;
    fn string_count(&self, key: usize) -> io::Result<usize>;
    fn string(&self, key: usize, index: usize) -> io::Result<&[u8]>;
    fn u32_value(&self, key: usize) -> io::Result<u32>;
}

impl Metadata for Gguf {
    fn find(&self, name: &CStr) -> Option<usize> {
        Self::find(self, name.to_bytes())
    }

    fn string_count(&self, key: usize) -> io::Result<usize> {
        Self::string_count(self, key)
    }

    fn string(&self, key: usize, index: usize) -> io::Result<&[u8]> {
        Self::string(self, key, index)
    }

    fn u32_value(&self, key: usize) -> io::Result<u32> {
        Self::u32_value(self, key)
    }
}

fn open_gguf(path: &CStr) -> io::Result<Gguf> {
    let path = path.to_str().map_err(|_| invalid())?;
    Gguf::open(Path::new(path))
}

fn candidate(metadata: &impl Metadata) -> io::Result<Box<Owner>> {
    let token_key = metadata.find(TOKENS).ok_or_else(invalid)?;
    let merge_key = metadata.find(MERGES).ok_or_else(invalid)?;
    let tokens = metadata.string_count(token_key)?;
    let merges = metadata.string_count(merge_key)?;
    if tokens > MAX_METADATA_ENTRIES || merges > MAX_METADATA_ENTRIES {
        return Err(invalid());
    }
    let mut owner = new_owner()?;
    for index in 0..tokens {
        reject_stage(2)?;
        owner.tokenizer.add_token(
            metadata.string(token_key, index)?,
            i32::try_from(index).map_err(io::Error::other)?,
        )?;
    }
    for index in 0..merges {
        reject_stage(3)?;
        owner.tokenizer.add_merge(
            metadata.string(merge_key, index)?,
            i32::try_from(index).map_err(io::Error::other)?,
        )?;
    }
    reject_stage(4)?;
    owner.tokenizer.finish()?;
    Ok(owner)
}

pub(super) fn reload_from(slot: &mut *mut c_void, metadata: &impl Metadata) -> io::Result<()> {
    let replacement = candidate(metadata)?;
    replace_owner(slot, replacement);
    Ok(())
}

fn reload(slot: &mut *mut c_void, path: &CStr) -> io::Result<()> {
    let metadata = open_gguf(path)?;
    reload_from(slot, &metadata)
}

fn specials(
    owner: &mut crate::tokenizer::Tokenizer,
    metadata: &impl Metadata,
    keys: impl IntoIterator<Item = impl AsRef<CStr>>,
) -> io::Result<()> {
    for name in keys {
        if let Some(key) = metadata.find(name.as_ref()) {
            let bits = metadata.u32_value(key)?;
            reject_stage(6)?;
            let id = bits.cast_signed();
            #[cfg(test)]
            tests::SPECIAL_IDS.with(|ids| ids.borrow_mut().push(id));
            owner.add_special_id(id)?;
        }
    }
    Ok(())
}

// SAFETY: The generated noncopyable native object supplies an exclusively
// borrowed owner slot containing null or one live Box<Owner> pointer, plus a
// NUL-terminated path. Success replaces that slot; failure leaves it unchanged.
#[unsafe(no_mangle)]
pub(super) extern "C" fn bonsai_qt_bpe_load_gguf(
    slot: *mut *mut c_void,
    path: *const c_char,
) -> bool {
    guard(|| {
        if slot.is_null() || !slot.is_aligned() || path.is_null() {
            return false;
        }
        // SAFETY: The generated object provides one exclusively borrowed live
        // pointer slot. No other references to its value overlap this call.
        let slot = unsafe { &mut *slot };
        if !slot.is_null() && !slot.cast::<Owner>().is_aligned() {
            return false;
        }
        // SAFETY: The native caller borrows its NUL-terminated path until return.
        let path = unsafe { CStr::from_ptr(path) };
        reload(slot, path).is_ok()
    })
    .unwrap_or(false)
}

// SAFETY: The generated wrapper borrows a live exclusive Box<Owner>, a
// NUL-terminated path and key_count readable C-string pointers until return.
#[unsafe(no_mangle)]
pub(super) extern "C" fn bonsai_qt_bpe_load_specials(
    owner: *mut c_void,
    path: *const c_char,
    keys: *const *const c_char,
    key_count: c_int,
) -> c_int {
    guard(|| {
        if path.is_null() {
            return -1;
        }
        let Some(owner) = owner_mut(owner) else {
            return -1;
        };
        let count = usize::try_from(key_count.max(0)).unwrap_or(usize::MAX);
        if count > 64 || (count != 0 && (keys.is_null() || !keys.is_aligned())) {
            return -1;
        }
        let pointers = if count == 0 {
            &[][..]
        } else {
            // SAFETY: The native caller supplies count live pointer entries,
            // disjoint from the mutably borrowed owner; count is bounded above.
            unsafe { std::slice::from_raw_parts(keys, count) }
        };
        let mut names = Vec::new();
        if names.try_reserve_exact(count).is_err() {
            return -1;
        }
        for &name in pointers {
            if name.is_null() {
                return -1;
            }
            // SAFETY: Each borrowed key pointer names a native C string whose
            // storage remains live through this synchronous setup call.
            names.push(unsafe { CStr::from_ptr(name) });
        }
        // SAFETY: The native path storage remains NUL-terminated and live.
        let path = unsafe { CStr::from_ptr(path) };
        let Ok(metadata) = open_gguf(path) else {
            return 0;
        };
        if specials(owner, &metadata, names).is_ok() {
            1
        } else {
            -1
        }
    })
    .unwrap_or(-1)
}

#[cfg(test)]
#[path = "tokenizer_loading_tests.rs"]
mod tests;
