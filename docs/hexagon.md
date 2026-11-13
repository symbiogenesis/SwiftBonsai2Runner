# Hexagon deployment and execution contracts

Runner pins [Prism 0.1.27](../config/prism-inference.lock.json), source
`1429a33772089dfd3a50a96c698b9dbe0b36836a`. On Windows ARM64 Snapdragon X2 (`v81`),
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
with interactive/battery objectives remain separate work in [PLAN.md](../PLAN.md#hexagon-npu).
`native-q2` uses
bounded floating-point tolerance; it does not promise the bitwise PQ2/Q8_K
parity of the independent block-preserving controls. Authenticated serving readiness does not report
effective NPU placement.
Multi-token prefill packets batch independent `N = 1` operations; a true
`N > 1` matrix solver remains separate work.

## Resident packets and prefill

The native-Q2 executor submits the existing gate/up, SwiGLU, optional supported
Hadamard, down and optional residual chain in one correlated packet. Operation/tensor
pairs are 4/12, 5/14, 5/15 and 6/17. Borrowed and owned sessions retain checked handles,
dimensions, cache flushes and exact response correlation. Adapters disable this fused
chain; arbitrary signs and grouped rotations are unsupported. The optional down
Hadamard requires every authored sign to be positive one.

Two complete independent token chains can share one packet, with odd tails using
the single-token path. Queue writes become `ceil(tokens / 2)`; every logical operation,
lifetime attempt and conservative memory charge still applies. Multi-tile packets
likewise batch independent single-token projections against one resident weight tile.
The signed solver remains `HVX_QUANT_ROW_N1`; batching does not implement an `N > 1`
matrix solver or qualify new arithmetic.

Tile-major preparation packs each weight tile once across token rows and reuses one
partial-output buffer. Each token keeps its original row/column accumulation order.
Inputs validate before submission; provisional completion follows the declared
submission order. Correlated completion and validation of every output precede
publication. Unsupported shapes reject before submission. Failure faults the executor,
clears outputs and retains uncertain owners through terminal completion or supervised
exit, without replay.

Source work counts establish no measured latency, memory or energy saving. Complete
model state/logit/adapter and cancellation acceptance, ARM64 code generation, signed
vendor lifetimes/resources and matched complete-request benefit remain independent
qualification gates under [PLAN.md](../PLAN.md#hexagon-npu).

## Device and route boundaries

Acceptance requires completed-response latency or energy benefit within memory, quality and
sustained-comfort
limits against the same Swift artifact on CPU and qualified Adreno. TOPS, utilization and
simultaneous
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

The [GenieX v0.10.0 catalog][geniex-signed] covers only its bundled kernels, not newer or
rebuilt code. Custom binaries require supported signing/distribution.
[Test-signing][hexagon-windows], Secure Boot, boot/signing policy and certificate trust changes
require a separate owner decision. Missing signed deployment blocks execution.

## Packed arithmetic and graph state

The [assessment lock](../config/swift-assessment.lock.json) identifies PQ2;
[current-weight PTQ1](swift-memory.md)
is separate. Fix weights, tokenizer/template, sampler, transforms, attention and declared
activation/cache
precision within comparisons. PQ2 Q8_K (odd-group Q8_0), PTQ1 Q8_0 and generic upstream types have
distinct
arithmetic. [SIMD contracts](cpu-kernels.md) own fixtures; pinned Prism remains the whole-model
oracle until
the relevant Rust parity scope qualifies.

Q2_0 has 64 weights per scale versus PQ2's 128. Splitting blocks and duplicating scales adds about
5.9% packed storage while preserving symbols, raw scale bits and transform metadata. Equal decoded
weights do not establish activation quantization, reduction/scale order, logits or PQ2/Q8_K state
parity. [Operation fixtures](cpu-kernels.md#packed-arithmetic) cover fourth symbols, extremes/zeros,
malformed/unaligned blocks,
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

Apply the [accelerator resource contract](hardware-capabilities.md#accelerator-storage-and-lifetime)
to storage, asynchronous buffers, recovery and private
caches. Packed weights may stay in shared RAM; bound VTCM expansion, DMA staging, padding/alignment
and repack scratch. Persistent expansion needs its own budget. One session is the control;
partitions charge overhead and state duplication.

The [X1/v73 design][hexagon-design] describes an approximate 3.5 GB mapping window/dynamic mapping;
the selected signed X2/v81 runtime has separate DMA64/VTCM-only constraints. Address windows are not
physical capacity. Preserve the selected Swift service allowance and 4 GiB physical reserve,
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

Correctness precedes timing. Shared [qualification](hardware-capabilities.md#qualification-scope)
and
[measurement rules](performance-expectations.md#measurement-and-comparison) own identities,
workload/state/power cases, cost accounting, acceptance and upgrades. Separate NPU RPC/submission,
synchronization, mappings and supported counters. Verify effective power modes: the
[guide][hexagon-guide]
describes burst aliasing and warning fallback for unknown modes. Utilization/overlap proves no
benefit.

Admitted 8K trials use 512/2K/near-limit prompts, full output reservation, fixed 128/512-token and
natural creative outputs, occupied state, cancellation and failed recovery. Larger contexts qualify
separately. Compare same-contract CPU, qualified Vulkan and NPU; retain median/p95/variance and
separate sustained runs in owning CI. Owner-device runs, including self-hosted CI,
default to five minutes total; only a predeclared justified workload may use the
smallest longer allowance, up to fifteen minutes, including setup and retention.
Sustained acceptance that cannot fit remains pending; do not schedule a
30-minute laptop run. Missing evidence or critical failures keep profiles experimental; absence
supplies no verdict. The
[fixed native Rust compute probe](../tools/bonsai/src/hexagon_tools/device.rs) offers F32 and
guarded-Q2 controls with
separate timed/nonblocking writes, checked queue responses and poisoned output. The
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
enabled by a passing projection receipt. The
[projection guide](../tools/hexagon/packed-projection.md)
defines the external input contract and remaining qualification.

[rust-runtime]: https://github.com/symbiogenesis/prism-inference/blob/main/README.md
[geniex-sdk]: https://github.com/qualcomm/GenieX/blob/main/sdk/README.md
[geniex-models]: https://geniex.aihub.qualcomm.com/en/models/supported
[geniex-signed]: https://github.com/qualcomm/GenieX/releases/tag/v0.10.0
[hexagon-windows]: https://github.com/ggml-org/llama.cpp/blob/master/docs/backend/snapdragon/windows.md
[hexagon-guide]: https://github.com/ggml-org/llama.cpp/blob/master/docs/backend/snapdragon/README.md
[hexagon-design]: https://github.com/ggml-org/llama.cpp/blob/master/docs/backend/snapdragon/developer.md
[hexagon-dma64]: https://github.com/ggml-org/llama.cpp/pull/29197
[hexagon-q2]: https://github.com/ggml-org/llama.cpp/pull/30627
[qnn-genai]: https://github.com/microsoft/onnxruntime-genai/blob/main/docs/qnn.md
[htp-prototype]: https://github.com/aakar-mutha/llama.cpp/tree/ternary-bonsai-htp
[approximate-inverse]: https://arxiv.org/abs/2606.06034
