//! Strict public safetensors and fixed adapter descriptors.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read as _;
use std::path::Path;

use super::{decode, require, text};
use serde_json::{Value, json};

pub(super) fn tensor_header(raw: &[u8], file_bytes: u64) -> crate::Result<Value> {
    let base = u64::try_from(raw.len()).map_err(|error| error.to_string())? + 8;
    require(
        (2..=4_194_304).contains(&raw.len()) && base <= file_bytes,
        "safetensors header exceeds bound",
    )?;
    let value = decode(raw)?;
    let values = value.as_object().ok_or("invalid safetensors root")?;
    require(values.len() <= 8193, "safetensors inventory exceeds bound")?;
    let mut descriptors = serde_json::Map::new();
    let mut ranges = Vec::new();
    for (name, record) in values {
        if name == "__metadata__" {
            let metadata = record.as_object().ok_or("invalid safetensors metadata")?;
            require(
                metadata.len() <= 64
                    && metadata.iter().all(|(key, value)| {
                        key.len() <= 512 && value.as_str().is_some_and(|s| s.len() <= 4096)
                    }),
                "invalid safetensors metadata",
            )?;
            continue;
        }
        require(
            !name.is_empty() && name.len() <= 512 && !name.contains('\0'),
            "invalid safetensors tensor name",
        )?;
        let record_object = record.as_object().ok_or("invalid safetensors tensor")?;
        require(
            record_object.len() == 3
                && record_object.contains_key("dtype")
                && record_object.contains_key("shape")
                && record_object.contains_key("data_offsets"),
            "safetensors tensor schema differs",
        )?;
        let width: u64 = match text(record, "dtype")? {
            "BOOL" | "U8" | "I8" => 1,
            "U16" | "I16" | "F16" | "BF16" => 2,
            "U32" | "I32" | "F32" => 4,
            "U64" | "I64" | "F64" => 8,
            _ => return Err("unsupported safetensors dtype".into()),
        };
        let shape = record["shape"].as_array().ok_or("invalid tensor shape")?;
        require(shape.len() <= 8, "safetensors rank exceeds bound")?;
        let mut dimensions = Vec::new();
        for value in shape {
            let dimension = value
                .as_u64()
                .filter(|d| *d <= 1_000_000_000)
                .ok_or("invalid tensor dimension")?;
            dimensions.push(dimension);
        }
        // Zero-size tensors remain valid even when preceding nonzero dimensions
        // would overflow a machine integer; no payload allocation is requested.
        let elements = if dimensions.contains(&0) {
            0
        } else {
            dimensions.iter().try_fold(1_u64, |count, &dimension| {
                count.checked_mul(dimension).ok_or("tensor shape overflow")
            })?
        };
        let offsets = record["data_offsets"]
            .as_array()
            .filter(|a| a.len() == 2)
            .ok_or("invalid tensor offsets")?;
        let begin = offsets[0].as_u64().ok_or("invalid tensor offset")?;
        let end = offsets[1].as_u64().ok_or("invalid tensor offset")?;
        require(
            begin <= end
                && end <= file_bytes
                && end - begin
                    == elements
                        .checked_mul(width)
                        .ok_or("tensor byte extent overflow")?
                && end <= file_bytes - base,
            "tensor shape/storage extent differs",
        )?;
        descriptors.insert(name.clone(), json!({"dtype":record["dtype"],"shape":shape,"absoluteByteBegin":base + begin,"absoluteByteEnd":base + end}));
        if begin != end {
            ranges.push((begin, end));
        }
    }
    ranges.sort_unstable();
    let mut cursor = 0;
    for (begin, end) in ranges {
        require(begin == cursor, "safetensors payload hole or overlap")?;
        cursor = end;
    }
    require(
        !descriptors.is_empty() && base + cursor == file_bytes,
        "unindexed safetensors payload",
    )?;
    Ok(Value::Object(descriptors))
}

pub(super) fn safetensors_header(path: &Path) -> crate::Result<Value> {
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let length = file.metadata().map_err(|error| error.to_string())?.len();
    let mut prefix = [0_u8; 8];
    file.read_exact(&mut prefix)
        .map_err(|error| error.to_string())?;
    let header_bytes = u64::from_le_bytes(prefix);
    require(
        (2..=4_194_304).contains(&header_bytes)
            && header_bytes.checked_add(8).is_some_and(|n| n <= length),
        "safetensors header exceeds bound",
    )?;
    let mut raw = vec![0; usize::try_from(header_bytes).map_err(|error| error.to_string())?];
    file.read_exact(&mut raw)
        .map_err(|error| error.to_string())?;
    Ok(
        json!({"descriptors":tensor_header(&raw,length)?,"raw":String::from_utf8(raw).map_err(|error| error.to_string())?}),
    )
}

pub(super) struct Reader<'a> {
    pub(super) bytes: &'a [u8],
    pub(super) cursor: usize,
}
impl<'a> Reader<'a> {
    pub(super) const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, cursor: 0 }
    }
    pub(super) fn take(&mut self, size: usize) -> crate::Result<&'a [u8]> {
        let end = self
            .cursor
            .checked_add(size)
            .ok_or("binary reference extent overflow")?;
        let value = self
            .bytes
            .get(self.cursor..end)
            .ok_or("truncated binary reference")?;
        self.cursor = end;
        Ok(value)
    }
    pub(super) fn skip(&mut self, size: u64) -> crate::Result<()> {
        self.take(usize::try_from(size).map_err(|error| error.to_string())?)?;
        Ok(())
    }
    pub(super) fn u32(&mut self) -> crate::Result<u32> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().map_err(|_| "truncated integer")?,
        ))
    }
    pub(super) fn u64(&mut self) -> crate::Result<u64> {
        Ok(u64::from_le_bytes(
            self.take(8)?.try_into().map_err(|_| "truncated integer")?,
        ))
    }
    fn string(&mut self) -> crate::Result<String> {
        let count = self.u64()?;
        require(count <= 4096, "oversized GGUF text")?;
        String::from_utf8(
            self.take(usize::try_from(count).map_err(|error| error.to_string())?)?
                .to_vec(),
        )
        .map_err(|error| error.to_string())
    }
    fn metadata(&mut self, kind: u32, depth: u32) -> crate::Result<Value> {
        require(depth <= 2, "GGUF metadata nesting exceeds bound")?;
        Ok(match kind {
            0 => u64::from(self.take(1)?[0]).into(),
            1 => i64::from(i8::from_le_bytes([self.take(1)?[0]])).into(),
            2 => u64::from(u16::from_le_bytes(
                self.take(2)?.try_into().map_err(|_| "truncated integer")?,
            ))
            .into(),
            3 => i64::from(i16::from_le_bytes(
                self.take(2)?.try_into().map_err(|_| "truncated integer")?,
            ))
            .into(),
            4 => u64::from(self.u32()?).into(),
            5 => i64::from(i32::from_le_bytes(
                self.take(4)?.try_into().map_err(|_| "truncated integer")?,
            ))
            .into(),
            6 => {
                let value = f32::from_bits(self.u32()?);
                require(value.is_finite(), "nonfinite GGUF metadata")?;
                Value::from(value)
            }
            7 => {
                let value = self.take(1)?[0];
                require(value <= 1, "invalid GGUF bool")?;
                (value != 0).into()
            }
            8 => self.string()?.into(),
            9 => {
                let element = self.u32()?;
                let count = self.u64()?;
                require(count <= 65_536, "GGUF array count exceeds bound")?;
                let mut values = Vec::new();
                for _ in 0..count {
                    values.push(self.metadata(element, depth + 1)?);
                }
                values.into()
            }
            10 => self.u64()?.into(),
            11 => i64::from_le_bytes(self.take(8)?.try_into().map_err(|_| "truncated integer")?)
                .into(),
            12 => {
                let value = f64::from_bits(self.u64()?);
                require(value.is_finite(), "nonfinite GGUF metadata")?;
                Value::from(value)
            }
            _ => return Err("unsupported GGUF metadata type".into()),
        })
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "The fixed GGUF adapter contract validates every descriptor before exposing any factor bytes."
)]
pub(super) fn adapter(raw: &[u8]) -> crate::Result<Value> {
    let mut reader = Reader::new(raw);
    require(
        reader.take(4)? == b"GGUF" && reader.u32()? == 3,
        "adapter is not GGUF v3",
    )?;
    let count = reader.u64()?;
    let metadata_count = reader.u64()?;
    require(
        count == 258 && metadata_count <= 128,
        "adapter inventory differs",
    )?;
    let mut metadata = BTreeMap::new();
    for _ in 0..metadata_count {
        let name = reader.string()?;
        let kind = reader.u32()?;
        let value = reader.metadata(kind, 0)?;
        require(
            metadata.insert(name, value).is_none(),
            "duplicate GGUF metadata",
        )?;
    }
    let field = |name: &str| metadata.get(name).unwrap_or(&Value::Null);
    require(
        (field("general.alignment").is_null() || field("general.alignment") == 32)
            && field("general.architecture") == "qwen35"
            && field("general.type") == "adapter"
            && field("adapter.type") == "lora"
            && field("adapter.lora_alpha").as_f64() == Some(1.0),
        "adapter metadata differs",
    )?;
    let mut descriptors = BTreeMap::new();
    for _ in 0..count {
        let name = reader.string()?;
        require(
            name.len() <= 512 && reader.u32()? == 2,
            "adapter name/rank differs",
        )?;
        let shape = [reader.u64()?, reader.u64()?];
        require(
            shape.iter().all(|n| (1..=248_320).contains(n)) && reader.u32()? == 0,
            "adapter tensor shape/type differs",
        )?;
        let offset = reader.u64()?;
        require(
            offset % 32 == 0
                && offset <= u64::try_from(raw.len()).map_err(|error| error.to_string())?,
            "misaligned GGUF factor",
        )?;
        require(
            descriptors.insert(name, (shape, offset)).is_none(),
            "duplicate GGUF factor",
        )?;
    }
    let base = reader.cursor.checked_add(31).ok_or("GGUF base overflow")? & !31;
    let mut expected = BTreeMap::from([("token_embd.weight".to_owned(), [1_u64, 248_320])]);
    for block in 0..64 {
        expected.insert(format!("blk.{block}.ffn_down.weight"), [17_408, 1]);
        expected.insert(
            format!(
                "blk.{block}.{}.weight",
                if block % 4 == 3 {
                    "attn_output"
                } else {
                    "ssm_out"
                }
            ),
            [6144, 1],
        );
    }
    let mut factors = serde_json::Map::new();
    let mut extents = Vec::new();
    for (site, a_shape) in expected {
        for (suffix, expected_shape) in [(".lora_a", a_shape), (".lora_b", [1, 5120])] {
            let name = site.clone() + suffix;
            let (shape, offset) = descriptors
                .get(&name)
                .ok_or("adapter writer inventory differs")?;
            require(*shape == expected_shape, "adapter writer shape differs")?;
            let begin = base
                .checked_add(usize::try_from(*offset).map_err(|error| error.to_string())?)
                .ok_or("GGUF extent overflow")?;
            let size =
                usize::try_from(shape[0] * shape[1] * 4).map_err(|error| error.to_string())?;
            let end = begin.checked_add(size).ok_or("GGUF extent overflow")?;
            let bytes = raw
                .get(begin..end)
                .ok_or("GGUF factor extends beyond file")?;
            factors.insert(name, crate::identity::hex(bytes).into());
            extents.push((begin, end));
        }
    }
    require(
        descriptors.len() == factors.len(),
        "adapter writer inventory differs",
    )?;
    extents.sort_unstable();
    let mut cursor = base;
    for (begin, end) in extents {
        require(begin >= cursor, "GGUF factors overlap")?;
        cursor = end;
    }
    Ok(Value::Object(factors))
}
