# Runtime conformance contract

[Prism Inference](https://github.com/symbiogenesis/prism-inference) owns runtime
algorithms, backend/source catalogs, codecs, native overlays and source tests.
Upstream acceptance is defined by its [conformance
guide](https://github.com/symbiogenesis/prism-inference/blob/main/docs/prism-upstream-tests.md),
[71-source catalog](https://github.com/symbiogenesis/prism-inference/blob/main/tools/prism/upstream/catalog.json),
[migration inventory](https://github.com/symbiogenesis/prism-inference/blob/main/tools/prism/upstream/migration.json),
[main-only workflow](https://github.com/symbiogenesis/prism-inference/blob/main/.github/workflows/runtime-conformance.yml)
and
[focused diagnostics](https://github.com/symbiogenesis/prism-inference/blob/main/.github/workflows/prism-focused.yml)
.
This page defines Runner's boundary; [PLAN.md](../PLAN.md#deferred-prism-migration)
owns deferred migration.

Runner's [Foundation](../.github/workflows/foundation.yml) covers the app, broker,
process ownership, provisioning and releases. It neither builds the upstream catalog
nor runs runtime algorithm/codec suites. Full suites and benchmarks are CI-only in
the owning repository. Runner targets Windows ARM64 on Snapdragon X and X2,
with Apple excluded. Full device profiles and family-specific performance claims
need matching-family evidence; shared CPU source promotion follows the narrower
[cross-generation rule](cpu-kernels.md#cross-generation-cpu-promotion).
Hosted ARM64 results do not establish Snapdragon performance.
The APIs and model families below are upstream conformance obligations, not
Runner broker features. Runner retains its
[fixed local API subset](broker-policy.md#public-request-contract)
and [pinned Swift runtime boundary](native-rust-engine.md).

## Source fidelity and evidence

Safe Rust implementations and thin adapters preserve imported inputs, assertions,
tolerances, schedules and stopping rules. Constructors, source-specific expected output
and focused tests cannot substitute for full source execution. Production bridges stay
in place; original C++ tests remain retired. The [reuse review](#source-reuse-review)
records extraction sources and supported subsets.

| Acceptance dimension | Required scope |
| --- | --- |
| Source inventory | Exactly 70 Windows-applicable source results per target, with the single Apple-only exclusion explicit in the complete 71-source inventory and passing native reference results |
| Backend correctness | All backend execution partitions and all 22,817 ordered geometries in both evaluation and support modes; original source IDs, random offsets, input domains, tolerances and result hashes |
| Backend performance | All 1,530 workloads, separate from correctness; unsupported operations are explicit and cannot produce fabricated timings |
| Handoff | Compact plan/execution handoff plus separate complete diagnostics; artifact extraction limits cannot drop source results |
| Baseline promotion | Complete reviewed hosted evidence, unchanged required green IDs and exact tested main revision, hardware, run identity, report hashes and actual verdict |
| Device conclusions | Actual target hardware identity; hosted ARM64 evidence alone supplies no X Elite speed, memory, thermal or quality verdict |

The reference includes the approved, hash-verified DSpark saver and Llama 3 production
decoding corrections; it is not an unmodified upstream build. Immutable model/vocabulary
inputs, synthetic DSpark references, the explicit invalid-UTF-8 skip and Falcon
diagnostic exclusion remain part of the contract.

Evaluation is the original default source mode; default acceptance also requires support
preflight. Gradient mode is a separate explicit diagnostic. Pinned upstream aborts
during GEGLU backward graph construction; no invented gradient skip or claim of complete
gradient success is permitted. The pinned public `FLASH_ATTN_BACK` constructor also
aborts: dormant CPU code does not establish a passing public reference. Unsupported
derivatives remain explicit errors. The `FLASH_ATTN_BACK` abort is at `ggml.c:5545`;
dormant `ops.cpp:9516/9520` also uses query-head counts for key/value gradient batch
strides.

## Numerical operations and codec contracts

Backend comparison preserves actual shape, stride, aliasing, arithmetic and allocation
behavior. Relevant operation identities include F32/F16/BF16 `ADD1` (4), `IM2COL_BACK`
(53), `POOL_2D_BACK` (62), `FLASH_ATTN_BACK` (75), `WIN_PART`/`WIN_UNPART` (78/79),
`GET_REL_POS`/`ADD_REL_POS` (80/81) and custom/map callbacks (92–95). Packed `ADD1`
supports 20 encodable formats, F32 scalars, padded rows and one fourth-axis slab. `Q8_1`
has no native decoder; `Q8_K` lacks native operation dispatch, so both reject. The slab
restriction excludes pinned `ops.cpp`'s `i3*nb0` destination-offset defect.
Two-/three-dimensional views derive unused strides from supplied row/plane strides.
Backward expansion is transactional, accumulators are owned and gradient snapshots/reset
preserve state. Cross-entropy normalization covers every trailing batch axis.

Packed format identities include types 16 `IQ2_XXS`, 17 `IQ2_XS`, 18 `IQ3_XXS`, 19
`IQ1_S`, 21 `IQ3_S`, 22 `IQ2_S`, 29 `IQ1_M`, 41 `Q1_0` and 42 `Q2_0`. Checked layouts,
arbitrary-byte decoding, native packed-activation dots, importance encoding and graph
routing are distinct contracts. Native CPU encoder availability stays explicit. IQ4_NL's
native zero-scale reference path packs uninitialized payload nibbles; Rust canonicalizes
unused nibbles while preserving numeric zero. Undefined native bytes have no byte-parity
contract, and native stack addresses are not portable data.

`test-quantize-fns.cpp`, `test-quantize-perf.cpp` and `test-quantize-stats.cpp` require
the owned `qb-*` transport/model-tensor APIs, all 144 pinned trait slots and all 22
required eligible type identities. Buffers have checked byte regions, stale-handle
rejection, overlapping-call rules and unchanged output on failure. Native capabilities
and implemented Rust operations are distinct; an unported CPU quantizer cannot fall back
to the reference. Scalar input order, block boundaries, alignment offsets, native dot
operand layouts, per-call schedules, statistical accumulation and original source
thresholds remain exact. Statistics consume the actual pinned model's F32/F16 tensors.
Timing uses real clocks for supported counters, with unavailable operations explicit.
Entire performance sources and timing paths execute only in CI.

## Chat, parser and tokenizer contracts

`test-chat-analysis.cpp`, `test-chat-auto-parser.cpp` and `test-chat.cpp` cover
production direct apply/statistics, differential template analysis, format detection,
generated parsers and reasoning/tool mapping. Server conversion preserves
tool-call/content-part JSON, delimiter splitting, streaming diffs, grammar validation,
reasoning budgets, template kwargs, Responses conversion, developer roles and
model-specific thinking/tool-order workarounds. Byte-regex captures/offsets and
malformed-UTF-8 retry/partial-input behavior are distinct from Unicode-regex splitting.
Jinja, PEG and typed message APIs cannot be replaced with source-specific expected text.

| Source | Exact acceptance scope |
| --- | --- |
| Auto-parser | 513 native assertions plus 11 normalization conditions |
| Chat analysis | 37 diagnostic templates, 19 variable probes and zero original expected-output assertions; supplied-time direct apply and undefined-variable statistics |
| General chat | All 14 groups, 431 parser cases, 363 fixed checks and every original byte-prefix parse/diff/reconstruction |
| Capability probes | All six probes, including object/string argument fallback and usage after exceptions |
| Tokenizers | All original decoder/loader failure cases and every source; file-wrapper/generator behavior, randomized HF references, immutable external corpora, complete SPM/BPE token and Unicode roundtrips and the original ignore-merges branch |

For Swift serving, model/tokenizer identity binds the exact special-token vocabulary.
Compare token IDs, spellings, template behavior and control-token rejection against an
independent reference; two observations derived from the same tokenizer are not
independent evidence. Qualify concatenated content parts, forged turn/role markers,
literal lookalikes and malformed input without altering legitimate user text.
The broker's supported subset remains separate from broader upstream tokenizer APIs.

Apertus, Mistral-Small-3.2, Firefunction-v2, IBM Granite 3.3, Llama 3.2 and GPT-OSS have
explicit capability-execution gaps. A reduced template sample or string-content probe
does not establish complete chat-analysis equivalence.

## Acquisition, model and session contracts

The full CLI contract covers production registration, negation and environment handling
across all 27 example contexts, actual parameter mutation, numeric/CSV validation,
speculative output-limit conversion, inventory uniqueness and original invalid-option
cases. Model-resolution handlers alone are narrower than the production CLI.

Retrieval preserves supplied headers, timeouts and body bounds, including original
200/404/oversize/cancellation behavior. Content-Length/chunked framing, curl capture,
live HTTPS, redirects, actual URL/multi-shard downloads, refs/tree listing retrieval,
offline finalization, recursive cached snapshots and metadata-inferred draft resolution
have separate input domains. Rust regex syntax does not imply native ECMAScript
quant-tag pattern compatibility; unsupported syntax is explicit.

Generic model APIs include generated-tensor/progress callbacks, streaming GGUF saving,
encoder/decoder contexts, vocabulary/logit ownership, graph reservation/export,
sequence/KV state, independent checkpoints, file/buffer save/restore and persistent
concurrent session transport with callback errors. Decoded nonfinite payloads reject
even when stored codec scales are finite. Qwen35 runtime or vocabulary-only sessions
alone do not satisfy generic model contracts.

`test-llama-archs.cpp` defines 149 names/151 variants, of which 128 are
source-supported. Unsupported variants keep their original distinctions. Its contract
includes generated tensor callbacks, hparam/tensor shape validation, context creation,
encode for T5/DREAM/LLADA/LLADA_MOE/RND1, original 128-token decode/logit checks,
CPU/backend NMSE, graph export, saver/file-pointer reload and exact restored logits. A
copied architecture-name table is insufficient.

Model/progress/state/concurrency contracts in `test-autorelease.cpp`,
`test-model-load-cancel.cpp`, `test-save-load-state.cpp`,
`test-state-restore-fragmented.cpp` and `test-thread-safety.cpp` retain their unchanged
stories15M assets and complete schedules. Real model behavior is required for
`test-recurrent-state-rollback.cpp`, `test-backend-sampler.cpp`,
`test-export-graph-ops.cpp` and `test-kv-mean-center.cpp`; standalone snapshots, graph
allocators or greedy helpers do not satisfy these sources.

## DSpark, adapters, grammar and ABI

The four DSpark sources require production metadata/log-SNR, forward/loop/speculative
state and real-eval APIs, using original synthetic references and the complete 24-prompt
AR/verification schedule. Both approved upstream production corrections remain applied.
LoRA's original source contract includes actual conversion, tensor merge/application and
inference; file conversion alone is insufficient.

`test-grammar-llguidance.cpp` uses the actual LLGuidance/model-session grammar contract;
forcing the existing GBNF converter is not equivalent. The public C header/ABI needed by
`test-c.c` retains safe Rust boundaries. That source is compile-only and contains zero
behavioral assertions; successful compilation cannot be reported as behavioral
qualification.

## Source reuse review

Source reuse preserves pinned contracts/licenses and adds no framework dependency.
Upstream comments do not authorize script execution.

Camelid attribution is retained in [NOTICE](../NOTICE) and its complete [MIT
license](https://github.com/symbiogenesis/prism-inference/blob/main/CAMELID-LICENSE.txt).
Existing ggml attribution covers direct Prism adaptations.

The bounded Q1_0/Q2_0 implementation supplies checked storage, reference encoding,
arbitrary-byte decoding, native Q8_0 dots, prepared matvec and graph routing. Its
[native
fixture](https://github.com/symbiogenesis/prism-inference/blob/main/tests/fixtures/upstream-signquants.json)
binds independent byte/value/dot oracles; production admission stays Qwen35-only. The IQ
formats listed above similarly retain independent decoded-float and native Q8_K
packed-dot oracles; float GEMV cannot substitute for packed dots. Decoder/dot support
does not establish reference or importance encoding.

F32/F16/BF16 window, relative-position, im2col and pooling operations retain padded
rows, both patch axes, channels/batches, strided gradients, overlapping pooling,
first-maximum ties, nonfinite comparisons, native accumulation, raw F16/BF16 copying and
in-place aliases. Window/relative-position extraction requires contiguous sources.
Custom callbacks, complete backward graphs and broader session/architecture APIs remain
outside this implemented subset.

Implemented retrieval enforces Content-Length, complete chunk termination and trailers,
unambiguous framing, decoded-body and wire-size caps. Bounded curl children terminate
and are reaped on failure/overflow; invalid transfers cannot publish blobs/snapshots.
Optional chunk-extension syntax is not fully validated. These checks do not establish
broader CLI/download parity, full source/backend qualification or performance.
