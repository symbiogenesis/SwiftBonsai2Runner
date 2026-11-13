# Rust Qwen VoiceDesign inference

The service runs Qwen3-TTS 1.7B VoiceDesign/Base Talker, CodePredictor, codec and
speaker inference in Rust on CPU. The app prepares models in its native window.
[ASR](../speech-asr/README.md) has a separate build boundary.

`model.lock.json` pins Q8_0 talkers and codec; `model-q4.lock.json` selects Q4_K_M
talkers with the same codec. Callers verify and retain every file before loading.
The bounded GGUF reader validates geometry and packed weights, retains compressed
projections and dequantizes embedding rows on demand.

Inputs are plain text and a voice description. Design/Base weights must identify
`voice_design`/`base`; presets, reference recordings and arbitrary engine options
are unsupported. Output is finite mono 24 kHz PCM, at most 120 seconds. Missing
EOS, exhausted token limits or invalid samples fail speech, even after playback.
The library opens no devices/listeners; its separately supervised process owns
admission, cancellation, deadlines and shutdown. See the
[HTTP contract](../../tools/qwen-tts/README.md#http-contract),
[app controls](../../docs/development-guide.md#speech-setup-and-operation) and
[resource bounds](../../docs/streaming-resources.md#native-speech-generation-and-streaming).

## Voice conditioning

The first request designs a fixed reference phrase with seed 42, independently
of the message. Design weights are released before Base loads; Rust retains
reference samples, speaker embedding, RVQ codes and transcript across the handoff.
These condition subsequent utterances for that profile. A changed description
replaces the reference; restart regenerates it from the same phrase/seed.
Conversation IDs and line breaks do not choose voices.

This follows Qwen's [Voice Design then Clone workflow](https://github.com/QwenLM/Qwen3-TTS#voice-design-then-clone).
Only one talker is resident. Reference/loading/extraction failure releases owned
state and stops speech instead of redesigning each segment's voice.

The production Design-to-Base handoff uses the same ownership routine as the
independent drop-order control. It releases Design before Base weight intake and
keeps one waveform allocation alive through speaker and acoustic extraction,
including failure exits. Bootstrap, codec priming, missing audio after EOS,
generation and output failures clear both the selected voice and retained codec
reference before retry. Successful turns may reuse the exact reference prefix.

## CPU execution

`Synthesizer::load` reserves two logical CPUs, with at least one worker.
`load_with_threads` and companion `--threads` accept 1–256. A bounded reusable
thread pool belongs to the inference thread. Projections and codec convolutions/
ConvNeXt frames share that team; short convolutions may split output channels.
Independent ranges run concurrently, preserving accumulation order and sequential
streaming history/overlap ownership. `--threads 1` retains serial execution.

F32/F16, Q8_0, Q4_K and Q6_K projections decode directly during multiplication,
without whole-weight expansion or activation requantization. Four-row groups
retain per-row rounding/accumulation; SIMD dispatch stays outside block loops.

Base retains one bounded exact reference-prefix snapshot between successful
utterances. Reuse requires identical input-row bits; changed rows, voice
replacement, cancellation or synthesis failure discard it. It retains no generated
continuation state and changes no sampling. The resource contract owns its bounds.

## Native ABI and failure handling

Rust errors expose only fixed synthesis-stage labels and I/O categories, excluding
request text, descriptions and raw model diagnostics. Speech failure leaves chat
available. Readiness covers initial loading; voice-reference/Base setup is lazy.
Cancellation checks bracket prompt projection, layers/codebooks and frame delivery;
the supervisor tears down work already executing.

## Build boundary

Cargo uses the pinned Rust compiler and ordinary Windows linking tools, without
a native speech library or CMake toolchain. The port derives from reviewed
`qwentts.cpp` revision `5916bf7c79dbb4e66fc059cdc507a79a23a28bb4` and its model layout.
Retain MIT attribution in `LICENSE.qwen-core`, applicable GGML terms in
`LICENSE.ggml` and model Apache-2.0 notices.

## Optional Hexagon runtime

Serving is CPU-only. Signed execution requires the independent
[Windows queue ABI](../../tools/hexagon/windows_queue_abi.c) and
[source-bound RPC](../../tools/hexagon/native_rpc/README.md) controls, exact
package/catalog/driver admission, contained execution, a supported Q8 tensor/solver
mapping, immutable placement/cache ownership and validated correlated completions.
Cancellation and failure must retain uncertain buffers through supervised exit.
CPU PTQ1 bitplanes cannot substitute for the signed DSP's tiled Q2 format.

Fixed F32/guarded-Q2 controls do not qualify Q8 prefill, shared KV coherence or
complete Talker/CodePredictor/codec state. These requirements and NPU/GPU serving,
ternary talkers and 48 kHz artifacts remain in [PLAN.md](../../PLAN.md).

## Rust sampler port

`philox.rs` preserves Philox4x32-10 draws, signed seed/subsequence bits and separate
floating rounding. `sampling.rs` preserves reserved-token suppression, a trailing
72-token repetition window, greedy/top-k/top-p selection and stochastic sampling.
Each frame draws one semantic and fifteen acoustic codes. Both profiles use
seed 42, top-k 50 and repetition penalty 1.05:

| Profile | Semantic temperature / top-p | Acoustic temperature / top-p |
| --- | --- | --- |
| Design | 0.9 / 1.0 | 0.9 / 1.0 |
| Base | 0.85 / 0.9 | 0.6 / 1.0 |

Reserved semantic tokens except EOS are suppressed. Exhausting token/context
budgets without EOS fails. Independent scalar fixtures retain tie ordering,
exceptional-float observations and source provenance.

## Rust speech tokenizer

`tokenizer.rs` preserves the reviewed byte encoder, pretokenization, ranked merges,
special matching and prompt framing. It uses speech-specific tables rather than
Prism's text tokenizer. Bounds are 64 KiB UTF-8 input, 4 KiB spellings, one million
IDs/ranks/table entries, 64 special tokens, 64 MiB unique spelling bytes and
64 million merge-pair probes per encoding. Invalid metadata/encoding publishes
no partial IDs.

## Native waveform ownership

WAV samples remain Rust-owned through PCM conversion and HTTP writes. Streaming
may play chunks before a later failure. The resource contract owns scratch,
output limits and successful completion.

Actual-model parity, first-audio/synthesis latency, Q8/Q4 listening, lifecycle,
combined resources and clean-machine acceptance remain in [PLAN.md](../../PLAN.md).
Model-free controls and captured fixtures do not qualify those properties.
