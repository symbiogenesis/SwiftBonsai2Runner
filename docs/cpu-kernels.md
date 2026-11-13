# CPU kernels and X2 thread evaluation

Baseline NEON supports PQ2, odd-group PQ2 and PTQ1 with scalar reference/fallback.
This guide owns arithmetic, dispatch, qualification and the
[X2 thread screen](#x2-cpu-thread-screen). Synthetic parity qualifies neither
whole-model parity nor device speedup. Runner's active
[release lock](../config/prism-inference.lock.json) pins Prism 0.1.27 at
`1429a33772089dfd3a50a96c698b9dbe0b36836a`; source candidates retain separate identities. The fixed CPU
comparison baseline remains 0.1.23 with explicit FP16 KV. The current development repin
does not qualify CPU performance; [PLAN.md](../PLAN.md#cpu-kernels) owns remaining work.

## Cross-generation CPU promotion

Shared CPU work reductions may be enabled on X1 and X2 after matched CPU-only
complete-request benefit on one selected family and an applicability review of
the other. This permits a common runtime repin; measured speed, energy and
sustained-use claims remain specific to the tested device.

The review must establish:

- Final-source numerical, complete-state, adapter, cancellation, Release/linking
  and resource acceptance, including native, portable and feature-disabled paths.
  Optional SDOT/I8MM keeps checked CPU/OS detection and baseline fallback.
- A paired same-artifact comparison charging preparation, dispatch, fallback and
  cleanup under the [promotion criteria](validation.md#promotion-criteria),
  [measurement rules](performance-expectations.md#measurement-and-comparison) and
  [device budget](../AGENTS.md#owner-device-execution-budgets).
- Why the work reduction applies without depending on the unmeasured family's
  cache size, topology, optional ISA or increased memory admission. Source work
  counts alone do not establish benefit.
- An explicit control or matching-family qualification for cost tradeoffs such
  as LUT versus SDOT, tile/cache sizes, workers/affinity or extra reads to reduce
  register pressure. Without a selector, defer uncertain choices from shared
  promotion.

The families share ARM64/NEON but differ in core topology, cache and bandwidth;
see Qualcomm's [X1][x1-brief] and [X2][x2-brief] briefs. A regression reopens the
shared choice and requires verified rollback or an available explicit control.
Keep device receipts separate. Accelerator, precision/quality, memory-admission
and complete device-profile changes retain their own qualification gates.

## CPU comparison controls

The [native CPU bundle lane](../tools/cpu-bundle/README.md) fixes Prism 0.1.23
versus 0.1.25, FP16 KV, adapter 1.3 and four workers. It does not qualify the active
runtime, K4/V4 default or individual optimizations. The
[X2 thread screen](#x2-cpu-thread-screen) instead compares publisher `4/4` and
`6/6` thread budgets. Neither supplies a qualified CPU performance result.

## Prompt and attention consumers

These explicit source consumers are outside ordinary Runner launch policy:

| Consumer | Contract |
| --- | --- |
| Parallel quantized attention | Share the private row pool across independent query heads; preserve each head's arithmetic and complete-state transaction. Serial compressed attention is the control. |
| `SwiftSession::load_profile_with_prefill` and private server | Explicit 8/16/32-token tiles with FP16, K4/V4 or K4/V3 cache. Pair tile size with scratch and charge the full reserve before artifact I/O. Readiness binds tile/reserve/graph; implicit Hexagon installation is skipped. Latest-prefix reuse remains explicit. |
| Sampled suffix profile | Use the actual target sampler with the tiled verifier. Matching draws accept deterministic proposals; rejection restores complete state and replays the accepted prefix. Charge separate logits and full-prefix checkpoints in addition to tile and ordinary rollback reserves; load no second model. |

The [algorithm contract](inference-algorithms.md#hybrid-cache-reuse-and-speculative-state)
owns verifier and speculative-state requirements. Triangular GDN remains a
separate profile. All candidates below require the
[cross-generation gates](#cross-generation-cpu-promotion); source presence does
not establish selection, benefit or a Runner repin. New helper APIs require
publication before dependent runtime bundles and reviewed Runner locks.

## CPU preparation candidates

### Provisional GDN and prepared current-token KV

- Graph-owned eight-row GDN writes directly to an unpublished recurrent delta.
  Committed state remains immutable until success; public defensive helpers
  retain their failure behavior. Swift's geometry omits a nominal 144 MiB final
  copy per token, without removing recurrent reads/writes or changing admission.
- Packed attention prepares the current K/V once in private records and reuses
  those bytes at final append. Layer entry, failure, commit, reset and copying
  invalidate reuse; narrowed final-layer evaluation retains ordinary encoding.
  Tail/page reservation still occurs at append. No new record payload is retained.

Qualify exact native/portable arithmetic, stale-record and late-failure rejection,
selected-cache and FP16 state/adapter/cancellation behavior, codegen and resources.
Packed-cache equivalence is within the selected codec, not equivalence to FP16.

### Preparation storage and tokenizer

- PQ2 SDOT uses compact byte planes; baseline NEON retains widened lanes.
  Each 128-column native payload is 136 rather than 392 bytes, plus a 24-byte
  vector header per scratch owner. Q8_K bytes and floating accumulation order
  remain unchanged; these are logical storage counts, not process-memory savings.
- Width-four short convolution prepares immutable coefficients at graph
  construction without a second copy. Decode/prefill retain FMA, SiLU and history
  order; other widths and raw-slice helpers keep their existing representation.
- Uncached BPE pieces up to sixteen bytes merge on bounded stack storage.
  Adjacent-rule caching preserves minimum-rank/leftmost selection, corruption
  errors and token limits; longer pieces retain the bounded heap path.

### Dispatch and attention preparation

- Prepared PQ2 activations retain a safely detected SDOT capability for one-,
  four- and eight-row consumption, avoiding feature probes inside block loops.
  Native scratch adds eight padded header bytes; fallback remains independent.
- Six-head and paired-token attention keep aligned native score reductions in
  registers with scalar tails, preserving temporal FMA lanes and addition order.
- Eligible parallel attention joins query normalization/RoPE with K/V
  normalization, RoPE and rounding. Divisible partitions omit one submission and
  error-slot allocation per full-attention layer. Query failures take precedence;
  the earliest original K/V chunk wins after all workers join. Only provisional
  scratch may change on failure.

### Tiled dispatch and adapter preparation

- PQ2 prefill reuses prepared SDOT/I8MM capabilities across row/token tiles and
  tails, preserving integer kernels, symbols, scales and baseline fallback.
- Single-, paired- and triple-buffer dispatches keep at most eighteen error
  slots on the stack, including nested calls. All tasks join before returning
  the earliest failing chunk; worker reservations remain conservative.
- Adapter-enabled FFN combines original-basis SwiGLU preparation with the down
  projection's signed Hadamard preparation in existing buffers. The down adapter
  continues to consume original-basis input; accelerator eligibility is unchanged.

### Grouped dispatch, hardware half widening and exact LUT execution

- Compatible PQ2/Q8_K siblings share one row-pool submission and scoped input
  preparation. Check every extent first; clear all sibling outputs on failure
  and retain sibling/chunk error order. At most four matrices use seventy-two
  bounded descriptors, without weight copies or a second pool.
- Safe `bonsai-neon-dot` FP16 helpers use baseline NEON `FCVTL`/`FCVTL2` for finite
  arrays and exact integer decoding for infinity/NaN arrays, retaining sign and
  signaling-NaN payloads. Portable decoding and raw-u32 rounding remain controls.
- Explicit `Pq2LutExecutor` builds a 2,048-byte activation-pair table per 128
  columns for all output rows. Each invocation, including siblings, rebuilds its
  tables; retained capacity is bounded by maximum width (80 KiB at 5,120).
  Preserve Q8_K fused/separate accumulation, odd-group Q8_0 and all PQ2 symbols.
  Unsupported tensors stay on CPU. Include table/cache costs against SDOT before
  selecting this executor; it is never selected automatically.

### Complete-row SDOT, fixed attention lanes and bounded GDN phases

- SDOT keeps complete one-/four-/eight-row projections, half widening and ordered
  floating accumulation inside one feature call. Preserve Q8_K scales, symbols,
  fused/separate arithmetic and 136-byte prepared blocks. Baseline NEON, scalar
  and multi-token kernels retain their paths.
- Six-head/paired-twelve-query attention consumes sixteen key columns per loop
  with four fixed accumulator indices, preserving half-conversion sharing,
  FMA lanes, masks, tails, reductions and paired-token sharing.
- Eight-row GDN uses two four-row arithmetic phases with sixteen accumulator
  vectors per dot phase. All key dots precede updates; publication remains atomic.
  Keep 4 KiB staging and eight scalar deltas. Repeated key/query reads must be
  included in benefit and codegen/spill/stack review.

### Scoped PQ2 validation, tiled Q/K/V preparation and temporal GDN

- A borrowed prepared-PQ2 view validates activation scales once per projection
  or sibling group. The immutable borrow binds current bytes; weight-scale,
  geometry and result checks remain. Public unprepared calls stay defensive.
- Tiled attention joins Q/K/V preparation in one token-partitioned submission,
  preserving query-first and earliest-K/V error precedence and transactional state.
- Multi-token eight-row GDN uses four-row phases for source-state and in-place
  paths. All key dots precede updates; staging and publication stay atomic.
  Lower source register pressure alone does not establish benefit.

### Prepared packed-KV queries and CPU preparation reuse

- Prepare each immutable rotated query before scanning keys, moving query/
  codebook validation and native selection outside the history loop. Each
  record still validates length, scale, canonical zero and finite result;
  public single-record scoring stays defensive.
- Internally quantized PQ2 activations reuse exact per-sixteen-value sums.
  Arbitrary public blocks retain value-derived sums. Bytes, scales, reduction,
  ISA fallback and allocations are unchanged.
- Eligible prompt convolution reads committed history into provisional next
  history without a preceding copy. Preserve FMA/SiLU and publication order;
  other geometries keep their existing path.

### Scan-owned packed values and joined prompt projections

- Packed attention owns one provisional value accumulator for the complete
  history scan. Validate codebook/native dispatch once and each record's
  encoding, weight, scale and arithmetic. Failure prevents publication;
  public single-record accumulation keeps unchanged-output rejection.
- Compatible prompt-tile PQ2 siblings share a row-pool submission and prepared
  activations. Targeted adapters retain sequential failure precedence;
  unsupported geometry/transforms/executors keep existing paths. Ordinary
  single-token ingestion does not acquire prompt tiles.
- Prepared native PQ2 consumption uses the helper's extent check on success
  and classifies malformed extents after rejection, preserving shape-error
  precedence. Baseline NEON and portable consumers keep their own checks.

Verify the exact release lock and consumer before execution.
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

The explicit `Pq2LutExecutor` connects bounded LUT preparation to matvec and graph
execution; it is not automatically selected. Cross-token Vec-LUT remains a
separate consumer. Existing packed token tiles reuse decoded weight blocks,
which is different from activation-LUT execution. The
[lookup contract](inference-algorithms.md#packed-lookup-kernels) distinguishes them.

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
traffic/latency remain unmeasured. Eligible NEON builds use direct eight/four-row
BF16 helpers through the existing row-worker pool;
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

[Ben Joffe's algorithm](https://www.benjoffe.com/fast-floordiv) is conditional on
an identified signed constant-divisor hotspot with floor-rounding semantics.
No eligible production caller is identified. Unsigned indexing, power-of-two
extents, floating scales and runtime-stride division do not supply that consumer.

For a proven sign-selected multiplier/shift pair, the formula is
`(i128(x) * (M + (x >> 63))) >> S`. Some divisors require bounded adjustment;
the formula is not universal. Preserve the full admitted range, reject zero and
`i64::MIN / -1`, and prepare invariant parameters outside hot loops. Rust `/`
truncates toward zero; `div_euclid` is not a general negative-divisor floor oracle.

A selected implementation needs a pinned licensed reference, independent widened
quotient/remainder checks across signs/extrema/multiples/neighbors, ARM64 codegen
review and complete-request benefit including setup/dispatch. Keep this work
within the chosen kernel's [backlog](../PLAN.md#cpu-kernels).
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

## X2 CPU thread screen

The screen targets native Windows ARM64 on X2 Elite Extreme. Capture current
CPU/kernel/features, topology, driver and power identities for each workload;
stored inventory snapshots do not establish current conditions or kernel eligibility.
Native ISA semantics live in [capability contracts](hardware-capabilities.md).

The manual [X2 CPU workflow](../.github/workflows/x2-cpu-evaluation.yml) prepares exact maintenance
tools on hosted ARM64 CI. Its default `run_device: false` performs no owner-device execution.
The optional device job requires settled `main`, a native ARM64 owner runner with the single-run
label `x2-cpu-<run_id>` and the shared model-capture concurrency group. Register that runner as
ephemeral only for the reviewed run; a persistent generic runner could accept old queued jobs.
Performance execution remains [CI-only](validation.md).

### Controls and cost

The Rust maintenance planner selects only `6/6` decode/prefill threads against the unchanged
`4/4` control. These are thread counts without affinity pinning; Windows chooses placement.
One fixed short synthetic creative-writing prompt runs in two reversed pairs: at most four fresh processes, a 16-token cap and a 45-second process deadline. OS file cache is not flushed. Other thread budgets and long prompts remain in PLAN.

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

#### Headless startup prerequisite

Model trials require the exact verified publisher completion binary to pass the
bounded worker preflight under unchanged containment. Inspection-only help or
version success cannot substitute for that launch. Diagnose a startup failure
before timing models; never weaken Win32k denial to make it run. The
[UCRT argv source candidate](runtime-candidates/README.md#headless-ucrt-argv-candidate)
has a separate source, build and device-qualification contract and does not
automatically replace the installed publisher runtime.

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
observation; relabeling X1 evidence rejects. Device-dependent runtime promotion gates
cannot combine different families. The [shared CPU promotion rule](#cross-generation-cpu-promotion)
reuses a reviewed deployment decision, without relabeling or mixing those records.
Identity observations alone do not qualify an X2 profile.

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
