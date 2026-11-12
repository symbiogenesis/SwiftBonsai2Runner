# Native Snapdragon quantized KV

This is the contract for the **experimental compressed-KV source candidate
implemented in Prism** for Snapdragon X1 Elite and X2 Elite Extreme. It does not
qualify a new application profile, increase Runner's 8K context limit or replace
the pinned 0.1.23 runtime bundle. [PLAN.md](../PLAN.md#bounded-calibration-and-optional-precision)
owns the remaining application integration, delivery and qualification work.

The intended endpoint is a standard compressed-cache profile **if measured
quality loss is negligible and the selected device gains a material resource or
performance benefit**. Until those independent gates pass, FP16 remains the
application default and an explicit comparison/rollback control.

## Source selection and current checks

The safe codec API is `kv_quant::Quantizer::{new, scalar}`. Graph construction
selects `graph::KvCachePolicy::{F16, Rotated4, Rotated4Keys3Values}` explicitly;
existing constructors retain FP16. The raw generation CLI accepts
`--kv-cache f16|rsq4|rsq4k3v` and records the policy and current graph identity in
its completion metadata. Tokenize mode rejects this generation-only option.

`SwiftSession::load_profile_with_kv_cache` and the private server's trusted
`--kv-cache` option expose the candidate at the existing fixed 8K service bound.
Client request bodies cannot select precision. Non-FP16 readiness reports
`kv_cache` and the actual `startup_graph_identity`; the startup label remains
accurate when a later request changes the adapter scale. Existing strict Runner
brokers reject those different profile fields. Application use requires a
published, verified Prism bundle and matching broker/profile/context contracts.

The sibling Prism checkout owns `src/kv_quant.rs`,
`src/graph/quantized_kv.rs` and its `docs/quantized-kv.md` runtime contract.
These local source changes are separate from Runner's active release pin.
At local Prism `a4659c0`, 30 focused Release controls pass: 12 codec, four
packed-cache graph, four NEON-helper, six tiny hybrid-graph, one CLI policy and
three private-service checks. The ten affected storage/graph controls passed
again after the final allocation correction. Strict Release all-target Clippy
passes for the runtime, server and NEON helper on native ARM64 and Windows x64,
with `parallel-rows` and pinned nightly-2026-11-09. Formatting and changed Markdown
checks pass. No full-model quality, long-context, performance, complete CI or
per-device promotion verdict follows from those focused controls. The bounded
validation and power observations are recorded in Prism's integration commit.

## Scope and algorithm

Swift has 16 full-attention layers with four 256-channel KV heads per layer.
Compression applies to those keys and values. Its 48 Gated DeltaNet/convolution
layers retain their existing FP32 state; model weights, adapters, sampler and
output reservations are separate identities and costs.

The native Rust candidate combines inspectable algorithmic ideas:

| Source | Selected idea | Transfer boundary |
| --- | --- | --- |
| [TurboQuant paper](https://arxiv.org/abs/2504.19874) | Deterministic orthogonal rotation followed by symmetric Lloyd-Max scalar codebooks | Published distortion and downstream scores do not prove Swift quality or identical text. |
| [Qdrant engineering](https://qdrant.tech/articles/turboquant-quantization/) | CPU SIMD consumption, explicit normalization and real storage accounting | Embedding search is not KV softmax/value attention; the complete internal crate includes C helpers. |
| [turbovec](https://github.com/RyanCodrai/turbovec) | Packed-code scoring and ARM NEON kernels | A search index does not supply causal attention, weighted values or hybrid-state rollback. |
| [Standalone Rust KV implementation](https://github.com/SaschaOnTour/turboquant) | Small codec/cache interfaces and explicit compression modes | Its QJL-based modes have reported quality problems; its CUDA/mistral.rs path supplies no Snapdragon qualification. |

This is an original implementation from those concepts, with no external
TurboQuant dependency or copied third-party source. Retain research attribution.
Any later source reuse requires an immutable revision and an independent review
of the copied bytes, applicable licenses and notices.

The codec uses fixed 256-channel heads, three deterministic signed, normalized
Walsh-Hadamard passes and 3-bit or 4-bit packed scalar codes. A four-byte
little-endian per-head scale accompanies each record. Symmetric Gaussian
Lloyd-Max levels approximate the rotated distribution; no exact optimality or
negligible downstream loss is claimed. Queries rotate once, packed keys supply
scores directly and packed values accumulate in the rotated basis before one
inverse transform. Per-head/tile scratch is bounded, with no whole-history
expansion or retained FP16/F32 mirror.

K4/V4 is the conservative candidate; K4/V3 is the asymmetric capacity candidate.
Their codebooks, transform/sign seed, normalization, layout version and arithmetic
must be sealed into the numerical identity. QJL residual sketches, fitted
anisotropy corrections and precision changes to recurrent state are excluded
until separately justified. An unbiased inner-product estimator alone does not
establish unbiased softmax, negligible quality loss or faster inference.

## Snapdragon execution and state

X1 and X2 share a runtime-detected 128-bit NEON execution path with an explicit
scalar reference. There is no SKU-specific timing or thread-count tuning.
Compressed attention currently streams query heads serially while preserving
the existing projection worker policy. Further grouped-head tiling needs its
own checks and measured benefit. X2 qualification cannot establish an X1 result,
and source-level work reduction is not a measured speedup.

Checkpoint/restore must preserve the **exact bytes of the selected quantized
profile**, including scale bits, packed tails, page ownership and sequence
position. That is exact restoration of an approximate profile, not exact recovery
of the original FP16 values. Failed append/allocation, cancellation, edited-prefix
reuse and adapter changes must preserve transaction atomicity and provenance.
State export must identify reconstructed quantized rows instead of labelling
them as original FP16 observations. Tiled prefill rejects these profiles;
ordinary cancellation-aware sequential block ingestion remains supported.

Full-context admission must charge packed records and scales, page tables,
retained checkpoint pages/tails, transform/codebook storage and scratch. Keep
FP32 recurrent/convolution storage and process physical reserves separate. A
smaller serialized cache is insufficient grounds to lower process admission.

For a 256-channel record with one four-byte scale, K4/V4 uses 132 bytes per key
and value head; K4/V3 uses 132 key bytes and 100 value bytes. Under that candidate
layout, calculated attention payloads are:

| Total context | FP16 | K4/V4 including scales | K4/V3 including scales |
| --- | ---: | ---: | ---: |
| 8K | 512 MiB | 132 MiB | 116 MiB |
| 32K | 2 GiB | 528 MiB | 464 MiB |
| 64K | 4 GiB | 1,056 MiB | 928 MiB |

These are arithmetic projections, not measured process peaks. They exclude the
other storage above. Increasing context still requires matching runtime,
tokenizer, broker, capability/readiness and UI limits, supported request bounds
and an exact-source verified runtime release.

## Qualification and promotion contract

Freeze the exact Runner/Prism source, model/adapter, compiler, codec/layout,
selected precision, context/output bounds and device/driver identities before
acceptance. Meaningful focused controls must cover independent packed-code and
transform oracles, zero/extreme/nonfinite inputs, invalid lengths, scalar/NEON
agreement, attention outputs and complete graph logits. Complete-state and
lifecycle controls must cover checkpoint independence, restore atomicity,
reset/scale changes, edited/reused histories, cancellation and failure recovery.

Before generating quality captures, declare the corpus, seeds, context depths,
task failure criteria, acceptable numerical/error margins and the definition of
negligible quality loss. Use held-out long-context retrieval, task completion,
continuations and canon-sensitive creative work with independent human review.
Report confidence/uncertainty and all losing cases. Local reconstruction MSE,
one matching greedy answer or another model's benchmark cannot close this gate.

Compare admitted **complete-task** memory, latency, energy, retries and output
quality with FP16 on X1 and X2 independently. Require a material benefit under
the declared thresholds without a disqualifying quality, stability or comfort
regression before making the candidate standard/mandatory for that profile.
Keep an explicit FP16 recovery/comparison path and retain no-go outcomes.

Use the existing [research budget and power rules](research-register.md#experiment-scope-and-budgets).
Every owner-device workload defaults to five minutes total. A fixed, predeclared
dedicated workload may take the smallest justified allowance up to fifteen
minutes, including preparation, all comparison arms, cleanup and evidence.
Record AC before/after the aggregate workload and each arm, plus battery saver
and charge when available. Unknown or mismatched power disqualifies performance
comparisons; matching endpoint readings do not prove uninterrupted stability.
Do not retry a closed clock or split a longer matrix into repeated allowances.
Complete suites and benchmarks remain in owner-operated CI. Retain only the
[evidence needed](validation.md#evidence-retention) for the exact acceptance claim.
