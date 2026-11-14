# Native Qwen3-ASR recognition

This Rust library runs the selected **Qwen3-ASR 1.7B** checkpoint
through the CPU-only `transcribe-cpp` 0.4.1 binding. Its packaged native sources
come from `handy-computer/transcribe.cpp` commit
`4d7c1ccefd39e5a9ea36cca5f8fefc6dcc14b99b`.

Before `Recognizer::load`, the service must verify and retain the Q8_0 file in
`model.lock.json`. Rust first checks the bounded GGUF metadata, tokenizer tables,
complete required tensor catalog and aligned, nonoverlapping file ranges while
holding a read-only file guard. The native library then maps it, verifies architecture `qwen3_asr`, variant
`qwen3-asr-1.7b` and audio capabilities, then reuses one four-thread session with
an 8,192-token decoder context. It accepts up to thirty seconds of normalized
mono 16 kHz audio, detects language automatically and returns complete text.
Cancellation discards partial text. The backend checks cancellation before
inference; owned process teardown, deadlines and memory limits bound active
inference because it does not check between decoder steps.

Windows ARM64 native builds require clang-cl and Ninja, the ARM64 MSVC libraries,
and Windows SDK. ggml's ARM backend does not support MSVC cl.exe. Build with CPU
features only, `GGML_NATIVE=OFF`, and OpenMP disabled; the launcher/build tooling
owns that explicit child environment. Upstream build scripts were reviewed:
they compile/install vendored C++ sources and create a verified short build
junction outside Git, without downloading or executing speech models.

Host-bound source builds explicitly enable DOTPROD and I8MM independently when
Windows reports support, retaining ARMv8-A for unavailable observations. The
selected architecture partitions the native build cache. Portable ASR candidates
keep ARMv8-A regardless of their build host. This selects existing quantized CPU
kernels; actual-model transcript parity and latency still require qualification.

The original Qwen model and converted GGUF are **Apache-2.0** licensed; their
immutable source and conversion revisions are recorded in the lock.
The selected Q8_0 file is 2,185,030,624 bytes. Its revision, size and SHA-256
come from the converter publisher's public Hugging Face file metadata.
This is the transcribe.cpp conversion, not a llama.cpp multimodal split model.
transcribe.cpp and its Rust bindings are **MIT** licensed.

Native device qualification of loading, transcription, microphone lifecycle,
quality, accuracy, performance and resources remains pending. Model-free regressions cover family/variant/audio
identity rejection, bounded input and cancellation; compilation and protocol checks
do not establish model qualification.

## Rust replacement prerequisite

`protocol.rs` implements the native-independent automatic-language prompt,
chunk/pad geometry, EOS completion boundary and first-`<asr_text>` envelope split.
The current native engine still performs inference; the production wrapper shares
only final transcript validation with these helpers. The prompt leaves 256 output
tokens inside the smaller of the model context and the current 8,192-token session
ceiling. Per-chunk three-stage convolution rounding retains 390 real audio rows
for 3,000 mel frames in 100-frame chunks, rather than incorrectly rounding the
whole utterance to 375 rows.

The named `rust_asr_protocol_` controls use literal symbolic prompt expansion,
hand-calculated integer geometry, UTF-8 byte bounds and incomplete/cancelled
output rejection. They need no speech engine or model execution and are not
captured native inference. `rust-protocol.json` binds their scope to the exact
reviewed upstream source bytes; `LICENSE.transcribe` retains MIT attribution.

`intake.rs` adds native-independent GGUF v3 metadata/tensor admission and checks
tokenizer table types, special IDs and Qwen tensor geometry before native loading.
Its 64 MiB charged metadata budget, bounded counts/strings, checked arithmetic,
alignment and range checks reject malformed inputs before model allocation.
`rust-intake.json` binds the reviewed native source and ten independent wire/
miniature-catalog controls. The explicitly ignored `rust_asr_intake_pinned_model_index`
case additionally admits the cached 2,185,030,624-byte model's metadata and tensor
index; it does not read every weight, authenticate the file or execute inference.
The caller still owns complete SHA-256 verification and lifetime guards.

`frontend.rs` implements the 16 kHz/128-bin log-mel subset with periodic Hann,
400-point mixed-radix FFT, reflect padding, Slaney filters and per-utterance
normalization. Checkpoint window/filterbank tensors override generated tables.
Cancellation and invalid input publish no partial features. Seven mathematical
`rust_asr_frontend_` controls cover independent DFT answers, reflection, tables,
layout, normalization and cancellation; `rust-frontend.json` binds source identities.
This frontend is preparation for the Rust engine; native serving still computes
its own frontend and performs inference.

The [native package inventory](native-sources.json) records 1,718 immutable package
file identities, including GGML, build inputs and notices. Its
[Rust verifier](../../tools/ci/README.md#native-source-inventory) binds the original
archive checksum and Cargo lock. Selected compiled/preprocessed/generated-source
closure remains separate. A failed local Cargo-lock control exposed the use of a
TOML value parser for a document. The document-parser correction passes its
owning-crate runtime control and strict Clippy; local verification also admits all
1,718 pinned package files and the original archive checksum. Owning hosted
acceptance and complete selected compiler/link closure remain pending.

`tokenizer.rs` now provides bounded Qwen2 pretokenization, ranked byte-level BPE
and literal GPT-2 byte decoding, with a GGUF extraction entry point that first
admits the complete model index. It preserves first-occurrence duplicate entries,
leftmost merge ties, missing-merged-piece byte fallback and split UTF-8 bytes.
It does not recognize special-looking input as prompt controls; decoding retains
their literal spellings. The pinned Unicode category table is part of the
tokenizer definition and does not change with the host Rust Unicode version.
Only absent/empty/`qwen2` pretokenizer metadata is admitted by this replacement.
Seven `rust_asr_tokenizer_` controls use literal Unicode splits, byte answers and
hand-calculated miniature merge schedules; `rust-tokenizer.json` binds their
source and scope. Native serving still owns tokenization and inference. Complete
pinned-vocabulary observations and owning-crate acceptance remain necessary.

`decoder.rs` implements a complete independent scalar causal decoder: token/audio
prefill, GQA, per-head Q/K RMSNorm, temporal NeoX RoPE, SwiGLU, tied output logits
and isolated session state. Its GGUF loader reads F32/F16/Q8_0 decoder projections
and F32 normalization vectors from the admitted tensor ranges. F16 and F32 cache
policies are explicit; the native engine defaults to F16. No cache append becomes
visible until every layer and the logits succeed. Step failure/unwind preserves
all committed KV bits, while failed prefill resets the entire attempted prompt.
The `rust_asr_decoder_` controls cover signed matrix answers, real wire ranges,
binary16 rounding/overflow, uniform grouped attention, complete cache/reset and
cancellation/unwind. `rust-decoder.json` identifies this scalar arithmetic
separately from native activation requantization and flash attention.

`encoder.rs` now implements the full scalar CNN/transformer graph: three
kernel-three stride-two convolutions, independently padded chunks, channel/frequency
flattening, per-chunk positional sinusoids, ragged-tail trimming, full bidirectional
attention, biased layer normalization/GELU FFNs and the decoder-width output head.
Its GGUF loader rejects different frontend metadata and preserves checkpoint
window/filterbank tensors. `encode_pcm` consumes the Rust frontend directly.
Six `rust_asr_encoder_` controls cover hand-calculated convolution edges, GELU,
normalization, bidirectional attention, sinusoidal positions and complete chunk-tail
output, including failure/cancellation replay. The scalar F32 convolution,
f64 erf-based GELU and F32-activation Q8 projections have distinct arithmetic
identities in `rust-encoder.json`; they do not establish native optimized-kernel
or full-model parity.

`rust_engine::RustRecognizer` composes the complete Rust graph as an explicit
qualification API. It loads all components through one immutable model-file
handle and enforces the selected 16 kHz PCM, automatic-language prompt, native
output-budget/repetition rules, lowest-ID greedy ties, mandatory EOS and bounded
complete-transcript contract. Every request clears all KV/position state at
entry and on success, failure, cancellation or unwind. It never calls or falls
back to a native engine. Six `rust_asr_engine_` controls exercise the real tiny
Rust frontend/CNN/transformer/decoder graph with known EOS, plus numerical driver
boundaries and reset/cancellation. `rust-engine.json` binds its separate arithmetic
and qualification limits. Default service/application inference remains native.

## Rust-only owning-crate qualification

The default `native-engine` feature retains `transcribe-cpp` and the existing
`bonsai-speech-asr` executable. With defaults disabled, the complete Rust library
has no native inference dependency, service provisioning or CMake build. This
is an owning-crate build of the production Rust modules, rather than a standalone
source-module harness:

```powershell
cargo clippy --locked --package bonsai-speech-asr --no-default-features --lib --all-targets --release
cargo test --locked --package bonsai-speech-asr --no-default-features --lib --release --no-run
```

Select only named focused controls for local execution; full suites stay CI-only.
For example, after the build, `rust_asr_engine_complete_rust_pipeline_eos_and_fresh_replay`
exercises the complete miniature Rust graph and session reset without models.

An explicit qualification service uses the same compiled model lock, immutable
file/ancestor guards, protected credentials, bounded loopback protocol and native
Windows ARM64 host requirement as the retained control service:

```powershell
cargo build --locked --package bonsai-speech-asr --no-default-features --features rust-qualification --bin bonsai-asr-rust-qualification --release
```

Its CLI accepts the ordinary ASR `--model`, `--token-file` and optional `--port`
arguments. `--version` identifies `bonsai-asr-rust-qualification`, the independent
scalar arithmetic identity and `ProductionQualified: false`; it cannot match the
production binary identity used by provisioning. The application neither builds
nor selects this binary automatically. It performs no native inference or
fallback, even if the native feature is explicitly enabled alongside it. Active
service inference still requires the caller's existing contained process owner,
absolute deadline and verified teardown; cooperative graph cancellation alone
does not establish service cancellation acceptance.

Independent numerical observations and actual-model comparisons remain necessary
before replacing default serving or removing native dependencies. Owning-crate
build/link and miniature graph controls do not establish native arithmetic,
transcript/state parity, microphone integration, quality, listening or resources.
