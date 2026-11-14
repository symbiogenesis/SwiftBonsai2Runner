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
parity of the independent block-preserving controls. The published runtime's
authenticated serving readiness does not report effective NPU placement.
Reviewed local Prism source adds a model-thread initialization handshake and a
versioned `startup_projection` observation. Runner accepts that optional object
only with consistent requested/effective placement, the fixed
168-tensor/2,888,663,040-byte resident layout and bounded host metadata; CPU and
external observations cannot assert resident weights. Missing observations remain
compatible with the published 0.1.27 pin. This closes no device, performance or
release gate; requiring the new object awaits a verified source-bound repin.
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

### Resident identity and bounded lookup

The next Prism source candidate keeps resident projection records append-only
and uses a separately reserved sorted index for lookup. Cache identity includes
immutable shared storage and matrix geometry; presentation names remain outside
cache identity. Distinct matrix views sharing bytes cannot hide one another,
and inserting a lower search key cannot invalidate saved projection indices.
Native-Q2 repeated sibling references reuse one prepared weight extent while
every submitted operation still consumes its own capacity. The HMX cache uses
the same bounded lookup design instead of a linear scan.

All vector capacities, including the new indirection, count against the existing
metadata reservation. Fewer entries may fit; a full index admits no further
record, preserving the existing executor admission and fallback rules.
Selected tensor admission remains
name-sensitive and distinguishes shared-storage views independently of caching.
Model-free identity, stable-handle, shared-view, duplicate-sibling and capacity
controls do not qualify signed execution, device lifetime or request benefit.
Native/fallback compiled controls, production linking, full-model state/adapters,
cancellation/resources and source-bound device acceptance remain required.
The [local source-check limit](cpu-kernels.md#current-source-comparison-admission)
also applies to this delivery: supervisor setup failed before compiled controls.

Guarded/model-probe whole-request reservation and explicit partial placement
already exist in source. Their terminal 24,000-attempt policy remains separate
from default native-Q2 persistent serving, which selects a distinct session
lifetime policy. Reset cannot replenish the guarded cap. Neither those source
controls nor this cache change qualifies persistent service admission.

## Device and route boundaries

X1/v73 and X2/v81 qualify independently. Receipts bind actual SKU/capabilities,
package/DSP/catalog, installed driver, host executable, ABI and resource limits.
Unknown or changed identities exclude the candidate. Source availability cannot
replace Windows ABI, supported signing or distribution. The
[GenieX v0.10.0 catalog][geniex-signed] covers only bundled kernels. Trust,
certificate, Secure Boot or test-signing changes require a separate owner decision;
missing signed deployment blocks execution.

| Route | Required boundary |
| --- | --- |
| Prism/ggml Hexagon | Packed formats, transforms, hybrid graph, adapters and signed deployment |
| GenieX `llama_cpp` | Exact plugin/package, supported format and measured placement |
| GenieX `qairt` or ONNX Runtime GenAI/QNN | Faithful prepared package with chipset/precision/shapes/cache identity and bounded memory |
| Native Rust | Audited safe driver/ownership boundary without a C/C++ inference library |

[Plugins][geniex-sdk], [format recommendations][geniex-models] and the
[ternary HTP prototype][htp-prototype] are leads, not Swift support.
[QNN][qnn-genai] requires a prepared package; expanding 27B weights to one/two
bytes costs roughly 27/54 GB before scales, state or duplicates. Small-model
access controls cannot change model selection, and another large resident model
exceeds the 32 GB contract.

[DMA64][hexagon-dma64] targets v81+ mapping limits; [Q1/Q2 Hadamard support][hexagon-q2]
does not establish Swift PQ2/PTQ1, full-graph or signed Windows coverage. Preserve
independent X1 mapping/HVX and X2 DMA/VTCM/HMX evidence. TOPS and utilization do
not qualify latency, energy or complete-request benefit.

## Packed arithmetic and graph state

The [assessment lock](../config/swift-assessment.lock.json) identifies original
PQ2; [current-weight PTQ1](swift-memory.md) is separate. Fix artifact, tokenizer/
template, sampler, transforms and activation/cache precision in comparisons.
[CPU arithmetic](cpu-kernels.md#packed-arithmetic) owns independent fixtures:
PQ2 Q8_K/odd-group Q8_0, PTQ1 Q8_0 and generic upstream types differ.

Splitting 128-weight PQ2 into two 64-weight Q2 blocks preserves symbols and raw
scale bits while adding 5.88235% storage. It does not preserve activation
quantization, floating reduction order or logits/state by itself. The exact
HMX bridge and tolerance-based `native-q2` path have separate numerical contracts;
see [source/ABI boundaries](hexagon-source-audit.md) and the
[packed operation control](../tools/hexagon/packed-projection.md).

A placement ledger owns every graph operation, adapter, cache/checkpoint,
shape/stride/precision and prefill/decode fallback, transfer and synchronization.
Transform signs, normalization, axes and grouped recurrent basis are artifact
identity. [LoRA basis acceptance](swift-lora.md) survives fusion. Preserve complete
attention/convolution/GDN state, checkpoints and deterministic continuation across
prefix reuse/edits, reset, cancellation and failure. Projection completion cannot
commit a token. [Chunkwise GDN](inference-algorithms.md#chunkwise-gated-deltanet-prefill),
cache approximation and speculation retain separate algorithm gates.

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

Follow [profile qualification](hardware-capabilities.md#qualification-scope),
[measurement rules](performance-expectations.md#measurement-and-comparison) and
[owner-device budgets](../AGENTS.md#owner-device-execution-budgets). Compare the
same artifact on CPU, qualified Adreno and NPU, charging RPC/submission, waits,
mappings and complete setup/teardown. Verify effective power modes; the
[Hexagon guide][hexagon-guide] documents burst aliases and warning fallbacks.

Admitted 8K coverage includes 512/2K/near-limit prompts, full output reservation,
128/512-token and natural creative outputs, occupied state, cancellation and
failed recovery. Larger contexts qualify separately. Retain median/p95/variance;
sustained acceptance remains pending when it cannot fit the laptop allowance.

The [fixed compute supervisor](../tools/bonsai/src/hexagon_tools/device.rs)
checks F32/guarded-Q2 access under source-bound runtime, catalog and installed-driver
identities. Independent hosted ABI/build controls precede device intake; callers
verify same-run provenance. Native diagnostics use absolute system-temp lanes
and inherited Windows system/profile variables.

The [artifact-bound projection diagnostic](../tools/hexagon/packed-projection.md#artifact-bound-signed-projection-diagnostic)
extends this to hashed Swift weights/signs and an explicit activation vector.
It owns operation/input bounds, per-limb and ordered-FMA checks, runtime/catalog
verification, native lifetimes and cleanup deadlines. A passing receipt does not
qualify actual model activations, full state/adapters, serving isolation or speed.
The [model probe](../tools/hexagon/model-probe.md) separately defines bounded
CPU-shadow graph comparisons. Remaining acceptance belongs in
[PLAN.md](../PLAN.md#hexagon-npu).
[rust-runtime]: https://github.com/symbiogenesis/prism-inference/blob/main/README.md
[geniex-sdk]: https://github.com/qualcomm/GenieX/blob/main/sdk/README.md
[geniex-models]: https://geniex.aihub.qualcomm.com/en/models/supported
[geniex-signed]: https://github.com/qualcomm/GenieX/releases/tag/v0.10.0
[hexagon-guide]: https://github.com/ggml-org/llama.cpp/blob/master/docs/backend/snapdragon/README.md
[hexagon-design]: https://github.com/ggml-org/llama.cpp/blob/master/docs/backend/snapdragon/developer.md
[hexagon-dma64]: https://github.com/ggml-org/llama.cpp/pull/29197
[hexagon-q2]: https://github.com/ggml-org/llama.cpp/pull/30627
[qnn-genai]: https://github.com/microsoft/onnxruntime-genai/blob/main/docs/qnn.md
[htp-prototype]: https://github.com/aakar-mutha/llama.cpp/tree/ternary-bonsai-htp
