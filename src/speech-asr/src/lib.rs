//! Bounded Qwen3-ASR recognition and the independent Rust qualification graph.
//!
//! The default `native-engine` feature retains production native serving. Disable
//! defaults to build the complete Rust graph without a C++ inference dependency.
//! Source-bound model, numerical and application acceptance remain separate gates.
#![forbid(unsafe_code)]

pub mod decoder;
pub mod encoder;
pub mod frontend;
mod intake;
#[cfg(feature = "native-engine")]
mod native;
pub mod protocol;
pub mod rust_engine;
pub mod tokenizer;

#[cfg(feature = "native-engine")]
pub use native::{Cancellation, Recognizer};
use std::io;

/// Required sample rate for normalized mono input.
pub const SAMPLE_RATE: usize = 16_000;
/// Longest admitted clip, in seconds.
pub const MAX_CLIP_SECONDS: usize = 30;

/// Transpose `FastConformer` conv-subsampling activations from `[B, C, T, F]`
/// into time-major `[B, T, C * F]` before the linear encoder projection (`parakeet-rs` `1d46c176`).
///
/// # Errors
/// Rejects zero dimensions or an input slice whose length does not equal `batch * channels * frames * freqs`.
pub fn transpose_conv_subsampling_bctf_to_btcf(
    input: &[f32],
    batch: usize,
    channels: usize,
    frames: usize,
    freqs: usize,
) -> io::Result<Vec<f32>> {
    let total = batch
        .checked_mul(channels)
        .and_then(|v| v.checked_mul(frames))
        .and_then(|v| v.checked_mul(freqs))
        .ok_or_else(|| invalid("FastConformer conv-subsampling dimensions overflow."))?;
    if total == 0 || input.len() != total {
        return Err(invalid(
            "FastConformer conv-subsampling input length does not match [B, C, T, F].",
        ));
    }
    let merged_features = channels * freqs;
    let batch_stride = channels * frames * freqs;
    let channel_stride = frames * freqs;
    let mut out = vec![0.0_f32; total];
    for b in 0..batch {
        let b_in = b * batch_stride;
        let b_out = b * frames * merged_features;
        for t in 0..frames {
            let t_out = b_out + t * merged_features;
            for c in 0..channels {
                let src_start = b_in + c * channel_stride + t * freqs;
                let dst_start = t_out + c * freqs;
                out[dst_start..dst_start + freqs]
                    .copy_from_slice(&input[src_start..src_start + freqs]);
            }
        }
    }
    Ok(out)
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::transpose_conv_subsampling_bctf_to_btcf;
    use std::io;

    #[test]
    fn fastconformer_conv_subsampling_transposes_bctf_to_btcf() -> io::Result<()> {
        // B=1, C=2, T=2, F=2:
        // c=0: t=0 [1, 2], t=1 [3, 4]
        // c=1: t=0 [5, 6], t=1 [7, 8]
        let input = [1.0_f32, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        let out = transpose_conv_subsampling_bctf_to_btcf(&input, 1, 2, 2, 2)?;
        // t=0: c=0 [1, 2], c=1 [5, 6] -> [1, 2, 5, 6]
        // t=1: c=0 [3, 4], c=1 [7, 8] -> [3, 4, 7, 8]
        assert_eq!(out, vec![1.0, 2.0, 5.0, 6.0, 3.0, 4.0, 7.0, 8.0]);
        assert!(transpose_conv_subsampling_bctf_to_btcf(&input, 1, 2, 2, 3).is_err());
        Ok(())
    }
}
