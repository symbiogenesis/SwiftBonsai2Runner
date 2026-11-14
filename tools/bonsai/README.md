# Bonsai maintenance tools

`bonsai-tools` provides protected credential creation, fixed CPU and server invocation plans,
bounded
completion-process supervision and LoRA qualification scoring. It forbids unsafe code and reuses the
audited Windows boundary crates.

## Single executable builds

Use the [single-executable build](../../docs/development-guide.md#build-a-single-executable)
to assemble the five native tools and notices. The
[package contract](../../docs/chat-packages.md) owns `chat-package`, `chat-bundle`,
verified extraction, installed launch and rollback. Both GUI-subsystem
`bonsai-launcher` and console `bonsai-tools` expose the maintenance protocol.

## Packaged use

Use a trusted source-bound Windows ARM64 executable outside Git. `--version`
advertises commands and protocol 5; unsupported protocols reject before work.
JSON commands accept `--json JSON_OBJECT`, `--request ABSOLUTE_JSON_PATH` or
`--request -` for stdin, with identical size/schema checks.

```powershell
$env:BONSAI_TOOLS_PATH = 'C:\BonsaiTools\bonsai-tools.exe'
& $env:BONSAI_TOOLS_PATH credentials --request C:\Evidence\credentials.json
& $env:BONSAI_TOOLS_PATH cpu-check --request C:\Evidence\cpu-check.json
& $env:BONSAI_TOOLS_PATH qualification --json (@{
    operation = 'file-score'; qualificationProfile = 'standard-scale-one'
    evidencePath = 'C:\Evidence\capture.json'; reviewPath = $null
} | ConvertTo-Json -Compress)
```

For source builds and `chat` launch options, use the
[development guide](../../docs/development-guide.md#contributor-source-launch),
[runtime selection](../../docs/development-guide.md#runtime-selection) and
[source-cache contract](../../docs/development-guide.md#source-launch-caches-and-runtime-selection).
`scripts/Start-Chat.ps1` only forwards supplied options to the native tool.

Credential requests contain only `parentDirectory`, an existing absolute external
local directory; results contain protected file paths, never key bytes.
Planning commands never start inference.

`weight-patch` is offline research tooling for source-bound rank-one FFN
corrections. Its `direction` operation estimates a unit direction from fitting
residuals; `fit` solves the soft-constrained L1 objective and checks a dual bound
after F32 export. Held-out answer groups stay outside fitting. Numerical
convergence grants no behavioral qualification or permission to install a patch.
See the [research contract](../../docs/weight-space-programming.md#offline-fitting-and-an-explicit-native-research-path)
for capture identities, basis, bounds and the separate experimental native path.

`adapter-edit` edits the existing approved adapter's floating-point weights,
retaining all 129 sites, the rank-one B directions, embedding and alpha. A request
supplies `schemaVersion: 1`, `operation: "inspect"` or `"create"`, exact-source
`adapterPath`, and a rational `keepNumerator` / `keepDenominator` in `(0,1]` with
denominator at most 1024. Per linear site it retains the largest absolute A
coefficients, rounding the keep count upward and resolving ties by original
index. Other coefficients become exact zeros. `inspect` reports per-site/family
weight-space error and predicted sparse payloads without writing. `create`
requires a new external `outputPath` and atomically publishes a separate GGUF;
it cannot replace or install the approved adapter. Zeroing F32 payloads leaves
the GGUF byte length unchanged. Native sparse execution is separately required
for memory or arithmetic savings. Neither retained weight energy nor a factual
smoke establishes broad quality or an inference-speed improvement.

`ffn-surrogate` fits an offline low-rank replacement of a whole FFN branch for
W8, or a recovery adapter for structured FFN omission. Unlike an FFN-down LoRA,
`y = B*(A^T*x)` consumes the original normalized
5,120-wide FFN input and predicts the complete 5,120-wide branch delta, including
the teacher's frozen adapter contribution. A future schema-1 replacement must omit that
site's original gate/up/down and adapter work; applying the skipped adapter again
would double-count it. No runtime or application currently loads this artifact.

Supply `schemaVersion: 1`, `source`, `site`, `rows`, optional `controls` and an
optional new external `outputPath`. `source` binds `modelSha256`, `runtimeSha256`,
`runtimeSourceCommit`, nullable `adapterSha256`, exact hex `adapterScaleBits`,
`weightRepresentation` (`PQ2_0` or `current-repacked-PTQ1_0`), `executionProfile`
(`experimental-ffn-cpu-scalar-unfused-v1`) and `graphIdentity`. `site` names `layer`
0–63, `inputBasis: "original-ffn-normalized-input"`,
`targetBasis: "complete-ffn-branch-delta"`, and `inputWidth`/`outputWidth` 5120.
Existing post-gating FFN-down probe rows cannot substitute for these inputs.

Each row supplies `sampleId`, content `groupId`, `tokenIndex`, `split` (`fit` or
`development`), and finite F32 `input`/`target` vectors. At most 64 total rows are
accepted, including at least two fit rows and one development row. Samples and
content groups cannot cross splits; final held-out answer data is not accepted.
Controls are `rank` 1–16 (default 8, no greater than fit row count), positive
`ridge` 1e-8–1e6 (default 0.001), and `timeBudgetMilliseconds` 1–30000 (default
5000). This deadline starts at typed admission, covers validation, numerical
fitting, diagnostics and export preparation, and is checked immediately before
publication. It is not a hard end-to-end cutoff: synchronous file write, sync,
link and cleanup operations cannot be preempted by this function. Run the command
under native owned-process supervision that enforces the original aggregate
deadline across CLI intake, publication, CLI output and cleanup; retain cleanup
time within the original owner-device allowance. Candidate hashing completes
before publication.

The deterministic fitter forms a pivoted output subspace from fit targets only,
then solves dual ridge regression with Cholesky; dependent targets can reduce
effective rank. It exports rank-major F32 factors, hashes the candidate bytes,
and recomputes fit/development reconstruction errors from those exported factors.
The 300-million-cell work cap and numerical deadline reject exhaustion before
publication. Emitted `limits.timeBudgetScope` records this checkpoint boundary;
it does not replace the required aggregate supervisor deadline.
Factor bytes and multiply-accumulate counts are accounting estimates: they
establish neither actual process RAM nor latency nor complete-answer quality.
Final held-out answers, source-authenticated capture, runtime state/rollback,
real weight omission and the original/cheap/compensated comparison remain separate
acceptance requirements under the [research contract](../../docs/weight-space-programming.md).

For structured omission, supply `schemaVersion: 2` with the same `source` and
controls. The site instead uses `targetBasis: "teacher-minus-retained-ffn-branch"`.
Add `studentExecution` with the distinct
`executionProfile: "experimental-ffn-group-omission-cpu-scalar-unfused-v1"`,
`graphIdentity` and `prefixPolicySha256`, plus the producer's `captureSha256`.
`omission` requires `groupWidth: 1024`, `originalHiddenWidth: 17408`, sorted
distinct `omittedGroups` in 0–16 with at least one group retained,
`transform: "independent-contiguous-1024-signed-hadamard-v1"`, and
`retainedAdapterPolicy: "zero-omitted-original-hidden-before-standard-adapter"`.
Its `signIdentity` contains `fullSignsSha256` for the complete metadata sign
array in canonical I32-le order, `downSignOffset: 11264` and
`downSignCount: 17408`.

Each row replaces `target` with `teacherTarget`, `retainedBranch` and
`originalPrefixTeacherTarget`, all width 5120. The full and retained branches
must share the supplied normalized student `input`; `sharedInputSha256` is
verified against its F32-le bytes. `studentPrefixSha256` and
`originalPrefixSha256` identify their separate replay histories. Samples,
content groups, identical inputs and prefix identities cannot cross the
fit/development boundary. Select capture tokens after any shared prompt prefix;
otherwise duplicate inputs can correctly reject the packet. Identities bind
declarations; the offline fitter
cannot authenticate the runtime, branches, signs or content independence.

The fitter uses F64 subtraction of the supplied F32 teacher/retained values
and evaluates exported F32 factors against complete teacher-branch energy.
It reports the uncompensated error and original-prefix drift separately.
Schema 2 withholds publication and returns `status: "development-rejected"`
if either split has zero teacher energy, development relative squared error
exceeds 0.1, its gap above fit exceeds 0.05, or compensation worsens development
error versus the retained branch. A passing algebraic screen leaves behavior,
prefix-drift recovery, performance and process RAM unqualified. Resource
estimates distinguish PQ2 34-byte and PTQ1 28-byte blocks; all-64-site figures
are geometry only. Existing schema-1 behavior remains available.

Prism's source-only `ffn_probe` collector accepts
`captureKind: "ffn-group-omission"` and `omittedGroups` in its version-1 request
and emits schema 2 directly. It replays the original graph, resets the same
model, then replays the group-omitted graph using the same fixed token IDs.
Both passes share its original deadline and 256-token total cap. The capture
loads every weight and executes the complete teacher FFN, then propagates the
retained branch. It applies no recovery factors and establishes no savings.
Retain its exact request and source-bound build identity for independent replay;
`captureSha256` is the producer's domain-framed packet digest before insertion
of that field, not the capture file's SHA-256. Runtime publication, compensated
rollout, multiple edited sites and free-generation acceptance remain pending.

`native-link-verify` binds retained native link inputs, real ARM64 PE import tables
and an exact flat package to a separately reviewed schema-3 compiler-source
closure. It runs no linker or inspected image. Supply `--request PATH` containing
`{"manifest":{"path":"ABSOLUTE_PATH","bytes":N,"sha256":"REVIEWED_SHA256"}}`.
The source verifier must have run independently; digest binding of its reviewed
receipt is not a fresh source-verification result. See the
[link/package capture contract](#selected-native-link-and-package-capture).

`pe-closure` audits ARM64 PE files without loading them. Supply `bundleDirectory`
and `systemDirectory`; omitted/null `aliases` reads `apisetschema.dll`. It follows
candidate normal/delay imports and recursive system normal imports, rejects GUI
dependencies and records optional system delays without following them. The
schema-1 report binds bounded file digests/import edges and declares unresolved
forwarded-export scope. Native supervision has a 120-second hard cutoff; success
retains `imports.json`, while failures retain useful diagnostics.

`publisher-startup` accepts `candidateDirectory`, fresh external `outputDirectory`
and `workBudgetSeconds` (45–180, default 90). Ten fixed model-free X2 probes use
the archived hosted package. Optional `modelSmoke: true` plus `dataDirectory`
adds one four-token original-PQ2 smoke under unchanged CPU policy. Both caller
identity fields bind external ownership. Nothing builds/downloads; setup through
teardown shares one allowance. See [candidate inputs](../../docs/runtime-candidates/README.md).

## Command boundary

### Selected native link and package capture

Run `bonsai-ci native-closure-verify` against the independently digest-bound
original schema-3 [source manifest](../ci/README.md#selected-compiler-source-closure)
first. Independently review that result and retain its exact JSON receipt. Then
run the source-bound maintenance executable:

```powershell
& $reviewedTools native-link-verify --request C:\Evidence\reviewed-link-request.json
```

The request contains only `manifest`, with absolute `path`, nonzero `bytes` and
lowercase `sha256`. That independently reviewed file has the strict schema below.
Every `Identity` uses the same three fields; unknown fields reject.

| Manifest field | Required original retained value |
| --- | --- |
| `schemaVersion`, `kind` | `1`, `selected-lld-arm64-link-package`. |
| `sourceManifest`, `sourceReceipt` | Identities of the original schema-3 source manifest and separately reviewed source-verifier JSON receipt. Their manifest/source/compiler/database/count identities must match. |
| `linker` | Exact original `lld-link` executable identity; no compiler/linker is executed by verification. |
| `links` | Nonempty array of original successful invocations, at most 128. |
| `packageDirectory`, `packageFiles` | Absolute existing flat directory and complete `{name, bytes, sha256}` file inventory, at most 4,096. Names are lowercase plain basenames. No directories, redirects, missing or extra files are admitted. |
| `systemImports` | Exact distinct lowercase DLL basenames of imports deliberately outside the package. Every name must actually be imported; packaged-file collisions reject. |

Each link has `workingDirectory`, full `arguments` starting with the exact linker
path, `exitCode: 0`, `inputs`, `output` identity, `packageName`, `normalImports`
and `delayImports`. Each input is `{kind, identity}` with `kind` one of
`compiled-object`, `toolchain-object` or `library`. Objects use `.obj`; libraries
use `.lib` or `.a`. Every selected source-manifest object must occur as an exact
`compiled-object`; relabeling it as external cannot omit its source obligation.
Input identities must exactly cover the explicit input paths in the original
argument array. Distinct links can share inputs; input/output aliases reject.

This supported subset requires explicit `/nodefaultlib`, `/machine:arm64`,
`/manifest:no` and `/out:ABSOLUTE_OUTPUT`. DLL links additionally require
`/noimplib`. Other accepted options are `/nologo`, `/subsystem:console`,
`/subsystem:windows`, `/dll`, `/opt:ref` or `/opt:icf`, `/debug:none`, `/nxcompat`,
`/dynamicbase`, `/highentropyva`, `/fixed:no`, `/incremental:no`, `/brepro`,
`/entry:SYMBOL` and distinct `/delayload:NAME.dll`. Other options, response files,
implicit/default libraries and library search paths reject. Export an original
explicit invocation in this subset; editing an original response-file invocation
into a new array cannot establish that it actually ran. The caller authenticates
original successful invocation provenance and the selected build's completeness.

Every packaged `.exe`/`.dll` must match exactly one link output in length/SHA-256,
including renamed packaged copies. Verification uses the existing bounded ARM64
PE parser to read both real normal and delay import tables; neither a manifest
assertion nor a source receipt supplies those values. Each table must match the
link's reviewed import names. Imports must resolve by name to another packaged
image or a declared system boundary. No image/DLL is loaded. Non-image package
assets retain exact bytes too. Windows file/ancestor guards remain alive during
verification; no receipt promises that files remain immutable after return.

Bounds are 16 MiB documents, 256 MiB individual files, 32 MiB PE images and 2 GiB
aggregate verified bytes including repeated inputs; each link has at most 4,096
inputs / 8,192 arguments, each at most 16 KiB. These bounds do not extend an
owner-device workload allowance. Retain only the original selection needed by the
owning replay consumer under the evidence-retention policy.

The receipt declares `sourceClosureReexecuted: false` and
`independentSourceReceiptReviewRequired: true`. Hashing a caller-supplied receipt
cannot prove its origin or substitute for independently running/reviewing the
source verifier. Static libraries may still contain C++ runtime code; this tool
does not inspect archive members, OS import implementations, API-set routing or
forwarded exports. `freshLinkerExecution`, `systemDependencyClosureChecked`,
`staticLibrarySourceClosureChecked`, `cppRuntimeAbsenceQualified` and
`nativeRetirementQualified` remain false. Separate `pe-closure`/owning-build and
actual-model qualification retain their existing scope. The named `native_link_`
controls use literal miniature files, with no original compiler execution or
claim that the fixture source receipt came from that verifier.

### Evidence archive

`evidence-archive` preserves an explicitly reviewed credential-free upload
selection outside Git. It performs no download, CI query, model work or gate
scoring. A `seal` request supplies existing `sourceDirectory`, fresh
`outputDirectory` (its parent must exist), `credentialFreeSelectionReviewed: true`,
an `identities` object containing the exact source/build/device/run/artifact and
review references, and `files`: explicit `{path, bytes, sha256}` records from the
reviewed capture selection. Relative paths use `/`; no recursive source discovery
occurs. Keep failed/no-go receipts in the selection. Model/adapters, credentials,
conversations and executable payloads are excluded; the caller still reviews file
contents, because filename checks cannot establish that arbitrary text is public.

The command copies exact selected bytes and writes `evidence-manifest.json` last.
It bounds each file to 64 MiB, the selection to 4096 files / 256 MiB and the
manifest to 1 MiB. Existing destinations, path aliases, redirects, Git storage and
overlapping source/destination trees reject. Failed writes leave a new incomplete
directory for diagnosis and never replace a backup. Preserve the returned
`manifestSha256` **separately from the archive**, in owner-managed durable storage.
An archive with a self-supplied replacement digest is not authenticated evidence.

```powershell
bonsai-tools evidence-archive --request C:\Evidence\reviewed-seal-request.json
bonsai-tools evidence-archive --json '{"operation":"verify","directory":"C:/Evidence/retained","manifestSha256":"SEPARATELY_RETAINED_64_HEX_DIGEST"}'
bonsai-tools evidence-archive --json '{"operation":"restore","directory":"C:/Evidence/retained","manifestSha256":"SEPARATELY_RETAINED_64_HEX_DIGEST","outputDirectory":"C:/Evidence/retrieved"}'
```

`verify` and `restore` reject missing, changed or extra archive entries, including
an altered manifest, and retain read-only file/ancestor guards while checking.
Restore preserves manifest bytes, relative paths and identities without the
original runner. Integrity success grants no qualification or default promotion.
The owner chooses durable storage and retention/backup automation; a local or
Actions-temporary copy alone does not establish durable retention.

```text
bonsai-tools --version
bonsai-tools credentials --request ABSOLUTE_JSON_PATH
bonsai-tools memory --request ABSOLUTE_JSON_PATH
bonsai-tools cpu-check --request ABSOLUTE_JSON_PATH
bonsai-tools server-plan --request ABSOLUTE_JSON_PATH
bonsai-tools cpu-check-plan --request ABSOLUTE_JSON_PATH
bonsai-tools cpu-check-report --request ABSOLUTE_JSON_PATH
bonsai-tools server-policy --request ABSOLUTE_JSON_PATH
bonsai-tools runtime-stage --request ABSOLUTE_JSON_PATH
bonsai-tools runtime-verify --request ABSOLUTE_JSON_PATH
bonsai-tools completion --request ABSOLUTE_JSON_PATH
bonsai-tools qualification --request ABSOLUTE_JSON_PATH
bonsai-tools weight-patch --request ABSOLUTE_JSON_PATH
bonsai-tools adapter-edit --request ABSOLUTE_JSON_PATH
bonsai-tools speech-models --request ABSOLUTE_JSON_PATH
bonsai-tools inference-lock --request ABSOLUTE_JSON_PATH
bonsai-tools inference-capabilities --request ABSOLUTE_JSON_PATH
bonsai-tools speech-build --request ABSOLUTE_JSON_PATH
bonsai-tools source-build --request ABSOLUTE_JSON_PATH
bonsai-tools chat-launch --request ABSOLUTE_JSON_PATH
bonsai-tools chat-package --request ABSOLUTE_JSON_PATH
bonsai-tools gguf --request ABSOLUTE_JSON_PATH
bonsai-tools host-capabilities --request ABSOLUTE_JSON_PATH
bonsai-tools native-acceptance-budget --request ABSOLUTE_JSON_PATH
bonsai-tools startup-diagnostics --request ABSOLUTE_JSON_PATH
bonsai-tools ptq1-capture --request ABSOLUTE_JSON_PATH
bonsai-tools native-qualification --request ABSOLUTE_JSON_PATH
bonsai-tools ptq1-packing --request ABSOLUTE_JSON_PATH
bonsai-tools packing-qualification --request ABSOLUTE_JSON_PATH
```

`host-capabilities` accepts `operation: "project"` with bounded `facts`, or `operation: "identity"`
with the v1 `capabilities` report. Native `doctor` supplies registry, CPU-set, memory and SetupAPI
observations; optional unavailable fields remain explicit unknowns. Rust owns projection, canonical
ordering and digest bytes. See [host inventory](../../docs/hardware-capabilities.md).

`native-acceptance-budget` receives canonical seven-fraction UTC timestamps,
monotonic ticks/frequency, run/revision and the optional complete owner binding.
The workflows capture `jobStartedTick` before checkout and charge the larger of
wall and monotonic setup time using integer ceilings. The receipt keeps
`PreparationWallElapsedSeconds` and `PreparationMonotonicElapsedSeconds` separate;
`PreparationElapsedSeconds` is their maximum. A supplied invalid, null or future
job tick rejects. Legacy callers may omit it, leaving monotonic setup explicitly
unknown; only wall time is charged.
Optional `wholeJobMaximumSeconds` defaults to 300 and accepts 300..900, explicitly
declared before starting the workload. The native-runtime acceptance workflow
offers 5/10/15 minutes, retaining a workload estimate/reason for either extension.
The case maximum is the declared total minus sixty seconds (240 by default,
840 at the ceiling). Setup and the inherited original owner clock can only
shorten it; a 270-second owner remains bounded even with a 900-second job.
Thirty seconds for postprocessing, ten seconds between case/process limits and
fifteen seconds before inherited owner work expiry remain reserved. Other capture
lanes retain their narrower limits. The owning workflow charges
tool-resolution/IPC time before starting the acceptance process. Both this call
and `startup-diagnostics` require the source-bound hosted maintenance tool on
the owner device; no source build can start inside their allowance.

`startup-diagnostics` receives `evidenceDirectory`, nullable `acceptanceExitCode`
and `budgetExpired`. It reads only the fixed bounded worker logs, validates all
session keys before publication, sanitizes across sessions, and returns only
`Ready`/`Classification`. Unsafe paths or incomplete collection suppress uploads.
`ptq1-capture` supports `receipt` with bundle/repository directories, revision and
reviewed pins. Receipt validation binds the collector source and ABI controls,
the pinned native ARM64 compiler version and compiler/linker digest metadata,
and exactly one record each for `swift-ptq1-reference.exe` and
`swift_model_check.exe`, with bounded positive lengths and lowercase SHA-256.
Binary record ordering is irrelevant; names are case-sensitive and duplicates
reject. This validates metadata only: owner intake still verifies the complete
archive, binary bytes and ARM64 images before execution.
`compare-abi` receives actual/expected paths and the source-bound expected SHA-256.
Neither command loads a model or qualifies numerical parity.

`cpu-evaluation` additionally owns `observed-plan` and `capture` operations for
the fixed four-trial X2 thread screen. Supply `dataDirectory` and `cimProcessors`
with one independent `{name, logicalProcessors}` CIM observation; Rust checks it
against native topology and complete processor/kernel identity. Capture requires
main-only native ARM64 owner CI, the checked maintenance receipt for `GITHUB_SHA`,
original `workDeadlineTick`, `workBudgetSeconds` in 1..240 and the owning
`parentProcessId`/`parentCreationTimeFileTime`. Setup, verification, preflight,
all four samples and reconciliation share that deadline, retaining twenty seconds
for cleanup/reporting. The workflow launches the native command directly and
retains CIM as its only host observation adapter. No downloads or builds occur.
`observeOutputProgress: true` retains the separate file/exit attribution and
suppresses every timing ratio. Rust owns atomic partial evidence, complete sample
identity checks, fixed-token pairing and observed-condition comparisons. Pure `plan`, `compare-conditions`
and `compare-sample-conditions` operations remain available.

`native-qualification` defaults to `captureMode: "numerical"`: `kind: "ptq1"`
selects one native/Rust pair at backend `auto`, `scalar` or `neon` and row-worker
count 1 or 4. Its `adapterCondition` selects `standard` (default, approved adapter
at 1.3), `loaded-zero` (the approved adapter is loaded at zero) or `no-adapter`
(adapter loading is omitted) for both processes. Exactly one condition runs;
there is no control matrix. `kind: "rows"` fixes the original PQ2
serial/four-worker pair with the standard adapter.
Requests provide `repositoryDirectory` and `operation: "plan"`, `"source-pins"`
or `"capture"`. Capture additionally requires existing `dataDirectory`,
`captureToolsDirectory`, `workBudgetSeconds` in 1..240 and the original
`workDeadlineTick` from Windows QueryPerformanceCounter/.NET Stopwatch.
The deadline can only shrink; the tool never bootstraps, provisions or renews it.
Rust validates current source pins, exact hosted binary inventories and the
running maintenance executable's revision receipt, then owns memory admission,
ABI preflights, capture supervision and atomic bounded partial receipts.
Both workloads refuse an existing Bonsai app or model process before model work.
Its independent PowerShell 7.6 decoder and packaged Rust numerical helper run
in owned processes under the same remaining allowance. The checked schema-3
maintenance receipt digest-binds the sibling helper; qualification holds its
verified file throughout work and never compiles it on the owner device.
Model-free plan, source, deadline and receipt regressions
and hosted compiler preparation run in Rust.
An isolated loaded-zero or no-adapter pass establishes only that selected native/Rust
one-token comparison. Cross-condition logits/state identity, nonzero application
and immutable writer authoring provenance require their own matched evidence;
do not collect them by splitting a matrix into repeated owner-device jobs.

Explicit `captureMode: "request-timing"` selects one source-built Rust checker
request with current PTQ1 and the standard adapter at 1.3; other adapter conditions
and `kind: "rows"` reject. The same backend and one/four-worker choices remain
explicit. A fixed public tree-name prompt, thinking disabled, production sampler,
seed 42, no prefix reuse and one reserved output token bound the work. This mode
executes no native reference model or ABI process. It reuses the exact hosted
capture package, artifact locks, memory admission, parent lifetime and native Job
supervision; stdout may be empty only in timing mode. The checker receives one
absolute deadline derived from the original remaining allowance, retaining 32
seconds for receipt verification and owned cleanup. No clock renews at model load.
The raw started/final receipts and supervision remain retained on failure; a
missing or empty final receipt leaves work unknown. Successful source/executable,
input, model/adapter, backend/worker and deadline identities, positive operation
timings and bounded successful counters are verified before reporting completion.
A sampled stop may commit zero output tokens. These source-session brackets use
a no-op text consumer and qualify neither HTTP throughput, isolated kernel timing,
complete answers, released-service behavior nor default promotion. The existing
main-only numerical workflow exposes this separate mode and defaults to numerical
capture; its tools-only preparation starts no device workload.

`ptq1-packing` independently verifies the complete current-weight export against
the original PQ2 file. Supply expanded absolute `sourcePath`, `outputPath` and
`timeoutSeconds` (1 through 300). Both complete file lengths and SHA-256 digests
must match the compiled export receipt. The verifier uses its own GGUF header
reader and fractional-trit decoder rather than the exporter or its codec. It
checks every canonical PTQ1 block against the original raw PQ2 symbols and scale
bits, every other tensor payload and every header byte except the declared
file-type/tensor-type/offset edits. The receipt binds the export receipt digest
and full block/tensor counts; retain it with source/build/run identities for
independent promotion review. No payload sample can substitute for a full pass.
The entire verification shares one deadline, including file hashing. On an
owner device, setup, intake and retention also consume the shared five-minute
allowance; leave the gate pending if the complete scan cannot fit. This reads
weights as data without starting inference, and qualifies no logits, state,
adapter basis, quality, resource or speed behavior.

`packing-qualification` owns the complete hosted reproduction workflow in Rust.
`operation: "execute"` accepts `dataDirectory` and a new `evidenceDirectory`,
both expanded absolute external paths. It rejects owner-device execution and
requires source-bound main-only GitHub-hosted Windows ARM64 context before any
artifact work. Compiled pins select the original model and exporter release.
Owned child processes bound download/verification, runtime staging, export and
independent scanning, including hashing outside the exporter's codec deadline.
Atomic phase receipts and a digest manifest survive failure. `operation: "limits"`
returns the fixed hosted budgets without accessing artifacts. Foundation's Rust
suite owns admission, receipt mutation, workflow-budget and failure-retention
controls.

Requests are JSON objects read through a retained read-only handle, capped at
16 MiB. Qualification envelopes allow 160 MiB for escaped responses; original
evidence/review files remain limited to 16 MiB each and require standard JSON.
Success writes one JSON object to stdout; failure writes stderr diagnostics and
returns nonzero. Credential receipts contain paths only. Native execution uses
external working directories and preserves inherited Windows/profile environment
variables. Caller identities are observed natively when omitted for supervised work.

`speech-models` validates the compiled native Qwen3-ASR 1.7B and Qwen3-TTS GGUF locks and
returns the selected download plan and aggregate paths/byte total without
networking or loading models. `speech-build` also owns native ASR compiler
preparation, selected daemon builds and explicit model provisioning in Rust.
Provisioning uses the shared native Rust HTTPS client, with Schannel and system
certificate trust on Windows, for HTTPS-only resumable downloads. It validates
range responses, streams into exclusively owned bounded partials, verifies exact
bytes/SHA-256, then publishes exclusively. No curl executable or version probe is
required. Ordinary startup size-checks
cached speech files; the native services verify their hashes before first use.
Select recognition with `model: "QwenAsr"`; the legacy `Canary` selector,
`CanaryModelPath` result and `--canary-*` connection options remain compatibility
names for the Qwen3-ASR service. Recognition uses a separate
`speech/native/qwen3-asr/<revision>/` cache and never substitutes Canary weights.

`source-build` owns content fingerprints, executable digest receipts, immutable
maintenance snapshots and the pinned Cargo invocation for source maintenance/chat
builds. Native publication writes a bounded atomic bootstrap receipt with the
snapshot digest and checked capabilities. Changed images receive their version
handshake in Rust. Cache hits skip Cargo; rebuilds use content freshness without
cache cleanup. `chat` and JSON `chat-launch` own option validation, provisioning,
credentials, arguments and native app lifetime. Rust holds the persistent per-user
instance lock until the owned app tree exits.
Build variants use independent, flushed-and-atomically-published receipts.
Non-build documentation/helper changes preserve reuse; referenced embedded
content and configuration changes invalidate it. Damaged outputs and incomplete
receipts never count as ready. Source-cache entries publish only after full
verified writes, and bad completed download partials get one fresh retry while
retaining diagnostic bytes.

Startup commands additionally advertise `StartupProtocolVersion: 1`. Their
optional `parentProcessId` and `parentCreationTimeFileTime` fields bind long work
to the caller's retained Windows creation identity. Native CLI admission observes
these fields when omitted; explicit identities remain validated.
Compiler/tool descendants belong to an atomic Windows Job with an inherited
Unicode environment and child-only overlays. Cancellation waits for the tree to
exit before build-lock release; owner death closes the Job. Model memory and
process policies remain separate and unchanged.

`ChatLaunchProtocolVersion: 1` identifies native instance-lock ownership.
`BootstrapProtocolVersion: 1` identifies native maintenance snapshot publication.
These additive contracts leave the general JSON protocol at version 5.

`chat-package` and `installed-chat` use the
[package contract](../../docs/chat-packages.md), including fixed paths, trusted
verification, in-window preparation and rejection of build/source overrides.

Portable discovery, downloads, confirmation, deferred replacement and recovery
follow the [update contract](../../docs/ui-architecture.md#application-updates).
Source and preview launches perform no update work.

`app-update` implements portable `portable-check`, `portable-download` and
`portable-install` operations. Download selects `version`; installation also
requires the current launch's `handoffPath`. The native launcher owns the final
replacement-helper handoff; a queued request alone never authorizes replacement.

MSI `check`, `download` and `install` accept `installationDirectory`; download
also selects `version`, and install requires `version`/`handoffPath`. A queue
alone cannot authorize installation: native confirmation, owned teardown and
fresh publication validation must precede Windows Installer handoff.

`gguf` inspects models and exports PTQ1. Use
`operation: "inspect"` with `modelPath`, optional new `outputPath`, and either
both `expectedBytes`/`expectedSha256` or neither. Omitted identity fields use the
compiled reference-Qwen pin. `dialect` defaults to `"Stock"`; `censusPq2: true`
requires `"Prism-73bfe6cc485554822f80c473a3c0c4f25e80f41b"`. Inspection defaults
to a 300-second complete-process deadline, including artifact hashing.
`operation: "export-ptq1"` requires `modelPath` and new `outputPath`, uses the
compiled original-Swift identity and annotates `SourceRevision` after success.
Its `timeoutSeconds` defaults to 1800; both operations accept 1 through 3600.
The export limit covers the complete owned process and is also passed to the
exporter's conversion deadline. Owner-device diagnostics still share the
five-minute total allowance; select a feasible bound including setup/cleanup.

Both operations accept an explicit trusted `ggufToolPath` (or
`BONSAI_GGUF_TOOL`). Otherwise they verify the compiled inference archive and
complete installed inventory under `dataDirectory`, `BONSAI_DATA_DIRECTORY`,
or `%LOCALAPPDATA%/SwiftBonsai2`, retaining artifact guards through execution.
Missing binaries fail; these operations never download or compile tools.
Metadata inspection skips tensor payloads; census and export read them.

`gguf` operation `check` runs independent GGUF/PQ2/PTQ1 fixtures. It requires `ggufToolPath` matching the exact
compiled release digest and a new `evidenceDirectory`. `case` defaults to `all`;
individual cases are `parser`, `strings`, `arrays`, `descriptors`, `artifact`,
`census`, `repack` and `rejection`. Fixture execution is restricted to hosted
native Windows ARM64 CI on `main`, and missing tools fail without skips.
Independent byte serialization, tokenizer hashes, exact descriptor diagnostics,
all 256 packed-byte patterns, invalid scales, buffer-boundary payloads, the PTQ1
wire oracle and failed-publication controls remain outside the released codec.
Ordinary Rust unit tests cover command policy without requiring that release.

`inference-lock` reviews independently selected Prism release and source ZIPs.
It retains read-only input handles, checks bounded inventories, file hashes,
ARM64 headers, source package versions and the compiler pin, then writes a new
candidate lock and review evidence. It does not extract or execute the release
bundle. `provision` operation `inference` owns explicit acquisition/staging; it
does not build the runtime or require Cargo.

`provision` operation `inference` with `includeCapabilities: true` invokes
`inference-capabilities` to verify the compiled release archive and every
installed file, retaining their read handles through an owned ten-second,
512 MiB model-free `--capabilities` probe. The exact v1 report is parsed with
the shared CPU-policy contract and must match the pinned version and native
Windows ARM64 target. Each output stream is limited to 16 KiB; stdout remains
separate from stderr. The result adds `Capabilities` and `CapabilityVerification`,
including the retained evidence directory. No model is loaded or profile selected.
This option requires a trusted current maintenance tool and rejects custom locks.

`speech-build` plans the pinned ASR compiler and prepares speech model files.
Its `environment` operation selects the ASR compiler/SDK environment; `build`
selects ASR, Rust TTS or both; `models` downloads and verifies the selected GGUFs.
TTS-only builds use ordinary Cargo without LLVM, CMake, Qwen/GGML sources or
native libraries. Bundled use receives prebuilt companions and app-owned model
setup, without running these contributor commands.

The retired TTS `stage-source` request is rejected before archive or filesystem
intake. Its Qwen/GGML ZIP extraction, nested source-cache validation and publication
paths are removed. Original source pins and license notices remain attribution
data. ASR still uses its pinned registry-package native engine and compiler;
`validate-names` retains the LLVM compiler archive path checks.

The model-free `speech-build` request `{"operation":"host"}` returns the native
Windows ARM64 `runner-native-speech-host` report and its full `HostCacheKey`.
Its versioned SHA-256 identity binds CPU registry name/vendor/identifier, Windows
kernel version, native target and positive Windows processor-feature flags from
a bounded query. It excludes power state, parking and thread counts. Missing
mandatory observations reject. ASR builds partition their Cargo directory and
receipt by this identity so changing CPU/OS/features repeats native compilation.
TTS-only builds use the Rust source/compiler identity and perform no native host
probe. Discovery does not qualify kernels or portable distribution.

`cpu-check-plan` combines the shared `bonsai-cpu-policy` plan with a current
available-memory observation. Its request contains `operation: "plan"`,
`dataDirectory`, `profile`, `useAbliterateLora` and `useRepackedPtq1`. It returns
the policy, compiled pins, selected paths and complete CPU-check argv without
reading model bytes or starting inference. `cpu-check-report` adds the existing
ordered CPU report and admission status using that same single observation;
the native `cpu-check` command owns timestamping, installed-runtime probes,
artifact selection, aggregate deadlines, supervision and retained failure receipts.
Its request contains `dataDirectory`, optional `profile` (default `reference-cpu`),
`checkOnly`, `useAbliterateLora` and `useRepackedPtq1` (all default `false`).
The original plan commands retain their existing responses. `cpu-check` loads no
model when `checkOnly` is true or memory admission fails; execution charges setup,
verification and cleanup to one 240-second allowance, within the owner budget.

`server-plan` accepts `dataDirectory`, optional `port` (8080) and `verification`
(`none`, `runtime`, or `artifacts`). Rust owns local storage checks, environment
policy, installed version/help probes and optional model verification. The fixed
profile remains launch-disabled; no request may add arguments or provision keys.
The lower-level `server-policy` command retains the pure shared broker policy.

Upstream partitioning and baseline policy are owned by Prism's
[conformance policy tool](https://github.com/symbiogenesis/prism-inference/blob/main/tools/conformance-policy/README.md).
They are not maintenance commands in this application repository.

Qualification `invocations` requests take `modelPath` and validated `controls`.
They return all 24 paired and four zero-scale argument vectors with the same
`ControlsSha256` used for scoring. Qualification `plan` optionally accepts
`weightRepresentation` to include the shared capture memory policy. The sampler
and reasoning settings of ordinary CPU checks remain distinct from qualification.

Qualification `capture` retains the CI/device and admission requirements, derives
the fixed batch in Rust and uses the existing completion supervisor once. It saves
initial evidence, reconciles only completed sample receipts after ordinary batch
failures, and retains pending human/logits/basis checks. Abrupt termination leaves
the durable per-sample records and initial summary for inspection.

Completion supervision retains verified artifact handles and owns the complete
child Job lifetime. It checks parent PID/creation time/liveness, physical/process
memory, deadlines and caller-specific output limits. LoRA batches share
verification across samples. Restricted tokens, filesystem/egress isolation and
daily-service security remain unqualified.

`runtime-verify` verifies the whole pinned archive, the complete installation and
ARM64 EXE/DLL headers through `bonsai-artifacts`, then runs bounded version and
optional help probes while every verified file and ancestor remains guarded.
It returns the runtime descriptor and file identities. These receipts do not
retain handles across the JSON boundary: completion receives the descriptor,
verifies it again in Rust and keeps the resulting guard until all children exit.
The selected completion executable must belong to that verified installation.

`runtime-stage` verifies the locked archive and uses the same Rust archive policy
to extract and verify a fresh external staging directory. It rejects unsafe
directory entries, Windows filename aliases, collisions, incorrect ARM64 PE
headers, and archives exceeding 4096 entries or 512 MiB expanded. It runs no probes
and publishes no installation. Failed current-attempt staging is removed after
local handles close; cleanup failures retain the stage and report both errors.
The pinned archive remains available to reproduce the original failure.
Rust `provision` downloads the archive, publishes staging with a no-overwrite
directory move, then calls `runtime-verify`. Staging receipts do not
retain handles across that boundary; the published runtime is verified again
before probes or execution.

Completed CI samples publish a single `capture.json` containing exact response
text, its UTF-8 SHA-256 and diagnostics. Rust decodes stdout strictly and retains
leading U+FEFF and trailing newlines. `diagnostics.json` remains available for
inspection; the completed `supervision.json` is published after the capture
records. Native qualification joins these records without rereading stdout. Failed
and incomplete samples never contribute completed response records to scoring.

Qualification scoring reads bounded original evidence/reviews directly.
[Committed fixtures](../../tests/fixtures/lora-qualification-v1.json) bind control
serialization, Unicode response hashes and accepted/pending/rejected reports.
Missing human reviews, native logits or basis evidence remain pending; known
regressions still reject. Scoring never launches inference.

`qualification` operations `run` and `screen-run` own complete native preparation
for the paired LoRA and PTQ1 prefix-screen captures. Supply an expanded absolute
`dataDirectory`; `run` also accepts `useRepackedPtq1` and `qualificationProfile`
(`standard-scale-one` or `app-scale-1.3`). Rust observes the processor and admission,
verifies the compiled runtime and adapter identities, retains the adapter guard,
constructs the capture envelope, and records atomic preparation failures. The
screen's ordinary headless `--version` worker preflight must pass before either
model admission or adapter verification. Runtime files and model/adapter bytes are
verified again and retained by the existing capture supervisor.

Both front-door and low-level captures require credential-free main-branch hosted
CI on an actual native Snapdragon X/X2 Elite. The 28/14-process full matrices have
no demonstrated complete five-minute owner bound, so self-hosted/owner execution
rejects before preparation. Use native nonlaunching `plan`/`screen-plan` commands
for inspection and a prebuilt tool for capture. Never partition a matrix across
repeated owner jobs to bypass the aggregate allowance.

Frozen policy/scoring fixtures cover CPU policies, artifact identities and argument
vectors, including Unicode prompts and adapter scales, independently of native commands.

## Development checks

Use explicit workspace packages/features and external working/target directories.
Prism native linking and compiler trials need separate build lanes. Local builds,
Release analyzers and focused regressions are allowed; complete suites and all
benchmarks remain CI-only. See [validation](../../docs/validation.md).

## Offline Hexagon policy

`bonsai-tools hexagon-policy --request ABSOLUTE_JSON_PATH` owns fixed dispatch,
session and computation plans; hosted source/build and retained driver identity
validation; and native/worker result consistency. Operations are `dispatch-plan`,
`session-plan`, `compute-plan`, `environment`, `json`, `driver-identity`,
`dispatch-build`, `session-build`, `dispatch-result`, `dispatch-worker`,
`session-result`, `session-worker` and `compute-result`. Plan requests supply
`maximumSeconds`; session requests explicitly supply `referenceInitialization`.
Compute requests select exactly one `case`: `F32`, `GuardedQ2`, `F32NoBlock` or
`GuardedQ2NoBlock`. Result requests carry original JSON in the `json` string and
native results carry `exitCode`. Build/worker requests supply the expected
source identities independently of the receipt.

Evidence JSON is limited to 64 KiB and depth 32. Parsing rejects duplicate or
case-aliased object names before map construction, trailing data and non-object
roots. The fixed native ABI uses ASCII object names; non-ASCII aliases are also
rejected. Exact integers and booleans cannot be replaced with strings, floats or
each other. Successful mapping never implies compute, and verified computation
never implies session teardown, Swift qualification or profile admission.

Rust owns OS signature observation, immutable file guards and owned worker
lifecycles through `hexagon-tools`. Hexagon hosted controls run native policy,
package-boundary, pipe-capture and progress-retention regressions and publish a
revision/digest-bound maintenance artifact. Session/dispatch owner jobs verify
that same-run artifact inside the existing aggregate setup clock and never
bootstrap Cargo on the owner device.

`speech-evaluation` also owns the complete hosted TTS comparison. A model-free
`{"operation":"host-plan","repetitions":2}` request derives the bounded local
thread grid; `plan` still accepts explicit thread counts. `evaluate` accepts
`repositoryRoot`, `dataDirectory`, optional `ttsPath` and `evidenceDirectory`,
`repetitions` (1–3, default 2), and `responseFormat` (`pcm` or `wav`, default `pcm`).
Execution requires this repository's hosted Windows ARM64 main CI and a prebuilt
tool. The workflow's retained Snapdragon selector rejects before checkout/setup;
owner-device speech qualification remains pending.

The native evaluator provisions both pinned quantizations before timed trials,
retains protected credentials, and starts the existing `session` supervisor in
an owned Windows Job. The supervisor keeps the unchanged memory limits and
25 ms observations. Native HTTP connects only to numeric IPv4 loopback, never
uses proxies/cookies/redirects, checks caller/session liveness while waiting, and
limits readiness/audio requests to 300/190 seconds. Strict HTTP framing and the
shared mono PCM16 codec bound every listening clip to 120 seconds at 24 kHz.
Failure receipts, phase timings, raw clips and separate process-lifetime memory
counter differences remain available; voice quality stays pending human listening.

## Hexagon preparation and bounded device probes

`hexagon-tools` owns preparation and probe lifetimes. Hosted-only
`build-dispatch`, `build-packed-artifact` and `sdk-inventory` requests take
`repositoryDirectory` and a new external `outputDirectory`; the extractor also
takes `ggufSourceDirectory`. They retain exact source/tool/lock/run identities
and never load device code or admit profiles.

`preflight` takes optional `packagePath`/new `outputPath` and `maximumSeconds`
(5..300). The Rust worker inventories present SetupAPI ComputeAccelerator devices
and bounded top-level package files. Signature failures remain unverified.

`device` takes a `request` object with `kind` (`dispatch`, `session`, `compute`,
`projection`),
`repositoryDirectory`, `packageDirectory`, `fixtureDirectory`, `outputDirectory`,
`signtoolPath`, `expectedRunnerRevision` and `maximumSeconds` (1..240). Session
accepts `referenceInitialization`; compute accepts one `case`: `F32`, `GuardedQ2`,
`F32NoBlock` or `GuardedQ2NoBlock`. Projection requires an external
`projectionRequestFile` under the
[original-projection contract](../hexagon/packed-projection.md#artifact-bound-signed-projection-diagnostic)
.
The native supervisor reserves 75 seconds for owned cleanup, verifies every
pinned file and catalog member, retains immutable input handles through child
exit, and records only fixed entered-stage markers. Windows verifies the observed
DriverStore signer's trust and publisher.
Same-run artifact provenance remains the caller's responsibility. These fixed
device probes change no OS trust, install no driver and load no model; their
results do not qualify Swift inference.
All setup/intake/execution/cleanup shares the owner's five-minute total budget.
Offline `hexagon-policy` plans and evidence validators remain available.
The separate [`model-device` operation](../hexagon/model-probe.md) runs the bounded
actual-model diagnostic; its inputs, snapshot limits and evidence scope differ
from these fixed device probes.

## Practical compressed-KV review

`kv-promotion` is an offline plan/review command for the selected-device
[compressed-KV development promotion](../../docs/quantized-kv.md). It never starts a
model, changes the runtime pin or enables compression. It accepts approximate
answer equivalence; exact FP16 answers/logits are not required. A result of
`ready-for-owner-review` is a bounded practical finding, not release qualification.

First freeze a `declaration` with `identities` and exactly three ordered
`workloads`. All names below are case-sensitive JSON fields:

- `identities`: full lowercase `runnerRevision` and `prismRevision` Git commits;
  lowercase SHA-256 `runtimeSha256`, `modelSha256`, `adapterSha256`,
  `templateSha256`, `codecIdentity` and `samplerSha256`; concrete
  `compilerIdentity`, unique `deviceIdentity`, observed `processorName`,
  `driverIdentity` and `projectionBackend`; `adapterScale: 1.3`, `seed: 42`,
  `contextTokens: 8192`, declared `maxCompletionTokens` in 1..4096 and
  `rowWorkers` of 1 or 4. `codecIdentity` hashes the complete reviewed codec
  numerical/layout descriptor. `samplerSha256` hashes the complete sampler
  settings. Runtime digest identifies the actual executable used by both arms.
- Each workload: `id`, `promptSha256` over exact UTF-8 prompt bytes, and an array
  `objectiveChecks` of unique predefined check labels. The IDs, in order, are
  `deep-retrieval`, `instructions`, `creative-continuation`.

Useful initial held-out tasks are below. Finalize their exact text, objective
checks, padding, sampler settings and token budget **before** requesting a plan.
Do not tune them after seeing candidate answers. Use a source-bound native token
count to make the deep task occupy at least 7,168 tokens while preserving its
reply reservation inside 8,192; an approximate character count is insufficient.
Count preparation and both comparison arms against the same declared workload.

| Task | Suggested prompt and objective checks |
| --- | --- |
| `deep-retrieval` | Place `The sealed archive access phrase is amber heron 47.` near the start of a fixed, unrelated numbered manifest. Put `The night ferry leaves pier nine at 06:40.` near its end. Ask for the phrase and ferry departure without adding facts. Declare both exact retrieval checks. Freeze all padding before capture. |
| `instructions` | `Return exactly three numbered steps for safely restoring a mistakenly deleted local text file. Do not propose downloading software, running commands, or changing another file. State uncertainty where recovery is not guaranteed.` Check three steps, useful recovery advice, stated uncertainty and all restrictions. |
| `creative-continuation` | `Mara cannot swim. The town's only bridge washed away yesterday. Her brother is waiting across the river with a lantern. Continue this scene in 150 words or fewer, preserve those facts, and end with a question spoken by Mara.` Check canon, length, speaker/question and coherent useful continuation. |

The protocol uses the existing common JSON entry point:

```powershell
# $declaration contains the frozen fields above; retain this exact declaration.
$planRequest = @{ operation = 'plan'; declaration = $declaration }
$plan = (& $bonsai kv-promotion --json ($planRequest | ConvertTo-Json -Depth 30 -Compress)) | ConvertFrom-Json
# Store $plan before capture. No capture or default change happens here.
```

The emitted policy fixes K4/V4 (`rsq4`) against FP16, at least 65% lower deep-task
peak KV bytes including scales, at least 128 MiB lower deep-task whole-process
peak private bytes, no individual usefulness drop over one point, and no average
drop over half a point on a blind human 1..5 rubric; three means usable and both arms must reach
it. The owner may review anonymous shuffled answers. Every objective check
must pass in both arms; a failing baseline needs investigation rather than a
candidate pass. A qualified complete-task latency regression above 10% is a no-go;
no speedup or energy saving is required.

A `review` request has the same `declaration` and an `evidence` object:

- `planSha256`: the emitted plan digest. `controls`: booleans `exactPackedRestore`,
  `cancellationRecovery`, `editedPrefixRecovery`, `adapterScaleRecovery`,
  `explicitF16Rollback`, `ownedCleanup`, `noFullHistoryMirror`, and
  `comparableWholeTaskMemoryCoverage`, all attested against this candidate. Coverage includes
  startup, the complete request and retained checkpoint/restore overhead.
- `pairs`: exactly three records in declaration order, each with `id`, exact
  `prompt`, `baseline`, `candidate`, `quality` and `objectiveChecks`.
- Each arm: the complete declared `identities`; `kvCache` (`f16` or `rsq4`);
actual distinct raw `graphIdentity` including its matching `kv-f16` or `kv-rsq4` marker;
`inputTokens`,
  `generatedTokens`, `occupiedTokens`; `complete: true`; full UTF-8 `answer` and
  matching `answerSha256`; complete-task `peakPrivateBytes`, `peakKvBytes`; equal
  `kvObservationTokens` at the measured cache position (at least 7,168 for the deep task), and
  `extraFailedOrRetriedRequests: 0`;
  optional `endToEndMilliseconds`; `powerBefore`, `powerAfter`, and
  `uninterruptedPowerObserved`. Power objects contain nullable `acConnected`,
  `batterySaver`, `batteryPercent`. Use explicit null for unavailable observations.
  Occupancy allows the final sampled token to remain unconsumed. Both arms use
  identical input counts and the same predeclared output limit, with that limit and the 64-token
  reserve fitting inside 8K. Cache memory is compared at the same observed position; natural
  answer lengths may differ.
- `quality`: `reviewerId`, `blind: true`, exact
  `baselineAnswerSha256`, `candidateAnswerSha256`, integer
  `baselineUsefulness`/`candidateUsefulness` in 1..5, and `majorRegression`.
  Major fact, task or canon failure is a regression even if average scores pass.
- `objectiveChecks`: records with `id`, `baselinePassed`, `candidatePassed`, in
  the declaration's exact order. Record losing results instead of dropping them.

```powershell
$reviewRequest = @{ operation = 'review'; declaration = $declaration; evidence = $evidence }
$reviewRequest | ConvertTo-Json -Depth 40 | & $bonsai kv-promotion --request -
```

The reviewer recomputes prompt and complete-answer digests and binds review to
those answer bytes. It rejects mismatched identities, missing or partial answers
and altered checks. Preserve the request containing those exact bytes as the
replay input, plus the concise result; neither a bare hash nor the result alone
can replace it. Observations and reviewer identity still need independent review:
this command cannot authenticate attestations or prove trial provenance.

Timing is qualified only with known matching AC/battery-saver observations and
attested uninterrupted observation across both arms. Equal endpoints alone are
insufficient. Unknown or mismatched power leaves timing unqualified, while valid
capacity/quality evidence remains available; the owner must separately record
acceptance of observed responsiveness before choosing a capacity-only default.
X1 and X2 results stay separate. Retain FP16 recovery, the fixed 8K limit and
existing admission reserves. Full release and hosted gates remain independent.
