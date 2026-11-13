# CPU kernels and X2 thread evaluation

Baseline NEON supports PQ2, odd-group PQ2 and PTQ1 with scalar reference/fallback.
This guide owns arithmetic, dispatch, qualification and the
[X2 thread screen](#x2-cpu-thread-screen). Synthetic parity qualifies neither
whole-model parity nor device speedup. Runner's active
[release lock](../config/prism-inference.lock.json) pins Prism 0.1.25; source candidates
and historical publisher screens retain their own identities. The fixed CPU
comparison baseline remains 0.1.23 with explicit FP16 KV. This development repin
closes no CPU qualification gates; [PLAN.md](../PLAN.md#cpu-kernels) owns remaining work.

## Cross-generation CPU promotion

Shared CPU work reductions may be promoted for X1 and X2 using matched
complete-request CPU benefit on one selected family; X2 evidence is sufficient
without a separate X1 benchmark when the applicability review below passes.
This permits a shared Prism release, Runner repin and enabling the reviewed
common CPU path on X1. It does not establish a measured X1 speedup or qualify
the complete X1 runtime/device profile.

The architectural basis is shared ARM64 execution, with baseline NEON and
runtime-checked optional instructions in this implementation. The CPUs are not
identical: Qualcomm's [X Elite brief][x1-brief] lists twelve Oryon cores, 42 MB
total cache and 135 GB/s memory bandwidth; its [X2 Elite brief][x2-brief] lists
third-generation Oryon and, for X2E-94-100, twelve prime plus six performance
cores, 53 MB cache and 228 GB/s bandwidth. Our inference is that removing shared
work can justify common deployment, while cache, scheduling and memory-cost
tradeoffs can have different performance outcomes.

For each shared candidate:

- Retain final-source correctness, numerical/error, full-state, adapter,
  cancellation, strict Release/linking and resource gates. Review the common
  ARM64 code path and generated code; optional SDOT/I8MM execution must retain
  runtime CPU/OS detection and tested baseline-NEON/portable fallback. X2-only
  features or changed X1 paths need their own applicable correctness controls.
- Require a paired baseline/candidate CPU-only complete-request comparison on
  the selected family, including preparation, dispatch, fallback and cleanup
  costs. Bind the actual backend, source/build/artifacts, workload, workers and
  matched power conditions; an unobserved CPU/NPU mixture or a microbenchmark
  cannot supply this gate. Retain existing benefit thresholds, repetition and
  uncertainty requirements, CI ownership and aggregate budgets.
- Record why the work reduction applies to the reviewed X1 path without relying
  on X2-specific cache capacity, core topology, instruction availability or
  additional memory admission; review plausible X1 regressions without claiming
  that this proves X1 benefit. Eliminating duplicate checks, allocations,
  helper calls or worker submissions can qualify under this review; source
  counts alone do not replace the selected-device benefit gate.
- Keep cost-tradeoff choices explicit or retain the existing control on an
  unmeasured family when applicability remains uncertain. LUT versus SDOT,
  tile/cache sizes, worker counts/affinity and added reads to reduce register
  pressure need matching-family benefit before changing that family's tuned
  default. They may still ship as opt-in candidates without blocking shared
  improvements that pass the review. If a family-sensitive candidate has no
  selector, defer/exclude it from shared promotion or implement reversible
  selection before claiming the previous control remains available.

Receipts keep their actual device identities and results. X1-specific speed,
energy, sustained behavior and tuning claims require X1 measurements; an X1
regression reopens the affected shared choice and requires verified release
rollback or an available explicit control, without assuming a runtime selector.
Accelerator selection, precision/quality changes, memory-admission increases
and complete runtime/device-profile qualification retain their separate gates.
Do not mix X1 and X2 records inside an existing single-family qualification
packet or transfer historical passes across changed source identities.

## Current CPU comparison readiness

The [2026-11-12 source preflight](evidence/cpu-comparison-readiness-20261112/summary.json)
binds Runner `212bd98e` and Prism `8d2b306`, independently of the historical
attempts below. On that exact Prism source, native ARM64 and x64 strict Release
Clippy covered the runtime, NEON helper and server with all targets; the
feature-disabled runtime analysis and native runtime/server binary linking also
passed. Three exact controls passed for request-local BPE reuse, prepared finite
proof precedence/storage and aligned six-head score reductions. The fourth
fully qualified selector matched zero tests and exited zero; it supplies no GDN
control verdict. The harness stopped without retry and skipped the remaining
two controls. No model, performance comparison, CI operation or promotion ran.

The aggregate and command endpoints observed battery operation with saver off;
charge fell from 78% to 75%. These snapshots establish neither uninterrupted
conditions nor performance benefit. This separate 300-second allowance is
closed; remaining time cannot be reused to retry the missing control or begin
comparisons. Final hosted coverage, remaining exact controls, codegen/stack
review and complete-model state/logit/adapter/cancellation/resource acceptance
remain required. The record qualifies no release bundle or runtime pin.

The native source-bound collector, hosted preparation/evaluator and
[Runner comparison lane](cpu-bundle-comparison.md) are published. Hosted
[preparation for run 43236051696](https://github.com/symbiogenesis/SwiftBonsai2Runner/actions/runs/43236051696)
passed; its queued comparison was canceled before any model started. No CPU
performance result exists; the selected lane remains available for a future
owner-operated workload. The [X2 thread screen](#x2-cpu-thread-screen) compares the publisher's
`4/4` and `6/6` thread budgets; the existing one-token source timing capture is
attribution only. Most newer CPU changes directly replace code without per-change
selectors. Exact LUT selection is a library executor, and tiled/triangular GDN
prefill needs its explicit graph consumer; ordinary private-service requests
do not exercise all those paths. The selected next scope is bundle-first:
fixed `0.1.23` control versus published registry Prism `0.1.25`, with FP16 KV, standard adapter 1.3
and four CPU workers. A bundle result cannot establish each change's individual
benefit. Keep exact LUT, tiled/triangular prefill and latest-prefix variants
separate. Match actual CPU-only placement, artifacts, work/output equivalence,
per-arm power and full-state/recovery controls under the promotion policy above.

The new tooling's focused verification passed strict collector analysis and
seven controls against each API, 26 offline evaluator controls, workflow startup
and its twelve regressions, upstream action-lock verification, PowerShell parsing
and selected Markdown/whitespace checks. No model or performance comparison ran.
The declared 300-second tooling allowance was not met through later temporary
cleanup: that cleanup record reached 899.495 seconds from the original start.
Keep the passing command observations separate from this aggregate limit failure;
neither this check nor the earlier preflight qualifies a promotion or new README
performance figure. The original clocks are closed without a retry.

## New local prompt and attention consumers

The November 13 local Prism candidate (source integration `b146491`) adds these
consumers independently of the published 0.1.25 bundle and the fixed 0.1.23
comparison control:

- Quantized KV attention shares the existing private row pool across independent
  query heads, preserving each head's arithmetic and complete-state transaction.
  The current release's serial compressed-attention path remains the control.
- `SwiftSession::load_profile_with_prefill` and the private server expose explicit
  8/16/32-token CPU prompt tiles with the selected FP16, K4/V4 or K4/V3 cache.
  Trusted launch arguments must pair tile size and scratch and provide
  context/process-memory admission.
  The entire additional reserve is charged before artifact I/O. Startup readiness
  binds the tile, reserve and graph identity; this profile skips implicit Hexagon
  installation and can retain explicit latest-prefix reuse. Quantized attention
  keeps its existing causal codec; triangular GDN remains a separate profile.
- An explicitly selected sampled suffix profile connects the tiled verifier to
  the actual Swift sampler. Matching target draws accept deterministic proposals;
  rejection restores complete state and replays the accepted prefix. A separate
  logits reserve and additional full-prefix checkpoint reserve are charged in
  addition to the tile and ordinary request rollback. No second model is loaded.

These are local implementation candidates, not a runtime repin, new default or
measured speedup. Require final-source focused/native/fallback checks, complete
model logits/state/adapter and cancellation recovery, resource accounting and
matched selected-device whole-request benefit. The independent batched target
verifier and sampled suffix consumer retain their
[algorithm contract](inference-algorithms.md#hybrid-cache-reuse-and-speculative-state).
Runner's current bundle and launch policy do not select them.

## X1-oriented source candidates

Three further CPU changes are implemented in local Prism source. They target
repeated preparation and cache pressure without requiring X2 Hexagon execution:

- PQ2 prepared activations keep their byte planes separate from optional widened
  baseline-NEON storage. The existing runtime-detected SDOT route uses compact
  byte planes without reserving unused widened lanes; baseline NEON retains
  prepared widening. Decode and tiled prefill share this representation, with
  original Q8_K blocks, packed weights and floating accumulation order preserved.
  On a 64-bit native SDOT build, each 128-column payload is 136 rather than 392
  bytes, while each scratch owner adds one 24-byte vector header. These logical
  storage counts do not measure process memory or change admission.
- Pinned width-four short convolution prepares immutable coefficients in place
  at graph construction. Four-channel groups then load tap vectors directly
  instead of transposing the same coefficients during every step. Decode and
  tiled prefill retain original FMA, SiLU and history order; other arithmetic,
  widths and raw-slice helper contracts retain their existing representation.
  Preparation retains the original coefficient count without a second copy.
- Uncached BPE pieces of at most sixteen bytes merge in bounded stack storage.
  Cached adjacent rules update only around a completed merge, preserving
  minimum-rank/leftmost selection, byte corruption errors and token limits.
  The request-local piece cache and longer-piece heap path retain their bounds.

These are source work/storage reductions, not measured X1 or X2 speedups.
The 0.1.25 development bundle includes these source changes. Final-source hosted
checks, feature-disabled fallback, ARM64 codegen/stack review, complete-model
state/logit/adapter and cancellation parity, tokenizer/application acceptance,
and matched complete-request benefit remain required for qualified promotion
under [cross-generation CPU promotion](#cross-generation-cpu-promotion).
X2 CPU evidence can support shared deployment; X1 speed claims remain unmeasured. No
backend profile, runtime pin or memory-admission limit changes here.

### Next CPU candidates

The next three local Prism changes target repeated CPU dispatch and attention
work, independently of Hexagon availability:

- Prepared PQ2 activations retain an opaque, safely detected SDOT capability.
  Eligible one-, four- and eight-row consumption kernels reuse that capability
  instead of probing CPU features again for every packed block. Original
  activation bytes, all four PQ2 symbols and ordered floating scales remain
  unchanged. Baseline NEON and portable fallback remain separate paths. Native
  scratch adds one capability field (eight padded header bytes on ARM64), with
  no per-block or weight storage increase. Multi-token prefill wrappers retain
  their existing dispatch.
- Six-head decode and paired-token attention scores retain the native reduction
  tree in NEON registers for aligned key widths. The scalar finish remains for
  tails. This removes source-level accumulator scalarization for Swift's
  256-element keys without changing key precision, temporal FMA lanes or the
  final addition order. Release code generation must establish actual spills
  and instruction savings before a performance claim.
- Eligible parallel full-attention preparation combines query normalization
  and RoPE with key normalization/RoPE and value rounding in one worker pass.
  When KV heads divide evenly across workers, matching query/KV partitions
  remove one worker dispatch and transient error-slot allocation per
  full-attention layer (sixteen of each per Swift token). Other geometries keep
  the existing path. A cold error collector defers key/value failures until all
  queries succeed and retains the earliest failing original KV chunk. Only
  provisional scratch may change before a failure; committed state stays
  transactional.

These candidates need final-source hosted checks, feature-disabled and portable
fallback, ARM64 code-generation/resource review, complete-model state/logit/
adapter and cancellation parity, and matched complete-request measurements on
one selected family under [cross-generation CPU promotion](#cross-generation-cpu-promotion).
Development on X2 supplies no X1 speedup verdict. Runner's 0.1.25 development
repin supplies no performance qualification and retains conservative admission.
The [0.1.25 registry delivery](evidence/prism-0125-publish-20261112/summary.json)
published `bonsai-neon-dot 0.1.2` and advanced the runtime's exact dependency for
the new capability API. Actual-model/device and benefit qualification remain pending.

### Further X1 CPU candidates

Three additional local source candidates remove repeated CPU preparation and
scheduling work, including paths used with the standard adapter:

- PQ2 tiled prefill reuses the safely detected SDOT/I8MM capability retained by
  prepared activations. Four-/eight-row, four-/eight-token integer tiles and
  their token tails avoid repeated optional-ISA selection inside packed-block
  loops. Integer kernels, all four PQ2 symbols, floating scale order and the
  baseline NEON fallback remain unchanged. The opaque helper API is published
  in `bonsai-neon-dot 0.1.2` and consumed by registry runtime 0.1.25.
- Single-, paired- and triple-buffer elementwise worker dispatches keep their
  at-most-eighteen error slots on the stack. Each dispatch removes one transient
  heap allocation while retaining the earliest failing chunk after all tasks
  join. Slot storage stays private to the invocation, including nested calls;
  the existing worker reservation and conservative admission remain in force.
- Adapter-enabled CPU FFN preparation retains original-basis SwiGLU values in
  the existing up buffer while preparing the down projection's signed Hadamard
  basis in the existing transformed buffer. Combining those block operations
  removes a separate worker pass. The down adapter continues to consume its
  original-basis input; adapters and accelerator eligibility are unchanged.

These are source work reductions, with no measured X1 or X2 benefit. Require
final-source exact arithmetic, failure and adapter controls, feature-disabled
fallback, strict Release analysis and linking, ARM64 codegen/stack review,
complete-model state/logit/cancellation parity and matched selected-family CPU
request benefit under [cross-generation CPU promotion](#cross-generation-cpu-promotion).
The 0.1.25 development runtime includes this source; qualified promotion remains pending.

### Grouped dispatch, hardware half widening and exact LUT execution

Local Prism source `ae9c5a785dcf9490c9573b32da7136222aa8504a` contains three
additional X1 CPU candidates targeting repeated worker scheduling, cached-half
conversion and packed integer work:

- Compatible CPU PQ2/Q8_K sibling projections share one joined worker-pool
  submission. Query/key/value and gate/up retain their existing scoped input
  preparation, row partitions and packed 8/4/1-row kernels. Other geometries,
  quantizers and external executors keep their existing paths. The group checks
  every extent before submission and clears all sibling outputs on failure;
  failures retain sibling/chunk order after workers join. At most four matrices
  use seventy-two bounded task descriptors, without copying weight payloads or
  creating a second pool.
- Cached FP16 attention loads and four-/eight-row half-scale helpers use
  [baseline NEON hardware widening](https://doc.rust-lang.org/stable/core/arch/aarch64/fn.vcvt_f32_f16.html)
  through the isolated safe `bonsai-neon-dot` API. Finite
  arrays use `FCVTL`/`FCVTL2`; arrays containing infinity or NaN use exact integer
  decoding to preserve signs and signaling-NaN payloads. This changes conversion
  work without changing stored precision or floating reduction order. Portable
  decoding remains available; the raw-u32 rounding helper keeps its prior path.
- Explicit `Pq2LutExecutor` connects activation-pair tables to the existing
  prepared-matvec and graph projection interfaces. One 2,048-byte table per
  128 columns serves every output row through the existing row-worker policy.
  Tables are rebuilt for each invocation, including sibling invocations, and
  retained capacity is bounded by the caller's maximum width: 80 KiB at 5,120
  columns. Original Q8_K fused/separate accumulation, odd-group Q8_0 scales and
  all four PQ2 symbols remain supported. Unsupported tensors keep CPU execution.
  This experimental executor is never selected automatically; table construction
  and cache traffic may erase its integer-work savings against SDOT.

These implementations establish no measured X1 speedup. Require final-source
strict Release analysis and linking, native and portable controls, exhaustive
half-encoding parity, sibling failure/adapter/state controls and ARM64
codegen/stack/resource review. Matched selected-family CPU complete-request measurements must
include table preparation, worker dispatch and all fallback costs, with losing
or no-go outcomes retained. Apply [cross-generation CPU promotion](#cross-generation-cpu-promotion)
to shared work reductions; LUT selection remains explicit pending matching-family
benefit. Full-model logits/state/adapter and cancellation
acceptance remain independent. The new half-widening API is published in
`bonsai-neon-dot 0.1.2`, consumed by the 0.1.25 registry and binary development
deliveries. This repin does not qualify a LUT default or measured request benefit.

### Complete-row SDOT, fixed attention lanes and bounded GDN phases

Local Prism source `8504810` adds three CPU candidates for X1, without requiring
Hexagon execution:

- Detected SDOT PQ2 consumption keeps complete one-, four- and eight-row
  projections inside one target-feature call. Integer dots, half-scale widening
  and ordered floating accumulation stay inside that call rather than returning
  between every 128-column block. At width 5,120, a four-/eight-row group replaces
  forty integer-helper calls and forty half-widening calls with one complete-row call.
  Original Q8_K scales, all four PQ2 symbols, fused/separate arithmetic and
  136-byte prepared blocks remain unchanged. Baseline NEON, scalar and tiled
  multi-token kernels retain their existing paths. The new safe helper API is
  published in `bonsai-neon-dot 0.1.2`, consumed by registry runtime 0.1.25.
- Six-head and paired twelve-query attention scores consume sixteen key columns
  per loop with four fixed accumulator indices. Swift's width-256 key changes
  from thirty-two to sixteen loop iterations and query-row slices. Cached-half
  conversion sharing, native FMA lanes, scalar tails, masked padding, reductions
  and paired-token sharing remain unchanged. Fewer source loop operations do
  not establish fewer machine instructions or spills.
- Eight-row GDN decode retains its eight-row transactional staging and commits
  through two four-row arithmetic phases. Each dot phase has sixteen live
  accumulator vectors rather than thirty-two, leaving room for inputs on
  ARM64's thirty-two-vector-register architecture. Every key dot still precedes
  updates, and all eight destinations remain untouched until success. Swift's
  private staging remains 4 KiB, plus eight scalar deltas between phases; the
  small key/query inputs are read again for each four-row phase. Four-row decode
  shares the same arithmetic helpers; batched token kernels are unchanged.

These are source candidates, with no measured X1 speedup. Independent focused
controls cover exact native/portable arithmetic, forced baseline consumption,
FMA rounding, awkward extents, temporal state and late-failure publication.
Final-source strict Release analysis, native and portable execution, ARM64
codegen/spill/stack review, complete-model logits/state/adapter/cancellation
parity and matched selected-family CPU complete-request benefit remain qualification
gates under [cross-generation CPU promotion](#cross-generation-cpu-promotion).
In particular, GDN's extra input reads must be included in that comparison.
The 0.1.25 development bundle includes this source without a measured benefit
claim; conservative memory admission remains unchanged.

One fixed model-free local workload on `01fbf5d` closed after 40.412 seconds of
its 300-second allowance. Native exact controls and strict analysis stopped at
the pre-existing half-widening helper's missing explicit NEON function context;
no tests executed. Strict x64 analysis found helper documentation and existing
LUT executor lints. Source corrections are included in `8504810`, without a
retry. Rust formatting, these Markdown files and whitespace checks passed on
the captured source; final-source compiler and execution checks remain pending.
Aggregate and per-command endpoints observed AC connected, saver off and 100%
charge; they establish neither continuous stability nor performance benefit.
No model, benchmark or CI operation ran.

### Scoped PQ2 validation, tiled Q/K/V preparation and temporal GDN

Local Prism `b98574a` integrates three further CPU source candidates targeting
repeated work in projection and prompt ingestion, without requiring X1 Hexagon
access:

- A borrowed prepared-PQ2 view checks activation scales once per projection or
  compatible sibling group and shares that proof across row tiles. For a
  5,120-column, 17,408-row projection with eight-row tiles, this changes 87,040
  activation-scale finite predicates to forty. Original weight-scale, geometry
  and result checks remain required. The immutable borrow binds the proof to
  current activation bytes; public unprepared calls remain defensive. No
  persistent weight copy or new activation allocation is needed.
- Tiled full-attention preparation joins query normalization/RoPE and key/value
  normalization, RoPE and rounding in one token-partitioned worker submission.
  This extends the existing decode scheduling idea to prompt tiles and omits
  sixteen submissions per ordinary Swift tile, or fifteen when the final
  full-attention layer uses its existing KV-only narrowing. Query failures
  retain precedence over every key/value failure, and the earliest
  original key/value chunk wins after workers join. Only provisional scratch
  changes before the existing state publication boundary.
- Eight-row multi-token GDN uses four-row arithmetic phases in both source-state
  and in-place paths. Each dot phase has sixteen accumulator vectors rather
  than thirty-two; all eight key dots still precede updates. Original per-token
  arithmetic, private eight-row staging and atomic destination publication
  remain intact. The small key/query arrays are read again for the second
  four-row phase, so lower source register pressure alone establishes no gain.

These changes establish no measured X1 speedup. Require final-source native and
portable strict Release analysis, exact arithmetic and failure controls,
feature-disabled fallback and production linking; ARM64 codegen/spill/stack
review; complete-model state/logit/adapter/cancellation parity; and matched
selected-family CPU complete-request benefit under
[cross-generation CPU promotion](#cross-generation-cpu-promotion).
Include preparation, dispatch and repeated input reads in timing.
The prepared-PQ2 API is published in `bonsai-neon-dot 0.1.2` and consumed by
registry runtime 0.1.25. Runner's development repin consumes the same reviewed
production source; qualification remains pending and admission is unchanged.

The fixed model-free check on `750f7b3` closed after 15.026 seconds of its
300-second allowance. Strict native all-target Release Clippy stopped at an
obsolete GDN line-count expectation and two test assertion lints. Source
corrections remove that expectation and make the empty-score assertions
explicit, without reopening the clock. No test executed; x64, feature-disabled
analysis and production linking were skipped. Rust formatting, Markdown and
whitespace checks passed on the captured source. Aggregate and per-command
endpoints observed AC disconnected, saver off and 85% charge; uninterrupted
power stability remains unknown. All selected children exited, command logs
were removed and the external build cache was retained. No model, benchmark or
CI operation ran. Final-source compiler and execution acceptance remain pending.

### Prepared packed-KV queries and CPU preparation reuse

Further local Prism source work targets repeated CPU preparation on X1 without
requiring an accelerator or changing the selected precision:

- Packed-KV attention prepares each immutable rotated query once before scanning
  cached keys. The borrowed proof retains query/codebook validation and native
  selection outside the history loop; each record still checks its length,
  scale, canonical zero encoding and finite result. The public single-record
  scoring API remains defensive. This reduces repeated validation at longer
  contexts while preserving the existing scalar/native dot reduction.
- Internally quantized PQ2 activations can reuse their exact per-sixteen-value
  integer sums during packed preparation. Arbitrary public activation blocks
  retain defensive value-derived sums; their compatibility fields do not become
  trusted. Packed bytes, scales, accumulation and runtime ISA fallback stay the
  same, without an additional weight or activation allocation.
- Eligible pinned prompt convolution reads committed history directly into the
  existing provisional next-history destination, omitting its preceding full
  history copy. Original FMA/SiLU order and the graph's state publication boundary
  remain required; other geometries keep their existing path.

These changes are local source candidates, not measured X1 speedups or a Runner
runtime repin. Require focused exact native/portable and failure controls,
strict Release analysis of runtime and changed helpers on ARM64 and x64,
feature-disabled fallback, production linking and ARM64 codegen/resource review.
Complete-model logits/state/adapter/cancellation acceptance and matched
CPU-only complete-request benefit remain pending under
[cross-generation CPU promotion](#cross-generation-cpu-promotion).
New helper APIs need publication before a dependent registry release; an exact
source-bound runtime bundle and reviewed Runner repin are required before the
application consumes these changes. Retain the current runtime as rollback.

The fixed model-free check on Prism `9703109` closed after 16.420 seconds of its
300-second allowance. Rust formatting passed; native all-target strict Release
analysis stopped on documentation/test-style lints, including two existing
speculative-buffer assertions. The source corrections were reviewed without
retrying the workload. No regression test executed; x64 and feature-disabled
analysis, production linking and Markdown checks were skipped. Aggregate and
command endpoints observed AC disconnected, battery saver off and 82% charge;
continuous power stability remains unknown. Owned commands exited and temporary
driver/logs were removed; the external Cargo cache and one compact local summary
were retained. No model, benchmark, CI operation or performance qualification ran.
Final-source compiler and execution acceptance remain pending.

### Scan-owned packed values and joined prompt projections

Further local Prism CPU work targets repeated per-record and per-row-tile work
on the shared X1/X2 path:

- Packed attention owns one provisional value accumulator for the complete
  history scan. Codebook validation and native dispatch happen once; each
  record still checks its encoding, weight, scale and finite arithmetic. This
  removes repeated scans of the previous 256-value sum and its per-record
  transactional copy. Failure permanently prevents publication of that private
  sum. The public single-record accumulation API keeps its defensive checks
  and unchanged-output failure contract; stored precision and operation order
  are unchanged.
- Compatible CPU PQ2 prompt-tile siblings share one joined row-pool submission
  and the existing prepared activations. The graph consumes this for eligible
  projection pairs and triples, including siblings untouched by the standard
  adapter. A targeted adapter retains sequential execution so that its failures
  keep their original precedence. Unsupported geometry, transforms and external
  executors retain their existing paths. This extends the explicit tiled-prefill
  candidate; ordinary single-token ingestion does not acquire prompt tiles.
- Prepared native PQ2 row consumption uses the helper's existing extent check
  on the successful path. The runtime classifies malformed extents after a
  rejection, retaining shape-error precedence without checking the same row
  geometry twice for every one-, four- or eight-row tile. Baseline NEON and
  portable consumers retain their existing checks.

These are source work reductions, not measured X1 speedups. Native/portable
exact arithmetic, corrupt-record and late-failure controls, strict Release
analysis and linking, feature-disabled fallback and ARM64 codegen/resource
review remain required. Complete-model logits/state/adapter/cancellation and
matched CPU-only complete-request benefit retain the
[cross-generation promotion gates](#cross-generation-cpu-promotion). New helper
APIs require publication before a dependent runtime release, followed by a
genuine source-bound bundle and reviewed Runner repin. Runner's active 0.1.25
runtime does not consume this local source candidate.

## Execution and dispatch

The default `experimental-neon` feature uses [`fearless_simd` 1.1.0][fearless]'s safe core API under
`forbid(unsafe_code)`. `MatvecBackend::Auto` selects once per matrix operation; Qwen35 and the token
driver inherit it. `Qwen35::with_backend` and `--backend auto|scalar|neon` select explicit policies.

`backend_requested` reports the requested policy. `backend_statistics` retains cumulative matrix
attempts/successes, scalar/NEON dispatches, preparation failures and fallback reasons, including
provisional work. `MatvecScratch::with_backend` forces a kernel policy; `selected_backend()` reports
the last operation and `statistics()` exposes aggregate counts without payloads. Counters saturate
at `u64::MAX`; reset and checkpoint restoration preserve diagnostics. These
packed-kernel diagnostics report floating projections as scalar; independent
baseline SIMD helpers can still execute inside those floating/state operations.
Ineligible packed operations retain their scalar path.

The pinned state SIMD implementation also uses safe baseline NEON for pinned GDN dot/update
arithmetic and attention value
tiles on the same supported Windows ARM64 build. These graph helpers retain their
scalar implementations when the feature or target is unavailable. `MatvecBackend`
continues to govern packed projections. Direct floating/BF16 helpers and pinned
graph-state helpers have separate compile-time baseline-NEON gates, preserving
their original reduction contracts. Their use is not identified by the packed
dispatch counters. These graph helpers add no optional ISA, precision mode,
graph-state layout or selection profile.

GDN keeps four independent native FMA registers, the same reduction tree and unfused
scalar tails. Its scaled update checks four results together; a failing group is
replayed in scalar lane order before publication, preserving the original finite-prefix
mutation and untouched failing lane. Attention groups four adjacent channels in each
vector while retaining all sixteen temporal accumulators, FP16 decoding, rounded
probabilities, masked-zero FMAs and the original reduction/publication order. The
accumulator payload remains 2 KiB; non-multiple-of-four tile widths stay scalar.
No model or KV payload is expanded. Compiler code generation, actual stack footprint,
complete-model parity and matched request/prompt benefit remain qualification gates;
explicit vectors alone do not establish fewer instructions or a speedup.

Baseline eligibility requires native `aarch64-pc-windows-msvc`, the compiled feature and baseline
NEON. Complete PQ2/PTQ1 rows use packed kernels; unsupported formats/targets and disabled features
use scalar. Scalar runs natively on ARM64. Optional `dotprod`/`i8mm` primitives
use the isolated `bonsai-neon-dot` feature checks; they do not require a global
target override or persistent expanded weight copy. Kernel `Auto` does not
select Snapdragon generations or
CPU/GPU/NPU profiles; that boundary belongs to the [hardware contract](hardware-capabilities.md#automatic-profile-selection).

Runtime, loader, graph, codecs and broker retain `forbid(unsafe_code)`. Safe dependency APIs are the
first boundary. Intrinsics requiring local unsafe belong in a reviewed isolated crate with safe
slice APIs and explicit bounds, alignment, aliasing, initialization, lifetime and CPU/OS feature
invariants. Public vector types and a relaxed runtime prohibition are outside that contract; Rust's
[target-feature rules][target-feature] apply.

The checked facade validates kinds, dimensions, byte extents, input/output lengths and finite
activations. Immutable payloads are checked at load or per checked block; public arbitrary `Tensor`
calls still reject malformed payloads. Fallible scratch growth stays outside hot loops, errors leave
empty output, and immutable weights/LoRA B remain shareable. Unaligned loads, scalar tails or local
copies are bounded; overreads are invalid.

Optional dotprod and i8mm require verified Windows CPU/OS detection, independently qualified
layouts/shapes/arithmetic and useful device benefit. Compile-time enabled features can bypass
runtime probing; model names, Linux reports, compilation and fault recovery do not establish
eligibility. The relevant interfaces are [`is_aarch64_feature_detected!`][detection],
[`IsProcessorFeaturePresent`][windows-features] and the installed SDK. Distributable flags remain at
the reviewed baseline; global `target-cpu=native`, `+dotprod` or `+i8mm` are outside the contract.
Detection/policy selection stays outside row/block loops. Overrides can disable features or request
supported paths.

## Packed arithmetic

The [kernel module][kernels], [graph][graph], [ternary codec][ternary], [Swift
metadata](../config/swift.metadata.json) and [symbol census](../config/swift.pq2-census.json) define
format identities. Q4_0/Q8_0/F16 helpers do not establish loader support for every artifact
inventory.

| Operation | Required arithmetic |
| --- | --- |
| PQ2 rows divisible by 256 | Q8_K activation groups; each 128-weight block decodes two-bit codes to -1, 0, 1, 2; preserve integer dots and ordered floating scales/adds |
| Odd-group PQ2 | Four independent Q8_0 scales per 128 weights, then ordered sums |
| PTQ1 | Direct canonical base-three decode, four Q8_0 groups per 128 weights (or direct `dot_ptq1_q8k` over `Q8_K` blocks), then block scale; no intermediate PQ2 block |
| TQ1_0 / TQ2_0 / TL1 LUT | Compile-time `TQ1_BYTE_LUT`/`TQ2_BYTE_LUT` tables, 128-bit NEON `dot_tq2_block`, and 16-entry INT16 `Tl1BlockLut` nibble tables |
| Hadamard/elementwise work | Preserve stage boundaries, signs, axes/basis, normalization and multiply/add order |
| Activation quantizers | Preserve maxima/ties, clipping, scales, rounding and rejection |
| Floating reductions/RMS/attention/recurrent work | Rounded f32 products and specified f64 accumulation; reassociation has a separate numerical contract |
| Prefill tiles | Bounded causal/state scheduling; faster matvec alone does not batch graph execution |
| Isolated `bonsai-neon-dot` (`SDOT`/`SMMLA`) & `u8x16` `PTQ1_0` | Safe boundary crate (`tools/neon-dot`) wrapping ARMv8.2-A `vdotq_s32` (`dotprod`) and `vmmlaq_s32` (`i8mm` `2x2` row-by-token tiles) behind runtime feature checks, plus non-widening `u8x16` base-3 division-by-multiplication `PTQ1_0` trit unpacking (`dot_ptq1_q80_block`, `dot_ptq1_q8k`) |
| Fused `RMSNorm + Sign*FWHT` & `SwiGLU + Sign*FWHT` | Single-pass 16-register NEON block helpers (`rms_norm_signed_fwht_in_place`, `swiglu_signed_fwht_into`) eliminating intermediate scratch round-trips; last-layer state-only prefill (`Evaluation::full_attention_kv_only`, `Evaluation::recurrent_attention_state_only`) skips dead output-proj/FFN on `layer 63`; `parallel_tile` uses a spin-then-park completion barrier |

The earlier **2026-11-12** local-source audit found `Tl1BlockLut` construction
only in tests, besides its public reexport. The explicit `Pq2LutExecutor`
candidate above now connects it to prepared matvec and graph execution; it is
not an automatically selected kernel. No cross-token Vec-LUT consumer is
implemented. Existing packed token tiles reuse decoded weight blocks; they do
not implement activation-LUT execution. The
[lookup contract](inference-algorithms.md#packed-lookup-kernels) distinguishes
these mechanisms and the research register retains the original sources.

An integrated exact LUT path must share bounded activation preparation across
eligible output rows and token tiles while preserving Q8_K/Q8_0 quantizers,
128-weight scales, block/lane accumulation order and all intermediate bounds.
Generic PQ2 must retain its +2 symbol or take the checked generic fallback;
Swift's ternary census does not narrow the public tensor contract. Include
table construction, cache traffic, layouts/repacking, overlapping retained
storage and dispatch in comparisons with qualified SDOT/I8MM and baseline
NEON. Smaller integer work or a faster isolated lookup does not establish a
complete-request improvement. PLAN retains the bounded candidate comparisons
and release/qualification gates; this review enables no new kernel default.

PQ2 code 3 means **+2**, despite the exporter's narrower ternary-only contract. PTQ1 digits are
interleaved: bytes 0–15 hold five digits at stride 16, bytes 16–23 hold five at stride 8, and bytes
24–25 hold four at stride 2. Q4_0 stores 16 low nibbles before 16 high nibbles. Canonicality and
byte order remain exact.

The [integer-bound derivation](research-register.md#t3-exact-low-bit-dots-need-bounds-at-every-intermediate)
covers arbitrary i8 activations, including -128: a 128-element PQ2 dot fits i16
(`[-32768, 32512]`), but its uncorrected unsigned-code dot does not (`[-49152, 48768]`).
At 32 elements the ternary/PQ2 absolute bounds are 4,096/8,192. Bound every intermediate and
correction before narrowing; Rust overflow flags do not check wrapping SIMD. Apply each block's
scale before accumulating later blocks. Malformed scales/noncanonical PTQ1 bytes reject even at
zero scale.

The local Prism PQ2 decode candidate accumulates signed product pairs in two
independent i16 vectors, then widens before the horizontal i32 reduction. Products
are in `[-256, 254]`, pairs in `[-512, 508]`, each accumulator lane in
`[-2048, 2032]`, and their combined lane in `[-4096, 4064]`. These bounds cover
arbitrary PQ2 fourth symbols and i8 activations, independent of the Swift census.
No floating operation, quantizer, PTQ1 native lane reduction or dispatch eligibility
changes. Fewer source-level widening operations require Release code-generation
inspection and matched request measurements before a performance claim.

The explicit tiled-prefill candidate selects baseline NEON integer reductions
over one decoded PQ2/PTQ1 block shared across the tile's tokens. It preserves odd
PQ2 groups, PTQ1's four native integer lanes, ordered scales/FMAs, explicit scalar
selection and unsupported-target fallback. The graph passes its requested backend
into the tile; decoded vectors remain bounded stack storage, and preparation does
not allocate unused PQ2 digit-plane activations. The follow-up source candidate
shares the graph's existing private row-worker pool across disjoint output slices,
without transposition or a second pool. The original non-Sync cancellation callback
stays on the caller; workers observe cancellation between rows and join before
failed output is cleared. Its extra 16 KiB reservation is charged separately from
existing worker stacks. Compare complete prompt cost with the ordinary
NEON/four-worker control before choosing this path.
Model and performance qualification remain pending; source controls do not
establish measured benefit.

The CPU implementation also covers direct BF16 weight consumption for Swift's recurrent alpha/beta
projections. Tensor geometry is checked before row access, each weight remains
finite-checked, and BF16 activations, rounded F32 products and the original F64
sum/final F32 rounding are unchanged. No decoded row is materialized. Across
48 recurrent layers' two 5,120-by-48 matrices this avoids a nominal 90 MiB of
F32 scratch writes and matching rereads per token; compiler behavior and actual
traffic/latency remain unmeasured. In the historical 0.1.23 eligible NEON build,
BF16 uses direct eight/four-row helpers through the existing row-worker pool;
the unsupported-build path retains direct scalar row consumption. This does
not require a decoded F32 row or change the conservative memory admission.

Q8_K chooses the first signed value with strictly greater absolute magnitude, uses `-127 / maximum`,
generic ties-to-even rounding and an f32 scale. Pinned Swift instead retains
the native fused magic-add Q8_K rounding; independent half-boundary captures
distinguish the two in the [packed control](../tools/hexagon/packed-projection.md).
Q8_0 uses an absolute maximum, ties-away rounding and an
FP16-rounded stored scale, while quantizing with the original inverse scale. Opposite-sign ties,
signed zero, finite extremes and nonfinite rejection are part of the contract. Equal decoded
PQ2/PTQ1 weights can produce different matvec outputs; see the [packing
assessment](swift-memory.md).

## Signed constant floor division

[Ben Joffe's algorithm](https://www.benjoffe.com/fast-floordiv), published
2026-10-27 and reviewed here on **2026-10-30**, computes signed integer division
rounded toward negative infinity using a multiply and arithmetic shift. It is
a candidate for a measured signed constant-divisor hotspot in Prism. Ordinary
Rust signed `/` truncates toward zero; replacing it requires an existing floor
contract or proof that the caller's admitted inputs give the same result.

The [reference implementation](https://github.com/benjoffe/fastdiv/blob/main/algorithms/benjoffe.hpp)
searches multiplier/shift pairs for both rounding directions. When both work,
it can select the multiplier using the dividend's sign. Some divisors require
a bounded input adjustment instead; the sign-selected formula is not universal.
For a valid sign-selected pair `(M, S)`, the quotient is
`(i128(x) * (M + (x >> 63))) >> S`, with an arithmetic right shift.
Positive powers of two already reduce to an arithmetic shift. Resolve parameters
at compile time for fixed constants; a runtime-invariant variant must prepare them
outside the hot loop. A Rust port must use widened integer products, preserve
full-range behavior and explicitly reject zero and the unrepresentable `i64::MIN / -1`.
Negative-divisor floor division differs from `div_euclid`; do not use the latter
as its general reference. The reference's `main` link is a discovery pointer;
pin a reviewed revision and retain its license notices before copying source.

The **2026-10-30 source audit found no eligible production caller in Runner**:

| Existing arithmetic | Applicability |
| --- | --- |
| Speech graph `(n_kv_pad + 255) / 256` in `src/speech-tts/build_overlay.rs` | Nonnegative ceiling division by a power of two |
| Audio elapsed time `/ 48` in `src/chat-ui/src/speech/pipeline.rs` | Unsigned byte-count conversion |
| Buffer, sample and packed-code indexing | Unsigned extents or power-of-two divisors |
| Quantization scales and timing ratios | Floating-point division |

The audit also covered the clean pinned Prism revision
`fce300b94234c2dd70756dd4bbc33ea703848cb6`. Its
[convolution extent calculation](https://github.com/symbiogenesis/prism-inference/blob/fce300b94234c2dd70756dd4bbc33ea703848cb6/src/backend/call.rs#L73)
uses an `i128` numerator and a runtime stride. Its
[backward-image divisions](https://github.com/symbiogenesis/prism-inference/blob/fce300b94234c2dd70756dd4bbc33ea703848cb6/src/backend/backward_image.rs#L90)
use runtime strides after exact-divisibility guards. Neither is a fixed-`i64`
replacement site. Runtime-invariant specialization would require its own setup
and reuse-cost evidence. No runtime code or dependency pin changes follow from
this audit, and no throughput, latency or complete-request gain has been measured.

The following are conditional opportunities in existing planned work, inferred
from the algorithm's input requirements; neither is an identified current hotspot:

| Planned work | Where to check |
| --- | --- |
| [Tiled projection-prefill](../PLAN.md#cpu-kernels), attention tiles and [chunkwise GDN](inference-algorithms.md#chunkwise-gated-deltanet-prefill) | Signed padded or relative coordinates mapped to tile/chunk indices before conversion to `usize`, if floor rounding and a fixed non-power-of-two divisor are required. Initial 8/16/32 tiles already permit shifts; retain those choices. |
| [Deferred Prism numerical kernels](../PLAN.md#deferred-prism-migration), when reopened for a Swift dependency | Specialized convolution/pooling coordinate mapping with an invariant stride reused across many elements. Require an admitted `i64` numerator range and unchanged rounding/rejection behavior; charge parameter setup and dispatch as well as division. |

Keep this check within the chosen kernel's design and measured cost. Unsigned
PTQ1 base-three decoding, nonnegative cache/page indices and floating quantizers
do not become signed floor-division candidates. Any future integer requantizer
would first need its own rounding contract; this algorithm does not preserve
ties-to-even or ties-away rounding by itself.

For a future eligible caller, compare exact results with an independent widened
quotient/remainder reference: both signs, zero dividend, signed extrema, exact
multiples and their neighbors, large divisors, forced round-up/down cases and
power-of-two cases. Check optimized ARM64 code generation under the pinned Rust
compiler; the article's C++ results do not establish Rust performance. Measure
throughput, dependent latency and complete-request cost in owning CI under the
[five-minute device budget](validation.md), retaining losing results. Implement
only a justified consumer upstream; [PLAN.md](../PLAN.md#cpu-kernels) owns that gate.

## Numerical and resource acceptance

Optimized-Rust/scalar and Rust/Prism comparisons have separate purposes; an existing
scalar/reference tolerance does not authorize additional SIMD drift.

| Scope | Acceptance |
| --- | --- |
| Symbols, scales, activations, integer sums | Exact bytes/bits/integers, including ties and signs |
| Order-preserving packed dots, elementwise work, butterflies | Bitwise finite f32 parity, signed zero and matching failures; preserve grouping and avoid unintended FMA |
| Reassociated reductions/FMA | Predeclared absolute/relative bounds, error distribution and logits/state/token checks |
| Full graph | Fixed IDs/positions/context/cache/adapter/decoding; layer/logit, KV/GDN, greedy tie, long-prefix, restoration and cancellation checks |

Full attention retains FP16 Q/K/V and normalized-probability conversions. GDN retains
decay/delta/state-update/output order. Fusion cannot move reads across state mutation. Scalar
transcendental functions are the control; vector versions have explicit NaN/infinity/subnormal/mask
and accuracy contracts. Negative-infinity masking and all-masked-row rejection remain required.
[Algebraic float operators][algebraic] and fast-math have distinct numerical contracts.

Exact fixtures include packed codes/offsets, lane extrema, immutable C++ bytes, unaligned blocks,
scale/quantization edges, awkward rows, malformed extents, format changes and error/rollback
recovery. Incomplete packed blocks are invalid. Negative optional-capability cases are simulated;
real optional ISA execution requires eligible hardware. See [kernel fixtures][kernel-tests] and
[SIMD fixtures][simd-tests].

A performance receipt binds artifacts, workload, context, threads, power/cache state, adapter,
tolerances, primary metric, useful-benefit threshold and allowed regressions. It distinguishes
row/tile dispatch, hot-cache/streaming weights, quantization/unpack/dot/transforms/scheduling, cold
load/repack, prefill/first-token/ decode/task cost, scratch/residency/allocations and overlapping
source/destination storage. Compiler/LLVM/options, actual backend/fallback and native CPU/OS
identity are part of its scope. A microkernel result does not qualify a whole-model replacement or
another X1/X2 device. Selected-family whole-request CPU evidence may support shared source
promotion under [the applicability review](#cross-generation-cpu-promotion); measured performance
claims remain device-specific. Standard LoRA TPS/quality remains independent.

Local builds, disassembly and focused correctness checks are permitted. Full suites and every
benchmark run only in CI under the [validation policy](validation.md). Hosted ARM64 supplies
correctness infrastructure; target-device CI supplies performance/thermal/energy evidence. Four
threads and total core count are control settings, not universal optima; phase placement and
mixed-device contention are part of a profile's declared scope.

## CPU kernel research reviewed in October 2026

The [research catalog](research-register.md#packed-projections-cpu-kernels-and-compression) owns
sources, versions and transfer limits through **2026-10-29**. Its CPU applicability rules are:

| Candidate | Constraint |
| --- | --- |
| Layout/LUT reuse (L1–L4, L10) | Derive NEON consumption order from lanes, quantizers and scale groups. Include unpack/preparation/dot/scale work, repacking and retained copies. Larger tables/accumulators can spill registers or evict useful cache; choose bounded tiles using complete-workload CI. |
| Entropy coding (L12) | Charge indexes, decode dependencies, scratch and consumed bytes. Independent streams may feed kernels, but 8-bit/communication results do not establish packed-matvec benefit. |
| Representations (L11, L13–L17) | Nonuniform codes, compensation, fitted rotations, lattices and trellises need new artifacts and separate qualification. |
| Hardware (L18–L19) | Simulated ISA/datapath changes are outside existing Snapdragon software work. |

Retain packed storage without persistent expansion, distinguish decode from genuine multi-token
prefill and apply the intermediate/correction bounds above to every borrowed layout.

## X2 CPU thread screen

The **2026-10-29 model-free inventory** identifies the development target, not a qualified speedup
or GPU/NPU profile. Native ISA semantics live in [capability contracts](hardware-capabilities.md).

| Observation | Retained value |
| --- | --- |
| CIM/independent hardware-registry name | `Snapdragon(R) X2 Elite Extreme - X2E94100 - Qualcomm Oryon(TM) CPU` |
| Native toolchain/target/kernel | Pinned Rust; `aarch64-pc-windows-msvc`; Windows `10.0.26300` (`10.0.26300.0` probe display) |
| CPU topology | 18 physical/logical cores, group 0, efficiency/scheduling classes `[0, 1]`; class numbers do not name core types |
| Native Windows ISA | Positive NEON, dot-product and **NEON I8MM** flags; dispatched kernels still need codegen/numerical/device qualification |
| GPU inventory | Adreno X2-90, driver `32.0.149.0` |
| Power policy | Effective `balanced`; scheme `381b4222-f694-41f0-9685-ff5bb260df2e` |
| Planned screen | `d6-p6`, `d4-p12`, `d8-p18`; 24 samples; `ModelProcessStarted: false` |

Identity checks accept unchanged observations and reject injected AC/mode/kernel changes and legacy
records lacking host identity. The speech-host query agrees on CPU/kernel. Reports, plans and
comparison receipts are retained externally in `Bonsai-PerfNext3-20261029/x2-evidence`; they contain
no model timing.

The manual [X2 CPU workflow](../.github/workflows/x2-cpu-evaluation.yml) prepares exact maintenance
tools on hosted ARM64 CI. Its default `run_device: false` performs no owner-device execution.
The optional device job requires settled `main`, a native ARM64 owner runner with the single-run
label `x2-cpu-<run_id>` and the shared model-capture concurrency group. Register that runner as
ephemeral only for the reviewed run; a persistent generic runner could accept old queued jobs.
Performance execution remains [CI-only](validation.md).

### Controls and cost

The Rust maintenance planner selects only `6/6` decode/prefill threads against the unchanged
`4/4` control. These are thread counts without affinity pinning; Windows chooses placement.
One fixed short synthetic creative-writing prompt runs in two reversed pairs, at most four fresh
processes with a 16-token cap and a 45-second per-process deadline. The former 24-process,
two-prompt matrix is retired, not partitioned across repeated short jobs. Other budgets and long
prompts remain pending work in PLAN. OS file cache is not flushed.

All setup, verification, preflight, model loading and comparison share one at-most-240-second
process budget within the five-minute owner job. Hosted tool compilation is outside that device job;
the device verifies this run's exact revision, digest and ARM64 tools instead of compiling. The
parent charges checkout and tool intake to its remaining job clock, caps the child at the lesser of
240 seconds and the remaining time before 270 job seconds, and bounds teardown at ten seconds,
within the original deadline. Rust reserves twenty seconds for cleanup/reporting and refuses model
trials when the remaining budget cannot fit the four per-sample deadlines plus teardown room.

The completion batch's opt-in `totalTimeoutSeconds` (1..240) charges artifact/runtime hashing and
all samples to one monotonic clock. Hash verification checks that clock at the existing one-second
caller checkpoints. Before each launch, native supervision requires room for the full per-sample
deadline, the ten-second owned teardown and one watchdog interval. Exhaustion retains failure and
partial sample receipts; it neither resumes the matrix nor produces a completed comparison.
The outer process watchdog also bounds synchronous setup and reporting. If startup, verification
or even one sample cannot fit, leave the gate pending; never weaken policy or extend the job.

The screen fixes the pinned original PQ2 model, runtime, adapter at scale 1.3, publisher sampler,
seed 42, 8,192-token context, FP16 KV, 128/64 prompt batches, CPU-only placement, no repacking and
zero polling. The legacy completion executable uses its own template defaults; these differ from the
chat server's explicit thinking policy. This is a CPU thread screen, not chat qualification.

Artifacts must already exist; the batch supervisor verifies and holds exact model, adapter and
runtime files. Every sample retains 15.5 GiB admission, an 11.5 GiB Job commit ceiling, 4 GiB
physical reserve, one-process/caller ownership and bounded logs. The screen downloads no models
and promotes no runtime/default.

Before verifying model bytes or starting thread trials, execution runs the exact verified completion
executable with only `--version` under the ordinary headless worker policy. This model-free
preflight retains a separate `worker-preflight/` receipt and bounded logs, with a 512 MiB Job and
ten-second deadline. Failure stops the screen and leaves `ModelTrialsRequested: false` in its
report. The shared supervisor's `ModelProcessStarted` field means an executable was started; the
preflight's explicit `ModelLoaded: false` distinguishes it from model trials. `observed-plan` remains
nonexecuting. The relaxed inspection launch cannot satisfy this worker check.

### Inspecting the plan

From native ARM64 PowerShell 7.6+, use an existing checked maintenance executable and an absolute
data directory outside Git:

```powershell
$processors = @(Get-CimInstance Win32_Processor -Property Name, NumberOfLogicalProcessors -OperationTimeoutSec 10 |
    ForEach-Object { @{ name = $_.Name; logicalProcessors = [int]$_.NumberOfLogicalProcessors } })
$request = Join-Path $env:TEMP 'x2-plan-request.json'
@{ operation = 'observed-plan'; dataDirectory = $data; cimProcessors = $processors } |
    ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $request -Encoding utf8NoBOM
& $tool cpu-evaluation --request $request
& $tool doctor --json '{"nativeGraphics":true}'
```

Plan inspection probes hardware and reads compiled policy without loading a model. It requires an
observed X2 Elite Extreme name and agreement between CIM and the native CPU-set count, plus
agreement between the CIM processor name and the bounded native registry name. Missing native
processor/kernel/feature identity rejects planning. The plan retains `HostIdentity`, separate from
live power or CPU availability. The planner never treats a name as kernel eligibility. Unknown
topology or a foreign device rejects the screen.

Native `doctor` exposes the same `host-cpu` output as `NativeCpuObservations`. [Native CPU observations](hardware-capabilities.md#native-cpu-observations)
defines group-relative topology, transient availability and the separate compatibility-digest scope.
CPU-set IDs/classes are not persistent affinity settings.

### Reading evidence

The workflow retains plans, CIM/native processor/kernel/feature/topology observations, ISA,
effective arguments, response hashes, raw synthetic captures and per-sample supervision.
Prompt/decode timings and process wall time remain separate. `WallMilliseconds` includes load,
but excludes prior artifact verification and CPU/power observations before creation/after owned
teardown. Fresh processes retain OS file cache; they are not cold-storage trials.

| Memory observation | Meaning and limitation |
| --- | --- |
| `ProcessMemory` | Existing nominal one-second supervisor queries plus final exit when available; successful count retained, including failed/over-ceiling samples; missing data is null with zero samples |
| `SampledPeakPrivateBytes`, `SampledPeakWorkingSetBytes` | Maxima of current process counters; brief allocations may escape sampling |
| `LifetimePeakWorkingSetBytes` | Windows lifetime resident peak, including mapped pages |
| Job commit / system available RAM | Job commit omits mapped residency; one-second physical observations can miss brief pressure |

Private commit, working set, Job commit and available RAM overlap: never add them. These
aggregates identify neither phase allocations/lifetimes, complete model storage nor energy.

For a separate main-only CI attribution capture, select the X2 workflow's
`observe_output_progress` dispatch input, or set `observeOutputProgress: true` in
the native `cpu-evaluation` capture request in its owning CI lane. The default remains off.
Supervision's optional `observeOutputProgress: true` requires a finite stdout cap and reuses the existing
file-length checks; it reads no payload and creates no extra polling loop.
`ProcessMemory.OutputProgress` partitions current-counter samples before the first
observed nonempty stdout, after that observation while the root has not been observed
exited, and after root exit. It retains the last observed empty-file time, first
nonempty-file time and whether that first observation happened after exit. Missing
boundaries and unsampled stage peaks remain null, with zero stage samples.

These are file/exit observations: prompt echoes, progress, buffering, final markers and
an exit racing a running observation prevent startup/prefill/decode or first-token
claims. The final exit observation belongs only to the exit bucket. The OS lifetime
working-set peak remains process-wide; stage maxima cannot be summed. Instrumented
captures set `InstrumentedAttribution: true` and suppress every timing ratio even when
conditions and token counts match. Actual-model coverage, phase instrumentation inside
the runtime and complete allocation/lifetime attribution remain pending.

Timing ratios require **both** gates below; invalid conditions preserve raw samples and
independent token-count comparability:

| Gate | Required comparison |
| --- | --- |
| Batch `ExecutionConditions` | Known unchanged native topology, AC, battery saver, active scheme/effective mode and CPU/kernel/full-feature identity at initial/final boundaries |
| `PairExecutionConditions` | Both samples' `CpuConditions.Before`/`After`/`Comparison` plus the transition between them, captured by optional `observeCpuConditions` outside process wall time without model polling |

The pair gate catches mode changes that reverse before batch end. Missing/malformed observations
or legacy receipts without `host_identity` remain raw-only. CIM/registry agreement proves neither
physical SKU nor optional-kernel parity. Power notifications are sequential with a one-second
initial wait. Charge percentage/transient parking/allocation do not gate comparison; equal
boundaries cannot exclude within-sample changes or same-scheme edits. Conditions measure no energy.

All repository device workflows, including TTS/sandbox trials, share one non-cancelling physical-
runner concurrency group; unrelated applications may still contend. Emit candidate/control
ratios only when both reach 16 tokens with matching prompt/decode counts. Natural early
completion stays visible without a fixed-length ratio; response equality is separate from quality.
Two short pairs give an initial smoke, not significance or sustained qualification.
Retain losses/failures; promising phases need complete-task, quality, responsiveness and comfort
follow-up. The screen supplies no energy verdict, accelerator enablement or new default.

#### CI availability and headless startup

The retained **2026-10-29** GitHub Actions inventory contains zero self-hosted runners and no
X2 screen runs. A fresh read-only `gh api repos/symbiogenesis/SwiftBonsai2Runner/actions/runners`
query on **2026-10-30** also returned `total_count: 0` and `runners: []`. This is an inventory
snapshot, not proof of a broken persistent configuration. Device evidence requires native ARM64
on settled `main` and the exact `x2-cpu-<run_id>` ephemeral label; local model execution cannot
substitute for CI. No runner was registered and no device job was dispatched for this check.

The pinned legacy `llama-completion.exe --version` fails with `3221225794` (`0xC0000142`, DLL
initialization failure) under ordinary supervision: ten seconds, 512 MiB Job, one process, all 63
verified files held through exit and no selected/loaded model. Identity: source
`73bfe6cc485554822f80c473a3c0c4f25e80f41b`, archive SHA-256
`cc808b33d35c72a3bd5c69a0d609d422426feb9dce23868e77992b6c33b8ddb3`. The PE chain
`llama-completion.exe` → `llama-completion-impl.dll` → `llama-common.dll` → `SHELL32.dll` suggests a
shell dependency without identifying the failing DLL. External
`Bonsai-X2-HeadlessProbe-dcbb58f9ed3440ff809d03673989ec31` retains `runtime-verification.json` and
`verified-headless/supervision.json`. Model trials require a passing bounded worker preflight;
inspection-only help/version cannot qualify it or authorize weaker Win32k denial.

The [UCRT argv source candidate](runtime-candidates/README.md#headless-ucrt-argv-candidate) removes the shell
import in an exact-source CPU build. Its paired control reproduces `0xC0000142`; candidate version
and seven Unicode/quoting/ownership cases pass unchanged worker policy on X2. Source/compiler/
imports/hashes/supervision are retained there. This model-free source result leaves provisioned
runtime/thread defaults unchanged pending upstream release review and device CI.

### Device evidence separation

Native speech builds use a separate checked host identity for build-cache reuse. CPU registry
identity, Windows kernel version, native target and OS feature observations partition the entire
Cargo/native output directory and its receipt. Changing from X1 to X2 repeats native compilation and
CMake's ISA probes. Speech CI retains the same host identity in every compiled-cache restore prefix
and verifies it in the static-library receipt. This prevents an older machine's native output from
silently bypassing X2 compilation; it does not select new thread counts or qualify a speech
performance gain.

LoRA and PTQ1 captures retain the observed processor name and distinguish `Snapdragon X Elite`,
`Snapdragon X2 Elite` and `Snapdragon X2 Elite Extreme`. X2 receipts require a matching processor
observation; relabeling historical X1 evidence rejects. Device-dependent runtime promotion gates
cannot combine different families. The [shared CPU promotion rule](#cross-generation-cpu-promotion)
reuses a reviewed deployment decision, without relabeling or mixing those records.
Existing frozen X1 contract hashes and legacy receipt shapes
remain readable; identity observations alone do not qualify any X2 profile.

[fearless]: https://docs.rs/fearless_simd/1.1.0/fearless_simd/
[target-feature]: https://doc.rust-lang.org/reference/attributes/codegen.html#the-target_feature-attribute
[detection]: https://doc.rust-lang.org/std/arch/macro.is_aarch64_feature_detected.html
[windows-features]: https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-isprocessorfeaturepresent
[algebraic]: https://doc.rust-lang.org/std/primitive.f32.html#algebraic-operators
[kernels]: https://github.com/symbiogenesis/prism-inference/blob/main/src/kernels.rs
[graph]: https://github.com/symbiogenesis/prism-inference/blob/main/src/graph.rs
[ternary]: https://github.com/symbiogenesis/prism-inference/blob/main/tools/ternary/src/lib.rs
[kernel-tests]: https://github.com/symbiogenesis/prism-inference/blob/main/tests/kernels.rs
[simd-tests]: https://github.com/symbiogenesis/prism-inference/blob/main/tests/simd.rs
[x1-brief]: https://www.qualcomm.com/content/dam/qcomm-martech/dm-assets/documents/Product-Brief-Snapdragon-X-Elite.pdf
[x2-brief]: https://www.qualcomm.com/content/dam/qcomm-martech/dm-assets/documents/Snapdragon-X2-Elite-Product-Brief.pdf
