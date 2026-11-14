//! Bounded, native-independent GGUF v3 index and tokenizer admission.
//!
//! This checks structure, not artifact authenticity or inference equivalence.
//! The service must retain its digest-verified file guard through recognition.
#![forbid(unsafe_code)]
#![expect(
    clippy::redundant_pub_crate,
    reason = "Keep the source-bound GGUF preflight private to the ASR crate."
)]

use std::{
    collections::BTreeMap,
    io::{self, BufReader, Read, Seek, SeekFrom},
};

#[path = "intake_decoder.rs"]
mod decoder;
#[path = "intake_encoder.rs"]
mod encoder;
#[path = "intake_model.rs"]
mod model;

const MAX_ITEMS: u64 = 262_144;
const MAX_METADATA: usize = 64 * 1024 * 1024;
const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024 * 1024;

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Invalid Qwen3-ASR GGUF structure.",
    )
}

#[derive(Debug)]
enum Value {
    Integer { kind: u32, value: u64 },
    Float { kind: u32, value: f64 },
    Boolean(bool),
    Text(String),
    Strings(Vec<String>),
    Array { kind: u32, count: usize },
    UnsetToken,
    Other,
}

#[derive(Debug)]
struct Tensor {
    shape: Vec<u64>,
    kind: u32,
    offset: u64,
    bytes: u64,
}

struct Index {
    metadata: BTreeMap<String, Value>,
    tensors: BTreeMap<String, Tensor>,
}

struct Reader<R> {
    source: R,
    length: u64,
    remaining: usize,
}

impl<R: Read + Seek> Reader<R> {
    fn bytes<const N: usize>(&mut self) -> io::Result<[u8; N]> {
        let mut bytes = [0; N];
        self.source.read_exact(&mut bytes)?;
        Ok(bytes)
    }

    fn u32(&mut self) -> io::Result<u32> {
        Ok(u32::from_le_bytes(self.bytes()?))
    }

    fn u64(&mut self) -> io::Result<u64> {
        Ok(u64::from_le_bytes(self.bytes()?))
    }

    fn charge(&mut self, bytes: usize) -> io::Result<()> {
        self.remaining = self.remaining.checked_sub(bytes).ok_or_else(invalid)?;
        Ok(())
    }

    fn text(&mut self, limit: usize) -> io::Result<String> {
        let count = usize::try_from(self.u64()?).map_err(|_| invalid())?;
        if count > limit {
            return Err(invalid());
        }
        self.charge(count.checked_add(32).ok_or_else(invalid)?)?;
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(count).map_err(|_| invalid())?;
        bytes.resize(count, 0);
        self.source.read_exact(&mut bytes)?;
        String::from_utf8(bytes).map_err(|_| invalid())
    }

    fn skip(&mut self, bytes: u64) -> io::Result<()> {
        let end = self
            .source
            .stream_position()?
            .checked_add(bytes)
            .ok_or_else(invalid)?;
        if end > self.length {
            return Err(invalid());
        }
        self.source.seek(SeekFrom::Start(end))?;
        Ok(())
    }

    fn value(&mut self) -> io::Result<Value> {
        let kind = self.u32()?;
        Ok(match kind {
            0 => Value::Integer {
                kind,
                value: u64::from(self.bytes::<1>()?[0]),
            },
            1 => signed(kind, i64::from(i8::from_le_bytes(self.bytes()?))),
            2 => Value::Integer {
                kind,
                value: u64::from(u16::from_le_bytes(self.bytes()?)),
            },
            3 => signed(kind, i64::from(i16::from_le_bytes(self.bytes()?))),
            4 => Value::Integer {
                kind,
                value: u64::from(self.u32()?),
            },
            5 => signed(kind, i64::from(i32::from_le_bytes(self.bytes()?))),
            6 => Value::Float {
                kind,
                value: f64::from(f32::from_bits(self.u32()?)),
            },
            7 => match self.bytes::<1>()?[0] {
                0 => Value::Boolean(false),
                1 => Value::Boolean(true),
                _ => return Err(invalid()),
            },
            8 => Value::Text(self.text(65_536)?),
            9 => self.array()?,
            10 => Value::Integer {
                kind,
                value: self.u64()?,
            },
            11 => signed(kind, i64::from_le_bytes(self.bytes()?)),
            12 => Value::Float {
                kind,
                value: f64::from_bits(self.u64()?),
            },
            _ => return Err(invalid()),
        })
    }

    fn array(&mut self) -> io::Result<Value> {
        let kind = self.u32()?;
        let count = self.u64()?;
        if count > MAX_ITEMS {
            return Err(invalid());
        }
        let count = usize::try_from(count).map_err(|_| invalid())?;
        if kind == 8 {
            self.charge(count.checked_mul(size_of::<String>()).ok_or_else(invalid)?)?;
            let mut strings = Vec::new();
            strings.try_reserve_exact(count).map_err(|_| invalid())?;
            for _ in 0..count {
                strings.push(self.text(4096)?);
            }
            return Ok(Value::Strings(strings));
        }
        let width = match kind {
            0 | 1 | 7 => 1,
            2 | 3 => 2,
            4..=6 => 4,
            10..=12 => 8,
            // GGUF prohibits arrays of arrays.
            _ => return Err(invalid()),
        };
        let bytes = count.checked_mul(width).ok_or_else(invalid)?;
        self.charge(bytes)?;
        if kind == 7 {
            for _ in 0..count {
                if self.bytes::<1>()?[0] > 1 {
                    return Err(invalid());
                }
            }
        } else {
            self.skip(u64::try_from(bytes).map_err(|_| invalid())?)?;
        }
        Ok(Value::Array { kind, count })
    }

    fn index(&mut self) -> io::Result<Index> {
        if self.bytes::<8>()? != *b"GGUF\x03\0\0\0" {
            return Err(invalid());
        }
        let tensors = self.u64()?;
        let metadata = self.u64()?;
        if !(1..=8192).contains(&tensors) || !(1..=65_536).contains(&metadata) {
            return Err(invalid());
        }
        let mut values = BTreeMap::new();
        for _ in 0..metadata {
            self.charge(128)?;
            let name = self.text(1024)?;
            if name.is_empty() || name.contains('\0') || values.contains_key(&name) {
                return Err(invalid());
            }
            values.insert(name, self.value()?);
        }
        let alignment = match values.get("general.alignment") {
            None => 32,
            Some(Value::Integer { kind: 4, value })
                if value.is_power_of_two() && *value <= 4096 =>
            {
                *value
            }
            _ => return Err(invalid()),
        };
        let mut entries = BTreeMap::new();
        for _ in 0..tensors {
            self.charge(512)?;
            let name = self.text(1024)?;
            let dimensions = self.u32()?;
            if name.is_empty()
                || name.contains('\0')
                || entries.contains_key(&name)
                || !(1..=4).contains(&dimensions)
            {
                return Err(invalid());
            }
            let mut shape = Vec::new();
            let mut elements = 1_u64;
            for _ in 0..dimensions {
                let dimension = self.u64()?;
                if dimension == 0 {
                    return Err(invalid());
                }
                elements = elements.checked_mul(dimension).ok_or_else(invalid)?;
                shape.push(dimension);
            }
            let kind = self.u32()?;
            let offset = self.u64()?;
            let (block, width) = layout(kind)?;
            if !shape[0].is_multiple_of(block) || !offset.is_multiple_of(alignment) {
                return Err(invalid());
            }
            let bytes = (elements / block).checked_mul(width).ok_or_else(invalid)?;
            entries.insert(
                name,
                Tensor {
                    shape,
                    kind,
                    offset,
                    bytes,
                },
            );
        }
        let end = self.source.stream_position()?;
        let data = end.checked_add(alignment - 1).ok_or_else(invalid)? / alignment * alignment;
        if data > self.length {
            return Err(invalid());
        }
        let mut ranges = Vec::with_capacity(entries.len());
        for tensor in entries.values_mut() {
            tensor.offset = data.checked_add(tensor.offset).ok_or_else(invalid)?;
            let end = tensor
                .offset
                .checked_add(tensor.bytes)
                .ok_or_else(invalid)?;
            if end > self.length {
                return Err(invalid());
            }
            ranges.push((tensor.offset, end));
        }
        ranges.sort_unstable();
        if ranges.windows(2).any(|pair| pair[0].1 > pair[1].0) {
            return Err(invalid());
        }
        Ok(Index {
            metadata: values,
            tensors: entries,
        })
    }
}

fn signed(kind: u32, value: i64) -> Value {
    if kind == 5 && value == -1 {
        return Value::UnsetToken;
    }
    u64::try_from(value).map_or(Value::Other, |value| Value::Integer { kind, value })
}

// Source-defined GGML block widths for the native ASR linear allowlist.
fn layout(kind: u32) -> io::Result<(u64, u64)> {
    match kind {
        0 => Ok((1, 4)),
        1 | 30 => Ok((1, 2)),
        2 => Ok((32, 18)),
        3 => Ok((32, 20)),
        6 => Ok((32, 22)),
        7 => Ok((32, 24)),
        8 => Ok((32, 34)),
        12 => Ok((256, 144)),
        13 => Ok((256, 176)),
        14 => Ok((256, 210)),
        _ => Err(invalid()),
    }
}

#[cfg(any(feature = "native-engine", test))]
pub(super) fn validate(source: &mut (impl Read + Seek), length: u64) -> io::Result<()> {
    model::validate(&read_index(source, length)?)
}

fn read_index(source: &mut (impl Read + Seek), length: u64) -> io::Result<Index> {
    if !(24..=MAX_FILE_BYTES).contains(&length) {
        return Err(invalid());
    }
    source.seek(SeekFrom::Start(0))?;
    let mut reader = Reader {
        source: BufReader::with_capacity(256 * 1024, source),
        length,
        remaining: MAX_METADATA,
    };
    reader.index()
}

pub(super) fn decoder(
    source: &mut (impl Read + Seek),
    length: u64,
    precision: crate::decoder::CachePrecision,
    cancel: &mut impl FnMut() -> bool,
) -> io::Result<crate::decoder::Model> {
    let index = read_index(source, length)?;
    model::validate(&index)?;
    decoder::load(index, source, precision, cancel)
}

pub(super) fn encoder(
    source: &mut (impl Read + Seek),
    length: u64,
    cancel: &mut impl FnMut() -> bool,
) -> io::Result<(crate::encoder::Encoder, crate::frontend::Frontend)> {
    let index = read_index(source, length)?;
    model::validate(&index)?;
    encoder::load(index, source, cancel)
}

pub(super) fn tokenizer(
    source: &mut (impl Read + Seek),
    length: u64,
) -> io::Result<crate::tokenizer::Tokenizer> {
    let mut index = read_index(source, length)?;
    model::validate(&index)?;
    if let Some(Value::Text(pre)) = index.metadata.get("tokenizer.ggml.pre")
        && !matches!(pre.as_str(), "" | "qwen2")
    {
        return Err(invalid());
    }
    let Some(Value::Strings(tokens)) = index.metadata.remove("tokenizer.ggml.tokens") else {
        return Err(invalid());
    };
    let Some(Value::Strings(merges)) = index.metadata.remove("tokenizer.ggml.merges") else {
        return Err(invalid());
    };
    let Some(Value::Integer { value: eos, .. }) =
        index.metadata.remove("tokenizer.ggml.eos_token_id")
    else {
        return Err(invalid());
    };
    crate::tokenizer::Tokenizer::from_tables(
        tokens,
        merges,
        u32::try_from(eos).map_err(|_| invalid())?,
    )
}

#[cfg(test)]
#[path = "intake_tests.rs"]
mod tests;
