# Rust Qwen VoiceDesign inference

The serving path loads the pinned Qwen3-TTS 1.7B VoiceDesign/Base GGUF artifacts
and performs Talker, CodePredictor, codec and speaker inference in Rust on the CPU.
It does not compile or load `qwentts.cpp`, GGML or a TTS DLL backend. The app
prepares its model files during in-app setup; running the portable executable
requires no TTS compiler, PowerShell provisioning or runtime installation. ASR
has its own [build boundary](../speech-asr/README.md).

`model.lock.json` pins ordinary Q8_0 talkers and codec; `model-q4.lock.json`
selects Q4_K_M talkers with the same codec. The calling service must verify and
retain every selected file before loading. `weights.rs` reads bounded GGUF
metadata/tensor descriptions, validates dimensions and retains compressed
projection weights. Text embeddings dequantize individual rows instead of
expanding the complete vocabulary into floating-point storage.

Only plain text and a plain-language voice description are exposed. Design
weights must identify `voice_design`, and reusable-speaker weights must identify
`base`. Presets, user reference recordings and arbitrary engine options remain
outside the public interface. Output is finite mono 24 kHz PCM within the existing
120-second bound. Missing EOS, token-ceiling and invalid output fail speech;
partial playback does not establish success.

`Synthesizer::load` selects two fewer threads than the available CPU count, with
a minimum of one. `load_with_threads` and companion `--threads` accept 1?256
workers. The library opens no devices or listeners. Keep it in its separately
supervised process so admission, cancellation, deadlines and shutdown retain
their ownership rules. The [HTTP contract](../../tools/qwen-tts/README.md#http-contract)
owns authentication, schemas, WAV/PCM framing and completion; the
[speech operation guide](../../docs/development-guide.md#speech-setup-and-operation)
owns app setup, controls, service lifetime and resource limits.

## Voice conditioning

The first request for a profile designs a fixed reference phrase with seed 42,
independent of the message being read. Design weights are released before the
separately pinned Base talker loads. Rust extracts a speaker embedding and RVQ
codes from retained reference audio. Those values and its fixed transcript
condition subsequent paragraphs/messages using that profile. Changing the
profile replaces its reference; restarting regenerates it from the same phrase,
seed and description. Conversation IDs and line breaks never choose it.

This follows Qwen's
[Voice Design then Clone workflow](https://github.com/QwenLM/Qwen3-TTS#voice-design-then-clone).
Only one talker model is resident at a time. Reference failure stops speech
instead of independently designing a voice per segment. Rust owns reference
samples and embeddings through model handoff and releases them on failed loading
or extraction.

## Native ABI and failure handling

Production TTS has no foreign model-inference ABI, C++ exception boundary or native
waveform allocator. Rust errors propagate through the owned speech service without
exposing supplied text, descriptions or model diagnostics. Speech failure leaves
text chat available. Request failures retain a fixed synthesis-stage label and
the I/O error category in the app-owned service diagnostics. These distinguish
reference generation, speaker extraction, Base loading, codec work and audio
output without retaining request text, descriptions or raw engine errors.
Readiness confirms initial model loading; it does not certify the first
profile's lazy voice-reference and Base initialization.

Cancellation is checked during prompt projection, between
decoder layers/codebooks and before frame delivery. The supervisor retains process
teardown for work already executing.

Historical scalar-ABI captures remain independent replay inputs. They describe
their original source/compiler, rather than fresh C++ engine execution or
qualification of complete Rust speech.

## Build boundary

TTS builds through Cargo with the pinned Rust compiler and ordinary Windows linking
tools. It needs no speech-source download, LLVM/CMake/Ninja provisioning, generated
C++ overlay or external native speech library. These are contributor details;
users open the portable executable and follow progress in its native window.

The Rust ports derive from reviewed `qwentts.cpp` revision
`5916bf7c79dbb4e66fc059cdc507a79a23a28bb4` and its model layout. Retain MIT
attribution in `LICENSE.qwen-core` and applicable GGML attribution in `LICENSE.ggml`.
Qwen model files retain their Apache-2.0 notices. Attribution does not imply that
those C++ libraries are linked.

## Optional Hexagon runtime

TTS serves on the CPU. It does not register the former local `HTP0` GGML DLL
backend or use a manually installed speech runtime directory. Rust NPU packet
descriptors prepare future signed execution; they do not dispatch inference or
qualify acceleration. Swift's separate `native-q2` policy is unchanged. NPU/GPU
serving, ternary talkers and new 48 kHz artifacts retain independent gates in
[PLAN.md](../../PLAN.md).

## Rust parameter and CPU thread helpers

`rust_talker.rs` loads Talker/CodePredictor geometry, projections, normalization
weights and special tokens. The two-layer text projection uses SiLU. Prompts
retain automatic-language role, codec-prefix and text-EOS ordering. Base aligns
reference transcript and sixteen-codebook streams, retaining unconsumed text as
generation overlays.

`engine.rs` supplies GQA with per-head Q/K normalization, half-split RoPE, SwiGLU,
owned KV and acoustic prediction. Explicit worker counts reach projections;
compatibility methods retain one-worker behavior. Model-free controls check ICL
alignment, embedding bounds, worker consistency and cancellation between
codebooks/layers. They establish neither model parity nor usable speech on a
fresh machine.

## Rust sampler port

`philox.rs` retains Philox4x32-10 draws, signed seed/subsequence bit patterns and
the reference's separate floating-point rounding. `sampling.rs` retains reserved
token suppression, the trailing 72-token repetition window, greedy selection,
top-k/top-p and stochastic sampling. Historical fixtures retain captured ARM64
tie ordering and exceptional-float cases.

Every frame uses one semantic draw followed by fifteen acoustic draws. Design
uses temperature 0.9/top-p 1.0. Base uses semantic temperature 0.85/top-p 0.9 and
acoustic temperature 0.6/top-p 1.0. Both retain top-k 50, seed 42 and repetition
penalty 1.05. Reserved semantic tokens are suppressed except EOS. Exhausting
token/context budgets without EOS returns an error.

Fixtures under `tests/fixtures/` retain independent scalar replay and provenance.
Their captured bits do not qualify changed matrix arithmetic, complete codes,
audio, memory or performance. Actual-model VoiceDesign/Base, Q8/Q4 and lifecycle
acceptance remain in PLAN. Full suites and benchmarks remain CI-only.

## Rust speech tokenizer

`tokenizer.rs` and its scanner retain the reviewed byte encoder, pretokenization,
ranked merges and special matching. `gguf.rs` reads bounded tokenizer tables
without a native metadata API. Rust owns tables, IDs, prompt framing and
projections throughout serving. Prism's text-model tokenizer is not substituted.

The supported subset bounds UTF-8 input to 64 KiB, spellings to 4 KiB, IDs/ranks
and table entries to one million, special tokens to 64, retained unique spelling
bytes to 64 MiB and merge-pair probes to 64 million per encoding. Malformed
metadata/encoding fails without publishing partial IDs. Historical provenance
retains its original source identities and replay limits.

## Native waveform ownership

Complete WAV requests now retain a Rust-owned waveform through validated PCM
conversion and HTTP writes. Streaming requests deliver bounded PCM chunks; heard
samples can precede a later failure. Production has no `qt_audio` allocation or
native release call. The [resource contract](../../docs/streaming-resources.md#native-speech-generation-and-streaming)
owns output limits, scratch bounds and completion. Fresh-machine startup,
actual speech, numerical parity, voice quality and combined resource acceptance
remain pending until the named qualification work runs.
