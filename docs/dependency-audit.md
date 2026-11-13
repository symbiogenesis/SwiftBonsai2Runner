# Dependency policy and choices

[Cargo.toml](../Cargo.toml), package manifests and [Cargo.lock](../Cargo.lock) own
the exact dependency selection. [PLAN.md](../PLAN.md#deferred-dependency-experiments)
owns pending alternatives. This guide explains current responsibilities and acceptance
requirements; prior audit snapshots and prototype results belong in Git history.

## Dependency policy

Artifact verification belongs in `bonsai-artifacts`; the broker has no direct `sha2` dependency.
Windows artifact/maintenance hashing uses the shared CNG boundary in
`bonsai-windows-credentials`; portable and test paths retain `sha2`.
ZIP 9.5.1 disables defaults and enables only `deflate-flate2-zlib-rs`.
Upstream hardening includes [symlink extraction](https://github.com/zip-rs/zip2/pull/1116),
[Unicode-path renaming](https://github.com/zip-rs/zip2/pull/1177) and
[9.1.3 archive fixes](https://github.com/zip-rs/zip2/releases/tag/v9.1.3).
Runner still requires bounded custom extraction; these fixes establish neither
an exploitable Runner flaw nor a measured size/speed gain.

[The source-checks audit job](../.github/workflows/source-checks.yml) runs for relevant
main pushes, weekly and manually. Pinned cargo-audit fetches RustSec anew;
vulnerabilities/yanks fail, while maintenance notices remain visible without
blanket ignores.

## Dependency footprint and duplication

Assess normal/build/dev dependencies and enabled features for each shipped executable
and target. All-target lockfile counts include foreign platforms; package counts do
not measure linked code, binary size, build cost or runtime speed. Cargo feature
unification can broaden a dependency even when Runner disables its defaults.

The required MXC ProcessContainer integration broadens the shared process boundary.
Its lifecycle and validation policy cannot be removed to reduce dependency counts.
A separate supervisor or archive crate split needs demonstrated build/size benefit,
reviewed ownership/IPC and unchanged containment. Native speech remains in separate
executables with independently qualified model, cancellation and resource contracts.

### Renderer features

`wgpu_no_default_features` plus the direct wgpu edge selects Windows DX12, Vulkan and
GLES support. Native chat explicitly selects DX12; fallback policy and device support
are application contracts. Removing a Windows backend requires a separate support
and qualification decision. Font parsers, color-font rendering and accessibility
serve the native UI; they are not evidence of a browser frontend.

## Direct dependency decisions

| Dependency | Decision |
| --- | --- |
| `serde`, `serde_json`, `tokio` | Retain: [JSON](#json-selection)/[runtime](#runtime-screening) screens did not qualify replacements. Tokio explicitly enables io-util/net/rt/signal/sync/time; test-util is dev-only, while MXC broadens features. |
| `http`, `httparse` | Complementary typed policy and bounded wire parsing; no general web framework needed. |
| `subtle` | Constant-time bearer comparison; preserve disabled release debug assertions. |
| `sha2`, `zip` | Artifact verification and minimal deflate extraction at ZIP 9.5.1; custom extraction policy remains necessary. |
| `socket2`, `windows-sys` | Pre-bind options/backlog/exclusive sockets and selected process/credential/audio/accessibility APIs. |
| `windows`, `windows-future` | Compatible WinRT microphone permission/capture; distinct from windows-sys. Retain future 0.3.2 with windows 0.62.2; 0.100 is incompatible. |
| `eframe`, `wgpu` | Native UI, accessibility, clipboard, fallback/system fonts and all three explicit Windows backends. |
| `egui_kittest`, `egui_system_font_provider` | Test harness/font checks; capture-only splitting offers little while production needs graphics. |
| `toml`, `ureq` | CI-only manifest parsing (parse/serde/std) and synchronous HTTPS (Windows Schannel, otherwise Rustls); ureq defaults/compression disabled. |
| `transcribe-cpp` | Static CPU ASR, defaults disabled; reviewed CMake script downloads no source and explicitly enables MSVC optimization. |
| `simd-json` | Exact optional qualification-only crates.io pin; ordinary requests still use serde_json. |
| `fearless_simd` | Pinned safe SIMD (`=1.2.0` in Runner speech and the active external runtime, with independent locks). ASR uses it in `validate_pcm`; TTS uses it in the serving Rust `engine`/`codec` modules. Prism's `bonsai-neon-dot` kernels are a separate runtime dependency. |
| `rayon` | TTS pins `=1.12.0` for safe borrowed projection work on a reusable, explicitly bounded team owned by its calling thread. No global pool or whole-weight copies; one-worker and failed-team construction run serially. This replaces per-projection OS thread creation and requires owning hosted dependency/model acceptance. |
| `memchr` | Promoted from transitive to direct (`=2.8.3`, `default-features = false`) in `bonsai-broker`, `bonsai-chat`, and `bonsai-speech-service` for NEON-vectorized HTTP header, SSE line, multipart boundary, and ASCII folded Find scanning. |
| Windows CNG | Runner artifact and maintenance digests use `bonsai_windows_credentials::Sha256` (`BCryptHashData`); CPU dispatch belongs to the Windows provider. Prism's `bonsai-native-hash` is external to this workspace. These paths alone establish no device speedup. |
| `mxc-sdk`, `flatbuffers` | Required security integration; pursue upstream modularity. |
| Internal crates | Retain shared process/credential/audio/policy/artifact boundaries; no unused direct path edge found. |

ASR/TTS retain separate executables and ownership. ASR disables Metal/optional
accelerators and pins CPU-only CMake; RustSec does not fully cover that bundled
C/C++. TTS now loads ordinary pinned GGUF weights and performs inference in Rust
on the CPU, without GGML/CMake linkage or the former local `HTP0` DLL backend.
[Speech source documentation](../src/speech-tts/README.md#optional-hexagon-runtime)
keeps future signed NPU work separate from current serving. This source change
does not transfer historical device or dependency qualification to the Rust engine.
Speech only needs `VerifiedFile` from `bonsai-artifacts`, whose ZIP APIs are
unconditional. Splitting them may reduce standalone compile scope, but workspace
feature unification/linker elimination limit likely benefit. Pending work stays in PLAN.

## JSON selection

Production uses `serde_json`; the optional parser candidate cannot change ordinary
requests before qualification. The gate requires at least 5% lower equal-weight
geometric-mean time with no case more than 5% slower in both cold and reused screens.
Six workloads cover plain/escaped small inputs, 8/32/128-message histories and near-1
MiB bodies. Timing includes immutable-input preparation, schema/history validation,
canonical serialization, allocations/copies and destruction. Discard one warmup round;
rotate order over nine measured rounds. Unicode, duplicate/unknown fields, shapes,
bounds and canonical bytes must pass before interpreting timings.

### Current candidate and pending gate

The optional `json-candidate` pins published **0.19.0**, upstream
[`00a71f6`](https://github.com/simd-lite/simd-json/commit/00a71f6a931f7f365c23d7fc8d1f84ad693304d3),
with Cargo.lock registry checksum; no submodule is required. It includes Unicode
fixes and [PR #495](https://github.com/simd-lite/simd-json/pull/495) for
[issue #494](https://github.com/simd-lite/simd-json/issues/494), merged 2026-10-27 as
[`77ec801`](https://github.com/simd-lite/simd-json/commit/77ec80113f9990ef7fd89b0ee859e7415bfc4827).
Only package-version metadata differs from that commit. `Deserializer::from_tape`
recovers/resets tape allocation while preserving borrowing/validation.

`padded-tape-serde-reuse` reuses padded input, parser buffers and tape across requests:

| Screen | Included work |
| --- | --- |
| Cold | Scratch setup/destruction plus all per-request work below. |
| Reused | Immutable-body copy, validation, canonical serialization, output destruction and decimal/error fallback; excludes scratch setup/destruction. |

Explicit adapter scales and SIMD parse failures fall back to production parsing:
correctly rounded SIMD conversion can differ from serde_json's default without
either being wrong. Supplemental scale cases include fallback cost. Main-only
`json-candidate.yml` runs correctness before the original six-case and supplemental
5%/5% gates; any case over 5% slower rejects qualification.
**Both cold/reused screens must pass; qualification remains pending.**

## Runtime screening

Tokio remains the broker runtime. A replacement must preserve bounded HTTP parsing,
queues, one-active-request admission, disconnect cancellation and absolute deadlines.
Include thread creation/stacks and lifecycle work when comparing blocking and async
designs; a loopback echo microbenchmark cannot qualify the complete broker or model.

## Reviewed pins and tooling

Use the exact [Rust toolchain](../rust-toolchain.toml), workspace/package manifests,
model/source locks and action pins. A pin gives reproducibility, not continuing
advisory coverage. Review security, source, supported-target and native-linking
obligations independently whenever an identity changes. Follow the
[runtime lock-update procedure](development-guide.md#prepare-a-runtime-lock-update)
for engine releases; speech source locks have their own build and model acceptance.

## Validation and evidence

The owner operates dependency-audit and Foundation acceptance under
[AGENTS.md](../AGENTS.md). Local format, Release analyzers and focused checks establish
only their tested source/target properties. Full suites and benchmarks remain CI-only.
Keep audit notices visible, without blanket ignores or removing accessibility merely
to suppress dependencies. Audits do not exhaustively inspect bundled native sources,
downloaded tools, drivers, models or every transitive build script. Qualification
requires reviewed results bound to the current revision and selected artifacts.
