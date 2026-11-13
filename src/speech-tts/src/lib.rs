//! Native, process-owned Qwen3-TTS voice design with reusable Base conditioning.
//!
//! The caller verifies and retains the pinned GGUF model artifacts before
//! loading them. Weight intake, Talker/CodePredictor, codec and speaker inference
//! run in Rust on the CPU. This library has no network, preset-voice, external
//! inference DLL or audio-device dependency. Keep it in its supervised process.
#![cfg(windows)]

use std::{
    ffi::{CStr, CString},
    io,
    path::{Path, PathBuf},
};

#[cfg(test)]
#[allow(
    unsafe_code,
    reason = "Replay the retained Rust ABI controls without linking a foreign engine."
)]
mod native;
mod rust_codec;
mod rust_talker;
mod weights;

pub mod codec;
#[cfg(test)]
mod decode_graph;
pub mod engine;
mod gguf;
mod philox;
mod sampling;
pub mod speaker_encoder;
mod tokenizer;

/// Immutable source provenance of the Qwen implementation ported into Rust.
pub const ENGINE_REVISION: &str = "5916bf7c79dbb4e66fc059cdc507a79a23a28bb4";
/// Exact upstream checkpoint represented by the pinned quantized GGUF files.
pub const MODEL: &str = "Qwen/Qwen3-TTS-12Hz-1.7B-VoiceDesign";
/// Default mono PCM output sampling rate (`24_000` Hz).
pub const SAMPLE_RATE: u32 = 24_000;
/// High-fidelity 48 kHz mono PCM output sampling rate (`takuma104/Qwen3-TTS-Tokenizer-12Hz-48kHz`).
pub const SAMPLE_RATE_48K: u32 = 48_000;
const MAX_TEXT: usize = 2048;
const MAX_VOICE: usize = 1024;
const MAX_SAMPLES: usize = 24_000 * 120;
const MAX_SAMPLES_48K: usize = 48_000 * 120;
const TOKEN_CEILING_SAMPLES: usize = (1500 - 2) * 1920;
const TOKEN_CEILING_SAMPLES_48K: usize = (1500 - 2) * 3840;

/// Returns `true` when `sample_rate` is a supported Qwen3-TTS output rate (`24_000` or `48_000` Hz).
#[must_use]
pub const fn is_supported_sample_rate(sample_rate: u32) -> bool {
    sample_rate == SAMPLE_RATE || sample_rate == SAMPLE_RATE_48K
}

/// Complete validated mono speech audio, retained only in memory.
pub struct Audio {
    /// Finite normalized floating-point PCM samples, in playback order.
    pub samples: Vec<f32>,
    /// Sampling rate (`24_000` or `48_000` Hz).
    pub sample_rate: u32,
}

/// Complete validated mono samples in one Rust-owned allocation.
///
/// Borrow with [`AsRef::as_ref`] without copying the complete waveform.
pub struct Waveform {
    samples: Vec<f32>,
    sample_rate: u32,
}

impl Waveform {
    /// Sampling rate (`24_000` or `48_000` Hz) of the retained waveform.
    #[must_use]
    pub const fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
}

impl AsRef<[f32]> for Waveform {
    fn as_ref(&self) -> &[f32] {
        &self.samples
    }
}

/// Exclusive ownership of one native model and its selected reusable voice.
///
/// `VoiceDesign` creates a fixed reference once per profile. Base synthesis then
/// uses that reference for every text, including separate paragraphs/messages.
/// The two model contexts never coexist, preserving the speech memory policy.
pub struct Synthesizer {
    voices: VoiceCache<Voice>,
    design: Option<rust_talker::Talker>,
    model_path: PathBuf,
    voice_model_path: PathBuf,
    codec: rust_codec::Codec,
    threads: usize,
}

struct Voice {
    talker: rust_talker::Talker,
    reference: rust_talker::Reference,
}

// The cache key deliberately excludes utterance text, request identifiers,
// playback mode and chunk boundaries. Replacing a profile releases its model
// before constructing the next one; acquisition failures cannot fall back to
// an unrelated voice or retain two loaded models.
struct VoiceCache<T>(Option<(CString, T)>);

// The waveform allocation is independent of its model context.
// Keep its owner until synchronous Base extraction returns, including failures,
// while releasing Design before allocating Base's model and workers.
#[cfg(test)]
fn finish_voice_bootstrap<C, S: AsRef<[f32]>, R>(
    design: C,
    samples: S,
    load_base: impl FnOnce() -> io::Result<C>,
    extract: impl FnOnce(&mut C, &[f32]) -> io::Result<R>,
) -> io::Result<(C, R)> {
    drop(design);
    let mut base = load_base()?;
    let reference = extract(&mut base, samples.as_ref())?;
    drop(samples);
    Ok((base, reference))
}

impl<T> VoiceCache<T> {
    fn get_or_create(
        &mut self,
        profile: &CStr,
        create: impl FnOnce() -> io::Result<T>,
    ) -> io::Result<&mut T> {
        if let Some((key, voice)) = self.0.as_mut()
            && key.as_c_str() == profile
        {
            return Ok(voice);
        }
        self.0 = None;
        let voice = create()?;
        Ok(&mut self.0.insert((profile.to_owned(), voice)).1)
    }
}

fn load_talker(path: &Path, kind: &str, threads: usize) -> io::Result<rust_talker::Talker> {
    let mut weights = weights::Weights::open(path)?;
    if weights.string("general.architecture")? != "qwen3-tts"
        || weights.string("qwen3-tts.model_type")? != kind
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Unsupported speech model architecture.",
        ));
    }
    let tokenizer = tokenizer::Tokenizer::load(path)?;
    rust_talker::Talker::load(&mut weights, tokenizer, threads)
}

impl Synthesizer {
    /// Load the verified pinned `VoiceDesign`, codec and Base GGUFs in Rust.
    ///
    /// # Errors
    /// Rejects missing, unsafe or incompatible weights and unsupported metadata.
    pub fn load(model: &Path, codec: &Path, voice_model: &Path) -> io::Result<Self> {
        Self::load_with_threads(model, codec, voice_model, None)
    }

    /// Load verified weights with an optional CPU compute team size (1?256).
    ///
    /// The default reserves two available CPUs for the UI and text runtime.
    ///
    /// # Errors
    /// Returns the same failures as [`Self::load`] or rejects an invalid team size.
    pub fn load_with_threads(
        model: &Path,
        codec: &Path,
        voice_model: &Path,
        threads: Option<usize>,
    ) -> io::Result<Self> {
        for path in [model, codec, voice_model] {
            let _ = model_path(path)?;
        }
        let threads = select_threads(threads, std::thread::available_parallelism()?.get())?;
        let mut codec_weights = weights::Weights::open(codec)?;
        let codec = rust_codec::Codec::load(&mut codec_weights)?;
        let design = load_talker(model, "voice_design", threads)?;
        Ok(Self {
            voices: VoiceCache(None),
            design: Some(design),
            model_path: model.to_owned(),
            voice_model_path: voice_model.to_owned(),
            codec,
            threads,
        })
    }

    /// Active mono PCM output sampling rate (24 kHz for the pinned codec).
    #[must_use]
    pub fn sample_rate(&self) -> u32 {
        self.codec.sample_rate()
    }

    fn voice(&mut self, profile: &CStr) -> io::Result<&mut Voice> {
        let Self {
            voices,
            design,
            model_path,
            voice_model_path,
            codec,
            threads,
        } = self;
        voices.get_or_create(profile, || {
            let talker = match design.take() {
                Some(talker) => talker,
                None => load_talker(model_path, "voice_design", *threads)?,
            };
            let profile = profile.to_str().map_err(io::Error::other)?;
            let mut codes = Vec::new();
            talker.generate(
                rust_talker::REFERENCE_TEXT,
                Some(profile),
                None,
                &mut |frame| {
                    codes.push(frame);
                    Ok(())
                },
                &mut || Ok(()),
            )?;
            let samples = codec.decode(&codes)?;
            let count = sample_count(&samples, 0).ok_or_else(invalid_audio)?;
            if !(1920..=24_000 * 30).contains(&count) {
                return Err(invalid_audio());
            }
            // Release Design before allocating Base; the waveform survives that
            // model switch and supplies both speaker and acoustic conditioning.
            drop(talker);
            let mut weights = weights::Weights::open(voice_model_path)?;
            if weights.string("qwen3-tts.model_type")? != "base" {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Speech voice model must be Base.",
                ));
            }
            let speaker = rust_codec::Speaker::load(&mut weights)?.extract(&samples)?;
            let reference_codes = codec.encode_reference(&samples)?;
            let tokenizer = tokenizer::Tokenizer::load(voice_model_path)?;
            let talker = rust_talker::Talker::load(&mut weights, tokenizer, *threads)?;
            drop(samples);
            drop(weights);
            let reference = rust_talker::Reference {
                speaker,
                codes: reference_codes,
            };
            Ok(Voice { talker, reference })
        })
    }

    /// Generate a complete clip using the voice selected by its description.
    ///
    /// # Errors
    /// Rejects invalid input, incomplete token generation, invalid samples and
    /// inference failures. Text and voice descriptions are never logged.
    pub fn synthesize(&mut self, text: &str, voice: &str) -> io::Result<Audio> {
        let audio = self.synthesize_owned(text, voice)?;
        Ok(Audio {
            samples: audio.samples,
            sample_rate: audio.sample_rate,
        })
    }

    /// Generate a complete clip in a single Rust-owned waveform buffer.
    ///
    /// # Errors
    /// Returns the same failures as [`Self::synthesize`].
    pub fn synthesize_owned(&mut self, text: &str, voice: &str) -> io::Result<Waveform> {
        let mut samples = Vec::new();
        self.synthesize_stream(text, voice, &mut |chunk| {
            samples.try_reserve(chunk.len()).map_err(io::Error::other)?;
            samples.extend_from_slice(chunk);
            Ok(())
        })?;
        Ok(Waveform {
            samples,
            sample_rate: self.sample_rate(),
        })
    }

    /// Generate bounded mono PCM chunks using the selected reusable voice.
    ///
    /// Partial audio heard before an error remains partial. Success requires EOS
    /// and complete sample validation. Sink errors stop generation immediately.
    ///
    /// # Errors
    /// Returns input, inference, output validation or sink failures without
    /// exposing private text or model diagnostics.
    pub fn synthesize_stream(
        &mut self,
        text: &str,
        voice: &str,
        sink: &mut (dyn FnMut(&[f32]) -> io::Result<()> + Send),
    ) -> io::Result<()> {
        let text = bounded_input(text, MAX_TEXT)?;
        let profile = bounded_input(voice, MAX_VOICE)?;
        let _ = self.voice(&profile)?;
        let Self { voices, codec, .. } = self;
        let (_, voice) = voices.0.as_mut().ok_or_else(invalid_audio)?;
        codec.prime_reference(&voice.reference.codes)?;
        let mut count = 0;
        let result = voice.talker.generate(
            text.to_str().map_err(io::Error::other)?,
            None,
            Some(&voice.reference),
            &mut |frame| {
                let chunk = codec.decode_stream(&[frame])?;
                count = sample_count(&chunk, count).ok_or_else(invalid_audio)?;
                sink(&chunk)
            },
            &mut || Ok(()),
        );
        codec.reset();
        if result.is_err() {
            // KV, codec and sampler state from a failed turn is never reused.
            voices.0 = None;
        }
        result?;
        if count == 0 {
            return Err(invalid_audio());
        }
        Ok(())
    }
}

fn invalid_audio() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Speech generation did not produce complete valid audio.",
    )
}

// Shared by borrowed complete output and streaming callbacks. Native output is
// admitted before copying or delivering it, with one token-completion policy.
fn sample_count(samples: &[f32], previous: usize) -> Option<usize> {
    sample_count_for_rate(samples, previous, SAMPLE_RATE)
}

fn sample_count_for_rate(samples: &[f32], previous: usize, sample_rate: u32) -> Option<usize> {
    let ceiling = if sample_rate == SAMPLE_RATE_48K {
        MAX_SAMPLES_48K.min(TOKEN_CEILING_SAMPLES_48K - 1)
    } else if sample_rate == SAMPLE_RATE {
        MAX_SAMPLES.min(TOKEN_CEILING_SAMPLES - 1)
    } else {
        return None;
    };
    bonsai_audio::finite_sample_count(samples, previous, ceiling)
}

fn select_threads(requested: Option<usize>, available: usize) -> io::Result<usize> {
    let count = requested.unwrap_or_else(|| available.saturating_sub(2).clamp(1, 256));
    if count == 0 || count > 256 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid speech CPU thread count.",
        ));
    }
    Ok(count)
}

fn model_path(path: &Path) -> io::Result<CString> {
    if !path.is_absolute() || !path.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Native speech needs existing absolute GGUF model files.",
        ));
    }
    let text = path.to_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Native speech requires UTF-8 paths.",
        )
    })?;
    CString::new(text).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Native speech requires regular file paths.",
        )
    })
}

fn bounded_input(text: &str, maximum: usize) -> io::Result<CString> {
    if text.trim().is_empty() || text.chars().take(maximum + 1).count() > maximum {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Speech text or voice description exceeds its bound.",
        ));
    }
    CString::new(text).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Speech text and voice descriptions cannot contain NUL.",
        )
    })
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_TEXT, MAX_VOICE, SAMPLE_RATE, SAMPLE_RATE_48K, TOKEN_CEILING_SAMPLES,
        TOKEN_CEILING_SAMPLES_48K, VoiceCache, bounded_input, finish_voice_bootstrap,
        is_supported_sample_rate, sample_count, sample_count_for_rate, select_threads,
    };

    #[test]
    fn voice_bootstrap_keeps_one_waveform_through_model_switch_and_releases_on_every_exit()
    -> std::io::Result<()> {
        use std::{cell::Cell, io, rc::Rc};

        struct Model(Rc<Cell<usize>>);
        impl Drop for Model {
            fn drop(&mut self) {
                self.0.set(self.0.get() - 1);
            }
        }
        struct Samples {
            data: Vec<f32>,
            releases: Rc<Cell<usize>>,
        }
        impl AsRef<[f32]> for Samples {
            fn as_ref(&self) -> &[f32] {
                &self.data
            }
        }
        impl Drop for Samples {
            fn drop(&mut self) {
                self.releases.set(self.releases.get() + 1);
            }
        }

        // Success, Base initialization failure, then extraction failure.
        for failure in 0..3 {
            let models = Rc::new(Cell::new(1));
            let releases = Rc::new(Cell::new(0));
            let samples = Samples {
                data: vec![0.25, -0.5, 1.0],
                releases: Rc::clone(&releases),
            };
            let original = samples.data.as_ptr();
            let extracted = Cell::new(false);
            let result = finish_voice_bootstrap(
                Model(Rc::clone(&models)),
                samples,
                || {
                    assert_eq!(models.get(), 0, "Design must be released before Base loads");
                    assert_eq!(releases.get(), 0, "Base load still owns the waveform");
                    if failure == 1 {
                        return Err(io::Error::other("injected Base load failure"));
                    }
                    models.set(1);
                    Ok(Model(Rc::clone(&models)))
                },
                |_, samples| {
                    extracted.set(true);
                    assert_eq!(models.get(), 1);
                    assert_eq!(releases.get(), 0);
                    assert_eq!(samples.as_ptr(), original);
                    assert_eq!(samples, [0.25, -0.5, 1.0]);
                    if failure == 2 {
                        return Err(io::Error::other("injected extraction failure"));
                    }
                    Ok(42)
                },
            );
            assert_eq!(releases.get(), 1);
            assert_eq!(extracted.get(), failure != 1);
            if failure == 0 {
                let (base, reference) = result?;
                assert_eq!(reference, 42);
                assert_eq!(models.get(), 1);
                drop(base);
            } else {
                assert!(result.is_err());
            }
            assert_eq!(models.get(), 0);
            assert_eq!(releases.get(), 1);
        }
        Ok(())
    }

    #[test]
    fn selected_voice_is_reused_until_profile_changes_and_failures_never_fallback()
    -> std::io::Result<()> {
        use std::{cell::Cell, io, rc::Rc};
        struct FakeVoice {
            identity: usize,
            alive: Rc<Cell<usize>>,
        }
        impl Drop for FakeVoice {
            fn drop(&mut self) {
                self.alive.set(self.alive.get() - 1);
            }
        }
        let alive = Rc::new(Cell::new(0));
        let acquisitions = Cell::new(0);
        let acquire = || {
            assert_eq!(alive.get(), 0, "Two speech model contexts coexist");
            acquisitions.set(acquisitions.get() + 1);
            alive.set(1);
            Ok(FakeVoice {
                identity: acquisitions.get(),
                alive: Rc::clone(&alive),
            })
        };
        let mut cache = VoiceCache(None);
        // Neither new utterances, paragraphs nor complete/stream output mode
        // are cache inputs: they must all use the selected identity unchanged.
        for _utterance in [
            "First paragraph.",
            "Second paragraph.\n\nA line break.",
            "Another message.",
        ] {
            assert_eq!(cache.get_or_create(c"Warm narrator", acquire)?.identity, 1);
        }
        assert_eq!(acquisitions.get(), 1);
        assert_eq!(cache.get_or_create(c"Clear alto", acquire)?.identity, 2);
        assert!(
            cache
                .get_or_create(c"New profile", || Err(io::Error::other(
                    "synthetic acquisition failure"
                )))
                .is_err()
        );
        assert_eq!(alive.get(), 0);
        assert!(
            cache.0.is_none(),
            "Failed voice acquisition left an unrelated fallback"
        );
        assert_eq!(cache.get_or_create(c"Warm narrator", acquire)?.identity, 3);
        drop(cache);
        assert_eq!(alive.get(), 0);
        Ok(())
    }

    #[test]
    fn complete_and_streamed_samples_share_token_ceiling_and_finite_validation() {
        assert!(is_supported_sample_rate(SAMPLE_RATE));
        assert!(is_supported_sample_rate(SAMPLE_RATE_48K));
        assert!(!is_supported_sample_rate(16_000));
        assert_eq!(sample_count(&[0.0, 2.0], 0), Some(2));
        assert_eq!(
            sample_count(&[0.0], TOKEN_CEILING_SAMPLES - 2),
            Some(TOKEN_CEILING_SAMPLES - 1)
        );
        assert_eq!(sample_count(&[0.0], TOKEN_CEILING_SAMPLES - 1), None);
        assert_eq!(
            sample_count_for_rate(&[0.0], TOKEN_CEILING_SAMPLES_48K - 2, SAMPLE_RATE_48K),
            Some(TOKEN_CEILING_SAMPLES_48K - 1)
        );
        assert_eq!(
            sample_count_for_rate(&[0.0], TOKEN_CEILING_SAMPLES_48K - 1, SAMPLE_RATE_48K),
            None
        );
        assert_eq!(sample_count_for_rate(&[0.0], 0, 16_000), None);
        assert_eq!(sample_count(&[0.0], usize::MAX), None);
        for samples in [&[][..], &[f32::NAN], &[f32::INFINITY], &[f32::NEG_INFINITY]] {
            assert_eq!(sample_count(samples, 0), None);
            assert_eq!(sample_count(samples, 1), None);
        }
    }

    #[test]
    fn cpu_threads_reserve_ui_capacity_and_bound_explicit_overrides() -> std::io::Result<()> {
        assert_eq!(select_threads(None, 12)?, 10);
        assert_eq!(select_threads(None, 1)?, 1);
        assert_eq!(select_threads(Some(12), 12)?, 12);
        assert!(select_threads(Some(0), 12).is_err());
        assert_eq!(select_threads(Some(16), 12)?, 16);
        assert!(select_threads(Some(257), 512).is_err());
        Ok(())
    }

    #[test]
    fn voice_design_input_bounds_count_unicode_without_truncating() {
        for (limit, text) in [
            (MAX_TEXT, "界".repeat(MAX_TEXT)),
            (MAX_VOICE, "🎙".repeat(MAX_VOICE)),
        ] {
            assert_eq!(
                bounded_input(&text, limit)
                    .ok()
                    .as_ref()
                    .map(std::ffi::CString::as_bytes),
                Some(text.as_bytes())
            );
            assert!(bounded_input(&(text + "x"), limit).is_err());
        }
        assert!(bounded_input(" \n\t", MAX_TEXT).is_err());
        assert!(bounded_input("voice\0description", MAX_VOICE).is_err());
    }
}
