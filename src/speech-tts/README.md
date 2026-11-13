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
Packed projections validate their bytes once at construction; dense tensors
validate decoded values. Metadata parsing reuses bounded scratch for discarded
tokenizer strings instead of allocating each spelling separately.

Only plain text and a plain-language voice description are exposed. Design
weights must identify `voice_design`, and reusable-speaker weights must identify
`base`. Presets, user reference recordings and arbitrary engine options remain
outside the public interface. Output is finite mono 24 kHz PCM within the existing
120-second bound. Missing EOS, token-ceiling and invalid output fail speech;
partial playback does not establish success.

`Synthesizer::load` selects two fewer threads than the available CPU count, with
a minimum of one. `load_with_threads` and companion `--threads` accept 1–256
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

The production Design-to-Base handoff uses the same ownership routine as the
independent drop-order control. It releases Design before Base weight intake and
keeps one waveform allocation alive through speaker and acoustic extraction,
including failure exits. Bootstrap, codec priming, missing audio after EOS,
generation and output failures clear both the selected voice and retained codec
reference before retry. Successful turns may reuse the exact reference prefix.

## CPU execution

Packed float/half projections process four output rows together while retaining
each row's original accumulation order. Codec convolutions and ConvNeXt frames
share the projection worker team and honor the selected thread count. Independent
output ranges run concurrently; streaming history and overlap carry retain their
sequential ownership. The serial path remains available with `--threads 1`.
Short, expensive convolutions split output channels across the same team when
there are too few audio frames to occupy it. Four-row SIMD groups and each
output's accumulation order remain unchanged.

Base synthesis retains one bounded, exact reference-prefix snapshot between
successful utterances. Reuse requires identical input-row bits; changed rows,
voice replacement, cancellation or synthesis failure discard the snapshot.
The cache does not retain generated continuation state or change sampling.
See the [resource contract](../../docs/streaming-resources.md#native-speech-generation-and-streaming)
for its storage bounds. These implementation changes still require pinned-model
first-audio, steady-synthesis, listening and resource qualification in PLAN.

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

TTS serves on the CPU. It uses no GGML DLL backend or manually installed speech
runtime directory. The unused GGML registration module and six-word
`HtpPrefillPacket` serializer have been removed. The latter was a host-side
placeholder, not a signed operation packet: its matrix opcode and Q2 type ID
were incompatible with the retained signed ABI. Its former length assertion
established no device behavior.

The independent [Windows queue ABI control](../../tools/hexagon/windows_queue_abi.c)
and [source-bound RPC controls](../../tools/hexagon/native_rpc/README.md) retain
the actual operation, tensor, buffer and completion contracts. They remain
separate controls; successful fixed F32 or guarded-Q2 work does not supply TTS
Q8 prefill, shared KV, a model dispatcher or production admission.

Signed TTS execution still needs exact package/catalog/driver admission and
supervised containment, a source-bound supported Q8 tensor/solver mapping,
immutable placement and cache ownership, validated correlated completions,
and cancellation/failure teardown that retains uncertain buffers through
supervised exit. CPU PTQ1 bitplanes cannot be relabelled as the signed DSP's
tiled Q2 format. Shared cache coherence and complete Talker/CodePredictor/codec
state must be qualified independently before any hybrid path is selected.
Swift's separate `native-q2` policy is unchanged. NPU/GPU serving, ternary
talkers and new 48 kHz artifacts retain independent gates in [PLAN.md](../../PLAN.md).

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

Projection row workers reuse a bounded Rust thread pool on the inference thread
instead of creating OS threads for every projection. Codec encoder/decoder
transformer projections also use the selected worker count. Packed F32/F16,
Q8_0, Q4_K and Q6_K kernels decode directly during multiplication, with SIMD
dispatch outside the block loop and no complete floating-point weight expansion.
They preserve the existing block accumulation and rounding order without
requantizing activations. Independent packed-decoder controls and worker/state
checks cover this source change; end-to-end first audio, steady synthesis,
Q8/Q4 listening and peak-memory qualification remain pending.

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

Complete WAV requests retain a Rust-owned waveform through validated PCM
conversion and HTTP writes. Streaming requests deliver bounded PCM chunks; heard
samples can precede a later failure. Production has no `qt_audio` allocation or
native release call. The [resource contract](../../docs/streaming-resources.md#native-speech-generation-and-streaming)
owns output limits, scratch bounds and completion. Fresh-machine startup,
actual speech, numerical parity, voice quality and combined resource acceptance
remain pending until the named qualification work runs.
