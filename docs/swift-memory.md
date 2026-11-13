# Swift artifacts, memory and PTQ1

**Original PQ2 on native Rust Prism 0.1.27 is the development default**, with four
CPU row workers, standard adapter scale 1.3 and [X2 NPU placement](hexagon.md) when initialization
succeeds. `--publisher-engine` explicitly selects rollback; PTQ1 remains opt-in.
Creative quality, performance and daily-service isolation remain unqualified.
The [Qwen reference](model-selection.md) has a separate
lock/profile; no local training or fine-tuning is planned.

This guide covers artifacts, resource policy, lossless packing/allocation and
separately labelled approximation research. [PLAN.md](../PLAN.md) owns qualification.

## Pinned artifacts

[swift-assessment.lock.json](../config/swift-assessment.lock.json) pins
`Swift-Bonsai-2-PQ2_0.gguf` at revision `a3bdac086bbb6b04d87d0108d18943b5f63868f7`:
**7,206,168,928 bytes (6.71 GiB)**. The merged-weight release is distinct from the
earlier PTQ checkpoint and about 5.53 GiB smaller than Qwen; file size is not peak RAM
or speed. Sources: [publisher](https://ukisai.com/swift-bonsai-2) and [immutable
metadata](https://huggingface.co/api/models/ukisai/Swift-Bonsai-2-GGUF/revision/a3bdac086bbb6b04d87d0108d18943b5f63868f7?blobs=true).

The publisher ARM64 package is Prism `prism-b10787-73bfe6c`, commit
`73bfe6cc485554822f80c473a3c0c4f25e80f41b`: packed NEON, transforms and graph support
for PQ2/PTQ, which stock upstream binaries lack. Full builds require Clang; the upstream
MSVC builder needs changes. Integrity/architecture checks do not attest source-to-binary
correspondence or model correctness. See [backend
support](https://github.com/PrismML-Eng/Bonsai-demo/blob/main/BACKEND-SUPPORT.md).

## Actual artifact census

The size/SHA-256-verified artifact's [metadata](../config/swift.metadata.json) records 851 tensors across PQ2/F32/BF16, 64
main blocks, no MTP block, and tokenizer/template identities. The streamed
[census](../config/swift.pq2-census.json) used a 64 KiB payload buffer and found 402 PQ2
tensors, 209,920,000 blocks and 26,869,760,000 encoded symbols:

| Symbol | Count |
| --- | ---: |
| −1 | 9,042,814,485 |
| 0 | 8,802,947,576 (32.76154%) |
| +1 | 9,023,997,939 |
| +2 | 0 |

Every scale is finite and nonzero. All 402 tensors qualify for lossless three-symbol
repacking under the census rules; no floating-point weight conversion is involved.

## Current-weight PTQ1 option

Prism **PTQ1_0** losslessly repacks these weights; it is distinct from the
weight-changing PTQ1.61 research algorithm and older published PTQ model.

The verified output is **5,946,648,928 bytes**, saving **1,259,520,000 bytes (1.173019409
GiB)**. Its [receipt](../config/swift.ptq1-export.json) records SHA-256
`e658f0f0736a03a4549535a439f9b07254ebf8ab88ae27cbe6e52d098ca5d1f5`, 209,920,000 verified
blocks and 258,720 working-buffer bytes. Native pinned PQ2/PTQ dequantizers also agree
bit-for-bit on 853,760 synthetic values.

PTQ1 uses Q8_0 activations; PQ2 uses Q8_K. Equal weights do not prove equal logits,
text or speed: see [numerical evidence](#numerical-and-admission-boundaries).
The [packing decision](#packing-decision) excludes BITCOS.

Either packing can use the experimental [LoRA](swift-lora.md) at app scale 1.3;
its behavior, runtime cost and standard-profile qualification are independent.

## Release decision

PTQ1 remains an experimental opt-in. Exact reconstructed weights establish
lossless packing, not identical activation arithmetic, answers or performance.
Promotion needs current-source application and adapter-basis acceptance, full
context/output and recovery checks, independent quality review, eligible measured
benefit, resource/comfort acceptance and original-PQ2 rollback. All nineteen
PTQ1 promotion gates must pass for one frozen candidate; missing evidence leaves
promotion pending. [PLAN.md](../PLAN.md) owns that work.

## Exact PTQ1 packing

PTQ1 stores each 128-symbol group in **28 bytes**, versus PQ2's **34**: 24 five-trit
bytes, two four-trit bytes and the unchanged two scale bytes. The pinned codec uses
ceiling-scaled base-3 codes, 16-lane then 8-lane order, and a canonical zero unused tail
trit. Conventional group-256 TQ1 cannot retain both original group-128 scales.

Independent 128-trit blocks require `ceil(128 log2(3)) = 203` bits, or 26 whole
code bytes: PTQ1 meets this bound before its scale. [Cross-block
compression](inference-algorithms.md#compression-and-approximation-constraints),
[KV precision](quantized-kv.md) and checkpoint storage have separate contracts.

The [native
layout](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-common.h)
and
[codec](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-quants.c)
define the representation. The [Rust ternary
crate](https://github.com/symbiogenesis/prism-inference/blob/main/tools/ternary/README.md)
and [GGUF
exporter](https://github.com/symbiogenesis/prism-inference/blob/main/tools/gguf/README.md)
preserve symbols, raw scale bits, other tensor bytes and tokenizer/template/transform
metadata without floating-point quantization. Bounded caller-owned buffers and
batchwise byte-identical PQ2 reconstruction checks avoid a complete expanded model.

Runner also provides the independent Rust verifier through
`bonsai-tools ptq1-packing`. It binds both complete artifact hashes and the compiled export
receipt, compares every original symbol and raw scale with a separately derived
decoder, and checks preserved metadata and non-PQ tensor bytes. Header reads,
payload buffers and execution time are bounded. Its synthetic controls cover all
wire bytes in every lane and malformed tensor extents.

All converted tensor block counts are divisible by sixteen, retaining 32-byte alignment.
Non-tensor bytes occupy the 11,120,992-byte header/data-offset region; there are no
source inter-tensor or trailing gaps. Retain the original source on disk, but avoid
holding complete old/new buffers simultaneously.

The [ARM dot
kernel](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-cpu/arch/arm/quants.c)
decodes PTQ in place without retaining an expanded model. [CPU type
traits](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-cpu/ggml-cpu.c)
select the distinct activation routes documented in [numerical boundaries](#numerical-and-admission-boundaries).
Preserving PQ2's Q8_K route would require a separately qualified PTQ1×Q8_K kernel.

## Packing decision

Current-weight PTQ1 is the supported lossless packing option.
[BITCOS](https://arxiv.org/abs/2609.16338v1) (2026-09-14 preprint) is not supported.
Swift's 32.76154% zero-symbol rate falls below BITCOS's 37.5% ideal same-scale PTQ1
break-even before offsets, padding or indexes. Its proposed layout would occupy
6,158,339,606 bytes before container/index overhead, **211,690,678 bytes more** than the
verified 5,946,648,928-byte PTQ1 export.

This artifact's serialized-size disadvantage alone excludes the proposed layout;
it does not establish a universal BITCOS limit or a device-performance verdict.

## Supported selection and resource policy

`bonsai-tools chat --use-repacked-ptq1` selects only this verified artifact with the
default native Rust engine. `--publisher-engine` selects the explicit publisher CPU
rollback; direct app/broker launches also accept `--use-repacked-ptq1`. The artifact
handle stays verified/held through owned teardown. Arbitrary paths, checkpoint
substitutions and missing-file fallback are unsupported. Service implementation
alone does not establish release qualification. Native chat defaults
to four row workers, with explicit one-worker selection. Publisher rollback keeps
its separate fixed four-thread CPU policy. Separate source-checker evidence does not qualify app TPS.
The default NPU placement selects original PQ2 projections, not PTQ1 tensors.

Both packings retain the [CPU resource policy](#memory-selected-serving-context), context, retry
and recovery. Serialized savings alone do not lower admission or promote PTQ1.

## Reproduce the assessment

Use native ARM64 PowerShell 7.6, the existing owner-selected external data directory and
a new external report path. Select a trusted prebuilt maintenance executable from the
evaluated main revision, replacing the example tool path below. These are explicit
publisher CPU assessment commands; they do not change the native Swift chat default.
Provisioning, census, export and each model check are separate substantial operations:
select and predeclare only work that fits one aggregate owner allowance. The blocks
below are a command reference, not a single admitted laptop workload:

```powershell
$maintenanceExecutable = 'C:\BonsaiTools\bonsai-tools.exe'
$data = Join-Path $env:LOCALAPPDATA 'SwiftBonsai2'
$lockPath = Join-Path $PWD 'config/swift-assessment.lock.json'
$lock = Get-Content -Raw -LiteralPath $lockPath | ConvertFrom-Json -DateKind String
& $maintenanceExecutable provision --json (@{ operation = 'model'; dataDirectory = $data; lockPath = $lockPath } | ConvertTo-Json -Compress)
& $maintenanceExecutable provision --json (@{ operation = 'engine'; dataDirectory = $data; lockPath = $lockPath } | ConvertTo-Json -Compress)
& $maintenanceExecutable provision --json (@{ operation = 'inference'; dataDirectory = $data } | ConvertTo-Json -Compress)
$model = Join-Path $data -ChildPath 'models', $lock.model.revision, $lock.model.filename
$request = Join-Path $data 'swift-census-request.json'
@{ operation = 'inspect'; modelPath = $model; expectedBytes = $lock.model.bytes; expectedSha256 = $lock.model.sha256;
   dialect = 'Prism-73bfe6cc485554822f80c473a3c0c4f25e80f41b'; censusPq2 = $true;
   outputPath = 'D:\Reports\new-swift-census.json'; dataDirectory = $data } |
    ConvertTo-Json | Set-Content -LiteralPath $request -Encoding utf8NoBOM
& $maintenanceExecutable gguf --request $request
& $maintenanceExecutable cpu-check --json (@{ dataDirectory = $data; profile = 'swift-pq2-assessment'; checkOnly = $true } | ConvertTo-Json -Depth 64 -Compress) | ConvertFrom-Json -AsHashtable
& $maintenanceExecutable cpu-check --json (@{ dataDirectory = $data; profile = 'swift-pq2-assessment' } | ConvertTo-Json -Depth 64 -Compress) | ConvertFrom-Json -AsHashtable
```

Optional exact export and adapter assessment:

```powershell
$derived = Join-Path ([IO.Path]::GetDirectoryName($model)) 'Swift-Bonsai-2-current-repacked-PTQ1_0.gguf'
$request = Join-Path $data 'swift-export-request.json'
@{ operation = 'export-ptq1'; modelPath = $model; outputPath = $derived; dataDirectory = $data; timeoutSeconds = 240 } |
    ConvertTo-Json | Set-Content -LiteralPath $request -Encoding utf8NoBOM
& $maintenanceExecutable gguf --request $request
& $maintenanceExecutable cpu-check --json (@{ dataDirectory = $data; profile = 'swift-pq2-assessment'; useRepackedPtq1 = $true } | ConvertTo-Json -Depth 64 -Compress) | ConvertFrom-Json -AsHashtable
& $maintenanceExecutable provision --json (@{ operation = 'adapter'; dataDirectory = $data } | ConvertTo-Json -Compress)
& $maintenanceExecutable cpu-check --json (@{ dataDirectory = $data; profile = 'swift-pq2-assessment'; useRepackedPtq1 = $true; useAbliterateLora = $true } | ConvertTo-Json -Depth 64 -Compress) | ConvertFrom-Json -AsHashtable
```

Inspection/export use a trusted prebuilt `bonsai-tools.exe` selected as
`$maintenanceExecutable`, invoke the pinned GGUF CLI and release buffers before
inference. `ggufToolPath` selects an explicit trusted GGUF tool; otherwise Rust
verifies the installed inference release in `dataDirectory`. No tool is built or
downloaded by these commands. The complete-process deadline includes hashing;
leave the gate pending if setup, work and cleanup cannot fit the owner budget.

## Memory-selected serving context

Native Prism 0.1.27 and publisher Swift serving select context once per model
launch from available physical memory. Retry takes a new observation; an active
model keeps its context
so counting, generation, readiness and the native UI use the same capacity.
The reviewed native pin activates the paired context/memory protocol and
selected-cache accounting. Serving capacity is separate from historical fixed-8K
qualification workloads; those evidence identities remain unchanged.

The pure planner in `bonsai-cpu-policy::context` retains the existing 4 GiB
Windows reserve and non-context allowance from the 11.5 GiB/8K reference budget.
Publisher sizing charges 137,344 bytes per token: two 65,536-byte FP16 KV copies
for live and rollback state, 6,144 bytes for the publisher's 64-token/24-head
score scratch, and 128 bytes for context metadata. The reviewed native profile
instead charges the immutable selected cache and sequential score scratch:

| Sizing profile | One KV copy, bytes/token | Process growth, bytes/token | State ceiling, tokens |
| --- | ---: | ---: | ---: |
| Publisher FP16 | 65,536 | 137,344 | 62,720 |
| Native FP16 recovery | 65,536 | 131,296 | 62,720 |
| Reviewed full-context K4/V4 (Prism 0.1.27) | 16,896 | 34,016 | 262,144 (separate state allowance) |
| Native K4/V3 capacity candidate | 14,848 | 29,920 | 262,144 (model limit) |

Native process growth retains two complete caches, 96 bytes per token for
24 F32 score rows and 128 bytes for metadata. K4/V4 records include their
four-byte scales; no FP16 history mirror is assumed. The metadata allowance
covers the 64-token cache page tables and headers, token histories and tails;
256-token selection alignment spans complete cache pages. Codec/codebook storage,
prepared rows and fixed checkpoint overhead stay in the fixed allowance and the
runtime's minimum 1 GiB state/checkpoint allowance. A divergent request can retain
the previous cache, so immutable page sharing does not reduce the two-cache charge.
Explicit tiled prefill and speculation retain separate additive reservations.
The non-context allowance remains 11 GiB, the reference
Job budget minus its original 512 MiB KV charge; it includes weights, adapters,
recurrent state, transforms and runtime headroom. These are conservative source
allowances, not measured peaks. Native Prism 0.1.27 retains this cache-aware
sizing through its exact reviewed release pin; this establishes no measured
memory savings. PTQ1 receives no reduction in the fixed allowance.

Capacity rounds down to 256-token pages and must accommodate the 4,096-token
reply cap plus the 64-token template reserve. It never exceeds Swift's declared
262,144-token model context or the selected live-state bound after charging
156,893,184 recurrent bytes plus single-copy native KV, score and metadata
charges. FP16's 4 GiB state ceiling permits 62,720 tokens. The reviewed Swift
K4/V4 scope permits 262,144 tokens
through its geometry-derived state allowance. The separate K4/V3 candidate fits
the same model ceiling within 4 GiB, but its serving gate remains closed.
At 20 GiB available before launch, publisher sizing selects 37,120 tokens and
the native K4/V4 profile selects 149,760; K4/V3 selects 170,240.
At 24 GiB available, the reviewed K4/V4 profile reaches the model ceiling.
These are calculated admission
bounds, not measured usable capacities or long-range quality results.
Less available RAM selects a smaller window and more can
select a larger one, while insufficient space for a usable window rejects launch.
The larger charge for rollback and batch scratch can require more available RAM
than the historical 15.5 GiB admission floor.
Sizing also leaves a separate 256 MiB startup margin so process creation need
not consume the last page of admission slack. The immediate pre-spawn and child
checks still require the Job allowance plus the 4 GiB host reserve.
The Job ceiling and launch admission follow the selected budget. Unknown or
insufficient memory rejects launch, and running pressure still tears down the
owned model. An available-memory snapshot reserves no RAM.

Publisher serving passes the selected capacity through `--ctx-size`. Native
**Prism 0.1.27**, source `1429a33772089dfd3a50a96c698b9dbe0b36836a`, uses paired
`--context-tokens` and `--memory-limit-bytes` child arguments with the reviewed
selected-cache/sequential-scratch accounting. Runner binds capability activation
to this exact release identity; unrelated versions and an older FP16-only memory
protocol cannot receive the smaller K4/V4 allowance. The binary archive and files
must still match their compiled lock; a version string alone enables no protocol.

Runner prepares explicit `--kv-cache rsq4k3v` selection through trusted source,
installed and deferred launches, including Retry. A separate exact-source release
gate keeps it unavailable with the current 0.1.27 pin. Future activation requires
review of both the K4/V3 numerical identity and the complete M1/M2 accounting
contract; neither a version string nor a raw CLI option establishes support.
The K4/V4 default and explicit FP16 recovery remain separate selections.

The reviewed Prism 0.1.27 full-context K4/V4 profile preserves that cache precision and
the 34,016-byte process charge. Its separate state allowance is 4,644,798,464
bytes: 156,893,184 recurrent bytes plus 262,144 times 17,120 bytes for cache,
sequential scores and metadata. This is 333.625 MiB above the generic 4 GiB bound;
generic graph constructors, FP16 and individual allocation/scratch limits retain
their existing ceilings. The larger scope requires the exact verified Swift
model identity and geometry, including all 16 attention and 48 recurrent layers.
With unchanged two-cache, fixed, host and startup charges, the planner projects
262,144 tokens at 24 GiB available before launch. Runner's separate exact-source
gate binds this allowance to the same reviewed release as memory sizing, with
matching graph, live-state and checkpoint guards. Full-state/resource and
long-range quality qualification remain pending. Arithmetic alone establishes
no usable capacity or latency.

The capacity-first alternative is available as trusted `--placement cpu` (JSON
`Placement: "cpu"`, PowerShell `-Placement cpu`). It survives source, installed
and deferred setup and model Retry. Its exact-source release gate selects the
reviewed 0.1.27 runtime, which skips accelerator initialization and reports both
`placement: "cpu"` and
`projection_backend: "cpu"` at readiness. Omission or explicit `auto` keeps the
existing automatic placement. Explicit placement is native-only, and requests
cannot change it. The fixed 11 GiB allowance, two-cache charge and host/startup
reserves stay unchanged until measurements establish distinct usable headroom.
Omitting the roughly 2.69 GiB NPU-resident payload is a capacity investigation,
not a measured RAM saving or a latency result.

Release activation must review cache selection, default four-worker support,
latest-prefix reuse and memory-context accounting together. The exact reviewed
memory release is also recognized by those native capability checks; unrelated
future versions remain rejected. K4/V3 retains its separate closed release gate;
CPU placement is enabled only for the reviewed 0.1.27 source. Qualify each selected
combination independently; publication does not transfer historical model evidence.

For memory-sized serving, closing memory-heavy applications before a fresh launch
or Retry can leave more RAM for the selected context. This cannot enlarge an
already running model. Historical fixed-8K releases keep their original capacity.
Keep completed work before
restarting; the planner retains the same host and startup reserves.

The broker publishes the actual capacity to its owned UI. Until it is known,
the Context control shows a pending capacity; afterward its estimates, meter and
warnings use that capacity. Exact admission still counts the canonical rendered
prompt and reserves the requested reply plus 64 tokens. It never trims history,
changes reply settings or replays a request. Larger windows and changing-memory
startup need independent source-bound application/resource qualification; the
historical 8K captures establish no larger-context result.
The 1 MiB serialized chat-request bound also remains effective. A token
capacity alone does not guarantee that every Unicode or JSON-escaped history fits
the transport; larger-profile acceptance must check representative complete
requests against both token and byte limits.
The native draft path accepts up to 1,048,576 Unicode scalar characters, with
whole-edit overflow rejection and complete rejected-draft restoration. Draft
capacity is independent of request eligibility: serialized history, escaping,
reply reservation and exact rendered token counts still apply. Recovery retains
its separate 8 MiB encoded-workspace bound and reports a failure rather than
silently shortening a draft. Increasing transport requires a coordinated Prism
server, message, tokenizer and worker-protocol change and a reviewed release;
changing the desktop HTTP ceiling alone cannot deliver that capability.

## Source text in messages

Paste source text directly into the message, add any title or version useful to
the request, and review it before Send. Pasted text becomes ordinary draft and
conversation content under the existing recovery preference. The composer
rejects oversized insertions as a whole; the broker still enforces exact token
counts, output reservation, input-marker policy and serialized request bounds.

## Numerical and admission boundaries

PQ2 and PTQ1 use different activation routes. Full-model comparisons must bind
the exact model, adapter, reference, graph arithmetic, cache policy and
scalar/NEON backend. Compare every logit and complete live KV, convolution and
recurrent state under the declared tolerance; matching token IDs cannot override
a logits/state failure. The FP16 reference bound is
`1e-3 + 1e-3 * abs(reference)`; [K4/V4](quantized-kv.md) requires a separate
approximate-cache quality and resource packet.

Service acceptance also requires loaded-zero/no-adapter identity, nonzero adapter
application, full-context trajectories, complete answers, cancellation/recovery
and same-release original-PQ2 rollback. Narrow numerical checks cannot supply
those independent gates. The [runtime contract](native-rust-engine.md) defines the
provisioned release and supported subset.

File savings do not reduce admission or the Job ceiling by themselves. Keep
mapped residency, private commit and device allocations separate and preserve
the running physical reserve. PTQ1 currently receives no discount in the fixed
memory allowance; [memory-selected serving](#memory-selected-serving-context)
charges the chosen context and cache independently.

## Bounded decision-screen interface

Native `qualification` operation `screen-plan` inspects the fixed contract without
model execution. Capture remains pending: the fourteen-process matrix has no
demonstrated complete five-minute owner bound, and native capture rejects
self-hosted/owner execution.
Native `qualification` operation `screen-run` prepares verified external artifacts
from `dataDirectory` and requires a credential-free hosted ARM64 Snapdragon target.
Its fixed workload is one creative prompt, both
packings, adapter off/on at 1.3 and three interleaved repetitions: **12 timed 64-token
prefixes plus two zero-scale controls**. All benchmark execution is CI-only.
The native capture rejects Actions credentials in the evaluation process tree. Before
model admission or artifact hashing, the verified completion executable must pass
a retained, model-free `--version` preflight under the ordinary headless worker
policy: ten seconds, 512 MiB Job, unchanged admission and physical reserve.
Failure stops the screen and records no PTQ1 suitability verdict; the pinned
publisher runtime's known X2 DLL startup failure is tracked under
[headless startup](cpu-kernels.md#headless-startup-prerequisite).

Inspect its plan without loading a model:

```powershell
& $env:BONSAI_TOOLS_PATH qualification --json (@{ operation = 'screen-plan' } | ConvertTo-Json -Depth 64 -Compress) | ConvertFrom-Json -AsHashtable
```

Inspect a capture offline:

```powershell
& $env:BONSAI_TOOLS_PATH qualification --json (@{
    operation = 'screen-file-score'; evidencePath = 'C:/external/screen-evidence.json'; reviewPath = $null
} | ConvertTo-Json -Depth 64 -Compress) | ConvertFrom-Json -AsHashtable
```

The screen verifies artifacts/resources, retains failures and compares median decode
throughput. Schema 2 retains native CPU identity, topology and power observations
before/after all fourteen processes and compares adjacent boundaries. Changed or
unknown conditions make timing **inconclusive**, requiring a new capture; legacy
schema-1 captures remain inspectable with the same timing limitation. Boundary
observations do not establish steady clocks, thermal stability or an idle machine.
Rejection blocks the candidate only for the declared screen; continue means only this
prefix screen did not reject it. Neither verdict qualifies completed answers, long
contexts, statistical equivalence, total physical RAM, defaults or the
scale-one/512-token LoRA contract.

## Allocation audit

This audit targets Prism `73bfe6cc485554822f80c473a3c0c4f25e80f41b`.
Storage reductions are not measured RAM/throughput gains. Lossless changes preserve
weights, KV precision, checkpoints, adapters and the [resource policy](#memory-selected-serving-context).

### Native allocation candidates

The newer local Rust allocation work removes redundant storage without changing
the selected model weights, cache precision, floating-point operations or worker
count. It requires a reviewed, published runtime bundle before Runner can consume
it; the installed 0.1.27 bundle does not contain these changes.

| Candidate | Source-derived storage reduction | Remaining boundary |
| --- | ---: | --- |
| Retain serialized A only for embedding lookup; retain decoded A for linear adapter sites | 6,029,312 bytes (5.75 MiB) for the approved adapter | Preserve factor validation, embedding row access and every linear reduction/scale/add stage. |
| Preserve the loader's exact B-factor sharing in decoded immutable storage | 2,621,440 bytes (2.5 MiB) for the approved adapter | Share only matching kind/layout and the same immutable source allocation; preserve signed zero and dynamic scales. |
| Reuse packed-attention score scratch across serial heads within each worker chunk | At four workers and 24 query heads: 640 KiB at 8K, 5 MiB at 64K, 20 MiB at 262,144 occupied tokens | Keep the existing head partition, per-head arithmetic and joined failure handling; cache precision and conservative admission charges stay unchanged. |

The adapter reductions total 8.25 MiB of payload. Combined with the four-worker
score reduction, the calculated payload reduction is 8.875 MiB at 8K or
28.25 MiB at the model's full window. These are allocation calculations, not
measured whole-process peaks or established performance results. Actual retained
score capacity follows the largest evaluated history and allocation growth.
Source controls and complete-model/resource/performance acceptance remain
separate gates; unchanged arithmetic does not itself prove unchanged latency.

Existing checkpoints already share sealed KV pages, omit canonical-zero
recurrent payloads, reuse live recurrent buffers during restore and share retained
logits. Those implemented savings cannot be counted again. Sharing mutable tails
or recurrent state would require independent rollback storage or copying during
generation, so no further saving is credited to it.

The larger exact-storage opportunity remains PTQ1's verified 1.173 GiB weight
packing reduction. Current PTQ1 serving uses Q8_0 activations rather than PQ2's
Q8_K contract. The standalone `dot_ptq1_q8k` helper is not a serving path and
does not preserve the pinned PQ2 fused reduction. A suitable integrated execution
path, independent full-state/adapter controls and matched performance evidence
are required before treating that packing as a no-loss, no-performance-cost
runtime saving. Neither the lossy K4/V4-versus-FP16 comparison nor omission of
NPU payloads supplies that guarantee; their separate gates remain in PLAN.

### Mapping and startup

Runner's shared Windows artifact verifier streams complete SHA-256 checks through
[Windows CNG](../src/windows-credentials/src/hash.rs), avoiding the pinned
RustCrypto ARM64 detector's software fallback on Windows. Verification still reads
every byte, checks the exact length and retains the same read-only file guards;
it does not trust timestamps or a cached model digest. This also applies to runtime,
adapter and speech artifact verification. The pinned Rust inference runtime also
uses Windows CNG through its `native-hash` helper for its own complete model hash
before loading. That independent pass remains; CNG dispatch does not eliminate
verification or establish a measured startup gain.

The [CPU check](../tools/bonsai/src/cpu_check.rs) selects mmap, no repack, no warmup, CPU
buffers and 128/64 batch/microbatch. The
[loader](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/llama-model-loader.cpp)
can point supported CPU tensors into the read-only mapping; other buffer types may copy
them. Load receipts determine which allocations count as savings. Hashing/census/export
use bounded buffers rather than whole-file arrays or model-wide float conversion and
precede inference.

The pinned [Windows mapping
code](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/llama-mmap.cpp)
prefetches the complete mapping and cannot unmap fragments. Windows
[prefetch](https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-prefetchvirtualmemory)
populates the physical cache, not the process working set. Disabling it could avoid
eager startup caching, but does not reduce tensor bytes or prove sustained savings once
inference touches the weights. No current CLI switch exposes it.

### Graph and output buffers

The [context
allocator](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/llama-context.cpp)
reserves prompt/decode graphs and grows the persistent output buffer to the largest
requested count. With 248,320 vocabulary entries, one F32 logits row is 993,280 bytes;
reducing a retained 64-row buffer to one row would save **62,576,640 bytes**. Completion
normally samples one row and may already retain only that row; graph logits and
persistent host output have distinct lifetimes.
Single-row output, optional embeddings, capture layers, MTP and backend sampling each
have separate allocation contracts.

Microbatch 32 preserves context length but changes execution grouping, so the 64-token
profile's numerical/device evidence does not transfer to it. One F32 17,408-wide
activation occupies 4,456,448 bytes at 64 tokens and 2,228,224 at 32. These are
component sizes; do not multiply them by all layers because the planner reuses storage.
The [CPU
backend](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-cpu/ggml-cpu.cpp)
retains a maximum-sized work buffer. It has no current post-prefill trimming contract;
releasing it could cause later allocations.

### Other allocation bounds

The [Hadamard
implementation](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/llama-model.cpp)
shares one 1024² F32 rotation per CPU buffer type and signs by width/type:
4,423,680 retained rotation/sign/metadata bytes (including 114,688 sign-metadata bytes)
and a temporary 4 MiB initialization vector. Charging per transformed weight would
double-count shared storage. A butterfly rewrite changes floating-point order.

Original F32/BF16 weights are retained: 10.09 MiB F32 and 45 MiB BF16, totalling
57,767,936 bytes. Narrowing all F32 to sixteen bits could save at most 5,291,008 bytes
and would require exactness proofs. BF16 already has a [native dot
route](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-cpu/ggml-cpu.c);
there is no demonstrated complete F32-expanded copy to remove. FP16 can lose BF16 range.
The 8K FP16 KV/recurrent estimates remain 536,870,912 and 156,893,184 bytes; lowering
context or precision changes the contract.

[Matching thread
settings](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/common/common.cpp)
avoid a separate batch pool. Four threads mean three workers plus the caller in the
non-OpenMP path. Windows [stack
reservation](https://learn.microsoft.com/en-us/windows/win32/procthread/thread-stack-size)
is not committed/resident memory. PE headers and actual stack depth determine whether
stack settings represent a material allocation bound.

The [pinned Rust runtime's atomic checkpoint restore](native-rust-engine.md#swift-cpu-supported-subset)
stages fallible KV copies and reuses live recurrent/convolution storage, avoiding
149.625 MiB of transient Swift payload. This is neither measured peak RAM nor a
steady-state saving.

## Approximation boundary

KV/recurrent quantization, low-rank caches, approximate attention and weight
codebooks change numerical or state identity. They remain separate from lossless
PTQ1 packing and allocation reuse. Retain complete live/rollback state, scales,
rotations, reconstruction scratch and any high-precision copies in accounting.
Compression ratios alone do not establish usable memory or complete-request benefit.

The [quantized-cache contract](quantized-kv.md) owns the current K4/V4 development
selection and explicit FP16 recovery. [Inference constraints](inference-algorithms.md#compression-and-approximation-constraints)
own arithmetic and error boundaries; the [research catalog](research-register.md#research-catalog)
holds optional alternatives. Only a demonstrated bottleneck justifies another
codec or precision experiment, with its own exact-source, quality and resource gates.
