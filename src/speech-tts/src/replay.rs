//! Bounded streaming replay of independently retained complete TTS observations.
//!
//! This module executes Rust serving only. Callers authenticate the original
//! packet and reviewed source, executable, artifact and input identities. A
//! matching packet does not establish provider independence or model parity.
#![forbid(unsafe_code)]

use std::io::{self, BufReader, Read};

use sha2::{Digest as _, Sha256};

use crate::{
    Synthesizer,
    engine::{LayerKvCache, TOTAL_CODE_GROUPS},
    observation::{
        ARITHMETIC_IDENTITY, Observation, Phase, PredictorObservation, TalkerObservation,
    },
};

/// Reviewed identity of one independently retained observation provider.
pub struct Provider<'a> {
    /// Original twenty-byte Git revision.
    pub revision: [u8; 20],
    /// Exact compiler identity, rather than a channel alias.
    pub compiler: &'a str,
    /// Exact target triple.
    pub target: &'a str,
    /// Complete executing image SHA-256.
    pub executable: [u8; 32],
    /// Separately defined numerical arithmetic identity.
    pub arithmetic: &'a str,
}

/// Caller-reviewed identities that the independent packet must match exactly.
///
/// Retain immutable artifact guards and authenticate the actual candidate image
/// and packet separately. Packet claims alone cannot establish provenance.
pub struct Binding<'a> {
    /// Independent expected-observation provider.
    pub reference: Provider<'a>,
    /// Live Rust candidate provider.
    pub candidate: Provider<'a>,
    /// Complete model-lock bytes SHA-256.
    pub model_lock: [u8; 32],
    /// Complete Design, Base and codec file SHA-256s, in that order.
    pub models: [[u8; 32]; 3],
    /// SHA-256 of u64 text byte count, text, u64 voice byte count and voice.
    pub request: [u8; 32],
    /// Exact effective CPU worker count.
    pub workers: usize,
    /// Whether synthesis must reuse the already selected matching voice.
    ///
    /// False requires a cold or changed voice and its complete Design graph.
    pub retained_voice: bool,
}

/// Explicit numerical acceptance rule, with no implicit exclusions.
#[derive(Clone, Copy)]
pub enum Tolerance {
    /// Exact f32 bits, including signed zeros and exceptional payloads.
    Bits,
    /// Finite absolute/relative comparison, evaluated in f64.
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

/// Separately predeclared rules for every observed float family.
#[derive(Clone, Copy)]
pub struct Tolerances {
    /// Complete prompt rows and trailing overlays.
    pub prompt: Tolerance,
    /// Complete normalized Talker and predictor hidden rows.
    pub hidden: Tolerance,
    /// Every semantic and acoustic vocabulary logit, before sampling.
    pub logits: Tolerance,
    /// Every committed key/value in every layer and position.
    pub cache: Tolerance,
    /// Selected Base speaker embedding.
    pub speaker: Tolerance,
    /// Complete Design waveform and every validated Base PCM chunk.
    pub pcm: Tolerance,
}

impl Tolerances {
    /// Exact float-bit rules for all families.
    pub const BITS: Self = Self {
        prompt: Tolerance::Bits,
        hidden: Tolerance::Bits,
        logits: Tolerance::Bits,
        cache: Tolerance::Bits,
        speaker: Tolerance::Bits,
        pcm: Tolerance::Bits,
    };

    const fn rules(self) -> [Tolerance; 6] {
        [
            self.prompt,
            self.hidden,
            self.logits,
            self.cache,
            self.speaker,
            self.pcm,
        ]
    }
}

/// Single-use complete replay, retaining a fixed 16 KiB input buffer only.
///
/// No expected tensors/caches/audio are allocated. Only [`Self::synthesize`]
/// grants success, after the live engine returns successfully and the terminal
/// record plus EOF match. Failed or unwound comparisons cannot be resumed.
#[must_use]
pub struct Replay<R> {
    input: BufReader<R>,
    tolerance: Tolerances,
    request: [u8; 32],
    workers: usize,
    retained_voice: bool,
    events: usize,
    failed: bool,
    complete: bool,
}

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "TTS replay mismatch or incomplete packet.",
    )
}

fn check_cancel(cancel: &mut impl FnMut() -> bool) -> io::Result<()> {
    if cancel() {
        return Err(io::ErrorKind::Interrupted.into());
    }
    Ok(())
}

impl<R: Read> Replay<R> {
    /// Bind the packet header to independently reviewed identities and rules.
    ///
    /// This reads no model and starts no inference. Supply a trusted regular
    /// file or reader with caller-bounded I/O and the original absolute deadline;
    /// cooperative cancellation cannot interrupt a blocking `Read` call.
    ///
    /// # Errors
    /// Rejects missing/changed identities, malformed headers or invalid rules.
    pub fn new(input: R, binding: &Binding<'_>, tolerance: Tolerances) -> io::Result<Self> {
        if binding.candidate.arithmetic != ARITHMETIC_IDENTITY
            || !(1..=256).contains(&binding.workers)
            || !tolerance.rules().into_iter().all(Tolerance::valid)
        {
            return Err(invalid());
        }
        let mut replay = Self {
            input: BufReader::with_capacity(16 * 1024, input),
            tolerance,
            request: binding.request,
            workers: binding.workers,
            retained_voice: binding.retained_voice,
            events: 0,
            failed: false,
            complete: false,
        };
        replay.bytes(b"TTSREPL1")?;
        replay.provider(&binding.reference)?;
        replay.provider(&binding.candidate)?;
        replay.digest(&binding.model_lock)?;
        for model in &binding.models {
            replay.digest(model)?;
        }
        replay.digest(&binding.request)?;
        replay.number(binding.workers)?;
        replay.bytes(&[u8::from(binding.retained_voice)])?;
        for rule in tolerance.rules() {
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

    /// Compare one complete ordinary Rust synthesis without retaining audio.
    ///
    /// The exact request bytes, selected voice state and worker count must match
    /// before inference. Comparison cancellation checks bracket events, every
    /// 4,096 float values and EOF. Model compute remains under the caller's
    /// supervised process owner and original deadline. No audio device is used.
    ///
    /// # Errors
    /// Returns input, graph, cancellation, packet, schedule, numerical or EOF
    /// errors. After input admission all failures/unwinds discard voice/codec
    /// reuse through ordinary synthesis cleanup. Emitted events stay partial.
    pub fn synthesize(
        mut self,
        synthesizer: &mut Synthesizer,
        text: &str,
        voice: &str,
        cancel: &mut impl FnMut() -> bool,
    ) -> io::Result<()> {
        let _ = crate::bounded_input(text, crate::MAX_TEXT)?;
        let profile = crate::bounded_input(voice, crate::MAX_VOICE)?;
        check_cancel(cancel)?;
        let retained = synthesizer
            .voices
            .0
            .as_ref()
            .is_some_and(|(key, _)| key == &profile);
        if request_digest(text, voice)? != self.request
            || synthesizer.threads != self.workers
            || retained != self.retained_voice
        {
            return Err(invalid());
        }
        synthesizer.synthesize_stream_observed(text, voice, &mut |_| Ok(()), &mut |event| {
            self.observe(event, cancel)
        })?;
        if self.failed || !self.complete {
            return Err(invalid());
        }
        Ok(())
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

    fn number(&mut self, expected: usize) -> io::Result<()> {
        self.bytes(
            &u64::try_from(expected)
                .map_err(|_| invalid())?
                .to_le_bytes(),
        )
    }

    fn digest(&mut self, expected: &[u8]) -> io::Result<()> {
        if expected.iter().all(|byte| *byte == 0) {
            return Err(invalid());
        }
        self.bytes(expected)
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
        for chunk in actual.chunks(4096) {
            check_cancel(cancel)?;
            for actual in chunk {
                let mut raw = [0; 4];
                self.input.read_exact(&mut raw)?;
                if !tolerance.accepts(f32::from_le_bytes(raw), *actual) {
                    return Err(invalid());
                }
            }
        }
        Ok(())
    }

    fn rows(&mut self, actual: &[Vec<f32>], cancel: &mut impl FnMut() -> bool) -> io::Result<()> {
        self.number(actual.len())?;
        for row in actual {
            check_cancel(cancel)?;
            self.floats(row, self.tolerance.prompt, cancel)?;
        }
        Ok(())
    }

    fn cache(
        &mut self,
        actual: &[LayerKvCache],
        cancel: &mut impl FnMut() -> bool,
    ) -> io::Result<()> {
        self.number(actual.len())?;
        for (index, layer) in actual.iter().enumerate() {
            check_cancel(cancel)?;
            self.number(index)?;
            self.number(layer.seq_len)?;
            self.floats(&layer.k, self.tolerance.cache, cancel)?;
            self.floats(&layer.v, self.tolerance.cache, cancel)?;
        }
        Ok(())
    }

    fn codes(&mut self, codes: &[i32; TOTAL_CODE_GROUPS]) -> io::Result<()> {
        for code in codes {
            self.bytes(&code.to_le_bytes())?;
        }
        Ok(())
    }

    fn record(&mut self, tag: u8, phase: Phase) -> io::Result<()> {
        self.bytes(&[tag, u8::from(phase == Phase::Base)])
    }

    fn observe(
        &mut self,
        event: Observation<'_>,
        cancel: &mut impl FnMut() -> bool,
    ) -> io::Result<()> {
        if self.failed || self.complete {
            return Err(invalid());
        }
        // Set before I/O/callbacks so refusal and unwind make failure sticky.
        self.failed = true;
        check_cancel(cancel)?;
        self.event(event, cancel)?;
        self.events = self.events.checked_add(1).ok_or_else(invalid)?;
        self.failed = false;
        Ok(())
    }

    fn event(
        &mut self,
        event: Observation<'_>,
        cancel: &mut impl FnMut() -> bool,
    ) -> io::Result<()> {
        match event {
            Observation::Talker { phase, observation } => {
                self.talker(phase, observation, cancel)?;
            }
            Observation::Speaker(values) => {
                self.bytes(&[8])?;
                self.floats(values, self.tolerance.speaker, cancel)?;
            }
            Observation::ReferenceCodes(frames) => {
                self.bytes(&[9])?;
                self.number(frames.len())?;
                for frame in frames {
                    check_cancel(cancel)?;
                    self.codes(frame)?;
                }
            }
            Observation::Pcm { phase, samples } => {
                self.record(10, phase)?;
                self.floats(samples, self.tolerance.pcm, cancel)?;
            }
            Observation::Complete {
                samples,
                sample_rate,
            } => {
                self.bytes(&[11])?;
                self.number(samples)?;
                self.bytes(&sample_rate.to_le_bytes())?;
                self.bytes(&[255])?;
                self.number(self.events.checked_add(1).ok_or_else(invalid)?)?;
                check_cancel(cancel)?;
                let mut tail = [0];
                let count = self.input.read(&mut tail)?;
                check_cancel(cancel)?;
                if count != 0 {
                    return Err(invalid());
                }
                // EOF is checked inside the synthesis guard: terminal failures
                // invalidate retained voice/codec state before method success.
                self.complete = true;
            }
        }
        Ok(())
    }

    fn talker(
        &mut self,
        phase: Phase,
        event: TalkerObservation<'_>,
        cancel: &mut impl FnMut() -> bool,
    ) -> io::Result<()> {
        match event {
            TalkerObservation::Prompt {
                rows,
                trailing,
                cacheable_rows,
            } => {
                self.record(1, phase)?;
                self.rows(rows, cancel)?;
                self.rows(trailing, cancel)?;
                self.number(cacheable_rows)?;
            }
            TalkerObservation::Logits {
                frame,
                hidden,
                logits,
                cache,
            } => {
                self.record(2, phase)?;
                self.number(frame)?;
                self.floats(hidden, self.tolerance.hidden, cancel)?;
                self.floats(logits, self.tolerance.logits, cancel)?;
                self.cache(cache, cancel)?;
            }
            TalkerObservation::Decision { frame, token, eos } => {
                self.record(3, phase)?;
                self.number(frame)?;
                self.bytes(&token.to_le_bytes())?;
                self.bytes(&[u8::from(eos)])?;
            }
            TalkerObservation::Predictor { frame, observation } => {
                self.predictor(phase, frame, observation, cancel)?;
            }
            TalkerObservation::Frame { frame, codes } => {
                self.record(7, phase)?;
                self.number(frame)?;
                self.codes(codes)?;
            }
        }
        Ok(())
    }

    fn predictor(
        &mut self,
        phase: Phase,
        frame: usize,
        event: PredictorObservation<'_>,
        cancel: &mut impl FnMut() -> bool,
    ) -> io::Result<()> {
        match event {
            PredictorObservation::Prefill { cache } => {
                self.record(4, phase)?;
                self.number(frame)?;
                self.cache(cache, cancel)?;
            }
            PredictorObservation::Logits {
                codebook,
                hidden,
                logits,
                cache,
            } => {
                self.record(5, phase)?;
                self.number(frame)?;
                self.number(codebook)?;
                self.floats(hidden, self.tolerance.hidden, cancel)?;
                self.floats(logits, self.tolerance.logits, cancel)?;
                self.cache(cache, cancel)?;
            }
            PredictorObservation::Decision { codebook, token } => {
                self.record(6, phase)?;
                self.number(frame)?;
                self.number(codebook)?;
                self.bytes(&token.to_le_bytes())?;
            }
        }
        Ok(())
    }
}

fn request_digest(text: &str, voice: &str) -> io::Result<[u8; 32]> {
    let mut digest = Sha256::new();
    for value in [text, voice] {
        digest.update(
            u64::try_from(value.len())
                .map_err(|_| invalid())?
                .to_le_bytes(),
        );
        digest.update(value.as_bytes());
    }
    Ok(digest.finalize().into())
}

#[cfg(test)]
#[path = "replay/tests.rs"]
mod tests;
