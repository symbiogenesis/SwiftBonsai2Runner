# Bonsai maintenance tools

`bonsai-tools` provides protected credential creation, fixed CPU and server invocation plans,
bounded completion-process supervision and LoRA qualification scoring. It forbids unsafe code and reuses the
audited Windows boundary crates.

## Packaged use

Select Foundation's native Windows ARM64 executable from the evaluated main
revision and extract it outside Git:

```powershell
$env:BONSAI_TOOLS_PATH = 'C:\BonsaiTools\bonsai-tools.exe'
& $env:BONSAI_TOOLS_PATH credentials --request C:\Evidence\credentials.json
& $env:BONSAI_TOOLS_PATH cpu-check --request C:\Evidence\cpu-check.json
Import-Module ./scripts/Bonsai.Tools.psm1
Invoke-BonsaiTool -Command qualification -Request @{
    operation = 'file-score'; qualificationProfile = 'standard-scale-one'
    evidencePath = 'C:\Evidence\capture.json'; reviewPath = $null
}
```

Wrappers also accept `-ToolPath` where applicable. Version/protocol checks establish
compatibility, not source-to-binary attestation; use a trusted build. Without a
selected binary, the resolver builds pinned sources with `--locked --release` in
an external incremental system-temp lane. This requires Rust and locked
dependencies. GGUF inspection and export use the native `gguf` command below.

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

These wrappers require `bonsai-tools` version 0.1.0, protocol 5. The version
handshake advertises supported commands, so a selected binary must support the
requested operation before its request is written. Runtime guards remain held
through version/help probes and complete completion batches. Older protocols are
rejected before an operation can start.

## Command boundary

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
removed; invoke the existing native JSON commands directly or use the shared
`Invoke-BonsaiTool` bridge as in the examples above.

`host-capabilities` accepts `operation: "project"` with bounded `facts`, or
`operation: "identity"` with the v1 `capabilities` report. Doctor copies selected
CIM fields and UTC date strings; Rust owns validation, status, canonical ordering
and digest bytes. Doctor requires a selected prebuilt tool for these optional
details, and otherwise retains basic Windows/tool inventory without compilation.

`native-acceptance-budget` receives canonical seven-fraction UTC timestamps,
monotonic ticks/frequency, run/revision and the optional complete owner binding.
It can only shorten the original job allowance. The owning workflow charges
tool-resolution/IPC time before starting the acceptance process. Both this call
and `startup-diagnostics` require the source-bound hosted maintenance tool on
the owner device; no source build can start inside their allowance.

`startup-diagnostics` receives `evidenceDirectory`, nullable `acceptanceExitCode`
and `budgetExpired`. It reads only the fixed bounded worker logs, validates all
session keys before publication, sanitizes across sessions, and returns only
`Ready`/`Classification`. Unsafe paths or incomplete collection suppress uploads.
`ptq1-capture` supports `receipt` with bundle/repository directories, revision and
reviewed pins, and `compare-abi` with actual/expected paths and the source-bound
expected SHA-256. Neither command loads a model or qualifies numerical parity.

`native-qualification` owns one fixed numerical workload: `kind: "ptq1"`
selects one native/Rust pair at backend `auto`, `scalar` or `neon` and row-worker
count 1 or 4; `kind: "rows"` fixes the original PQ2 serial/four-worker pair.
Requests provide `repositoryDirectory` and `operation: "plan"`, `"source-pins"`
or `"capture"`. Capture additionally requires existing `dataDirectory`,
`captureToolsDirectory`, `workBudgetSeconds` in 1..240 and the original
`workDeadlineTick` from Windows QueryPerformanceCounter/.NET Stopwatch.
The deadline can only shrink; the tool never bootstraps, provisions or renews it.
Rust validates current source pins, exact hosted binary inventories and the
running maintenance executable's revision receipt, then owns memory admission,
ABI preflights, capture supervision and atomic bounded partial receipts.
Its independent Python comparator runs in an owned process under the same
remaining allowance. Model-free plan, source, deadline and receipt regressions
run in Rust; only hosted compiler preparation remains PowerShell.

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
returns nonzero. Credential receipts contain paths only. The bridge uses external
native working directories, retains failed requests and preserves inherited
Windows/profile environment variables.

`speech-models` validates the compiled native Canary and Qwen GGUF locks and
returns the selected download plan and aggregate paths/byte total without
networking or loading models. `speech-build` also owns native compiler/source
preparation, selected daemon builds and explicit model provisioning in Rust.
Provisioning uses Windows curl 8.4+ for HTTPS-only resumable downloads, verifies
exact bytes/SHA-256, then publishes exclusively. Ordinary startup size-checks
cached speech files; the native services verify their hashes before first use.

`source-build` owns content fingerprints, executable digest receipts, immutable
maintenance snapshots and the pinned Cargo invocation for source maintenance/chat
builds. Native publication writes a bounded atomic bootstrap receipt with the
snapshot digest and checked capabilities. Warm PowerShell bootstrap checks the
receipt and image digest without starting a separate version process; changed
images receive their version handshake in Rust. `-ForceBuild` can repair a
damaged bootstrap through the cold compiler path. Cache hits skip Cargo;
rebuilds use content freshness without cache cleanup. `chat-launch` owns
the source launcher's option validation, provisioning selection, credentials,
arguments and native app lifetime. PowerShell remains a bootstrap/parameter bridge.
Rust holds the persistent per-user instance lock until the owned app tree exits;
the PowerShell wrapper only probes for an existing owner before tool resolution.
Build variants use independent, flushed-and-atomically-published receipts.
Non-build documentation/helper changes preserve reuse; referenced embedded
content and configuration changes invalidate it. Damaged outputs and incomplete
receipts never count as ready. Source-cache entries publish only after full
verified writes, and bad completed download partials get one fresh retry while
retaining diagnostic bytes.

Startup commands additionally advertise `StartupProtocolVersion: 1`. Their
optional `parentProcessId` and `parentCreationTimeFileTime` fields bind long work
to the caller's retained Windows creation identity. PowerShell supplies these
fields automatically; selected older tools receive current-build guidance.
Compiler/tool descendants belong to an atomic Windows Job with an inherited
Unicode environment and child-only overlays. Cancellation waits for the tree to
exit before build-lock release; owner death closes the Job. Model memory and
process policies remain separate and unchanged.

`ChatLaunchProtocolVersion: 1` identifies native instance-lock ownership;
PowerShell rejects older launch tools before sending a request.
`BootstrapProtocolVersion: 1` identifies native maintenance snapshot publication.
These additive contracts leave the general JSON protocol at version 5.

`chat-package` owns `create`, `inspect`, `install`, `rollback`, `installed` and
`start` operations. The [package contract](../../docs/chat-packages.md) defines
their JSON requests. New schema-2 packages contain only five native executables
and three notices; the PowerShell packaging module, launch/install wrappers and
their tests are retired. Rust retains schema-1 package inspection and rollback.
Run the command using an independently trusted maintenance executable: package
hashes cannot authenticate the program performing the checks. `start` verifies
the active package and derives its fixed executable paths before native launch.
The legacy `launch` operation still supports a caller-bound directory, ID and
manifest digest. Caller options cannot override paths or request a source build.
The native launcher opens the pending UI before preparing backend and speech.

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
bundle. `Get-Inference.ps1` keeps its separate provisioning path without a
maintenance-tool or Cargo dependency.

`Get-Inference.ps1 -IncludeCapabilities` additionally invokes
`inference-capabilities` to verify the compiled release archive and every
installed file, retaining their read handles through an owned ten-second,
512 MiB model-free `--capabilities` probe. The exact v1 report is parsed with
the shared CPU-policy contract and must match the pinned version and native
Windows ARM64 target. Each output stream is limited to 16 KiB; stdout remains
separate from stderr. The result adds `Capabilities` and `CapabilityVerification`,
including the retained evidence directory. No model is loaded or profile selected.
This option requires a trusted current maintenance tool (or the normal source
build fallback) and rejects custom locks. Ordinary provisioning is unchanged.

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
PowerShell downloads the archive, publishes staging with its existing
no-overwrite directory move, then calls `runtime-verify`. Staging receipts do not
retain handles across that boundary; the published runtime is verified again
before probes or execution.

Completed CI samples publish a single `capture.json` containing exact response
text, its UTF-8 SHA-256 and diagnostics. Rust decodes stdout strictly and retains
leading U+FEFF and trailing newlines. `diagnostics.json` remains available for
inspection; the completed `supervision.json` is published after the capture
records. PowerShell joins these records without rereading native stdout. Failed
and incomplete samples never contribute completed response records to scoring.

Qualification scoring reads bounded original evidence/reviews directly.
[Committed fixtures](../../tests/fixtures/lora-qualification-v1.json) bind control
serialization, Unicode response hashes and accepted/pending/rejected reports.
Missing human reviews, native logits or basis evidence remain pending; known
regressions still reject. Scoring never launches inference.

Frozen pre-port PowerShell fixtures cover complete CPU policies, artifact identities
and argument vectors, including Unicode prompts and adapter scale syntax. Pure
policy tests run in Rust; focused script checks cover the packaged JSON bridge.

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

PowerShell retains OS signature observation, held file handles and owned worker
lifecycles. Its policy adapters require `BONSAI_TOOLS_PATH` from the source-bound
hosted build and never bootstrap Cargo on the owner device. Hexagon hosted
controls run the native policy regressions and publish a revision/digest-bound
maintenance artifact. Session/dispatch owner jobs verify that same-run artifact
inside the existing aggregate setup clock. Rust tests replace the former
PowerShell build/result/plan tests; `Test-HexagonCapture.ps1` keeps only the
remaining PowerShell pipe-capture and progress-retention checks.
