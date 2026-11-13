//! Bounded GGUF tokenizer metadata and shared GGUF wire primitives.
#![forbid(unsafe_code)]
#![expect(
    clippy::redundant_pub_crate,
    reason = "Keep GGUF metadata parsing private to the speech crate."
)]

use std::{
    collections::HashMap,
    fs::File,
    io::{self, BufReader, Read},
    path::Path,
};

const MAGIC: &[u8; 4] = b"GGUF";
const MAX_TENSORS: u64 = 1_000_000;
const MAX_KV_ENTRIES: u64 = 65_536;
const MAX_KEY_BYTES: u64 = 65_536;
const MAX_STRING_BYTES: u64 = 16 * 1024 * 1024;
const MAX_ARRAY_ELEMENTS: u64 = 1_000_000;
const MAX_RETAINED_STRING_BYTES: usize = 64 * 1024 * 1024;

const TYPE_UINT8: u32 = 0;
const TYPE_INT8: u32 = 1;
const TYPE_UINT16: u32 = 2;
const TYPE_INT16: u32 = 3;
const TYPE_UINT32: u32 = 4;
const TYPE_INT32: u32 = 5;
const TYPE_FLOAT32: u32 = 6;
const TYPE_BOOL: u32 = 7;
const TYPE_STRING: u32 = 8;
const TYPE_ARRAY: u32 = 9;
const TYPE_UINT64: u32 = 10;
const TYPE_INT64: u32 = 11;
const TYPE_FLOAT64: u32 = 12;

pub(super) fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Invalid speech tokenizer metadata.",
    )
}

enum Entry {
    StringArray(Vec<Box<[u8]>>),
    OtherArray,
    U32(u32),
    OtherScalar,
}

/// Bounded GGUF metadata table retaining string arrays and `u32` scalars.
pub(super) struct Gguf {
    keys: HashMap<Box<[u8]>, usize>,
    entries: Vec<Entry>,
}

impl Gguf {
    pub(super) fn open(path: &Path) -> io::Result<Self> {
        let file = File::open(path)?;
        Self::from_reader(BufReader::new(file))
    }

    pub(super) fn from_reader<R: Read>(mut reader: R) -> io::Result<Self> {
        let mut magic = [0_u8; 4];
        reader.read_exact(&mut magic)?;
        if &magic != MAGIC {
            return Err(invalid());
        }
        let version = read_u32(&mut reader)?;
        if !matches!(version, 2 | 3) {
            return Err(invalid());
        }
        let tensor_count = read_u64(&mut reader)?;
        let kv_count = read_u64(&mut reader)?;
        if tensor_count > MAX_TENSORS || kv_count > MAX_KV_ENTRIES {
            return Err(invalid());
        }
        let kv_len = usize::try_from(kv_count).map_err(|_| invalid())?;
        let mut keys = HashMap::with_capacity(kv_len);
        let mut entries = Vec::with_capacity(kv_len);
        let mut retained_bytes = 0_usize;

        for index in 0..kv_len {
            let key = read_key(&mut reader)?;
            if keys.insert(key, index).is_some() {
                return Err(invalid());
            }
            let kind = read_u32(&mut reader)?;
            let entry = match kind {
                TYPE_UINT32 => Entry::U32(read_u32(&mut reader)?),
                TYPE_STRING => {
                    let len = read_u64(&mut reader)?;
                    if len > MAX_STRING_BYTES {
                        return Err(invalid());
                    }
                    skip_bytes(&mut reader, len)?;
                    Entry::OtherScalar
                }
                TYPE_ARRAY => read_array(&mut reader, &mut retained_bytes)?,
                scalar => {
                    let width = scalar_width(scalar).ok_or_else(invalid)?;
                    skip_bytes(&mut reader, width)?;
                    Entry::OtherScalar
                }
            };
            entries.push(entry);
        }

        Ok(Self { keys, entries })
    }

    pub(super) fn find(&self, name: &[u8]) -> Option<usize> {
        self.keys.get(name).copied()
    }

    pub(super) fn string_count(&self, key: usize) -> io::Result<usize> {
        match self.entries.get(key) {
            Some(Entry::StringArray(values)) => Ok(values.len()),
            _ => Err(invalid()),
        }
    }

    pub(super) fn string(&self, key: usize, index: usize) -> io::Result<&[u8]> {
        match self.entries.get(key) {
            Some(Entry::StringArray(values)) => {
                values.get(index).map(Box::as_ref).ok_or_else(invalid)
            }
            _ => Err(invalid()),
        }
    }

    pub(super) fn u32_value(&self, key: usize) -> io::Result<u32> {
        match self.entries.get(key) {
            Some(Entry::U32(value)) => Ok(*value),
            _ => Err(invalid()),
        }
    }
}

pub(super) const fn scalar_width(kind: u32) -> Option<u64> {
    match kind {
        TYPE_UINT8 | TYPE_INT8 | TYPE_BOOL => Some(1),
        TYPE_UINT16 | TYPE_INT16 => Some(2),
        TYPE_UINT32 | TYPE_INT32 | TYPE_FLOAT32 => Some(4),
        TYPE_UINT64 | TYPE_INT64 | TYPE_FLOAT64 => Some(8),
        _ => None,
    }
}

pub(super) fn read_u32(reader: &mut impl Read) -> io::Result<u32> {
    let mut bytes = [0_u8; 4];
    reader.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

pub(super) fn read_u64(reader: &mut impl Read) -> io::Result<u64> {
    let mut bytes = [0_u8; 8];
    reader.read_exact(&mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
}

fn read_key(reader: &mut impl Read) -> io::Result<Box<[u8]>> {
    let len = read_u64(reader)?;
    if len == 0 || len > MAX_KEY_BYTES {
        return Err(invalid());
    }
    let len = usize::try_from(len).map_err(|_| invalid())?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(len).map_err(|_| invalid())?;
    bytes.resize(len, 0);
    reader.read_exact(&mut bytes)?;
    if bytes.contains(&0) {
        return Err(invalid());
    }
    Ok(bytes.into_boxed_slice())
}

fn read_array(reader: &mut impl Read, retained_bytes: &mut usize) -> io::Result<Entry> {
    let element_kind = read_u32(reader)?;
    let count = read_u64(reader)?;
    if count > MAX_ARRAY_ELEMENTS {
        return Err(invalid());
    }
    if element_kind == TYPE_STRING {
        let count = usize::try_from(count).map_err(|_| invalid())?;
        let mut values = Vec::new();
        values.try_reserve_exact(count).map_err(|_| invalid())?;
        for _ in 0..count {
            let len = read_u64(reader)?;
            if len > MAX_STRING_BYTES {
                return Err(invalid());
            }
            let len = usize::try_from(len).map_err(|_| invalid())?;
            *retained_bytes = retained_bytes
                .checked_add(len)
                .filter(|&total| total <= MAX_RETAINED_STRING_BYTES)
                .ok_or_else(invalid)?;
            let mut raw = Vec::new();
            raw.try_reserve_exact(len).map_err(|_| invalid())?;
            raw.resize(len, 0);
            reader.read_exact(&mut raw)?;
            // Preserve the retired C-string handoff: bytes after an embedded NUL are ignored.
            if let Some(nul) = raw.iter().position(|&byte| byte == 0) {
                raw.truncate(nul);
            }
            values.push(raw.into_boxed_slice());
        }
        return Ok(Entry::StringArray(values));
    }
    let width = scalar_width(element_kind).ok_or_else(invalid)?;
    let total = count.checked_mul(width).ok_or_else(invalid)?;
    skip_bytes(reader, total)?;
    Ok(Entry::OtherArray)
}

fn skip_bytes(reader: &mut impl Read, count: u64) -> io::Result<()> {
    if count == 0 {
        return Ok(());
    }
    let mut skipped = 0_u64;
    let mut scratch = [0_u8; 1024];
    while skipped < count {
        let step = usize::try_from((count - skipped).min(1024)).map_err(|_| invalid())?;
        reader.read_exact(&mut scratch[..step])?;
        skipped += u64::try_from(step).map_err(|_| invalid())?;
    }
    Ok(())
}
