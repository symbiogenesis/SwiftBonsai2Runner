# Swift LoRA: artifact, basis and qualification

The native app experimentally enables the pinned OrcaRouter adapter at scale **1.3**;
`bonsai-tools chat --no-abliterate-lora` opts out. Settings accepts 0 to 2 in 0.1 steps
for the next response and every Extra effort pass, without reloading. Zero disables
correction while retaining the adapter. The broker exposes an explicit development
option; no local training is needed.

Bounded native ARM64 loading/decode passes, but **Swift basis correctness,
effectiveness, quality and standard/daily-service acceptance remain unqualified**.
No optimal scale is established. [Prism 0.1.25](native-rust-engine.md) is active;
historical results retain their original runtime identities.
[PLAN.md](../PLAN.md#creative-quality-adapters-and-memory) owns remaining work.

The [historical 0.1.13 bounded lifecycle](evidence/ptq1-0113-lifecycle-41539984210/independent-review.json)
passes on PTQ1 with Prism 0.1.13, Runner `74bef68`, and the explicitly selected
four-worker policy. Three one-token-cap streams complete at scales 1.3, zero and
1.3, with invalid-scale rejection and same-owner continuation. The final request
supplies the actual first response as assistant history, bound by captured content
and message digests. This verifies the bounded scale/history application sequence
and owned cleanup. All three streams terminate at `length`; their short visible
text does not prove complete-answer quality, loaded-zero/no-adapter logits identity,
nonzero numerical application or correct native SSM basis. That workload resets
state per request; generated text supplied as history is not prefix-state reuse.
This one attempt does not qualify other worker policies, full context/output,
performance, isolation, installation or default promotion.

## Artifact and published evidence

[swift-lora.lock.json](../config/swift-lora.lock.json) pins
`Continuum-AI-Corp/OrcaBonsai-27B-Uncensored`, revision
`703028bb7fba03496e796b144839b161cf0f5409`, path `gguf/bonsai-abliterate-lora.gguf`:
**9,682,464 bytes**, SHA-256
`f1669534803d340a496015f5c45125f3437b4d13ec764f40e34488ce83967f42`, Apache-2.0.
Downloaded size/digest are verified. The [pinned
README](https://github.com/Continuum-AI-Corp/OrcaBonsai-27B-Uncensored/blob/703028bb7fba03496e796b144839b161cf0f5409/README.md#gguf--llamacpp)
describes rank 1, alpha 1 and testing on original Ternary Bonsai **PTQ1**; it leaves
**PQ2** untested. Those tests do not establish transfer to Swift's modified weights. The
repository's `swift/` directory is Apple-language code, not evidence about the Swift
Bonsai model.

Provision or reverify the fixed artifact:

```powershell
bonsai-tools provision --json (@{ operation = 'adapter'; dataDirectory = "$env:LOCALAPPDATA/SwiftBonsai2" } | ConvertTo-Json -Compress)
bonsai-tools provision --json (@{ operation = 'adapter'; dataDirectory = "$env:LOCALAPPDATA/SwiftBonsai2"; verifyOnly = $true } | ConvertTo-Json -Compress)
```

The destination is `adapters/<pinned-source-commit>/bonsai-abliterate-lora.gguf` in the
existing external data directory. The bounded downloader verifies size/hash; it accepts
no alternative lock or URL and enables no profile.

Other candidates are not qualified alternatives:

| Candidate | Evidence boundary |
| --- | --- |
| [AtomicChat original-Bonsai LoRAs, `bfb7f8a`](https://huggingface.co/AtomicChat/Ternary-Bonsai-2-27B-Uncensored-LoRA-GGUF/blob/bfb7f8a74dd01625bf30ff63d798786fe27719b4/README.md) | Publisher reports original PTQ1/PQ2 and grouped SSM-order tests. Its own42/own38 artifacts remain unqualified on this Swift pack/host. |
| [Swift KATANA, `fe9f2da7`](https://huggingface.co/ayshinko/Swift-Bonsai-2-27B-Katana-MTP-GGUF/blob/fe9f2da74ee3d47b2455158461172c2561ef57c3/README.md#experimental-compatibility-status) | A separate model integration. Orca structural compatibility is claimed without Swift inference validation; GPU/MTP examples differ from this CPU profile. |

## Structural compatibility and unresolved basis

The verified adapter has 258 F32 tensors, `general.architecture=qwen35`,
`adapter.type=lora` and alpha 1. All 129 target names and rank-1 shapes match the actual
851-tensor Swift base. Dimensions use GGUF order, first dimension fastest:

| Target family | Sites | Base shape | LoRA A | LoRA B |
| --- | ---: | --- | --- | --- |
| `blk.N.ffn_down.weight` | 64 | `[17408,5120]` | `[17408,1]` | `[1,5120]` |
| `blk.N.attn_output.weight` | 16 | `[6144,5120]` | `[6144,1]` | `[1,5120]` |
| `blk.N.ssm_out.weight` | 48 | `[6144,5120]` | `[6144,1]` | `[1,5120]` |
| `token_embd.weight` | 1 | `[5120,248320]` | `[1,248320]` | `[1,5120]` |

The [Prism
graph](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/llama-graph.cpp)
feeds original activation `cur` to LoRA and separately transformed `cur_mm` to folded
base weights. Do not apply the base Hadamard transform to both branches. Embedding
correction follows inverse-Hadamard lookup. The [adapter
loader](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/llama-adapter.cpp)
checks shapes; the [Qwen35
graph](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/models/qwen35.cpp)
uses LoRA-aware operations for the target families.

**SSM input order remains unresolved.** Swift records
`prism.hadamard.gdn_v_grouped=true`. Its
[loader](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/llama-model.cpp)
applies a three-way tiled/grouped permutation to base-weight inputs while the adapter
receives `cur`. Orca's
[exporter](https://github.com/Continuum-AI-Corp/OrcaBonsai-27B-Uncensored/blob/703028bb7fba03496e796b144839b161cf0f5409/scripts/export_gguf_lora.py)
builds A from an unfolded checkpoint and asserts no SSM permutation is needed;
AtomicChat reports an explicit order correction. Different source-matrix orders could
explain that disagreement. Orca accepts an external checkpoint directory, and the
shipped metadata declares neither its revision nor input order.

The exporter's optional `--check` does not resolve this gate. It checks only layer
0, compares inverse-Hadamard stored columns without the native SSM input
permutation, and imposes no absolute SSM-error ceiling. An identity mapping that
scores best in that comparison is not proof of the native writer's input basis.
The producer [does not ship the checkpoint](https://github.com/Continuum-AI-Corp/OrcaBonsai-27B-Uncensored/blob/703028bb7fba03496e796b144839b161cf0f5409/README.md#L680-L683),
but describes it as an [FP16 expansion of the packed ternary representation](https://github.com/Continuum-AI-Corp/OrcaBonsai-27B-Uncensored/blob/703028bb7fba03496e796b144839b161cf0f5409/README.md#L237-L241).
That supplies a concrete independent public reconstruction route. The
[single-recipe hosted reference auditor](adapter-public-reference.md) attempts all
48 shipped SSM A vectors at the existing tight bounds, with no native permutation
or precision search. It has no successful payload result yet. Original-run
authentication still needs immutable checkpoint/conversion identities bound to
the approved adapter hash; native writer qualification needs explicit channel
mapping and independent residual evidence.

Native residual-writer evidence must establish the correction's basis on Swift;
loading or changed text cannot. Even correct input order does not establish that an
original-Bonsai correction suits Swift's modified weights. The approved file is unmodified.

Closing this gate requires the following evidence together:

- Immutable checkpoint, conversion and direction identities tied to the shipped
  adapter, with an explicit authoring-to-native SSM channel mapping.
- Independent verification of all 48 shipped SSM A vectors under that mapping,
  declared numerical bounds and native writer input/correction captures.
- Separate no-adapter/loaded-zero logits and state identity, scale-1.3 nonzero
  application and scalar/explicit-NEON qualification on the intended release.
- Retained evidence digests and independent review in the durable promotion archive.

The missing original checkpoint/conversion identity is external authentication
evidence. Public reconstruction can establish a candidate's numerical agreement;
additional native/Rust agreement cannot reconstruct original-run provenance.
Source review closes no model qualification gate.

The [2026-11-02 public-source recheck](evidence/ptq1-authoring-inputs-20261102.json)
finds the same publisher revision and no new authoring material in the inspected
repository, releases or referenced model metadata. It records the exact required
producer handoff: checkpoint shard/tensor identities, the original conversion and
toolchain records bound to the approved adapter, and all-48 reproduction/mapping
inputs for the existing intake. The selected direction's later B-factor agreement
does not supply those missing inputs. The subsequent
[source-only reassessment](evidence/ptq1-public-authoring-route.json) narrows the
earlier external-only interpretation: independent A reconstruction has a public
route even though authenticating the original authoring run remains unresolved.
The hosted reference report keeps original provenance, native writer, logits/state
and promotion flags false, including after a complete numerical match.

### Direction consistency

`bonsai-tools qualification` operation `basis-direction` independently checks all
129 shipped B factors against the negative of a selected unit direction. This
bounded check needs the approved adapter and the direction only; it can detect a
wrong direction without inventing the missing authoring checkpoint or conversion
record. The request contains absolute external `adapterPath` and `directionPath`
values and an independently selected raw-file `directionSha256`.

The direction must already contain exactly 5120 finite little-endian F32 values
(20,480 bytes), with L2 norm within 1e-5 of one. Supply the raw normalized vector,
not a safetensors container. The checker does not silently normalize it or claim
that it matches the exporter's normalization procedure. It verifies the exact
approved adapter length/hash, independently reads all 258 named F32 factors and
their expected shapes, and rejects missing, extra, overlapping or misnamed factors.
For every B channel it requires
`abs(B + direction) <= 1e-6 + 1e-6 * abs(direction)`, with the bound computed using
F64 fused multiply-add. The receipt retains the selected input digests, norm,
per-site B digest, compared element count, mismatch count and maximum difference.

`direction-consistent` establishes agreement with the selected direction only.
It always leaves authoring provenance, native writer basis and default promotion
unqualified. Authenticating the direction's source, reproducing A factors and
verifying the native SSM order still require the evidence below. Run this on
hosted CI or charge its intake and review to the same aggregate owner allowance.

### Authoring evidence intake

`bonsai-tools qualification` operation `basis-intake` accepts an external evidence
directory, an external `manifestPath` and its independently selected raw-byte
`manifestSha256`. It reads the approved 9.7 MB adapter and bounded evidence files;
it never loads a checkpoint or starts inference. Keep collection and review within
the owner's aggregate budget, or perform reproduction and intake on hosted CI.

The schema-2 manifest contains `modelSha256` for the current PTQ1 export,
`runtimeCommit` for the pinned release, `adapter`, `checkpointManifest`,
`conversionManifest`, `toolchainManifest`, `direction`, a nonempty `normalization`
description, `mapping`, `absoluteTolerance`, `relativeTolerance` and `ssmSites`.
Every artifact reference is `{ "file": "NAME", "bytes": LENGTH, "sha256": "SHA256" }`;
files must be individual ordinary files in `evidenceDirectory`. Raw-file sizes and
SHA-256 digests are checked. The checkpoint/conversion/toolchain manifests follow
the [structured authoring-record contract](adapter-authoring-intake.md). Intake
checks the actual bounded original safetensors index against every declared shard
and all 48 canonical original SSM tensor names, shapes and payload identities. It
also binds the exact conversion argv, exporter source and toolchain records to the
selected checkpoint/direction/adapter/normalization. These consistency checks do
not authenticate declarations or read checkpoint payloads. The former schema-1
opaque-manifest intake is rejected; retained historical records remain unchanged.

`mapping.nativeToAuthoring` is a permutation of all 6144 channel indices and
`mapping.signs` contains 6144 values of either -1 or 1. Its explicit equation is
`native[channel] = signs[channel] * authoring[nativeToAuthoring[channel]]`.
This intake supports signed permutations only; other transforms need a separately
reviewed comparator. `ssmSites` contains exactly the 48 SSM blocks (0 through 63,
excluding 3, 7, ... 63), each `{ "block": N, "authoringA": ARTIFACT_REFERENCE }`.
Each independently reproduced authoring-order A vector contains exactly 6144
finite little-endian F32 values. Intake independently locates the named
`blk.N.ssm_out.weight.lora_a` payload in the digest-verified adapter and compares
every channel after the declared mapping. Both tolerances are bounded by 1e-5;
the comparison is `abs(actual - expected) <= absoluteTolerance + relativeTolerance * abs(expected)`,
with the right-hand bound computed using F64 fused multiply-add.
Receipts retain each vector digest, mismatch count and maximum absolute difference.

An `intake-complete` result records byte integrity and the declared all-site vector
comparison. It always leaves `writerBasisQualified` and `defaultMayChange` false.
Independent authoring-provenance authentication and reproduction, native writer
input/correction captures, loaded-zero identity, nonzero application and the other
runtime/application gates above remain required. Repeated or invented reference
vectors cannot establish authoring provenance merely by passing this comparator.

The [independently reviewed hosted comparison](evidence/ptq1-direction-41530766802/independent-review.json)
at Runner `6c25a5d` passed all 129 sites and 660,480 channels with zero numeric
difference. Its [complete report](evidence/ptq1-direction-41530766802/direction-comparison.json)
binds the approved adapter and selected public direction; the
[file manifest](evidence/ptq1-direction-41530766802/file-manifest.json) retains the
exact extracted receipt bytes. This resolves the selected direction's consistency
with B, leaving checkpoint/conversion provenance, A reproduction, native SSM
mapping and model behavior unqualified.

The native `basis-direction` comparator remains available for the exact approved
adapter and [immutable public raw direction](../config/swift-lora.direction.json).
Retain the complete per-site report, native intake receipts, input locks and
source/tool identities when reviewing those inputs. The selected public raw vector
cannot authenticate the original authoring invocation or qualify native writer
basis and promotion. The completed hosted comparison above preserves its original
source/run identities independently of the retired one-shot workflow.

## Memory and execution cost

The process counters and GGML allocation behavior in this section describe the
historical publisher CPU checks. They do not measure the active Rust/NPU path.

The adapter contains **9,664,512 F32 payload bytes** (about 9.23 MiB on disk). Prism
retains packed base weights and adds adapter branches rather than merging a dense
replacement. Each of 128 linear sites adds two rank-1 multiplications, scale and
addition; the embedding uses lookup plus multiplication. Backend buffers, graph nodes,
intermediates and changed lifetimes add overhead beyond the file's payload.

Bounded 16-token checks pass with peak Job commit of **2,074,329,088 bytes**
with the adapter and **1,487,458,304 bytes** for the base: a diagnostic difference of
**586,870,784 bytes (559.68 MiB)**. These separate process counters exclude mapped
weights and do not isolate adapter allocation. They establish bounded loading/decode,
not reliable refusal changes, quality or performance.

The difference does not isolate persistent buffers, workspace high-water allocations
or graph lifetimes. The
[allocator](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-alloc.c)
already reuses eligible SCALE/ADD parent storage; explicit in-place calls do not
guarantee savings. F32 factor/input multiplication needs no activation-type conversion
in the [CPU
workspace](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-cpu/ggml-cpu.c).
At microbatch 64, one rank-1 projection temporary is 256 bytes and one expanded
5120-channel correction is 1.25 MiB. The embedding does not construct a dense vocabulary
correction. Do not multiply per-site buffers by 128 without proving their lifetimes
overlap.

## Allocation semantics

The following graph-reservation and B-storage-overlay discussion describes the
publisher comparison engine. Native Rust graph ownership is covered separately
in the [runtime contract](native-rust-engine.md).

Adapter activation reserves the graph again after base initialization, freeing old
compute buffers before replacement. The commit difference does not prove coexistence.
Prism Inference owns the immutable B-storage overlay,
native regression and
[builder](https://github.com/symbiogenesis/prism-inference/blob/main/scripts/Build-PrismEngine.ps1).
Its [overlay guide](https://github.com/symbiogenesis/prism-inference/blob/main/docs/prism-storage-overlay.md)
records byte-identical operations and a 2.5 MiB storage saving. The installed assessment
runtime does not automatically include the source overlay; Runner consumes reviewed pins.

All 129 B tensors are byte-identical F32 `[1,5120]`, SHA-256
`1cdf553ae2623501140ec2e52c3dfa6a80d33fcfe6f91a6d3c4dcd5f887eeca0`. The overlay shares
storage only after matching buffer type/device, shape/layout and bytes, preserving each
target's metadata and adapter lifetime. Its saving is modest and does not resolve the
larger counter difference.

Lossless allocation changes preserve public initialization, scale-zero filtering,
load-without-apply, fit/no-allocation estimates, dynamic changes, A's reduction, all F32
rounding stages and adapter order. Base-result reuse depends on last-use/view/output
constraints. Reassociation or FMA contraction can change signed zero, subnormals and
results; bit identity is part of this contract.

Quantizing factors, reducing rank/scale, pruning sites or merging into the ternary base
changes the correction or weights; these are outside lossless allocation work.

## Standard-profile qualification

Standard-profile acceptance requires **decode throughput and output quality each to
match or improve on the same model without the adapter**. Target-device evaluation,
native correctness evidence and independent human review remain pending. The fixed
scale-one/512-token harness below does not qualify app scale 1.3, broader creative
quality, comfort or daily service.

A separate opt-in [app-scale contract](../config/swift-lora.app-qualification.json)
uses **scale 1.3** with the same four prompts, 24 paired samples, four zero-scale
controls, greedy sampler and **512-token output cap**. Its distinct contract digest
binds captures, nonzero-logits proof and independent complete-answer reviews;
scale-one evidence cannot satisfy it. Select `qualificationProfile: app-scale-1.3`
in the native command, or inspect it without inference:

```powershell
& $env:BONSAI_TOOLS_PATH qualification --json (@{
    operation = 'plan'; qualificationProfile = 'app-scale-1.3'
    weightRepresentation = 'current-repacked-PTQ1_0'
} | ConvertTo-Json -Depth 64 -Compress) | ConvertFrom-Json -AsHashtable
& $env:BONSAI_TOOLS_PATH qualification --json (@{
    operation = 'file-score'; qualificationProfile = 'app-scale-1.3'
    evidencePath = 'C:/external/evidence.json'; reviewPath = 'C:/external/review.json'
} | ConvertTo-Json -Depth 64 -Compress) | ConvertFrom-Json -AsHashtable
```

`AppSettingGatesPassed` reports only these matched LoRA gates.
`StandardProfileMayEnable` stays false for this separate contract. The 512-token cap
does not prove answer completeness: every exact response still requires human review.
Thinking, sampler changes, full 4K output, long context, broad creative quality,
comfort and daily service remain separate qualification. Omitting `qualificationProfile`
preserves the original scale-one contract.

### Fixed acceptance contract

[swift-lora.qualification.json](../config/swift-lora.qualification.json) pins four
general/creative prompts, three repetitions, alternating baseline/adapter order, fresh processes,
seed 42, greedy sampling, a 512-token cap, 8K FP16 context, 128/64 batch/microbatch,
four CPU threads and exact artifact identities.

Every prompt must independently pass all three requirements:

| Measure | Requirement |
| --- | --- |
| Decode throughput | Median ratio **≥ 1.0** against no LoRA. |
| Human-reviewed quality | Every applicable dimension **≥ 3/4**, with median no lower than baseline; any unwanted adapter refusal rejects quality. |
| Memory | Median peak Job-commit premium **≤ 768 MiB**, within the unchanged **11.5 GiB** Job cap and **4 GiB** available-physical-memory floor. |

Reduced refusals, shorter answers and memory savings cannot offset either primary
failure. Job commit excludes mapped weights; the one-second physical watchdog is not a
continuous peak-RAM measurement. The memory premium is a policy allowance, not measured
adapter allocation.

The standard scorer and [PTQ1 prefix screen](swift-memory.md#bounded-decision-screen-interface)
validate the Job cap against compiled Swift process policy. Contract-hash checks
reject captures under the obsolete 12 GiB scoring allowance; recapture rather than
rewrite evidence or review hashes.

### Capture on the target device

The native planning command downloads no model weights and schedules no
owner-device capture. PQ2 is the default. The exact current-weight PTQ1 export is
a separate fixed comparison because its activation quantization differs.

Inspect the plan locally without model loading or benchmarking:

```powershell
& $env:BONSAI_TOOLS_PATH qualification --json (@{
    operation = 'plan'; weightRepresentation = 'PQ2_0'
} | ConvertTo-Json -Depth 64 -Compress) | ConvertFrom-Json -AsHashtable
```

Native `qualification` operation `run` constructs and verifies the complete capture
envelope from `dataDirectory`, optional `useRepackedPtq1` and `qualificationProfile`.
It requires credential-free, main-branch hosted CI on an actual Snapdragon X/X2 Elite.
Both the front door and low-level capture reject owner/self-hosted execution.
The 24 paired samples plus four loaded-at-zero controls have no demonstrated complete
five-minute bound. Capture remains pending a suitable hosted target; do not split
the matrix across repeated owner jobs.

| Capture component | Contract |
| --- | --- |
| Integrity and lifetime | Verify the complete publisher package/model/adapter, retain write/delete-denying handles and copy the verified adapter into a unique external temp lane. Every process enters a one-process kill-on-close Windows Job at creation, with a three-minute worker deadline and bounded diagnostics. All owner-device setup, samples, comparison and cleanup must fit five minutes total. |
| Receipts | Rust parses timings, strictly decodes UTF-8 and hashes exact responses. Each sample has `capture.json`, raw stdout and `diagnostics.json`; completed supervision is written last. Rust assembles paired evidence and retains preparation failures. Upload controls, stdout/logs/arguments/counters/failures; exclude model/adapter files. |
| Scale | The pinned [`--lora-scaled` parser](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/common/arg.cpp) splits every colon in `FNAME:SCALE`; relative filenames avoid drive-colon ambiguity. [Zero scale](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/llama-context.cpp) loads the adapter without its active graph branch. |
| Throughput | Native single-token **eval-run count / eval time**, from the pinned [stderr timing printer](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/common/sampling.cpp), invariant decimals, missing/duplicate/invalid counters rejected. `decodeTokens` counts eval runs: final sampled tokens normally are not decoded, and a one-token prompt remainder may count. [Completion diagnostics](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/tools/completion/completion.cpp) cross-check sampler accepts minus verbose-prompt tokens, including EOG; visible text is not a token counter. |
| Limits | Exact stdout preserves reasoning, EOG and trailing newlines; EOG is not an authoritative finish reason. Wall time includes startup; first-token latency, power and thermals are absent. This small greedy corpus proves neither broad creative quality nor statistical equivalence. |

Use the [complete-task measurement protocol](performance-expectations.md#2026-literature-measurement-and-comparison)
for broader useful-completion comparisons under equal budgets.

### Independent review and correctness evidence

`evidence.json` binds the LF-normalized UTF-8 contract and matched controls by hash. The
capture reports SSM basis verification, zero-scale logits and nonzero logits application
as pending: completion exposes no logits or generated token IDs. Native writer-level SSM
basis, loaded-at-zero response/logits identity and nonzero application are prerequisites
for interpreting effects; response changes or successful exits cannot satisfy them.
Capture identity binds the exact model, adapter, runtime, template, sampler, context,
batching, threads and seed. Failed generations and individual regressions remain
visible.

After external human review, score the capture without loading a model:

```powershell
& $env:BONSAI_TOOLS_PATH qualification --json (@{
    operation = 'file-score'; qualificationProfile = 'standard-scale-one'
    evidencePath = 'C:/external/evidence.json'; reviewPath = 'C:/external/review.json'
} | ConvertTo-Json -Depth 64 -Compress) | ConvertFrom-Json -AsHashtable
```

Supply a genuine review of every sample in this form:

```json
{
  "schemaVersion": 1,
  "source": "external-human-review",
  "attestation": "I reviewed the exact captured responses without automated scoring.",
  "runId": "COPY_FROM_CAPTURE",
  "contractSha256": "COPY_FROM_CAPTURE",
  "reviews": [{
    "sampleId": "s0001",
    "responseSha256": "COPY_EXACT_RESPONSE_HASH",
    "reviewerId": "REVIEWER",
    "humanReviewed": true,
    "reviewedAtUtc": "2026-10-23T18:00:00Z",
    "answerComplete": true,
    "refused": false,
    "scores": { "instructionFollowing": 3, "correctness": 3, "coherence": 3 },
    "rationale": "Replace with a specific assessment of this exact response."
  }]
}
```

Creative prompts also require `creativeUsefulness` under the fixed 0-4 rubric.
Broader fiction/art acceptance separately covers dialogue, continuity, tone,
composition, sensitive permitted themes, coherence, instruction following, refusal,
repetition and usable completions. Example scores illustrate the schema, not actual
reviews; one response cannot settle the scale.
The scorer rejects malformed/hash-mismatched reviews but cannot authenticate reviewers
or attestations. Missing reviews/native evidence stay pending; known regressions reject.

`StandardProfileMayEnable` becomes true only when all implemented mandatory gates pass.
The scorer enables no serving profile, and daily security and broader creative/comfort
gates remain separate.
