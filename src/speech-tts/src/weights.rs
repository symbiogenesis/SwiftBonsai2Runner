//! Bounded Rust GGUF tensor loading and packed GGML weight decoding.
//!
//! `Q4_K/Q6_K` layouts follow the retained MIT-licensed GGML revision recorded in
//! model.lock.json and LICENSE.ggml. No native library participates in loading
//! or decoding. The caller owns the artifact's complete digest verification.
#![forbid(unsafe_code)]
#![expect(
    clippy::redundant_pub_crate,
    reason = "The speech modules share this private weight reader."
)]

use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    io::{self, BufReader, Read, Seek, SeekFrom},
    path::Path,
};

use crate::engine::{LinearProjection, WeightQuantization};

const MAX_ITEMS: u64 = 131_072;
const MAX_METADATA_BYTES: usize = 128 * 1024 * 1024;
const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024 * 1024;

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Invalid or unsupported speech GGUF weights.",
    )
}

#[derive(Debug)]
enum Metadata {
    U32(u32),
    F32(f32),
    String(String),
    U32Array(Vec<u32>),
    Other,
}

#[derive(Debug)]
struct Tensor {
    shape: Vec<usize>,
    kind: u32,
    offset: u64,
    bytes: usize,
}

/// Validated tensor index retaining a non-writable model file handle.
pub(crate) struct Weights {
    file: File,
    metadata: BTreeMap<String, Metadata>,
    tensors: BTreeMap<String, Tensor>,
}

fn read_u32(reader: &mut impl Read) -> io::Result<u32> {
    let mut bytes = [0; 4];
    reader.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_u64(reader: &mut impl Read) -> io::Result<u64> {
    let mut bytes = [0; 8];
    reader.read_exact(&mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
}

fn text(reader: &mut impl Read, budget: &mut usize, limit: usize) -> io::Result<String> {
    let length = usize::try_from(read_u64(reader)?).map_err(|_| invalid())?;
    if length > limit || length > *budget {
        return Err(invalid());
    }
    *budget -= length;
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes)?;
    String::from_utf8(bytes).map_err(|_| invalid())
}

fn skip(reader: &mut (impl Read + Seek), bytes: u64, length: u64) -> io::Result<()> {
    let end = reader
        .stream_position()?
        .checked_add(bytes)
        .ok_or_else(invalid)?;
    if end > length {
        return Err(invalid());
    }
    reader.seek(SeekFrom::Start(end))?;
    Ok(())
}

fn scalar_width(kind: u32) -> io::Result<u64> {
    match kind {
        0 | 1 | 7 => Ok(1),
        2 | 3 => Ok(2),
        4..=6 => Ok(4),
        10..=12 => Ok(8),
        _ => Err(invalid()),
    }
}

fn metadata_value(
    reader: &mut (impl Read + Seek),
    length: u64,
    budget: &mut usize,
) -> io::Result<Metadata> {
    Ok(match read_u32(reader)? {
        4 => Metadata::U32(read_u32(reader)?),
        5 => {
            let value = read_u32(reader)?;
            if value <= i32::MAX.unsigned_abs() {
                Metadata::U32(value)
            } else {
                Metadata::Other
            }
        }
        6 => {
            let value = f32::from_bits(read_u32(reader)?);
            if !value.is_finite() {
                return Err(invalid());
            }
            Metadata::F32(value)
        }
        8 => Metadata::String(text(reader, budget, 16 * 1024 * 1024)?),
        9 => {
            let kind = read_u32(reader)?;
            let count = read_u64(reader)?;
            if count > 1_000_000 {
                return Err(invalid());
            }
            if kind == 4 || kind == 5 {
                let count = usize::try_from(count).map_err(|_| invalid())?;
                let bytes = count.checked_mul(4).ok_or_else(invalid)?;
                if bytes > *budget {
                    return Err(invalid());
                }
                *budget -= bytes;
                let mut values = Vec::with_capacity(count);
                for _ in 0..count {
                    values.push(read_u32(reader)?);
                }
                if kind == 5 && values.iter().any(|value| *value > i32::MAX.unsigned_abs()) {
                    Metadata::Other
                } else {
                    Metadata::U32Array(values)
                }
            } else if kind == 8 {
                for _ in 0..count {
                    let _ = text(reader, budget, 16 * 1024 * 1024)?;
                }
                Metadata::Other
            } else {
                skip(
                    reader,
                    count.checked_mul(scalar_width(kind)?).ok_or_else(invalid)?,
                    length,
                )?;
                Metadata::Other
            }
        }
        kind => {
            skip(reader, scalar_width(kind)?, length)?;
            Metadata::Other
        }
    })
}

impl Weights {
    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        if !path.is_absolute() {
            return Err(invalid());
        }
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt as _;
            // Read sharing only: replacements and writes cannot race loading.
            options.share_mode(1).custom_flags(0x0020_0000);
        }
        let file = options.open(path)?;
        let properties = file.metadata()?;
        if !properties.is_file() || properties.len() > MAX_FILE_BYTES {
            return Err(invalid());
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt as _;
            if properties.file_attributes() & 0x400 != 0 {
                return Err(invalid());
            }
        }
        Self::read(file, properties.len())
    }

    fn read(file: File, length: u64) -> io::Result<Self> {
        let mut reader = BufReader::new(file);
        let mut magic = [0; 4];
        reader.read_exact(&mut magic)?;
        if &magic != b"GGUF" || !matches!(read_u32(&mut reader)?, 2 | 3) {
            return Err(invalid());
        }
        let tensor_count = read_u64(&mut reader)?;
        let metadata_count = read_u64(&mut reader)?;
        if tensor_count == 0 || tensor_count > MAX_ITEMS || metadata_count > MAX_ITEMS {
            return Err(invalid());
        }
        let mut budget = MAX_METADATA_BYTES;
        let mut metadata = BTreeMap::new();
        for _ in 0..metadata_count {
            let key = text(&mut reader, &mut budget, 65_536)?;
            if key.is_empty() || key.contains('\0') || metadata.contains_key(&key) {
                return Err(invalid());
            }
            metadata.insert(key, metadata_value(&mut reader, length, &mut budget)?);
        }
        let alignment = match metadata.get("general.alignment") {
            None => 32,
            Some(Metadata::U32(value)) if value.is_power_of_two() && *value <= 4096 => {
                u64::from(*value)
            }
            _ => return Err(invalid()),
        };
        let mut tensors = BTreeMap::new();
        for _ in 0..tensor_count {
            let name = text(&mut reader, &mut budget, 65_536)?;
            let dimensions = read_u32(&mut reader)?;
            if name.is_empty()
                || name.contains('\0')
                || tensors.contains_key(&name)
                || !(1..=4).contains(&dimensions)
            {
                return Err(invalid());
            }
            let mut shape = Vec::new();
            let mut elements = 1_usize;
            for _ in 0..dimensions {
                let size = usize::try_from(read_u64(&mut reader)?).map_err(|_| invalid())?;
                if size == 0 {
                    return Err(invalid());
                }
                elements = elements.checked_mul(size).ok_or_else(invalid)?;
                shape.push(size);
            }
            let kind = read_u32(&mut reader)?;
            let offset = read_u64(&mut reader)?;
            let (block, bytes) = packed_layout(kind)?;
            if !shape[0].is_multiple_of(block) || !offset.is_multiple_of(alignment) {
                return Err(invalid());
            }
            let bytes = (elements / block).checked_mul(bytes).ok_or_else(invalid)?;
            tensors.insert(
                name,
                Tensor {
                    shape,
                    kind,
                    offset,
                    bytes,
                },
            );
        }
        let position = reader.stream_position()?;
        let data = position.checked_add(alignment - 1).ok_or_else(invalid)? / alignment * alignment;
        let mut ranges = Vec::with_capacity(tensors.len());
        for tensor in tensors.values_mut() {
            tensor.offset = data.checked_add(tensor.offset).ok_or_else(invalid)?;
            let end = tensor
                .offset
                .checked_add(u64::try_from(tensor.bytes).map_err(|_| invalid())?)
                .ok_or_else(invalid)?;
            if tensor.offset < data || end > length {
                return Err(invalid());
            }
            ranges.push((tensor.offset, end));
        }
        ranges.sort_unstable();
        if ranges.windows(2).any(|pair| pair[0].1 > pair[1].0) {
            return Err(invalid());
        }
        Ok(Self {
            file: reader.into_inner(),
            metadata,
            tensors,
        })
    }

    pub(crate) fn u32(&self, key: &str) -> io::Result<u32> {
        match self.metadata.get(key) {
            Some(Metadata::U32(value)) => Ok(*value),
            _ => Err(invalid()),
        }
    }

    pub(crate) fn f32_metadata(&self, key: &str) -> io::Result<f32> {
        match self.metadata.get(key) {
            Some(Metadata::F32(value)) => Ok(*value),
            _ => Err(invalid()),
        }
    }

    pub(crate) fn string(&self, key: &str) -> io::Result<&str> {
        match self.metadata.get(key) {
            Some(Metadata::String(value)) => Ok(value),
            _ => Err(invalid()),
        }
    }

    pub(crate) fn u32_array(&self, key: &str) -> io::Result<Vec<u32>> {
        match self.metadata.get(key) {
            Some(Metadata::U32Array(values)) => Ok(values.clone()),
            _ => Err(invalid()),
        }
    }

    pub(crate) fn tensor_shape(&self, name: &str) -> io::Result<&[usize]> {
        self.tensors
            .get(name)
            .map(|tensor| tensor.shape.as_slice())
            .ok_or_else(invalid)
    }

    fn tensor_bytes(&mut self, name: &str, shape: &[usize]) -> io::Result<(u32, Vec<u8>)> {
        let tensor = self.tensors.get(name).ok_or_else(invalid)?;
        if tensor.shape != shape {
            return Err(invalid());
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(tensor.bytes)
            .map_err(|_| invalid())?;
        bytes.resize(tensor.bytes, 0);
        self.file.seek(SeekFrom::Start(tensor.offset))?;
        self.file.read_exact(&mut bytes)?;
        validate_packed(tensor.kind, &bytes)?;
        Ok((tensor.kind, bytes))
    }

    pub(crate) fn tensor_f32(&mut self, name: &str, shape: &[usize]) -> io::Result<Vec<f32>> {
        let (kind, bytes) = self.tensor_bytes(name, shape)?;
        let (block, size) = packed_layout(kind)?;
        let count = (bytes.len() / size)
            .checked_mul(block)
            .ok_or_else(invalid)?;
        let mut output = Vec::new();
        output.try_reserve_exact(count).map_err(|_| invalid())?;
        output.resize(count, 0.0);
        for (source, destination) in bytes.chunks_exact(size).zip(output.chunks_exact_mut(block)) {
            decode_block(kind, source, destination);
        }
        if output.iter().any(|value| !value.is_finite()) {
            return Err(invalid());
        }
        Ok(output)
    }

    pub(crate) fn projection(
        &mut self,
        name: &str,
        in_dim: usize,
        out_dim: usize,
    ) -> io::Result<LinearProjection> {
        let (ggml_type, bytes) = self.tensor_bytes(name, &[in_dim, out_dim])?;
        LinearProjection::new(
            name,
            in_dim,
            out_dim,
            WeightQuantization::Packed { ggml_type, bytes },
            None,
        )
    }
}

pub(crate) fn packed_layout(kind: u32) -> io::Result<(usize, usize)> {
    match kind {
        0 => Ok((1, 4)),
        1 => Ok((1, 2)),
        8 => Ok((32, 34)),
        12 => Ok((256, 144)),
        14 => Ok((256, 210)),
        _ => Err(invalid()),
    }
}

fn half(bytes: &[u8]) -> f32 {
    let bits = u16::from_le_bytes([bytes[0], bytes[1]]);
    let fraction = u32::from(bits & 1023);
    let exponent = u32::from((bits >> 10) & 31);
    let sign = u32::from(bits & 0x8000) << 16;
    match exponent {
        0 => {
            let value = f32::from(u16::try_from(fraction).unwrap_or(0)) * (1.0 / 16_777_216.0);
            if sign == 0 { value } else { -value }
        }
        31 => f32::from_bits(sign | 0x7f80_0000 | (fraction << 13)),
        _ => f32::from_bits(sign | ((exponent + 112) << 23) | (fraction << 13)),
    }
}

pub(crate) fn validate_packed(kind: u32, bytes: &[u8]) -> io::Result<()> {
    let (_, size) = packed_layout(kind)?;
    if !bytes.len().is_multiple_of(size) {
        return Err(invalid());
    }
    for block in bytes.chunks_exact(size) {
        let finite = match kind {
            0 => f32::from_le_bytes([block[0], block[1], block[2], block[3]]).is_finite(),
            1 | 8 => half(block).is_finite(),
            12 => half(block).is_finite() && half(&block[2..]).is_finite(),
            14 => half(&block[208..]).is_finite(),
            _ => false,
        };
        if !finite {
            return Err(invalid());
        }
    }
    Ok(())
}

// The caller validates the exact kind, source block size and destination width.
#[expect(
    clippy::suboptimal_flops,
    reason = "Preserve GGML's separately rounded Q4_K scale and minimum operations."
)]
pub(crate) fn decode_block(kind: u32, source: &[u8], destination: &mut [f32]) {
    match kind {
        0 => destination[0] = f32::from_le_bytes([source[0], source[1], source[2], source[3]]),
        1 => destination[0] = half(source),
        8 => {
            let scale = half(source);
            for (value, byte) in destination.iter_mut().zip(&source[2..]) {
                *value = scale * f32::from(i8::from_le_bytes([*byte]));
            }
        }
        12 => {
            let scale = half(source);
            let minimum = half(&source[2..]);
            let scales = &source[4..16];
            for group in 0..8 {
                let (s, m) = if group < 4 {
                    (scales[group] & 63, scales[group + 4] & 63)
                } else {
                    (
                        (scales[group + 4] & 15) | ((scales[group - 4] >> 6) << 4),
                        (scales[group + 4] >> 4) | ((scales[group] >> 6) << 4),
                    )
                };
                for index in 0..32 {
                    let byte = source[16 + (group / 2) * 32 + index];
                    let quant = if group % 2 == 0 { byte & 15 } else { byte >> 4 };
                    destination[group * 32 + index] =
                        scale * f32::from(s) * f32::from(quant) - minimum * f32::from(m);
                }
            }
        }
        14 => {
            let scale = half(&source[208..]);
            for half_index in 0..2 {
                for index in 0..32 {
                    let high = source[128 + half_index * 32 + index];
                    for quarter in 0..4 {
                        let low = source[half_index * 64 + (quarter % 2) * 32 + index];
                        let nibble = if quarter < 2 { low & 15 } else { low >> 4 };
                        let quant = i16::from(nibble | (((high >> (quarter * 2)) & 3) << 4)) - 32;
                        let s = i8::from_le_bytes([
                            source[192 + half_index * 8 + quarter * 2 + index / 16]
                        ]);
                        destination[half_index * 128 + quarter * 32 + index] =
                            scale * f32::from(s) * f32::from(quant);
                    }
                }
            }
        }
        _ => destination.fill(f32::NAN),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    struct Fixture(std::path::PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    fn fixture(tensors: &[(&str, &[u64], u32, u64)], payload: &[u8]) -> io::Result<Fixture> {
        let mut bytes = b"GGUF".to_vec();
        bytes.extend(3_u32.to_le_bytes());
        bytes.extend(
            u64::try_from(tensors.len())
                .map_err(|_| invalid())?
                .to_le_bytes(),
        );
        bytes.extend(0_u64.to_le_bytes());
        for &(name, shape, kind, offset) in tensors {
            bytes.extend(
                u64::try_from(name.len())
                    .map_err(|_| invalid())?
                    .to_le_bytes(),
            );
            bytes.extend(name.bytes());
            bytes.extend(
                u32::try_from(shape.len())
                    .map_err(|_| invalid())?
                    .to_le_bytes(),
            );
            for value in shape {
                bytes.extend(value.to_le_bytes());
            }
            bytes.extend(kind.to_le_bytes());
            bytes.extend(offset.to_le_bytes());
        }
        bytes.resize(bytes.len().div_ceil(32) * 32, 0);
        bytes.extend(payload);
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| invalid())?
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "Bonsai-RustWeights-{}-{nonce}.gguf",
            std::process::id()
        ));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        file.write_all(&bytes)?;
        Ok(Fixture(path))
    }

    #[test]
    fn gguf_tensor_loading_preserves_shape_and_rejects_invalid_index_ranges() -> io::Result<()> {
        let valid = fixture(&[("weight", &[2, 1], 0, 0)], &[0, 0, 128, 63, 0, 0, 0, 64])?;
        let mut loaded = Weights::open(&valid.0)?;
        assert_eq!(loaded.tensor_shape("weight")?, [2, 1]);
        assert_eq!(loaded.tensor_f32("weight", &[2, 1])?, [1.0, 2.0]);
        assert!(loaded.tensor_f32("weight", &[1, 2]).is_err());
        assert!(loaded.tensor_f32("missing", &[2, 1]).is_err());
        let projection = loaded.projection("weight", 2, 1)?;
        assert_eq!(projection.row(0)?, [1.0, 2.0]);
        assert!(projection.row(1).is_err());
        drop(loaded);
        for tensors in [
            vec![
                ("duplicate", &[1_u64][..], 0, 0),
                ("duplicate", &[1][..], 0, 32),
            ],
            vec![("first", &[1][..], 0, 0), ("second", &[1][..], 0, 0)],
            vec![("unaligned", &[1][..], 0, 1)],
            vec![("truncated", &[100][..], 0, 0)],
            vec![("quant-row", &[1][..], 8, 0)],
            vec![("unsupported", &[1][..], 99, 0)],
            vec![("empty", &[0][..], 0, 0)],
            vec![("overflow", &[u64::MAX, 2][..], 0, 0)],
        ] {
            let invalid_file = fixture(&tensors, &[0; 64])?;
            assert!(Weights::open(&invalid_file.0).is_err());
        }
        Ok(())
    }

    #[test]
    fn gguf_integer_metadata_accepts_nonnegative_signed_rates_and_rejects_negative_rates()
    -> io::Result<()> {
        let mut bytes = Vec::new();
        bytes.extend(9_u32.to_le_bytes());
        bytes.extend(5_u32.to_le_bytes());
        bytes.extend(4_u64.to_le_bytes());
        for value in [8_i32, 5, 4, 3] {
            bytes.extend(value.to_le_bytes());
        }
        let length = u64::try_from(bytes.len()).map_err(|_| invalid())?;
        let mut budget = MAX_METADATA_BYTES;
        let value = metadata_value(&mut io::Cursor::new(&bytes), length, &mut budget)?;
        assert!(matches!(value, Metadata::U32Array(values) if values == [8,5,4,3]));
        bytes[16..20].copy_from_slice(&(-1_i32).to_le_bytes());
        assert!(matches!(
            metadata_value(&mut io::Cursor::new(bytes), length, &mut budget)?,
            Metadata::Other
        ));
        Ok(())
    }

    #[test]
    fn packed_q4_k_scales_minima_and_q6_k_signed_planes_decode_independently() -> io::Result<()> {
        let mut q4 = vec![0; 144];
        q4[..2].copy_from_slice(&0x3c00_u16.to_le_bytes());
        q4[2..4].copy_from_slice(&0x3800_u16.to_le_bytes());
        q4[4..8].fill(1);
        q4[8..12].fill(2);
        q4[12..16].fill(0x21);
        q4[16..].fill(0x73);
        let mut output = [0.0; 256];
        validate_packed(12, &q4)?;
        decode_block(12, &q4, &mut output);
        for (group, values) in output.as_chunks::<32>().0.iter().enumerate() {
            assert!(
                values.iter().all(|value| value.to_bits()
                    == if group % 2 == 0 { 2.0_f32 } else { 6.0_f32 }.to_bits())
            );
        }
        let mut q6 = vec![0; 210];
        q6[..128].fill(0x21);
        q6[128..192].fill(0b11_10_01_00);
        for (index, value) in q6[192..208].iter_mut().enumerate() {
            *value = i8::try_from(index).map_err(|_| invalid())?.to_le_bytes()[0];
        }
        q6[192] = (-2_i8).to_le_bytes()[0];
        q6[208..].copy_from_slice(&0x3c00_u16.to_le_bytes());
        validate_packed(14, &q6)?;
        decode_block(14, &q6, &mut output);
        assert_eq!(output[0], 62.0);
        assert_eq!(output[16], -31.0);
        assert_eq!(output[32], -30.0);
        assert_eq!(output[64], 8.0);
        assert_eq!(output[96], 108.0);
        q6[208..].copy_from_slice(&0x7c00_u16.to_le_bytes());
        assert!(validate_packed(14, &q6).is_err());
        Ok(())
    }

    #[test]
    fn packed_projection_computes_without_expanding_or_accepting_partial_rows() -> io::Result<()> {
        for kind in [0, 1, 8, 12, 14] {
            let (width, size) = packed_layout(kind)?;
            let mut bytes = vec![0; size];
            match kind {
                0 => bytes.copy_from_slice(&1.0_f32.to_le_bytes()),
                1 => bytes.copy_from_slice(&0x3c00_u16.to_le_bytes()),
                8 => {
                    bytes[..2].copy_from_slice(&0x3c00_u16.to_le_bytes());
                    bytes[2..].fill(1);
                }
                12 => {
                    bytes[..2].copy_from_slice(&0x3c00_u16.to_le_bytes());
                    bytes[4..8].fill(1);
                    bytes[12..16].fill(1);
                    bytes[16..].fill(0x11);
                }
                14 => {
                    bytes[..128].fill(0x11);
                    bytes[128..192].fill(0b10_10_10_10);
                    bytes[192..208].fill(1);
                    bytes[208..].copy_from_slice(&0x3c00_u16.to_le_bytes());
                }
                _ => return Err(invalid()),
            }
            let projection = LinearProjection::new(
                "fixture",
                width,
                1,
                WeightQuantization::Packed {
                    ggml_type: kind,
                    bytes: bytes.clone(),
                },
                Some(vec![2.0]),
            )?;
            assert_eq!(projection.row(0)?, vec![1.0; width]);
            let expected = f32::from(u16::try_from(width).map_err(|_| invalid())?) + 2.0;
            assert_eq!(projection.forward_vec(&vec![1.0; width], None)?, [expected]);
            bytes.pop();
            assert!(
                LinearProjection::new(
                    "truncated",
                    width,
                    1,
                    WeightQuantization::Packed {
                        ggml_type: kind,
                        bytes
                    },
                    None
                )
                .is_err()
            );
        }
        Ok(())
    }

    #[test]
    fn packed_half_and_q8_keep_signed_zero_subnormals_and_signed_quants() -> io::Result<()> {
        assert_eq!(
            half(&0x8000_u16.to_le_bytes()).to_bits(),
            (-0.0_f32).to_bits()
        );
        assert_eq!(half(&1_u16.to_le_bytes()), 1.0 / 16_777_216.0);
        assert_eq!(half(&0x3c00_u16.to_le_bytes()), 1.0);
        let mut bytes = [0; 34];
        bytes[..2].copy_from_slice(&0x3800_u16.to_le_bytes());
        bytes[2] = (-128_i8).to_le_bytes()[0];
        bytes[3] = 127;
        let mut decoded = [0.0; 32];
        validate_packed(8, &bytes)?;
        decode_block(8, &bytes, &mut decoded);
        assert_eq!(decoded[..2], [-64.0, 63.5]);
        Ok(())
    }
}
