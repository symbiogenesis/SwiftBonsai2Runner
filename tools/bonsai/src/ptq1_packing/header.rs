use std::collections::BTreeSet;
use std::fs::File;
use std::io::{BufReader, Read as _};

use sha2::{Digest as _, Sha256};

use super::Deadline;

const HEADER_LIMIT: u64 = 256 * 1024 * 1024;
const ELEMENT_LIMIT: u64 = 8 * 1024 * 1024;

pub(super) struct Tensor {
    pub(super) kind: u32,
    pub(super) offset: u64,
    pub(super) bytes: u64,
}

pub(super) struct Header {
    pub(super) tensors: Vec<Tensor>,
    pub(super) data_offset: u64,
    pub(super) normalized_digest: Vec<u8>,
}

struct Reader<'a> {
    file: BufReader<&'a mut File>,
    digest: Sha256,
    position: u64,
    file_bytes: u64,
    elements: u64,
    deadline: &'a Deadline,
}

impl Reader<'_> {
    fn read(&mut self, bytes: &mut [u8], masked: bool) -> crate::Result<()> {
        self.deadline.check()?;
        let end = self
            .position
            .checked_add(u64::try_from(bytes.len()).map_err(|error| error.to_string())?)
            .filter(|end| *end <= HEADER_LIMIT && *end <= self.file_bytes)
            .ok_or("PTQ1 packing header is truncated or exceeds its byte bound.")?;
        self.file
            .read_exact(bytes)
            .map_err(|error| error.to_string())?;
        if masked {
            self.digest.update(vec![0; bytes.len()]);
        } else {
            self.digest.update(&*bytes);
        }
        self.position = end;
        Ok(())
    }

    fn u32(&mut self, masked: bool) -> crate::Result<u32> {
        let mut bytes = [0; 4];
        self.read(&mut bytes, masked)?;
        Ok(u32::from_le_bytes(bytes))
    }

    fn u64(&mut self, masked: bool) -> crate::Result<u64> {
        let mut bytes = [0; 8];
        self.read(&mut bytes, masked)?;
        Ok(u64::from_le_bytes(bytes))
    }

    fn skip(&mut self, mut bytes: u64) -> crate::Result<()> {
        let mut buffer = [0; 4096];
        while bytes != 0 {
            let count = usize::try_from(bytes.min(4096)).map_err(|error| error.to_string())?;
            self.read(&mut buffer[..count], false)?;
            bytes -= u64::try_from(count).map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    fn text(&mut self) -> crate::Result<String> {
        let size = self.u64(false)?;
        if size > 1024 {
            return Err("PTQ1 packing metadata/tensor name exceeds its bound.".into());
        }
        let mut bytes = vec![0; usize::try_from(size).map_err(|error| error.to_string())?];
        self.read(&mut bytes, false)?;
        String::from_utf8(bytes).map_err(|error| error.to_string())
    }

    fn value(&mut self, tag: u32, depth: u32) -> crate::Result<()> {
        self.elements += 1;
        if depth > 4 || self.elements > ELEMENT_LIMIT {
            return Err("PTQ1 packing metadata exceeds its element/depth bound.".into());
        }
        match tag {
            0 | 1 | 7 => self.skip(1),
            2 | 3 => self.skip(2),
            4..=6 => self.skip(4),
            10..=12 => self.skip(8),
            8 => {
                let size = self.u64(false)?;
                if size > 8 * 1024 * 1024 {
                    return Err("PTQ1 packing metadata string exceeds its byte bound.".into());
                }
                self.skip(size)
            }
            9 => {
                let item = self.u32(false)?;
                let count = self.u64(false)?;
                if count > ELEMENT_LIMIT.saturating_sub(self.elements) {
                    return Err("PTQ1 packing metadata array exceeds its element bound.".into());
                }
                for _ in 0..count {
                    self.value(item, depth + 1)?;
                }
                Ok(())
            }
            _ => Err("PTQ1 packing metadata has an unsupported tag.".into()),
        }
    }
}

fn tensor_bytes(kind: u32, elements: u64, row: u64) -> crate::Result<u64> {
    let (block, bytes) = match kind {
        0 => (1, 4),
        30 => (1, 2),
        142 => (128, 34),
        143 => (128, 28),
        _ => return Err("PTQ1 packing tensor encoding is outside the pinned Swift subset.".into()),
    };
    if !row.is_multiple_of(block) {
        return Err("PTQ1 packing tensor row has a partial quantization block.".into());
    }
    (elements / block)
        .checked_mul(bytes)
        .ok_or_else(|| "PTQ1 packing tensor extent overflowed.".into())
}

#[expect(
    clippy::too_many_lines,
    reason = "Walk the independent GGUF header in serialized field order."
)]
pub(super) fn read(file: &mut File, packed: bool, deadline: &Deadline) -> crate::Result<Header> {
    let file_bytes = file.metadata().map_err(|error| error.to_string())?.len();
    let mut reader = Reader {
        file: BufReader::new(file),
        digest: Sha256::new(),
        position: 0,
        file_bytes,
        elements: 0,
        deadline,
    };
    if reader.u32(false)? != 0x4655_4747 || reader.u32(false)? != 3 {
        return Err("PTQ1 packing requires little-endian GGUF version 3.".into());
    }
    let tensor_count = reader.u64(false)?;
    let metadata_count = reader.u64(false)?;
    if tensor_count == 0 || tensor_count > 1000 || metadata_count > 10_000 {
        return Err("PTQ1 packing header inventory exceeds the pinned Swift subset.".into());
    }
    let mut metadata_names = BTreeSet::new();
    let mut file_type = None;
    let mut alignment = 32_u64;
    for _ in 0..metadata_count {
        let name = reader.text()?;
        if !metadata_names.insert(name.clone()) {
            return Err("PTQ1 packing header has duplicate metadata names.".into());
        }
        let tag = reader.u32(false)?;
        match name.as_str() {
            "general.file_type" if tag == 4 => file_type = Some(reader.u32(true)?),
            "general.alignment" if tag == 4 => alignment = u64::from(reader.u32(false)?),
            _ => reader.value(tag, 0)?,
        }
    }
    if !(if packed {
        file_type == Some(143)
    } else {
        matches!(file_type, Some(141 | 142))
    }) || !alignment.is_power_of_two()
        || alignment > 1_048_576
    {
        return Err(
            "PTQ1 packing file type or alignment differs from the supported export.".into(),
        );
    }
    let mut tensor_names = BTreeSet::new();
    let mut tensors = Vec::new();
    for _ in 0..tensor_count {
        if !tensor_names.insert(reader.text()?) {
            return Err("PTQ1 packing header has duplicate tensor names.".into());
        }
        let dimensions = reader.u32(false)?;
        if !(1..=4).contains(&dimensions) {
            return Err("PTQ1 packing tensor rank is outside 1..4.".into());
        }
        let mut elements = 1_u64;
        let mut row = 0;
        for dimension in 0..dimensions {
            let extent = reader.u64(false)?;
            if dimension == 0 {
                row = extent;
            }
            elements = elements
                .checked_mul(extent)
                .filter(|value| *value > 0)
                .ok_or("PTQ1 packing tensor has zero or overflowing dimensions.")?;
        }
        let kind = reader.u32(true)?;
        if (packed && kind == 142) || (!packed && kind == 143) {
            return Err("PTQ1 packing tensor type is in the wrong representation.".into());
        }
        let offset = reader.u64(true)?;
        if !offset.is_multiple_of(alignment) {
            return Err("PTQ1 packing tensor offset is misaligned.".into());
        }
        tensors.push(Tensor {
            kind,
            offset,
            bytes: tensor_bytes(kind, elements, row)?,
        });
    }
    let data_offset = reader.position.next_multiple_of(alignment);
    reader.skip(data_offset - reader.position)?;
    let mut extents = tensors.iter().collect::<Vec<_>>();
    extents.sort_unstable_by_key(|tensor| tensor.offset);
    let mut end = data_offset;
    for tensor in extents {
        let start = data_offset
            .checked_add(tensor.offset)
            .ok_or("PTQ1 packing tensor offset overflowed.")?;
        let next = start
            .checked_add(tensor.bytes)
            .filter(|next| *next <= file_bytes)
            .ok_or("PTQ1 packing tensor extends past the artifact.")?;
        if start < end {
            return Err("PTQ1 packing tensor extents overlap.".into());
        }
        end = next;
    }
    Ok(Header {
        tensors,
        data_offset,
        normalized_digest: reader.digest.finalize().to_vec(),
    })
}
