//! Bounded streaming comparison against independently retained ASR observations.
//!
//! This reader executes no reference engine and grants no production verdict.
//! The caller authenticates the original packet and binds its reviewed provider,
//! candidate, verified model and exact little-endian PCM identities. See the
//! owning README for the versioned wire format and independence requirements.
#![forbid(unsafe_code)]

use std::io::{self, BufReader, Read};

use sha2::{Digest as _, Sha256};

use crate::rust_engine::{ARITHMETIC_IDENTITY, Observation, RustRecognizer};

const MAGIC: &[u8; 8] = b"ASRREPL1";
const MAX_TEXT: usize = 128 * 1024;

/// Source-bound identity of one observation provider.
pub struct Provider<'a> {
    /// Reviewed Git source revision, as its original twenty bytes.
    pub revision: [u8; 20],
    /// Exact compiler identity, rather than a channel alias.
    pub compiler: &'a str,
    /// Exact target triple.
    pub target: &'a str,
    /// SHA-256 of the executing provider image.
    pub executable: [u8; 32],
    /// Independently defined arithmetic identity.
    pub arithmetic: &'a str,
}

/// Trusted identities expected from the original independent packet.
///
/// These claims must come from reviewed provenance, not untrusted packet fields.
/// Equality of identities establishes binding only, not provider independence.
pub struct Binding<'a> {
    /// Independent provider of expected observations.
    pub reference: Provider<'a>,
    /// Candidate whose live observations will be compared.
    pub candidate: Provider<'a>,
    /// SHA-256 of the complete verified model file, shared by both arms.
    pub model: [u8; 32],
    /// SHA-256 of the exact normalized mono PCM f32 little-endian input bytes.
    pub pcm: [u8; 32],
}

/// Explicit numerical acceptance rule for one observation family.
#[derive(Clone, Copy)]
pub enum Tolerance {
    /// Require every f32 bit, including signed zero and exceptional payloads.
    Bits,
    /// Require finite values and `abs(actual-expected) <= absolute + relative*abs(expected)`.
    Finite {
        /// Nonnegative finite absolute allowance.
        absolute: f32,
        /// Nonnegative finite relative allowance.
        relative: f32,
    },
}

impl Tolerance {
    fn valid(self) -> bool {
        match self {
            Self::Bits => true,
            Self::Finite { absolute, relative } => {
                absolute.is_finite() && relative.is_finite() && absolute >= 0.0 && relative >= 0.0
            }
        }
    }

    fn accepts(self, expected: f32, actual: f32) -> bool {
        match self {
            Self::Bits => expected.to_bits() == actual.to_bits(),
            Self::Finite { absolute, relative } => {
                expected.is_finite()
                    && actual.is_finite()
                    && (f64::from(actual) - f64::from(expected)).abs()
                        <= f64::from(relative)
                            .mul_add(f64::from(expected).abs(), f64::from(absolute))
            }
        }
    }
}

/// Separately declared tolerances; cache comparison is never silently omitted.
#[derive(Clone, Copy)]
pub struct Tolerances {
    /// Normalized mel features.
    pub frontend: Tolerance,
    /// Complete encoder rows.
    pub encoder: Tolerance,
    /// Every vocabulary logit at every generation position, including EOS.
    pub logits: Tolerance,
    /// Every committed key and value in every layer.
    pub cache: Tolerance,
}

impl Tolerances {
    /// Exact float-bit comparison for all observation families.
    pub const BITS: Self = Self {
        frontend: Tolerance::Bits,
        encoder: Tolerance::Bits,
        logits: Tolerance::Bits,
        cache: Tolerance::Bits,
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Frontend,
    Encoder,
    Prompt,
    Decoder,
    Failed,
}

/// Single-use comparison with a fixed 16 KiB read buffer and no duplicate cache.
///
/// Only [`Self::transcribe`] can complete a replay. Failed, truncated, reordered,
/// cancelled or unwound requests cannot return a comparison success.
#[must_use]
pub struct Replay<R> {
    input: BufReader<R>,
    tolerance: Tolerances,
    stage: Stage,
    generation: usize,
    prompt_positions: usize,
    layers: Option<usize>,
    pcm: [u8; 32],
}

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "ASR replay mismatch or incomplete packet.",
    )
}

fn check_cancel(cancel: &mut impl FnMut() -> bool) -> io::Result<()> {
    if cancel() {
        return Err(io::ErrorKind::Interrupted.into());
    }
    Ok(())
}

impl<R: Read> Replay<R> {
    /// Bind the original packet header to caller-reviewed identities and tolerances.
    ///
    /// No tensor, model or PCM is loaded here. The caller retains immutable model
    /// guards and authenticates the packet and actual candidate/input identities.
    ///
    /// # Errors
    /// Rejects missing/changed identities, invalid tolerances and malformed headers.
    pub fn new(input: R, binding: &Binding<'_>, tolerance: Tolerances) -> io::Result<Self> {
        if binding.candidate.arithmetic != ARITHMETIC_IDENTITY
            || ![
                tolerance.frontend,
                tolerance.encoder,
                tolerance.logits,
                tolerance.cache,
            ]
            .into_iter()
            .all(Tolerance::valid)
        {
            return Err(invalid());
        }
        let mut replay = Self {
            input: BufReader::with_capacity(16 * 1024, input),
            tolerance,
            stage: Stage::Frontend,
            generation: 0,
            prompt_positions: 0,
            layers: None,
            pcm: binding.pcm,
        };
        replay.bytes(MAGIC)?;
        replay.provider(&binding.reference)?;
        replay.provider(&binding.candidate)?;
        replay.digest(&binding.model)?;
        replay.digest(&binding.pcm)?;
        for rule in [
            tolerance.frontend,
            tolerance.encoder,
            tolerance.logits,
            tolerance.cache,
        ] {
            match rule {
                Tolerance::Bits => replay.bytes(&[0])?,
                Tolerance::Finite { absolute, relative } => {
                    replay.bytes(&[1])?;
                    replay.bytes(&absolute.to_le_bytes())?;
                    replay.bytes(&relative.to_le_bytes())?;
                }
            }
        }
        Ok(replay)
    }

    /// Compare one complete live Rust request and return its exact transcript.
    ///
    /// The successful engine result, complete terminal transcript and packet EOF
    /// are all mandatory. Comparison work shares the caller's original allowance;
    /// cancellation is checked between records and every 4,096 numerical values.
    ///
    /// # Errors
    /// Returns graph/cancellation errors or any identity, schedule, numerical,
    /// state, transcript, truncation or trailing-data mismatch. The recognizer's
    /// unconditional reset also applies when comparison fails or unwinds.
    pub fn transcribe(
        mut self,
        recognizer: &mut RustRecognizer,
        pcm: &[f32],
        cancel: &mut impl FnMut() -> bool,
    ) -> io::Result<String> {
        if pcm.len() > crate::SAMPLE_RATE * crate::MAX_CLIP_SECONDS {
            return Err(invalid());
        }
        let mut digest = Sha256::new();
        for chunk in pcm.chunks(4096) {
            if cancel() {
                return Err(io::ErrorKind::Interrupted.into());
            }
            for sample in chunk {
                digest.update(sample.to_le_bytes());
            }
        }
        if <[u8; 32]>::from(digest.finalize()) != self.pcm {
            return Err(invalid());
        }
        // The engine and comparator must share the same cancellation owner.
        // RefCell serializes their synchronous calls without cloning authority.
        let cancel = std::cell::RefCell::new(cancel);
        let transcript = recognizer.transcribe_observed(
            pcm,
            &mut || (*cancel.borrow_mut())(),
            &mut |event| self.observe(event, &mut || (*cancel.borrow_mut())()),
        )?;
        self.finish(&transcript, &mut || (*cancel.borrow_mut())())?;
        Ok(transcript)
    }

    fn bytes(&mut self, expected: &[u8]) -> io::Result<()> {
        let mut buffer = [0_u8; 1024];
        for expected in expected.chunks(buffer.len()) {
            self.input.read_exact(&mut buffer[..expected.len()])?;
            if &buffer[..expected.len()] != expected {
                return Err(invalid());
            }
        }
        Ok(())
    }

    fn digest(&mut self, expected: &[u8]) -> io::Result<()> {
        if expected.iter().all(|byte| *byte == 0) {
            return Err(invalid());
        }
        self.bytes(expected)
    }

    fn number(&mut self, expected: usize) -> io::Result<()> {
        self.bytes(
            &u64::try_from(expected)
                .map_err(|_| invalid())?
                .to_le_bytes(),
        )
    }

    fn text(&mut self, expected: &str) -> io::Result<()> {
        if expected.is_empty() || expected.len() > 1024 || expected.chars().any(char::is_control) {
            return Err(invalid());
        }
        self.number(expected.len())?;
        self.bytes(expected.as_bytes())
    }

    fn provider(&mut self, expected: &Provider<'_>) -> io::Result<()> {
        self.digest(&expected.revision)?;
        self.text(expected.compiler)?;
        self.text(expected.target)?;
        self.digest(&expected.executable)?;
        self.text(expected.arithmetic)
    }

    fn floats(
        &mut self,
        actual: &[f32],
        tolerance: Tolerance,
        cancel: &mut impl FnMut() -> bool,
    ) -> io::Result<()> {
        self.number(actual.len())?;
        for (chunk_index, chunk) in actual.chunks(4096).enumerate() {
            if cancel() {
                return Err(io::ErrorKind::Interrupted.into());
            }
            for (index, actual) in chunk.iter().enumerate() {
                let mut raw = [0; 4];
                self.input.read_exact(&mut raw)?;
                if !tolerance.accepts(f32::from_le_bytes(raw), *actual) {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "ASR replay numerical mismatch at element {}.",
                            chunk_index * 4096 + index
                        ),
                    ));
                }
            }
        }
        Ok(())
    }

    fn observe(
        &mut self,
        event: Observation<'_>,
        cancel: &mut impl FnMut() -> bool,
    ) -> io::Result<()> {
        let stage = self.stage;
        let result = self.event(event, cancel);
        result.map_err(|error| {
            self.stage = Stage::Failed;
            io::Error::new(
                error.kind(),
                format!(
                    "ASR replay {stage:?}, generation {}: {error}",
                    self.generation
                ),
            )
        })
    }

    fn event(
        &mut self,
        event: Observation<'_>,
        cancel: &mut impl FnMut() -> bool,
    ) -> io::Result<()> {
        if cancel() {
            return Err(io::ErrorKind::Interrupted.into());
        }
        match (self.stage, event) {
            (Stage::Frontend, Observation::Frontend(mel)) => {
                self.bytes(&[1])?;
                self.number(mel.frames())?;
                self.floats(mel.values(), self.tolerance.frontend, cancel)?;
                self.stage = Stage::Encoder;
            }
            (Stage::Encoder, Observation::Encoder(audio)) => {
                self.bytes(&[2])?;
                self.number(audio.rows())?;
                self.number(audio.width())?;
                self.floats(audio.values(), self.tolerance.encoder, cancel)?;
                self.stage = Stage::Prompt;
            }
            (Stage::Prompt, Observation::Prompt(prompt)) => {
                self.bytes(&[3])?;
                self.number(prompt.ids().len())?;
                for id in prompt.ids() {
                    self.bytes(&id.to_le_bytes())?;
                }
                self.number(prompt.audio_rows().start)?;
                self.number(prompt.audio_rows().end)?;
                self.prompt_positions = prompt.ids().len();
                self.stage = Stage::Decoder;
            }
            (
                Stage::Decoder,
                Observation::Decoder {
                    generation_index,
                    logits,
                    token,
                    state,
                },
            ) => {
                if generation_index != self.generation
                    || state.position()
                        != self
                            .prompt_positions
                            .checked_add(self.generation)
                            .ok_or_else(invalid)?
                    || self
                        .layers
                        .is_some_and(|layers| state.layer_count() != layers)
                    || state.layer_count() == 0
                {
                    return Err(invalid());
                }
                self.bytes(&[4])?;
                self.number(generation_index)?;
                self.floats(logits, self.tolerance.logits, cancel)?;
                self.bytes(&token.to_le_bytes())?;
                self.number(state.position())?;
                self.number(state.layer_count())?;
                for layer in 0..state.layer_count() {
                    self.number(layer)?;
                    let (keys, values) = state.cache(layer).ok_or_else(invalid)?;
                    self.floats(keys, self.tolerance.cache, cancel)?;
                    self.floats(values, self.tolerance.cache, cancel)?;
                }
                self.layers = Some(state.layer_count());
                self.generation = self.generation.checked_add(1).ok_or_else(invalid)?;
            }
            _ => return Err(invalid()),
        }
        Ok(())
    }

    fn finish(mut self, transcript: &str, cancel: &mut impl FnMut() -> bool) -> io::Result<()> {
        check_cancel(cancel)?;
        if self.stage != Stage::Decoder || self.generation == 0 || transcript.len() > MAX_TEXT {
            return Err(invalid());
        }
        self.bytes(&[5])?;
        self.number(self.generation)?;
        self.number(transcript.len())?;
        check_cancel(cancel)?;
        for chunk in transcript.as_bytes().chunks(1024) {
            self.bytes(chunk)?;
            check_cancel(cancel)?;
        }
        let mut tail = [0];
        let tail_bytes = self.input.read(&mut tail)?;
        check_cancel(cancel)?;
        if tail_bytes != 0 {
            return Err(invalid());
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "replay/tests.rs"]
mod tests;
