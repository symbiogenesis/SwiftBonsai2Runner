//! Minimal independent GGUF F32 adapter reader; no runtime or model dependency.

use std::collections::{BTreeMap, BTreeSet};

use super::{Result, require};

struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

struct Tensor {
    name: String,
    dimensions: Vec<usize>,
    offset: usize,
    bytes: usize,
}

type Ranges = BTreeMap<String, (Vec<usize>, std::ops::Range<usize>)>;

impl Reader<'_> {
    fn take(&mut self, size: usize) -> Result<&[u8]> {
        let end = self
            .position
            .checked_add(size)
            .ok_or("GGUF extent overflow.")?;
        let result = self
            .bytes
            .get(self.position..end)
            .ok_or("Truncated basis GGUF.")?;
        self.position = end;
        Ok(result)
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().map_err(|_| "Invalid GGUF U32.")?,
        ))
    }

    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(
            self.take(8)?.try_into().map_err(|_| "Invalid GGUF U64.")?,
        ))
    }

    fn count(&mut self, maximum: usize) -> Result<usize> {
        let count = usize::try_from(self.u64()?).map_err(|error| error.to_string())?;
        require(
            count <= maximum,
            "GGUF count exceeds the basis reader bound.",
        )?;
        Ok(count)
    }

    fn text(&mut self) -> Result<String> {
        let length = self.count(4096)?;
        String::from_utf8(self.take(length)?.to_vec()).map_err(|error| error.to_string())
    }

    fn value(&mut self, kind: u32, depth: u32) -> Result<()> {
        require(depth <= 4, "GGUF metadata exceeds its nesting bound.")?;
        let bytes = match kind {
            0 | 1 | 7 => 1,
            2 | 3 => 2,
            4..=6 => 4,
            10..=12 => 8,
            8 => {
                self.text()?;
                return Ok(());
            }
            9 => {
                let element_kind = self.u32()?;
                let count = self.count(65_536)?;
                for _ in 0..count {
                    self.value(element_kind, depth + 1)?;
                }
                return Ok(());
            }
            _ => return Err("Unsupported GGUF metadata type in basis intake.".into()),
        };
        self.take(bytes)?;
        Ok(())
    }

    fn tensors(&mut self, count: usize) -> Result<Vec<Tensor>> {
        let mut names = BTreeSet::new();
        let mut tensors = Vec::new();
        for _ in 0..count {
            let name = self.text()?;
            require(names.insert(name.clone()), "Duplicate GGUF tensor name.")?;
            let rank = self.u32()?;
            require((1..=4).contains(&rank), "Invalid GGUF adapter tensor rank.")?;
            let mut dimensions = Vec::new();
            let mut elements = 1_usize;
            for _ in 0..rank {
                let dimension = self.count(248_320)?;
                require(dimension != 0, "GGUF tensor has an empty dimension.")?;
                elements = elements
                    .checked_mul(dimension)
                    .ok_or("GGUF tensor extent overflow.")?;
                dimensions.push(dimension);
            }
            require(self.u32()? == 0, "Basis adapter tensors must be F32.")?;
            let offset = self.count(self.bytes.len())?;
            require(
                offset.is_multiple_of(32),
                "GGUF tensor offset is misaligned.",
            )?;
            let bytes = elements
                .checked_mul(4)
                .ok_or("GGUF tensor extent overflow.")?;
            tensors.push(Tensor {
                name,
                dimensions,
                offset,
                bytes,
            });
        }
        Ok(tensors)
    }
}

fn ranges(bytes: &[u8]) -> Result<Ranges> {
    let mut reader = Reader { bytes, position: 0 };
    require(
        reader.take(4)? == b"GGUF" && reader.u32()? == 3,
        "Basis adapter must be GGUF v3.",
    )?;
    let tensors = reader.count(258)?;
    let metadata = reader.count(128)?;
    require(tensors == 258, "Approved adapter must have 258 tensors.")?;
    let mut keys = BTreeSet::new();
    for _ in 0..metadata {
        let key = reader.text()?;
        require(keys.insert(key.clone()), "Duplicate GGUF metadata name.")?;
        let kind = reader.u32()?;
        if key == "general.alignment" {
            require(
                kind == 4 && reader.u32()? == 32,
                "Basis adapter requires 32-byte alignment.",
            )?;
        } else {
            reader.value(kind, 0)?;
        }
    }
    let descriptors = reader.tensors(tensors)?;
    let data = reader
        .position
        .checked_next_multiple_of(32)
        .ok_or("GGUF header overflow.")?;
    let mut extents = Vec::new();
    let mut payloads = BTreeMap::new();
    for tensor in descriptors {
        let begin = data
            .checked_add(tensor.offset)
            .ok_or("GGUF tensor offset overflow.")?;
        let end = begin
            .checked_add(tensor.bytes)
            .ok_or("GGUF tensor extent overflow.")?;
        bytes
            .get(begin..end)
            .ok_or("GGUF adapter tensor extends beyond the file.")?;
        extents.push((begin, end));
        payloads.insert(tensor.name, (tensor.dimensions, begin..end));
    }
    extents.sort_unstable();
    require(
        extents.array_windows().all(|[a, b]| a.1 <= b.0),
        "GGUF adapter tensor payloads overlap.",
    )?;
    Ok(payloads)
}

pub(super) fn ssm_vectors(bytes: &[u8]) -> Result<BTreeMap<u32, &[u8]>> {
    let mut vectors = BTreeMap::new();
    for (name, (dimensions, range)) in ranges(bytes)? {
        if let Some(block) = name
            .strip_prefix("blk.")
            .and_then(|name| name.strip_suffix(".ssm_out.weight.lora_a"))
        {
            let block: u32 = block.parse().map_err(|_| "Invalid SSM block name.")?;
            require(
                dimensions == [6144, 1],
                "SSM adapter A shape differs from [6144,1].",
            )?;
            require(
                vectors.insert(block, &bytes[range]).is_none(),
                "Duplicate SSM site.",
            )?;
        }
    }
    require(
        vectors.len() == 48,
        "GGUF adapter must expose all 48 SSM A vectors.",
    )?;
    Ok(vectors)
}

pub(super) fn direction_vectors(bytes: &[u8]) -> Result<BTreeMap<String, &[u8]>> {
    let mut payloads = ranges(bytes)?;
    let mut vectors = BTreeMap::new();
    let mut sites = vec![("token_embd.weight".to_owned(), vec![1, 248_320])];
    for block in 0_u32..64 {
        sites.push((format!("blk.{block}.ffn_down.weight"), vec![17_408, 1]));
        let family = if (block + 1).is_multiple_of(4) {
            "attn_output"
        } else {
            "ssm_out"
        };
        sites.push((format!("blk.{block}.{family}.weight"), vec![6144, 1]));
    }
    for (site, expected_a) in sites {
        let (shape_a, _) = payloads
            .remove(&format!("{site}.lora_a"))
            .ok_or("Approved adapter lacks a required A factor.")?;
        let (shape_b, range) = payloads
            .remove(&format!("{site}.lora_b"))
            .ok_or("Approved adapter lacks a required B factor.")?;
        require(
            shape_a == expected_a && shape_b == [1, 5120],
            "Approved adapter factor dimensions differ from the fixed writer inventory.",
        )?;
        vectors.insert(site, &bytes[range]);
    }
    require(
        payloads.is_empty(),
        "Approved adapter contains unexpected factors.",
    )?;
    Ok(vectors)
}

/// Exact byte ranges for the verified inventory's linear A factors.
#[expect(
    clippy::redundant_pub_crate,
    reason = "The crate-only re-export serves adapter editing without exposing qualification internals publicly."
)]
pub(crate) fn linear_a_ranges(bytes: &[u8]) -> Result<Vec<(String, std::ops::Range<usize>)>> {
    direction_vectors(bytes)?;
    Ok(ranges(bytes)?
        .into_iter()
        .filter_map(|(name, (_, range))| {
            name.strip_suffix(".lora_a")
                .filter(|site| *site != "token_embd.weight")
                .map(|site| (site.to_owned(), range))
        })
        .collect())
}
