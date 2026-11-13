# Native Rust Prism engine contract

[Prism Inference](https://github.com/symbiogenesis/prism-inference) owns the Rust
CPU runtime, tokenizer, packed kernels and hybrid graph. Runner owns the native
app, broker, provisioning and release integration. The
[compiled release lock](../config/prism-inference.lock.json) is authoritative for
the runtime version, source, archive and installed file identities.

The development default is Prism **0.1.27**, original Swift PQ2, four CPU row
workers, K4/V4 cache and the standard adapter at scale 1.3. Supported X2 workers
attempt [resident Hexagon `native-q2`](hexagon.md), retaining CPU execution when
accelerator initialization is unavailable. Trusted `--placement cpu` explicitly
selects CPU execution. These placement choices preserve the engine and weights;
`--publisher-engine` separately selects the pinned publisher rollback. Failures
never silently switch engines, weights or cache precision.

Development defaults do not establish full-context, creative-quality,
performance, daily-isolation or clean-machine acceptance. Those gates remain in
[PLAN.md](../PLAN.md) and the [promotion contract](#default-runtime-promotion).
PTQ1 remains opt-in. The [native Rust UI](ui-architecture.md) is a broker client;
UI availability does not qualify inference.

## Swift CPU supported subset

| Area | Contract |
| --- | --- |
| Platform | Native Windows ARM64 on Snapdragon X and X2; other device profiles require independent acceptance |
| Model | Exact original PQ2 from the [assessment lock](../config/swift-assessment.lock.json), or explicitly selected [current-weight PTQ1 export](../config/swift.ptq1-export.json); length, SHA-256 and metadata are mandatory |
| Weights | Dense `qwen35`; [851 tensors](../config/swift.metadata.json): 353 F32, 96 BF16 and 402 PQ2_0 or PTQ1_0 |
| Execution | Scalar or eligible NEON CPU kernels, with the explicit [X2 placement subset](hexagon.md); unsupported operations reject |
| State | One active conversation; 16 full-attention and 48 recurrent blocks; [K4/V4](quantized-kv.md) by default, explicit FP16 recovery, F32 recurrent/convolution state |
| Capacity | Context selected once at launch from available RAM under [cache-aware memory accounting](swift-memory.md#memory-selected-serving-context); readiness, counting and generation share that capacity |
| Input/output | Verified embedded template, exact prompt tokens, supplied reasoning history, declared stops and incremental text |
| Generation | [Inference defaults](inference-defaults.md), optional Thinking and production MT19937 draw ordering/native sampler ties; total completion cap 4,096 tokens |
| Lifecycle | Verified readiness, canonical count/generation requests, one active generation, authentication, streaming, cancellation, deadlines, reset and recovery |
| Admission | Selected context and Job budget, 4 GiB running physical reserve and startup margin; the fixed 8K/11.5 GiB Job/15.5 GiB admission profile remains a separate qualification control |

The verified runtime source is
`1429a33772089dfd3a50a96c698b9dbe0b36836a`. Runtime and exact-source numerical
capture locks advance together. A version string alone grants no capability;
archive and installed file digests must match their compiled locks. Publishing
the public runtime crate does not publish the private server or replace Runner's
binary lock.

The service supports the selected PQ2/PTQ1 profile with or without the exact
approved rank-one adapter. Canonical count/generation requests bind its single
adapter slot and finite scale in `[0, 2]`. The default policy resets graph state
at the sealed scale before each request. Explicit [latest-prefix reuse](#latest-prefix-reuse)
can retain a complete committed prefix; scale changes invalidate its provenance.
Successful requests preserve sampler progression; explicit reset reseeds it.

Private readiness must identify the instantiated row-worker budget and every
selected capability. Missing, duplicate, coerced, unsupported or mismatched
fields reject startup. Worker counts identify a budget, not actual utilization
or device benefit. Capability depends on the compiled source/file identity guard.

Typed context rejection invalidates stale counts while preserving same-owner
recovery. Startup validates immutable payloads before graph publication, without
rescanning them during shape binding. PTQ1 keeps extent/finite-scale checks
without decoding and discarding valid trits. Windows hashing uses CNG through
the retained verified file handle.

The FP16 numerical reference requires logits and complete logical KV,
recurrent and convolution state within `1e-3 + 1e-3 * abs(reference)` against
pinned native execution. This does not qualify K4/V4's different numerical and
state identity; it needs its [independent packet](quantized-kv.md#first-practical-promotion-packet).
Exact prompt/tokenizer and sampler fixtures belong to Prism. Runner's serving
subset excludes speculation and client-selected cache, worker or backend overrides.

The opt-in [owned request-attribution mode](validation.md#bounded-source-request-attribution)
uses the pinned checker from a hosted source-bound package, with retained
parent/Job ownership, admission, deadline and cleanup reserve. Its one-token
public request supplies attribution only, not released-service or performance acceptance.

## Numerical and replacement boundaries

Preserve model/adapter identities, decoded scales, activation/cache precision,
transforms, attention topology, sampler and ordinary decoding. PQ2, verified
current-weight PTQ1 and Qwen are separate controls; lossless weights do not imply
identical activation arithmetic. Generic PQ2's fourth symbol remains valid despite
Swift's ternary census.

Lossless layout changes, SIMD, tiling, buffer reuse, fusion and scheduling use the same
arithmetic contract. Reassociation and altered reductions/FMA need declared numerical
tolerances. Precision reduction, approximate attention, pruning, retraining and
speculation are separate numerical experiments. The [algorithm
reference](inference-algorithms.md) defines packed-lookup, attention-rounding, chunkwise
recurrence and state-recovery constraints.

A CPU replacement owns Rust model/LoRA storage, has no C/C++ inference-library dependency
and confines device/OS calls to audited safe boundaries; ggml pointer bindings do not
satisfy it. Source reuse needs immutable revisions, licenses/notices, hardware assumptions
and correctness evidence.

Prefill, decode and completed-task
time are separate measurements, with cold load/repacking, allocation/copying,
synchronization, memory traffic and the measured critical path included. [Orangu's
published results](https://mnemosyne-systems.github.io/orangu/doc/server.html#scope) on
CIX P1/Mali use [different activation/repacking
choices](https://mnemosyne-systems.github.io/orangu/doc/server-internals.html);
[Camelid's results](https://github.com/timtoole02/Camelid/blob/main/docs/BONSAI2.md)
include speculation. Neither establishes unchanged-arithmetic Snapdragon gains.

## Token and worker API semantics

These generic token-driver APIs are separate from the pinned Swift chat session and
native sampler:

| Interface | Supported behavior |
| --- | --- |
| Generation stops | Verified EOS/EOT/EOM plus ordered caller stops, at most 256 distinct IDs; vocabulary/BOS/padding/insertion declarations are checked. Stops are neither emitted nor committed to history. |
| Raw IDs | Exact order and duplicates, with complete context/output reservation; no text parsing or implicit BOS/EOS insertion. Strict JSON files/stdin have independent byte/token bounds. |
| Text input | Verified `gpt2`/`qwen35` vocabulary, ranked leftmost-tie BPE with exact byte fallback and bounded heap work, pinned Qwen35 Unicode boundaries; no normalization or dropped bytes and no invented boundaries for oversized pieces. |
| Special input | Explicit literal/parse policy; control/user-defined spellings use bounded earliest-start/longest-match scanning, ambiguous spellings reject, recognition inserts no BOS/EOS. |
| Insertion | `--insert-special none\|model`, default `none`; model mode honors declared flags, adds enabled boundaries even when supplied explicitly, and charges them against prompt allowance. |
| Prompt reader | Explicit absolute UTF-8 file or stdin, mutually exclusive with inline input; 1 MiB plus one overflow byte, preserving newlines/BOM. Empty/malformed/failed reads return no partial prompt; sanitized errors retain I/O kind and return immediately, including interruption. Synchronous readers do not own I/O deadlines or cancellation. |
| Text output | Exact original token bytes plus incremental lossy UTF-8 display; at most three incomplete bytes retained, truncation flushed on finish, provisional suffix discarded on reset. Failed pushes preserve prior state. |
| Reference sampler | Explicit seeded temperature/top-k/top-p selection with stable ID ties and defined draws; greedy driver default, checked options before model loading. This generic sampler is distinct from native Swift sampler parity. |
| Tokenization-only mode | Full artifact identity and bounded metadata verified without graph allocation or retained weight payload; whole-file hashing still reads the artifact. |
| Verified sessions | Fixed tokenizer/special-token declarations and exact text/raw-ID accounting; failed requests leave sessions reusable. Output-piece sessions retain a bounded vocabulary and declared control status, supplying independent display streams without retaining the model file. |
| JSONL worker | Fixed verified readiness identity/policy and bounded count/encode operations; malformed requests have value-free recoverable errors, oversized framing/transport failures terminate. |
| Worker client | Exact readiness identity, policy, response shape, correlation, counts and vocabulary/raw sequences checked; protocol/transport faults close admission. Callers own process lifetime, deadlines and cancellation. |

## Transactional state

Generation, graph work and caller-owned cloneable sampling state participate in one
transaction. Admission, sampling, decoder, delivery, cancellation and deadline failures
restore state, including failed graph recovery. Successful stop draws advance sampling
state without entering conversation history. Absolute deadlines bracket synchronous
decoder and delivery work; an in-flight operation must return before expiry is detected.

Reusable logits and graph scratch never alias independently owned checkpoints.
State-only prompt ingestion omits unused output/logit work while validating every
computed state value; final uncached tokens and decode steps produce logits, and exact
prefix hits restore saved output. Explicit 1–256-token blocks retain sequential
arithmetic and whole-request rollback, with cancellation and deadline checks between
tokens. Sequential blocks do not imply tiled matrix-matrix execution or measured prefill
benefit.

Matvec activation preparation is call-scoped and shared only for matching input,
basis/permutation, width and quantizer; mixed Q8_K/Q8_0 or transformed/plain inputs stay
separate. Under the FP16 reference policy, full-attention state stores exact
binary16 bits and widens them before scalar products. The Swift development
policy instead retains the selected K4/V4 packed representation; its checkpoints
must preserve those exact packed bytes and scale bits. Convolution/GDN stays FP32.
Snapshot budgets conservatively charge every referenced sealed 64-token KV page, page
table, mutable-tail capacity and recurrent state per snapshot, without depending on
sharing for admission.

### Latest-prefix reuse

`bonsai-tools chat --prompt-reuse latest` (PowerShell
`Start-Chat.ps1 -PromptReuse latest`) selects the native runtime's latest
committed prefix. Omission or `disabled` resets every request. Selection survives
preparation and Retry; publisher serving rejects `latest`. The compiled
capability guard requires reviewed source/file identities and authenticated
readiness `"prompt_reuse":"latest"`, omitted when disabled. Any mismatch rejects
startup without fallback.

Reuse retains current committed state and logits, with no saved-prefix tree.
It requires an exact token prefix in the same model session and bit-identical
adapter scale. Shortened, switched or edited nonextending histories recompute
from empty state. Scale changes, including signed zero and change-back,
invalidate provenance. Changing policy clears model state while preserving
sampler progression. Reset also clears the count seal and reseeds sampling.
Cancellation or failed provisional output restores the transaction and closes
service admission until an acknowledged reset.

Requests cannot override cache policy. Compare complete multi-turn latency,
cached/uncached token counts, resources and complete logits/state against fresh
recomputation across edits, switches, scales, page boundaries, cancellation and
reset. Decode TPS alone cannot demonstrate saved prefill. Actual-model service
acceptance and measured benefit remain pending.
## Acceptance ownership

The [conformance contract](prism-upstream-tests.md) preserves original source inputs,
assertions, tolerances, unsupported results and production bridges. Full source
conformance, generic APIs and accelerator coverage exceed the bounded Swift subset.
The [lock-update procedure](development-guide.md#prepare-a-runtime-lock-update)
separately reviews source, immutable released bytes and Runner integration.

[Validation policy](validation.md) permits local builds, analyzers, assembly inspection
and focused correctness checks; full suites and benchmarks are CI-only. The [promotion
contract](#default-runtime-promotion) requires reviewed main-only target-device evidence,
native fallback, independent standard-LoRA TPS/quality and no material correctness,
quality, security or resource regression. Hosted ARM64 results cannot establish
Snapdragon performance. Backend changes also require complete-state phase handoff and
compatible state/numerical identity.

## Runtime work and storage contracts

The selected source includes the following mechanisms. Counts describe source
work or logical payload; they establish no measured latency or process-memory gain.
Conservative snapshot and admission charges remain in force.

| Mechanism | Required behavior |
| --- | --- |
| Grouped six-head decode/prefill | Reuse cached K/V conversion across the six query heads of each KV group where worker partitioning permits. Preserve each head's reduction lanes, FP16 probabilities, gates and current-token F32 K/V. Other geometries retain their own paths. |
| Paired prefill scores/values | Consecutive prompt tokens share common cached K/V conversion. Each token retains its causal prefix and original current-token F32 values; second-only history remains FP16. Odd tails use the existing path. Private scratch may discover a later failure earlier, but incomplete work cannot publish success. |
| Immutable logits snapshots | Checkpoints share complete immutable logits; later evaluation detaches without copying stale output. Retained snapshots cannot observe mutation. A Swift result is 248,320 F32 values (993,280 logical bytes); logical budget charges remain conservative. |
| Reset-only terminal ingestion | When prompt reuse is disabled, ingest the final non-stop capped token without computing unused next-token logits. Complete state and sampler rollback remain transactional. EOS enters neither state nor output. Generic Runtime, prefix reuse and speculative APIs retain terminal logits. |
| Bounded special parsing | Scan bounded windows with longest-spelling lookahead, retaining earliest-start/longest-at-start matching and borrowed UTF-8. Window boundaries create no token boundaries; failure returns no partial result. |
| Request-local BPE reuse | Repeated short pretokenized pieces reuse exact completed token IDs within one encoding request/rule owner. Full byte comparison prevents collision reuse; long pieces bypass it. No cross-piece merges or persistent tokenizer cache. |
| Empty reset/checkpoint | Canonical empty state avoids redundant zero filling and copying 149.625 MiB of zero recurrent/convolution payload. Nonempty state and independent checkpoints remain complete. |
| Bounded top-20 sampling | Traverse all logits for finite-value validation while retaining only the required candidates. Preserve tie order, softmax and RNG progression; reject nonfinite tail values before drawing. |
| Projection/preparation reuse | Stream eligible F32/BF16 weights and share only identical call-scoped activation preparation. Preserve decoded-scale, rounded-product, F64-sum and failure contracts; retained storage never substitutes for input identity. |

Terminal-ingestion receipts must reject double-counted decode/terminal work or EOS ingestion. The source's BPE cache is bounded at sixteen entries of
at most 32 bytes and 32 token IDs; hits retain ordinary token-limit/error precedence.

[Resident packets and prefill](hexagon.md#resident-packets-and-prefill) own the
separate NPU transport, operation lifetime and signed-solver boundaries.
[CPU kernels](cpu-kernels.md) owns later source candidates and dispatch policy.

## Bounded parallel-row qualification

Runner explicitly selects four row workers; trusted
`bonsai-tools chat --native-row-workers 1` selects one. A persistent pool and
readiness identity establish availability, not correctness or benefit.

For each selected source and worker policy, compare all logits and every logical
KV, convolution and recurrent component against the declared reference, including
finite values, exact extents, checkpoint/reset behavior and failed-request recovery.
Retain observed dispatch separately from the configured worker budget. Require
actual-model lifecycle, cancellation, resources and complete-answer benefit under
[application acceptance](validation.md#actual-model-application-acceptance).
Fixed-step timings exclude costs such as loading/snapshots and cannot supply
application throughput or full-runtime promotion.

Full-output workloads need a feasibility estimate covering prefill, decoding,
cleanup and retention under one original owner allowance. Leave an infeasible
gate pending; a longer ceiling alone does not make the workload eligible.

## Default-runtime promotion

The Rust `runtime-promotion` command reports the reviewed requirements for full
runtime qualification with the pinned LoRA at scale 1.3. Its policy and scoring
remain unchanged by the owner-directed Rust/PQ2 development default. Pending gates
stay pending; the development disclaimer and manual publisher rollback remain in
place. A default selection alone cannot produce `promotionReady: true`.
The evaluator's existing full-context numerical gate still names the FP16
reference contract; it does not score or qualify K4/V4 cache quality. That
approximate cache needs its [independent packet](quantized-kv.md#first-practical-promotion-packet).
It reads the compiled release/model/adapter/template locks and requires an explicit full
Runner revision. Omitting `weightRepresentation`, or explicitly selecting
`original-PQ2_0`, preserves the existing PQ2 policy. Select
`current-repacked-PTQ1_0` for a separate PTQ1 policy bound to the complete export
receipt, exact derived model and original PQ2 rollback identities. It retains all
seventeen runtime/application gates and adds independently reviewed lossless-packing
and actual-device original-PQ2 rollback gates. Its paired benefit gate compares
PQ2 and PTQ1 on the same released engine and app settings. Scoring changes no launch default.

### Inspect the policy

```powershell
$revision = git rev-parse main
$plan = & $env:BONSAI_TOOLS_PATH runtime-promotion --json (@{
    operation = 'plan'; runnerRevision = $revision; nativeRowWorkers = 1
} | ConvertTo-Json -Depth 64 -Compress) | ConvertFrom-Json -AsHashtable
$plan.policy.requiredGates
& $env:BONSAI_TOOLS_PATH runtime-promotion --json (@{
    operation = 'file-score'; runnerRevision = $revision; nativeRowWorkers = 1
    evidencePath = 'D:\Evidence\promotion.json'
} | ConvertTo-Json -Depth 64 -Compress) | ConvertFrom-Json -AsHashtable
# Review the four-worker candidate independently; its evidence cannot qualify one worker.
& $env:BONSAI_TOOLS_PATH runtime-promotion --json (@{
    operation = 'plan'; runnerRevision = $revision; nativeRowWorkers = 4
} | ConvertTo-Json -Depth 64 -Compress) | ConvertFrom-Json -AsHashtable
# Review the current-weight PTQ1 candidate independently from original PQ2.
& $env:BONSAI_TOOLS_PATH runtime-promotion --json (@{
    operation = 'plan'; runnerRevision = $revision; nativeRowWorkers = 1
    weightRepresentation = 'current-repacked-PTQ1_0'
} | ConvertTo-Json -Depth 64 -Compress) | ConvertFrom-Json -AsHashtable
# Bind a reviewed owning Prism or Runner main harness separately from the release.
& $env:BONSAI_TOOLS_PATH runtime-promotion --json (@{
    operation = 'plan'; runnerRevision = $revision; nativeRowWorkers = 1
    serviceHarnessRevision = $reviewedHarnessMain
} | ConvertTo-Json -Depth 64 -Compress) | ConvertFrom-Json -AsHashtable
```

The evaluator checks offline completeness and byte integrity, without inference,
benchmarks, selection changes, reviewer authentication or network provenance checks.
The version-2 policy binds the selected `nativeRowWorkers` (one by default, or explicit four)
and `servingStatePolicy: reset-every-request`. Evidence for a different worker count
or experimental state reuse cannot qualify this policy. Regenerate pending manifests
after a policy identity change; retained historical reports remain unchanged.
PTQ1 `file-score` requests must select the same `weightRepresentation`; PQ2 evidence,
changed packing receipts and changed original-PQ2 rollback identities are rejected.
The scored report retains the policy digest, profile, exact identities, gate counts
and observed device family. Each gate includes its requirement, owning repository,
device/human-review requirements and submitted review note and reviewer identity,
so a retained score identifies the candidate and the work still pending. Filter
`gates` by `status` to review pending or rejected requirements; a successful command
with either status still leaves `promotionReady: false`.
File scores also retain `evidenceManifest.bytes` and `evidenceManifest.sha256`
over the exact input file bytes before parsing. This digest differs from the
canonical JSON `policySha256`. Completed gate records include the verified
relative `reportFile` and service `harness.packageFile` for archive retrieval;
pending attachments expose no unverified retrieval path. Archive the original
manifest and those files with the score.
Independently review CI/source/report/reviewer identities and require `promotionReady:
true` before claiming full qualification or promoting PTQ1; command success alone
cannot authorize that promotion. The development default is a separate owner choice.

`serviceHarnessRevision` defaults to the pinned runtime source; explicitly select a
reviewed Prism or Runner main revision when its owning checker tests a separately
pinned release. Only `released-adapter-service-integration` uses that harness revision.
Runtime Foundation/conformance and all other Prism gates still require the exact
released source, and Runner gates retain their exact application revision.
The policy exposes both accepted `serviceHarnessProvenance` triples; their addition
changes the policy digest, so regenerate the evidence envelope. A scored gate's
`repository` identifies its owning project; `executionRepository` identifies the
actual CI repository alongside `executionRevision` and `runUrl`.

### Required evidence

Required gates cover release provenance, runtime Foundation/conformance, Runner
Foundation, 8K logits/full state, prompts/sampling/stops, 4K-output lifecycle/recovery,
complete-model resources, adapter basis/logits, standard-scale TPS/human quality,
actual-model sandbox/transport, speech/offline recovery, sustained device/creative
acceptance, installation and rollback. Released private-service adapter readiness and
per-request scale/state lifecycle qualify independently of source-graph support or
adapter-disabled smoke checks.

Device gates require self-hosted ARM64 Snapdragon X Elite, X2 Elite or X2 Elite
Extreme evidence, with the same family across all completed device gates.
`processorName` must match that family.
Paired promotion requires predeclared **10% end-to-end latency benefit**, or reviewed
capacity/energy benefit within accepted limits; same-engine LoRA TPS is independent.
Full suites/benchmarks stay CI-only; focused local correctness checks are allowed.

### Manifest format

The 1 MiB UTF-8 JSON evidence manifest must live outside Git alongside its retained
reports. Copy `policySha256` and the entire `policy.identities` object from the plan.
`gates` may omit unfinished work; every omitted gate stays pending. A complete pending
skeleton is also returned as `evidenceTemplate` by the plan. A supplied record has this
shape:

```json
{
  "schemaVersion": 2,
  "policySha256": "<exact plan policySha256>",
  "identities": { "<copy every policy.identities member>": "<unchanged>" },
  "gates": [
    {
      "id": "runner-foundation",
      "status": "passed",
      "note": "Reviewed required jobs, exact revision and retained final verdict.",
      "report": { "file": "runner/foundation.json", "sha256": "<report SHA-256>" },
      "execution": {
        "repository": "symbiogenesis/SwiftBonsai2Runner",
        "revision": "<exact selected Runner main revision>",
        "gitRef": "refs/heads/main",
        "runId": "123456789",
        "runAttempt": 1,
        "architecture": "ARM64",
        "deviceModel": "Snapdragon X Elite",
        "selfHosted": true
      },
      "reviewerId": "<actual independent reviewer>",
      "humanReviewed": false
    }
  ]
}
```

Each completed (`passed` or `rejected`) gate requires a nonempty report, bounded to 16
MiB, with an exact lowercase SHA-256 and a relative path without traversal. Reports must
remain outside Git, namespace paths and reparse points. Runtime gates bind the selected
Prism source revision except the explicitly separated released-service harness;
application gates bind the supplied settled Runner revision. Each
named CI execution must use `refs/heads/main`. Passed gates require an identified
reviewer; human quality and sustained comfort also require `humanReviewed: true`.
Pending records need only `id`, `status` and a bounded nonempty `note`.

A completed released-service record additionally requires `testedRuntime` equal to
the selected policy's release identity, and `harness` identifying the exact owning
workflow and retained package. Its `execution` describes the selected owning harness
main revision, run and device. For example (placeholders must be replaced):

```json
{
  "testedRuntime": {
    "revision": "<runtimeRevision>", "version": "<runtimeVersion>",
    "archiveSha256": "<runtimeArchiveSha256>",
    "serverSha256": "<serverSha256>", "workerSha256": "<workerSha256>"
  },
  "harness": {
    "workflowPath": ".github/workflows/private-service-acceptance.yml",
    "artifactId": "<numeric Actions artifact ID>",
    "artifactName": "private-service-check-<harness revision>-<run ID>-<attempt>",
    "package": { "file": "prism/harness.zip", "sha256": "<retained package SHA-256>" }
  }
}
```

The existing Prism pair requires `execution.repository` equal to
`symbiogenesis/prism-inference` with the workflow and artifact naming above.
Runner's owning harness requires `execution.repository` equal to
`symbiogenesis/SwiftBonsai2Runner`, `workflowPath` equal to
`.github/workflows/native-runtime-acceptance.yml`, and `artifactName` equal to
`native-acceptance-bin-<run ID>-<attempt>`. Both require the exact selected
`serviceHarnessRevision`, full tested-runtime identities and retained package
digest. Mixing repositories, workflows or artifact naming families rejects.
The owning source/run/artifact identities must be independently verified; a
successful narrow service workload alone cannot attest that every gate requirement
was exercised.

The retained package is bounded to 64 MiB and uses the same external relative-path,
locked-file and digest checks as reports. Artifact ID, owning workflow/run and
package contents still need independent provenance review; the offline evaluator
does not query GitHub or authenticate those claims. Separate harness fields on
other completed gates reject. Only schema-2 manifests can pass scoring.

### Evaluation rules

Unknown or duplicate gates, malformed identities, non-main provenance, wrong devices,
missing reports and digest mismatches fail the command. Any known rejected gate makes
the aggregate rejected even when other work remains pending. Only every required gate
passing produces `promotionReady: true`.

Evidence collection/review follows [validation](validation.md), the supported runtime contract above and [PLAN.md](../PLAN.md).
