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

Prompt prefill evaluates the tied vocabulary projection only at the final
position, matching the pinned native graph's selected output. Earlier positions
retain all block, final-normalization and cache-commit validation; only their
discarded vocabulary outputs are omitted. Literal controls cover unused-head
overflow, exact cache contents, final-head failure/reset and fresh replay. This
removes redundant source work without establishing measured model latency.

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

Prompt intake retains the three admitted audio IDs from the model metadata,
including a selected later duplicate spelling; chat-role IDs retain the first
vocabulary occurrence, as in the pinned native provider. A literal duplicate-ID
control covers this distinction without loading a model or executing C++.

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

### Independent numerical replay boundary

`RustRecognizer::transcribe_observed` exposes the Rust provider's complete
normalized mel features, trimmed encoder rows, exact prompt IDs/audio injection
range, and every generation position's full vocabulary logits, greedy selection
and all layers' committed K/V. The EOS decision is observed before unconditional
request reset. `Session::layer_count` defines the complete cache inventory; each
layer is read in `[position, kv_head, head_dim]` order. Values are borrowed only
during the synchronous callback. The engine allocates no capture or duplicate
cache and does not expose mutable state. Normal service inference installs no
capture writer.

The caller owns the independent observation provider and replay packet. Before
collecting any values, bind the reviewed source revision, compiler/target and
executable digest, complete verified model lock/file identity, exact PCM bytes,
`ARITHMETIC_IDENTITY`, F16 KV policy and ordered observation schedule. Retain the
original expected observations, their independent provider/source identities,
dimensions, float bits, tolerances and exclusions; hashes alone cannot replace
the replay inputs. This API supplies observations of Rust execution and grants no
native parity verdict. Native frontend/layerwise/logit/KV captures still require
an independent provider and owning qualification.

Callbacks may compare or stream data without retaining an entire second cache.
Their work, power observations, output and cleanup share the original declared
workload allowance. Callback errors, panic and cancellation clear all committed
state and publish no partial transcript; previously emitted events remain an
incomplete failed capture. Four focused `rust_asr_observer_` controls check literal
mel/encoder/prompt/logit answers, all bits of distinct two-layer F16 caches,
every generation step, refusal at each boundary, unwind, cancellation and fresh
replay. These are mathematical miniature-graph controls, with no model loading or
native inference observation.

### Streaming independent replay

`replay::Replay` compares an authenticated retained observation stream directly
with `RustRecognizer::transcribe_observed`, using one fixed 16 KiB input buffer
and borrowed candidate tensors/caches. The caller supplies reviewed `Binding`
identities for both independent and candidate providers (source revision, exact
compiler, target, executable SHA-256 and arithmetic), the complete verified model
SHA-256 and normalized mono PCM SHA-256. The packet header must match them exactly.
`transcribe` hashes the actual f32 little-endian PCM before execution. The caller
still authenticates the expected packet, the actual candidate image/source and
loaded model, retaining immutable file/ancestor guards. Matching claims alone do
not establish provider independence, source provenance or native parity.

Declare separate frontend, encoder, full-logit and complete-KV tolerances before
the comparison. Each is either exact f32 bits (including signed zero and
exceptional payloads) or finite absolute/relative tolerance, evaluated in f64 as
`abs(actual-expected) <= relative.mul_add(abs(expected), absolute)`. The header
also binds these rules. Finite mode rejects NaN/infinity and negative/nonfinite
allowances. No stage, vocabulary entry, decoder position or cache layer may be
excluded by this format; a separately reviewed exclusion needs a new explicit
contract rather than truncating the stream.

The version-one wire format is deliberately simple for an independent producer.
Integers are unsigned little-endian: counts/indices are u64, token IDs and f32
bits are u32. Strings have a u64 UTF-8 byte count and no terminator. Numeric arrays
have a u64 element count followed by original f32 bits. No padding is present.

| Record | Bytes in order |
| --- | --- |
| Header | Eight ASCII bytes `ASRREPL1`; reference provider; candidate provider; 32-byte complete-model SHA-256; 32-byte PCM SHA-256; four tolerance rules in frontend/encoder/logits/cache order. |
| Provider | 20 original Git revision bytes; compiler string; target string; 32 executable SHA-256 bytes; arithmetic string. Identity strings are nonempty, at most 1,024 bytes and contain no control characters; all-zero revisions/digests are rejected. |
| Tolerance | One byte `0` for exact bits, or byte `1` followed by absolute then relative f32 bits. |
| Frontend | Byte `1`; frame count; complete bin-major mel array. |
| Encoder | Byte `2`; row count; width; complete time-major encoder array. |
| Prompt | Byte `3`; ID count; all token IDs; audio-placeholder start and exclusive end. |
| Decoder | Byte `4`; generation index; full vocabulary-logit array; greedy token ID; committed position count; layer count; then each ordered layer index and complete key/value arrays, in `[position, kv_head, head_dim]` order. |
| Terminal | Byte `5`; decoder-record count; transcript UTF-8 byte count; exact complete transcript bytes (at most 128 KiB); mandatory EOF. |

Exactly one frontend, encoder and prompt record precede contiguous decoder
indices beginning at zero. Every decoder record binds all committed positions
and every layer; the EOS decision is included. Only a successful engine return,
matching terminal transcript and EOF complete a comparison. Reordered/duplicate,
missing/truncated, mismatched or extra records fail; failure is sticky. Engine
failure, cancellation, observer I/O failure and unwind retain the engine's complete
reset contract and cannot turn a partial stream into success. Cancellation checks
occur between records, every 4,096 numerical values, during terminal transcript
intake and after the final EOF probe. Supply a trusted regular file or another
reader with caller-bounded I/O and an absolute deadline: a blocking `Read`
implementation cannot be interrupted cooperatively. Comparison and retained
bytes share the original workload/deadline and evidence-retention rules.

Seven named `rust_asr_replay_` controls use literal mathematical miniature-graph
answers, including distinct two-layer F16 caches and known RoPE zero signs. They
cover complete/fresh replay, identity/PCM/tolerance changes, every record boundary,
cache bits, truncation/trailing bytes, sticky ordering failures and cancellation/
unwind while the decoder is active, and cancellation during terminal/EOF intake.
They establish streaming comparison behavior; they provide no full-model or
native-engine observation. Independent complete
native captures and owning hosted/model/device qualification remain pending.
