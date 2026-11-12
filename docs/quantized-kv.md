# Native Snapdragon quantized KV

This is the contract for the **TurboQuant-inspired K4/V4 development default
implemented in Prism** for Snapdragon X1 Elite and X2 Elite Extreme. Runner's
0.1.25 development pin activates this reviewed default without qualifying a new
device profile or increasing the 8K context limit.
[PLAN.md](../PLAN.md#bounded-calibration-and-optional-precision)
owns remaining delivery and qualification work.

The new [adaptive-context source](adaptive-context.md) uses the selected cache's
actual record cost when computing memory-backed capacity for each prompt. It
requires a new verified runtime bundle and Runner repin; the old 0.1.25 binary
does not gain that behavior from a broker or UI update alone.

The owner selects **K4/V4 (`rsq4`) as the development default and required cache
for ordinary Swift sessions**. This decision accepts approximate answers and
pending actual-model quality, memory and device qualification; perfect
reconstruction, identical tokens, FP16 logits and a completed comparison packet
are not prerequisites for this development default. The paired packet below
remains the standard for a measured quality/memory qualification claim.

Explicit `f16` selection starts a comparison/recovery session. A request or failed
compressed session cannot silently choose FP16. Runner's reviewed 0.1.25 pin
selects `rsq4`; the historical 0.1.23 comparison control retains FP16. The
development selection requires the exact verified runtime bytes and never
waives artifact verification. Qualify X1 and
X2 separately before making device-specific benefit claims.

## Source selection and current checks

The safe codec API is `kv_quant::Quantizer::{new, scalar}`. Graph construction
selects `graph::KvCachePolicy::{F16, Rotated4, Rotated4Keys3Values}` explicitly;
generic graph constructors retain FP16 reference behavior. Normal Swift-session
loaders, raw CLI generation with verified Swift weights and the private Swift
service default to `rsq4`; other raw CLI models retain FP16.
The raw generation CLI accepts
`--kv-cache f16|rsq4|rsq4k3v` and records the policy and current graph identity in
its completion metadata. Tokenize mode rejects this generation-only option.

`SwiftSession::load_profile_with_kv_cache` and the private server's trusted
`--kv-cache` option expose the candidate at the existing fixed 8K service bound.
Client request bodies cannot select precision. Non-FP16 readiness reports
`kv_cache` and the actual `startup_graph_identity`; the startup label remains
accurate when a later request changes the adapter scale. Runner's trusted launch
policy selects the cache from the exact reviewed runtime source and checks the
matching cache and startup graph identity; older releases retain their FP16
contract. Application use requires a published, verified Prism bundle with that
source identity. The trusted launch option `--kv-cache f16|rsq4` permits explicit
recovery without exposing precision in client request bodies.

The model-free capability inventory still lists the FP16 graph contracts and is
not the cache selector. Rotated-profile discovery remains follow-up integration
work; current cache selection relies on reviewed source and live readiness.

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

K4/V4 is the selected development default; K4/V3 is the opt-in capacity candidate.
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

## Qualification contract

Freeze the exact Runner/Prism source, model/adapter, compiler, codec/layout,
selected precision, context/output bounds and device/driver identities before
acceptance. Meaningful focused controls must cover independent packed-code and
transform oracles, zero/extreme/nonfinite inputs, invalid lengths, scalar/NEON
agreement, attention outputs and complete graph logits. Complete-state and
lifecycle controls must cover checkpoint independence, restore atomicity,
reset/scale changes, edited/reused histories, cancellation and failure recovery.

### First practical promotion packet

Start with K4/V4 alone against FP16. K4/V3 is a later capacity experiment and does
not enlarge this first comparison. The fixed packet contains three held-out tasks:
a deep-context factual retrieval, a practical instruction-following task and a
canon-sensitive creative continuation. Declare the exact prompts and required
facts/constraints before generating either arm. The retrieval case must reach at
least 7,168 occupied tokens within the existing 8,192-token total bound; keep the
same reserved output in both arms. Do not tune the codec or replace losing prompts
after observing results. Use the same immutable original PQ2 model, standard
adapter at scale 1.3, prompt/template, seed, sampler, worker count, accelerator
placement and request limits in each arm.

The initial decision margins are deliberately practical:

| Observation | Passing margin |
| --- | --- |
| Useful answer quality | A human reviewer, with candidate labels hidden, scores complete answers from 1 (unusable) to 5 (fully useful), with both arms at least 3 (usable). Mean compressed quality may be at most 0.5 lower, with no individual task more than 1 lower. All predeclared objective checks passed by FP16 remain passing; no new material factual, canon or task-completion failure is accepted. |
| Cache memory | Actual packed K/V storage, including scales, is at most 35% of matched FP16 K/V storage at the same occupied positions. Verify that no full-history FP16/F32 mirror remains. |
| Process memory | Near-full-context complete-task peak private bytes fall by at least 128 MiB, with matched measurement coverage and checkpoint/recovery costs included. Report absolute peaks and the actual saved fraction of total process memory. |
| Responsiveness and reliability | Where matched-power timing is available, no complete task is more than 10% slower. Otherwise retain an unqualified latency result and require the owner to accept observed responsiveness before recording capacity-only qualification. No extra failed/retried request or selected-profile state, cancellation or recovery regression is accepted. A speedup or energy improvement is not required for a memory-led decision. |

Use [`bonsai-tools kv-promotion`](../tools/bonsai/README.md#practical-compressed-kv-review)
to freeze the packet and review its retained complete answers, matched cache
positions, quality scores and process-memory observations offline. It reports a
scoped owner-review result; it neither captures a model run nor changes defaults.
The suggested prompts and exact request fields are documented with the command.

`Qwen35::state_memory` and `SwiftSession::state_memory` supply committed cache
payload, the calculated full-context payload bound and live state capacities;
the raw generation CLI includes `state_memory` in its completion record.
Retained capacities after reset remain charged. These observations exclude model,
workspace, external checkpoints, allocator and process/device memory; collect
complete-task process peaks independently rather than treating a state budget or
full-context calculation as a measured saving.

Bind the declaration and complete paired answers to immutable source, executable,
model/adapter and profile identities. Retain every pair, objective result and
human score, including losing cases. Report cache storage separately from process
peaks: the projected 8K K4/V4 saving is 380 MiB of attention payload, not a 74%
reduction of the whole model process. Missing memory observations, quality review
or a comparison arm leave the decision pending; source tests alone cannot pass it.
A baseline that fails its own required task is not positive equivalence evidence.
Unknown or mismatched power prevents a latency/energy verdict; it does not erase
valid directly measured cache/process memory and answer-quality observations.
Keep that capacity-only result and its usability-acceptance limit explicit.

This small packet supports a scoped measured qualification, with limited confidence
about unseen tasks; it does not establish universal quality equivalence. It is
follow-up validation of the owner-selected development default. Keep
ordinary application release, isolation and installation obligations separate.
Longer held-out reviews can use the same retained complete answers where applicable,
without making a broader matrix a prerequisite for this selected-device decision.

### Delivery and profile decision

Deliver an exact-source published and verified Prism runtime bundle and matching
Runner launch, readiness and context contracts. The reviewed source selects
`rsq4` by default and requires it for that session; requests cannot quietly
override it or fall back to FP16 after a failure. Keep an explicit FP16
comparison/recovery profile, selected at a safe fresh-session boundary. Do not
reduce process admission or increase context from cache arithmetic alone.

Passing the source/lifecycle controls and paired actual-model packet qualifies
the measured device profile. A pass on X2 leaves X1 qualification pending, and
vice versa; the shared development selection does not transfer evidence between
them. Record the selected source/device scope and every no-go. K4/V3, precision changes to GDN or
convolution state and context above 8K require separate decisions.

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
