# Inference algorithms and mathematical constraints

This reference defines numerical/state constraints for the [Rust engine](native-rust-engine.md).
The [research register](research-register.md) owns sources and derivations; [PLAN.md](../PLAN.md#runtime-and-correctness)
owns outstanding work. Pinned Prism controls same-artifact comparisons. Reuse requires immutable
source/license review; published speedups do not qualify Snapdragon execution.

[Hardware policy](hardware-capabilities.md#automatic-profile-selection) qualifies algorithms and placement
separately for prefill/decode, preserving numerical/state identity and charging handoffs. Batched
projections/chunkwise GDN are NPU prefill candidates; packed HVX, NEON and Adreno are decode
comparisons. Sequential scheduling alone provides no matrix-matrix reuse.

## Runtime semantics

| Mechanism | Contract |
| --- | --- |
| Prompt ingestion | `Decoder::ingest`/Qwen35 skip output-only work for intermediate uncached prompt tokens; final tokens and prefix hits retain logits, with computed-state checks, cancellation and rollback |
| Activation preparation | Compatible sibling projections share scoped transforms/quantization only when input, basis/permutation, width and quantizer match |
| KV storage | Explicit FP16 state retains exact binary16 bits in `Vec<u16>` and widens on reads. The [K4/V4 development default](quantized-kv.md) retains its packed approximate representation instead; GDN/convolution stays FP32 and budgets charge actual selected-state capacities. |
| Packed decode | NEON PQ2/PTQ1 and odd-group PQ2 preserve scalar quantizers, scale order and generic PQ2's fourth symbol |
| Prefill | Bounded 1–256-token blocks retain sequential arithmetic and whole-request rollback; block scheduling alone gives no tiled matmul reuse |
| Checkpoints | Sealed immutable KV pages may be shared; mutable tails and complete convolution/GDN state remain independently owned, with conservative snapshot budgets |
| Speculation & sampling | Generic `SuffixDrafter` proposals and greedy `Runtime::verify_draft_block` are separate from Runner serving, which disables speculation. The verifier captures a rollback checkpoint before evaluating/committing steps. `DFlash2DraftConfig` and `GdnVerifyTape` are standalone configuration/tape APIs; they do not establish an integrated trained drafter or checkpoint-free verifier. Swift sampling retains the fixed native top-20 contract. |

Source map: [runtime](https://github.com/symbiogenesis/prism-inference/blob/main/src/runtime.rs),
[decoder API](https://github.com/symbiogenesis/prism-inference/blob/main/src/lib.rs),
[graph/state/transforms](https://github.com/symbiogenesis/prism-inference/blob/main/src/graph.rs),
[kernels](https://github.com/symbiogenesis/prism-inference/blob/main/src/kernels.rs) and
[codecs](https://github.com/symbiogenesis/prism-inference/blob/main/tools/ternary/src/lib.rs).

Retained scratch proves storage reuse, not eliminated computation or model-level allocation cost.
State-only ingestion validates every computed state value while omitting unused output/logit checks.
Block prefill preserves cancellation/backpressure bounds and atomic commit.

The source review distinguishes three guarantees: lossless storage
reconstructs original bits; exact execution preserves the declared arithmetic and
state; corrected speculative sampling preserves the target distribution. The
last guarantee does not promise seed-for-seed text identity. Similar benchmark
scores alone establish none of them. A learned drafter can preserve the target
distribution when its proposals receive the required target verification and
sampling correction; training a drafter is not itself a target approximation.

## Scratch lifetimes

Prism's sequential graph shares attention-projection/FFN-up storage and their gate storage, then
convolves recurrent QKV in place after each raw element's last use. This is fixed reuse; the
[lifetime-planning review](research-register.md#t7-state-size-lifetimes-and-prefix-identity) covers
broader bounded-program transformations.

At Swift's widths, these buffers occupy 34,816 F32 slots versus 73,728 separately: **152 KiB less
nominal high-water payload** and four fewer vectors. Arithmetic/reduction order, committed state,
transaction deltas and checkpoints are unchanged. Frozen baseline hybrid logits/state fixtures cover
both FFN/QKV size orderings, failed evaluation, retry, restore and reset. The [graph implementation](https://github.com/symbiogenesis/prism-inference/blob/326d0dd45b2d3c61ea6ce8ebe67f8249bdfa22a6/src/graph.rs)
defines ownership. Allocator rounding/process peaks require separate measurement; nominal payload
does not establish process-memory or throughput gains. Moving arenas/general placement search need
actual-workload evidence.

## Cache, batch and transfer planning

The pinned [sglang-rs runtime scope](https://github.com/imReese/sglang-rs/blob/f9b5bade089e64032c07f5e751bd00a3af2b3847/README.md),
[architecture](https://github.com/imReese/sglang-rs/blob/f9b5bade089e64032c07f5e751bd00a3af2b3847/docs/architecture.md)
and [roadmap](https://github.com/imReese/sglang-rs/blob/f9b5bade089e64032c07f5e751bd00a3af2b3847/docs/runtime-roadmap.md)
are design references for Prism's existing prefix, prefill and accelerator work.
Source and license review precede reuse. Adopting its server, CUDA dependencies,
NexusKV, broad API routes or distributed serving is outside this scope. Swift
retains packed artifacts, its adapter/numerical contracts, one active generation
and the native Rust UI/broker containment boundary. Proposals supply no device
or performance qualification.

Reusable prefixes require checked finite page geometry, token-to-slot mappings,
immutable complete pages and private mutable tails. Publication waits for complete,
committed KV and recurrent/convolution checkpoints. Allocation failure, aborted
prefill or stale execution/model/adapter identity leaves no reusable partial entry.
Prepared batches identify tokens, positions, lengths, cache slots and uncached
token counts; context/output reservations and bounded chunk scratch remain required.

One logical cache owner separates scheduling and transfer plans from device
execution inside the contained inference worker. Typed plans distinguish submitted,
in-flight, completed and failed work, with bounded queues and waits. Consumers wait
for valid contents; publication waits for successful completion. Cancellation
revokes publication immediately, while buffer reuse or release waits for terminal
device completion or verified worker teardown. An abort request is not a fence.

Reusable device-visible pools remain conditional on the qualified API's registration
or staging needs. Their cost includes cold initialization, locked memory, copies,
synchronization and cleanup. Phase receipts identify cached/uncached tokens, unique
live pages, allocation/copy bytes, queue bounds and transfer/completion timing.
Invalid geometry, exhaustion, partial pages, late/failed completion and stale
publication require fault coverage. Whole-request parity, resources and measured
benefit remain the acceptance boundary under the existing [validation policy](validation.md).

## Packed lookup kernels

### Exact ternary tables for decode

[Bitnet.cpp, section 3.2](https://arxiv.org/html/2502.11880v1) distinguishes exact **TL1_1/TL2_1**,
using INT16 sums/two byte lookups, from approximate INT8-table **TL1_0/TL2_0**. The [official
implementation](https://github.com/microsoft/BitNet) is a technique reference subject to this
engine's `q8k_into`/`q8_into`, 128-weight scales, 256-activation Q8_K groups, 32-activation Q8_0
groups and floating scale/add order. Stock BitNet quantization is not this engine's specification.

The [CPU contract](cpu-kernels.md#packed-arithmetic) owns packed controls, including generic PQ2's **+2** symbol and bounds for every
LUT intermediate/correction; the ternary [Swift census](../config/swift.pq2-census.json) does not authorize ternary-only dispatch for
arbitrary tensors. Validated immutable representations may avoid repeated canonical decoding;
arbitrary public tensors still need rejection checks. `dot(code - 1, q) = dot(code, q) - sum(q)`
permits using `Q8KBlock.sums` under the same scale contract as lane subtraction.

The explicit [Pq2LutExecutor candidate](cpu-kernels.md#grouped-dispatch-hardware-half-widening-and-exact-lut-execution)
shares bounded table preparation across output rows in prepared matvec and graph
execution. Selection is manual. Tables rebuild per invocation, including siblings;
compare complete preparation/cache/dispatch cost against SDOT/NEON before selecting it.

### Vec-LUT for multiple prompt tokens

[Vec-LUT](https://arxiv.org/html/2512.06443v2) and its [implementation](https://github.com/OpenBitSys/vlut.cpp)
group lookups across prompt tokens and stream cache-sized tables using bounded INT16 partials/INT32
accumulation. Transfer requires unchanged quantizers/scales, bounds for all accepted inputs and
accounting for layouts, repacking, retained copies and scratch; published platforms do not qualify Swift.

Prism's token tiles share decoded weights across tokens; they do not implement
multi-token activation lookup tables.

## Chunkwise Gated DeltaNet prefill

The pinned Prism source includes explicit 8/16/32-token FFN projection tiles and a
separate `gdn-wy-f32-v1` triangular GDN mode, consumed by `Qwen35::ingest_block`.
Exact tiles retain sequential recurrent arithmetic. The GDN mode batches recurrent
input projections, preserves causal convolution and uses interval decay products
without cumulative-product division, then reconstructs outputs and final FP32
state. Its declared `1e-3 + 1e-3 * abs(reference)` qualification tolerance differs
from bit identity. Both paths require explicit empty-graph selection, extra scratch
reservation and whole-request rollback. New local Prism source exposes the exact
tile mode through an explicitly admitted private-service profile, retaining the
selected FP16, K4/V4 or K4/V3 cache. Packed-cache attention stays causal and uses
its existing codec without expanding history to FP16. Triangular GDN remains a
separate FP16 profile. Neither mode is enabled by Runner's current launch policy
or bundle. Source controls and work counts establish no actual-model parity or
prompt-time gain. The plan retains full-model, long-prefix, adapter and resource
qualification before application selection.

Packed tiles now have an exact baseline NEON integer path that shares one decoded
weight block across tokens, honors the graph's scalar/auto/NEON policy and retains
the scalar fallback. PQ2/Q8_K, odd PQ2/Q8_0 and PTQ1's native four-lane floating
accumulation keep their existing operation order. The follow-up source candidate
also shares the graph's existing private row-worker pool over disjoint output
slices. Caller-thread cancellation and joined tasks preserve the callback contract;
an extra 16 KiB reservation covers bounded scheduling storage. Compare whole
prompts against the ordinary row-parallel matvec path. The
[CPU kernel contract](cpu-kernels.md#packed-arithmetic) records bounds and the
separate decode candidate; neither changes the triangular GDN numerical contract.

[Gated Delta Networks](https://arxiv.org/html/2412.06464v3) derives chunkwise computation with
compact WY/UT structure; the [official FLA
implementation](https://github.com/fla-org/flash-linear-attention/blob/main/fla/ops/gated_delta_rule/chunk.py)
is an algebra reference. Scalar recurrent execution is the decode control; each chunk size, backend
and phase has a separate qualification identity.

For column keys/values, `S_t = alpha_t S_(t-1) (I - beta_t k_t k_t^T) + beta_t v_t k_t^T`. Chunking
preserves the real-arithmetic model while changing floating evaluation order. Its numerical contract
needs declared state/logit tolerances and long-prefix restoration checks, preserving normalization,
`head % ssm_keys`, convolution history, grouped/sign transforms and FP32 recurrent state.

## Attention locality and the I/O bound

The [attention I/O theorem](research-register.md#t4-attention-io-optimality-has-a-computation-model)
motivates tiling under its stated memory/computation model; it forecasts neither device speed nor
unconditional exact subquadratic work. FlashAttention also faces the rounding boundary below.

Scalar nonflash attention rounds **globally normalized probabilities to FP16 before multiplying V**.
Ordinary online-softmax merging changes that operation. Grouped-head tiling and contiguous V reads
can retain reduction order. A rounding-compatible normalization may retain scores or recompute them;
a conventional Flash path needs a distinct numerical contract and qualification. Do not share
checkpoints across incompatible backend identities.

## Hybrid cache reuse and speculative state

[Unified Radix Cache](https://www.lmsys.org/blog/2026-08-11-unified-radix-cache/) distinguishes
reusable full-attention KV paths from recurrent state valid at an exact prefix checkpoint. For at
most eight saved prefixes, linear-lookup cost is separate from payload-copy cost. State budgets
include unique pages, mutable tails, metadata, rollback and every retained GDN snapshot; shared
ownership cannot create mutable cross-snapshot aliases.

[SuffixDecoding](https://arxiv.org/abs/2411.04975) proposes tokens from prompt/history suffixes.
[SpecLA](https://arxiv.org/abs/2607.16673) retains verification factors and delays commit to
reconstruct accepted recurrent states. The generic greedy block verifier does not
qualify this tape-based approach or stochastic verification. SpecLA's
trained drafter/H100 evidence and reported FP32 state differences do not establish Bonsai
compatibility or exact replay.

Stochastic speculation needs proven acceptance/rejection correction, as in [Leviathan et
al.](https://proceedings.mlr.press/v202/leviathan23a.html); greedy agreement does not qualify
creative sampling. Retain request rollback even if candidate recovery becomes cheaper.

The published 0.1.25 `Runtime::verify_draft_block` and
`generate_speculative_inner` call target `evaluate` one token at a time.
New local Prism source adds an explicitly selected CPU tiled verifier:
`Qwen35::with_tiled_verification` reserves per-position logits separately and
shares layer and output-head packed weights across a bounded candidate block.
`Runtime::verify_draft_block` consumes that capability, verifies each candidate
against the preceding target distribution, and restores the complete checkpoint
before accepted-prefix replay and correction on rejection. Admission charges
proposed and replayed work; ordinary graphs retain sequential verification.
Each selected FP16 or rotated-cache profile retains its own arithmetic and state
identity; this does not make quantized caches equivalent to FP16. Triangular GDN
and installed external projection executors reject this CPU selection.

The local `Runtime::generate_sampled_speculative_until` consumer draws once from
the caller's actual target sampler at each output position. Deterministic suffix
proposals are point masses: a matching target draw accepts the proposal; a
different draw already samples the conditional residual. This preserves the
sampler's filters and RNG order when the batch logits are identical. It adds no
acceptance RNG and does not implement correction for a learned probabilistic
drafter. All-accepted blocks can feed another block directly; rejection or a stop
restores complete state and replays the accepted prefix before proceeding.

An explicitly configured private-service profile connects this consumer to
Swift's temperature 1, top-20 and top-p 0.95 sampler. It separately charges
verification logits and an additional complete-prefix checkpoint beside the
request rollback. Existing phase-timing capture rejects this selection because
its counters cannot describe speculative work. Greedy APIs and the
standalone `verify_stochastic_step` remain separate. This local candidate is not
in Runner's runtime bundle; Runner still requires speculation to be disabled.
Source integration establishes no serving speedup, actual-model parity or
sampler qualification.

A weight-amortizing verifier needs layerwise token-tiled target projections,
causal attention and convolution, and target logits at every candidate position.
Partial acceptance must restore the accepted prefix's complete KV, GDN and
convolution state before correction or bonus tokens commit. EOS, output caps,
failed publication and cancellation retain whole-request rollback. Qualify
greedy behavior first, then actual conditional proposal probabilities and
residual sampling under the deployed sampler. `GdnVerifyTape` and
`DFlash2DraftConfig` remain standalone APIs rather than an integrated verifier,
loaded drafter or checkpoint-free serving path.

[Inverse-state rollback can be singular or unstable](research-register.md#t5-chunking-recurrence-and-recovering-rejected-tokens);
floating gates can underflow/saturate and convolution overwrites history. Use snapshots or saved
factors with checked forward reconstruction.

### 2026 state-traffic and verification research

The [hybrid-state catalog](research-register.md#hybrid-recurrence-prefix-caching-and-speculative-decoding)
owns KVBuffer/TreeWY sources and applicability. [KVBuffer v2](https://arxiv.org/html/2605.19049v2)
Its main decoding gains use large batches; its low-request-rate Qwen3.5-4B
result includes no throughput gain, and FP32 comparisons have nonzero state or
output differences. Neither technique is implemented in the selected runtime:

| Candidate | Swift proof obligation |
| --- | --- |
| KVBuffer deferred state writes | Charge buffer reads, flush/materialization and precision conversion. Checkpoints must retain/materialize every pending decay/key/delta plus convolution history; buffered state cannot claim materialized-state identity. |
| TreeWY accepted-path reconstruction | Qualify topology, attention causality, convolution, failed commit and prefix-cache integration. Real-arithmetic equivalence does not imply bit identity; smaller state does not prove faster batch-one decode. |

### Trained parallel proposals and full-state verification

| Proposal/verification route | Swift boundary |
| --- | --- |
| DFlash target-feature diffusion drafts | Require compatible tokenizer, ternary weights, feature interface, adapter and CPU execution. Admit drafter weights/KV, retained target features and verification scratch; compare complete cycles. |
| DFlash 2 candidate scoring/path selection | Require provisioned drafter identities and actual conditional proposal probabilities: coherent paths alone do not preserve the target distribution. Its released Qwen3.8-27B drafter is no Swift/LoRA qualification. |
| VeriCache compressed drafts/full-KV verification | Correct stochastic acceptance separately from greedy identity. Full plus compressed KV can increase RAM; KV-only verification omits GDN/convolution recovery, and multi-request GPU overlap does not transfer to this one-request CPU runner. |

The [research catalog](research-register.md#hybrid-recurrence-prefix-caching-and-speculative-decoding)
retains sources, mechanisms and evaluation scope. DFlash 2 is an author report with released
drafters, not another peer-reviewed paper; learned components remain separate artifacts.

Published [Bonsai 2-specific DFlash2 drafters](https://huggingface.co/naklitechie/Qwen3.8-27B-DFlash2-ternary-bonsai2)
make compatible-artifact screening concrete. The author reports workload-dependent
results, slower creative chat in one Apple comparison and changed greedy CUDA
output near ties during batched verification. These observations qualify neither
Swift's adapter profile nor its stochastic sampler or Snapdragon execution.
Validate tokenizer, transformed basis, shared embeddings/head and feature taps
against exact artifact identities. The published tap indices `[5,19,33,47,61]`
differ from the standalone local default `[2,17,32,47,63]`; this is an interface
check to resolve before integration, not an active serving defect. A separate
Qwen reference with MTP tensors does not establish an admitted Swift MTP path.
Adaptive short draft lengths may reduce wasted verification, but acceptance
alone cannot establish benefit without full drafting/verify/recovery cost.

### Certified sparse attention is a separate approximation

The [approximation
catalog](research-register.md#approximation-quantization-theory-and-numerical-analysis) records
sources and [T9](research-register.md#t9-per-call-certificates-need-a-run-wide-failure-budget)
defines run-wide failure budgeting.

| Method | Guarantee boundary |
| --- | --- |
| vAttention (Q22) | Practical pilot/CLT budgets differ from conservative relative operator-output certificates. `(epsilon, delta)` gives neither exact output nor an answer-wide certificate. |
| Perturbed retrieval (Q23) | Require exact candidates, fresh independent per-step perturbations and a run-wide head/layer/step failure budget. Charge the linear key scan and retained full KV/indexes; bounded value reads do not imply bounded total work or smaller RAM. |
| DoSA (Q24) | Bounds preserve an existing sparse indexer's Top-K, including numerical recovery, rather than dense attention. Swift lacks its learned indexer/artifacts. Transfer bounded early rejection with full fallback, not silent dense-token omission. |

Both certified approximation methods need references consistent with Prism's FP16 normalized-
probability rounding before their bounds transfer.

## Compression and approximation constraints

[T1's packing bound](research-register.md#t1-packing-limits-and-the-difference-between-sparse-and-small)
requires 26 code bytes per independent 128-trit block, already used by PTQ1 plus its two-byte scale.
It leaves correlated/cross-block coding open; the [packing decision](swift-memory.md#packing-decision)
accounts for execution and indexing costs.

| Research | Constraint |
| --- | --- |
| [Qualcomm multiplication-only GDN inverse](https://arxiv.org/abs/2606.06034) | Truncated Neumann/residual correction is a separate approximation after faithful chunkwise GDN. Phone NPU results do not qualify Windows X Elite. |
| [Near-I/O-optimal approximate attention](https://arxiv.org/abs/2605.23751) | Transfer improvements use approximation; require separate quality/resource qualification. |
| [Signed-digit LUT KV](https://arxiv.org/abs/2608.03229) | Changes KV representation/precision for a hardware datapath; separate from exact FP16-bit storage. |
| [Gated DeltaNet-2](https://arxiv.org/abs/2605.22791), [Sherry](https://arxiv.org/abs/2601.07892), [ReaDy](https://arxiv.org/abs/2610.20503) | Changed gates, sparsity or trained speculative components require compatible artifacts, not inference-only substitution. |

[T5](research-register.md#t5-chunking-recurrence-and-recovering-rejected-tokens) derives the finite
Neumann expansion and explains why spectral radius zero cannot bound low-order error or
intermediate growth. Check signed Gram entries/ranges before applying approximation arguments.

## Validation and promotion

[Validation policy](validation.md) applies independently to each change. Exact integer/storage work
needs bitwise fixtures, scales, malformed-input and -128/+2 cases. Reordered floats need declared
logits/KV/GDN and full-continuation tolerances. Both require checkpoint isolation, edits, cancellation,
failed recovery/reset and memory-budget checks.

Local builds/focused correctness checks are allowed; full suites, benchmarks and performance captures
remain CI-only. Promotion requires same-artifact/adapter target-laptop comparisons and independent
LoRA TPS/quality gates. Synthetic parity, smaller files and other-device results are insufficient;
paper speedups cannot be multiplied.
