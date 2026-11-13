# Validation

Runner validates its native UI, broker, process/artifact policy, provisioning and
released-runtime integration.
[Prism](https://github.com/symbiogenesis/prism-inference/blob/main/docs/build-and-validation.md)
owns algorithm/upstream conformance. **Local builds, Release analyzers and named
focused tests are allowed. Full suites and benchmarks are CI-only; local filters
must not reconstruct a suite.**

The owner operates Host/Guest CI; other CI needs explicit direction. The sole
standing exception is [AGENTS.md's bounded Foundation release-bundle operation](../AGENTS.md).
Workflow documentation authorizes no dispatch/query. Record CI acceptance as
pending without delaying local completion.

Owner-laptop diagnostics, qualification and benchmarks default to five minutes
total, preferably less. A dedicated workload may predeclare the smallest justified
extension, up to fifteen minutes. Before starting, record fixed work, a conservative
estimate and the reason. Include preparation, intake, loading, every comparison arm,
cleanup and evidence retention; stop on completion or a known no-go. This applies
to self-hosted CI; narrower limits need explicit extension, and hosted cloud CI
keeps its own limits. Reduce work or leave acceptance pending if it cannot fit.
Never reset clocks, retry a failed trial or partition a matrix across repeated runs.
Historical clocks stay closed.

Within that allowance, record AC/battery before and after each trial and each
comparison arm, plus battery saver/charge when available. Keep unknowns explicit.
Missing, changed or mismatched power makes performance comparisons ineligible.
Equal endpoints do not prove continuous stability; later queries cannot establish
past conditions.

Native application acceptance records model-case `powerBefore`, `powerAfter` and
`powerConditions`, with separate observations for each owner, representation or worker
comparison arm. These Rust endpoints follow workflow checkout, package intake and
release verification; they do not establish power during that earlier setup or
later receipt uploads. The owner must separately retain pre-setup and post-workload
aggregate observations within the same declared allowance before another device trial.
Absent aggregate observations remain unknown; later case snapshots cannot fill them.
Native numerical and request-timing captures retain the same
observations for each model sample and the overall capture. Queries use the
existing read-only Windows power API. Unknown or mismatched AC/battery-saver
conditions set `performanceComparisonEligible: false` and leave eligible timing
ratios null; explicitly unqualified ratios and raw observations remain available.
Completed answers and original-PQ2 rollback correctness are evaluated separately.

Physical Snapdragon capture/qualification workflows, including speech/sandbox,
share the non-cancelling `swift-lora-snapdragon` concurrency group. Hosted targets
have independent groups. Serialization prevents overlapping laptop CI jobs,
without proving the machine otherwise idle. [PLAN.md](../PLAN.md) owns pending acceptance.

## Runner gates

### Actual-model application acceptance

The main-only [native-runtime acceptance workflow](../.github/workflows/native-runtime-acceptance.yml)
defaults to `run_device: false`, preparing an exact-source package on hosted ARM64.
The owner must review that package and predeclare one feasible workload before
selecting `run_device: true`. Device dispatch builds its own same-run package;
cached compiler outputs never supply acceptance receipts. Ignored model cases
never run in ordinary local or Foundation checks.

Preparation requires strict Release all-target Clippy and production linking for
`bonsai-chat`, `bonsai-broker` and `bonsai-tools`. The schema-2 package contains
`native-model-acceptance.exe`, `bonsai-tools.exe`, `bonsai-ci.exe`,
`strict-release-analyzers.json`, `workflow-contract.json` and `receipt.json`.
The shared [admission guard](../tools/ci/README.md#native-acceptance-admission)
validates source/compiler identities, exact inventory, successful checks and
workflow syntax/regression receipts at publication and intake. Renderer/default/
rollback and `owner_shutdown_`/`owned_broker_` controls run without a model;
actual native UI Stop/Retry remains an independent device gate.

#### Budget and cleanup

`owner_budget_minutes` selects 5, 10 or 15 minutes. Extended totals require
`budget_reason` recording fixed work, estimate and why five minutes cannot fit.
UTC and monotonic clocks start before checkout/intake; charge the larger elapsed
value, rounded up. An inherited shorter allowance remains binding.

| Bound | Requirement |
| --- | --- |
| Rust case allowance | At most 240/540/840 seconds for a 5/10/15-minute job, reduced by setup, verification and inventory. |
| Process exit | Ten seconds after the Rust allowance; stop at least 30 seconds before the original job deadline for forced cleanup, diagnostics and uploads. |
| Inherited work allowance | At most 270/570/870 seconds from the original monotonic start, retaining 15 seconds for owner exit and the ten-second process gap. |
| Adapter request work | Ends 30 seconds before the case deadline, including startup. Drop expired HTTP work before Stop; start none inside the cleanup reserve. |
| Further lifecycle/boundary request | Requires more than 45 seconds remaining. |
| Fresh owner | Requires more than 60 seconds remaining, except the rollback tail below. |
| Optional history/stall work | Requires more than 120 seconds for cleanup and fresh-owner recovery. |

Device intake verifies run/revision, SHA-256 and ARM64 identity, then executes
prebuilt native helpers without Cargo. All setup, owners, comparisons, power,
cleanup and retention share the declared allowance. Never retry, renew a clock,
continue missing steps in another job or divide a larger matrix into profiles.

The UI requires the broker's private model-cleanup acknowledgment, broker exit
and outer-Job cleanup before Retry: a PSEC model can bypass outer-Job inheritance.
Only connection refusal confirms listener release; timeout/access/network errors
leave it unknown. Task errors leave Job emptiness unknown. Stop retains ten
seconds to abort and await a stuck task within its existing 30-second bound;
synchronous native verification/teardown cannot be preempted by an async timeout.
Unknown cleanup blocks another owner and post-cleanup hashing.

#### Profiles

All adapter profiles bind the compiled runtime lock, verified model/adapter,
scale, sampler, canonical request/message digests and selected worker policy.
PTQ1 also binds its exact export receipt. Optional history/stall extensions are
accepted only by `pq2-no-adapter`; other profiles reject them. All require native
sandbox admission, unchanged resource limits and retained artifact guards.

| `profile` | Fixed workload |
| --- | --- |
| `pq2-no-adapter` (default) | Original PQ2: supplied-history streaming, framed context rejection/reuse, owned shutdown, abandonment after content and fresh restart. Core streams use four-token caps with thinking disabled. |
| `ptq1-adapter` | Current-weight PTQ1, adapter 1.3: one four-token stream and cleanup. Scale-zero two-token and invalid-scale close/reuse checks are optional only while the same cleanup reserve fits. |
| `pq2-adapter-row4` | Original PQ2, adapter 1.3, four workers: exactly one four-token stream, readiness and cleanup; no extra requests. |
| `ptq1-adapter-lifecycle` / `ptq1-adapter-lifecycle-row4` | One/four workers: three one-token streams at 1.3, zero, then 1.3 with captured assistant history; invalid-scale connection close precedes the last stream. User messages are `Hi`. |
| `pq2-adapter-lifecycle-row4` | Four workers: 1.3 with four-token cap, zero with two-token cap, invalid-scale close, then 1.3 with captured assistant history and two-token cap. |
| `ptq1-adapter-boundaries-row4` | Four `Hi` one-token streams at 1.3, negative zero, 2.0, then 1.3; invalid-scale close and oversized-context rejection precede the final stream. Final bytes, digest, finish reason and one-token usage must reproduce the initial response. |
| `ptq1-adapter-service-row4` / `pq2-adapter-service-row4` | Four workers: three `Hi` one-token streams at 1.3, negative zero and 2.0; invalid-scale and context rejections; interrupt a scale-1.3 request with a 4,096-token reservation after its first visible prefix. Verify cleanup, then use a fresh authenticated owner/new credentials for an exact initial-response replay and cleanup. |
| `ptq1-adapter-service-rollback-row4` | Complete PTQ1 service sequence, then a third fresh owner explicitly selects original PQ2, adapter 1.3 and four workers, naturally completes the fixed tree-name request within 32 tokens and cleans up. Verify the original file's complete CNG length/SHA-256 after cleanup. |
| `pq2-adapter-speed` | Fixed-order one/four-worker fresh-owner PQ2 pair, adapter 1.3, seed 42, same public counting prompt, thinking off, eight-token caps. |
| `ptq1-pq2-complete-response` / `ptq1-pq2-complete-response-row4` | One/four workers respectively: fresh original-PQ2 owner followed by current-weight PTQ1, adapter 1.3, seed 42 and production sampling. Same tree-name prompt, thinking off, 32-token ceiling; both must stop naturally. |

Captured-history profiles replay the exact initial visible assistant UTF-8 text,
bound to its digest, source-message identity, terminal reason and usage. Missing
output or identity keeps the gate pending; fixture text cannot substitute.
Negative-zero requests preserve scale bits through serialization. Request hashes
are not server acknowledgments of scale or numerical adapter-effect evidence.

Service profiles retain the interrupted prefix only after a complete SSE record,
including its blank-line delimiter. Already terminal SSE cannot qualify interruption.
The first owner must report three completed and three failed operations; the fresh
owner must reproduce the initial cap-one response with one completed operation and
no failures. Same-owner prefix equality is informational because sampler draws
advance; the fresh owner resets the seed.

The combined rollback profile withholds a 75-second PQ2 tail from the PTQ1
sequence. It rejects before starting at 135 seconds or less; the third startup
needs more than 75 seconds. Rollback request/startup work ends 45 seconds before
the final deadline, preserving 30 seconds for cleanup and 15 for full-file
verification. Hashing checks cancellation between bounded reads and holds file/
ancestor guards through the final receipt. `serviceIntegration` and
`originalPq2Rollback` remain separate partial verdicts; complete success also
requires post-cleanup hashing. Transport/parse failures retain cleanup and power.

| Optional default-profile workload | Contract |
| --- | --- |
| `bounded_history` (`BONSAI_NATIVE_EXTENDED`) | Reject 4,500 synthetic forest-note repetitions with a 4,096-token reservation; admit 128 repetitions in assistant history with a four-token cap, then complete another four-token stream in the same owner. Thinking stays off. Repetitions are not tokens. |
| `stalled_consumer` | Reserve 4,096 tokens, thinking off; read first nonempty content, retain the connection without reading for one second, then Stop. Finish within 30 seconds with empty Job/released listener, one additional failed operation and no completed operation. Require fresh-owner content after Stop and another successful fresh-owner request after abandonment. |

Skipped requested work records `status: pending` and
`insufficient-cleanup-and-recovery-reserve` where applicable. It leaves
`allRequestedWorkloadsPassed` and `passed` false even if `lifecyclePassed` is true.
Short caps and reservations establish neither long-history/full-output behavior
nor backend activity, socket saturation or cancellation latency at Stop.

#### Receipts and comparison limits

Save credential-free `acceptance.json` before startup and after each completed
step, cleanup and recovery phase with `passed: false`; set it true only after
complete success. Missing, skipped, failed or cancelled work stays pending and
fails after owned cleanup. Preserve partial observations before the next request.
`requestBudgetExpired: true`, `startup-budget-expired` and `startup-failed` retain
distinct failures. Incomplete startup phases cannot establish readiness or cleanup.

Successful streams require complete HTTP framing, a supported terminal reason
and `[DONE]`. Adapter streams require positive reported completion usage within
their cap; `length` must report exactly the cap. Missing/zero/excessive/inconsistent
usage remains pending even with visible text. Bytes and SSE events are not tokens.
Configured workers remain distinct from the effective graph budget independently
verified at readiness, which observes neither executing threads nor parallel work.

`startupProgress` brackets policy/admission, model/adapter verification,
credentials/runtime preparation, process creation, authenticated readiness,
native identity and frontend binding. Missing completion leaves a phase unfinished;
panic or outer termination can prevent the final receipt copy. Readiness combines
several internal stages and cannot locate a kernel hotspot. Observer overhead is
included; synchronous verification is not preemptible.

For the speed pair, `requestTokensPerSecond` is reported completion usage divided
by monotonic connection-to-framed-response-and-close time, including prefill,
generation and transport but excluding startup and parsing. Its ratio needs two
`length` completions at eight tokens, identical visible output and verified cleanup.
It is application request throughput, not decode TPS or TTFT.

Complete-response pairs require natural stops, positive usage and both cleanups;
`length` is truncated. Retain exact text/digests, identities, sampler/template,
startup/request times and original-PQ2 rollback observations before the next arm.
Latency/resource ratios require identical bytes and usage, eligible matched power
and matching released worker-readiness identities. Compare each positive,
consistent native accounting domain separately: lifetime Job commit, lifetime
root residency, sampled serving private commit and sampled serving residency.
Never add domains; sampled system available memory is context, not model usage.
Missing telemetry leaves resource comparison pending without discarding answers.

All cases retain aggregate and per-owner power endpoints under the rules above.
They qualify only their selected sequence. Complete-state/logit parity, loaded-zero
identity, authoring basis, edited/generated histories, full context/output,
repeated benefit, quality, comfort, isolation and promotion need independent gates.
The [promotion evaluator](native-rust-engine.md#default-runtime-promotion) keeps
those verdicts separate.

Diagnostics collect only `backend.stdout`/`backend.stderr` from at most eight
immediate protected sessions, at most 64 KiB/log with strict UTF-8. Validate both
keys, redact across every retained log and recheck before publication; invalid keys
suppress logs. Upload only the two fixed sanitized logs and
`startup-diagnostics.json` after regular-file/reparse/credential checks. Unsafe
directories remain untouched and suppress that artifact. Base acceptance receipts
remain separate. `host.json` stores fixed startup stage/error/exit markers without
raw exceptions, arguments or keys; the harness exit is recorded separately.

Model-free `native_` acceptance regressions in
[`tools/bonsai/tests`](../tools/bonsai/tests/) own usage/framing, profile selection,
deadline/reserve admission, partial receipts, worker identities, response/resource
comparisons, startup attribution and Stop/recovery controls. Select exact named
cases under [focused local checks](#focused-local-checks); these fixtures supply
no actual-model verdict.

### Bounded source request attribution

The [PTQ1 capture workflow](../.github/workflows/native-ptq1-numerics.yml) defaults
to `capture_mode: numerical`. Explicit `request-timing` selects one production
SwiftSession request through the same source-bound hosted checker package and
native Rust owner; it does not execute the native reference model or ABI probes.
Only current PTQ1 with the standard adapter at 1.3 is admitted, with one selected
backend and one/four-worker policy. The fixed public tree-name messages use thinking
disabled, seed 42, production sampling/stops, no prefix reuse, at most 128 rendered
prompt tokens and one reserved output token. A sampled stop may commit zero tokens.

Inspect the selection without a model or device operation:

```powershell
bonsai-tools native-qualification --json (@{ operation='plan'; captureMode='request-timing'; repositoryDirectory=$PWD.Path; rustBackend='neon'; rowWorkers=4 } | ConvertTo-Json -Compress)
```

Hosted preparation binds the selected mode and runs its model-free controls.
`run_device: false` prepares tools only. A separately selected owner capture uses
the same original five-minute aggregate allowance, parent identity, unchanged
admission/Job/reserve policy and 32-second verification/cleanup reserve. An absolute
checker deadline derives once from the remaining original monotonic allowance;
blocking loading remains externally supervised. No default changes or further
numerical job are part of this mode.

Retained raw started/final receipts bind source, executable and input hashes,
model/adapter identities, backend, workers, settings and deadline. Successful
terminal state, positive finite timing brackets and successful operation/token
counts must be internally consistent before attribution is accepted. Missing,
empty, failed or incomplete final receipts leave the attempted work unknown;
completed phase lower bounds do not establish emitted response tokens. The exact
allowlist retains native supervision and public inputs on failure. No-op text
consumption measures source-session work, with no HTTP/backpressure, isolated
kernel, complete-answer quality, service-TPS or default-promotion claim. Actual
device attribution remains pending until this explicit workload is captured and
independently reviewed; it cannot continue a previous numerical matrix.

### First-run setup and portable update regressions

The normal Foundation crate checks collect the model-free tests below. They must
remain executable without model weights or real release downloads; refactoring
must preserve their observable ordering, identities and failure boundaries.

| Contract | Regression owner |
| --- | --- |
| A fresh data folder uses compiled Swift pins, preserves explicit engine/model/adapter options and finishes setup before broker handoff; failure cannot start speech or inference | `chat_launch::tests::automatic_setup_` and `chat_launch::setup::tests::` in `bonsai-tools` |
| Valid files need no transfer; interrupted downloads resume; corrupt originals remain until a verified atomic replacement; unsafe paths, concurrent writers and cancellation cannot publish | `startup_download::tests::setup_repair_` in `bonsai-tools` |
| Runtime repairs use fresh staging, preserve unrelated files, check expanded storage and restore the previous directory if publication fails | Focused `provision::tests::` runtime setup and publisher repair controls |
| Pause beats a queued model handoff; retry preserves draft/history/options without sending; first-session persistence works; unknown cleanup disables retry and updates | `window::startup_tests::`, `window::setup::tests::` and `startup::tests::` in `bonsai-chat` |
| Missing, unreadable and changed model bytes have distinct safe guidance; untrusted diagnostic text cannot select a recovery path | `client::tests::model_verification_guidance` and broker artifact marker controls |
| Update discovery retains bounded numeric/prerelease selection, conditional metadata requests, compression and server-directed backoff without stale success on HTTP errors | `app_update::` in `bonsai-tools` |
| Updates retain publication/digest/source binding, repair poisoned staging within one deadline and preserve the original on failure. Download precedes the prompt; Later leaves a Settings badge; deferred close installs without reopening, while immediate installation restarts. Both require current release authorization and confirmed owned teardown; unfinished downloads and unknown cleanup cannot activate a handoff | `app_update::` in `bonsai-tools` and `window::updates::tests::` in `bonsai-chat` |

Select named controls or these focused filters under one predeclared local budget.
Complete crate/workspace suites and real download/update acceptance remain with
the owner-operated CI lanes. Synthetic passes cannot establish fresh-machine,
actual-model, public-network, replacement/restart or continuous-power acceptance.

### Foundation and routing

[Source checks](../.github/workflows/source-checks.yml) provide a limited
hosted Linux ARM64 portable screen for pull requests to `main` and manual dispatch: workspace
Rust formatting, portable CI/CPU-policy/broker compilation and six exact named
model-free routing, gate and request-policy regressions. A missing named case fails.
The workflow also owns Markdown standards on relevant pushes and pull requests,
and fresh dependency audits on relevant main pushes and the weekly schedule.
Changed paths select each job without a compiler; documentation-only changes skip
portable compilation. Missing or unusable ranges select every check eligible for
that event. Manual dispatch selects portable and Markdown checks on any branch,
and dependency auditing on `main`. Each check supersedes only its own pending
work, while manual checks keep independent runs. A failed selector fails every
eligible check before checkout or compilation. These read-only jobs run no complete suite,
model, benchmark or self-hosted job; they qualify neither native Windows behavior
nor actual-model/device acceptance.

External actions and their transitive dependencies use full SHA pins and exact
lockfile coverage. Same-repository `$/` references require self-hosted runner
**v2.338.0 or newer**. Before committing workflow/action/lock changes, follow the
[action-pin preflight and maintenance commands](../tools/ci/README.md#action-dependency-pins):
GitHub startup rejection prevents its own verifier from running. Pin review and
upstream verification remain distinct from offline startup/syntax checks.

[Foundation](../.github/workflows/foundation.yml) runs on `main` pushes/manual
dispatch, skipping root-Markdown/`docs/`-only pushes. Its [Rust planner](../tools/ci/README.md)
reads [impact routing](../.github/ci/impact.json), selects changed packages plus
transitive workspace consumers, and requires every selected lane/app group to
pass. Only planned skips are accepted. The [package registry](../config/rust-packages.json)
contains application packages, not upstream runtime catalogs.

| Group | Coverage |
| --- | --- |
| Broker, UI, speech, maintenance | Affected packages/consumers with independent caches |
| Native integration | Released GGUF integration, native reference/PE fixtures and admission-only checks; Rust maintenance tests own provisioning, plans, supervision and package lifecycle/tamper/path checks |
| Released runtime | Archive/install/profile/adapter/admission policy, GGUF wrappers, help switching and actual pinned Rust inference provisioning with `includeCapabilities: true` |

For example, a broker edit selects broker checks; shared Windows-boundary edits also select
broker/CLI consumers. CLI edits select CLI/app integration. Missing/unclassified diffs or changes to
workspace/lockfile/toolchain/CI inputs select full coverage. Manual dispatch selects everything.
Each workflow compares with its newest successful main-push ancestor, covering failed/cancelled
intervening changes. New automatic pushes supersede older Foundation push runs; manual dispatch
groups remain independent. Planner/gate regressions run first; the tested cached Linux planner
executable is handed to aggregation, avoiding a second compiler install/build or Cargo metadata.

The `maintenance` lane owns affected checks for the six supporting maintenance
crates, including Windows Job tests. The independent `checked-tools` lane owns
the maintenance CLI's checks and publishes the CLI plus feature-gated Rust
completion probe. Consumers verify revision, both digests and ARM64 identities;
lifecycle fixtures retain production headless mitigations. Speech and native
integration lanes depend on `checked-tools` without waiting for independent
maintenance, broker or UI checks. The aggregate gate still requires every
selected lane.
ASR and TTS use separate lanes. ASR uses the checked CLI to prepare its pinned
native compiler environment. Its Cargo cache prefixes also bind the checked
native build-host hash, and the lane retains that model-free host report.
TTS compiles entirely as Rust and uses the ordinary lane-specific Cargo cache;
it has no native static-library cache or compiler-preparation step. Cached
compiler output never replaces the selected checks or fresh receipts.

Pinned C++ `--version`/`--help` use fixed-command inspection with 512 MiB,
one-process Job limits, caller lifetime, output bounds and deadlines. Inspection
permits required shell-DLL initialization; inference retains Win32k denial.
Inspection success cannot qualify legacy worker startup. Rust discovery retains
the ordinary headless policy. Package/runtime checks prove neither signing nor
actual-model/device acceptance.

Maintenance/UI regressions cover early-window preparation, preserved drafts,
bounded output, failures, descendant teardown, warm/build-variant reuse, damaged
inputs/receipts, non-build versus embedded-input changes, interrupted publication,
model-only provisioning and resumable partial downloads. Synthetic compiler trees
must leave no descendants after cancellation/timeout/early root exit before lock
reacquisition. Environment checks preserve Unicode/profile entries and reject
protected overlays. Rust checks packages and single-instance launch. These establish
correctness, not startup speed or inference performance.

Upstream-only coverage runs in Prism's
[conformance](https://github.com/symbiogenesis/prism-inference/blob/main/.github/workflows/runtime-conformance.yml)
,
[Foundation](https://github.com/symbiogenesis/prism-inference/blob/main/.github/workflows/foundation.yml),
[standards](https://github.com/symbiogenesis/prism-inference/blob/main/.github/workflows/standards.yml)
and
[compiler](https://github.com/symbiogenesis/prism-inference/blob/main/.github/workflows/rust-nightly.yml)
workflows, including source/backend catalogs, baseline/codec corpora, overlays and
focused upstream diagnostics. Runner's
[LoRA qualification contracts](swift-lora.md#fixed-acceptance-contract)
retain their native planning and scoring commands. Actual capture requires reviewed
artifacts and a suitable hosted Snapdragon target; qualification remains pending.

### Paired speech comparison

Main-only manual [TTS evaluation](../.github/workflows/tts-evaluation.yml) alternates
fresh Q8_0/Q4_K_M VoiceDesign processes over bounded thread budgets, reversing pair
order between repetitions. It separates first-request voice bootstrap from warm
Base synthesis. The Rust supervisor owns one kill-on-close process, caller lifetime,
deadlines and bounded logs, with **9 GiB admission, 16 GiB commit and 5 GiB reserve**.

Receipts retain revision, CPU, exact artifacts/executable, readiness, first audio,
real-time factor and synthetic WAV clips. Report sampled 25 ms phase private/resident
maxima, OS lifetime working-set peak, Job commit peak and minimum system available
memory separately: sampling misses brief peaks, mapped residency is outside commit,
and Design/Base talkers never coexist. Retain unsigned numeric/hex exit status,
observation before/after shutdown and last phase/sample count. Pre-shutdown exit
rejects even after complete audio; an earlier failure retains precedence.
Failed sessions cannot enter completed paired comparisons.

All execution, even one short profile, remains GitHub-hosted CI-only; the native
evaluator and supervisor reject local, self-hosted and unknown runner environments.
The retained `snapdragon` dispatch target fails on a hosted runner before checkout,
build or download, without queuing laptop work. The full matrix does not enforce
the complete five-minute owner-device budget across native builds, fresh model
provisioning and multiple bootstrap/warm sessions. X1-to-X2 speech migration and actual-model
speech acceptance remain pending a hosted-prepared reduced workload with one
aggregate setup/loading/comparison/cleanup allowance; repeated shorter runs cannot
partition this matrix. Host-partitioned cache checks alone do not close those gates.
`speech-evaluation` with `operation: "host-plan"` and `repetitions: 2` inspects the
local CPU thread schedule without provisioning/loading. The `evaluate` operation
owns preparation, paired requests, bounded PCM/WAV capture and retained comparisons.
Focused `speech_evaluation::tests::`/`speech_evaluation::windows::tests::` cover schedules,
context, phases, incomplete Stop, counter domains and prelaunch rejection.
`speech_evaluation::evaluate::tests::` and `speech_evaluation::http::tests::` cover
independent audio fixtures, readiness types, incomplete comparison pairs, hostile
HTTP framing and original request deadlines. Evaluation requires a prebuilt tool;
self-hosted, missing and unknown environments reject before preparation storage.
Hosted speech resource results establish no Snapdragon conclusion. Actual-model quality needs
independent listening.

## Standards

Repository tooling uses Rust and PowerShell 7.6 or newer, without an owned Python
runtime dependency. Rust owns binary/numerical work, typed policy and validation,
and native process lifetimes. PowerShell owns small environment/command adapters
and checks that must run before Rust bootstrap. Native application provisioning
and supervision stay in Rust; installed use must not require PowerShell.
Required replay receipts and pinned external source paths retain their identities. The [language guard](../tools/ci/check_owned_languages.ps1)
checks executable source rather than deleting those records. Remaining migrations
are tracked under [tooling implementation](../PLAN.md#tooling-implementation).

Rust uses the [exact nightly](../rust-toolchain.toml), edition 2024, checked Release
arithmetic, warnings-as-errors and strict Clippy/rustdoc. Safe roots forbid unsafe
code; documented Win32 operations stay in boundary crates reused by maintenance.
Scoped allowances retain reasons and safety documentation. UI checks preserve
[native Rust presentation and the explicit API subset](ui-architecture.md).

`bonsai-cpu-policy` shares profile limits, memory accounting, completion flags and
source/PTQ1 selection. Qualification derives argv from validated controls and
reports their canonical identity. Prism's ARM source route requires Clang rather
than silently reusing the MSVC builder; archives, configuration and overlays stay
pinned. Compiler-object and Cargo caches are separate and cannot bypass checks.
Native codec checks run the production Rust codec against the pinned DLL in a
supervised probe, using independent base-3 bytes, frozen FP32 scale bits and
captured C++ fixtures.

The [pinned nightly](../rust-toolchain.toml) enables Polonius Alpha and the
next-generation trait solver by default. Keep the dated pin reproducible across
local builds, hosted checks and source-build receipts. Do not override these
defaults with `-Zpolonius=off` or `-Znext-solver=coherence`.
See the Rust team's
[Polonius Alpha announcement](https://blog.rust-lang.org/2026/08/04/enabling-polonius-alpha-on-nightly/)
and
[trait solver announcement](https://blog.rust-lang.org/2026/08/21/enabling-next-solver-on-nightly/)
.

[Compiler regressions](../tools/ci/tests/compiler.rs) require branch-sensitive
mutable lending and recursive return-position `impl Trait` inference to compile
under the ordinary Cargo invocation, then check their behavior. Foundation
compiles and runs these during hosted Linux CI bootstrap before impact selection;
the native maintenance lane also checks and runs this registered package. These
gates detect a compiler or flag change that removes the required semantics.
Polonius already supports branch-sensitive broker lending; caller-owned reusable
logits also work on stable. Both compiler components analyze code at compile time;
their use alone establishes no application throughput or memory improvement.

All workspace packages inherit edition, minimum Rust version and lint policy.
Keep borrowing and disjoint-field mutation ahead of temporary copies or interior
mutability used solely to satisfy the borrow checker. Retain fallible allocation,
checked sizes, bounded I/O and cancellation at untrusted data boundaries. Native
unsafe operations stay small, documented and behind safe owners. Prefer scoped
`#[expect(..., reason = "...")]` for a known diagnostic; permanent boundary
allowances still require their specific reason. Existing all-target Clippy,
rustdoc and owning tests enforce the policy across supported feature profiles.

Settled-source hosted acceptance must use the current compiler and include owning
speech fixtures and production linking; see [PLAN.md](../PLAN.md#runtime-and-correctness).

[Compiler probes](https://github.com/symbiogenesis/prism-inference/blob/main/tools/rust-trial/README.md)
retain stable as an isolated control. Release overflow checks stay enabled and
debug assertions disabled to preserve
[subtle's release contract](https://github.com/dalek-cryptography/subtle/blob/2.6.1/src/lib.rs#L50-L53).

Shared `doc` planning uses `cargo doc --no-deps -Z unstable-options --output-format json`
for selected packages/features in Release, through `bonsai-ci rust-check` locally and in
Foundation. Missing docs, intra-doc links, markup and Rust-block lints still fail.
JSON emits no rustdoc HTML/JS, avoiding the generated `trait.Send.js` quarantine
trigger; this does not determine whether Defender's detection was false.
Existing HTML caches/machine settings are untouched. Recheck the
[unstable output option](https://doc.rust-lang.org/nightly/cargo/commands/cargo-doc.html#option-cargo-doc---output-format)
on toolchain changes. Ordinary validation builds neither browsable docs nor doctests;
complete tests retain their CI guard.

## Focused local checks

Keep scripts/logs/captures/native working directories in a unique system-temp
lane outside Git. Expand absolute paths, preserve inherited Windows/profile
environment variables and choose the compiler explicitly.

```powershell
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('Runner-Checks-' + [guid]::NewGuid().ToString('N'))
bonsai-ci rust-check --root $PWD --package bonsai-broker --target (Join-Path $scratch 'target') --evidence (Join-Path $scratch 'evidence')
```

Choose a relevant named Rust regression, such as
`cli::tests::bounded_input_rejects_nonobjects_truncation_and_overflow`, with
`cargo test --locked --release --package bonsai-tools --lib NAME -- --exact` and external
target/working
directories. `--full-tests` is reserved for main-branch GitHub Actions; benchmarks
are always CI-only.

`window::interaction_tests::` drives egui input/accessibility with inert channels:
Enter/Shift+Enter, streaming selection and startup drafting, with no GPU/model/audio
required. Set external absolute `BONSAI_UI_CAPTURE_DIRECTORY` and select
`window::interaction_tests::capture_chat_layouts_when_requested` for PNGs of wide,
narrow, empty and maximum-draft/minimum-window layouts. Captures do not qualify
native screen readers, actual-model streaming or audio.

For appearance/sidebar captures, set external absolute
`BONSAI_ERGONOMICS_CAPTURE_DIRECTORY` and select
`ergonomics_capture_when_requested`. Related controls use `ergonomics_sidebar_`,
`profile_`, `appearance_` and `reader_size_reflows_`; select named checks within
the local-check policy.

The [streaming/storage contract](streaming-resources.md) owns resource invariants.
Choose one named regression from the source locations below. These untimed checks
establish byte/pointer identity, ownership and bounded work; they measure neither
device latency nor process peak memory. Module prefixes locate cases, not permission
to run a complete module or reconstruct a suite locally.

| Area / source | Regression coverage |
| --- | --- |
| Broker [`request_buffer`](../src/broker/src/request_buffer.rs), [`backend` storage/payload tests](../src/broker/src/backend/) and [`response` memory tests](../src/broker/src/response/memory_tests.rs) | Final-destination/spare-tail reads, bounded growth, fragments/coalesced prefixes/chunk tails, empty reads, sticky truncation/I/O failure, exact-cap overflow probes, framing, deadlines and cancellation. Eligible payloads bypass 8 KiB staging. |
| UI [`model_lifecycle`](../src/chat-ui/src/window/model_lifecycle.rs) and [`window` lifecycle regressions](../src/chat-ui/src/window/tests.rs) | Explicit model states; late readiness, completion and teardown notices cannot reopen Send in the wrong phase. Retry waits for teardown and explicit action; terminal failure stays unavailable. Stop preserves completed history, retained effort drafts and newer composer text. Recoverable rejections preserve newer drafts. These model-free checks do not qualify actual-model lifecycle behavior. |
| UI [`bounded_json`](../src/chat-ui/src/bounded_json.rs), [`client` memory tests](../src/chat-ui/src/client/memory_tests.rs), [`recovery`](../src/chat-ui/src/recovery.rs) and [`persistence`](../src/chat-ui/src/window/persistence.rs) | Escaped-byte limits before full encoding; immutable body sharing with independent turn vectors; request bytes retained through partial write/flush and released before response reads/on failure; snapshot reservation through encryption/save; busy saves skip encoding. Exact restore counts reject extra objects before fields and preserve the live workspace. |
| [`startup_process`](../tools/bonsai/src/startup_process.rs) | UTF-8 allocation transfer, lossy-decode identity and stdout-before-stderr ordering. |
| Completion [`memory`](../tools/bonsai/src/completion/memory.rs), [`completion`](../tools/bonsai/tests/completion.rs) | Optional output attribution keeps first stdout observations separate from token latency, partitions current counters by observed output/exit, leaves unsampled peaks unknown and never assigns lifetime peaks to a stage. Native supervision requires a stdout cap and preserves total sample coverage. Model-phase and actual-model/device qualification remain pending. |
| Speech service [`http`](../src/speech-service/src/http.rs), [`request`](../src/speech-service/src/request.rs) and UI [`canary`](../src/chat-ui/src/canary.rs) | Borrowed multipart fields/PCM, pre-decode excess-field rejection, canonical/extensible WAV identity, boundary collisions, maximum clip, authenticated wire bytes and partial/cancelled writes. Final-storage JSON/WAV reads cover retained headers, coalesced/partial delivery, exact limits, overflow and truncation. Header tests count delimiter comparisons and cover every delimiter split at exact limits. |
| UI [`client`](../src/chat-ui/src/client.rs) and [`canary::pcm`](../src/chat-ui/src/canary/pcm.rs) | Ready prefixes arrive before remaining body/chunk framing. Gated reads check fixed-buffer reuse, odd PCM fragments, buffered cancellation, truncation and terminal validation without byte-sized socket reads. |
| [`speech` workers](../src/chat-ui/src/speech/) and [`command_queue`](../src/chat-ui/src/speech/command_queue.rs) | Stalled-worker queue/payload bounds, coalescing, overload, tail compaction, cancellation and fresh recovery. Cold workers wake on command/disconnect/Stop/shutdown; active, warm and retained-input workers continue periodic monitoring. |
| [`speech::pipeline`](../src/chat-ui/src/speech/pipeline.rs) | Bounded packet/spare reuse across segment tails, byte identity, nonblocking recycling, stable descriptors with cleared fields, eight-descriptor limit, retained PCM/header on unprepare refusal and quarantine teardown. Fake callbacks do not invoke an audio driver. |
| Rust TTS [`engine`](../src/speech-tts/src/engine.rs), [`codec`](../src/speech-tts/src/codec.rs) and [`rust_talker`](../src/speech-tts/src/rust_talker.rs) | Model-free controls cover parallel projection/codebook consistency, cancellation between layers, streaming/offline PCM parity and bounded exact-prefix reuse/invalidation. They compile ordinary Rust and do not load a native speech library. Full pinned-model speech, resource and listening acceptance remain separate. |
| Transcript [`cache_tests`](../src/chat-ui/src/window/transcript/cache_tests.rs), [`search` work tests](../src/chat-ui/src/window/transcript/search/performance_tests.rs) and [`shortcuts`](../src/chat-ui/src/window/transcript/search/shortcuts.rs) | Fixed highlight identities at 10,000 results; navigation, same-length replacements/queries, Find reopen/eviction and no duplicated range vectors. Appends scan/copy only new bytes/results with one continuation, query-sized scratch and folded-Unicode oracle checks; saturated prefixes skip tail work while preserving lookahead, selection and navigation. Accounting checks exact totals and a combined count/byte eviction prefix at the shared 512-message live/restore limit, without visiting completed messages on ordinary append. Keyboard/IME/Escape precedence needs no service. |

For promotion tooling, the Rust `release_lock` integration test (also in
Foundation's maintenance crate checks) uses synthetic bundles without a model. Candidate generation
cannot promote a pin; follow the
[lock-update review](development-guide.md#prepare-a-runtime-lock-update).

## Acceptance limits

| Boundary | What focused/native evidence establishes |
| --- | --- |
| TTS bootstrap | `tests::voice_bootstrap_keeps_one_waveform_through_model_switch_and_releases_on_every_exit` checks Rust waveform identity/limits through Design teardown/Base extraction; failure releases once and retains at most one model. |
| Rust speech threads | `tests::cpu_threads_reserve_ui_capacity_and_bound_explicit_overrides` checks the serving thread policy: reserve two logical CPUs, use at least one, and accept explicit 1–256 overrides. See [CPU execution](../src/speech-tts/README.md#cpu-execution). |
| Rust speech sampler/tokenizer | `philox::tests::`, `native::philox_tests::rust_philox_scalar_abi_matches_captured_bits`, `native::sampling_tests::` and `native::tokenizer_tests::` check independent answers and captured observations with bound fixture/input identities. Philox replay covers 4,411 observations. These Rust checks start no C++ compiler or native reference process and do not qualify complete-model codes/audio, resources or listening quality. See [sampler](../src/speech-tts/README.md#rust-sampler-port), [tokenizer](../src/speech-tts/README.md#rust-speech-tokenizer) and [retirement](cpp-retirement.md) contracts. |
| ISA discovery | `native::isa::tests::native_isa_queries_neon_dotprod_and_neon_i8mm_independently`: exact Windows feature IDs through an injected query, without executing optional kernels. |
| X2 conditions | `cpu_evaluation::conditions::tests::condition_comparison_rejects_known_drift_and_unknowns_without_treating_charge_as_energy`; `condition_comparison_binds_exact_native_host_even_when_core_counts_match`; `host_cpu::tests::host_identity_retains_native_cpu_kernel_and_feature_facts_separately`: reject topology/power/registry/kernel/feature drift, malformed/missing identities, legacy receipts and cross-sample changes; separate identity from live availability. |
| OpenCL inventory | `opencl_inventory_rejects_unbounded_or_malformed_native_properties` (process crate), `opencl_inventory_rejects_overrides_and_unqualified_claim_changes` (tools): property/count bounds without a driver. `host-opencl` only queries bounded information; no executable GPU workload or inference verdict. See [feasibility](adreno.md#opencl-access-and-source-audit). |
| Native containment | Main-only [sandbox qualification](../.github/workflows/sandbox-qualification.yml) runs explicitly ignored PSEC fixtures on hosted ARM64/selected owner hardware, retaining revision/SDK/OS and synthetic logs. Checks file/temp authority, external-network denial, broader host-loopback scope, process limits, artifact/key validation and abrupt-owner cleanup. Foundation excludes these cases; no model/benchmark runs. |

The X2 screen records CPU/power before/after each sample outside process wall
time and rejects changed/unknown observations within or between paired boundaries,
even if the batch returns to its initial mode. There is no inside-sample monitoring
or energy verdict. Inspection success does not qualify worker startup or executing-thread
attribution.

Synthetic checks establish the tested policy/parser/storage/process properties;
native checks establish only the tested Windows behavior. Neither establishes
actual-model/daily acceptance, broker-only/same-user bypass prevention, quality,
throughput, sustained comfort, physical audio-driver behavior or speech/chat overlap.
In particular, untimed storage checks do not qualify the 15.5 GiB admission/11.5 GiB
Job budget. Source checks do not qualify a provisioned binary; release acceptance
requires independently reviewed artifacts and promotion evidence.

### Reusing qualification controls

Reuse existing control implementations and exact applicable fixtures when extending
prefix reuse, tiled prefill, speculative verification and accelerator recovery.
Declare which path each observation exercises; shared tooling does not transfer a
passing verdict between paths, releases, adapters or devices.

| Shared mechanism | Reuse boundary |
| --- | --- |
| [Latest-prefix controls](native-rust-engine.md#latest-prefix-reuse) and fresh-recompute comparisons | Compare complete logits, KV, recurrent/convolution state and sampler progression. Add each path's edited-history, provisional-failure, cancellation and restore cases; prefix agreement alone does not qualify tiled or speculative execution. |
| [Native acceptance budget](../tools/bonsai/src/native_acceptance_budget.rs) | Preserve one original deadline, residual setup charge and cleanup reserves. Reusing the calculator never starts another allowance or removes an independent owner. |
| [Power conditions](../src/cpu-policy/src/power.rs) | Retain aggregate and per-arm endpoints and explicit unknowns. Matching endpoints do not prove continuous stability or energy benefit. |
| [Typed receipt guard](../tools/ci/src/native_acceptance_guard.rs) and [selected regression action](../.github/actions/native-regressions/action.yml) | Execute each selected control and bind fresh results to exact source/compiler/package identities. Compilation caches supply no prior check verdict. |
| [Evidence archive](../tools/bonsai/README.md#command-boundary) and selected receipt staging | Preserve required replay bytes before temporary cleanup, then seal and retrieve the reviewed selection. Local staging is separate from durable storage deployment and independent review. |

Keep path-specific numerical, adapter, ownership, resource, quality and benefit
assertions alongside these shared controls. Reuse retained complete answers for
rubric preparation and eligible independent review before scheduling new captures;
historical answers retain their original identities and cannot qualify new source.

## Evidence retention

Keep only evidence supporting a current claim, actionable failure or required
independent replay. Routine edits and focused checks normally need commands,
results and limits in the commit/delivery message, not a dated evidence bundle.

For durable evidence, prefer one summary of workload, source/input/tool identities,
checks, outcomes, limits and pending gates. Owner-device trials also need predeclared
work/estimate/allowance, elapsed time, closed-clock status, aggregate/per-arm power
and cleanup. Preserve losing/no-go results and unknowns. Avoid duplicate summary,
delivery and status files unless a consumer requires a particular receipt format.

Capture output outside the repository and review an explicit selection before committing,
uploading or archiving. Omit blank logs, empty placeholder receipts, successful
build chatter, duplicate payloads/manifests, copied source and temporary validation
drivers. Record meaningful empty output, exit status, missing observations and
skips in the summary; a missing file never means success. Keep otherwise omitted
bytes only for a named gate, replay consumer or actionable diagnosis, stating why.
Use the smallest useful failure excerpt; retain full logs only when review needs
the context. Do not inventory omitted clutter. Purposeful empty fixtures and
schema-required files are valid. Selection must preserve collector intermediates
and receipt formats; missing/empty required terminal receipts mean incomplete evidence.

Device acceptance, sandbox, LoRA, PTQ1, X2 CPU and TTS workflows request 90-day
artifact retention. This extends the review window; Actions artifacts remain
temporary, and repository or organization policy can shorten their availability.
Before using a capture for promotion or a published claim, preserve its minimal
reviewed credential-free evidence selection in durable owner-managed storage.
Keep required raw receipts and replay inputs byte-for-byte, with their SHA-256
digests, repository revision, run URL/attempt, artifact
identity, device/build identities and independent reviews, including losing/no-go
results. A summary cannot replace raw data required to independently verify the
claim. Verify the archived bytes against the recorded digests before review.
Do not expand upload selections to include credentials, private conversations,
model weights or adapters. Run URLs and expired artifacts alone cannot reproduce
evidence; unavailable evidence leaves the associated gate pending.

The native [`evidence-archive` command](../tools/bonsai/README.md#command-boundary)
seals reviewed files, lengths, SHA-256s and identities; verification/restore uses
a separately retained manifest digest. It rejects missing, changed, extra or
redirected entries and never overwrites archives. Durable owner-managed storage
and capture before expiry remain separate requirements. Integrity establishes
neither execution, provenance, independent review nor promotion.

Before trimming evidence, check links, replay consumers and integrity manifests.
Preserve required claim/replay bytes in durable storage. Reconstructible scratch
and source reviews may remain in Git history with exact revisions and replay limits.
Never silently edit sealed archives, historical results or closed trials; revised
archives need a new selection and integrity record.

## Promotion criteria

Rust/PQ2 with four row workers is the owner-directed development default, with
an explicit disclaimer and manual publisher rollback. That selection makes no
full-qualification claim and changes none of the evaluator gates below. PTQ1
promotion and removal of the qualification disclaimer still require the complete
reviewed evidence.

Shared CPU source promotion follows the
[cross-generation applicability rule](cpu-kernels.md#cross-generation-cpu-promotion):
matched CPU-only whole-request benefit on X2 may authorize reviewed common CPU
paths on X1 without a separate X1 benchmark. All applicable source correctness,
state, fallback and resource gates remain required. This does not qualify a full
X1 device profile, authorize mixed-family evaluator packets or establish X1
performance, energy or sustained-use results. Family-sensitive tuned defaults
retain matching-family evidence.

Every promoted runtime/profile must:

- Pass its correctness, security, full-state recovery, creative-quality and
  resource gates, including independent LoRA decode-TPS and quality requirements.
- Keep memory bounded without sustained paging, with owner-accepted
  responsiveness, heat and fan behavior during sustained use.
- Show paired target-device CI benefit: initially **10% lower end-to-end latency**,
  or documented capacity/energy benefit within accepted latency/quality limits.
  Predeclare tolerances and retain losing/no-go results.
- Reproduce on settled `main` with immutable artifact, build and device identities,
  run/capture references, limitations and a rollback profile.

Full suites and benchmarks run only in their owning main-only CI workflows.
Local builds, analyzers and focused correctness checks remain allowed. Passing
an experiment or synthetic check does not waive any independent promotion gate.
