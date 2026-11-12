//! Bounded Qwen3-ASR 1.7B inference on the native CPU backend.
//!
//! The service owns artifact verification, authentication, process limits and
//! microphone permission. This library accepts only the selected Qwen3-ASR family,
//! normalized mono 16 kHz samples and complete successful recognition results.
#![forbid(unsafe_code)]

use fearless_simd::{Level, dispatch, f32x4, prelude::*};
use std::{
    fs::File,
    io::{self, Read},
    path::Path,
    sync::Once,
};
use transcribe_cpp::{Backend, Model, ModelOptions, RunOptions, Session, SessionOptions};

/// Required sample rate for normalized mono input.
pub const SAMPLE_RATE: usize = 16_000;
/// Longest admitted clip, in seconds.
pub const MAX_CLIP_SECONDS: usize = 30;
const MAX_SAMPLES: usize = SAMPLE_RATE * MAX_CLIP_SECONDS;
const MAX_TEXT_BYTES: usize = 32 * 1024;
const ARCHITECTURE: &str = "qwen3_asr";
const VARIANT: &str = "qwen3-asr-1.7b";
static LOGGING: Once = Once::new();

/// A clonable, per-request cooperative cancellation handle.
///
/// Cancellation is checked before and after inference and at the native pre-run
/// boundary. Owned process teardown bounds cancellation during active inference;
/// this Qwen3-ASR backend does not promise cooperative decoder interruption.
#[derive(Clone, Debug, Default)]
pub struct Cancellation(transcribe_cpp::CancelToken);

impl Cancellation {
    /// Construct a fresh request that has not been cancelled.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Stop this request; this handle must not be reused for another request.
    pub fn cancel(&self) {
        self.0.cancel();
    }

    /// Whether cancellation has been requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.is_cancelled()
    }
}

/// A loaded Qwen3-ASR model and one reusable recognition session.
///
/// The caller must retain its verified model-file guard while this value lives:
/// the native runtime memory-maps weights from that immutable file.
pub struct Recognizer {
    session: Session,
}

impl Recognizer {
    /// Load the selected Qwen3-ASR 1.7B GGUF on CPU, without downloading anything.
    ///
    /// # Errors
    /// Rejects relative paths, malformed or unexpected model headers, a different
    /// model identity/audio contract, unavailable memory, and native load faults.
    pub fn load(path: &Path) -> io::Result<Self> {
        if !path.is_absolute() {
            return Err(invalid("Use an absolute, verified Qwen3-ASR model path."));
        }
        let mut file = File::open(path)?;
        if !file.metadata()?.is_file() {
            return Err(invalid("The Qwen3-ASR model must be a regular file."));
        }
        validate_header(&mut file)?;
        LOGGING.call_once(transcribe_cpp::disable_logging);
        let model = Model::load_with(
            path,
            &ModelOptions {
                backend: Backend::Cpu,
                device: None,
                no_mmap: false,
            },
        )
        .map_err(|error| native_error(&error))?;
        let capabilities = model.capabilities().map_err(|error| native_error(&error))?;
        validate_identity(
            model.arch(),
            model.variant(),
            capabilities.native_sample_rate,
            capabilities
                .languages
                .iter()
                .any(|language| language == "en"),
        )?;
        let session = model
            .session_with(&SessionOptions {
                n_threads: 4,
                n_ctx: 8192,
                ..SessionOptions::default()
            })
            .map_err(|error| native_error(&error))?;
        Ok(Self { session })
    }

    /// Recognize one complete mono 16 kHz clip of at most thirty seconds.
    ///
    /// # Errors
    /// Rejects empty, oversized or nonfinite/out-of-range samples and incomplete
    /// recognition. Native diagnostics and partial transcripts are not exposed.
    pub fn transcribe(&mut self, pcm: &[f32]) -> io::Result<String> {
        self.transcribe_cancellable(pcm, &Cancellation::new())
    }

    /// Recognize a clip with an independently clonable request cancellation flag.
    ///
    /// # Errors
    /// Returns `Interrupted` for cancellation; otherwise has the same error
    /// contract as [`Self::transcribe`]. Cancelled partial text is discarded.
    pub fn transcribe_cancellable(
        &mut self,
        pcm: &[f32],
        cancellation: &Cancellation,
    ) -> io::Result<String> {
        validate_pcm(pcm)?;
        check_cancelled(cancellation)?;
        self.session.set_cancel_token(&cancellation.0);
        let result = self.session.run(pcm, &RunOptions::default());
        self.session.clear_cancel_token();
        check_cancelled(cancellation)?;
        let transcript = result.map_err(|error| native_error(&error))?;
        if transcript.text.len() > MAX_TEXT_BYTES || transcript.text.contains('\0') {
            return Err(invalid(
                "Qwen3-ASR returned an invalid or oversized transcript.",
            ));
        }
        Ok(transcript.text)
    }
}

fn validate_identity(
    architecture: &str,
    variant: &str,
    sample_rate: i32,
    supports_english: bool,
) -> io::Result<()> {
    if architecture != ARCHITECTURE
        || variant != VARIANT
        || sample_rate != 16_000
        || !supports_english
    {
        return Err(invalid(
            "The loaded model is not the required Qwen3-ASR 1.7B model.",
        ));
    }
    Ok(())
}

fn check_cancelled(cancellation: &Cancellation) -> io::Result<()> {
    if cancellation.is_cancelled() {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Qwen3-ASR recognition was cancelled.",
        ));
    }
    Ok(())
}

#[expect(
    clippy::inline_always,
    reason = "fearless_simd dispatch! requires inline(always) to inherit target_feature attributes."
)]
#[inline(always)]
fn pcm_in_unit_range_simd<S: Simd>(simd: S, pcm: &[f32]) -> bool {
    let min_v = f32x4::splat(simd, -1.0_f32);
    let max_v = f32x4::splat(simd, 1.0_f32);
    let (chunks, tail) = pcm.as_chunks::<4>();
    for chunk in chunks {
        let v = f32x4::from_slice(simd, chunk);
        let valid = v.simd_ge(min_v) & v.simd_le(max_v);
        if !valid.all_true() {
            return false;
        }
    }
    tail.iter()
        .all(|sample| sample.is_finite() && (-1.0..=1.0).contains(sample))
}

fn validate_pcm(pcm: &[f32]) -> io::Result<()> {
    let level = Level::new();
    let in_range = dispatch!(level, simd => pcm_in_unit_range_simd(simd, pcm));
    if pcm.is_empty() || pcm.len() > MAX_SAMPLES || !in_range {
        return Err(invalid(
            "Qwen3-ASR requires bounded, finite mono 16 kHz samples normalized to [-1, 1].",
        ));
    }
    Ok(())
}

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

fn validate_header(reader: &mut impl Read) -> io::Result<()> {
    let mut header = [0_u8; 24];
    reader.read_exact(&mut header)?;
    if &header[..8] != b"GGUF\x03\0\0\0" {
        return Err(invalid(
            "Qwen3-ASR requires the pinned GGUF version 3 model.",
        ));
    }
    // The pinned publisher's first metadata entry names the architecture. Read
    // only this bounded field before allowing the native runtime to allocate.
    if bounded_string(reader)? != "general.architecture" {
        return Err(invalid("The Qwen3-ASR GGUF identity field is missing."));
    }
    let mut kind = [0_u8; 4];
    reader.read_exact(&mut kind)?;
    if u32::from_le_bytes(kind) != 8 || bounded_string(reader)? != ARCHITECTURE {
        return Err(invalid(
            "The GGUF does not contain the selected Qwen3-ASR architecture.",
        ));
    }
    Ok(())
}

fn bounded_string(reader: &mut impl Read) -> io::Result<String> {
    let mut length = [0_u8; 8];
    reader.read_exact(&mut length)?;
    let length = usize::try_from(u64::from_le_bytes(length))
        .ok()
        .filter(|length| *length <= 128)
        .ok_or_else(|| invalid("The Qwen3-ASR GGUF identity is invalid."))?;
    let mut value = vec![0_u8; length];
    reader.read_exact(&mut value)?;
    String::from_utf8(value).map_err(|_| invalid("The Qwen3-ASR GGUF identity is not UTF-8."))
}

fn native_error(error: &transcribe_cpp::Error) -> io::Error {
    use transcribe_cpp::Error;
    match error {
        Error::Aborted { .. } => io::Error::new(
            io::ErrorKind::Interrupted,
            "Qwen3-ASR recognition was cancelled.",
        ),
        Error::OutOfMemory(_) => io::Error::new(
            io::ErrorKind::OutOfMemory,
            "The native Qwen3-ASR model exceeded available memory.",
        ),
        Error::ModelFileNotFound(_) => io::Error::new(
            io::ErrorKind::NotFound,
            "The verified Qwen3-ASR model is unavailable.",
        ),
        Error::OutputTruncated { .. } | Error::OutputRepetition { .. } => {
            invalid("Qwen3-ASR did not produce a complete transcript.")
        }
        _ => io::Error::other("The native Qwen3-ASR inference request failed."),
    }
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::{
        ARCHITECTURE, Cancellation, MAX_SAMPLES, VARIANT, check_cancelled,
        transpose_conv_subsampling_bctf_to_btcf, validate_header, validate_identity, validate_pcm,
    };

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
    use std::io::{self, Cursor};

    #[test]
    fn normalized_audio_rejects_nonfinite_samples_and_clip_overflow() {
        for samples in [&[][..], &[f32::NAN], &[f32::INFINITY], &[1.01], &[-1.01]] {
            assert!(validate_pcm(samples).is_err());
        }
        assert!(validate_pcm(&[-1.0, 0.0, 1.0]).is_ok());
        assert!(validate_pcm(&vec![0.0; MAX_SAMPLES]).is_ok());
        assert!(validate_pcm(&vec![0.0; MAX_SAMPLES + 1]).is_err());
    }

    #[test]
    fn cancelled_request_handles_cannot_cancel_a_fresh_request() {
        let first = Cancellation::new();
        let worker = first.clone();
        assert!(std::thread::spawn(move || worker.cancel()).join().is_ok());
        assert_eq!(
            check_cancelled(&first).err().map(|error| error.kind()),
            Some(io::ErrorKind::Interrupted)
        );
        assert!(check_cancelled(&Cancellation::new()).is_ok());
    }

    #[test]
    fn selected_qwen_asr_identity_is_checked_before_native_loading() {
        let mut header = b"GGUF\x03\0\0\0".to_vec();
        header.extend(1574_u64.to_le_bytes());
        header.extend(67_u64.to_le_bytes());
        header.extend(20_u64.to_le_bytes());
        header.extend(b"general.architecture");
        header.extend(8_u32.to_le_bytes());
        header.extend(
            u64::try_from(ARCHITECTURE.len())
                .unwrap_or_default()
                .to_le_bytes(),
        );
        header.extend(ARCHITECTURE.as_bytes());
        assert!(validate_header(&mut Cursor::new(&header)).is_ok());
        header[0] = b'X';
        assert!(validate_header(&mut Cursor::new(&header)).is_err());
        assert!(validate_header(&mut Cursor::new(b"GGUF")).is_err());
    }

    #[test]
    fn qwen_asr_rejects_smaller_variant_and_incompatible_audio_identity() {
        assert!(validate_identity(ARCHITECTURE, VARIANT, 16_000, true).is_ok());
        for (architecture, variant, sample_rate, english) in [
            (ARCHITECTURE, "qwen3-asr-0.6b", 16_000, true),
            ("canary_qwen", "canary-qwen-2.5b", 16_000, true),
            (ARCHITECTURE, VARIANT, 48_000, true),
            (ARCHITECTURE, VARIANT, 16_000, false),
        ] {
            assert!(validate_identity(architecture, variant, sample_rate, english).is_err());
        }
    }

    #[test]
    fn qwen_asr_header_rejects_other_families_and_unbounded_identity() {
        let mut header = b"GGUF\x03\0\0\0".to_vec();
        header.extend(1574_u64.to_le_bytes());
        header.extend(67_u64.to_le_bytes());
        header.extend(20_u64.to_le_bytes());
        header.extend(b"general.architecture");
        header.extend(8_u32.to_le_bytes());
        for architecture in ["canary_qwen", "whisper", "qwen3"] {
            let mut wrong = header.clone();
            wrong.extend(
                u64::try_from(architecture.len())
                    .unwrap_or_default()
                    .to_le_bytes(),
            );
            wrong.extend(architecture.as_bytes());
            assert!(validate_header(&mut Cursor::new(wrong)).is_err());
        }
        header.extend(129_u64.to_le_bytes());
        assert!(validate_header(&mut Cursor::new(header)).is_err());
    }
}
