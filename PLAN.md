# Swift Bonsai 2 Runner plan

This is the single backlog. [Technical docs](docs/README.md) own current behavior,
contracts, commands and evidence; completed work belongs in Git history.

PTQ1 default promotion is a **no-go under the current evidence**. Follow the
[release decision](docs/swift-memory.md#release-decision) and close the required
adapter, exact-source application, device/human and benefit gates below before
reconsidering it. Keep historical 0.1.13 scores separate from 0.1.14 acceptance.

Use native Rust Prism 0.1.23 (`055c7430cb4771dfddb24ddaa30494af6ad182da`) as the project's
development default: original Swift PQ2, four CPU row workers, the standard adapter at scale 1.3,
and default Snapdragon X2 Hexagon NPU `native-q2` resident acceleration
(`DEFAULT_NATIVE_Q2_PROJECTIONS`, all 168 `PQ2_0` projections across the first 27 layers
`blk.0..=blk.26`, `2,888,663,040` bytes / `2,754.84` MiB across six `<= 480` MiB `FastRPC` weight
arenas pre-warmed at worker startup with automatic 4-worker NEON CPU fallback when the signed
Hexagon runtime package or CDSP driver is absent). Preserve explicit publisher rollback with
`--publisher-engine` (`-PublisherEngine` in Start-Chat.ps1), original-PQ2 selection and unchanged
artifact, sandbox and resource admission. Keep PTQ1 opt-in. This owner-directed development choice
does not assert complete release qualification or a measured advantage; retain the disclaimer and
pending evidence below. No failure may switch engine or representation automatically.

Prioritize usable native startup, model readiness, stop/retry and explicit rollback,
then actual-model application acceptance, daily-service isolation, creative
quality and comfort. Research order:
measure batch-one bottlenecks and PTQ1 suitability; improve packed CPU projections and
prefill; qualify exact prefix reuse and short-chain speculation; then consider precision
changes or compatible drafters. Bounded GPU/NPU access discovery can run in parallel.
Automatic profiles require device qualification; broader Prism work needs a Swift
dependency or chosen milestone.

The chosen parallel accelerator milestone is native Rust Prism Hexagon promotion:
build on the passed X2 live-model projection through adapter, lifecycle,
resource and benefit acceptance, then qualify measured CPU/GPU/NPU selection.
Qualify X1 independently when competitive; neither family names nor nominal
TOPS choose a backend or displace the CPU gates below.

The long-term migration target is complete C++ retirement in Runner and Prism,
including reference tests, generated code, external inference dependencies and
compiler/build plumbing. Follow the [file-by-file contract and inventory](docs/cpp-retirement.md);
native reference controls are temporary, with independent evidence retained as data.

The next development milestone is reliable daily use of the Rust/PQ2 default on
the owner-managed hardware. Complete focused source checks, qualify an explicit
DX12 launch after the observed Vulkan renderer failure, and retain source-bound
model readiness, stop/retry and manual publisher rollback evidence within the
existing owner budget. Hosted checks and device workloads remain owner-operated.
Fix observed blockers before pursuing a wider qualification matrix.

A fully qualified Swift current-weight PTQ1 CPU default with the standard adapter
at scale 1.3 remains a separate release milestone. Retain original PQ2 as its explicit
rollback. Close the application, isolation, creative-quality, resource and installation
gates below with reproducible evidence before that promotion. Missing adapter authoring
inputs and incomplete benefit/quality evidence do not block the Rust/PQ2 development choice.

The PTQ1 critical path is:

1. Run the [one declared public-pack reconstruction](docs/adapter-public-reference.md)
   of all 48 approved SSM A vectors on hosted CI, then independently replay its
   retained comparisons with the offline scalar auditor. Public source describes an FP16
   expansion of the immutable ternary pack, so reconstruction has a concrete route;
   no successful payload result is claimed. Separately obtain/authenticate the
   original authoring inputs through the bounded schema-2 `basis-intake` contract
   and verify native writer mappings. Independent reconstruction cannot establish
   the author's original checkpoint, invocation or toolchain provenance.
2. Qualify the selected worker policy's scalar/explicit-NEON logits and live state,
   loaded-zero identity and nonzero application beyond the
   [passed bounded four-worker scale/history sequence](docs/evidence/ptq1-0113-lifecycle-41539984210/independent-review.json).
   Extended lifecycle and prefix-state reuse remain separate.
3. Retain matched complete-answer benefit, per-prompt adapter throughput and
   independent human quality/comfort review, then close full context/output,
   resources, isolation, recovery and installation while retaining verified
   original-PQ2 rollback.
4. Independently score every gate against settled source/release/app identities
   before promoting PTQ1 or claiming fully qualified runtime/representation defaults.

The separate PTQ1 qualification packet freezes the published Prism 0.1.14 CPU
runtime and one settled Runner revision. Complete hosted Runner integration and source-bound
acceptance preparation with `run_device: false`, which leaves the laptop lane
available and queues no owner job. Review that exact-source package before any
explicit device selection. The [fixed combined-packet estimate](docs/evidence/native-owner-packet-controls-20261103/owner-workload-plan.json)
is 450 seconds including all setup, three startups, requests, cleanup, hashing
and retention; its smallest supported workflow allowance is ten minutes.
Have the owner predeclare that allowance before running
`ptq1-adapter-service-rollback-row4` once, stopping early when done or infeasible.
Reuse one PTQ1 owner for scale/count/context controls and
retain the required fresh owners for interruption replay and original-PQ2
rollback. Do not remove those independent-owner checks to save startup time.
Archive the complete raw receipts and independently review them before another
model workload. The old 0.1.13 six-pass score cannot qualify this candidate.

Keep expensive work off the laptop where possible: prepare exact tools on hosted
ARM64, reuse verified immutable downloads, replay retained numerical data offline,
and run the one declared public-pack reconstruction before considering any new
authoring-basis recipe. Combine evidence collection within the selected lifecycle
workload, but leave complete-state parity, full context/output, paired benefit,
human quality/comfort and clean-machine acceptance independently pending. Missing
original adapter authoring inputs remain an external prerequisite.

## Research scope and practical budgets

Follow the [research scope, budgets and experiment
checklist](docs/research-register.md#experiment-scope-and-budgets): existing native
Windows ARM64 X Elite/X2 Elite hardware, independent family qualification and a
five-minute preferred/default budget for owner-device diagnostics, model trials and
benchmarks, including self-hosted CI. A predeclared dedicated workload may use up
to fifteen minutes total only when its supported harness and conservative estimate
require it; select the smallest allowance and stop as soon as its required work
finishes. Preparation, every arm, power observations, cleanup and archival share
one clock. Preserve historical trial limits and the smaller limits in harnesses
that have not implemented an explicit extension. Never restart a clock or divide
a long matrix across repeated runs. Longer or infeasible qualification stays pending.
Apply the [promotion
criteria](#promotion-criteria) before full qualification, representation promotion
or changes to admission. The Rust/PQ2 development choice above retains those
pending gates. Full suites, benchmarks and performance captures stay in owning CI.

## Application and daily service

### Actual-model acceptance

Qualify the [live conversation and window persistence fixes](docs/evidence/chat-save-window-20261104/receipt.json)
on settled main through owner-managed hosted checks and native application acceptance.
Verify a normal `Start-Chat.ps1` launch retains drafts, conversations and completed
model history while model artifact guards are active and after closing/reopening,
honors a persistent explicit saving opt-out, and never automatically replaces
unreadable recovery or replays interrupted requests. Verify ordinary and maximized
window bounds across close/reopen, minimized close, display/DPI changes and a removed
monitor; geometry saving remains independent of conversation opt-out.
The local run passed four artifact/encryption controls, then stopped before UI tests
on two redundant name qualifications in the new geometry test. Those names are
corrected and independently source-reviewed but not recompiled. The owner explicitly
retains follow-up validation and rebuilding; maintenance refresh and native builds
remain pending, with existing installed executables unchanged.
Focused model-free controls and native compilation do not close actual-model recovery
or daily-use acceptance; keep that existing owner work pending.

Resolve the installed broker's earlier graceful-shutdown failure through the
required source-bound private owner-shutdown controls and actual application
acceptance. The separate task-specific owner used for the
[separated token timings](docs/evidence/token-timing-20261103/summary.json) stopped
all nine started Jobs explicitly, but does not qualify the installed broker or
GUI Stop/Retry. Preserve the [earlier failed capture](docs/evidence/local-throughput-20261103/summary.json)
and both original closed clocks; do not transfer the new owner's cleanup pass.

Diagnose the publisher PQ2/PTQ1 startup failures before selecting further
publisher timing work. Both pinned backends exited before HTTP readiness; the
empty logs and omitted backend exit codes leave the cause unknown. Retain those
failures without retrying the capture. The pinned Qwen reference model is absent
from the selected installed data and remains unmeasured. All seven installed
Rust profiles now have short-window, client-observed TTFT and generation rates;
they do not close sustained kernel throughput, repeated-trial benefit, quality,
full resources or application acceptance. The user-requested matrix, one offline
Release owner build, power observations, cleanup and final archival consumed
473.890 seconds of one predeclared fourteen-minute allowance, with no repeated arm.

Qualify sustained Prism 0.1.22 performance, repeated benefit and full application
acceptance beyond the [November 9 short-request capture](docs/evidence/readme-performance-20261109-attempt2/summary.json).
It supplies two eight-token public-broker observations with confirmed cleanup,
not sustained decode or effective NPU execution. Keep the observed power conditions
and first-request preparation scope attached to the README numbers; do not infer
CPU/NPU placement from HTTP readiness or a controlled cross-version speedup against
the historical direct-backend capture. Preserve the
[earlier preparation failure](docs/evidence/readme-performance-20261109/summary.json),
its original closed 69.432-second clock and AC/saver-off/99% observations separately
from the explicitly authorized new attempt's closed 59.250-second clock and
AC/saver-off/98% observations. Neither clock authorizes a further trial or retry.

The separately authorized [fresh 0.1.22 refresh](docs/evidence/readme-performance-20261109-refresh/summary.json)
stopped at the driver's explicit source-path guard before power, build or model work.
Its aggregate endpoint and power remain unknown; the tool's 0.7794621-second process
wall time is a separate observation. Keep that attempt closed and retain existing
README figures until a successful new-version capture.

The owner now requests Prism 0.1.23 promotion, a real binary repin and then performance
measurement. The [final-source registry check](docs/evidence/prism-0123-registry-20261109/summary.json)
passed six receipt controls and formatting, then stopped on eight test-only strict
Clippy errors before packaging or upload. Preserve its closed 52.8568851-second
clock,71 AC/saver-off/99% observations and verified archive; no retry. Complete
corrected-source validation through the separately authorized bounded hosted
Foundation bundle, then normalized package verification and actual registry upload.
The [first bounded Foundation operation](docs/evidence/foundation-bundle-0123-20261109/summary.json)
pushed the exact authorized Prism source `a7cdee9`, then stopped on an operator
run-name identity error. Its automatic and dedicated runs are both confirmed
cancelled, with cleanup observed137.31 seconds from the original start inside
the300-second deadline; preserve the original raw failure and separate cleanup
supplement. The correction passed six offline fixture controls in0.149 seconds.
The separately approved [source-ready attempt](docs/evidence/foundation-bundle-0123-20261109-attempt2/summary.json)
passed three version controls, six receipt controls and formatting, then failed
strict Clippy on four additional test-only empty assertions in the Hexagon helper.
Its202.043-second clock is closed with confirmed terminal cleanup and146 verified
raw archive members. A separate1.659-second read-only terminal-log review retains
the diagnostics; it did not restart CI or reopen that clock. The four assertions
are corrected and source-reviewed locally, with production behavior unchanged.
The explicitly approved [corrected-source Foundation operation](docs/evidence/foundation-bundle-0123-20261109-attempt3/summary.json)
pushed `055c743`, passed nine version/receipt controls, formatting, strict Release
all-target analysis, portable library compilation, production linking and help
smoke checks. Its296.114-second clock is closed within the six-minute exception,
with both owned runs terminal and all217 raw archive members independently verified.
All415 power observations reported AC connected, saver off and99% charge; they
establish no uninterrupted stability. The genuine nine-file bundle, five ARM64
executables and743 canonical source blobs match the source-bound producer receipt.
The standing five-minute default and all earlier closed clocks remain unchanged.
The separate [registry publication](docs/evidence/prism-0123-publish-20261109/summary.json)
passed default Cargo package/publication verification and confirmed the public
0.1.23 checksum in40.064 seconds of its300-second allowance. Only the runtime
crate changed; all five registry helper versions remain unchanged. The separately
approved [GitHub prerelease delivery](docs/evidence/prism-0123-github-20261109/summary.json)
created only `v0.1.23` at `055c743` and verified all three uploaded asset digests
before and after publication in14.677 seconds of120. Runner selects the verified
nine-file binary pin. Complete the declared new-version capture before changing
README measurements. Keep full Foundation and application acceptance independently pending;
do not reopen the failed0.1.22 attempt or claim source reductions as measured gains.

Qualify the [shared startup-timeout receipt fix](docs/evidence/ptq1-startup-timeout-20261103/summary.json)
through owner-managed exact-source hosted preparation. An admitted extended case
whose deadline outlasts the unchanged 600-second startup cap now retains an ordinary
startup failure instead of unwinding before terminal evidence. Package preparation
requires the new paused-time regression. The one new local check stopped on a
multiline formatting difference before parsing or Cargo execution; the reported
correction is applied but uncompiled. Preserve its failed 0.706-second receipt,
closed clock and original AC/saver/charge observations without retrying it.
No model, public reconstruction, provenance, benefit or human gate closes here.

Qualify the [PTQ1 blocker fixes](docs/evidence/ptq1-unblock-20261103/summary.json)
on settled main through owner-managed hosted preparation. Require the new shared
typed admission guard and source-bound workflow-contract receipt before publishing
or admitting a schema-2 acceptance package; older prepared helper selections remain
stale. Run the one declared public reconstruction with its exact retained replay
source, and qualify response identities through failed/cancelled service reads.
The new source's thirteen focused local regressions, workflow parsing, guard build,
self-check and guard-library analyzer passed within one closed owner allowance;
full application analyzers/linking, the NumPy producer control, hosted execution,
actual models, original provenance and human acceptance remain separate pending
gates. Earlier failed trials retain their original results and closed clocks.

Qualify the [hosted native-link and renderer gates](docs/evidence/native-hosted-readiness-20261103/summary.json)
on settled main through owner-managed hosted preparation with `run_device: false`.
Publication and owner intake now require successful exact-source Release `clippy`
and production `build` receipts covering chat, broker and maintenance tools, plus
four exact renderer/default/deferred-rollback controls before package creation.
Independent source review confirms the CI batching and receipt schema; it supplies
no compiled verdict. The one local workflow-contract check stopped before any
parser or fixture executed because PowerShell blocked its generated `-File`
script under the machine execution policy. Retain that failed 0.848-second clock,
raw script/fixtures and original battery/saver observations without retrying it.
Parser, receipt-fixture, compiled regression, native-link and strict-analyzer
acceptance remain pending; the existing installed executable is unchanged.

Qualify the [daily-readiness source changes](docs/evidence/native-daily-readiness-20261103/summary.json)
on settled main through owner-managed hosted checks. Explicit
`--graphics-backend dx12` / `-GraphicsBackend dx12` now selects only the native
window's renderer; omission preserves inherited selection. The separate
`pq2-adapter-service-row4` profile prepares original-PQ2/four-worker Stop and
fresh-owner replay without PTQ1 metadata. The UI's private broker shutdown
handshake requires a model-cleanup acknowledgment, broker exit and outer-Job
cleanup before Retry; PSEC models can bypass outer Job inheritance. Qualify this
actual UI path separately from the direct broker acceptance harness.

The one new local model-free check applied canonical formatting and parsed the
launcher plus eight workflow PowerShell blocks, then stopped on a renderer
environment-map type mismatch before any test executed. The source correction
matches the existing String-map process contract but remains uncompiled. Keep
the failed 15.411-second clock closed, its original battery/power observations
and independently verified raw evidence intact. Focused controls, strict Release
all-target analyzers, native linking and maintenance delivery remain pending;
no installed executable changed. After exact-source hosted acceptance, retain
one feasible owner-observed DX12 launch, model readiness, Stop/Retry and manual
publisher rollback workload under the existing aggregate budget. No previous
failed clock or validation pass may qualify these new changes.

Validate the [Rust development-default integration](docs/evidence/rust-development-default-20261103/summary.json)
on settled main in owner-managed checks. Its one model-free local run passed thirty
focused selection, handoff and qualification-profile controls plus the maintenance
Release build, then stopped on three strict analyzer findings. The small source
corrections are applied but uncompiled; final all-target analyzers remain pending,
including chat, which was not reached. Preserve that closed 73.123-second clock and
its original power observations. The development selection does not close actual-model
acceptance, and no existing maintenance installation was replaced by that failed run.
Keep the separately recorded local delivery's tested executable identity distinct
from the later source-only analyzer corrections; it supplies no additional test verdict.

Qualify the [owner-packet controls](docs/evidence/native-owner-packet-controls-20261103/summary.json)
on settled main through owner-managed hosted preparation: explicit connection
refusal must confirm listener release, and the PTQ1 sequence must preserve the
separate seventy-five-second original-PQ2 tail. Exercise both new exact controls
and the native linking/strict Release analyzers before owner admission. The one
new-source local verification parsed seven workflow PowerShell blocks, then
stopped on a null formatter-process exit observation before any Cargo invocation.
Its failed receipt and closed clock remain retained; compiled checks are pending
and must not be retried as a continuation. Retain independent aggregate power
observations before setup and after final retention for the next owner trial;
Rust model-case and per-owner endpoints cannot establish the earlier setup conditions.

Qualify the [source-bound acceptance preparation](docs/evidence/native-primetime-preparation-20261103/summary.json)
on settled main. Hosted preparation now requires exact-revision strict Release
all-target Clippy for native chat, broker and maintenance tools, retaining a
digest-bound analyzer receipt before owner-device admission. Exercise the new
failed-cleanup receipts and shared absolute deadlines through the prepared
service/rollback controls. The new local model-free check stopped on a formatting
difference before any Cargo invocation; the reported correction is applied, but
compiled tests/analyzers remain pending. Its failed result and original closed
clock are retained; do not restart it or transfer earlier validation passes.

Complete the strict Release UI analyzer left pending by the
[bounded local readiness validation](docs/evidence/prism-0114-primetime-validation-20261103/retention.json)
in owning hosted checks. Preserve its original 570-second work cutoff and failed
600-second retention budget; do not reopen that source-validation session to finish
the analyzer. The local checks and verified artifact repair do not close actual-model
acceptance or replace a source-bound hosted package for the next owner workload.

Qualify the Rust-only maintenance migration through settled-main hosted ARM64
Foundation, released-runtime provisioning, source-bound collector/candidate builds,
speech static-cache preparation and signed Hexagon worker integration. Focused
model-free checks and removal of PowerShell/C# implementations do not qualify
actual devices, models, startup performance or promotion; retain the existing
bounded owner-device gates and exact source/release identities below.

Qualify source-launch time to the first native frame and authenticated model
readiness after excluding worker checkouts from startup fingerprints and switching
shared Windows artifact hashing to CNG. Retain cold-build versus unchanged-launch
attribution and complete checksum/cancellation controls. Qualify the CNG verifier
retained from Prism 0.1.13 in the published 0.1.14 release through Runner's
settled-main release integration and
actual-model startup. End-to-end startup gains remain pending within the
owner-device budget; performance comparisons and complete checks remain in owning
CI. Complete separate registry delivery for the reviewed native-hash, GGUF 0.1.1
and Prism 0.1.14 packages; hosted source-package checks do not publish them.

Qualify an explicit DX12 launch after the owner's
[repeated native renderer failure](docs/evidence/native-renderer-20261103/summary.json).
The retained diagnostics select Vulkan on Adreno X2-90 and report device loss
with a driver-fault message before the GUI upload panic. This attributes the
failed renderer path but does not establish the precise driver defect or infer
anything about inference. Backend-aware launcher guidance is prepared; compiled
checks and a restored launch remain separate pending gates. Preserve the
[earlier failure](docs/evidence/native-renderer-staging-failure.json) and its
unknown backend/device-loss details rather than backfilling them.

Retain hosted regression coverage for Rust acceptance deadlines, startup receipt
retention/redaction, host capability identities, PTQ1 collector admission, Find
replacement/Unicode matching and speech-runner policy before owner-device trials.

Qualify Rust speech tokenization/loading, greedy sampling, decode-workspace
ownership and compute leases in actual pinned-model lifecycle and listening
trials. Retain hosted layout/Find invalidation and speech scratch/error/owner
release coverage, including complete native linking. The earlier model-free
owning build/analyzer/integration obligations are recorded in
[main integration evidence](docs/evidence/prism-main-integration.json); they do
not replace actual-model application acceptance. Preserve the captured Windows
ARM64 MSVC exceptional-float behavior when qualifying other compiler versions.
Measure adaptive broker forwarding and retained idle speech scratch before
claiming a latency, throughput or process-memory gain.

Extend numerical qualification beyond the historical
[passed one-token explicit-NEON capture](docs/evidence/ptq1-numerics-41568281287.json):
all 129 files / 39,504,384 logits and complete live-state values match byte for
byte on X2 Elite Extreme with four row workers and the standard adapter at 1.3.
The source-built checker uses pinned Prism 0.1.13; this does not exercise the
released private HTTP service or qualify full 8192-token occupancy, broader
trajectories, scalar, loaded-zero/no-adapter identity or nonzero effect size.
Retain the separate source-built/checker and released-service identities and
select one independently useful workload within the shared owner budget;
do not continue a partial matrix through further owner runs.

Preserve the [closed pre-registration no-go](docs/evidence/ptq1-numerics-41565247997.json)
and its unknown original failure classification. The later exact-name inventory
correction and strict Rust receipt checks address a reproduced source defect;
they do not identify the discarded original exception. The successful new session
preserves its own original clock through a Python executable-lookup correction
before any child started, then one package intake and one native/Rust model pair.
Its owner cleanup finishes at second 108.553 and hash-verified archival at second
131.506, plus a conservative 40-second prior charge. The original independent
payload review remained incomplete after review-only path errors; the separate
[passed hosted retained-data audit](docs/evidence/ptq1-retained-audit-41571150128/independent-review.json)
checks all 155 captured files and replays the original comparator without another
model trial.
Keep execution, retained-data review and full promotion verdicts distinct.
The [Prism 0.1.13 four-worker sequence](docs/evidence/ptq1-0113-lifecycle-41539984210/interpretation.json)
now passes all three cap-one streams, invalid-scale rejection, same-owner reuse
after rejection and captured assistant-history binding within 268.464 charged
seconds. It does not qualify numerical parity, nonzero effect size or prefix-state
reuse. Obtain the matching numerical capture with `native_row_workers: 4` when
selecting that policy; do not continue a partial matrix through additional owner jobs. Retain
the [pre-model row4 no-go](docs/evidence/ptq1-delivery-20261101.json) separately:
an existing user chat was observed, no owner runner or model was started, and the
original clock was not restarted. Close the application before a future bounded
owner session; do not stop an existing user session as qualification cleanup.
Extend the bounded resource observations toward full-context and sustained use, distinguishing
OS lifetime Job commit, root working-set residency and sampled serving bounds; these
do not replace full-context, paging or sustained-comfort acceptance. Retain
exact generated source-response/message digests; empty or unbound output leaves
generated history pending. The current four-worker case observes 14.622-second
startup and 26.038/25.753/55.558-second requests. Attribute backend work before
optimizing a hotspot; these observations and older one-worker timings are not a
controlled speedup comparison. Retain bounded
live request/partial-prefix receipts; observed SSE delivery is not backend kernel
timing or decode TPS, and completed-operation counts do not prove zero emitted tokens.
Extend the [passed four-worker complete-response feasibility pair and original-PQ2
rollback](docs/evidence/ptq1-rollback-41573164864/independent-review.json) toward
independently reviewed benefit within a feasible aggregate allowance. The single
fixed-order tree-name pair returned identical complete answers but observed a
1.944 PTQ1/PQ2 request-latency ratio and lower bounded PTQ1 Job commit; retain the
losing outcome. AC/battery status was not captured, so performance comparison is
ineligible. Future laptop trials must retain power observations for each arm;
missing or mismatched power conditions cannot qualify timing or resource benefit.
Repeated interleaving, stable-device conditions, per-prompt adapter decode
throughput and independent complete-answer quality remain separate. Do not
transfer evidence between worker policies or repeat a matrix to bypass the shared
device budget. The completed session used 192.487 charged seconds through archival;
its clock is closed and no additional owner workload may extend it.
Source guards, [hosted build/ABI
controls](docs/evidence/ptq1-collector-hosted.json) and the prepared profile do not
close actual-model logits/state, writer-basis or promotion gates. Keep intake,
verification, model work and cleanup inside one aggregate five-minute owner allowance;
the [default-readiness audit](docs/evidence/ptq1-default-readiness.json) separates
historical passes from current-release qualification.
Qualify the [published Prism 0.1.14 release](https://github.com/symbiogenesis/prism-inference/releases/tag/v0.1.14)
from exact source `9e8ea3e2c042ab0c3f1a8e9596ff155e202c3c57`. The
[publication review](docs/evidence/ptq1-context-publication-014/independent-review.json)
verifies the tag and all four uploaded assets against owning CI. Complete Runner's
owning integration checks against the [prepared pin identities](docs/evidence/prism-0114-pin-preparation.json). Its typed token-capacity
error preserves the private context-rejection contract; unknown counter failures
remain failures. Hosted source checks do not establish actual-model behavior.
Retain `ptq1-adapter-service-row4` and the
[reviewed three scale-boundary streams and invalid-scale rejection](docs/evidence/ptq1-service-41577882160/independent-review.json).
The separately prepared `ptq1-adapter-service-rollback-row4`
[workload](docs/validation.md#actual-model-application-acceptance) combines the full
five-request PTQ1 service sequence with fresh original-PQ2 rollback on the same
0.1.14 runtime, adapter 1.3 and four workers. It requires context rejection,
active interruption, both PTQ1 owners reaped, seeded fresh-owner replay, a complete
original-PQ2 answer and post-cleanup full-file CNG verification. Its third-owner
floor is 75 seconds, retaining 30 seconds for cleanup and 15 for file verification
within the same absolute deadline. Every 0.1.14 model, service and rollback
qualification remains pending; missing work cannot qualify the combined case.
The earlier session is permanently closed; do not continue its missing steps
through another owner job.
Preserve per-owner power observations and distinguish missing child endpoints from
the retained parent endpoints. Visible output and matching prompt counts do not
establish numerical adapter effect, full-state parity or full promotion.

Use the [fresh 0.1.13 PTQ1 promotion score](docs/evidence/ptq1-promotion-bb020f4-review.json)
for frozen Runner `bb020f4`, current-weight PTQ1 and four workers: six independently
reviewed documentary/packing/rollback gates pass and thirteen remain pending. Its retained
input, policy, raw compiled score, exact CI records and independent review bind the
current runtime and candidate. The [older 0.1.10 score](docs/evidence/ptq1-promotion-85fd2f1.json)
remains historical; later application identities need their own applicability review.
Close the thirteen required gates using actual device, independent human and
clean-machine acceptance rather than counting narrow numerical/lifecycle checks
as complete broad gates. The [fresh authoring-input check](docs/evidence/ptq1-authoring-inputs-20261102.json)
still finds no published original checkpoint or adapter-bound conversion records.
The later [source reassessment](docs/evidence/ptq1-public-authoring-route.json)
identifies an independent reconstruction route from the immutable public pack;
its [single-recipe hosted auditor](docs/adapter-public-reference.md) still needs
actual all-48 output and independent review. Matching public A or B factors cannot
authenticate the missing original invocation.
Close its actual-model, adapter provenance, paired benefit, quality, resource,
isolation, recovery and installation gates before promoting PTQ1 or removing
the README's adapter qualification. Later application identities need their own review.
Preserve the 300-second aggregate owner ceiling, original clocks and model cleanup
reserves; never restart a clock or continue a partial matrix. Obtain the approved
adapter's immutable original checkpoint, conversion and direction identities and
independently verify all 48 SSM mappings. Original-run authentication remains an
external provenance prerequisite; public reconstruction and native/Rust agreement
must retain their separate scopes.

Retain the [historical four-worker startup](docs/evidence/ptq1-delivery-0112-20261101.json)
when planning a current-release owner workload. This one 0.1.12 attempt reached exact
PTQ1/adapter readiness and observed the effective four-worker graph budget, but
startup consumed 100.557 seconds of the 164-second model case. Its first request
expired after 33.445 seconds with no public headers or response bytes; none of its
three required streams completed. Owned cleanup and independent receipt review
finished within the original allowance, with no retry. Attribute broker startup
phases before claiming a hashing, loading, worker-policy or kernel hotspot; missing
public bytes do not establish zero backend tokens. Broader/full-context work still
needs a feasible workload or a faster qualified runtime, rather than split trials.

Retain the startup change `0da7bcf` delivered in the
[reviewed Prism 0.1.13 release](docs/evidence/prism-0113-release-review.json): graph construction retains
one complete payload-validation pass and removes duplicate scans when binding
projections, preserving malformed-payload rejection, including invalid unused-MTP
tensors. Valid unused tensors remain allowed. Its exact-source
Foundation, conformance and source-package checks passed. The bounded current
case observes 14.622-second startup, including 8.135 seconds of broker model
verification and 5.596 seconds awaiting authenticated readiness. These brackets
do not isolate backend loading/graph/kernel work or prove a controlled gain.
Qualify the explicit `capture_mode: request-timing` source diagnostic on the
owner device only after its owning hosted checks pass. Its native Rust launcher
now binds the exact checker and public input, parent ownership, Job/admission
limits, original monotonic deadline and 32-second verification/cleanup reserve.
It runs one standard-adapter PTQ1 source-session request with one reserved output
token, separately from the numerical pair. Validate actual operation attribution
before optimizing a hotspot; source timing does not qualify HTTP/decode throughput,
isolated kernels, complete answers or a default. Missing final receipts leave
work unknown, and no timing trial may extend the closed numerical session.
The independently reviewed
[published-direction result](docs/evidence/ptq1-direction-41530766802/independent-review.json)
checks all 129 B factors and 660,480 channels with zero numeric difference.
Independently reconstruct all 48 A factors through the predeclared public-pack
route, authenticate original checkpoint/conversion records separately, and verify
native SSM order; direction agreement does not close those gates.

Qualify the pinned Prism 0.1.13 release beyond its bounded four-worker scale/history
sequence before using it for promotion. Retain the 0.1.12 readiness and historical
0.1.10 configured-worker observations at their original identities. Retain the previously released
copy-then-scale recurrent path until the fused source candidate has actual-model
numerical, throughput and resource evidence. Exercise the selected numerical
adapter condition explicitly
(`standard`, `loaded-zero` or `no-adapter`), without turning separate owner runs
into a control matrix. One native/Rust condition pair does not close loaded-zero
identity or nonzero application across conditions.

- Qualify startup, supplied-history streaming, full 8K context and 4K output, long
  conversations, speech, offline restart and recovery. Cover Stop, abandonment,
  deadlines, memory pressure, owned-process cleanup and explicit retry without replay.
  Qualify the opt-in bounded supplied-history and `stalled_consumer` workloads on
  X2 within the shared five-minute total budget under the [acceptance
  contract](docs/validation.md#actual-model-application-acceptance), charging checkout,
  hosted-tool intake and verification against the actual remaining case allowance.
  Skipped requested workloads remain pending; bounded history does not close
  long-history acceptance. Retain context
  rejection, same-instance reuse, termination and usage, Stop, Job and listener cleanup
  and fresh-owner evidence, including completed optional substeps when a later
  request or nonreading pause is cancelled. Fixture size and output reservations
  do not prove exact occupancy or generation; stalled forwarding does not prove
  socket saturation or an active backend at Stop.
- Qualify [transcript and Find](docs/streaming-resources.md#native-chat-and-transcript)
  through appends, replacements, conversation switches and the 512-message limit,
  including retry-heavy sessions and saturated results, cached matches, highlights,
  active underlines, Unicode wrapping, query, width and scale changes, selection
  preservation and recovery. Include queued Find navigation after shortened or
  same-length response replacement and eviction, and identical Unicode query/body
  matching through streamed appends.
- Qualify speech after X1-to-X2 data-directory moves and CPU-feature or Windows changes;
  model-free cache checks are insufficient. Prepare a reduced actual-model workload
  on hosted CI with one aggregate owner-device setup/loading/comparison/cleanup
  allowance; the full TTS comparison remains hosted-only. Review Windows assistive
  technology, keyboard and focus, scaling, high contrast, minimum-window layout
  and multilingual input.
- Complete [signed distribution acceptance](docs/chat-packages.md): binary provenance,
  dependency and license notices, clean-machine ARM64 Visual C++ prerequisites,
  installation, upgrades and rollback.

### Isolation, transport and recovery

- Establish a [restricted inference identity](docs/sandboxing.md#worker-authority):
  read-only approved artifacts, private bounded cache and temp storage, no unrelated
  file or credential access, denied egress and no same-user backend bypass. Qualify MXC
  native loading, private storage and broker-only communication without broader host
  loopback authority. Preserve atomic Job limits, exact worker identity and [native
  integration](docs/sandboxing.md#native-integration-contract), with no weaker fallback.
- Qualify the trusted-host artifact ancestor sharing correction and its nested
  rename/DELETE regression through owning native/AppContainer checks. Preserve
  the contained reader's narrower metadata rights and root exclusion; trusted
  host guards must outlive every contained reader. Final-source integration is
  pending under the [local implementation review](docs/evidence/ptq1-local-implementation-20261103.json).
- Bound worker scratch, PSEC-private temporary storage and stdout/stderr disk usage;
  define retention for diagnostics and credentials across retries. Qualify storage
  exhaustion and owner-loss cleanup while preserving evidence needed for review.
- Qualify the [broker security and transport contract](docs/broker-policy.md): numeric
  loopback, exact Host, Origin and routes, private credentials, rejected tools/MCP,
  dynamic loading and capability overrides, bounds on queues, bytes, tokens and
  duration, cancellation on disconnect or stalled consumption. Exercise the global
  8-request burst, 1/second refill, absolute deadlines, complete HTTP 429 responses
  during writes, constant-space draining, reuse and explicit resend without replay.
- Extend [streaming resource contracts](docs/streaming-resources.md) to actual-model
  device trials: ready-prefix delivery, long transcripts, speech backlog and chat
  overlap, PCM/WinMM reuse, borrowed-PCM uploads, native-owned WAV synthesis and direct
  final-storage HTTP intake. Cover exact fragmented-header limits, maximum WAV fallback,
  buffered prefixes and framing tails, declared lengths, cap overflow, truncation or
  partial delivery, invalid output, failed writes, segment changes, driver refusal,
  deadlines, Stop and recovery. Measure task latency and peak memory; byte and pointer
  identity and work counts alone prove no gain.
- Qualify [control-token
  rejection](docs/oxibonsai-reuse.md#security-features-and-transfer-boundaries) in
  actual template and tokenizer sessions: all 33 Swift spellings, user and system
  content, assistant history and reasoning, split parts, JSON escapes and lookalikes.
  Verify fixed HTTP 400 recovery and identical count/generation requests; qualify the
  reference vocabulary separately.
- Qualify load, allocation-failure and backend-loss recovery: bounded restore attempts,
  unchanged workspace on failure, no restart loops, silent profile changes, corruption
  or automatic cloud fallback.

### Durable qualification evidence

- Deploy the native `evidence-archive` selection/seal/restore operations to durable
  owner-managed storage under [evidence retention](docs/validation.md#evidence-retention).
  Complete final-source maintenance CLI/analyzer and native guard acceptance. Bind
  exact files and SHA-256 digests to source/build/device identities, run and artifact
  references and independent reviews, retaining losing/no-go results. Automate capture
  before temporary Actions artifacts expire and rehearse the actual durable backup's
  digest-verified retrieval without the original runner. Local fixture retrieval does
  not establish durable deployment. Missing archived evidence keeps promotion pending.

## Runtime and correctness

Qualify the [CI repair](docs/evidence/ci-repair-20261103/summary.json) on settled
main through owner-managed hosted Foundation and public-reference preparation.
The workflow now supplies runner identity at supported step scope; numerical
capture pins match the reviewed released runtime; artifact lifetime controls
isolate independent private-directory and shared temporary-ancestor readers.
Retain the new-source focused regression, analyzer and workflow receipts separately
from the earlier closed diagnostic. Successful local checks do not establish a
hosted verdict or authorize model/reconstruction work.

Qualify the [four-lane speed source integration](docs/evidence/speed-four-source-20261103/summary.json)
on final settled source. It implements scoped packed PQ2 activation consumption,
explicit Runner latest-prefix selection, real packed FFN tiles and separately
identified triangular GDN chunks, plus combined CPU efficiency controls. Forty
focused Prism tests and selected strict Release runtime/server analyzers passed.
The one 376.962-second local source session closed after Runner all-target analysis
reported twenty-one preexisting thread-confined acceptance-future findings and two
unchecked duration subtractions. Their narrow test-only source corrections, and
three UI analyzer corrections, remain uncompiled; do not retry or extend that
closed session. Source-bound owning checks, native application linking, explicit
reuse lifecycle, full-model prefill/adapter/state acceptance, matched performance
and a qualified Prism release/Runner repin remain pending. No installed runtime,
default reset policy, numerical qualification or speed verdict changes here.

Complete the [current-nightly Rust standards run](https://github.com/symbiogenesis/SwiftBonsai2Runner/actions/runs/41386105716)
for source `d2a2962`: all 12 registered crates' owning Release Clippy, rustdoc,
tests and builds on `nightly-2026-11-02`. The preceding run passed its test,
documentation and build phases, including native speech fixtures/linking, but
failed Clippy on an oversized-buffer reset and a newly const-capable queue
accessor. Both corrections are pushed; the final combined hosted result is
pending. Polonius and trait-solver behavioral gates already passed on hosted
Linux and Windows. Keep model/device and performance qualification separate.

Use the [native Rust contract](docs/native-rust-engine.md). [Runtime
promotion](docs/native-rust-engine.md#default-runtime-promotion) retains the standard
adapter at scale 1.3, binds independent gates to exact source, release and app
identities and leaves missing evidence pending.

Extend the [bounded original-PQ2 four-worker lifecycle
evidence](docs/evidence/prism-0110-lifecycle-service.json) to active cancellation,
generated/edited history, fresh-owner restart and independent numerical, resource
and quality gates. Its static supplied-history fixture and short streams do not
close those gates. Keep promotion evidence separate for one versus four workers
and retain the reset-every-request serving policy until reuse is qualified.

- Complete bounded Prism-owned actual-model acceptance for the released private HTTP
  service and its internal model worker: canonical counting/generation, adapter
  readiness, scale changes, active cancellation, reset, rollback and stale-state
  rejection. Qualify the audited bounded Prism-owned device job around the pinned
  trusted Runner owner and consume its source-bound checker/owner package
  from the same owning workflow/run with
  one aggregate device allowance. Retain the separate release/source/bytes and
  harness/source/run/artifact identities under the
  [preparation record](docs/evidence/prism-parity-preparation.json).
  The [first owning attempt](docs/evidence/prism-owned-service-attempt.json)
  passed its capture step but failed artifact upload with `ENOTFOUND`; runner
  cleanup removed all four temporary case receipts. Requalify after durable
  selected-receipt staging passes owning checks. A green capture step without
  retained model receipts does not close this gate.
  Provisional-disconnect/reset replay leaves
  active graph cancellation and complete-state rollback pending. Runner lifecycle
  receipts alone do not close this Prism-owned gate. Use version-2 promotion evidence
  to bind tested release/source/bytes separately from the exact owning main
  harness/workflow/run and retained artifact identities. Preserve exact-source
  runtime Foundation/conformance gates and independent review.
- Qualify Prism's source-bound `swift_model_check rollback` capture on the actual
  original PQ2 model and approved adapter. Require every committed token, logits and
  live KV/convolution/recurrent bit to match before any reset, then full-state/logit
  continuation parity against fresh recomputation. Its four focused controls reject
  incomplete restore, unrelated graph failures and capture-bound violations; they
  do not replace actual-model evidence or qualify private HTTP cancellation,
  production sampling or interruption inside a graph kernel. Keep this independent
  capture pending when it cannot fit the shared owner-device allowance.
- Release the reviewed Windows read-consumer helper API through the registry,
  replace Prism server's exact Git dependency with that reviewed registry release,
  and validate packaging before re-enabling server crate publication. Preserve
  the explicit GitHub binary/source delivery boundary until those gates pass.
- Audit immutable comparison and reuse revisions: model, adapter, tensor and transform
  identity, hardware, numerics, licenses and notices. Keep PQ2, current-weight PTQ1 and
  Qwen as separate controls; respect [OxiBonsai reuse
  boundaries](docs/oxibonsai-reuse.md). Implement justified kernels in Prism.
- Qualify pinned Rust on full-context, long-output Swift: scalar/NEON logits, complete
  KV, recurrent and convolution state, exact prompts and token IDs, sampler, termination
  and lifecycle recovery at `1e-3 + 1e-3 * abs(reference)` tolerance.
- Qualify the [rank-one Rust
  graph](https://github.com/symbiogenesis/prism-inference/blob/c192575cb70be3aa2c4b6e2a5c313c6d4bd68525/docs/rank-one-adapter.md):
  native writer-basis and full-model zero/nonzero logits and state. Integrate
  private-service scale changes and lifecycle before adapter admission. Synthetic or
  source-only execution is insufficient; qualify PTQ1 and other formats separately.
- Produce batch-one attribution for projections, transforms and quantization, attention,
  GDN/convolution, copies, output, sampler, transport and speech. Measure complete-model
  allocations, scratch lifetimes and peaks for reusable logits, state-only ingestion,
  shared activations, physical FP16 KV, checkpoint pages and sampler candidates. Audit
  full-cache replacement, broadcast and transpose materialization and repeated
  preparation; remove the largest avoidable copy or allocation while preserving
  independent snapshots. Qualify long-context tiled attention-value traversal under the
  [overhead
  review](docs/native-rust-engine.md#2026-literature-runtime-overhead-and-work-placement).
- Qualify the local [inference efficiency candidates](docs/native-rust-engine.md#local-inference-efficiency-candidates):
  compact reset-state rollback checkpoints, bounded Swift top-20 sampler storage and
  direct one-column F32 adapter projections, followed by redundant empty-reset
  elimination and exact masked-attention padding pruning. Preserve exact state restoration,
  native tie/RNG behavior, floating-point bits and rejection semantics. Complete
  owning hosted checks and matched actual-model correctness/resource/request-time
  evidence before releasing or repinning; source-level work/storage reductions do
  not establish measured throughput, process RAM or lower admission limits.
  The four-lane source integration also fuses finite-logit validation into bounded
  selection and prepares an exact named hosted efficiency gate; require its final
  source/compiler-bound result, strict analyzers and production linking. Production
  fused recurrent copy/decay remains excluded pending its separate actual-model gate.
- Qualify the explicit Prism CPU tiled FFN and triangular GDN prefill source paths
  with 8/16/32-token tiles, separately identified numerical contracts, conservative
  extra scratch, cancellation and whole-request rollback. The exact tile mode keeps
  sequential GDN; the distinct `gdn-wy-f32-v1` mode batches recurrent projections and
  uses bounded decay-product triangular algebra. Require full-model/long-prefix
  state, logits, nonzero GDN adapter, continuation and restore evidence under the
  [prefill contract](docs/inference-algorithms.md#chunkwise-gated-deltanet-prefill),
  then matched whole-prompt benefit against qualified NEON/worker execution.
  Source work reuse and the SIMD tiled candidate establish no speedup or release gate.
- If attention is material, compare grouped-head KV reuse and bounded score/value tiles,
  retaining versus recomputing scores. Preserve globally normalized FP16 probabilities
  before V; online-softmax/Flash changes require separate numerical and continuation
  qualification under the [attention
  contract](docs/inference-algorithms.md#attention-locality-and-the-io-bound).
- If recurrent-state traffic is material, compare short FP32 KVBuffer-style factor
  buffering with sequential updates. Charge reads, materialization and flushes;
  checkpoints and cancellation must include pending updates and convolution. Keep
  writeback deferral separate from precision changes and stop without a complete-cycle
  benefit; see [state-traffic
  research](docs/inference-algorithms.md#2026-state-traffic-and-verification-research).
- Qualify exact prompt reuse across edits, switches, cancellation and restore: matching
  model, template, tokens and complete state, with bounded private snapshots and
  eviction. First qualify latest committed same-session state at bit-identical adapter
  scale: the pinned serving wrapper currently resets every request and defeats the
  generic runtime's live-prefix matcher, as recorded in the [source
  audit](docs/native-rust-engine.md#bounded-parallel-row-qualification). Require
  fresh-recompute parity, switched/edited histories, cancellation/rollback,
  scale-change/change-back and fault/explicit-reset recovery before changing that
  serving contract. Runner now forwards explicit
  [`--prompt-reuse latest`](docs/prefix-reuse.md) through deferred native startup,
  the UI and contained broker, checking exact pinned-source capability and matching
  service readiness. Qualify final settled source and actual-model private-service
  reuse after the passed hosted candidate complete-logits/state replay;
  no extra retained prefix snapshots or application selection are qualified.
  Start with immutable KV pages, exact suffix replay
  and simple lookup and eviction before radix trees or sparse placement. Resolve total
  versus incremental accounting: two 149.625 MiB recurrent/convolution snapshots already
  cost **299.25 MiB before KV/metadata**, exceeding a 256 MiB total cache. Keep that
  budget and conservative per-snapshot page charges.
- Build a short-chain CPU block verifier before proposals: complete state, partial
  acceptance, EOS and output limits, Stop, failed commit and rollback. Establish greedy
  parity, then conditional acceptance and residual sampling for actual temperature,
  top-k, top-p and penalties. Distribution preservation does not promise identical RNG
  use or seeded output.
- After verifier qualification, try bounded prompt/history suffix proposals with a few
  short windows and no second model. Compare repetitive and creative held-out prompts,
  charging drafting, verification, commit and recovery together. Retain ordinary
  decoding; stop if acceptance cannot repay the cycle.
- If chain recovery is material, compare snapshot/replay, saved factors and delayed
  commit using
  [SpecLA/TreeWY/RecoverSSM](docs/inference-algorithms.md#trained-parallel-proposals-and-full-state-verification).
  Preserve complete state and declared float identity; wide trees must beat chains.
- Only after a verifier leaves a feasible measured break-even, screen a published small
  drafter for exact tokenizer and vocabulary, Swift/LoRA and native ARM64 compatibility;
  check target-feature interfaces when consumed. Charge both models' weights, KV,
  features, scratch, startup and prefill before provisioning. Suffix failure does not
  rule out learned drafters; Qwen naming does not prove compatibility. Record no-go if
  none fits; exclude general training and DSpark.
- Qualify replacements against matching pinned-Prism artifacts, adapters and settings
  and all promotion gates, retaining deterministic streams where applicable. Retire old
  build, overlay and FFI paths only after acceptance.

### sglang-rs ideas for Prism integration

Use the pinned [runtime scope](https://github.com/imReese/sglang-rs/blob/f9b5bade089e64032c07f5e751bd00a3af2b3847/README.md),
[architecture](https://github.com/imReese/sglang-rs/blob/f9b5bade089e64032c07f5e751bd00a3af2b3847/docs/architecture.md)
and [roadmap](https://github.com/imReese/sglang-rs/blob/f9b5bade089e64032c07f5e751bd00a3af2b3847/docs/runtime-roadmap.md)
as references for the existing reuse, prefill and selected Hexagon milestones.
Upstream architectural proposals are not implementation or performance evidence.
Implement justified changes in Prism, then consume qualified pinned releases in Runner;
audit immutable source files and licenses before code reuse. Preserve Swift's packed
artifacts, adapter/numerical contracts, one active generation and native Rust UI/broker
containment. Adopting the server, CUDA/native-kernel dependencies, NexusKV, broad API
routes or distributed serving is outside this integration scope.

- Extend the exact-prefix gate above with checked finite page geometry and token-to-slot
  descriptors. Reuse immutable complete pages with private mutable tails; choose CPU
  layout from attention-locality measurements rather than copying a CUDA layout.
  Publish a reusable prefix only after its KV and exact recurrent/convolution checkpoint
  are complete and committed. Retain simple lookup before radix trees and the total
  snapshot/cache budget above. Allocation failure, aborted prefill and stale model,
  adapter or execution identities must leave no reusable partial entry.
- Feed uncached-token counts and validated prepared-batch metadata (tokens, positions,
  lengths and cache slots) into the existing chunkwise-prefill work after packed
  projections. Enforce context/output reservations, scratch bounds and cancellation
  between bounded chunks. Qualify cold prompts and exact-prefix suffixes without
  introducing concurrent requests or changing sampler progression.
- For the chosen accelerator milestone, keep one logical cache owner and separate
  scheduling/transfer decisions from device execution using typed batch, cache,
  transfer and dependency plans inside the existing contained inference worker.
  Bound command queues and completion waits; overlap only independent preparation
  or transfers. Treat submitted, in-flight, completed and failed work explicitly:
  consumption waits for valid contents, and publication waits for successful completion.
  Cancellation revokes publication immediately; page/buffer reuse and release wait for
  terminal device completion or verified worker teardown. An abort request is not a fence.
- Where the qualified Hexagon/Adreno API requires registration or staging, compare
  reusable device-visible pools with bounded staging instead of registering/allocating
  on every operation. Charge cold initialization, persistent locked memory, extra copies,
  synchronization and cleanup. Keep capability checks explicit for artifact, dtype,
  operations and device; unsupported paths must follow the existing qualified fallback
  or strict-rejection policy. Do not add CUDA registration machinery to the CPU path.
- Extend existing phase receipts with cached/uncached token counts, unique live pages,
  allocation/copy bytes, queue bounds and transfer/completion timing as applicable.
  Add owning hosted fault/lifecycle coverage for invalid geometry/slots, exhaustion,
  partial-page boundaries, failed or late completion, cancellation and stale publication.
  Require fresh-recompute logits and complete-state parity plus measured whole-request
  latency/resource benefit before changing serving defaults or selecting an accelerator
  profile; cache hits or apparent overlap alone prove no gain. Keep owner-device setup,
  loading, comparison and cleanup within the single five-minute aggregate allowance;
  full suites and benchmarks stay CI-only.

## Complete C++ retirement

Use the [retirement contract](docs/cpp-retirement.md) to track each file's behavior,
replacement evidence, source identity and remaining consumers. Source deletion does
not close pending hosted integration or full-model acceptance.

- Complete hosted integration and actual-model qualification for the Rust stochastic
  sampling pipeline (`sample_top_k_p`, `top_k_mask`, `top_p_mask`, MSVC STL
  `_Sort_unchecked`/`_Nth_element_unchecked` tie ordering, sequential `expf` arithmetic
  and accumulation, Philox draws, CDF walk, and `REPETITION_PENALTY_WINDOW = 72`, now
  implemented in `src/speech-tts/src/sampling.rs` and wired via `bonsai_qt_sample_top_k_p`
  in `src/speech-tts/src/native/sampling.rs` and `src/speech-tts/build_overlay.rs`) and
  CodePredictor's sampling path. Qualify actual pinned tokenizer vocabularies as well as
  the complete model-free scanner/ID captures. Qualify Rust graph-cache resource ownership,
  synchronized compute leases and tokenizer loading/ownership during actual-model speech
  lifecycle, including native graph-build failure, cancellation and model recovery.
  Preserve the passing full hosted Qwen consumer controls; its model-free metadata,
  callback and lifetime checks do not close actual-model acceptance.
- Retire Prism's native reference families file by file after matching hosted replay
  of their source-level assertions. Preserve all approved green IDs, full geometry
  schedules, corrections, unsupported outcomes and numerical/target boundaries.
  Remove dispatch, native ABI probes, build/cache/source-download paths and linkage
  only when their retained obligations have replacements.
- Replace Qwen/GGML TTS and Canary ASR's `transcribe-cpp` inference dependency with
  native Rust model/audio execution, qualifying exact artifacts, token/PCM/state,
  cancellation, lifecycle, quality and resource contracts. Then remove compiler,
  CMake, static/native library and source provisioning paths. Retire publisher and
  native DLL reference execution after immutable controls, Rust acceptance and
  fallback/promotion gates cover their consumers; moving C++ behind FFI does not
  complete retirement.
  The checked-in C++ collector is retired, but its Rust replacement still invokes
  the pinned native DLL engine. Preserve the complete source-bound ABI preflight
  and independent full-state numerical comparison until that engine is retired.
- Execute the phased pure-Rust Qwen3-TTS and `hexagon-native-rpc` NPU migration in
  `src/speech-tts`, retiring `qwentts.cpp`, `ggml`, and `ggml-hexagon.dll`/`ggml-base.dll`
  while preserving the system `qcnspmcdm` DriverStore `libcdsprpc.dll` FastRPC boundary:
  1. **Transport boundary**: Retain `libcdsprpc.dll` (resolved via Windows SCM `qcnspmcdm`
     from `DriverStore\FileRepository\qcnspmcdm*.inf_arm64_*` with both Win32
     `SetEnvironmentVariableW` and UCRT `_wputenv_s` for `ADSP_LIBRARY_PATH`) as the sole
     supported OS/driver FastRPC boundary (`remote_handle64_*`, `rpcmem_*`, `dspqueue_*`).
     Do not replace `libcdsprpc.dll` with undocumented raw `\Device\QcNspMcdm` `ioctl`
     calls that bypass FastRPC session handshakes, signed PD catalog checks, or SMMU
     `rpcmem` registration.
  2. **Immediate hybrid NPU/CPU placement, fused CPU flash attention, and ICL prompt/codec caching
     (completed in `src/speech-tts/build_npu.rs`)**: Run `Talker` multi-token prefill (`T_ctx`
     tokens, 28 layers, `Q8_0`) on Hexagon `HTP0` while executing latency-critical `T = 1`
     single-token `Talker` decode (via zero-copy `talker_cpu` GGUF views over cache-coherent
     `HTP0` `rpcmem` `F16` KV cache) and 5-layer `CodePredictor` (`15` residual codebooks per
     frame with `SAMPLER_TAIL_FIXED` top-k view) on host NEON CPU threads with fused
     `ggml_flash_attn_ext` enabled, routing prompt text/reference-code projections directly to
     `cpu_backend`, caching ICL reference codec/text streams and speaker-embedding prefixes in
     `PromptCache`, and priming/restoring causal `CodecStateSnap` reference snapshots so synthesis
     decodes only the generated `T_frames`.
  3. **Pure-Rust `Talker` + `CodePredictor` + `hexagon-native-rpc` NPU backend (implemented in
     `src/speech-tts/src/engine.rs`; actual-model & hosted qualification pending)**: Port
     `Talker` (28-layer Qwen3 transformer, 3D MRoPE, KV cache) and `CodePredictor`
     (5-layer residual RVQ predictor) to pure Rust in `src/speech-tts` atop the existing
     Rust BPE tokenizer (`src/speech-tts/src/tokenizer.rs`), GGUF reader
     (`src/speech-tts/src/gguf.rs`), and Philox/stochastic sampler
     (`src/speech-tts/src/sampling.rs`), dispatching multi-token prefill and heavy
     projections directly to the signed `libggml-htp-v81.so` / `v73.so` DSP skels through
     `hexagon-native-rpc` (`dspqueue` + `htp_ops`, `HtpPrefillPacket`) without `ggml-hexagon.dll` or
     `ggml-base.dll`, and expose optional rank-one / LoRA adapter overlays (`TalkerLoraAdapter` /
     `LoraProjectionPatch`, reusing `prism-inference`'s adapter infrastructure) so lightweight
     regional-accent or language LoRAs (such as `aguken-ai/qwen3-tts-indic-finetuned` variants)
     can attach to the shared `Base` `Talker` without duplicating base model files.
  4. **Pure-Rust 12 Hz audio codec, speaker encoder, and C/C++ build retirement (implemented in
     `src/speech-tts/src/codec.rs` and `src/speech-tts/src/speaker_encoder.rs`; C/C++ build
     removal gated on full-model acceptance)**: Port the 12 Hz causal ConvNeXt/SnakeBeta audio
     decoder, ECAPA-TDNN speaker encoder, and stateful `CodecStateSnap` streaming pipeline to
     pure Rust, then remove `qwentts.cpp` source provisioning, CMake/C++ compilation
     (`src/speech-tts/build*.rs`), and all `ggml-*.dll` runtime dependencies once actual-model
     and hosted acceptance pass. Support the drop-in 48 kHz decoder upgrade
     (`takuma104/Qwen3-TTS-Tokenizer-12Hz-48kHz`, which keeps the 12 Hz 16-codebook RVQ
     interface unchanged and appends one ~95K-parameter `< 400 KB` 2x upsampling decoder
     block) to double synthesis output from 24 kHz to 48 kHz (`SAMPLE_RATE_48K = 48_000`,
     `PACKET_BYTES_48K = 9_600` in `src/chat-ui/src/speech/pipeline.rs`) while 2:1-decimating
     to 24 kHz audio (`ref_audio_24k` via `decimate_48k_to_24k` and in-place reference
     decimation in `src/speech-tts/src/native.rs`) during `VoiceDesign` reference extraction
     (`qt_extract_voice_ref`).
  5. **Bonsai-style ternary `Talker` compression (`PQ2_0` / `PTQ1_0`, runtime kernels and policy
     implemented in `src/speech-tts/src/engine.rs`; offline artifact preparation & device
     qualification pending)**: After the pure-Rust `Talker` port (Step 3) and `Q4_K_M` baseline
     qualification, apply offline block-1024 Sylvester Walsh–Hadamard + sign folding
     (`input-last-dimension`, matching the 1.7B `Talker`'s `2048` hidden width via
     `sylvester_walsh_hadamard_in_place` and `apply_hadamard_sign_fold`) and
     Hessian/activation-calibrated group-128 ternary quantization (`PQ2_0` and lossless `PTQ1_0`
     repack via `repack_pq2_to_ptq1` and `validate_talker_ternary_policy`) to the 28-layer
     `Talker` `attn_q/k/v/output` and `ffn_gate/up/down` projections in the upstream
     `Qwen/Qwen3-TTS-12Hz-1.7B-VoiceDesign` and `Base` checkpoints. Keep `text_embd`,
     `codec_embd`, `text_proj`, `codec_head` and RMS norms in `Q8_0`/`F16`/`F32`, retain `Q8_0`
     or `Q4_K_M` for the 5-layer `CodePredictor`, and keep the 12 Hz ConvNeXt/`SnakeBeta` audio
     codec and ECAPA-TDNN speaker encoder at `Q8_0`/`F16`. Execute the ternary `Talker`
     projections through shared `prism-inference` Hadamard transform + `PQ2_0`/`PTQ1_0` NEON CPU
     kernels (`f32x4` SIMD `F32`/`Q8_0` row projections, byte-unrolled `PQ2_0`/`PTQ1_0` group
     loops, shared pre-folded Hadamard activations across `q/k/v` and `gate/up`, precomputed
     `f32x4` NEOX `RoPE`, in-place `RMSNorm`, and `f32x4` GQA scoring/value accumulation in
     `src/speech-tts/src/engine.rs`) and Snapdragon X2 Hexagon NPU `native-q2` (`HTP_TYPE_Q2_0`)
     resident offload.

## Creative quality, adapters and memory

### Quality and speech

- Evaluate the [weight-space programming lead](docs/weight-space-programming.md)
  with an early stop: first establish a reproducible Swift failure in complete
  reasoning/answer captures, using exact model, runtime, template, sampler,
  thinking, adapter and termination identities. No Swift loop defect or beneficial
  weight correction is established. Reuse existing captures and human review before
  adding instrumentation; short prefixes and synthetic fixtures cannot justify an edit.
  Budget one investigation session, then stop if there is no concentrated correctable
  fault or feasible evaluation. If justified, allow one or two candidate sessions for
  a Swift-specific FFN direction and small F32 correction, followed by separate hosted
  validation; these are contingent effort estimates, not promised improvement.
  Preserve the standard adapter when isolating the candidate effect: overlapping
  corrections need explicit composition beyond the current single rank-one slot.
  Bind the patch to original PQ2 and the current lossless PTQ1 repack independently,
  preserve original-activation basis, and retain loaded-zero identity and rollback.
  Require held-out complete-answer correctness, instruction following and creative
  quality without material throughput/resource regression; measure useful-answer time
  separately from tokens/second. Do not infer benefit from shorter output, copied
  article layer IDs, a different adapter/sampler or smaller context/output limits.
  A discovery/comparison matrix is not feasible under the retained startup evidence
  and aggregate five-minute laptop ceiling; leave model work pending until a complete
  workload fits or an appropriate hosted target is available. Do not split it across
  owner sessions. This assessment does not qualify a runtime/default change.
- Establish an independent writing and art-direction rubric: drafting, constrained
  revision, continuity, dialogue, voice and tone, composition, instruction following,
  coherence, repetition, unwanted refusals and useful completed answers. Qualify
  Thinking, output settings and Extra effort; changed text or benchmark averages are
  insufficient.
- Complete [standard LoRA
  qualification](docs/swift-lora.md#standard-profile-qualification): SSM writer-basis,
  loaded-at-zero response and logits identity, nonzero application, paired device trials
  and independent human reviews. Require both same-engine decode TPS and quality parity,
  bounded RAM premium and visible regressions. Qualify app scale 1.3 and other intended
  scales separately from the scale-one, 512-token contract; one response cannot choose a
  scale. Provide a hosted Snapdragon target for the full 28-sample LoRA and
  14-sample PTQ1 matrices, or define an independently useful owner capture whose
  complete workload fits five minutes. Their nonlaunching workflow plans leave
  qualification pending; splitting either matrix across owner runs is unsupported.
- Compare Q8_0/Q4_K_M and bounded speech thread choices through [paired device CI and
  independent listening](docs/validation.md#paired-speech-comparison). Retain quality,
  startup, first-audio, synthesis latency and resources; attribute bootstrap, warm Base
  prefill, residual-code prediction, codec, transport and playback. Distinguish
  generated PCM from audible output, supply gaps and underruns; use [existing resource
  evidence](docs/performance-expectations.md#q4-speech-resource-comparison) and reuse
  contracts to choose remaining work.
- Evaluate and qualify the mixed-precision ternary `Talker` candidate (`PQ2_0` / lossless
  `PTQ1_0` repack) against the verified `Q8_0` and `Q4_K_M` speech locks once the
  pure-Rust `Talker` runtime and offline Hadamard-folded artifact are prepared. Pin exact
  immutable GGUF lengths, SHA-256 digests and `prism.hadamard.*` transform metadata before
  device admission; never attempt to repack existing `Q8_0`/`Q4_K_M` weights directly or
  ternarize `CodePredictor`, `codec_head` or the 12 Hz ConvNeXt/`SnakeBeta` codec. Require
  paired target-device CI and independent human listening across `VoiceDesign` reference
  bootstrap and warm `Base` synthesis (timbre, prosody, phoneme stability, EOS
  termination, first-audio/chunk latency, peak Job commit/working set and recorded
  AC/battery power conditions) while retaining `Q8_0`/`Q4_K_M` rollback.
- Improve TTS enunciation, prosodic pacing, accent replication and high-frequency audio
  fidelity without regressing memory admission or warm synthesis latency (implemented in
  code and locally verified; paired target-device CI and independent human listening
  pending):
  - **Decoupled `Talker` vs. `CodePredictor` (`subtalker_*`) sampling and windowed
    repetition penalty (`src/speech-tts/src/native.rs`, `src/speech-tts/src/sampling.rs`,
    implemented)**:
    Preserve expressive `temperature` (`0.9`, `top_k: 50`, `top_p: 1.0`) during
    `VoiceDesign` reference bootstrap (`design_params`) while lowering `Talker`
    `temperature` (`0.85`), tightening `Talker` `top_p` (`0.9`), lowering
    `subtalker_temperature` (`0.6`) and keeping `subtalker_top_p` (`1.0`, `SAMPLER_TAIL_FIXED`)
    during `Base` synthesis (`clone_params`) so residual codebooks `1..=15` lock onto
    high-probability consonant formants without frame-to-frame timbre jitter or full-vocab
    CPU sampling overhead. Window `repetition_penalty` to
    the trailing `REPETITION_PENALTY_WINDOW = 72` tokens (~5.76 s at 12.5 Hz) to eliminate
    progressive speaking-rate acceleration and clipped word endings over multi-sentence
    read-aloud (`dffdeeq/Qwen3-TTS-streaming` and `grimavatar/Qwen3-TTS-Train` findings).
  - **Phonemically balanced, accent-eliciting reference conditioning (`src/speech-tts/src/native.rs`,
    lifted from `NeoKazuya/qwen3-tts-enhanced`, implemented)**: Replace the flat two-sentence
    `REFERENCE_TEXT` with a compact 17-word single-clause phonemically balanced elicitation
    passage covering trap-bath vowel splits, rhotic/non-rhotic post-vocalic `/r/`, dental
    fricatives (`/θ/, /ð/`), and diphthongs without internal comma pauses so `Base`
    in-context conditioning (`icl_codec_stream` / `icl_ref_text_stream`, cached once in
    `PromptCache` at zero warm-request projection cost) faithfully reproduces diverse
    regional accents without inflating ICL `ref_codes` length or inducing premature EOS at
    punctuation pauses.
  - **48 kHz drop-in codec decoder (`takuma104/Qwen3-TTS-Tokenizer-12Hz-48kHz`) and
    segment boundary smoothing (`src/chat-ui/src/speech/pipeline.rs`, `src/speech-tts`,
    `src/speech-service`, implemented; GGUF lock & device listening pending)**: Support
    both 24 kHz and 48 kHz codec output (`SAMPLE_RATE_48K = 48_000`, `PACKET_BYTES_48K = 9_600`),
    in-place 2:1 `ref_audio_24k` reference decimation for `Base`, dynamic `/health` and
    WAV/PCM sample-rate framing, and 64-sample (`HANN_BOUNDARY_SAMPLES = 64`) raised-cosine
    Hann-window boundary smoothing across 256-character UI sentence boundaries; qualify a
    pinned GGUF conversion of the 48 kHz tokenizer decoder through paired target-device CI
    and independent human listening (sibilant/plosive clarity, enunciation, accent
    fidelity, chunk continuity, and peak Job commit/working set).
- Qualify [native-owned VoiceDesign
  references](docs/streaming-resources.md#native-speech-generation-and-streaming)
  through Design teardown and Base extraction on first use, profile changes and
  bootstrap failure. Include Rust parameter defaults, CPU thread selection,
  reserved-token suppression, repetition penalties, Philox semantic and predictor
  sampling and Rust-owned speech tokenization in actual-model, listening and resource
  acceptance. Cover tokenizer initialization/rejection, worker teardown, exact utterance,
  instruction and Base reference token IDs; model-free token/vector/C++ field and bit
  comparisons are insufficient.
- Qualify chat alone, warm TTS and overlap with a small shared thread-budget set: UI
  frames, audio continuity, useful-answer time, sustained memory and heat. Check cold
  idle separately from active, warm and Canary monitoring, including wake and Stop
  during the 30-second TTS lease. Preserve [ASR/TTS exclusion and
  reserves](docs/streaming-resources.md#speech-scheduling-and-playback).
- Defer [speech
  speculation](docs/performance-expectations.md#2026-speech-inference-research) until a
  compatible native artifact or laptop-feasible tiny fit exists for selected 1.7B (not
  paper 0.6B). Before fitting, require residual-code verification, sampler correction,
  accepted-state recovery and bounded scratch; stop if costs erase complete-readout
  benefit.

### Memory and profiles

- Investigate lossless adapter reservation and rank-one accumulation with allocator and
  strict rounding and order checks. Resolve counter discrepancies and preserve dynamic
  scale and load-without-apply under [allocation
  rules](docs/swift-lora.md#allocation-semantics).
- Complete [PTQ1 release qualification](docs/swift-memory.md#release-decision) beyond
  the bounded Auto/explicit-NEON numerical and native-service smokes. Qualify full-model
  scalar/explicit-NEON logits and live state, no-LoRA/loaded-zero identity and nonzero
  application, immutable adapter writer-basis provenance, then stable-condition
  paired complete responses at app scale 1.3, context/output and recovery, quality
  and comfort with PQ2 rollback. Promote or reduce admission only after suitability,
  using the verified
  **1,259,520,000-byte** packing difference. Account for mapped residency separately
  from private commit and preserve reserves. Collect and independently review all
  gates in the explicit PTQ1 promotion policy, preserving exact packing provenance
  and the reviewed original-PQ2 rollback; PQ2 and worker variants cannot supply
  a PTQ1 promotion verdict. Retain the self-contained promotion score with its exact
  policy/profile identities and remaining gate requirements alongside reviewed evidence;
  an actionable pending-gate report does not close those gates.
  Extend the passed minimal Rust-service pair toward independently reviewed benefit
  while retaining complete-response,
  per-prompt adapter TPS/quality, stable conditions and independent-review gates.
  An incomplete feasibility pair cannot be resumed through additional jobs.
- Capture prefill, first useful answer, decode TPS, task time, retries and truncation,
  working, private, Job, system and device memory peaks, hard faults, observable energy,
  responsiveness and sustained heat and fan behavior. Retain X2 `ProcessMemory`
  aggregates and qualify complete-model coverage and model-phase attribution beyond
  observed stdout boundaries. Follow the [measurement
  rules](docs/performance-expectations.md#2026-literature-measurement-and-comparison):
  repeated interleaved pairs with uncertainty, independent X1 and X2 results,
  instrumented attribution apart from timing, idle versus active power and consistent
  token boundaries, energy per acceptable task with output and retry counts. Overlapping
  counters are not additive; sampled maxima can miss brief allocations.
- Qualify creative, daily, long-context and battery profiles within total budgets.
  Larger context, Q8/Q4 KV, Flash Attention and recurrent precision need separate
  numerical, state and quality gates; preserve canon and output reservations.
- Attribute mappings, microbatch 32/64, persistent output rows, post-prefill trimming,
  prefetch, rotation/sign metadata and stacks before [lossless
  reductions](docs/swift-memory.md#mapping-and-startup). Review immutable-B sharing
  against the intended runtime, preserving rounding and distinguishing overlapping and
  disjoint allocation lifetimes.
- Qualify Qwen loading and service control independently when its 18 GiB allowance plus
  4 GiB reserve is admissible.
- Add source-referenced scene packets and author-approved context. Manuscripts and
  edited project bible stay authoritative; start with manual excerpts or lexical
  retrieval, embeddings only for demonstrated need. Summaries require source versions
  and author approval; vision, image critique or generation needs a matching model and
  shared-memory budget.

## Bounded calibration and optional precision

After exact CPU controls, use small fixed calibration samples, disjoint held-out
prompts, bounded statistics and [research
budgets](#research-scope-and-practical-budgets). Retain full-context, continuation and
LoRA gates; calibration proves neither quality nor reduced admission.

- Estimate process savings and traffic against PTQ1 first: 8K FP16 attention KV is 512
  MiB, so INT8 saves at most ~256 MiB before scales and scratch; recurrent/convolution
  state is separately 149.625 MiB. Cache ratios do not apply to weights.
- Screen native Q8 KV, then at most one justified asymmetric K/V or rotation-plus-scalar
  candidate. Keep FP16 controls, explicit numerical and cache identity and exact
  checkpoints; charge metadata, conversion, rotations, dequantization and retained
  copies. Defer SVD or index schemes without an 8K need; follow [compression
  constraints](docs/inference-algorithms.md#compression-and-approximation-constraints).
- Only for material state traffic or storage, compare FP16 recurrence with FP32, then at
  most one DAMP-inspired FP16/INT8 split. Keep convolution separate; charge scales,
  rounding and RNG, restore and trajectory error. Add low-rank bypass or directional
  rounding only if update loss and net benefit justify factors, merges and rollback.
- Check packed entropy and correlation including scales, indexes, random access and
  decoding. Try Shannonic-inspired streams only if executable bytes and traffic beat
  PTQ1 without retained expansion; current-census BITCOS remains rejected. New
  codebooks, rotations or scales need native layerwise cost and benefit preflight, not
  whole-model fitting from GPU results; see [weight/state
  research](docs/swift-memory.md#2026-entropy-coding-and-alternative-weight-artifacts).

## Hardware selection

- Extend [shared Doctor and launch
  discovery](docs/hardware-capabilities.md#host-inventory) with checked SoC/SKU, GPU/NPU
  APIs, features, architecture, drivers, runtime and firmware, signed readiness and
  static identity. Keep unknowns explicit and live budgets separate from compatibility.
- Register a run-specific ephemeral owner-managed ARM64 runner before the bounded
  [X2 thread screen](docs/cpu-kernels.md#x2-cpu-thread-screen); the current inventory
  has none. Qualify the complete short screen within one shared setup, verification,
  loading, comparison and cleanup budget; leave broader thread choices pending.
  Resolve legacy `0xC0000142` startup with shell DLL audits and bounded `--version`
  probes under unchanged mitigations. Extend the [exact-candidate X2
  model smoke](docs/runtime-candidates/README.md#explicit-x2-candidate-model-smoke)
  to Unicode model paths, effective thinking control, continuation/state, adapters
  and cancellation qualification. Integrate and release the [headless UCRT
  candidate](docs/runtime-candidates/README.md#headless-ucrt-argv-candidate) upstream;
  qualify its exact binary before repinning Runner.
- Qualify scalar/NEON, optional ISA and phase-specific thread, batch, wait and power
  choices independently on X1/X2. Screen settled `main`; extend only for measured
  opportunity. Suppress comparisons if CPU, kernel, features, topology or power change
  or are unknown, including between pairs, under [screen
  controls](docs/cpu-kernels.md#controls-and-cost).
- Compare event waits with polling for idle CPU, wake latency, decode and cancellation.
  Inventory Windows sampling limits for timing, energy, heat and memory; require valid
  telemetry for energy verdicts. Core counts and family names do not choose profiles;
  bare-metal gang scheduling stays outside the app.
- Build the bounded reviewed [profile
  catalog](docs/hardware-capabilities.md#profile-and-evidence-keys): compatibility,
  workload, reuse and objective keys, paired evidence, thresholds and losing/no-go
  entries. Implement deterministic [eligibility, ranking and
  admission](docs/hardware-capabilities.md#eligibility-ranking-and-admission) for task
  latency and battery energy, charging startup, repacking, compilation, migration and
  sustained costs. Use stable ties without startup benchmark search.
- Recheck budgets and readiness before admission; use qualified CPU fallback or strict
  rejection without changing model, adapter, sampler, precision, context or output.
  Report requested/effective profile, placement, evidence and value-free fallback
  reasons in diagnostics and a simple native Rust Auto control.
- Reevaluate safely after power, memory, runtime or driver changes: invalidate evidence
  and caches, migrate complete state or tear down and rebuild, never partial requests.
  Cover unknown devices, absent ISA/accelerators, stale identity, pressure, workload
  boundaries, ties, objective changes, strict rejection and failed or cancelled handoff.
- Demonstrate [automatic interactive and battery
  operation](docs/hardware-capabilities.md#automatic-profile-selection) independently on
  X1/X2, including installation and rollback, sustained use and upgrade requalification.

## CPU kernels

Qualify the [parallel performance and registry candidate](docs/evidence/performance-release-20261109/summary.json)
on settled Prism main through owner-managed checks. It halves declared Swift 128-wide
multi-token GDN row staging, uses immutable validated logits to omit redundant
finite predicates in Greedy/Swift selection, and fills existing native-Q2
8-operation resident sibling tail packets. Exact numerical/error/RNG contracts,
conservative admission and serving defaults remain unchanged. The one local
check compiled all six selected targets and passed 22 exact Rust controls and
five receipt controls, then stopped on incompatible pinned `syn` 2/3 requirements
in strict analysis. Its 63.152-second clock is closed within the declared
ten-minute source/publication allowance, with AC connected, saver off and 100%
charge at all retained endpoints; do not retry it. The narrow documented
analyzer policy and hosted runtime-test/format-policy identity fixes subsequently
passed the separately approved exact-source bundle checks on `055c743`, including
strict analysis, portable fallback and production linking. Default normalized
package verification and actual crates.io publication of0.1.23 passed separately;
the private server's exact `=0.1.23` dependency remains unpublished. All five
helper versions remain unchanged. Runner's verified0.1.23 binary lock selects the
published five-executable prerelease with exact source/archive/file identities.
Full-model state/adapter and
cancellation parity, ARM64 codegen/resources, vendor lifecycle and matched
whole-request benefit remain pending; source work counts establish no speedup.

Follow [CPU arithmetic, dispatch and acceptance](docs/cpu-kernels.md). Pin artifacts,
runtime, compiler, format, shapes, ISA and numerics; choose work from CI hotspot,
bandwidth, packing and dispatch evidence.

Qualify the [direct prefill, linear suffix and transient tile source changes](docs/evidence/performance-advance-20261109/summary.json)
on settled Prism main through owner-managed hosted checks. Eligible ARM64 GDN
prefill now reads immutable projected token rows directly, omitting the raw
head-major staging copy (60 MiB of nominal writes plus matching reads for a
32-token Swift tile across 48 recurrent layers). Suffix drafting uses a bounded
64-byte reverse-Z table with linear token comparisons. Single-operation transient
native-Q2 tiles pack weights once across their ordered token submissions instead
of once per token; resident and multi-operation tiled dispatch are unchanged.
The exact hosted gate now includes seven new controls and binds the documented
incompatible Windows-binding analyzer policy into source identity.

The single new model-free source check passed canonical formatting and all five
receipt controls, then stopped at the selected compilation's 180-second cap
before any Rust control executed. Process-tree termination and parent exit were
confirmed; final retention closed at 185.058 seconds within the five-minute
aggregate allowance, with AC connected, saver off and 99% charge at the aggregate
endpoints. Preserve this failed receipt and closed clock without retrying it.
Complete selected-target compilation, all twenty exact Rust controls, strict
analysis, feature-disabled compilation, Release ARM64 codegen/resource review,
actual-model state/logit/adapter and cancellation parity, and matched whole-request
benefit in their owning lanes before release or Runner repinning. Source work
counts establish no measured speedup, reduced admission or new default; existing
installed runtimes remain unchanged.

Qualify the [grouped GDN, suffix correction and resident solver source changes](docs/evidence/performance-progress-20261109/summary.json)
on settled Prism main through owner-managed hosted checks. Swift's 128-wide GDN
decode rows now use half the private staging payload, including normal four/eight-row
groups; suffix rejection reuses its verified greedy correction; resident native-Q2
handles retain immutable solver parameters under the existing metadata cap.
The one 80.684-second model-free source check compiled the selected targets and
passed all eleven exact Rust controls plus four receipt controls, then stopped
on inherited strict-analysis findings: incompatible `windows-sys` versions and an
overlong documentation paragraph. Its comment-only paragraph correction is source-reviewed;
the dependency finding, final-source strict analysis and feature-disabled check remain
pending. Preserve the failed receipt and closed clock without retrying. Require
Release ARM64 codegen/stack review, full-model state/logit/adapter and cancellation
parity, vendor lifecycle/resources and matched whole-request benefit before a
qualified release or Runner repin. Source work/storage reductions qualify no speedup,
reduced admission or speculative default; installed runtime and defaults are unchanged.

Qualify the [parallel performance source candidate](docs/evidence/performance-fanout-20261109/summary.json)
on settled Prism main through owner-managed hosted checks, Release ARM64 codegen/spill
inspection and matched actual-model request/startup acceptance. It reuses private
GDN multi-token decay/update staging, scans suffix history once with bounded proposal
storage, and packs native-Q2 prewarmed weights directly into admitted resident arenas.
Require exact state/logit/adapter and cancellation/rollback parity, complete vendor
lifecycle/resources and matched whole-request or startup benefit before a qualified
release and Runner repin. The extended hosted gate must retain the exact numerical,
failure/publication, portable-fallback and source/compiler/executable identities.
Source work/storage reductions and model-free controls establish no measured speedup,
installed-runtime change, reduced admission or speculative-default qualification.

Qualify the [follow-up performance source candidate](docs/evidence/performance-next-20261109/summary.json)
on settled Prism main through owner-managed hosted checks. It reads full-attention
gates from their original projection in decode and tiled prefill, retains the fixed
Swift sampler's validated filter state, and omits unused packed-weight scratch from
lazy native-Q2 resident preparation. Require the exact grouped/tail-head, original
sampler-chain/RNG, copied/direct/transient packing, resource and failure controls,
strict Release analysis, portable fallback and native linking. The one local check
passed formatting and four receipt controls, then stopped during compilation on an
ambiguous float type in the new attention oracle before any Rust control ran. Its
explicit binary32 annotation is independently source-reviewed but uncompiled;
preserve the failed receipt and closed clock without retrying. Actual-model
state/logit/adapter parity and matched complete-request benefit remain pending
before release or Runner repinning; source work counts qualify no speed or RAM gain.

Qualify the [exact recurrent and attention SIMD source changes](docs/evidence/cpu-state-simd-20261103/summary.json)
on settled Prism main through owner-managed checks. Pinned GDN dot/update now uses
safe baseline NEON with native reduction/tail and first-failure mutation semantics;
attention value tiles group adjacent channels without changing their temporal
arithmetic or 2 KiB accumulator payload. Floating projections retain explicit scalar
execution and diagnostics. The one new 39.363-second model-free check passed
twenty-four focused Rust controls, including direct NEON/native captures and graph
recovery, plus four receipt controls, then stopped on eight strict analyzer findings.
Six typed-chunk findings and two earlier prefill/dispatch findings have source-reviewed
corrections, which remain uncompiled. Preserve that failed receipt, closed clock and
original battery/saver/charge observations. Require final-source exact controls,
strict analyzers, portable fallback and production linking, then actual-model
state/adapter parity, code-generation/spill review and matched complete-request benefit
before a qualified release and Runner repin. No installed speedup is established.

Qualify the [remaining CPU speed source changes](docs/evidence/cpu-remaining-speed-20261103/summary.json)
on settled Prism main through owner-managed checks. Direct BF16 projections omit
decoded-row materialization while preserving activation rounding and the original
binary64 accumulation. Explicit packed prefill tiles now share the graph's existing
private row-worker pool, borrowing disjoint output slices and keeping cancellation
on the caller thread; the extra 16 KiB reservation must remain charged. Require
exact numerical, worker-join/cancellation, graph restore and portable-fallback
controls, strict Release analysis and production linking, then actual-model
state/adapter parity and matched complete-request/prompt benefit before a qualified
release and Runner repin. Source work counts establish no measured speedup;
the installed runtime, defaults and resource admission are unchanged.
The one 16.369-second model-free local check applied canonical formatting and
passed four receipt controls, then stopped during compilation on an empty-slice
type mismatch and unnecessary prelude qualification in the worker controls.
Both narrow source corrections are applied but uncompiled. Preserve this failed
receipt, closed clock and its original battery/saver/charge observations;
numerical tests, strict analyzers and portable compilation remain pending.

Qualify the [packed decode and SIMD prefill source changes](docs/evidence/cpu-speed-next-20261103/summary.json)
on settled Prism main through owner-managed checks. PQ2 decode now accumulates
bounded i16 product pairs before widening once for the final i32 reduction; tiled
prefill now reuses baseline NEON decoded weights across tokens while preserving
scalar forcing and PTQ1 native lanes. The single 13.333-second local check applied
canonical formatting and passed four receipt-parser controls, then stopped before
any numerical test ran on a private-interface visibility error. Its minimal type
visibility correction is applied but uncompiled. Preserve that closed clock and
its original AC/saver/charge observations. Require final-source numerical controls,
strict Release analysis, codegen/spill inspection, actual-model state/adapter parity
and matched request benefit before a qualified release and Runner repin. No installed
runtime, default, resource admission or performance verdict changes here.

Qualify the Prism `0.1.22` (`6b4f452`, building on `0.1.21` `90eb974`) CPU NEON and row-parallel
source optimizations (`fb64903`..`795ee6e`, building on `fcb54af`, `386677c` and `547cf47`) on
settled Prism `main` through owning hosted checks, Release ARM64 codegen/spill inspection,
actual-model parity and matched request-throughput comparison for those changes,
now included in the pinned `0.1.23` (`055c743`) release bundle:

- Single-widen `PQ2` NEON consumption kernels (`pq2_half_unsigned_sum` and
  `pq2_half_signed_sum` in `fb64903`), widening each 16-byte `PQ2` half once to
  `(u16x8, u16x8)` across 1-, 4- and 8-row kernels instead of four times per half.
- Direct 16-bit `PTQ1` NEON unpacking (`decode_ptq1_i16` plus 2 KiB lookup table
  `PTQ1_TAIL_EVEN_U64`), streaming 6-register block accumulation
  (`accumulate_ptq1_block_pinned`) and vectorized 4/8-row `f16` block scale decoding
  (`f16_bits_to_f32_x4` / `f16_bits_to_f32_x8` in `f390ec2`), plus multi-row NEON
  `PTQ1` source-contract kernels (`dot_ptq1_eight_rows_source`,
  `dot_ptq1_four_rows_source` and `reduce4_i32x4` in `f4d1844`).
- Eight-row NEON `BF16` dot kernel (`bf16_dot_eight_rows` in `435e6be`) for 8-row
  parallel chunks of `BF16` projections (`ssm_alpha`, `ssm_beta`).
- Multi-worker row parallelism (`RowParallelism`) for FFN `SwiGLU`
  (`Arithmetic::swiglu_in_place_with_parallelism`, partitioning `17,408` elements per
  layer into 256-element chunks in decode and tiled prefill) and tiled-prefill
  inverse-embedding rotation (`BasisTransforms::rotate_with_parallelism` in `9801a60`).
- Vectorized 4-group `f32x4` SIMD lane accumulations in tiled prefill for `PTQ1`
  (`Block::accumulate_lanes128`) and `Q8_0` (`Block::dot128_q8` in `b107698`).
- Multi-worker head and channel parallelism (`RowParallelism` in `9a7da8f` and `33ddbc2`)
  across per-head GDN recurrence updates (`apply_gated_delta_net` across 48 heads),
  per-KV-head full attention (`apply_full_attention` across 8 KV heads / 4 query heads per
  KV head), causal short convolution (`apply_short_conv` / `pinned_short_conv_in_place`
  across `12,288` channels) and attention Q/K RMSNorm + RoPE
  (`apply_attention_qk_norm_and_rope` across 32 Q heads and 8 K heads) with exact per-head
  floating-point order and earliest-failure recovery.
- Batched non-FFN tiled CPU prefill projections and token-parallel prefill normalizations
  (`04f556e`), batching recurrent (`attn_qkv`, `attn_gate`, `ssm_beta`, `ssm_alpha`,
  `ssm_out`) and full-attention (`attn_q`, `attn_k`, `attn_v`, `attn_output`) projections
  across 8/16/32-token tiles with `TileReuse` separating Hadamard basis reuse from
  quantized `ProjectionTile` preparation reuse, and parallelizing per-tile token
  normalizations (`layer.norm`, `layer.post_norm`, post-convolution Q/K `l2_norm` and
  `alpha`/`beta` gates) across `RowParallelism` workers.
- Four-row × four-token (`4x4`) consumption-order `PQ2` (`dot_pq2_four_rows` with
  stack-scoped `Pq2Activation` deinterleaving, unsigned `[0..3]` digit decoding,
  `f16_bits_to_f32_x4` scale decoding and vectorized `f32x4` FMA/add reduction) and
  four-row `PTQ1` / `PQ2 (Q8_0)` (`dot_q8_four_rows`) NEON prefill `GEMM` kernels wired
  through `RowParallelism::fill_tile_four_rows` (`be62567`), preserving the
  zero-heap-allocation invariant on `scratch.pq2_consumption` and per-row error/cancel
  rewind semantics.
- Eight-row × eight-token (`8x8`) consumption-order `PQ2` (`dot_pq2_eight_rows` with
  `f16_bits_to_f32_x8` 8-row scale decoding and 8-token activation preparation reuse),
  eight-row `PTQ1` / `PQ2 (Q8_0)` (`dot_q8_eight_rows` widening each token's `Q8Block`
  once across 8 rows), 8/4-row multi-token `BF16` prefill `GEMM` (`dot_bf16_rows` with
  `reuse_prepared` sharing across sibling `ssm_beta`/`ssm_alpha` projections), and
  multi-worker parallel tile activation quantization (`prepare_input` across
  `RowParallelism` workers) (`929b6c7`).
- Four-head grouped-KV full-attention score and value-tile reuse (`dot_f16_four`,
  `dot_f32_four` and `value_tile_four` in `bb9d218`), reading and converting each cached
  `FP16` key and value row once per four query heads sharing the same KV head while
  preserving each head's independent 4-register key dot and 16-lane temporal value-tile
  FMA reduction trees.
- Three-sibling full-attention tiled-prefill NPU batching (`project_f32_triple_tokens`
  and `project_tile_triple` in `e8deb79`), packing `[attn_q, attn_k, attn_v]` across
  4-token windows into 4-op `submit_resident_multi4` `dspqueue` submissions with
  pointer-deduplicated `rpcmem` activations and `submit_resident_batch3`/`batch2` tail
  batches.
- Parallel chunkwise GDN prefill causal short-convolution, shared 16-key-head `(K·Q, K·K)`
  triangular dot-matrix precomputation with zero per-layer heap allocations, parallel
  full-attention KV head RMSNorm/RoPE/FP16 preparation (`ffd43cb`), and 4-token-per-group
  register-blocked `dot_q8_four_rows`/`dot_q8_eight_rows` NEON prefill kernels (`d20214a`).
- Single-dispatch head-major multi-token GDN causal short-convolution + `SiLU` + Q/K
  `l2_norm` (`conv_head_major` into `gdn_conv_outputs`), L1-resident row-major exact GDN
  recurrent tiled prefill (`exact_head_into` using `gdn_decay_and_dot`), paired NEON GDN
  dot kernel (`pinned_gdn::dot_pair` loading `state` / `key_prior` once per `(key, query)`
  pair), and batched multi-token full-attention tiled prefill (`full_attention_tile` with
  per-tile shared `rope_frequencies` across all 16 full-attention layers and single-pass
  grouped-KV cache scans across worker token chunks) (`5656461`).
- Eighteen-worker CPU row-parallelism ceiling (`MAX_ROW_WORKERS = 18` in `cd5ee64` matching
  the 18-core Snapdragon X2 Elite / X2 Elite Extreme topology), `TL1`-style 16-entry INT16
  activation-pair nibble lookup tables (`Tl1BlockLut` in `src/kernels/tquants.rs` for
  contiguous and 32-strided 128-element ternary blocks), compile-time `TQ1_BYTE_LUT` and
  `TQ2_BYTE_LUT` byte tables, 128-bit NEON SIMD `TQ2_0` block dot products
  (`neon::dot_tq2_block`), direct `PTQ1 × Q8_K` dot products (`dot_ptq1_q8k` in
  `src/kernels.rs`), and training-free prompt/history suffix speculative decoding
  (`SuffixDraftConfig`, `SuffixDrafter` / `SuffixDecoding`), Leviathan stochastic
  verification (`verify_stochastic_step`), and transactional short-chain block verification
  (`Runtime::generate_speculative`, `Runtime::verify_draft_block`, and
  `SwiftSession::complete_speculative` in `cada2ca`).
- Single-write fused GDN recurrent-state row step (`pinned_gdn::step_row_from` and
  `pinned_gdn::step_row_in_place` in `bbca1b3`), keeping each decayed 128-element state row
  in NEON registers across `dot_k` and `update_and_dot` to eliminate ~`301.9 MiB` of
  recurrent-state memory traffic per token across 48 GDN layers; 1/4/8-row NEON integer dot
  kernels for `Q8_0`, `Q4_0`, `Q4_1`, and 5-bit `Q5_0`/`Q5_1` (`dot_q8_0_block`,
  `dot_q8_0_eight_rows`, `dot_q4_0_block`, `dot_q4_0_eight_rows`, `dot_q4_1_block`,
  `dot_i16_block`) plus multi-worker `RowParallelism::fill` dispatch across all legacy,
  `K`-quant, `I`-quant, `TQ`, `FP4`, and `Q1_0`/`Q2_0` matvec rows (`dc0bd7a`); and
  single-pass row-parallel tiled prefill for all non-`PQ2`/`PTQ1` quantized projections
  (`supports_tiled_rows`, `dot_quantized_tile_row`, `dot_q8_0_tile_row`, and
  `dot_q4_0_tile_row` in `e53bd6a`), streaming each weight row once across up to 32 prefill
  tokens and reusing prepared `Q8`/`Q8_K` activations across compatible sibling projections.

Remaining CPU kernel, graph-level parallelism and tiled-prefill optimizations to
implement and qualify in Prism:

- Evaluate optional ARMv8.2-A `dotprod` / `i8mm` dispatch behind runtime feature
  detection where arithmetic contracts permit.
- Qualify actual prompt/response latency and complete application lifecycle for the
  [shared-slice row-parallel candidate](docs/oxibonsai-reuse.md#implementation-status),
  beyond the retained [fixed-step and short service evidence](docs/native-rust-engine.md#bounded-parallel-row-qualification).
  Complete a matched request-throughput pair after the [bounded attempt](docs/evidence/native-request-speed.json)
  retained only the one-worker measurement and cancelled the four-worker request
  at its shared soft deadline. Prepare a workload and owner allowance that fit
  both requests and every cleanup/retention cost inside one five-minute attempt;
  keep decode timing separate from request TPS and do not retry this capture.
  Preserve shared packed weights and prepared Q8 activations, complete state,
  cancellation, pool/task accounting and whole-request cost. Leave further shapes,
  formats and worker budgets pending; do not split a longer matrix across short jobs.
  Review exact source and released bytes before new selections; fixed-step timing
  and one short service stream do not qualify prefill, decode TPS or defaults.
- Qualify full-model and memory behavior of NEON PQ2, odd-group PQ2 and PTQ1. Preserve
  scalar forcing, diagnostics, fourth-symbol support and [intermediate LUT and
  correction bounds](docs/cpu-kernels.md#packed-arithmetic) before narrowing. Prototype
  direct PTQ1-by-Q8_K scalar on a few shapes, then NEON, preserving grouping, raw
  scales, quantized activations, float order and odd-group fallback. Keep its numerics
  separate from PTQ1-by-Q8_0 through full-model and adapter qualification.
- Compare at most four NEON layouts on three shapes, one matrix at a time, unpacking
  into consumption order without VNNI. Compare at most two exact TL1_1/TL2_1-style LUTs
  with ternary eligibility and generic PQ2 fallback; charge preparation, overlapping
  source and repacked storage, tables, cache and startup.
  Qualify the implemented scoped PQ2 NEON digit-plane activation preparation:
  original Q8_K and packed weights stay intact, while rows and compatible siblings
  share reordered activation bytes. Charge the additional 5,280 logical scratch
  bytes at 5,120 columns and require full-model parity plus matched total request
  benefit before release integration; no local throughput gain is established.
- Qualify the source tiled projection-prefill family for valid token tiles 8/16/32,
  with shared prepared activations and each packed block decoded once across token
  dots, without persistent INT8/FP16 expansion. The real graph consumer now selects
  baseline NEON reductions on eligible targets and preserves explicit scalar forcing.
  The source candidate now inherits the existing private row-worker pool. Compare
  it with ordinary NEON/workers and stop if decoded block construction, spills,
  scheduling or scratch erases whole-prompt gains.
- Establish installed Windows CPU/OS NEON/dotprod/I8MM eligibility before dispatch.
  Inspect Release code generation and forbidden-ISA leakage and require detection,
  parity and benefit under the [dispatch
  contract](docs/cpu-kernels.md#execution-and-dispatch).
- Optimize measured transforms, elementwise work, quantizers, reductions and prefill
  through safe APIs or audited isolated unsafe code. Cover edges, malformed input,
  rollback, non-ARM and absent ISA; recheck dependencies, licenses and codegen.
  Qualify the [fused recurrent copy/decay source candidate](docs/native-rust-engine.md#bounded-parallel-row-qualification)
  on actual-model logits/state, throughput and resources before releasing and
  repinning it; fewer copied bytes alone do not establish a speed or RAM gain.
- During planned tiled prefill, attention and chunkwise GDN work, check whether
  signed padded/relative index mapping introduces repeated floor division by a fixed
  non-power-of-two constant. Consider [Joffe's algorithm](docs/cpu-kernels.md#signed-constant-floor-division)
  within that measured parent hotspot; existing 8/16/32 tiles already permit shifts.
  Choose tile sizes for parent-kernel benefit; preserve unsigned PTQ1 decoding,
  floating quantizer rounding and runtime-stride semantics. The current audit found
  no eligible Runner caller;
  implement a justified consumer in Prism with full-range integer parity, pinned-Rust
  ARM64 codegen and paired complete-request CI evidence before repinning. Keep this
  conditional on useful parent work, without a standalone utility or benchmark project.
- Qualify the bounded PQ2 product-pair candidate above against the earlier
  [two-accumulator implementation](docs/oxibonsai-reuse.md#neon-retain-concepts-preserve-swift-arithmetic).
  Inspect exact baseline ARM64 Release codegen, every intermediate bound and spills
  on the selected shapes, then compare whole requests. Preserve Q8 groups, all four
  PQ2 symbols and float order; require owner-managed device benefit before repinning.
- Compare scalar Rust, optimized Rust and Prism in paired device CI, separating
  hot-cache and streaming weights, preparation, dispatch, prefill, decode and task
  costs, allocations and sustained energy and heat. Scope profiles by device, format,
  shape, phase and objective.

## Adreno Vulkan

Follow [Adreno contracts](docs/adreno.md); discovery alone changes no CPU profile.

- Capture native GPU, driver and API features, limits and [memory
  telemetry](docs/hardware-capabilities.md#native-vulkan-observations), rejecting
  emulation and preserving unknowns and conservative admission. Resolve X2 `host-gpu`
  headless-worker `instance-creation` failure (`VK_ERROR_INCOMPATIBLE_DRIVER`), using
  retained stage/result diagnostics and bounded current-worker module snapshots;
  resolve protected GDI32 reference lookup error `126` despite module-snapshot
  presence; the count-only graphics query was not reached. Do not treat that
  lookup failure as proof of the Vulkan failure cause or ICD identity;
  retain bounded on-disk loader/new-module hashes for dependency review. Module names
  and on-disk hashes do not identify failed DLLs, bind mapped-image bytes or establish
  signature and dependency provenance.
  Tool-policy X2-90 observations neither explain it
  nor qualify worker access.
- Pilot one native ARM64 OpenCL packed projection candidate and Swift shape in Prism:
  scalar PQ2-by-Q8_K control, exact grouping, raw scales and float order, separate PTQ1
  lane. Prove query and workload access under unchanged worker mitigations; refusal is a
  no-go. Follow [access/source
  boundaries](docs/adreno.md#opencl-access-and-source-audit), hard 30-minute feasibility
  and two-hour initial-CI caps. Charge preparation, scratch and transfers; preserve
  reserves. Exclude whole-model expansion and profile selection.
- Build a separately locked pinned-Prism `GGML_VULKAN=ON` candidate with reviewed ARM64
  SDK, shader, compiler and library identities, unrelated accelerators disabled and
  bounded experimental profile preserving service policy and CPU lane.
- Complete the PQ2/PTQ1 operation, placement, fallback and transfer ledger and
  descriptor checks without limit overrides. Qualify packed operations, full logits, KV,
  GDN and lifecycle, awkward shapes, Vulkan Q8_1 versus CPU Q8_K/Q8_0 and separate
  no-LoRA and approved-LoRA basis and forward lanes under [arithmetic/placement
  rules](docs/adreno.md#swift-arithmetic-and-placement).
- Compare CPU, partial and maximum feasible offload and eligible Hexagon, separating
  prefill and decode, batches, threads, waits, preparation, packing, submission,
  transfers, GPU service, sampling and migration; distinguish slow kernels from host
  starvation. Qualify attention, cache and Flash changes separately.
- Optimize measured kernels, layouts, residency, descriptors, pipelines, allocations,
  scheduling and caches: packed and coalesced layouts, vector unpacking, shape tiles and
  supported replay; textures require native support. Charge cold preparation and copies,
  count physical memory once, bound caches and preserve asynchronous lifetimes. Rerun
  numerical, state, adapter and lifecycle gates.
- Qualify device benefit, quality, comfort, memory, admission and effective-plan
  reporting, absence or loss, pressure, stale evidence, cancellation and upgrades and
  committed-state CPU rollback under [resource/failure
  rules](docs/adreno.md#resources-failure-and-acceptance). Qualify Rust GPU coverage and
  device safety separately after Rust CPU parity.

## Hexagon NPU

Follow [Hexagon contracts](docs/hexagon.md).

Extend the [supervised actual-model probe](tools/hexagon/model-probe.md) beyond
the retained [guarded-HVX token control](docs/evidence/hexagon-model-device-20261103-ac/summary.json)
and [signed HMX two-token control](docs/evidence/hexagon-hmx-session-20261103/summary.json).
The latter completed 80 operations and 11,141,120 independently checked plane
values for tokens 42 and 43, with complete per-token CPU-shadow logits/state
equality and explicit cleanup. Its original supervisor rejection is preserved:
the new validator misread the required driver-owner pinning flag. The corrected
validator accepts the retained terminal event in a model-free replay; no device
retry ran. Effective HMX placement/VTCM and complete vendor teardown remain unknown.
Broader activations/quantizer edges, nonzero adapters, prefix/failure lifecycle,
effective device resources, wider placement and persistent serving remain
outstanding. Retain all separate closed historical workloads. Neither control
establishes measured speedup or production eligibility.

Extend the [new bounded adapter and resident-host controls](docs/evidence/npu-adapter-progress-20261103/summary.json)
beyond their declared first-projection tokens. Standard-adapter/1.3 execution
passed for [42,43,128] with NEON/four workers, 120 signed HMX operations and
16,711,680 checked integer values; loaded-zero passed for token 42 with 40
operations and 5,570,560 checked values. The separate resident-host screen
passed complete logits/state parity for the same three standard-adapter tokens,
prepared 25,067,520 packed bytes once and observed two cache hits. That explicit
screen uses graph parity without per-dot CPU replay; do not transfer the earlier
integer-audit acceptance to it. All three owned Jobs ended empty with explicit
native cleanup. The snapshot-admission controls and Release builds now pass;
the earlier [four-token failure](docs/evidence/hexagon-adapter-session-20261103/summary.json)
and the new corrected import compilation failure remain retained and closed.
Broader activations/history, raw numerical replay, effective device resources,
native lifetime, persistent serving and application delivery remain pending.

The separate [exact host-packing controls](docs/evidence/npu-host-reduction-20261103/summary.json)
pass the same standard-adapter three-token streaming and resident placements
on Prism `16c5e53`. The zero-allocation two-bit transpose replaces 128 individual
symbol updates with eight four-byte copies per original block. Fourteen focused
arithmetic/failure controls, formatting, strict Release library Clippy and the
probe build passed; both signed controls matched complete logits/state and
ended with explicit native cleanup and empty Jobs. Streaming independently
checked all 16,711,680 returned integers; resident mode retains graph parity
without that per-dot oracle. CPU/native arm endpoints observed AC, saver off
and 48% charge. Resident CPU evaluations took 362-403 ms versus 420-477 ms
with native offload; no useful benefit or before/after packing speedup is
established. The original clocks remain closed, and production admission and
the installed application remain unchanged.

The [batch-four HMX controls](docs/evidence/npu-batch4-controls-20261104/summary.json) implement and verify the [signed batch-operation route](docs/hexagon-source-audit.md#batching-existing-signed-block-operations) on Prism
`6f31095`: each queue packet carries one buffer descriptor, twelve tensor descriptors, four
`K=128/N=8` operation descriptors and four reserved profile records (`1,816` command bytes in a
`2,048`-byte header), reducing first-projection queue writes from 40 to 10 per token (`30` writes
for `120` logical operations across tokens `42`, `43` and `128`) while preserving separate
logical-operation and queue-write counters under the unchanged 24,000-operation cap, full
`n_tensors=12` and `n_ops=4` response checks, descriptor regeneration and complete batch output
validation before ordered per-block CPU reconstruction. All 73 Prism unit tests, 10 Runner
`model_device` tests, formatting, strict Release Clippy and Release builds passed. Both supervised
`batch4-streaming` (`4,755,456` mapped bytes, all `16,711,680` returned plane values independently
checked) and `batch4-resident` (`29,822,976` mapped bytes with `25,067,520` resident packed weight
bytes, `1` prepared tensor and `2` cache hits under `graph-parity-only`) matched complete per-token
CPU logits and logical state and ended with explicit native cleanup and empty Jobs. The workload
clock opened on battery (`82%`) before AC was connected during build setup; all four CPU/native arm
endpoints in both device controls observed AC, saver off and `97%` charge (`141.81` seconds charged
out of `540.0` seconds). Batching reduced `nativeSubmitAndWaitMilliseconds` to `33.91-35.52` ms
(streaming) and `31.16-32.05` ms (resident), with resident native evaluations taking `446.33`,
`431.31` and `434.37` ms versus `398.77`, `385.18` and `386.30` ms on CPU. The follow-up
[host-reconstruction and multi-projection resident HMX controls](docs/evidence/npu-multi-projection-20261104/summary.json)
on Prism `ebc22c2` and Runner `bbc63034` eliminate redundant unused streaming-slot zeroing on
resident `frame_batch4`/`frame` submissions (`25.1` MB saved per projection), replace `5.57` million
scalar `truncf` checks per projection with exact `i16` roundtrip validation, fuse
padding/reconstruction plane checks into a row-major `batch_four` reconstruction loop (`4x` fewer
strided `tensor.data` passes while preserving exact per-128 FMA/nonfused order), and extend the
probe and supervisor to bounded multi-projection resident placement (`1..=2` resident projections
within the `64` MiB arena, `1..=4` streaming projections, beginning with `blk.0.ffn_up.weight`). All
74 Prism unit tests, 14 Runner `model_device` tests, formatting, strict Release Clippy and Release
builds passed. Under continuous AC power (`100%` charge across the outer `132.25`-second run within
the `540.0`-second predeclared allowance, with AC, saver off and `100%` charge at all four arm
endpoints of both device controls), `single-projection-batch4-resident` (`blk.0.ffn_up.weight`,
`120` ops in `30` queue writes, `29,822,976` mapped bytes) and `two-projection-batch4-resident`
(`blk.0.ffn_up.weight` + `blk.0.ffn_gate.weight`, `240` ops in `60` queue writes, `54,890,496`
mapped bytes with `50,135,040` resident weight bytes, `2` prepared tensors and `4` cache hits) both
matched complete per-token CPU logits and logical state across tokens `42`, `43` and `128` and
completed explicit native cleanup with empty owned Jobs. Single-projection native evaluation reached
`487.59`, `466.35` and `432.66` ms (`32.53-34.22` ms submit/wait) versus `412.34`, `389.91` and
`415.44` ms on CPU (`+17.22` ms on token 2); adding the second resident projection doubled
`nativeSubmitAndWaitMilliseconds` to `64.66-66.22` ms (`~32.5` ms per projection across `10` queue
writes) and raised native evaluation to `553.37`, `514.98` and `530.48` ms versus `579.56`, `452.71`
and `411.89` ms on CPU. The subsequent
[batch-ten HMX queue, bounded spin-polling and parallel L1-tiled reconstruction controls](docs/evidence/npu-batch10-fast-queue-20261105/summary.json) on Prism
`5136bd1` and Runner `580a0062` increase HMX queue batching to `10` `K=128/N=8` operations per
packet (`4,504` command bytes in an `8,192`-byte header, `n_tensors=30`, `n_ops=10`, reducing each
40-block projection from `10` to `4` queue writes), add bounded 15-millisecond spin-polling in
`Session::read` on `queue_read` `516` (`AEE_EWOULDBLOCK`) before 1 ms OS timer sleep fallback, and
parallelize 8-plane dot reconstruction across all four `RowParallelism` workers with 64-row L1
tiling (`accumulate_batch<B>`) while preserving exact per-128 FMA/nonfused accumulation order. All
81 Prism unit tests, 15 Runner `model_device` tests, formatting, strict Release Clippy and Release
builds passed. Under battery operation (`acConnected: false`, `batterySaver: false`, `89%` charge
across the outer `43.36`-second run within the `540.0`-second predeclared allowance and at all four
arm endpoints of both device controls; do not combine this battery session with prior AC-connected
sessions for cross-session performance comparisons), `single-projection-batch10-resident`
(`blk.0.ffn_up.weight`, `120` ops in `12` queue writes, `36,954,112` mapped bytes) and
`two-projection-batch10-resident` (`blk.0.ffn_up.weight` + `blk.0.ffn_gate.weight`, `240` ops in
`24` queue writes, `62,021,632` mapped bytes with `50,135,040` resident weight bytes, `2` prepared
tensors and `4` cache hits) both matched complete per-token CPU logits and logical state across
tokens `42`, `43` and `128` and completed explicit native cleanup with empty owned Jobs.
`nativeSubmitAndWaitMilliseconds` dropped to `22.21-23.48` ms for 1 projection (`12` queue writes)
and `44.08-46.26` ms for 2 projections (`24` queue writes, `~5.5-5.8` ms per 10-op packet), while
4-worker L1-tiled reconstruction kept 2-projection native evaluation (`525.34`, `513.61` and
`537.70` ms) close to 1-projection native evaluation (`529.85`, `502.53` and `487.91` ms). Because
the signed `ggml-htp` DSP binary executes each `K=128/N=8` `OP_MUL_MAT` descriptor independently and
writes 8 separate `FP32` bitplane vectors (`5.57` MB per `batch10` packet, `22.28` MB per
projection) back to shared memory, each projection still incurs `~22.2-23.5` ms of DSP
execution/writeback plus `~10-15` ms of host reconstruction versus `~2-3` ms on 4-worker NEON CPU;
prioritize evaluating whether multi-block `K>128` weight tiling or DSP-side plane/scale reduction is
supported by any signed Hexagon package symbol before wider admission. The
[static K>128 tiling, plane-reduction, and 8-HVX/2MiB-VTCM solver audit](docs/evidence/hexagon-k-tiling-and-reduction-audit.json) (reproduced by
`python -B tools/hexagon/verify_k_tiling_and_reduction_audit.py`) and
[8-HVX / 2 MiB VTCM / Burst-mode HMX controls](docs/evidence/npu-hmx-8hvx-2mib-vtcm-20261105/summary.json)
on Prism `9f61217` and Runner `76e3b38b` resolve that `K>128` tiling and DSP-reduction gate: (1)
`core_dot_chunk_fp16` (`hmx-mm-kernels-tiled.h:966-1040`) accumulates across all `K/32` dot tiles
inside HMX and truncates the output to IEEE-754 `FP16` via `HMX_STORE_AFTER_F16`
(`__fp16 *vtcm_output`) before widening to `FP32`, while `PQ2_0` carries distinct per-128-element
`FP16` scales and `Q8_K` carries per-256-element `FP32` scales accumulated via ordered per-128-block
`f32::mul_add` (`0xc1f90b01` golden vs `0xc1f90800` under `K=256` baked-scale
`HMX_STORE_AFTER_F16`); (2) `HTP_OP_SUM_ROWS` (`sum-rows-ops.c:1-215`) reduces along `ne[0]=17408`
rather than across `ne[1]=8` bitplanes, and `htp_matmul_is_hmx_eligible` requires `M > 4`; and (3)
enabling all 8 128-byte HVX contexts (`HMX_Q2_HVX_WORKERS=8`, `kparams.n_threads=8`,
`kparams.n_act_threads=4`, `div_n_act_threads=(1,0,2)`), raising `HMX_Q2_VTCM_REQUEST_BYTES` from 64
KiB (`n_chunk=128`, `136` chunks/op) to 2 MiB (`n_chunk=4352`, `vtcm_size=1,576,960` bytes, `4`
chunks/op with zero tail waste, `34x` fewer synchronous HMX/DMA/HVX chunk barriers), and selecting
`HTP_POWER_MODE_BURST` (`7`) cut `nativeSubmitAndWaitMilliseconds` by `2.4x-2.8x` under continuous
AC power (`95%` charge, saver off across the `199.09`-second run within the `300.0`-second default
budget and at all four arm endpoints of both controls): `8.18`, `9.33` and `13.28` ms for
`single-projection-batch10-resident` (`320.31-536.98` ms native evaluation vs `294.61-342.29` ms
CPU) and `26.77`, `16.84` and `15.60` ms for `two-projection-batch10-resident` (`~7.80-8.42` ms per
resident projection on tokens `43` and `128`, with native evaluation at `457.34` and `464.30` ms vs
`552.44` and `553.09` ms on CPU, bitwise-equal logits/state and clean teardown). Because the signed
generic `Q2_0` DSP binary irreducibly writes `22.28` MB of `FP32` bitplanes to DDR per projection
(`~7.8-9.3` ms on DSP plus `~6-8` ms of host reconstruction vs `~2-3` ms per projection on 4-worker
NEON CPU), beating 4-worker NEON CPU per projection under 0-ULP bitwise parity requires a custom
fused `PQ2_0 × Q8_K` DSP kernel with an accepted signing route. Under bounded floating-point
tolerance (`executionBackend: "native-q2"`, `integerAudit: "bounded-float-tolerance"`), the
[1-op full-K native Q2_0 HVX controls](docs/evidence/npu-native-q2-1op-20261105/summary.json) and [decision](docs/evidence/npu-native-q2-1op-20261105/decision.json) on Prism `ccb1339` and Runner `cd3485bb`
transcode `PQ2_0` weights losslessly at preparation time into `llama.cpp`'s native tiled
`HTP_TYPE_Q2_0` layout (repeating each 128-weight `FP16` block scale across both 64-weight tiles)
and dispatch the entire `[5120, 17408]` projection as a single `K=5120, M=17408, N=1` operation
(`hvx_mv_params` `kernel_type=7`, `n_hvx=8`, `n_prefetch=4`, `vtcm_size=1,912,832`), plus expose the
signed `v81` HTP opcode descriptors (`HTP_OP_RMS_NORM`, `HTP_OP_RMS_NORM_MUL`, `HTP_OP_GLU_SWIGLU`,
`HTP_OP_UNARY_SILU`, `HTP_OP_MUL`/`ADD`/`SUB`/`DIV`, `HTP_OP_ROPE`, `HTP_OP_FLASH_ATTN_EXT`,
`HTP_OP_SSM_CONV`, `HTP_OP_GATED_DELTA_NET`, `HTP_OP_HADAMARD`, and VTCM pass flags). Under
continuous AC power (`99%` charge, saver off across the `32.26`-second run within the `300.0`-second
default budget and at all four arm endpoints of both controls), DSP-side reduction shrinks
shared-memory output traffic from `22.28` MB of bitplanes to a single `69,632`-byte `F32` vector
(`320x` less output traffic per projection, zero host bitplane reconstruction) and cuts
`nativeSubmitAndWaitMilliseconds` to `0.9832`, `0.6769` and `0.6436` ms for
`single-projection-native-q2-resident` (`3` ops in `3` queue writes) and `1.4439`, `1.0908` and
`1.0821` ms (`~0.541` ms per `[5120, 17408]` projection, faster than 4-worker NEON CPU's `~2-3` ms)
for `two-projection-native-q2-resident` (`6` ops in `6` queue writes, with token `128` native
evaluation at `506.15` ms vs `516.49` ms on CPU), while matching all `248,320` vocabulary logits
across tokens `42`, `43` and `128` with cosine similarity `0.999906-0.999974`, RMSE `0.0221-0.0286`,
exact top-1 token agreement (`1183`, `12`, `248`), and `5/5` top-5 overlap. The follow-up
[full-layer and 13-projection multi-layer native Q2_0 HVX controls](docs/evidence/npu-full-layer-native-q2-a383e92-20261105/summary.json) and [decision](docs/evidence/npu-full-layer-native-q2-a383e92-20261105/decision.json) on Prism
`a383e92` (retaining the [closed 27.687-second diagnostic at 8a7980a](docs/evidence/npu-full-layer-native-q2-20261105/decision.json) that identified unconditional
`HmxQ2WeightRegistry` capacity checking in `Session` construction after `htp_iface_start`) verify
`ggml-hexagon.dll` `.data` buffer limits (`m-size = 512 MiB`, `v-size = 3000 MiB` on `v81`,
`h-size = 1024 MiB`) and `libggml-htp-v81.so` `4 MiB` VTCM acquisition, expand
`NativeQ2ProjectionLimits` to `K <= 17,408`, `M <= 17,408`, `4 MiB` VTCM (for `K > 6,144`
projections such as `ffn_down` `[17408, 5120]` where `n_prefetch=4` uses `2,949,376` bytes VTCM),
and `256 MiB` resident weight arena (`NATIVE_Q2_MAX_RESIDENT_WEIGHT_BYTES`), parallelize host
`PQ2_0 -> HTP_TYPE_Q2_0` tile packing across 4 row workers via `RowParallelism::update_chunks`, and
admit all 10 `PQ2_0` layer projection types across recurrent and full-attention layers. All 91 Prism
unit tests, 16 Runner `model_device` tests, formatting, strict Release Clippy and Release builds
passed. Under battery operation (`acConnected: false`, `batterySaver: false`, `90%` charge across
the `32.971`-second run within the `300.0`-second predeclared allowance, `60.658` seconds cumulative
including the closed `8a7980a` diagnostic; do not treat battery evaluation wall-clock windows as
qualified performance comparisons), `recurrent-layer-native-q2-resident` (all 6 `blk.0` projections:
`ffn_up`, `ffn_gate`, `ffn_down`, `attn_qkv`, `attn_gate`, `ssm_out`, `18` ops in `18` queue writes,
`132,804,608` mapped bytes with `107,642,880` resident weight bytes, `6` prepared tensors and `12`
cache hits) and `multi-layer-13proj-native-q2-resident` (all 6 `blk.0` recurrent projections plus
all 7 `blk.3` full-attention projections: `ffn_up`, `ffn_gate`, `ffn_down`, `attn_q`, `attn_k`,
`attn_v`, `attn_output`, `39` ops in `39` queue writes, `237,498,368` mapped bytes with
`212,336,640` resident weight bytes, `13` prepared tensors and `26` cache hits) both passed bounded
float tolerance across tokens `42`, `43` and `128` (`cosineSimilarity` `0.999893-0.999977`, `rmse`
`0.0228-0.0306`, `maxAbsDiff` `0.1080-0.1724`, exact top-1 token agreement `1183`, `12`, `248`, and
`5/5` top-5 overlap) and completed explicit native cleanup with empty owned Jobs, with total
`nativeSubmitAndWaitMilliseconds` at `2.8738`, `2.2704` and `2.3168` ms across all 6 projections of
`blk.0` (`~0.378` ms per projection) and `5.4553`, `4.3826` and `4.7865` ms across all 13
projections of `blk.0 + blk.3` (`~0.337-0.420` ms per projection). The subsequent
[scoped-flush 13-projection and 4-layer 25-projection native Q2_0 HVX controls](docs/evidence/npu-4layer-25proj-native-q2-20261105/summary.json) and [decision](docs/evidence/npu-4layer-25proj-native-q2-20261105/decision.json) on
Prism `e2349af` add `dirty_resident_weights` tracking to `Session` in `projection_native.rs` so the
first submission after `prepare_weights` flushes the full allocation while subsequent
`Weights::NativeResident` cache-hit submissions scope `QueueBuffer.size` (`dspqueue_buffer.size`) to
the page-aligned active working prefix
(`(active.output + rows * 4).next_multiple_of(4096) <= 94,208` bytes = `92 KiB` while keeping
`bufs[0].size` at `self.expected.size`), and raise `NATIVE_Q2_MAX_RESIDENT_WEIGHT_BYTES` from
`256 MiB` to `480 MiB` (`503,316,480` bytes within `ggml-hexagon.dll` `m-size = 512 MiB`),
`NATIVE_Q2_RESIDENT_METADATA_BYTES` to `256 KiB` (`262,144` bytes), and `max_projections` to `26`.
All 92 Prism unit tests, 16 Runner `model_device` tests, formatting, strict Release Clippy and
Release builds passed. Under battery operation (`acConnected: false`, `batterySaver: false`, `76%`
charge across the `37.13`-second run within the `300.0`-second predeclared allowance; retained as
diagnostic observations rather than AC-qualified performance comparisons),
`multi-layer-13proj-native-q2-scoped-flush` (`blk.0 + blk.3`, `13` projections, `39` ops in `39`
queue writes, `237,498,368` mapped bytes with `212,336,640` resident weight bytes, `13` prepared
tensors and `26` cache hits) reduced warm-token `nativeCheckedEvaluationMilliseconds` from `555.44`
and `586.48` ms (at `a383e92`) to `422.49` and `446.20` ms (beating the 4-worker NEON CPU baseline's
`463.97` and `478.07` ms on tokens `43` and `128`, and `1,386.00` ms vs `1,392.70` ms across all 3
tokens) with `4.8742`, `4.4271` and `4.5424` ms of NPU submit-and-wait per token (`~0.341-0.375` ms
per projection), and `four-layer-25proj-native-q2-resident` (all 25 `PQ2_0` projections of the
complete 4-layer repeating block `blk.0 + blk.1 + blk.2 + blk.3`, `75` ops in `75` queue writes,
`452,784,128` mapped bytes with `427,622,400` resident weight bytes, `25` prepared tensors and `50`
cache hits) completed all 75 operations across tokens `42`, `43` and `128` in `10.9094`, `10.3379`
and `10.0536` ms of NPU submit-and-wait per token (`~0.402-0.436` ms per projection) with bounded
float tolerance pass on all 3 tokens (`cosineSimilarity` `0.999895-0.999972`, `rmse`
`0.0217-0.0316`, `maxAbsDiff` `0.1163-0.1669`, exact top-1 token agreement `1183`, `12`, `248`,
`5/5` top-5 overlap, and clean teardown). The earlier resident screen's CPU evaluations took 502-536
ms and native evaluations 512-633 ms, with no per-dot audit in either native window; these short
diagnostic windows show no clear benefit and qualify no performance gate. Its CPU/native power
endpoints both observed AC with saver off; the earlier streaming and loaded-zero cases observed
battery. Do not combine those different power conditions into a streaming-versus-resident benefit
comparison. The
[independent default-feasibility review](docs/evidence/npu-default-feasibility-20261103/independent-review.json)
retains the still-applicable lifetime, production integration and benefit blockers.
Keep production NPU admission disabled; the longer session allowance does not
implement persistent serving or establish a speed advantage.

Qualify the 28-projection single-arena (`c574d6c`/`d8afaec` in Prism `0.1.17`, `eafcca5b` in Runner)
and multi-arena `native-q2` resident placement expansions (`545f43d`..`e60b11f` in Prism,
`0cc5f7d7`..`3895fbb5` in Runner) on settled main through owning hosted checks, supervised
AC-connected device receipts and the pinned Runner release-bundle `0.1.23` (`055c743`):

- Multi-arena resident weight allocation (`NATIVE_Q2_MAX_WEIGHT_ARENAS = 6`, alternating
  `[502_824_960, 460_062_720, 502_824_960, 460_062_720, 502_824_960, 460_062_720]` byte arenas up to
  `480 MiB` (`2,888,663,040` bytes / `2,754.84 MiB` total across `blk.0..=blk.26` with zero wasted
  bytes within CDSP `v-size = 3,000 MiB` on `v81`, `DEFAULT_NATIVE_Q2_PROJECTIONS = 168` in
  `e60b11f`, with full 6-arena `total_used` accounting fixed in `c8e96d7`), 4-op and 8-op
  multi-operation resident `dspqueue` submissions (`submit_resident_multi2..4`,
  `submit_resident_multi8` and `image_multi_resident` with shared-activation pointer
  deduplication), intra-layer fused 3-op (`submit_resident_ffn_swiglu` in `c8e96d7`) and
  2-token 6-op (`submit_resident_ffn_swiglu_multi2` and `image_ffn_swiglu_multi2_resident`
  in `48364f8`) `ffn_gate` + `ffn_up` + `HTP_OP_GLU_SWIGLU` VTCM tensor chaining
  (`project_f32_ffn_swiglu` and `project_f32_ffn_swiglu_tokens` wired into
  `Evaluation::project_ffn_swiglu_into` and `PrefillWorkspace::project_tile_ffn_swiglu` when
  no rank-one LoRA adapter is active, keeping intermediate `[17408]` FFN gate/up vectors in
  DSP VTCM via `HTP_OP_FLAG_BOTH_IN_VTCM` and writing back only one `69,632`-byte
  post-`SwiGLU` `F32` vector per token to DDR), and multi-token
  tiled-prefill NPU dispatch (`project_f32_tokens`, `project_f32_pair_tokens` and
  `project_f32_triple_tokens` wired through `Qwen35::ingest_cpu_tile`, `gdn::recurrent_tile`,
  `PlacedProjectionExecutor` and `WorkerThreadHexagonExecutor` in `e8deb79`/`e60b11f`), building on
  the 2-arena (`blk.0..=blk.8`, `962.89` MB) and 4-arena (`blk.0..=blk.17`, `1,836.56` MiB) each
  within `ggml-hexagon.dll` `m-size = 512 MiB`, `NATIVE_Q2_MAX_PROJECTIONS = 56`,
  `DEFAULT_NATIVE_Q2_PROJECTIONS` covering all 56 `PQ2_0` projections across the first 9 transformer
  blocks `blk.0..=blk.8`, `962.89` MB total weight capacity pre-warmed at worker startup) and
  grouped sibling `PQ2_0` dispatch (`execute_f32_projection_group`, skipping redundant CPU `Q8_K`
  quantization when all `PQ2_0` siblings in a group execute on the NPU `F32` executor).
- Run and archive supervised AC-connected device controls for
  `eight-layer-50proj-native-q2-resident` (`blk.0..=blk.7`, 50 projections, `855.24` MB
  across 2 arenas) and `nine-layer-56proj-native-q2-resident` (`blk.0..=blk.8`, 56
  projections, `962.89` MB across 2 arenas) within a single predeclared owner-device
  allowance, verifying bounded float tolerance, per-arena mapping/unmapping, scoped
  active-prefix cache flushes and complete native teardown.
- Remaining Hexagon NPU performance and placement work:
  - Evaluate expanding `MAX_NATIVE_Q2_ARENAS` beyond 2 arenas (`3..=6` `<= 480 MiB`
    `rpcmem` weight arenas within CDSP `v-size = 3000 MiB` on `v81` and host SMMU/Job
    commit limits) to cover additional transformer blocks (`blk.9..=blk.63`).
  - Implement multi-operation `dspqueue` packet batching and intra-layer VTCM tensor
    chaining for `native-q2` (fusing `HTP_OP_RMS_NORM`, sibling `Q2_0` projections,
    `HTP_OP_GLU_SWIGLU`, `HTP_OP_ROPE`, `HTP_OP_FLASH_ATTN_EXT`, `HTP_OP_SSM_CONV` and
    `HTP_OP_GATED_DELTA_NET` using the wired `v81` opcodes and VTCM pass flags) to
    eliminate per-projection host-to-DSP queue synchronization and intermediate DDR
    round-trips.
  - Add multi-token `N > 1` `native-q2` prefill dispatch (`N = tile_tokens` in
    `HTP_TYPE_Q2_0` `OP_MUL_MAT`) so prompt prefill reuses resident NPU weights across
    8/16/32-token tiles instead of executing `N = 1` per token or falling back to CPU.

### Promotion sequence

Resume with **NPU-1**. Close each applicable gate with retained, exact-source
evidence before enabling a daily-use profile. The detailed work below supplies
the implementation leads; this checklist owns the order and acceptance decision.
The [guarded-HVX receipt](docs/evidence/hexagon-model-device-20261103-ac/summary.json)
and [HMX receipt and review](docs/evidence/hexagon-hmx-session-20261103/summary.json)
cover only their declared first-projection tokens. Together with the new bounded
adapter/cache controls, they do not qualify the 24,000-submission lifetime,
broader adapter histories, effective HMX resources, persistent serving or benefit.
NPU-1 remains open for the broader packet and remaining prerequisites.

- [ ] **NPU-1 — Prepare the next qualification packet.** Freeze the selected
  Runner/Prism source, executable/toolchain, original-PQ2 model, adapter,
  X2 SKU, driver, runtime/catalog and selected DSP identities. Close applicable
  native ABI, SDK/license and signed-distribution prerequisites; retain
  independently retrievable raw inputs, outputs, hashes and numerical oracles.
  Extend the supervised probe for broader live activations/quantizer edges,
  multiple tokens and explicit adapter cases before selecting device work.
  Preserve local versus hosted provenance and retain individual cleanup return
  codes plus the required driver-owner lifetime pinning policy. The HMX receipt
  now carries these fields; the original HVX pass retains its aggregate
  source-bound completed-cycle acceptance.
- [ ] **NPU-2 — Qualify application arithmetic and adapters.** Compare every
  offloaded integer result, final logits and complete committed state against
  the same-artifact CPU oracle for varied prompts, activations, shapes and
  generated history. Cover the selected scalar/NEON paths and default policy
  of four CPU row workers, loaded-zero adapter identity, and the standard nonzero adapter at
  scale 1.3, including scale/history changes. Require no quality or representation
  drift. Qualify PTQ1/odd-block Q8_0 arithmetic independently if selected; the
  original-PQ2 pass transfers no acceptance to those paths.
- [ ] **NPU-3 — Close placement, resources and lifetime admission.** Bind
  observed selected DSP bytes, effective VTCM/DMA/mapping/cache behavior and
  each selected tensor/geometry to its real graph placement. Charge host packing,
  scratch, resident arenas, state, RPC and device allocations outside Job commit
  against the daily-service limits; the probe's 18 GiB free-memory threshold is
  not daily-service resource acceptance. Reserve the entire unreused prompt and
  maximum output plus prior attempted work before submission. Qualify relevant
  real-device session/24,000-attempt lifetime behavior within a feasible declared
  workload; resets/checkpoints must not replenish capacity. One all-eligible
  evaluation fits, while a hidden token plus evaluation needs 45,912 operations.
  Broader serving needs explicit partial placement or a separately reviewed
  supported lifetime/fused design, with no implicit cap or duration increase.
- [ ] **NPU-4 — Qualify persistent serving and failure recovery.** Cover fresh,
  reused and edited prefixes, repeated requests/tokens, full context/output
  reservations, cancellation before/after submission, stop/retry, reset,
  interrupted or failed continuation and committed-state CPU rollback. Exercise
  queue saturation, timeouts, unsupported shapes, memory pressure, allocation/
  mapping failure, device loss and stale caches. Require bounded ownership,
  isolation and verified native/vendor teardown; preserve uncertain buffers
  until supervised exit, with no secret replay or partial CPU continuation.
- [ ] **NPU-5 — Establish useful end-to-end benefit.** Use measured bottlenecks
  to choose placement, fused packed kernels, device weight reuse and true
  multitoken prefill. Qualify signed HMX arithmetic/placement if that route is
  selected; the two-token signed HMX arithmetic control does not qualify effective
  placement, resources or DSP residency. Compare the resulting profile against the matched CPU default
  at fixed model, adapter, prompts, outputs and settings. Retain repeated
  prefill/decode and complete-answer latency, throughput, memory, quality and
  sustained-comfort evidence, charging preparation, transfers, RPC, synchronization
  and startup. Apply the [promotion criteria](#promotion-criteria): at least 10%
  lower end-to-end latency, or accepted capacity/energy benefit within latency
  and quality limits. Retain per-arm AC/battery/saver/charge observations and
  uncertainty; missing or mismatched power cannot qualify a performance comparison.
  The existing 51,689 ms supervisor duration is not an inference-speed measurement.
- [ ] **NPU-6 — Deliver and qualify the explicit application profile.** Release
  and pin the native Rust helper/runtime and supported signed kernels, then wire
  checked placement/readiness into the broker/launcher with truthful effective-plan
  reporting. Validate authentication, exact counting, one-slot/deadline/cancellation
  policy, child-only environment, override rejection and isolation/resources beyond
  one process Job. Cover clean installation, driver/runtime absence or changes,
  upgrades, pressure and verified original-PQ2 CPU rollback. Keep the first-party
  UI native Rust and preserve the published CPU runtime's dependency boundary.
  No failure may silently change engine, representation or submitted work.
- [ ] **NPU-7 — Independently review and promote the scoped release.** Score
  correctness, adapters, lifecycle, placement/resources, quality/comfort, benefit,
  packaging/isolation and rollback against one settled main/release/device tuple.
  Retain independent numerical and human review plus reproducible evidence; a
  reviewer of receipts alone does not independently rerun the hardware/oracle.
  Enable only the passing explicit profile, document its supported subset and
  no-go cases, and invalidate/requalify evidence after relevant source, model,
  adapter, driver or kernel changes. Keep normal chat on CPU and NPU admission
  disabled until these applicable gates close.
- [ ] **NPU-8 — Qualify later backend/family expansion separately.** Connect
  measured request-boundary CPU/GPU/NPU selection only after common-evidence,
  resource and fallback checks. Adreno comparisons require a qualified GPU path;
  X1 requires independent device evidence. Unselected HMX, PTQ1, X1 and automatic
  selection research do not block an otherwise qualified explicit X2 partial-HVX
  profile or inherit its qualification.

For each later owner workload, declare fixed work, a conservative estimate and
the smallest justified allowance under [the execution budgets](#research-scope-and-practical-budgets),
including setup, every arm's power observations, cleanup and archival. The two
2026-11-03 workloads remain closed; do not reuse their clocks, retry failures or
divide a long matrix into repeated laptop runs. Complete suites and benchmarks
remain CI-only; the owner alone dispatches, monitors and judges CI. Pending CI
does not block local preparation completion. Retain sustained acceptance as
pending when it cannot fit an eligible environment's limits.

### Detailed qualification work

Qualify the [source-only Prism graph integration](https://github.com/symbiogenesis/prism-inference/tree/1e7ed92e7787b9d8b150f9bb3fb5a74fa92fb14b/tools/hexagon-runtime)
and bounded native HMX session on settled main. Exact PTQ1/odd-block PQ2 Q8_0
composition and complete-request operation admission now have
[focused model-free controls](docs/evidence/hexagon-ptq1-admission-preparation.json).
The previous integration's owning hosted Hexagon/Foundation checks passed.
The [current residency/placement preparation](docs/evidence/hexagon-resident-preparation.json)
retains its own hosted checks as pending until their final conclusions are reviewed;
neither source validation establishes signed-device acceptance. Runner still pins
the CPU release. Close signed PQ2/PTQ1 arithmetic
and full-model parity, persistent serving,
effective placement/resources and same-artifact performance before enabling NPU
admission or calling this full acceleration.

The [reproducible traffic ledger](docs/evidence/hexagon-swift-traffic-ledger.json)
derives 13.69 GB of logical HMX submission/result payload for one all-eligible
original-PQ2 evaluation token, excluding source reads and further host copies.
This is no measured traffic or speed verdict. Qualify the opt-in immutable mapped
host-weight arena, bounded graph cache and explicit partial placement through
signed execution, actual resources and same-artifact repeated-token comparison.
The [current resident-host bounds](docs/evidence/hexagon-resident-host-24000-ledger.json) separate
saved CPU packing from unchanged signed-kernel payload and potentially increased
whole-allocation cache maintenance. The arena does not establish DSP residency,
efficient full inference or a daily service session. Develop signed fused packed
kernels, device weight reuse and true multitoken prefill scheduling before treating
broader placement or a raised session cap as a performance solution. The owner
authorized a 24,000-submission terminal cap on 2026-11-03. Retain complete-request
admission and the existing absolute duration/cleanup limits; qualify selected real graph work,
logits/state, cancellation and teardown before publishing experimental output.

Use the [bounded native F32/Q2 evidence](docs/evidence/hexagon-x2-queue-compute.json) as primitive provenance beneath the passed live-model
projection. Its fixed guarded-Q2 integer and explicit cleanup cycle do not qualify a complete
projection, model, selected DSP bytes, vendor teardown or speed. Retain timed-write14 and old
close-before-unmap512 as losing observations. The investigation charged 223,797 ms of its single
300,000 ms owner allowance including preparation, capture and final review; its remainder cannot fit
another supported control plus teardown. Hosted waits and date changes do not reset it. That
historical investigation stays closed; select distinct future qualification work through the
promotion sequence above.

- Complete independent X1/v73 access and extend X2/v81 numerical, deployment,
  license/resource and effective-placement qualification. Bind the
  [driver-bound generic X2 dispatch](docs/evidence/hexagon-x2-driver-bound-dispatch.json)
  and [native FD cycle](docs/evidence/hexagon-x2-native-fd-cycle.json) to independently
  observed selected DSP bytes, mapping modes and teardown; neither identifies the
  complete device tuple. Verify DMA64 and effective VTCM against matching signed
  binaries. Trust/Secure Boot changes need a separate owner decision.
- Qualify the separate [newer signed Q2 candidate](docs/hexagon-source-audit.md#newer-signed-q2-candidate)
  before choosing a packed route. Its v0.10.0 archive, seventeen runtime files and
  five catalog members are [verified as data](docs/evidence/hexagon-signed-q2-package.json);
  extend the passed live-model projection beyond token 42 to broader inputs,
  shapes, quantizer edges and long-lived queue ownership. Compare the
  bounded guarded integer-dot
  composition with a directly fused packed kernel; ordinary Q2/Q8_1 substitution
  does not preserve Swift Q8_K/per-128 FMA. Custom kernels still require a supported
  signing/distribution operator, but they are not the only signed route to research.
- Review and release the experimental native Rust capability/terminal-buffer boundary
  prepared in Prism without changing its publication boundary. After the successful
  fixed module-open/4 KiB FD map/unmap/free/close and live first-projection cycles,
  qualify broader native computation, delayed/extended mapping, asynchronous
  ownership and complete vendor teardown.
  The explicit reference initializer requests an unsigned DSP process class while
  retaining Windows signed-package/catalog checks; it does not establish effective
  process class or authorize OS trust changes. Preserve the independent STATIC512,
  default-open14 and old mutable-input guard observations without assigning causes.
  Bind generated QAIC host marshalling/wire controls to exact SDK/tool and imported
  IDL identities under the [queue provenance gate](docs/hexagon-source-audit.md#native-rust-computation-requires-another-reviewed-boundary).
  The [hosted SDK recipe review](docs/evidence/hexagon-sdk-inventory-41168872514/provenance.json)
  retains the host command template. The [third data review](docs/evidence/hexagon-sdk-functional-41171339745/provenance.json)
  observes matching normalized remote/AEEStdDef IDL tokens and selected queue
  declarations, while whole queue/remote headers differ. Bind the SDK's unreviewed
  `remote_work.h` and effective Windows conditionals/layout/calling conventions
  and import closure before Rust invocation; the public Windows `remote_ext.h`
  input also remains unpinned. Establish applicable SDK tool/output terms; the uncertain NOTICE
  decoding and keyword observations establish no grant. The [public generated-C/Rust
  wire control](docs/evidence/hexagon-public-qaic-41171859264/provenance.json) passed
  for one fixed 48-byte start frame and null-argument stop on hosted Linux LP64,
  replaying pinned dependency inputs. Preserve its exact source/compiler identities
  when extending coverage; it does not qualify SDK/public equivalence, Windows
  ARM64 ABI, queue ownership, signed device computation or whole-model behavior.
  Preserve the reviewed Windows explicit-zero/read-noblock route and symbolic
  timeout default. Compare public infinite-wait, FD/cache/callback/import-close
  contracts and authoritative u64 close against the matching SDK; the reference
  wrapper's narrowing pointer typedef is not an ABI to reuse.
  Extend the fixed native Rust F32/Q2 observations and independent Windows ARM64
  queue/packet and public-branch start/stop wire controls beyond the passed live
  projection to longer lifecycle evidence. Retain exact-source hosted tests,
  numerical output,
  correlated responses, bounded waits, in-use buffer retention and explicit
  stop/close/unmap evidence. The public
  wire control selects the non-Windows Qualcomm header branch on a native ARM64
  compiler; it does not establish the missing Windows whole-header closure or SDK
  equivalence. Preserve the signed guarded-Q2 arithmetic when consuming a projection.
- Extend the passed `blk.0.ffn_up.weight` control, K=5120/M=17408/N=1, keeping
  transforms, adapter corrections and committed state on CPU. Consume the [verified original projection
  and signs](docs/evidence/hexagon-swift-projection-binding.json) through the
  [CPU operation/wire controls](tools/hexagon/packed-projection.md), extending signed
  device outputs, actual activations, logits and complete state comparisons beyond
  the one accepted token before timing or expanding placement. Preserve Q8_K
  preparation and original per-128 FMA; qualify
  PTQ1 canonicality/Q8_0 independently. The model-free PQ2/Q2 weight-layout control
  adds 419,840,000 bytes for a whole packed copy and is not a production bridge.
  Extend qualification of the bounded guarded-Q2 projection submitter, including its source-native
  576-byte host tile stride and separate 640-byte aligned VTCM destination stride.
  Full-M scheduling retains 40 original-block operations; broaden the fixed-shape
  signed solver/quantizer coverage and obtain total-cost evidence. Extend actual
  placement/fallback/transfer accounting, including the guarded
  primitive's additional weights, activations, returned limb dots and CPU reduction.
  Effective VTCM, DMA descriptors, session/queue/mapping costs, all host scratch and
  resources outside Job commit remain admission charges; the proposed 18,752-byte
  fused double-buffer reserve covers only its declared tiles.
  Retain the prepared original-artifact external-activation probe as an optional
  diagnostic for isolating arithmetic or wire failures, with the
  [source-bound hosted controls](docs/evidence/hexagon-original-projection-preparation.json).
  The live-graph control remains the acceptance baseline. Any newly selected
  diagnostic still needs its own feasible declaration.
  Enforce every-limb and final bitwise comparison,
  correlated operation counts, complete explicit cleanup and atomic publication.
  External activation bytes do not establish live-model provenance. Broader
  graph/state/logit coverage, adapter parity and end-to-end benefit remain
  separate gates after the one-token pass. Keep production NPU admission disabled
  until the applicable promotion sequence closes.
- Qualify the implemented HMX integer-bitplane packing/reconstruction control
  against the matching signed K=128/N=8 path. Its eight rows represent activation
  bits of one token, with original CPU Q8_K and per-128 FMA retained; they do not
  establish batched prefill. Bind native HMX session/solver fields, effective
  placement and resources, every returned plane, full-model state/adapter parity
  and total request cost before admission. The bounded host control and independent
  original-layout/FMA/cancellation checks establish no NPU speedup or PTQ1 support.
- Evaluate HVX packed work and HMX batched prefill with bounded VTCM, DMA and repacking
  scratch, exact fusion/recurrent basis and independent adapters. Cover ring saturation,
  partial tiles, double buffering, requested and effective VTCM, allocation refusal and
  transfer cancellation. Use the documented DMA-ring bug as a regression pattern without
  claiming a Swift defect.
  After the full-M baseline, qualify the [source-derived K=128/N=8 integer-bitplane
  HMX candidate](docs/hexagon-source-audit.md#hmx-integer-bitplane-source-lead).
  Qualify the implemented HMX-enabled native session/solver, exact plane reconstruction,
  padded activation/output resources and signed-device parity before timing.
  Its lower derived weight traffic and extra returned planes establish no TPS gain.
  Resolve the [static whole-model operation limit](docs/hexagon-source-audit.md#static-whole-model-operation-limit)
  before full-model admission: selecting every eligible original-PQ2 base
  projection requires at least 22,656 operations per hidden token and 23,256
  including logits. The owner-authorized 24,000-operation terminal cap fits one
  evaluation with 744 submissions left, but a hidden token followed by an
  evaluation needs 45,912 and still rejects. Retain the historical 21,760-cap
  ledger and use the [current budget](docs/evidence/hexagon-session-operation-budget.json).
  Plan explicit qualified placement and total session work before generation;
  smaller row tiles increase the count. Qualify a separately reviewed lifetime
  or fused design before persistent serving; do not replay failures or replenish
  capacity through resets. Extend the scoped accepted first-projection model worker
  under source-bound local supervision before broader placement.
- Qualify complete ownership and restore for fresh, reused and edited prefixes,
  cancellation, reset and failed continuation; qualify chunkwise scheduling separately.
  Account for host/device mappings and copies, RPC, state, caches, load overlap and
  resources outside Job commit. Verify X1 mapping windows and signed X2 DMA64; start
  with one session and justify partitions within reserves.
- Compare CPU, qualified Adreno, contiguous Hexagon and mixed phases at fixed artifacts
  and settings on X1/X2, prioritizing packed X2 prefill/decode when available. Charge
  startup, migration, traffic, RPC, synchronization, contention and sustained power;
  attribute queues, quantization, DMA, HVX/HMX, wake and waits. Phone rankings and
  unavailable root/DVFS controls do not choose Windows profiles.
- Connect the offline Rust comparison policy to independently verified archived
  captures and safe request-boundary selection only after qualification. Bind metrics,
  effective phase placement and common comparison digest to the same trusted evidence;
  recheck signed readiness and complete resource admission. Use device/workload and
  interactive/battery results to choose CPU, GPU or NPU dynamically, with independent
  X1/X2 scope, strict-backend rejection and unchanged CPU fallback. Ordinary startup
  must not benchmark, infer eligibility from TOPS or stitch unrelated phase timings.
- Integrate a locked experimental profile with checked placement and readiness,
  child-only environment and helper teardown, authentication, counting, one-slot,
  deadline and cancellation policy. Reject overrides and qualify isolation and resources
  beyond one process Job.
- Exercise unsupported shapes, bounded waits, mapping or allocation failure, device
  loss, post-submit cancellation and stale caches under [failure
  rules](docs/hexagon.md#memory-lifecycle-and-failure). Preserve in-use buffers and
  complete committed state or close admission through verified teardown, without secret
  replay or partial CPU continuation.
- Publish scoped profiles/no-go results from paired repeated CI: short and near-limit
  prompts, full output reserve, fixed and natural completions, edited and reused state,
  AC/battery, pressure and 15–30-minute sustained use in an eligible environment.
  Leave that sustained gate pending when setup, work and retention cannot fit the
  owner's aggregate ceiling; do not split it across repeated laptop trials.
  Retain raw evidence, uncertainty,
  LoRA parity, CPU rollback and [upgrade gates](docs/hexagon.md#qualification-evidence).
  Qualify Rust NPU coverage and device safety after Rust CPU parity, preserving signed
  deployment and full state.

## Survey ideas outside the current laptop backlog

The [research catalog](docs/research-register.md#research-catalog) owns excluded
literature: unavailable ISAs, datapaths and proprietary kernels, Android-root/QPT/DVFS,
new architectures/readers, large drafter training, full-Hessian pipelines and wholesale
lattice/rotation refits. GPU results alone justify neither a laptop job nor replacing
already-ternary Swift; bounded native preflights need the research budgets. Existing
Thinking/effort controls remain the baseline for methods requiring training.

Defer DASC/Tail-Replay checkpoint omission until exact reuse and suffix recomputation
cannot meet a real need. Wide trees, SVD caches and sparse-attention indexes need a
measured batch-one/8K bottleneck and bounded native benefit. Multi-tier or multi-request
serving is not a total-RAM saving here. [Withdrawn PolarQuant/unresolved
Litespark](docs/research-register.md#withdrawn-or-unresolved-reports) remain negative
evidence, not tasks.

## Deferred Prism migration

Reopen broader [Prism](https://github.com/symbiogenesis/prism-inference) work only for a
Swift dependency or chosen milestone. The [conformance
contract](docs/prism-upstream-tests.md) owns exact inventories, assertions and
exclusions. Remaining areas:

- Reference provenance and qualification: all backend partitions, 22,817 ordered
  evaluation and support geometries, 1,530 separate performance workloads and 70
  Windows-applicable source results, excluding Apple. Ratchet complete reviewed evidence
  only; retain required green IDs and full diagnostic handoffs.
- [Numerical/codec
  APIs](docs/prism-upstream-tests.md#numerical-operations-and-codec-contracts):
  operations, custom/map callbacks, backward graphs, remaining importance encoders,
  codec qualification and owned `qb-*` APIs.
- [Chat/parser/tokenizer
  behavior](docs/prism-upstream-tests.md#chat-parser-and-tokenizer-contracts): general
  templates and parsers, capability execution, server conversions and every tokenizer
  source.
- [Acquisition/model/session
  APIs](docs/prism-upstream-tests.md#acquisition-model-and-session-contracts):
  production CLI, retrieval/resolution, generic model/session APIs, architecture graphs
  and original model, state and concurrency sources.
- [DSpark/adapters/grammar/ABI](docs/prism-upstream-tests.md#dspark-adapters-grammar-and-abi):
  DSpark, actual LoRA conversion/merge/inference, LLGuidance and safe public C ABI.
  Preserve approved corrections and compile-only `test-c.c` scope.

## Deferred dependency experiments

- Follow the [audit](docs/dependency-audit.md) and [MXC capability
  issue](https://github.com/microsoft/mxc/issues/1579) before supervisor changes. Retain
  three Windows wgpu backends; removal needs support, device and fallback acceptance.
  Split archive APIs only for demonstrated independent-speech build or size benefit;
  preserve containment, accessibility and native UI.
- Qualify the 2026-11-08 Prism & Runner kernel, accelerator, speech, transport and
  dependency integrations in their owning CI workflows: `bonsai-neon-dot` (`SDOT`/`SMMLA`
  `2x2` tiles) and `u8x16` `PTQ1_0` unpacking, last-layer state-only prefill narrowing,
  fused `RMSNorm + Sign*FWHT` / `SwiGLU + Sign*FWHT` NEON block kernels, streaming NEON
  top-k sampler pre-filter and low-jitter `parallel_tile` barrier, Hexagon NPU full
  intra-layer FFN VTCM chain (`image_ffn_full_chain_resident`) + `DMA64` stride guard +
  hybrid CPU/NPU prefill scheduler, GDN rollback-by-replay verify tape (`GdnVerifyTape`) +
  `DFlash2DraftConfig` + checkpoint-free greedy step verification, Adreno `Q2_0+FWHT`
  3-processor placement planner (`plan_three_processor_placement`), `bonsai-native-hash` /
  Windows CNG SHA-256 routing, `fearless_simd` NEON + scoped multi-row
  `LinearProjection::forward_vec_parallel` + 4-frame TTS streaming + `FastConformer`
  `[B, C, T, F] -> [B, T, C * F]` transpose in `bonsai-speech-tts`/`bonsai-speech-asr`, and
  direct `memchr` NEON scanning in `bonsai-broker`/`bonsai-chat`/`bonsai-speech-service`.
- Qualify [SIMD-JSON tape
  reuse](docs/dependency-audit.md#current-candidate-and-pending-gate) in main-only CI:
  Unicode, duplicate/unknown fields, shapes, bounds and canonical bytes; at least 5%
  lower equal-weight geometric-mean time and no case over 5% slower in **both
  cold/reused screens**, including scales. Retain the current dependency decision until
  every gate passes.

## Optional agents

- Qualify creative chat first; evaluate OpenCode, then Aider, through a bounded
  authenticated inference bridge.
- Qualify MXC ProcessContainer isolation for agents, plugins, hooks and descendants:
  `allowDaclMutation:false`, no untrusted audit mode or unsandboxed fallback;
  filesystem/reparse, handle, subprocess and network bypass checks. A VM tier with a
  separate kernel needs separate acceptance.

## Promotion criteria

The owner-directed Rust/PQ2 development default keeps its explicit disclaimer and
manual publisher rollback. It does not satisfy or waive this full-qualification
policy, and does not change the offline promotion evaluator or PTQ1 admission.

Apply [validation's promotion criteria](docs/validation.md#promotion-criteria):
independent correctness, security, full-state, quality and resource gates, sustained
comfort without sustained paging, paired target-device benefit and reproduction on
settled `main` with immutable identities and rollback. The initial performance gate is
**10% lower end-to-end latency**, or documented capacity/energy benefit within accepted
latency and quality limits.
