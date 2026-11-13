# Hexagon deployment and execution contracts

Runner pins [Prism 0.1.25](../config/prism-inference.lock.json), source
`99bab71270b1e6a6fb6d47eec2e34b8097cdcd4c`. On Windows ARM64 Snapdragon X2 (`v81`),
`prism-inference-server` selects experimental `native-q2` resident NPU acceleration
when the verified signed GenieX v0.10.0 package and CDSP driver are available.
The expected DSP file is `<data_directory>/hexagon/runtime/libggml-htp-v81.so`.
When those prerequisites are absent, the [Rust runtime][rust-runtime] retains CPU
inference, with four NEON row workers under Runner's default launch policy.

The fixed placement (`DEFAULT_NATIVE_Q2_PROJECTIONS`) covers 168 `PQ2_0`
projections across `blk.0..=blk.26`. Its 2,888,663,040 resident weight bytes
(2,754.84 MiB) occupy six FastRPC arenas, each at most 480 MiB:
`[502_824_960, 460_062_720, 502_824_960, 460_062_720, 502_824_960, 460_062_720]` bytes.
Queue descriptors select those arenas through `req.n_bufs > 1` and `tensor.bi > 0`.
Weights are prepared at worker startup. Submission uses scoped cache flushes of
at most 92 KiB, 2/3-operation sibling packets, 4/8-operation prefill packets and
fused `ffn_gate + ffn_up + HTP_OP_GLU_SWIGLU` VTCM chains, with no per-token fixture allocation.

The [experimental lock](../config/hexagon-experiment.lock.json) records
`"admission": "default-x2-native-q2-resident"` and `"automaticSelection": true`
for this fixed development policy. Full-model X1/X2 qualification and the
[qualified profile selector](hardware-capabilities.md#automatic-profile-selection)
with interactive/battery objectives remain separate work in [PLAN.md](../PLAN.md#hexagon-npu). `native-q2` uses
bounded floating-point tolerance; it does not promise the bitwise PQ2/Q8_K
parity of the historical block-preserving controls below. The latest
[public-broker capture](performance-expectations.md#current-swift-evidence)
did not observe effective NPU placement.
Multi-token prefill packets batch independent `N = 1` operations; a true
`N > 1` matrix solver remains separate work.

## Historical projection controls

The [source audit, placement ledger and access evidence](hexagon-source-audit.md) distinguish signed
reference access, the early native Rust Prism boundary and the newer signed Q2 route. The first
Swift prototype is one stateless FFN projection. CPU-only [layout](../tools/hexagon/README.md) and
[operation/wire controls](../tools/hexagon/packed-projection.md) qualify no signed device implementation,
measured VTCM/traffic or inference.

[Prism source `1e7ed92`](https://github.com/symbiogenesis/prism-inference/tree/1e7ed92e7787b9d8b150f9bb3fb5a74fa92fb14b/tools/hexagon-runtime)
connected explicit guarded-HVX and HMX bitplane executors to the real Rust
graph through a borrowed session. Original quantization, transforms, adapters
and committed state remain CPU-owned. This historical integration was separate
from Runner's then-pinned CPU release. It included an explicit PTQ1/odd-block PQ2
Q8_0 executor preserving native DOTPROD lanes and float order, with actual remaining
native capacity and whole-request operation checks before new inference.
Broader signed PQ2/PTQ1 model execution, resource qualification and persistent
serving remain pending. The [static work ledger](evidence/hexagon-swift-operation-ledger.json)
records the historical terminal-cap refusal. The owner-authorized
[24,000-operation budget](evidence/hexagon-session-operation-budget.json) now
fits one all-eligible original-PQ2 evaluation, with 744 submissions left.
It does not fit a hidden token plus evaluation (45,912 submissions), waive
complete-request admission or extend the 120-second session/cleanup limits.
The separate [local actual-model probe](../tools/hexagon/model-probe.md) now
provides a runnable supervised first-projection path: one original-PQ2 token,
every returned integer limb checked independently, complete CPU-shadow
logits/state comparisons, and terminal publication only after explicit native
cleanup. After the separate
[local source validation](evidence/hexagon-model-execution-20261103/summary.json),
the owner-authorized [X2 AC device control](evidence/hexagon-model-device-20261103-ac/summary.json)
passed for token 42: 40 native operations, all 1,392,640 integer values, every
logit and complete logical state matched the CPU shadow. Signed package/driver
intake, explicit stop/close/unmap/free and empty owned-Job teardown passed.
Other projections remained CPU-owned; no adapter or HMX path ran. This qualifies
that single-token first-projection control and enables no production profile.
The separate [two-token HMX capture](evidence/hexagon-hmx-session-20261103/summary.json)
completed 80 signed bitplane operations for tokens 42 and 43. Its source-bound
worker checked all 11,141,120 returned plane values and complete per-token
CPU-shadow logits/state bits. All four explicit cleanup return codes were zero,
free completed and the owned Job was empty. The original supervisor rejected
the valid native ownership flag; that receipt remains unchanged. A corrected
validator and offline replay accept the retained terminal event without another
device run. `ownerPinnedUntilExit: true` preserves driver/module/catalog owners
through process exit after buffer cleanup; it is required by the native
completed-cycle contract. Effective HMX placement/VTCM, adapters, wider placement,
vendor teardown, persistent serving and performance remain unqualified.
The separate [adapter/default-policy preparation](evidence/hexagon-adapter-session-20261103/summary.json)
adds exact approved-adapter cases and explicit one/four-worker scalar/NEON policy
to this diagnostic. Its one four-token standard-adapter/1.3 control failed in the
CPU reference arm before observed native setup. No adapter/NPU parity or benefit
passed. The original generic error and pending supervisor receipt remain intact;
an empty owned Job passed, while a zero outer exit code is not acceptance.
Static geometry proves its four cumulative logical CPU snapshots require
628,883,456 bytes, above the fixed 512 MiB control bound. Source-only corrections
now reject insufficient capacity before model loading and retain CPU stage/token
diagnostics plus the child exit status. The fixed supervisor budget fits at most
three tokens even though the raw schema has an eight-token ceiling. Focused
model-free checks did not qualify another device run. That historical trial
remains closed; the distinct corrected-source controls below cover the new work.

The [bounded adapter and resident-host workload](evidence/npu-adapter-progress-20261103/summary.json)
now passes original-PQ2 first-projection execution with the standard adapter at
scale 1.3 for tokens 42, 43 and 128, explicit NEON and four row workers. It checked
all 16,711,680 returned integer values over 120 signed HMX submissions and matched
complete logits/state after every token. The separate loaded-zero token-42 case
checked 5,570,560 values over 40 submissions against an adapter-free CPU baseline.
All native cleanup codes were zero, allocations were freed, required owners
remained pinned through exit and each owned Job ended empty.

An explicit `residentHostWeights: true` screen prepares 25,067,520 packed bytes in
a 26,259,456-byte native mapping and admits at most 65,536 metadata bytes. The
three-token standard-adapter screen observed two cache hits and complete CPU
logits/state parity. It uses `integerAudit: graph-parity-only`: no repeated CPU
integer oracle, no copied weight oracle and no streaming fallback. This is mapped
host weight reuse, not demonstrated DSP residency. The original streaming default
still independently audits every returned integer. Both modes retain the same
native duration, operation, model and cleanup limits.

Historical milestones below retain their original source, power and trial boundaries.
Successful HMX rows matched complete per-token CPU logits/logical-state bits.
Native-Q2 rows passed bounded logit comparisons; their receipts report
`logitsBitwiseEqual: false` and `stateBitwiseEqual: false`, not full-state parity.
All successful rows completed explicit native cleanup with empty owned Jobs.
Earlier controls did not change the application default; bitplane HMX remained
experimental. Linked receipts retain exact packet layouts, allocations, counters
and raw timings; the next table compares the principal device timings.

| Control and source | Result | Power and workload limits |
| --- | --- | --- |
| [Resident-host screen](evidence/npu-adapter-progress-20261103/summary.json) | CPU evaluation 502-536 ms versus native 512-633 ms, including first-token preparation; no speed benefit established. | Resident arm endpoints: AC, saver off. Earlier streaming/loaded-zero controls: battery, saver off. No cross-power cache benefit. |
| [Packed-weight transpose](evidence/npu-host-reduction-20261103/summary.json), Prism `16c5e53` | Three-token streaming/resident parity with adapter/1.3, NEON/four workers; all 16,711,680 streamed integers audited. Fourteen focused controls, strict library Clippy, formatting and Release probe build passed. Resident CPU 362-403 ms versus native 420-477 ms; no useful benefit or before/after packing speedup. | Every CPU/native arm endpoint: AC, saver off, 48% charge; continuity unknown. Predates the `native-q2` development default. |
| [Batch-four HMX](evidence/npu-batch4-controls-20261104/summary.json), Prism `6f31095`; [signed batch route](hexagon-source-audit.md#batching-existing-signed-block-operations) | Four `K=128/N=8` operations per packet reduced writes from 40 to 10 per projection/token. Streaming audited all 16,711,680 plane integers; resident weights prepared once with two cache hits. Submit/wait: 33.91-35.52 ms streaming, 31.16-32.05 ms resident. | All four arm endpoints in both runs: AC, saver off, 97% charge. Outer workload opened on battery/82% and connected to AC during build setup. |
| [Fused reconstruction and two projections](evidence/npu-multi-projection-20261104/summary.json), Prism `ebc22c2`, Runner `bbc63034` | Removed unused resident streaming-weight zeroing; replaced `truncf` with exact `i16` roundtrip checks and fused row-major `batch_four` validation/reconstruction. Resident `blk.0.ffn_up.weight` + `blk.0.ffn_gate.weight` used 50,135,040 weight bytes, 240 operations and 60 writes across tokens 42, 43, 128. | AC, 100% charge at all arm endpoints. |
| [Batch-ten and parallel reconstruction](evidence/npu-batch10-fast-queue-20261105/summary.json), Prism `5136bd1`, Runner `580a0062` | Ten operations per packet; `queue_read` 516 (`AEE_EWOULDBLOCK`) spin-polling for at most 15 ms; four-worker 64-row L1-tiled reconstruction. One/two projections used 120/240 operations in 12/24 writes across tokens 42, 43, 128. Each projection still transferred 22.28 MB of eight-plane FP32 output; no default promotion. | Battery, saver off, 89% charge at workload/arm endpoints; 43.36 s workload. Separate from prior AC sessions. |
| [8-HVX / 2 MiB VTCM / Burst](evidence/npu-hmx-8hvx-2mib-vtcm-20261105/summary.json), Prism `9f61217`, Runner `76e3b38b` | Eight 128-byte HVX contexts, four active workers and 2 MiB chunking reduced chunks/op from 136 to four. Bitwise HMX parity passed, but 22.28 MB output/projection remained versus ~2-3 ms for four-worker NEON CPU. | AC, saver off, 95% charge at workload/arm endpoints; 199.09 s within 300 s. |
| [One-op full-K native Q2](evidence/npu-native-q2-1op-20261105/summary.json), [decision](evidence/npu-native-q2-1op-20261105/decision.json), Prism `ccb1339`, Runner `cd3485bb` | Lossless `PQ2_0` to `HTP_TYPE_Q2_0` weight transcoding; one `K=5120, M=17408, N=1` operation returned a 69,632-byte F32 vector, 320x less output with no host bitplane reconstruction. Bounded-float execution: cosine 0.999906-0.999974, RMSE 0.0221-0.0286, top-1 tokens 1183/12/248 and 5/5 top-5 overlap across tokens 42/43/128. | AC, saver off, 99% charge at workload/arm endpoints; 32.26 s within 300 s. |
| [Full-layer/multi-layer native Q2](evidence/npu-full-layer-native-q2-a383e92-20261105/summary.json), [decision](evidence/npu-full-layer-native-q2-a383e92-20261105/decision.json), Prism `a383e92` | All ten recurrent/full-attention projection geometries through `K=17408`; six `blk.0` or thirteen `blk.0 + blk.3` projections passed bounded logit comparisons, exact top-1 and 5/5 top-5 overlap. The earlier [27.69 s diagnostic](evidence/npu-full-layer-native-q2-20261105/decision.json) on `8a7980a` found unscoped `HmxQ2WeightRegistry` capacity validation in `Session` construction; its clock stays closed. | Battery, saver off, 90% charge at workload/arm endpoints; 32.97 s trial, 60.66 s cumulative within 300 s. |
| [Scoped flush / four-layer native Q2](evidence/npu-4layer-25proj-native-q2-20261105/summary.json), [decision](evidence/npu-4layer-25proj-native-q2-20261105/decision.json), Prism `e2349af` | Resident cache hits flush only the page-aligned active prefix (<=92 KiB), retaining full allocation size in `bufs[0].size`. Limits became 480 MiB resident weights, 256 KiB metadata and 26 projections. Thirteen- and 25-projection placements passed bounded logit comparisons, exact top-1 and 5/5 top-5 overlap. | Battery, saver off, 76% charge at workload/arm endpoints; 37.13 s within 300 s. |

The [static tiling and plane-reduction audit](evidence/hexagon-k-tiling-and-reduction-audit.json)
replays with `pwsh -NoProfile -File tools/hexagon/verify_k_tiling_and_reduction_audit.ps1`.
Pinned v0.10.0 HTP source rules out bitwise `PinnedSwift` parity from multi-block
`K>128` tiling: `HMX_STORE_AFTER_F16` truncates output to FP16, and per-128-block
`PQ2_0` scales cannot be factored out. `HTP_OP_SUM_ROWS` reduces `ne[0]=17408`,
not the eight bitplanes (`ne[1]=8`). The later `executionBackend: "native-q2"`
uses `integerAudit: "bounded-float-tolerance"`; its numerical contract differs
from the bitwise HMX controls.

Per-token windows separate CPU evaluation, native graph evaluation, native submission/wait
and the streaming integer oracle. They exclude loading, graph creation, device setup/cleanup
and state capture/comparison. Matching power endpoints do not prove uninterrupted stability.
These diagnostics establish no release performance acceptance or cross-power benefit;
wider placement, persistent service, effective resources and packaging remain separate gates.

## Snapdragon X2 Hexagon NPU (`v81`) projection and layer timings

Losslessly transcoding `PQ2_0` weights into tiled `HTP_TYPE_Q2_0` reduces shared-memory output
traffic per `[5120, 17408]` projection by **320x** (`69,632` bytes vs. `22.28 MB` of 8-plane `FP32`
bitplanes) and eliminates host bitplane reconstruction. The table below summarizes the supervised
3-token (`42, 43, 128`) Snapdragon X2 Elite Extreme device controls (`LoRA 1.3`, 4 CPU row workers):

| Placement / backend | Projections (`PQ2_0`) | Resident weight payload (`FastRPC` mapped) | NPU submit & wait / token (ms) | Per-projection NPU time (ms) | Token evaluation: NPU vs. 4-worker NEON CPU (ms) | Numerical verification | Power condition |
| --- | --- | ---: | ---: | ---: | ---: | --- | --- |
| [**1-op `native-q2` resident (`ccb1339`)**](evidence/npu-native-q2-1op-20261105/summary.json) | 1 (`blk.0.ffn_up`, `[5120, 17408]`) | 23.91 MiB (47.90 MiB) | **0.64–0.98** | **0.64–0.98** | 520.84–561.49 vs. 515.92–589.57 | Cosine `0.999908–0.999972`, RMSE `0.0221–0.0286`, top-1 `3/3`, top-5 `5/5` | AC (`99%`, saver off) |
| [**1-op `native-q2` resident (`ccb1339`)**](evidence/npu-native-q2-1op-20261105/summary.json) | 2 (`blk.0.ffn_up + ffn_gate`) | 47.81 MiB (71.81 MiB) | **1.08–1.44** | **0.54–0.72** | **506.15**–567.14 vs. 516.49–591.33 | Cosine `0.999906–0.999974`, RMSE `0.0226–0.0284`, top-1 `3/3`, top-5 `5/5` | AC (`99%`, saver off) |
| [**Full recurrent layer `native-q2` (`a383e92`)**](evidence/npu-full-layer-native-q2-a383e92-20261105/summary.json) | 6 (`blk.0` all recurrent projections) | 102.66 MiB (126.65 MiB) | **2.27–2.87** | **0.38–0.48** | 395.80–603.33 vs. 547.11–628.76 | Cosine `0.999893–0.999973`, RMSE `0.0249–0.0306`, top-1 `3/3`, top-5 `5/5` | Battery (`90%`, saver off; diagnostic) |
| [**2-layer `native-q2` unscoped flush (`a383e92`)**](evidence/npu-full-layer-native-q2-a383e92-20261105/summary.json) | 13 (`blk.0` recurrent + `blk.3` full-attn) | 202.50 MiB (226.50 MiB) | **4.38–5.46** | **0.34–0.42** | 496.00–586.48 vs. 544.23–604.43 | Cosine `0.999902–0.999977`, RMSE `0.0228–0.0290`, top-1 `3/3`, top-5 `5/5` | Battery (`90%`, saver off; diagnostic) |
| [**2-layer `native-q2` scoped `<= 92 KiB` flush (`e2349af`)**](evidence/npu-4layer-25proj-native-q2-20261105/summary.json) | 13 (`blk.0` recurrent + `blk.3` full-attn) | 202.50 MiB (226.50 MiB) | **4.43–4.87** | **0.34–0.37** | **422.49–446.20** (warm) vs. 463.97–478.07 (`1,386.00` vs. `1,392.70` total) | Cosine `0.999902–0.999977`, RMSE `0.0228–0.0290`, top-1 `3/3`, top-5 `5/5` | Battery (`76%`, saver off; diagnostic) |
| [**4-layer block `native-q2` scoped flush (`e2349af`)**](evidence/npu-4layer-25proj-native-q2-20261105/summary.json) | 25 (`blk.0..=blk.3` complete repeating block) | 407.81 MiB (431.81 MiB) | **10.05–10.91** | **0.40–0.44** | 807.50–1,166.27 vs. 425.83–970.25 | Cosine `0.999895–0.999972`, RMSE `0.0217–0.0316`, top-1 `3/3`, top-5 `5/5` | Battery (`76%`, saver off; diagnostic) |
| [**8-HVX / 2 MiB VTCM / Burst `batch10` HMX (`9f61217`)**](evidence/npu-hmx-8hvx-2mib-vtcm-20261105/summary.json) | 1 (`blk.0.ffn_up`, 40 ops in 4 writes/tok) | 23.91 MiB (35.24 MiB) | **8.18–13.28** | **8.18–13.28** | 320.31–536.98 vs. 294.61–342.29 | **0-ULP bitwise parity** (logits & state) | AC (`95%`, saver off) |
| [**8-HVX / 2 MiB VTCM / Burst `batch10` HMX (`9f61217`)**](evidence/npu-hmx-8hvx-2mib-vtcm-20261105/summary.json) | 2 (`blk.0.ffn_up + ffn_gate`, 80 ops in 8 writes/tok) | 47.81 MiB (59.15 MiB) | **15.60–26.77** | **7.80–13.39** | **457.34–464.30** (warm) vs. 552.44–553.09 | **0-ULP bitwise parity** (logits & state) | AC (`95%`, saver off) |
| [**4-worker L1-tiled `batch10` HMX (`5136bd1`)**](evidence/npu-batch10-fast-queue-20261105/summary.json) | 1–2 (`blk.0.ffn_up`, `+ ffn_gate`) | 23.91–47.81 MiB | 22.21–23.48 (1p) / 44.08–46.26 (2p) | 22.04–23.48 | 487.91–529.85 (1p) / 513.61–537.70 (2p) | **0-ULP bitwise parity** (logits & state) | Battery (`89%`, saver off; diagnostic) |
| [**Fused host-reconstruct `batch4` HMX (`ebc22c2`)**](evidence/npu-multi-projection-20261104/summary.json) | 1–2 (`blk.0.ffn_up`, `+ ffn_gate`) | 23.91–47.81 MiB | 32.53–34.22 (1p) / 64.66–66.22 (2p) | 32.33–34.22 | 432.66–487.59 (1p) / 514.98–553.37 (2p) | **0-ULP bitwise parity** (logits & state) | AC (`100%`, saver off) |

Battery-operated diagnostic runs (`a383e92`, `e2349af`, `5136bd1`) are kept separate from
AC-connected runs (`ccb1339`, `9f61217`, `ebc22c2`) and are not treated as qualified
cross-power speed comparisons. Matching power endpoints do not prove
uninterrupted stability, and these short diagnostics do
not qualify whole-task speedups. Full-model 6-arena (`168`-projection) AC-connected
end-to-end service qualification remains pending in [PLAN.md](../PLAN.md#hexagon-npu). Local Prism
also implements `image_ffn_full_chain_resident` (`NativeQ2Executor::project_f32_ffn_chain`
chaining `ffn_gate` + `ffn_up` + `HTP_OP_UNARY_SILU` + `HTP_OP_BINARY_MUL` + `HTP_OP_HADAMARD` +
`ffn_down` inside one `dspqueue` packet), `requires_dma64_stride`
(`HTP_DMA32_MAX_STRIDE_BYTES = 65,535`), `project_f32_with_rank1_adapter`, and
`plan_hybrid_prefill_pipeline` (double-buffered CPU attention/GDN + NPU `168`-projection FFN
pipeline schedule).

The historical bitplane [logical payload ledger](evidence/hexagon-swift-traffic-ledger.json)
derives 13.69 GB of signed-kernel input/result payload per all-eligible
original-PQ2 evaluation token. Its opt-in [mapped host-weight arena and graph cache](../tools/hexagon/resident-host.md) reuse immutable
packed tiles across tokens, with a 64 MiB HMX bitplane arena ceiling,
separate host metadata accounting and unchanged terminal operation/deadline
limits. The cache binds exact immutable source storage and geometry; complete capacity misses select
the streaming path before preparation. Preparation or submission failure closes the scope without
replay. This removes repeated CPU packing/copies when hits occur; the kernel payload stays
unchanged, and every submission still covers the expanded allocation with the existing cache flags.
Actual cache cost may increase. [Current static bounds](evidence/hexagon-resident-host-24000-ledger.json) establish no DSP residency, performance or
production admission. These bitplane bounds do not describe the default
`native-q2` direct-output path, whose six `<= 480` MiB arenas are described above.

Explicit immutable projection selection supports bounded partial-graph work,
including the original first-projection milestone, while the other projections
retain their CPU paths. Placement identity binds the model, executor, selected
tensors and operation counts; whole-request admission still precedes inference.
This remains an arithmetic compatibility bridge. Fused signed kernels, efficient
device weight reuse, true prefill batching and daily serving remain separate work.

[Preflight](../tools/bonsai/src/hexagon_tools/preflight.rs) inventories drivers and an explicit optional package without loading/installing it. It
uses a native Rust worker with an aggregate deadline and kill-on-close Job ownership. Present
devices come from SetupAPI; unavailable driver signatures and dates remain unknown. [Hosted CI](../.github/workflows/hexagon-qualification.yml) owns
full regressions and source-bound preparation. Its main-only opt-in owner job selects one inventory,
numeric dispatch or native session probe within five minutes total, including setup and cleanup.

## Device and route boundaries

Acceptance requires completed-response latency or energy benefit within memory, quality and sustained-comfort
limits against the same Swift artifact on CPU and qualified Adreno. TOPS, utilization and simultaneous
CPU/GPU/NPU activity do not qualify a backend. X1/X2 qualify independently.

| Device class | Execution boundary |
| --- | --- |
| Recorded X1E-78-100 / v73 | Packed HVX arithmetic, native signed access, mapping-window/dynamic-mapping costs |
| X2 / v81, initially Elite Extreme | Batched HMX prefill, packed HVX decode, verified DMA/VTCM limits and matching signed package |

Receipts bind exact SKU/capabilities, driver/runtime/kernel bytes, ABI and resource limits;
processor names establish none of these. Unknown or changed facts exclude the candidate. Shared
profile policy governs admission, fallback, bounded lifetime and teardown; packed work needs a
trusted operation oracle. [DMA64][hexagon-dma64] targets v81+ to avoid old-window dynamic weight
mappings via DMA/VTCM. [Q1/Q2 Hadamard matmul and row reads][hexagon-q2] are source leads; neither
proves Swift PQ2/PTQ1, full-graph or signed Windows coverage. Rebuilt source and bundled signed
binaries have distinct identities.

| Route | Required boundary |
| --- | --- |
| Prism/ggml Hexagon | Packed types, transforms, hybrid graph, adapters and signed deployment together |
| GenieX `llama_cpp` | Exact plugin/package, supported format and actual placement |
| GenieX `qairt` or ONNX Runtime GenAI/QNN | Faithful prepared package, chipset/precision/shapes/cache identity and bounded memory |
| Native Rust device boundary | Safe ownership API, audited driver ABI, no C/C++ inference-library dependency |

[GGUF/QAIRT plugins][geniex-sdk], a [Q4_0 recommendation][geniex-models] and the
[ternary HTP prototype][htp-prototype] do not establish Swift support. Supported-type fixtures or
separately pinned small models establish access only; they cannot change model selection. Concurrent
residency with another large control model exceeds the 32 GB contract. Wrapping an external engine
in Rust does not satisfy native replacement. [GenAI/QNN][qnn-genai] requires a prepared package, not
a GGUF provider switch: 27B weights at one/two bytes cost roughly 27/54 GB before
scales/state/duplicates. Expansion needs a demonstrated bounded representation.

The [GenieX v0.8.0 catalog][geniex-signed] covers only its bundled v73/v81 kernels, not newer or
rebuilt code. Custom binaries require supported signing/distribution.
[Test-signing][hexagon-windows], Secure Boot, boot/signing policy and certificate trust changes
require a separate owner decision. Missing signed deployment blocks execution.

## Packed arithmetic and graph state

The [assessment lock](../config/swift-assessment.lock.json) identifies PQ2; [current-weight PTQ1](swift-memory.md)
is separate. Fix weights, tokenizer/template, sampler, transforms, attention and declared activation/cache
precision within comparisons. PQ2 Q8_K (odd-group Q8_0), PTQ1 Q8_0 and generic upstream types have distinct
arithmetic. [SIMD contracts](cpu-kernels.md) own fixtures; pinned Prism remains the whole-model oracle until
the relevant Rust parity scope qualifies.

Q2_0 has 64 weights per scale versus PQ2's 128. Splitting blocks and duplicating scales adds about
5.9% packed storage while preserving symbols, raw scale bits and transform metadata. Equal decoded
weights do not establish activation quantization, reduction/scale order, logits or PQ2/Q8_K state
parity. [Operation fixtures](cpu-kernels.md#packed-arithmetic) cover fourth symbols, extremes/zeros, malformed/unaligned blocks,
tails, awkward dimensions, integer sums/scale order and independent PTQ1 canonicality/Q8_0. Initial
parity is bitwise; FMA/reassociation or precision changes need declared tolerances and quality
acceptance. HVX/HMX must support the arithmetic; HMX prefill needs batched weight reuse and
compatible hybrid-state scheduling.

Transform normalization/signs/axes and grouped recurrent basis are artifact identity.
[LoRA basis correctness](swift-lora.md), including grouped SSM, survives fusion; no-LoRA and approved-adapter
forwards have separate evidence. The placement ledger covers embedding, packed/floating projections,
norm/RoPE, attention, GDN/convolution, FFN/output, adapters, caches/checkpoints,
strides/shapes/precision and every prefill/decode fallback, transfer and synchronization boundary.
Contiguous regions and resident weights/scratch need explicit ownership. KV/convolution/GDN state,
checkpoints and phase layouts must preserve logits/state and deterministic streams across
fresh/reused/edited prefixes, reset, cancellation, failure and restored continuation.

[Chunkwise WY/UT GDN](inference-algorithms.md#chunkwise-gated-deltanet-prefill) has a separate schedule/float-order contract. The
[multiplication-only inverse][approximate-inverse] uses truncation/residual correction; strict
triangularity does not bound low-order error. Phone results do not qualify Windows X Elite.
[Low-bit LUT KV](inference-algorithms.md#compression-and-approximation-constraints) differs from lossless physical FP16. Requantization, training, retrieval
embeddings, drafters and speculation qualify separately.

## Memory, lifecycle and failure

Apply the [accelerator resource contract](hardware-capabilities.md#accelerator-storage-and-lifetime) to storage, asynchronous buffers, recovery and private
caches. Packed weights may stay in shared RAM; bound VTCM expansion, DMA staging, padding/alignment
and repack scratch. Persistent expansion needs its own budget. One session is the control;
partitions charge overhead and state duplication.

The [X1/v73 design][hexagon-design] describes an approximate 3.5 GB mapping window/dynamic mapping;
the selected signed X2/v81 runtime has separate DMA64/VTCM-only constraints. Address windows are not
physical capacity. Preserve the Swift 11.5 GiB service allowance and 4 GiB physical reserve,
including allocations outside Job commit; Qwen's 18 GiB allowance does not apply. Smaller placement
needs a qualified profile; refusal cannot silently shorten context or change model policy.

Retain reviewed libraries, fixed arguments, child-only environment, authentication, exact
Host/Origin/routes, canonical counts, one active conversation, caps, cancellation and deadlines.
Qualified readiness must bind effective placement/resources; the current
authenticated serving handshake does not report effective NPU placement.
Strict backend requests fail explicitly; preferred
fallback precedes generation under qualified eligibility and memory/state-transition rules.
Arbitrary inherited overrides remain unsupported. Helpers need owned lifetime, parent-death teardown
and independent resource/isolation qualification; drivers/services are not implicitly contained by
the single-process Job.

## Qualification evidence

Correctness precedes timing. Shared [qualification](hardware-capabilities.md#qualification-scope) and
[measurement rules](performance-expectations.md#2026-literature-measurement-and-comparison) own identities,
workload/state/power cases, cost accounting, acceptance and upgrades. Separate NPU RPC/submission,
synchronization, mappings and supported counters. Verify effective power modes: the [guide][hexagon-guide]
describes burst aliasing and warning fallback for unknown modes. Utilization/overlap proves no benefit.

Admitted 8K trials use 512/2K/near-limit prompts, full output reservation, fixed 128/512-token and
natural creative outputs, occupied state, cancellation and failed recovery. Larger contexts qualify
separately. Compare same-contract CPU, qualified Vulkan and NPU; retain median/p95/variance and
separate sustained runs in owning CI. Owner-device runs, including self-hosted CI,
default to five minutes total; only a predeclared justified workload may use the
smallest longer allowance, up to fifteen minutes, including setup and retention.
Sustained acceptance that cannot fit remains pending; do not schedule a
30-minute laptop run. Missing evidence or critical failures keep profiles experimental; absence
supplies no verdict. The [fixed native Rust compute probe](../tools/bonsai/src/hexagon_tools/device.rs) offers F32 and guarded-Q2 controls with
separate timed/nonblocking writes, checked queue responses and poisoned output. The
[bounded X2 capture](evidence/hexagon-x2-queue-compute.json) verified all 64 guarded integer outputs and its explicit cleanup cycle. Its
supervisor verifies exact sources, all seventeen newer runtime files, five catalog members and the
installed driver, retaining uncertain owners until the owned process exits. This qualifies fixed
access/arithmetic; Swift inference and a performance comparison remain separate. Independent hosted
ABI/build controls must pass before owner-device intake; the caller verifies trusted same-run
provenance. Native diagnostics use absolute system-temp lanes with inherited Windows system/profile
variables.

The [original-artifact projection control](../tools/hexagon/projection_probe.rs)
extends that fixed arithmetic control to independently hashed original Swift
weights/signs and an explicitly supplied activation vector. Its native Rust
submitter supports K=256/N=2 guarded limbs with up to 17,408 weight rows over
one mapping and queue. A full first projection schedules 40 original-block
operations. Every returned limb and the final ordered CPU FMA result must match
the CPU control before the result directory is published. These are implemented
bounds and comparison rules; full-M signed execution, actual model activations,
logits/state, adapters, vendor teardown and speed remain unqualified.

Hosted preparation publishes a source-bound `hexagon-projection-probe` bundle
alongside its model-free controls. The native `hexagon-tools` device supervisor
accepts `kind: projection` with an external `projectionRequestFile`; it retains
the request against mutation, verifies every runtime/catalog member and the
installed driver, clears inherited overrides, and owns the child through its
outer deadline and 75-second cleanup reserve. Device requests allow at most
120 seconds for input preparation, comparison and the native session, within
the original aggregate owner allowance. No profile or application backend is
enabled by a passing projection receipt. The [projection guide](../tools/hexagon/packed-projection.md)
defines the external input contract and remaining qualification.

## 2026 research evidence

The [mobile catalog E3–E8](research-register.md#2026-mobile-acceleration-and-speech) owns the **2026-10-29** review and counterexamples. Keep prepared/static
graphs, direct ggml HVX/HMX and signed Windows packages distinct. Attribute quantization, offload,
polling/wake, profiling and fallback against a competitive CPU; fast HMX GEMM can lose to graph
breaks. Validate static-context limits/loading order without reducing reservations. Different
packages, contexts or always-NPU reranking cannot rank same-artifact Swift or establish a GPU
ceiling. Sparse attention and fitted rotations/scales change dense/GDN computation or dynamic
Q8_K/Q8_0/lossless PTQ1 artifacts. Check foreground jank even at stable TPS; Windows arbitration
needs independent evidence and speculative yields need accepted-prefix verification/recovery.

These upstream regression leads are outside Runner's pin; moving links are not source pins:

- [DMA-ring fix #30189](https://github.com/ggml-org/llama.cpp/pull/30189), merged **2026-10-09**: descriptor-ring saturation silently dropped
  DDR-to-VTCM transfers. Check initialization, saturation, completion and reuse beyond small shapes;
  its convolution/patch-embedding failure is no proven Swift defect.
- [b11913](https://github.com/ggml-org/llama.cpp/releases/tag/b11913), prerelease **2026-10-27**: VTCM-size requests
  add no available VTCM or qualified concurrency. Bind allocation/refusal to the signed runtime; inherited
  overrides remain unsupported.

[rust-runtime]: https://github.com/symbiogenesis/prism-inference/blob/main/README.md
[geniex-sdk]: https://github.com/qualcomm/GenieX/blob/main/sdk/README.md
[geniex-models]: https://geniex.aihub.qualcomm.com/en/models/supported
[geniex-signed]: https://github.com/qualcomm/GenieX/releases/tag/v0.8.0
[hexagon-windows]: https://github.com/ggml-org/llama.cpp/blob/master/docs/backend/snapdragon/windows.md
[hexagon-guide]: https://github.com/ggml-org/llama.cpp/blob/master/docs/backend/snapdragon/README.md
[hexagon-design]: https://github.com/ggml-org/llama.cpp/blob/master/docs/backend/snapdragon/developer.md
[hexagon-dma64]: https://github.com/ggml-org/llama.cpp/pull/29197
[hexagon-q2]: https://github.com/ggml-org/llama.cpp/pull/30627
[qnn-genai]: https://github.com/microsoft/onnxruntime-genai/blob/main/docs/qnn.md
[htp-prototype]: https://github.com/aakar-mutha/llama.cpp/tree/ternary-bonsai-htp
[approximate-inverse]: https://arxiv.org/abs/2606.06034
