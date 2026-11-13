# Inference research and proof register

This register holds reusable proof obligations and optional research sources for
Swift Bonsai 2. Topic contracts define supported behavior; [PLAN.md](../PLAN.md)
owns outstanding work. A paper, report or source example neither approves an
implementation nor qualifies a device/default. Derivations below are reviewable
arguments, not machine-checked proofs. Historical reviews and trial diaries belong
in Git history.

## Existing evidence and project scope

The [runtime contract](native-rust-engine.md) and [inference defaults](inference-defaults.md)
define Swift's Windows ARM64 scope. Locks/metadata take precedence over similarly
named paper models. Prism owns kernels; Runner owns provisioning, admission,
orchestration and [native presentation](ui-architecture.md).

Evidence labels: **theorem** = published result under stated assumptions;
**derivation** = local algebra/counting; **paper** = authors' algorithm/evaluation;
**implementation** = inspectable source needing revision/license/conformance
review; **report** = author/maintainer account. Trace discussion leads to originals.

## Experiment scope and budgets

Predeclare candidate count, shapes, prompts/tokens, memory and complete workload
estimate under [validation](validation.md) and [owner-device budget/power policy](../AGENTS.md#owner-device-execution-budgets).
Use available native X/X2 hardware, compatible published artifacts and source-bound
prebuilt tools. WSL/emulation, unavailable kernels, full retraining and architecture
changes are outside this laptop lane.

## Priorities for this model and hardware

These are applicability judgments, not measured rankings. Check the pinned Prism
implementation before proposing existing work again; PLAN owns execution order.

| Priority | Candidate | Decisive question |
| --- | --- | --- |
| 1 | Exact packed decode: BitNet/T-MAC tables, NEON layouts | Can unpack/dot savings retain quantizers, scales, integer bounds and floating reduction order without expansion? |
| 2 | Vec-LUT/cache-tiled batched projections | Does weight reuse across prompt tokens repay LUT construction, packing and scratch? Sequential blocks alone do not reuse tiles. |
| 3 | Faithful chunkwise GDN | Can matrix prompt work preserve grouped/sign transforms within declared long-prefix/restore tolerances? |
| 4 | Immutable KV pages/sparse recurrent checkpoints | Which allocation dominates: KV, recurrent snapshots, transaction scratch or mappings? |
| 5 | Grouped-head KV reuse/bounded attention scratch | Preserve FP16 probability rounding, or declare a distinct numerical/state identity. |
| 6 | Suffix/prompt-lookup speculation | Does creative-prose acceptance pay for block verification and recurrent recovery without second-model weights? |
| 7 | Fusion, persistent workers, prepared plans | Does complete-request profiling justify more scheduling complexity? |
| 8 | Adreno/Hexagon phase placement | Include signed Windows access, packed support, mappings, state handoff, duplicate weights and sustained power. |
| Conditional learned-proposal lane | Compatible published trained drafters | An unchanged target with correct conditional verification can preserve its distribution; artifacts, feature interfaces, memory and complete-cycle benefit still need review. |
| Separate approximation lane | KV quantization, approximate attention/GDN, pruning | Compare changed target precision/computation/artifacts as distinct profiles. |

Implementation availability must be checked against the selected source before
proposing an experiment. Generic LUT/verifier APIs, configured draft interfaces
and token batching do not establish production consumers, compatible trained
artifacts or true multi-token matrix execution. Source work counts do not
establish complete-request benefit. Bit-identical storage, identical float
execution, distribution-preserving sampling and similar task scores are distinct claims.

## Mathematical results and proof obligations

### T1. Packing limits and the difference between sparse and small

**Derivation:** an independent 128-symbol ternary block has `3^128` strings, so an
injective fixed-length encoding needs `ceil(log2(3^128)) = 203` bits, or 26 bytes.
PTQ1's 26 code bytes plus unchanged two-byte scale reach that independent,
byte-aligned bound. Correlated/variable-length/cross-block coding remains possible
but must pay for indexes, padding, decoding and random access.
[Swift's 32.76154% zeros](swift-memory.md#packing-decision) miss BITCOS's
37.5% ideal break-even. Zeros do not skip dense-loop work; sparsity must repay
indexes and irregular execution.

### T2. Traffic, arithmetic and the whole-request ceiling

[Roofline](https://escholarship.org/uc/item/5tz795vq) (Williams/Waterman/Patterson, 2009)
models `P <= min(P_peak, B*F/Q)`, hence `t >= max(F/P_peak, Q/B)` for work `F` and
traffic `Q` at a chosen memory boundary. If each decode step streams `W` weight
bytes there, `tokens/s <= B_effective/W`. This does not prove achieved peak or
advertised-bandwidth saturation; cache/batching/speculation change traffic, and
GGUF length does not measure bytes/token. Prefill can amortize tiles across tokens.

For fraction `f` sped up by `s`, [Amdahl's relation](https://doi.org/10.1145/1465482.1465560)
is `speedup = 1/((1-f)+f/s)` if other work is unchanged. A 42x drafting microbenchmark
cannot imply 42x inference. Overlap retains the slowest stage's service-time limit;
bounded queues trade latency against memory rather than create compute.

### T3. Exact low-bit dots need bounds at every intermediate

**Derivation:** signed i8 `q_i` (including `-128`) and ternary `w_i` give
`abs(sum(w_i*q_i)) <= 128*n`; at `n=128`, 16,384 fits i16. Generic PQ2 also permits
`+2`: conservative absolute bound 32,768, tight final range `[-32768,32512]`, still
i16. That does not certify LUT construction, partial sums or bias corrections.
For codes `0..3`, `sum((code_i-1)*q_i) = sum(code_i*q_i)-sum(q_i)`, but the
uncorrected 128-term dot spans `[-49152,48768]` and needs wider storage.
Preserve grouping/scale order; checked scalar Rust does not check wrapping SIMD.
[Bitnet.cpp](https://arxiv.org/abs/2502.11880) distinguishes exact INT16 and approximate
INT8 tables; [T-MAC](https://arxiv.org/abs/2407.00088) also quantizes tables and may
approximate aggregation. LUTs are not automatically exact Q8_K/Q8_0 replacements.

### T4. Attention I/O optimality has a computation model

[Saha/Ye, ICML 2024](https://proceedings.mlr.press/v235/saha24a.html) prove
FlashAttention I/O-optimal up to constants for fast memory `M >= d^2` in their
model, with separate small-memory results: no universal latency optimum or
unconditional exact subquadratic attention follows.
[FlashAttention](https://arxiv.org/abs/2205.14135) merges tiled online-softmax
statistics `(m_a,l_a,o_a)`/`(m_b,l_b,o_b)` by rescaling sums to `m=max(m_a,m_b)`.
That real-arithmetic identity does not reproduce Prism's globally normalized
probabilities rounded to FP16 **before V**. Preserve that boundary or declare
separate tolerances and state identity.

### T5. Chunking recurrence and recovering rejected tokens

[Gated Delta Networks](https://arxiv.org/abs/2412.06464v3) derives triangular/WY
chunking. In column-vector convention,
`S_t = alpha_t*S_(t-1)*(I-beta_t*k_t*k_t^T) + beta_t*v_t*k_t^T`.
Real-arithmetic equivalence permits parallel prompt work; reordered floats still
need logits/state and long-continuation checks.

**Derived rollback limit:** transition eigenvalues `alpha_t` and
`alpha_t*(1-beta_t*||k_t||^2)` can be zero/near-zero, making the transition
singular or inversion unstable; convolution overwrites history too. Retain snapshots or saved factors
with forward reconstruction and atomic commit. For strictly triangular size-`c`
`L`, `L^c=0` and `(I-L)^(-1)=I+L+...+L^(c-1)` exactly, but short-truncation error
depends on powers/norms of `L`, not spectral radius zero alone. This constrains
[multiplication-only approximations](inference-algorithms.md#chunkwise-gated-deltanet-prefill).

### T6. Exact speculative sampling preserves a distribution

[Leviathan/Kalman/Matias, ICML 2023](https://proceedings.mlr.press/v202/leviathan23a.html):
for normalized target `p`, proposal `q`, propose `x~q`, accept with
`min(1,p(x)/q(x))`, otherwise sample normalized `(p-q)_+`. Accepted mass `min(p,q)`
plus rejected correction recovers `p`. Apply conditionally at each accepted prefix
with the actual temperature/top-k/top-p/repetition policy. Distributional equality
does not imply seed/draw-order identity; greedy agreement is narrower.
Recurrent/convolution state must match the accepted prefix, not just a KV watermark.
Under independent constant acceptance `a`, `k` proposals plus a target token give
expected progress `1+a+...+a^k`; charge drafting/verification/recovery. Creative
prose acceptance can differ sharply from repeated code/text.

### T7. State size, lifetimes and prefix identity

**Derived payload** from [metadata](../config/swift.metadata.json):
`16*8192*4*(256 K+256 V)*2 = 536,870,912 bytes = 512 MiB` full-attention KV at 8K.
Only the 16 full-attention layers count here; the 48 recurrent layers separately
retain [149.625 MiB FP32 matrix/convolution state](native-rust-engine.md).
Capacity, alignment, snapshots, scratch and allocator metadata are extra.
Physical accounting counts unique pages and independent recurrent snapshots,
including transient restore/rollback. Admission conservatively charges every
referenced sealed page per snapshot without relying on sharing; this survey
changes neither budget nor policy.

For deterministic execution, exact tokens/reset state/positions plus model, adapter,
transforms, precision and execution contract determine exact prefix state. A recurrent checkpoint cannot
stand for a shorter position; immutable sharing saves copies only with independent
mutable tails. Simultaneously live buffers bound storage; reuse waits for the
last use, including asynchronous device work. [Memory Allocation for Constant-Bounded Programs](https://arxiv.org/abs/2608.14471v1)
(2026-08-14 preprint) allows defragmentation/control-flow transformations beyond
a fixed arena. Existing [scratch reuse](inference-algorithms.md#scratch-lifetimes)
saves 152 KiB nominal payload, not a proved process peak.

### T8. Floating-point and approximation claims

[Goldberg](https://docs.oracle.com/cd/E19957-01/806-3568/ncg_goldberg.html) explains
reassociation, FMA, rounding and underflow differences. Integer dots, decoded
weights, logit tolerances, sampled-token identity and task quality are distinct
guarantees; none implies all others. Lossy KV compression changes attention; recurrent
quantization can accumulate prefix-wide error. Instantiate metric, Q/K norms,
randomness/failure probability, dimension and horizon before using a bound:
head/vector error does not certify creative-answer quality.

### T9. Per-call certificates need a run-wide failure budget

**Derivation:** conditional per-call failure bounds `delta_i` imply
`P(any failed certificate) <= sum(delta_i)` by the union bound. For at most `K`
calls, `delta_i <= delta_total/K` suffices without independence, but guarantees
must survive adaptive queries. Reused randomness can violate assumptions;
[perturbed retrieval](https://arxiv.org/abs/2610.24991v1) uses fresh perturbations
per decoding call. An output certificate still certifies neither token nor answer.

### T10. Recurrent quantization error depends on the trajectory

**Derivation:** for the same affine transitions `A_t`, state difference obeys
`E_t=E_(t-1)*A_t+R_t`, where `R_t` is local update error. Submultiplicativity gives
`norm(E_T) <= norm(E_0)*product(norm(A_j)) + sum_i(norm(R_i)*product_(j>i)(norm(A_j)))`.
This conditional bound does not establish uniform stability when perturbed hidden
states change future gates. Small one-step MSE misses coherent persistent error.
[Low-rank bypass/directional rounding](https://arxiv.org/abs/2610.15709v1) and
[decay-aware precision](https://arxiv.org/abs/2608.27513v2) address it experimentally,
without proving exact FP32 continuation.

### T11. Quantization search theorems optimize a stated objective

[CoS-VQ](https://arxiv.org/abs/2610.37336v1) proves surrogate-search properties over
finite candidate pools, not a global quantized-model optimum.
[YAQA](https://arxiv.org/abs/2505.22988v3) bounds second-order error under Hessian,
rounding and incoherence assumptions, not unconditional nonlinear KL/answer error.
[FastKron](https://arxiv.org/abs/2608.06291v2) proves core rounding equivalence and
reduced depth/work in its arithmetic model, excluding other offline preparation
and token generation. Retain objective, feasible set and included work for every
"optimal" or "equivalent" claim.

### T12. A logit-error bound preserves greedy choice only with a margin

**Derivation:** absolute per-logit error `epsilon` can shrink the top-two gap
`Delta` by `2*epsilon`, so `Delta > 2*epsilon` retains a unique greedy winner.
Arbitrarily small errors can swap near ties. Maximum-logit difference alone does
not prove token identity; greedy ranks do not prove equal sampling probabilities.
Reordered kernels/quantizers still need sampler and continuation checks.

## Research catalog

Keep research scoped to a demonstrated bottleneck. These are references for
selected design questions, not a proposed implementation backlog or a performance ranking.

### Packed projections, CPU kernels and compression

| Reference | Useful question and boundary |
| --- | --- |
| [BitNet](https://arxiv.org/abs/2502.11880), [T-MAC](https://arxiv.org/abs/2407.00088), [Vec-LUT](https://github.com/OpenBitSys/vlut.cpp) | Can packed table/layout reuse repay preparation and scratch? Preserve intermediate bounds, all generic PQ2 symbols, scale groups and floating order. Different activation/table quantizers are separate profiles. |
| [BITCOS](https://arxiv.org/abs/2609.16338v1) | A sparse packing must beat PTQ1 after indexes/padding. Swift's census already rules out its proposed same-scale layout on serialized size; see [packing](swift-memory.md#packing-decision). |
| [Signed floor division](https://www.benjoffe.com/fast-floordiv) | Consider only a measured signed constant-divisor hotspot with the existing floor-rounding contract; see [CPU constraints](cpu-kernels.md#signed-constant-floor-division). |

Full-precision entropy codecs do not establish savings on already packed ternary
weights. Charge random access, indexes, reconstruction, per-token decoding and
retained copies before selecting a new codec.

### Hybrid recurrence, prefix caching and speculative decoding

| Reference | Useful question and boundary |
| --- | --- |
| [Gated Delta Networks](https://arxiv.org/abs/2412.06464v3) | Chunkwise algebra can parallelize prompt work, but reordered floats require complete logits/state and continuation acceptance. |
| [Sparse Prefix Caching](https://arxiv.org/abs/2605.05219v1) | Sparse checkpoints trade retained state for exact suffix recomputation in edited/branching histories. Append-only chat can first use the existing [latest prefix](native-rust-engine.md#latest-prefix-reuse). |
| [Exact speculative sampling](https://proceedings.mlr.press/v202/leviathan23a.html), [SuffixDecoding](https://arxiv.org/abs/2411.04975v3) | Proposals need conditional target verification and accepted-prefix KV/GDN/convolution recovery. Greedy agreement does not qualify the deployed sampler. |
| [KVBuffer](https://arxiv.org/abs/2605.19049v2) | Deferred state writes must charge factor reads, conversion, flush/materialization and pending convolution history; batch gains do not establish Swift batch-one benefit. |
| [Bonsai DFlash2 draft](https://huggingface.co/naklitechie/Qwen3.8-27B-DFlash2-ternary-bonsai2) | A compatible trained proposal needs exact target/adapter, tokenizer, feature and shared-head identities, native resource admission and full stochastic verification. Another Bonsai target does not establish Swift compatibility. |

Tree/phrase/parallel proposals remain conditional on a useful verifier, compatible
published auxiliary and complete-cycle benefit. Smaller draft weights do not
establish runtime support or sampled-target preservation.

### Attention kernels and memory hierarchy

[FlashAttention](https://arxiv.org/abs/2205.14135), the [attention I/O theorem](https://proceedings.mlr.press/v235/saha24a.html)
and [PagedAttention](https://arxiv.org/abs/2309.06180) inform tiling, state lifetimes
and immutable sharing. Preserve the deployed FP16 probability boundary or declare
a different numerical profile. Hardware-specific GPU/RVV results do not forecast
NEON latency; low traffic does not prove low complete-task cost. Exact BF16 cache
codecs are not automatically FP16 or already-compressed K4/V4 codecs.

### Approximation, quantization theory and numerical analysis

| Reference | Useful question and boundary |
| --- | --- |
| [TurboQuant](https://arxiv.org/abs/2504.19874v1), [KIVI](https://proceedings.mlr.press/v235/liu24bz.html) | Rotated/asymmetric KV quantization changes attention. The current [K4/V4 contract](quantized-kv.md) owns quality, resource and explicit FP16 recovery gates. |
| [DAMP](https://arxiv.org/abs/2608.27513v2), [low-bit recurrent updates](https://arxiv.org/abs/2610.15709v1) | Recurrent error is trajectory-dependent. Charge factors, scales, rounding state and complete checkpoint/rollback lifetimes; local MSE is insufficient. |
| [Certified sparse attention](https://arxiv.org/abs/2610.24991v1) | Per-call certificates need exact candidates, appropriate fresh randomness and a run-wide failure budget. Indexing/full-KV overhead remains charged; attention output bounds do not certify complete answers. |
| [QuIP#](https://arxiv.org/abs/2402.04396v2), [CoS-VQ](https://arxiv.org/abs/2610.37336v1), [YAQA](https://arxiv.org/abs/2505.22988v3) | New codebooks/rounding optimize stated objectives under assumptions. They are new-artifact research, not lossless PQ2/PTQ1 conversion or creative-quality guarantees. |
| [Goldberg](https://docs.oracle.com/cd/E19957-01/806-3568/ncg_goldberg.html), [Higham](https://nhigham.com/wp-content/uploads/2023/10/high93s.pdf) | Reduction order, precision, FMA and range assumptions determine error bounds. Small logit error does not guarantee identical tokens near ties. |

Low-rank/SVD caches, omitted checkpoint heads and learned sparse indexes remain
conditional capacity research. They need an actual bottleneck, bounded native
cost and full quality/state qualification; they cannot claim exact dense restoration.

### 2026 mobile acceleration and speech

[Qualcomm's Adreno talk](https://www.iwocl.org/wp-content/uploads/IWOCL-2026-Wang-Llamacpp.pdf)
is a packed-layout and scheduling reference, subject to public Windows kernel
availability. [PowerBench](https://arxiv.org/html/2607.05475v2) and
[Sereno](https://www.usenix.org/conference/osdi26/presentation/xin) motivate
whole-task power and foreground-responsiveness measurement. Phone/GPU results
and private assembly do not establish supported Windows Swift placement.

[Qwen3-TTS](https://arxiv.org/abs/2601.15621) separates talker, residual predictor
and codec rates. Stage fusion or RVQ speculation must retain voice identity,
accepted-prefix state, bounded scratch and cancellation. Model-family similarity
does not establish compatible streaming ASR state or artifact support. The
[speech measurement contract](performance-expectations.md#q4-speech-resource-comparison)
requires real-device audio and independent listening.

### 2026 systems, measurement and output-budget work

[Roofline](https://escholarship.org/uc/item/5tz795vq), [ProfInfer](https://proceedings.mlsys.org/paper_files/paper/2026/hash/03dbc11a22e79cd38bea53cf518c2371-Abstract-Conference.html)
and [MLPerf Client](https://mlcommons.org/benchmarks/client/) inform attribution,
separate TTFT/decode and workload/quality controls. Use Runner's
[measurement protocol](performance-expectations.md#measurement-and-comparison).
Windows instrumentation, phase coordination and polling costs need native evidence.

Length predictors, trained budget tokens and multi-candidate stopping alter work
or answers; treat them as [output-budget experiments](inference-defaults.md#output-budget-experiments),
including predictor/extra-prefill cost and useful-answer quality. Fewer tokens do
not prove faster or better completed tasks.

### Conditional learned corpus retrieval

[Apple's learned retrieval](https://machinelearning.apple.com/research/faster-retrieval)
and its [paper](https://arxiv.org/html/2603.08001v3) are leads only after corpus
retrieval demonstrates a need for embeddings and vector search becomes a measured
bottleneck. For a large, stable corpus and representative queries, KeyNet maps
query vectors before an existing index; SupportNet is a separate cluster-routing
candidate. Neither is an active dependency, runtime integration or device trial.

Applicability depends on exact corpus, chunking, encoder and learned-model
identities, with invalidation or requalification after changes. Original-query
candidate scoring and ordinary retrieval fallback remain required. Top-1 recovery
does not establish passage relevance or complete scene-packet quality: evaluation
must cover multiple useful passages and complete-task latency. Offline training,
exact-target generation, vectors/index and auxiliary weights all count toward
native Windows ARM64 time and shared-memory budgets. GPU/batched results supply
no Snapdragon benefit or generation/attention shortcut.

## Turning a lead into an experiment

1. Identify the bottleneck and missing evidence; check existing Prism consumers
   before selecting a mechanism.
2. Declare numerical/state identity: exact bytes/integers, reordered floats with
   tolerances, distribution-preserving sampling or explicit approximation.
   Pin source/license, artifacts, adapter, checkpoint and complete resource costs.
3. Implement kernels in Prism, preserving Runner's policy and native UI. Qualify
   tails, generic PQ2 `+2`, i8 `-128`, continuation, edits, restore, cancellation
   and failed commit using independent controls.
4. Apply [promotion criteria](validation.md#promotion-criteria),
   [measurement requirements](performance-expectations.md#measurement-and-comparison)
   and [evidence retention](validation.md#evidence-retention). Include application
   delivery and speech costs; put outstanding work in PLAN.

## Keeping the register current

Keep only sources serving a current design decision or selected research question.
Pin the version/revision when adopting a source, retain its assumptions and
applicability limits, and verify mutable claims before use. Remove completed
review diaries and superseded trial summaries; keep useful numerical contracts
in their owning topics and outstanding work in PLAN.
