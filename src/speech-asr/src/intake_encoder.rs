//! Source-bound CNN/encoder and exact supported frontend metadata intake.

use super::{Index, Value, decoder::Loader, invalid};
use crate::{
    encoder::{Block, Config, Encoder, Linear, Norm, Weights},
    frontend::Frontend,
    protocol::EncoderLayout,
};
use std::io::{self, Read, Seek};

fn count(index: &Index, suffix: &str) -> io::Result<usize> {
    match index
        .metadata
        .get(&format!("stt.qwen3_asr.encoder.{suffix}"))
    {
        Some(Value::Integer { value, .. }) => usize::try_from(*value).map_err(|_| invalid()),
        _ => Err(invalid()),
    }
}

#[expect(
    clippy::float_cmp,
    reason = "Admit only exact fixed frontend metadata values, never approximate a different model frontend."
)]
fn frontend_metadata(index: &Index) -> io::Result<()> {
    for (key, expected) in [
        ("type", "mel"),
        ("window", "hann_periodic"),
        ("normalize", "per_utterance"),
    ] {
        if !matches!(index.metadata.get(&format!("stt.frontend.{key}")), Some(Value::Text(value)) if value == expected)
        {
            return Err(invalid());
        }
    }
    for (key, expected) in [
        ("sample_rate", 16_000),
        ("num_mels", 128),
        ("n_fft", 400),
        ("win_length", 400),
        ("hop_length", 160),
    ] {
        if !matches!(index.metadata.get(&format!("stt.frontend.{key}")), Some(Value::Integer { value, .. }) if *value == expected)
        {
            return Err(invalid());
        }
    }
    for (key, expected) in [
        ("dither", 0.0),
        ("pre_emphasis", 0.0),
        ("f_min", 0.0),
        ("f_max", 8000.0),
    ] {
        if !matches!(index.metadata.get(&format!("stt.frontend.{key}")), Some(Value::Float { value, .. }) if *value == expected)
        {
            return Err(invalid());
        }
    }
    for (key, expected) in [("pad_mode", "reflect"), ("mel_norm", "slaney")] {
        match index.metadata.get(&format!("stt.frontend.{key}")) {
            None => {}
            Some(Value::Text(value)) if value == expected => {}
            _ => return Err(invalid()),
        }
    }
    match index.metadata.get("stt.frontend.center") {
        None | Some(Value::Boolean(true)) => Ok(()),
        _ => Err(invalid()),
    }
}

fn linear<R: Read + Seek>(
    loader: &mut Loader<'_, R>,
    prefix: &str,
    cancel: &mut impl FnMut() -> bool,
) -> io::Result<Linear> {
    Ok(Linear {
        weights: loader.projection(&format!("{prefix}.weight"), cancel)?,
        bias: loader.vector(&format!("{prefix}.bias"), cancel)?,
    })
}
fn norm<R: Read + Seek>(
    loader: &mut Loader<'_, R>,
    prefix: &str,
    cancel: &mut impl FnMut() -> bool,
) -> io::Result<Norm> {
    Ok(Norm {
        weight: loader.vector(&format!("{prefix}.weight"), cancel)?,
        bias: loader.vector(&format!("{prefix}.bias"), cancel)?,
    })
}

pub(super) fn load<R: Read + Seek>(
    index: Index,
    source: &mut R,
    cancel: &mut impl FnMut() -> bool,
) -> io::Result<(Encoder, Frontend)> {
    frontend_metadata(&index)?;
    let c = Config {
        mel_bins: count(&index, "num_mel_bins")?,
        channels: count(&index, "downsample_hidden")?,
        hidden: count(&index, "d_model")?,
        heads: count(&index, "n_heads")?,
        intermediate: count(&index, "ffn_dim")?,
        output: count(&index, "output_dim")?,
        window: count(&index, "n_window")?,
        max_positions: count(&index, "max_source_positions")?,
    };
    let layers = count(&index, "n_layers")?;
    // Reject weight-sized hostile allocations before tensor reads.
    if c.mel_bins != 128
        || !(1..=1024).contains(&c.channels)
        || !(4..=8192).contains(&c.hidden)
        || !c.hidden.is_multiple_of(2)
        || c.heads == 0
        || c.heads > 128
        || !c.hidden.is_multiple_of(c.heads)
        || !(1..=65_536).contains(&c.intermediate)
        || !(1..=8192).contains(&c.output)
        || !(1..=256).contains(&layers)
        || EncoderLayout::new(1, c.window)?.rows_per_chunk > c.max_positions
    {
        return Err(invalid());
    }
    let mut loader = Loader {
        index,
        source,
        remaining: 8 * 1024 * 1024 * 1024,
    };
    let window = if loader.index.tensors.contains_key("frontend.window") {
        Some(loader.vector("frontend.window", cancel)?)
    } else {
        None
    };
    let bank = if loader.index.tensors.contains_key("frontend.mel_filterbank") {
        Some(loader.vector("frontend.mel_filterbank", cancel)?)
    } else {
        None
    };
    let frontend = Frontend::from_checkpoint(window.as_deref(), bank.as_deref())?;
    let mut convolutions = Vec::new();
    for layer in 0..3 {
        convolutions.push(Linear {
            weights: loader.convolution(&format!("enc.conv.{layer}.weight"), cancel)?,
            bias: loader.vector(&format!("enc.conv.{layer}.bias"), cancel)?,
        });
    }
    let mut blocks = Vec::new();
    blocks.try_reserve_exact(layers).map_err(io::Error::other)?;
    for layer in 0..layers {
        let prefix = format!("enc.blocks.{layer}.");
        blocks.push(Block {
            attention_norm: norm(&mut loader, &(prefix.clone() + "norm_attn"), cancel)?,
            ffn_norm: norm(&mut loader, &(prefix.clone() + "norm_ffn"), cancel)?,
            query: linear(&mut loader, &(prefix.clone() + "attn.q"), cancel)?,
            key: linear(&mut loader, &(prefix.clone() + "attn.k"), cancel)?,
            value: linear(&mut loader, &(prefix.clone() + "attn.v"), cancel)?,
            attention_out: linear(&mut loader, &(prefix.clone() + "attn.out"), cancel)?,
            ffn_in: linear(&mut loader, &(prefix.clone() + "ffn.fc1"), cancel)?,
            ffn_out: linear(&mut loader, &(prefix + "ffn.fc2"), cancel)?,
        });
    }
    let weights = Weights {
        convolutions: convolutions.try_into().map_err(|_| invalid())?,
        convolution_out: loader.projection("enc.conv_out.weight", cancel)?,
        blocks,
        output_norm: norm(&mut loader, "enc.ln_post", cancel)?,
        output_in: linear(&mut loader, "enc.proj1", cancel)?,
        output_out: linear(&mut loader, "enc.proj2", cancel)?,
    };
    Ok((Encoder::new(c, weights)?, frontend))
}
