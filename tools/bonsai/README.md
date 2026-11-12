# Bonsai maintenance tools

`bonsai-tools` provides protected credential creation, fixed CPU and server invocation plans, bounded
completion-process supervision and LoRA qualification scoring. It forbids unsafe code and reuses the
audited Windows boundary crates.

## Packaged use

Select Foundation's native Windows ARM64 executable from the evaluated main
revision and extract it outside Git:

```powershell
$env:BONSAI_TOOLS_PATH = 'C:\BonsaiTools\bonsai-tools.exe'
& $env:BONSAI_TOOLS_PATH credentials --request C:\Evidence\credentials.json
& $env:BONSAI_TOOLS_PATH cpu-check --request C:\Evidence\cpu-check.json
& $env:BONSAI_TOOLS_PATH qualification --json (@{
    operation = 'file-score'; qualificationProfile = 'standard-scale-one'
    evidencePath = 'C:\Evidence\capture.json'; reviewPath = $null
} | ConvertTo-Json -Compress)
& $env:BONSAI_TOOLS_PATH chat --repository-root $PWD
```

Every JSON command accepts `--json JSON_OBJECT`, `--request ABSOLUTE_JSON_PATH`,
or `--request -` for standard input. Rust enforces the same object, size and
schema checks for all three sources. No PowerShell module is needed for direct
use. `--version` advertises capabilities; use a trusted source-bound build.

For a source checkout, build the maintenance executable once with the pinned
compiler and an external target, then run it directly:

```powershell
$repo = $PWD.Path
$target = Join-Path $env:LOCALAPPDATA 'SwiftBonsai2/build/maintenance'
cargo build --locked --release --manifest-path (Join-Path $repo 'Cargo.toml') --package bonsai-tools --bin bonsai-tools --target-dir $target
$bonsai = Join-Path $target 'release/bonsai-tools.exe'
& $bonsai chat --repository-root $repo
```

`chat` is the native launch entry point; optional `scripts/Start-Chat.ps1`
forwards to it and offers `-NativeEngine`, `-PublisherEngine`, `-NativeRowWorkers`,
`-UseRepackedPtq1`, `-UseOriginalPq2`, `-PromptReuse` and `-GraphicsBackend`. The development
default is Rust Prism 0.1.23, original PQ2, four CPU row workers and the standard adapter at scale 1.3.
The [runtime lock](../../config/prism-inference.lock.json) owns the exact release/source identities;
the [Hexagon contract](../../docs/hexagon.md) describes conditional X2 NPU acceleration.
`--publisher-engine` selects manual publisher rollback; its worker default is one.
Use `--native-row-workers 1` for explicit single-worker Rust. Both engine flags
are mutually exclusive; `--native-engine` remains accepted for explicit Rust.
It uses the nearest Runner checkout and
`%LOCALAPPDATA%/SwiftBonsai2` unless `--repository-root` or `--data-directory`
is supplied. Boolean options are `--native-engine`, `--publisher-engine`,
`--use-repacked-ptq1`, `--use-original-pq2`,
`--no-abliterate-lora`, `--no-tts`, `--no-speech-models`, `--speech-models-only`
and `--force-build`. Value options are `--native-row-workers`, `--prompt-reuse`,
`--graphics-backend`, `--tts-quantization`,
`--tts-threads`, `--frontend-port`, `--backend-port`, `--canary-port`, `--tts-port`,
`--chat-path`, `--broker-path`, `--asr-path`, `--tts-path`, `--canary-key-file`,
`--tts-key-file` and `--working-directory`. All existing launch admission,
cache, verification, cancellation and teardown policy remains in Rust. An
unchanged direct launch needs no Cargo invocation. Rebuild the maintenance
executable after updating its sources; `--force-build` rebuilds application
companions, not the running maintenance image.

`--graphics-backend dx12|vulkan|gl` (JSON `GraphicsBackend`) explicitly selects
the native window renderer by overriding `WGPU_BACKEND` in the chat child only.
Omission preserves the inherited setting and upstream adapter selection. The
selected adapter/backend remains recorded in `chat.log`; the requested option
is retained in the deferred startup request. This does not change inference,
automatically retry a failed renderer or establish device qualification.

Rebuild the maintenance executable after source changes to receive current launch
defaults and options. The PowerShell facade forwards only supplied options; an older
maintenance executable can retain older defaults. Full-context/output, creative-quality,
sustained-performance, isolation and clean-machine qualification remain pending; this default
makes no speed claim. JSON `NativeEngine: false` remains an explicit publisher selection.
`--prompt-reuse disabled|latest` selects the [experimental prefix policy](../../docs/prefix-reuse.md);
it defaults to `disabled` and `latest` requires the Rust engine.

The two weight options are mutually exclusive. `--use-original-pq2` explicitly
selects the pinned original Swift artifact with either engine; the current default
remains original PQ2. JSON and installed-package requests express the same choice
with `UseRepackedPtq1: false`. That explicit false survives the deferred startup
request, and each launcher/app handoff emits one concrete weight-selection flag.
Missing or changed artifacts fail verification; no automatic format or engine
fallback occurs. The standard adapter remains enabled at scale 1.3 unless explicitly
changed. These controls prepare rollback selection; actual-device rollback
qualification remains a separate gate.

Credential requests contain only `parentDirectory`, an existing absolute external
local directory. The command creates fresh protected files and returns paths only.
Speech provisioning/build requests go directly to `speech-build`; their former
PowerShell wrappers have been removed. Planning never starts inference.

`publisher-startup` consumes a request with `candidateDirectory`, a fresh external
`outputDirectory`, and `workBudgetSeconds` (45–180, default 90). It pins the archived
hosted publisher package, runs ten fixed model-free probes on X2, and retains atomic
schema-2 startup receipts. Optional paired `modelSmoke: true` and `dataDirectory`
select one four-token original-PQ2 smoke with the unchanged CPU resource policy.
Supply both caller identity fields to tie work to an external caller. The command
builds/downloads nothing; setup, verification, workers and teardown share the one
allowance. A successful smoke does not qualify numerics, adapters or performance.
See the [candidate examples](../../docs/runtime-candidates/README.md).

The maintenance executable reports version 0.1.0, protocol 5 and its supported
commands through `--version`. Runtime guards remain held
through version/help probes and complete completion batches. Older protocols are
rejected before an operation can start.

## Command boundary

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

Host capability projection/CPU identities, acceptance-deadline policy, sanitized
startup diagnostics and PTQ1 collector receipt/ABI checks live in Rust. Their
former PowerShell modules/scripts have been removed. The offline qualification,
PTQ1 screen, runtime promotion and inference-lock wrapper scripts have also been
removed; invoke the native JSON commands directly as in the examples above.

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
unknown; they retain the earlier wall-only calculation.
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
identity checks, fixed-token pairing and observed-condition comparisons; the
former X2 PowerShell controller is removed. Pure `plan`, `compare-conditions`
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
Its independent Python comparator runs in an owned process under the same
remaining allowance. Model-free plan, source, deadline and receipt regressions
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
controls; the former packing PowerShell module, script and tests are removed.

Requests are JSON objects read through a retained read-only handle, capped at
16 MiB. Qualification envelopes allow 160 MiB for escaped responses; original
evidence/review files remain limited to 16 MiB each and require standard JSON.
Success writes one JSON object to stdout; failure writes stderr diagnostics and
returns nonzero. Credential receipts contain paths only. Native execution uses
external working directories and preserves inherited Windows/profile environment
variables. Caller identities are observed natively when omitted for supervised work.

`speech-models` validates the compiled native Qwen3-ASR 1.7B and Qwen3-TTS GGUF locks and
returns the selected download plan and aggregate paths/byte total without
networking or loading models. `speech-build` also owns native compiler/source
preparation, selected daemon builds and explicit model provisioning in Rust.
Provisioning uses Windows curl 8.4+ for HTTPS-only resumable downloads, verifies
exact bytes/SHA-256, then publishes exclusively. Ordinary startup size-checks
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

`chat-package` owns `create`, `inspect`, `install`, `rollback`, `installed` and
`start` operations. The [package contract](../../docs/chat-packages.md) defines
their JSON requests. New schema-2 packages contain only five native executables
and three notices; the PowerShell packaging module, launch/install wrappers and
their tests are retired. Rust retains schema-1 package inspection and rollback.
`installed-chat` is the human-facing alias for `start`; it accepts an optional leading
`--installation-directory PATH` and the installed launch options defined by the package contract.
It never enables source-path or build overrides.
Run the command using an independently trusted maintenance executable: package
hashes cannot authenticate the program performing the checks. `start` verifies
the active package and derives its fixed executable paths before native launch.
The legacy `launch` operation still supports a caller-bound directory, ID and
manifest digest. Caller options cannot override paths or request a source build.
The native launcher opens the pending UI before preparing backend and speech.

`app-update` provides installed-MSI `check`, `download` and `install` operations.
Requests identify `installationDirectory`; `download` also selects `version`, and
`install` requires `version` and `handoffPath`. A queued request cannot install by itself:
confirmed native UI exit and owned teardown must complete before the maintenance launcher
revalidates the publication and hands off to Windows Installer. Source/preview launches
perform no update work. See the [release/update contract](../../docs/releases.md#automatic-update-checks)
for publication identities, confirmation and pending installed acceptance.

`gguf` replaces `Read-Gguf.ps1`, `Export-SwiftPtq1.ps1` and their module. Use
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

`gguf` operation `check` replaces the PowerShell GGUF, PQ2 census and PTQ1
export tests and fixture module. It requires `ggufToolPath` matching the exact
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

`speech-build` validates the compiled native compiler/source pins, checks compiler
archive names in bulk and stages source ZIPs. ZIP identity, complete path inventory,
Windows aliases, entry types, expansion limits and existing cache bytes are checked
in Rust. ZIP link entries become ordinary files containing their archived bytes.
Its explicit `environment`, `build` and `models` operations additionally own
downloads, compiler TAR extraction, Visual Studio environment setup and native
process execution. Planning and source staging remain nonexecuting operations.

The model-free `speech-build` request `{"operation":"host"}` returns the native
Windows ARM64 `runner-native-speech-host` report and its full `HostCacheKey`.
Its versioned SHA-256 identity binds CPU registry name/vendor/identifier, Windows
kernel version, native target and positive Windows processor-feature flags from
a bounded query. It excludes power state, parking and thread counts. Missing
mandatory observations reject. Native speech builds partition their Cargo
directory and receipt by this identity, so changing CPU/OS/features repeats
native compilation instead of reusing another host's CMake or Cargo objects.
The build result retains the report as `NativeBuildHost`; discovery does not
qualify optional kernels or portable distribution.

Its `stage-sources` operation verifies and stages the compiled Qwen/GGML archive
pair together, then checks the complete cache inventory before returning.
Existing source-tree redirection and extra-file rejection run inside that operation.

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

The removed CPU/server and qualification compatibility wrappers are replaced by
these native commands and the existing `qualification` command. Their model-free
regressions run in Rust, retaining the independent frozen policy/scoring fixtures.

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
and publishes no installation. Failed staging is retained for inspection.
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
rejects before preparation. The former LoRA/PTQ1 plan-only workflows are retired; use
the native nonlaunching `plan`/`screen-plan` operations directly and keep full
qualification pending. The removed PowerShell wrappers are retired; use a prebuilt
executable for capture operations. Never split a
matrix across repeated owner jobs to bypass the aggregate allowance.

Frozen pre-port PowerShell fixtures cover complete CPU policies, artifact identities
and argument vectors, including Unicode prompts and adapter scale syntax. Pure
policy and packaged command checks run in Rust.

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
bootstrap Cargo on the owner device. The PowerShell implementation and its
script-only tests have been removed.

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

`hexagon-tools` replaces the Hexagon PowerShell scripts and modules. Hosted-only
`build-dispatch`, `build-packed-artifact` and `sdk-inventory` requests take
`repositoryDirectory` and a new external `outputDirectory`; the extractor also
takes `ggufSourceDirectory`. They retain exact source/tool/lock/run identities
and never load device code or admit profiles.

`preflight` takes optional `packagePath`/new `outputPath` and `maximumSeconds`
(5..300). The Rust worker inventories present SetupAPI ComputeAccelerator devices
and bounded top-level package files. Signature failures remain unverified.

`device` takes a `request` object with `kind` (`dispatch`, `session`, `compute`),
`repositoryDirectory`, `packageDirectory`, `fixtureDirectory`, `outputDirectory`,
`signtoolPath`, `expectedRunnerRevision` and `maximumSeconds` (1..240). Session
accepts `referenceInitialization`; compute accepts one `case`: `F32`, `GuardedQ2`,
`F32NoBlock` or `GuardedQ2NoBlock`. The native supervisor reserves 75 seconds
for owned cleanup, verifies every pinned file and catalog member, retains
immutable input handles through child exit, and records only fixed entered-stage
markers. Windows verifies the observed DriverStore signer's trust and publisher.
Same-run artifact provenance remains the caller's responsibility; no command
changes OS trust, installs drivers, loads a model or qualifies Swift inference.
All setup/intake/execution/cleanup shares the owner's five-minute total budget.
Offline `hexagon-policy` plans and evidence validators remain available.

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
drop over half a point on a blind human 1..5 rubric; three means usable and both arms must reach it. The owner may review anonymous shuffled answers. Every objective check
must pass in both arms; a failing baseline needs investigation rather than a
candidate pass. A qualified complete-task latency regression above 10% is a no-go;
no speedup or energy saving is required.

A `review` request has the same `declaration` and an `evidence` object:

- `planSha256`: the emitted plan digest. `controls`: booleans `exactPackedRestore`,
  `cancellationRecovery`, `editedPrefixRecovery`, `adapterScaleRecovery`,
  `explicitF16Rollback`, `ownedCleanup`, `noFullHistoryMirror`, and `comparableWholeTaskMemoryCoverage`, all attested against this candidate. Coverage includes startup, the complete request and retained checkpoint/restore overhead.
- `pairs`: exactly three records in declaration order, each with `id`, exact
  `prompt`, `baseline`, `candidate`, `quality` and `objectiveChecks`.
- Each arm: the complete declared `identities`; `kvCache` (`f16` or `rsq4`);
  actual distinct raw `graphIdentity` including its matching `kv-f16` or `kv-rsq4` marker; `inputTokens`,
  `generatedTokens`, `occupiedTokens`; `complete: true`; full UTF-8 `answer` and
  matching `answerSha256`; complete-task `peakPrivateBytes`, `peakKvBytes`; equal `kvObservationTokens` at the measured cache position (at least 7,168 for the deep task), and `extraFailedOrRetriedRequests: 0`;
  optional `endToEndMilliseconds`; `powerBefore`, `powerAfter`, and
  `uninterruptedPowerObserved`. Power objects contain nullable `acConnected`,
  `batterySaver`, `batteryPercent`. Use explicit null for unavailable observations.
  Occupancy allows the final sampled token to remain unconsumed. Both arms use
  identical input counts and the same predeclared output limit, with that limit and the 64-token reserve fitting inside 8K. Cache memory is compared at the same observed position; natural answer lengths may differ.
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
