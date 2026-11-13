//! Decode admitted source-bound tensor ranges into the independent Rust decoder.

use super::{Index, Value, invalid};
use crate::decoder::{Block, CachePrecision, Config, Model, Projection};
use std::io::{self, Read, Seek, SeekFrom};

struct Loader<'a, R> {
    index: Index,
    source: &'a mut R,
    remaining: u64,
}

impl<R: Read + Seek> Loader<'_, R> {
    fn count(&self, suffix: &str) -> io::Result<usize> {
        match self
            .index
            .metadata
            .get(&format!("stt.qwen3_asr.decoder.{suffix}"))
        {
            Some(Value::Integer { value, .. }) => usize::try_from(*value).map_err(|_| invalid()),
            _ => Err(invalid()),
        }
    }

    #[expect(
        clippy::cast_possible_truncation,
        reason = "GGUF F32 metadata was widened exactly by the admitted reader; reconstruct the original finite scalar."
    )]
    #[allow(
        clippy::as_conversions,
        reason = "This conversion restores a verified finite GGUF F32 scalar after exact widening."
    )]
    fn scalar(&self, suffix: &str) -> io::Result<f32> {
        match self
            .index
            .metadata
            .get(&format!("stt.qwen3_asr.decoder.{suffix}"))
        {
            Some(Value::Float { kind: 6, value }) if value.is_finite() => Ok(*value as f32),
            _ => Err(invalid()),
        }
    }

    fn bytes(
        &mut self,
        name: &str,
        cancel: &mut impl FnMut() -> bool,
    ) -> io::Result<(u32, Vec<u8>)> {
        let tensor = self.index.tensors.get(name).ok_or_else(invalid)?;
        self.remaining = self
            .remaining
            .checked_sub(tensor.bytes)
            .ok_or_else(invalid)?;
        let count = usize::try_from(tensor.bytes).map_err(|_| invalid())?;
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(count).map_err(io::Error::other)?;
        self.source.seek(SeekFrom::Start(tensor.offset))?;
        while bytes.len() < count {
            if cancel() {
                return Err(io::Error::from(io::ErrorKind::Interrupted));
            }
            let start = bytes.len();
            let end = count.min(start + 64 * 1024);
            bytes.resize(end, 0);
            self.source.read_exact(&mut bytes[start..end])?;
        }
        Ok((tensor.kind, bytes))
    }

    fn vector(&mut self, name: &str, cancel: &mut impl FnMut() -> bool) -> io::Result<Vec<f32>> {
        let (kind, bytes) = self.bytes(name, cancel)?;
        if kind != 0 || !bytes.len().is_multiple_of(4) {
            return Err(invalid());
        }
        let mut values = Vec::new();
        values
            .try_reserve_exact(bytes.len() / 4)
            .map_err(io::Error::other)?;
        for bytes in bytes.as_chunks::<4>().0 {
            values.push(f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]));
        }
        Ok(values)
    }

    fn projection(
        &mut self,
        name: &str,
        cancel: &mut impl FnMut() -> bool,
    ) -> io::Result<Projection> {
        let tensor = self.index.tensors.get(name).ok_or_else(invalid)?;
        let input = usize::try_from(tensor.shape[0]).map_err(|_| invalid())?;
        let output =
            usize::try_from(*tensor.shape.get(1).ok_or_else(invalid)?).map_err(|_| invalid())?;
        match tensor.kind {
            0 => Projection::f32(input, output, self.vector(name, cancel)?),
            1 => Projection::f16(input, output, &self.bytes(name, cancel)?.1),
            8 => Projection::q8(input, output, self.bytes(name, cancel)?.1),
            _ => Err(invalid()),
        }
    }
}

pub(super) fn load<R: Read + Seek>(
    index: Index,
    source: &mut R,
    precision: CachePrecision,
    cancel: &mut impl FnMut() -> bool,
) -> io::Result<Model> {
    let mut loader = Loader {
        index,
        source,
        remaining: 8 * 1024 * 1024 * 1024,
    };
    let config = Config {
        hidden: loader.count("hidden_size")?,
        heads: loader.count("n_heads")?,
        kv_heads: loader.count("n_kv_heads")?,
        head_dim: loader.count("head_dim")?,
        intermediate: loader.count("intermediate_size")?,
        context: loader.count("max_position_embeddings")?.min(8192),
        epsilon: loader.scalar("rms_norm_eps")?,
        rope_theta: loader.scalar("rope_theta")?,
        cache_precision: precision,
    };
    let count = loader.count("n_layers")?;
    // Validate geometry before any weight-sized allocation; Model::new checks
    // each admitted layer again after the complete tensor set is loaded.
    if !(1..=128).contains(&count)
        || !(1..=8192).contains(&config.hidden)
        || !(1..=128).contains(&config.heads)
        || config.kv_heads == 0
        || !config.heads.is_multiple_of(config.kv_heads)
        || config.head_dim == 0
        || config.head_dim > 256
        || !config.head_dim.is_multiple_of(2)
        || !(1..=65_536).contains(&config.intermediate)
        || config.context == 0
        || !config.epsilon.is_finite()
        || config.epsilon <= 0.0
        || !config.rope_theta.is_finite()
        || config.rope_theta <= 0.0
    {
        return Err(invalid());
    }
    let embedding = loader.projection("dec.token_embd.weight", cancel)?;
    let output_norm = loader.vector("dec.output_norm.weight", cancel)?;
    let mut blocks = Vec::new();
    blocks.try_reserve_exact(count).map_err(io::Error::other)?;
    for layer in 0..count {
        let prefix = format!("dec.blocks.{layer}.");
        blocks.push(Block {
            attention_norm: loader.vector(&(prefix.clone() + "norm_attn.weight"), cancel)?,
            ffn_norm: loader.vector(&(prefix.clone() + "norm_ffn.weight"), cancel)?,
            query: loader.projection(&(prefix.clone() + "attn.q.weight"), cancel)?,
            key: loader.projection(&(prefix.clone() + "attn.k.weight"), cancel)?,
            value: loader.projection(&(prefix.clone() + "attn.v.weight"), cancel)?,
            attention_out: loader.projection(&(prefix.clone() + "attn.o.weight"), cancel)?,
            query_norm: loader.vector(&(prefix.clone() + "attn.q_norm.weight"), cancel)?,
            key_norm: loader.vector(&(prefix.clone() + "attn.k_norm.weight"), cancel)?,
            gate: loader.projection(&(prefix.clone() + "ffn.gate.weight"), cancel)?,
            up: loader.projection(&(prefix.clone() + "ffn.up.weight"), cancel)?,
            down: loader.projection(&(prefix + "ffn.down.weight"), cancel)?,
        });
    }
    Model::new(config, embedding, blocks, output_norm)
}

#[cfg(test)]
mod tests {
    use super::{Index, Loader, Projection};
    use crate::intake::Tensor;
    use std::{
        collections::BTreeMap,
        io::{self, Cursor},
    };

    #[test]
    fn rust_asr_decoder_wire_weight_ranges_and_storage_types() -> io::Result<()> {
        let mut bytes = vec![0; 128];
        bytes[8..24].copy_from_slice(
            &[1.0_f32, -2.0, 3.0, 4.0]
                .iter()
                .flat_map(|value| value.to_le_bytes())
                .collect::<Vec<_>>(),
        );
        bytes[32..40].copy_from_slice(
            &[0x3c00_u16, 0xc000, 0x4200, 0x4400]
                .iter()
                .flat_map(|value| value.to_le_bytes())
                .collect::<Vec<_>>(),
        );
        bytes[64..66].copy_from_slice(&0x3800_u16.to_le_bytes());
        bytes[66..98].fill(2);
        let mut tensors = BTreeMap::new();
        for (name, kind, offset, count, shape) in [
            ("float", 0, 8, 16, vec![2, 2]),
            ("half", 1, 32, 8, vec![2, 2]),
            ("quant", 8, 64, 34, vec![32, 1]),
        ] {
            tensors.insert(
                name.into(),
                Tensor {
                    shape,
                    kind,
                    offset,
                    bytes: count,
                },
            );
        }
        let mut source = Cursor::new(bytes);
        let mut loader = Loader {
            index: Index {
                metadata: BTreeMap::new(),
                tensors,
            },
            source: &mut source,
            remaining: 128,
        };
        for name in ["float", "half"] {
            let projection = loader.projection(name, &mut || false)?;
            assert_eq!(
                projection.multiply(&[5.0, 6.0], &mut || false)?,
                [-7.0, 39.0]
            );
        }
        assert_eq!(
            loader
                .projection("quant", &mut || false)?
                .multiply(&[1.0; 32], &mut || false)?,
            [32.0]
        );
        assert!(loader.projection("missing", &mut || false).is_err());
        assert!(loader.projection("quant", &mut || true).is_err());
        assert!(Projection::f16(1, 1, &[0]).is_err());
        Ok(())
    }
}
