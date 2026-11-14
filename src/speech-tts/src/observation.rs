//! Borrowed Rust TTS observations for independently retained numerical controls.
//!
//! Callers bind exact source/compiler/executable, model locks, input bytes,
//! worker count and arithmetic before collecting. These events contain Rust
//! execution only; they do not supply independent answers or a parity verdict.
//! Callbacks run synchronously, allocate no capture in the engine and may refuse
//! any event. Already emitted events from failed/unwound requests are incomplete.
#![forbid(unsafe_code)]

use crate::engine::{LayerKvCache, TOTAL_CODE_GROUPS};

/// Separately identified arithmetic of ordinary Rust speech serving.
pub const ARITHMETIC_IDENTITY: &str = "rust-tts-packed-f32-activation-v1-f32-kv";

/// The selected model role; cold bootstrap and Base synthesis are separate graphs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Fixed seeded reference generation before Design teardown.
    Design,
    /// Utterance generation with the retained speaker and reference codes.
    Base,
}

/// Complete acoustic decoder observations for one independent frame session.
pub enum PredictorObservation<'a> {
    /// All layers after processing the Talker hidden row at position zero.
    Prefill {
        /// Committed K/V in `[position, kv_head, head_dim]` order.
        cache: &'a [LayerKvCache],
    },
    /// Raw full-vocabulary logits before the sampler mutates its scratch.
    Logits {
        /// Zero-based residual codebook (0 through 14).
        codebook: usize,
        /// Final normalized hidden row.
        hidden: &'a [f32],
        /// Every token's unmodified logit in token-ID order.
        logits: &'a [f32],
        /// Every layer's committed K/V, including this position.
        cache: &'a [LayerKvCache],
    },
    /// Seeded sample for the immediately preceding logit observation.
    Decision {
        /// Zero-based residual codebook.
        codebook: usize,
        /// Selected acoustic token ID.
        token: i32,
    },
}

/// Ordered observations of one Talker request, including its EOS decision.
pub enum TalkerObservation<'a> {
    /// Complete projected prompt and trailing text overlays before prefill.
    Prompt {
        /// Time-major rows, each in hidden-channel order.
        rows: &'a [Vec<f32>],
        /// Trailing overlays used for generated frames.
        trailing: &'a [Vec<f32>],
        /// Maximum exact-prefix candidate, before the byte bound is applied.
        cacheable_rows: usize,
    },
    /// Raw semantic logits and complete state before reserved-token suppression.
    Logits {
        /// Zero-based generated acoustic frame; EOS uses the next frame index.
        frame: usize,
        /// Final normalized Talker hidden row.
        hidden: &'a [f32],
        /// Full unmodified vocabulary in token-ID order.
        logits: &'a [f32],
        /// Every layer's complete committed K/V, including restored prefixes.
        cache: &'a [LayerKvCache],
    },
    /// Seeded semantic selection after masking and repetition policy.
    Decision {
        /// Frame index matching the immediately preceding logits.
        frame: usize,
        /// Selected semantic token ID.
        token: i32,
        /// Whether this is the terminal EOS decision.
        eos: bool,
    },
    /// One residual decoder observation, nested within its acoustic frame.
    Predictor {
        /// Owning acoustic frame index.
        frame: usize,
        /// Complete residual decoder state/logits/decision.
        observation: PredictorObservation<'a>,
    },
    /// Complete semantic-plus-fifteen-acoustic code frame before codec delivery.
    Frame {
        /// Zero-based frame index.
        frame: usize,
        /// Code groups in their original decoding order.
        codes: &'a [i32; TOTAL_CODE_GROUPS],
    },
}

/// Ordered numerical observations of explicit observed synthesis.
///
/// A cold voice emits Design events/PCM before Base conditioning and synthesis.
/// An unchanged warm voice emits no Design graph. The final `Complete` event is
/// provisional until the synthesis method returns `Ok`; callback refusal or
/// unwind invalidates the capture and clears reusable voice/codec state.
pub enum Observation<'a> {
    /// A borrowed prompt, semantic or acoustic graph observation.
    Talker {
        /// Exact model role for these values.
        phase: Phase,
        /// Ordered graph observation.
        observation: TalkerObservation<'a>,
    },
    /// Selected Base speaker embedding, also emitted for a warm voice.
    Speaker(&'a [f32]),
    /// Complete encoded reference conditioning, also emitted for a warm voice.
    ReferenceCodes(&'a [[i32; TOTAL_CODE_GROUPS]]),
    /// Validated mono PCM, before delivery or Design-to-Base extraction.
    Pcm {
        /// Design waveform or one Base streaming chunk.
        phase: Phase,
        /// Finite samples in playback order; borrowed only during the callback.
        samples: &'a [f32],
    },
    /// Complete Base output after EOS and all chunk/sink checks.
    Complete {
        /// Total validated Base samples.
        samples: usize,
        /// Active codec sampling rate.
        sample_rate: u32,
    },
}
