# Validation

Runner owns application validation: native UI, broker, process/artifact policy,
provisioning and released-runtime integration. Algorithms/upstream conformance
belong to [Prism](https://github.com/symbiogenesis/prism-inference/blob/main/docs/build-and-validation.md).
**Local builds, Release analyzers and focused named tests are allowed. Complete
suites and every benchmark run only in CI; do not recreate a full suite with local filters.**

Owner-laptop diagnostics, model qualification and benchmarks default to five minutes
total, preferably less. A dedicated, predeclared workload may use the smallest
justified longer allowance, up to fifteen minutes, including preparation, intake,
loading, all comparison arms, cleanup and evidence retention. Record the fixed work,
conservative time estimate and reason before starting; stop early on completion or
a known no-go. Self-hosted CI has the same policy, while narrower workflow/tool
limits remain effective until explicitly extended. Shrink the workload or retain an
unqualified gate when it cannot fit. Do not restart clocks, retry a failed trial or
split a long matrix into repeated runs. Historical sessions retain their original
closed clocks. Hosted cloud CI keeps its existing limits.

Every laptop trial must retain AC/battery observations before and after its
workload, separately for each comparison arm, within that same allowance.
Record battery saver and charge when available; unknown status stays unknown.
Missing, changed or mismatched power conditions leave performance comparison
ineligible. Equal endpoint observations alone do not prove continuous stability,
and a later power query cannot establish conditions for a completed trial.

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

Main-only, manually dispatched [native-runtime acceptance](../.github/workflows/native-runtime-acceptance.yml)
defaults to `run_device: false`: hosted ARM64 prepares and retains the exact-source
analyzer receipt and acceptance package without queuing a laptop job or occupying
the shared device lane. Review that preparation before selecting `run_device: true`
for one predeclared owner workload. The explicit device dispatch prepares and verifies
its own same-run package; it does not reuse another run's package or reopen a closed
trial. It runs an explicitly ignored case on the owner-managed Snapdragon X/X2 runner.
The default case requires provisioned original PQ2, the pinned Rust release, native
sandbox admission, unchanged limits and retained artifact handles. It tests supplied-history
streaming, framed context rejection/reuse, shutdown, abandonment after content,
and fresh restart. Uploads contain credential-free receipts and bounded sanitized
worker logs; protected key directories are excluded. Failure rejects without an unrestricted retry.
Ordinary Foundation/local runs never execute the ignored case.

The workflow defaults to **five minutes total**. Its explicit `owner_budget_minutes`
input selects 5, 10 or 15 minutes; 10/15 requires `budget_reason` to retain the fixed
workload estimate and why five minutes cannot fit. The job timeout uses that same
declared total. Its clock starts before checkout and hosted-binary intake. Each
job captures its UTC and monotonic starts there. Budget admission charges the
larger elapsed value, rounded up with integer arithmetic, so clock corrections
cannot increase the remaining allowance. Both observations remain in receipts;
the inherited owner's monotonic limit can independently shorten admission.
Each actual-model case receives a validated aggregate soft allowance, at most **240,
540 or 840 seconds** respectively, reduced by preceding setup, verification and
case inventory. Startup and cleanup share this allowance. Missing, malformed or
excessive limits reject before model use. The process hard limit leaves ten seconds
after the Rust allowance for owned-task teardown, and stops at least thirty seconds
before the original whole-job deadline for forced cleanup, sanitized diagnostics
and evidence uploads. Host and model receipts retain the declared total, preparation
cost and actual/maximum case allowance. Per-request deadlines cannot extend it. Do not
split a long matrix across repeated owner jobs. Hosted ARM64 prepares exact
source-bound test and maintenance binaries without loading models; the owner job
verifies their run/revision, SHA-256 and native architecture before direct execution,
with no Cargo build on the laptop.

Hosted preparation must first pass strict Release all-target Clippy for
`bonsai-chat`, `bonsai-broker` and `bonsai-tools` on that exact source and compiler.
Its `strict-release-analyzers.json` is the third digest-bound package file, alongside
the test and maintenance executables. Owner intake requires successful coverage of
all three packages and matching revision, toolchain and compiler identities before
model use; failed hosted analyzer logs remain independently retained. This is a
source prerequisite, and closes no device or promotion gate.

An inherited owner may predeclare a canonical work allowance up to thirty seconds
less than that same declared whole-job total (270 by default, at most 870).
The budget tool subtracts all elapsed time from its original monotonic start and
retains fifteen seconds for owner process exit plus the ten-second case/process
gap. The independent declared job limit, thirty-second postprocess reserve and
case maximum still apply. Selecting a longer job never enlarges a shorter inherited
owner allowance: original 210/270-second declarations retain their earlier limits.
A larger declaration neither resets the original clock nor authorizes another
attempt. Receipts record the declared work and exit reserve separately.

The workflow defaults to `profile: pq2-no-adapter`, preserving the original case
within that aggregate bound. Explicit `bounded_history` and `stalled_consumer`
dispatch inputs add reduced workloads only to `pq2-no-adapter`; both default to
false and share the existing lifecycle case's budget, including its three owned
startups and cleanup. They are not separate jobs or a way to partition a longer
qualification matrix. They remain unqualified until successful device evidence
is retained. Before either optional workload starts, more than 120 seconds must
remain for cleanup and fresh-owner recovery. A skipped requested workload records
`status: pending` and `insufficient-cleanup-and-recovery-reserve`; lifecycle
success remains separate from `allRequestedWorkloadsPassed`. Core successful
responses use four-token caps with thinking disabled to preserve that reserve.
Skipped requested workloads leave `passed: false` and fail the case only after
owned cleanup and fresh-owner recovery, even when `lifecyclePassed: true`.
Explicit `profile: ptq1-adapter`
selects a minimal required-sandbox smoke case for the exact current-weight export
and approved adapter: one scale-1.3 streamed request with a four-token cap and
owned cleanup. A scale-zero two-token request and invalid-scale connection-close/reuse
may run only if the same budget retains a conservative teardown reserve. Skipped
checks remain pending. Signed-zero/scale-two trials, supplied history, context
rejection, restart, long output and sustained use are not qualified by this smoke.
Credential-free progress receipts bind the complete runtime/model/adapter locks,
terminal reason, reported usage when supplied and cleanup. Partial failure retains
completed observations with `passed: false`. This does not establish complete-state
parity, adapter authoring basis, performance, output quality or default promotion.

Explicit `profile: ptq1-adapter-lifecycle` requires the current pinned Rust
service, exact current-weight PTQ1 and approved adapter with one row worker.
It requires three one-token-cap streams: scale 1.3, scale zero, then scale 1.3
with supplied user/assistant/user history after the invalid-scale connection-close
check. User messages remain the fixed public word `Hi`; the assistant message is
the exact UTF-8 visible response from the initial scale-1.3 stream. Its text,
SHA-256, source-message hash, finish reason and reported usage are retained and
bound before replay. Missing visible output or an incomplete source identity
keeps acceptance pending; no fixture text substitutes for model output. The
earlier four/two/two-token trial reached verified readiness but exhausted its first request. This smaller
workload preserves every required scale, rejection and history transition while
reducing prompt and generation work. The [bounded follow-up](evidence/ptq1-0110-bounded-scale-streams.json)
passes readiness and its first two streams in a 161.31-second complete session,
then skips invalid-scale rejection and history/change-back to retain the required
45-second request-and-cleanup reserve (30 seconds of cleanup plus 15 seconds of
request headroom). Overall required acceptance fails; no continuation job
or additional model attempt runs.
It preserves the request deadlines, cleanup reserve and partial-evidence rules of
`pq2-adapter-lifecycle-row4`, whose user prompts and caps are unchanged. Its receipt
binds the exact messages and completion caps, actual PTQ1 filename, byte
count and digest plus the complete export receipt; it cannot inherit PQ2 or
historical-release acceptance. Optional history/stalled-consumer dispatch inputs
are rejected. Missing, skipped or cancelled required steps keep `passed: false`
and fail after owned cleanup. This prepares a bounded qualification lane;
actual-model evidence for its complete required sequence remains pending. It does not qualify zero/no-adapter
logit identity, writer basis, generated/edited-history parity, full context/output,
recovery, quality, benefit or daily-service isolation. Supplying the captured
short response checks application history input only; fresh recomputation and
complete-state parity of generated/edited history remain independent.

Explicit `profile: ptq1-adapter-lifecycle-row4` prepares the same three-stream
PTQ1 scale/rejection/captured-history workload with four native row workers.
It keeps the same model and adapter identities, prompt, one-token caps, sampler,
request deadline, cleanup reserves and missing-evidence failures. Only the worker
count and selected case/profile change. Its receipt separately identifies four
configured workers; this does not independently observe actual thread use or
establish a throughput gain. One-worker or PQ2 evidence cannot qualify this
profile, and its evidence cannot qualify those controls. Hosted argument and
partial-progress controls prepare the lane; actual-device acceptance remains
pending. A later bounded trial requires its own complete allowance and cannot
continue the missing steps of an earlier workload.

Explicit `profile: ptq1-adapter-boundaries-row4` prepares a separate bounded
released-service scale-boundary/reset control with four workers. Four one-token
streams use the same `Hi` prompt at scale 1.3, negative zero, 2.0, then 1.3;
invalid-scale connection close and oversized-context rejection precede the final
stream. The final stream must reproduce the initial visible UTF-8 bytes, digest,
finish reason and reported one-token usage. Each selected request is bound by its
exact JSON digest and scale bits, preserving negative zero through serialization.
These request identities do not establish a server acknowledgement of each scale,
numerical adapter effect, complete live-state identity or absence of all stale state.
The released worker/adapter readiness remains independently verified by the broker.

The profile shares the original case deadline, thirty-second cleanup reserve and
forty-five-second minimum before each further request. Missing, expired or mismatched
observations stay pending and fail only after owned cleanup. It admits neither
extra history nor stalled consumers, and does not continue any earlier trial.
Actual-device execution remains pending; cancellation, fresh-owner restart,
numerical state, throughput, quality and the complete service-promotion gate still
require their own evidence. Its four minimal streams avoid the earlier history
lane's longer supplied-history prefill; this source choice establishes no device
timing or proof that every preparation can fit the owner allowance.

Explicit `profile: ptq1-adapter-service-row4` combines the remaining bounded
released-service transitions in one original allowance. Three cap-one streams
use the same `Hi` prompt at 1.3, negative zero and 2.0, followed by invalid-scale
and oversized-context rejections. A fourth request keeps that prompt and scale
1.3 with a 4096-token reservation. Stop follows its first visible prefix, which
is retained with its exact text/digest and bound request; a terminal SSE record
already received cannot qualify interruption. The prefix parser waits for a
complete SSE record, including its blank-line delimiter. Same-owner prefix
equality is informational because sampler draws advance between requests;
the fresh owner below resets the seed for exact baseline replay.
Three completed and three failed broker operations bind
the two rejections and interrupted operation, with verified empty Job/listener.
A fresh owner must independently verify readiness, replace credentials, then
reproduce the exact initial cap-one response and report one completed operation
without failures before its own verified cleanup.

Every request/startup uses the same absolute deadline with thirty seconds held
for owned cleanup. Fresh startup requires more than sixty seconds remaining;
missing work stays pending. Power endpoints are retained for the Rust model case
and each owner, without claiming uninterrupted stability. Exact request/message
digests and matching positive reported prompt counts bind completed streams;
the production broker retains one canonical body for counting and generation.
This source-bound contract is distinct from a private wire capture or a server
acknowledgement of each requested scale. The receipt leaves the complete service
gate to independent source/retained-data review and establishes no numerical
adapter effect, full-state parity, 4096 generated tokens, performance, quality,
full promotion or continuation of an earlier trial.

Explicit `profile: ptq1-adapter-service-rollback-row4` adds an original-PQ2 rollback
to the complete five-request PTQ1 service sequence above in the same allowance.
The existing service-only profile retains its workload. After both PTQ1 owners
have verified empty Jobs and released listeners, a third fresh owner selects
`--use-original-pq2` with pinned Prism 0.1.14, adapter 1.3 and four workers.

Both service cases retain one absolute case deadline with their outer timeout.
The combined case gives its PTQ1 sequence an earlier deadline, withholding the
seventy-five-second PQ2 tail through both PTQ1 teardowns. With at most 135 seconds
remaining, it rejects before the first PTQ1 owner: the existing sixty-second fresh
PTQ1 owner floor must fit in addition to the rollback tail. This necessary floor
does not establish that the complete packet can fit a particular allowance.
Their Stop path caps its existing thirty-second allowance at the applicable deadline,
reserving ten seconds to abort and await a stuck owner task so ownership is not
detached. Fixed pending reasons preserve failed Stop delivery, task/run failures,
unexpected exits and unconfirmed listener release before retaining power-after.
Only explicit connection refusal confirms listener release; an accepted connection
means occupied, while timeout, access, network and other probe failures remain unknown.
A task error leaves Job emptiness unknown. Confirmed cleanup is required before
another owner or the post-cleanup PQ2 hash check; synchronous native teardown
remains non-preemptible. These source controls require hosted and device acceptance.

The original-PQ2 owner must naturally complete the existing fixed tree-name request within its
32-token cap and release its Job/listener with one completed operation and no
failures. A complete post-cleanup CNG length/SHA-256 verification then binds
the retained original model file, with file/ancestor guards through the final
combined receipt. Transport errors and malformed/truncated rollback responses
remain pending; response parsing follows owned cleanup so those failures retain
the cleanup and power observations.

This profile keeps one absolute deadline and the existing thirty-second cleanup
reserve. The third startup requires more than seventy-five seconds remaining;
startup/request work stops forty-five seconds before that deadline, retaining
fifteen seconds after owned cleanup for complete-file verification. Hashing checks
cancellation before each bounded read; blocking OS work still requires the outer
owner deadline. Rust power endpoints cover each owner and the model case; the
separately retained aggregate observations above must cover setup and final retention.
`serviceIntegration` and `originalPq2Rollback` keep their distinct partial verdicts;
the complete case requires both plus post-cleanup file verification. Missing or
expired work remains pending, without a continuation job. Hosted preparation
does not authorize another owner trial or reopen an earlier closed allowance.
Performance/resource benefit, full-context numerics, provenance, human quality,
comfort, isolation and default promotion remain independent.

The [prepared packet estimate](evidence/native-owner-packet-controls-20261103/owner-workload-plan.json)
budgets 450 seconds conservatively for setup, three startups, five PTQ1 generation/prefix
operations, controls, one complete PQ2 response, all three cleanup reserves, hashing
and retention. This planning estimate has no measured 0.1.14 completion result.
The smallest currently supported workflow allowance covering it is ten minutes;
the fifteen-minute option is unnecessary for this fixed packet. An owner must
freeze the final source, review its hosted package and all prerequisite checks,
retain aggregate power endpoints and predeclare the allowance before execution.

Explicit `profile: pq2-adapter-row4` selects original PQ2, the approved adapter at
scale 1.3 and pinned Prism 0.1.10 with `--native-row-workers 4`. It requires exactly
one four-token-cap stream, verified model/adapter readiness and owned Job/listener
cleanup within the same budget. Its receipt binds the runtime lock, original
model identity and adapter lock, and distinguishes the configured worker count
from an independently observed thread count. It runs no optional requests or
retry. Multi-scale, rejection/reuse, history, context, restart, quality, sustained
service and TPS gates remain unrun; this smoke cannot enable a default.

Explicit `profile: pq2-adapter-lifecycle-row4` separately requires the original
PQ2 and approved adapter with four row workers through three short streams in
one owner: scale 1.3 (four-token cap), scale zero (two-token cap), invalid-scale
connection close, then scale 1.3 with a supplied user/assistant/user history (two-token
cap). The assistant message is the exact initial scale-1.3 model output, with
the same retained source binding and missing-output policy as the PTQ1 profile.
Every further request needs more than 45 seconds remaining; the complete
sequence has a request deadline thirty seconds before the aggregate deadline
for owned cleanup. A completed zero-scale or rejection observation is saved
before the next request. A skipped or cancelled step remains pending and fails
the case after cleanup, even when initial streaming and cleanup passed.
`allRequestedWorkloadsPassed` and the individual step flags are independent of
`lifecyclePassed`. No other workloads or retry are admitted to this profile.
This is application scale-change/rejection/supplied-history coverage; it does not
establish zero/no-adapter numerical identity, adapter writer basis, exact live
prefix reuse, edited/generated-history parity, full context/output, quality or
performance. Its actual-model gate remains pending until retained device CI
evidence within the shared predeclared total budget; do not partition a longer
matrix across repeated profiles/jobs.

All adapter-profile HTTP work, including the first required stream and optional
PTQ1 requests, shares the deadline thirty seconds before owned cleanup. Expiry
drops the in-flight request before Stop, retains completed observations, records
`requestBudgetExpired: true` and leaves `passed: false`; the test fails after
confirming the owned Job is empty and the listener released. No HTTP work starts
when only the cleanup allowance remains.

Explicit `profile: pq2-adapter-speed` makes one fixed-order application request
comparison with original PQ2, adapter scale 1.3, seed 42 and one then four row
workers, each in a fresh required-sandbox owner. Both use the same short public
counting prompt, thinking disabled and an eight-token output cap. Setup, both
model loads, requests, comparison and cleanup share the existing aggregate
predeclared owner deadline; no retry or continuation job is permitted. A new
owner needs more than 60 seconds remaining and each request ends at least 30
seconds before the deadline for owned cleanup. Completed observations are saved
before the second startup; a skipped or expired half remains pending.

`requestTokensPerSecond` divides reported completion usage by monotonic elapsed
time from connection start through complete framed HTTP response delivery and
connection closure,
including prefill, generation and transport. Model startup and response parsing
are excluded; startup is recorded separately. This is application request
throughput, **not device decode TPS**, TTFT or a rate inferred from SSE event or
byte counts. The ratio requires both streams to end with `length` at eight
reported tokens, identical visible output, empty Jobs and released listeners.
Early stop, unknown usage, different output or missing cleanup leaves comparison
pending. A single fixed-order pair establishes neither repeated-trial benefit,
complete-state parity, energy/comfort, creative quality nor default promotion.

Explicit `profile: ptq1-pq2-complete-response` prepares a separate complete-response
feasibility pair with original PQ2 then the verified current-weight PTQ1 export,
each in a fresh required-sandbox owner with one row worker, the approved adapter
at scale 1.3, production sampling and seed 42. The fixed public request asks for
one tree name with thinking disabled and a 32-token ceiling. Both responses must
stop naturally, report positive completion usage and complete owned cleanup;
`length` means a truncated capture and leaves the pair pending. Exact UTF-8
response strings and SHA-256 digests, model/export/runtime/adapter identities,
template and sampler controls, startup and framed request times are retained.
Different complete responses remain available for review; the request-latency
ratio requires identical response bytes and completion usage.

The pair also compares resource observations already retained during owned cleanup,
without additional probes or model work. Matched response bytes and usage are
required for these ratios too. Each accounting domain has its own PQ2 and PTQ1
byte counts and ratio: OS lifetime Job commitment, OS lifetime root residency,
sampled serving root private commitment and sampled serving root residency.
The domains are non-additive. Both arms require positive native queries and
serving guard samples; missing, malformed or internally inconsistent telemetry
leaves the resource comparison pending even when complete responses were captured.
Guard counts and minimum sampled system available memory accompany the comparison;
system available memory is context, not attributed model consumption. These are
bounded observations, with full-context resources, paging, sustained comfort and
resource benefit still unqualified.

Explicit `profile: ptq1-pq2-complete-response-row4` selects the same fixed pair
with four configured native row workers for both fresh owners. Its separate
case and receipts retain that policy; the original complete-response profile
keeps one worker. Prompt, sampler, adapter, model identities and all completion,
cleanup and budget requirements are identical. Each sample must bind the compiled
runtime's worker-readiness identity. For reviewed releases 0.1.11 through 0.1.13,
the effective graph worker budget is independently observed at readiness; missing
or mismatched observations leave the pair pending. This does not measure executing
threads or parallel work, and qualification does not transfer between counts.
Neither profile admits extra history or stalled-consumer workloads.

The original-PQ2 arm now selects `--use-original-pq2` explicitly, so a later
application-default change cannot change the rollback artifact. Its
`originalPq2Rollback` observation is saved immediately after owned cleanup and
survives a pending PTQ1 arm. Capture requires the exact original model identity,
matching released worker-readiness observation, a complete nonempty response and
released Job/listener. Independent review still binds CI, runtime/adapter policy
and preservation of the original artifact on disk before scoring the rollback
gate; this observation cannot qualify the pair, benefit or default.

The existing aggregate deadline includes both loads and cleanup. Each startup
requires more than 60 seconds remaining, each request reserves 30 seconds for
owned cleanup, and completed observations are saved before the next operation.
An unfittable arm stays pending; do not split the pair or a larger matrix into
repeated owner jobs. The small prompt measures feasibility only. It does not
replace repeated interleaved complete-task trials, independent quality review,
per-prompt adapter decode-TPS gates, loaded-zero/no-adapter controls, writer-basis
evidence, full context/output acceptance or default promotion.

Before runner temp cleanup, the workflow reads only `backend.stdout` and
`backend.stderr` from at most eight immediate protected session directories, with
a 64 KiB limit per log and strict UTF-8 decoding. It validates both actual keys
for every session, redacts those values across all retained logs, checks again
before publication, and suppresses logs when key validation fails. Only two fixed
sanitized top-level log names and `startup-diagnostics.json` enter the upload
selection, in a separate artifact gated on successful collection and final
regular-file, reparse and credential checks of all three paths. Unsafe output
directories or reparse points remain untouched and suppress the entire diagnostic
artifact; the three base acceptance JSON receipts retain their existing upload.
`host.json` retains fixed broker startup stage/error/exit-status markers;
the acceptance test process exit code is recorded separately. No raw exception,
argument or key content is copied into those classifications.

| Workload | Required behavior | Evidence limit |
| --- | --- | --- |
| Default | Valid streaming, context rejection/reuse, owned shutdown, abandonment and fresh restart | Short outputs do not qualify 8K/4K, numerical state, LoRA, sustained resources, speed or broker-only isolation. |
| Optional `bounded_history` (`BONSAI_NATIVE_EXTENDED` internally) | Reject 4,500 synthetic forest-note repetitions with a 4,096-token reservation; admit 128 repetitions in assistant history with a four-token cap and thinking disabled; complete another four-token non-thinking stream in the same instance | This replaces the larger disabled 3,900-repetition/4K-generation workload. Fixture repetitions are not tokens. Record visible bytes, terminal reason and usage when supplied; missing usage is unknown and events are not tokens. Bounded history does not qualify long-history behavior, 4K generation or exact 8K occupancy. |
| Optional `stalled_consumer` | Request the full 4,096-token output reservation with thinking disabled, read first nonempty content, retain the connection without reads for one second, then explicit owner Stop. Finish within 30 seconds with empty Job/released listener; require one additional failed operation and no completed operation, require generated content from a fresh owner after Stop and a further successful fresh-owner request after the existing abandonment trial | The output cap reserves capacity; this workload never waits for 4K generation. Natural completion cannot qualify interruption. The receipt proves unfinished broker forwarding, not backend activity at Stop, socket saturation, maximum backlog or measured cancellation latency. |

The original per-request safety limits, 8K context, 64-token margin, output,
memory and containment limits remain; the shorter aggregate device budget takes
precedence over every longer timeout. Streams require complete HTTP framing, a supported
terminal reason and `[DONE]`. Interrupted instances never reopen admission or replay
the request. Optional workloads remain unqualified until successful device CI
receipts are retained. The credential-free `acceptance.json` is saved before
startup and at each workload/cleanup/recovery phase with `passed: false`; only
complete success changes it to true. A timeout/failure retains the latest phase
and completed observations without claiming unrun checks. The [promotion evaluator](native-rust-engine.md#default-runtime-promotion) keeps exits independent.

The core PQ2 receipt saves supplied-history termination, visible bytes and reported
usage, context rejection and same-owner reuse as each completes. Initial Stop,
fresh-owner abandonment and final fresh-owner shutdown retain separate operation
counts and Job/listener cleanup observations. Missing usage remains null; visible
bytes never stand in for tokens. These observations survive a later phase failure
with `passed: false`; they do not qualify the missing phase or either optional
workload. The bounded supplied-history and stalled-consumer device gates remain
pending until successful owner-device evidence is retained within the shared budget.

The adapter profiles additionally require positive reported completion usage within
each stream's cap. A `length` termination must report exactly that cap. Missing,
zero, excessive or inconsistent usage leaves the stream and required scale/history
workload pending even when visible text arrived. The receipt preserves the text,
reported usage and fixed failure reason before owned cleanup; events and bytes
cannot replace worker token accounting.

Adapter lifecycle and paired-response receipts distinguish configured row workers
from the effective graph budget verified at readiness. The reviewed exact
`0.1.11`, `0.1.12` and `0.1.13` releases record that budget as independently observed after
successful strict readiness, bound to the compiled runtime lock and selected one/four-worker
policy. Startup receipts and legacy releases retain an unobserved/null result.
This observes neither executing threads nor parallel operations or device benefit.

Adapter startup begins cancellation at the original request-work deadline before
the 30-second cleanup reserve, even when readiness never arrives. Native teardown
may consume part of that reserve; synchronous artifact verification is not
preemptible by the async timeout. Its pending receipt records
`startup-budget-expired`; future cancellation does not establish independently
observed Job/listener cleanup or successful readiness.

The adapter lifecycle's `startupProgress` observations bracket broker policy and
admission, complete model verification, adapter verification, credentials/runtime
preparation, process creation, authenticated readiness, native identity and frontend
binding. The fixed phase/time callback records in memory; the acceptance harness
copies observations into its receipt after startup returns or its work deadline
cancels it. A returned startup error retains `startup-failed`. An outer process
termination or panic can prevent this final copy and leaves attribution pending.
A phase without a completion observation remains unfinished. Readiness combines
unobserved Prism verification, tensor loading, graph/state construction and broker
polling; these brackets do not identify an inference hotspot or establish a gain.
Observer overhead is included, and synchronous verification is not preemptible.

Optional supplied-history receipts persist context rejection and the completed
history stream separately before same-owner reuse. Cancellation or failure of a
later step leaves the workload pending while preserving those observations.
The stalled-consumer receipt likewise saves its visible prefix before the fixed
nonreading pause and owned Stop, recording pause completion separately; that
prefix alone establishes neither interruption nor cleanup.

Model-free acceptance regressions:

| Exact case | Property |
| --- | --- |
| `native_extended_acceptance_distinguishes_output_usage_from_delta_count` | Fixture policy; usage separate from bytes/events |
| `native_lifecycle_observations_separate_generation_usage_and_owned_cleanup` | Visible UTF-8 bytes remain distinct from reported tokens; unknown usage stays null, over-cap usage rejects and cleanup requires the expected owner exit |
| `native_bounded_history_retains_completed_steps_when_followup_is_cancelled` | Cancel the real HTTP fixture after rejection or history streaming; retain completed observations on disk without qualifying reuse or the workload |
| `native_bounded_history_and_stall_requests_preserve_distinct_output_limits` | Bounded history has 128 repetitions/four tokens; stopped stream reserves 4K; both disable thinking and stay in one aggregate lifecycle case |
| `native_stalled_consumer_retains_fragmented_prefix_connection_until_dropped` | Fragmented UTF-8/SSE intake and retained connection |
| `native_stalled_consumer_never_qualifies_natural_completion_as_interruption` | Operation-counter interruption guard |
| `native_stalled_consumer_rejects_error_or_unframed_stream_headers` | HTTP 200 and chunked event-stream framing before accepting content |
| `native_case_budget_rejects_missing_unbounded_and_malformed_allowances` | Reject missing, zero, malformed, overflowing and over-240-second default allowances; optional workloads use the reduced allowance and retain the 120-second recovery reserve |
| `native_case_budget_extension_requires_a_valid_predeclared_total` | Five-minute default remains 240 case seconds; explicit 10/15-minute totals permit at most 540/840, rejecting malformed totals or excessive/missing case limits |
| `native_representation_row4_profile_is_explicit_and_preserves_pair` | Separate complete-response selection configures both fixed-order owners with four workers while retaining the one-worker profile and identical approved model, adapter, request and containment policies |
| `native_representation_comparison_requires_released_worker_observation` | Both representation samples must retain the compiled release's matching worker-readiness version, observation flag and graph budget; missing or mismatched observations cannot produce a latency ratio |
| `native_representation_rollback_observation_survives_pending_ptq1_arm` | A complete original-PQ2 arm retains a separate observation when PTQ1 is missing; wrong identities, worker counts, truncated output and incomplete cleanup leave rollback pending, and independent gate review remains required |
| `native_representation_resources_compare_matching_domains_only` | Matched complete responses and usage permit separate Job-commit, root-residency and sampled-serving ratios without summing domains or claiming full resource qualification |
| `native_representation_resources_reject_missing_or_inconsistent_observations` | Missing, nonpositive or inconsistent native resource telemetry leaves the resource comparison pending while preserving complete-answer captures |
| `native_representation_profiles_leave_skipped_arms_pending_at_original_deadline` | Both worker policies retain partial observations and pending status when the original shared deadline reaches the 60-second startup/cleanup reserve; no missing owner starts or receives a new clock |
| `native_adapter_scale_history_profile_requires_separate_explicit_selection` | Scale/history lifecycle is a distinct original-PQ2 row4 selection; the one-stream smoke and no-extra-workload restrictions remain |
| `native_adapter_scale_history_retains_partial_steps_without_qualifying_missing_work` | Real model-free HTTP peers verify zero/rejected/changed-back scales and supplied history; cancelling after zero or rejection preserves completed receipts without qualifying later work; exhausted request reserve starts no HTTP work |
| `native_adapter_scale_history_requires_reported_bounded_terminal_usage` | Model-free zero/history HTTP responses with missing, zero, excessive or inconsistent usage retain exact visible output and pending status for each explicit worker policy; valid earlier steps remain recorded |
| `native_worker_receipts_require_exact_release_and_verified_readiness` | Exact reviewed release admission and worker identities; no observed budget before readiness, for legacy/missing versions or invalid counts; mutable report fields cannot replace the compiled runtime identity |
| `native_adapter_request_budget_drops_inflight_work_before_owned_cleanup` | Paused-time cancellation drops request-owned state before Stop and preserves the complete cleanup allowance; no request starts at or below the reserve and successful work retains the remaining budget |
| `native_adapter_startup_budget_retains_pending_receipt_and_cleanup_reserve` | Paused-time startup cancellation retains its pending on-disk receipt and unfinished fixed-phase observation at the async timeout boundary; no startup starts inside the reserve. This does not measure native teardown duration, preempt synchronous verification or establish readiness/cleanup |
| `native_adapter_startup_error_retains_unfinished_phase_without_qualifying_cleanup` | A returned startup error retains the unfinished fixed phase and failure reason in the on-disk receipt without manufacturing deadline expiry, readiness or cleanup |

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

### Foundation and routing

[Premerge portable checks](../.github/workflows/premerge.yml) provide a limited
hosted Linux ARM64 screen for pull requests to `main` and manual dispatch: workspace
Rust formatting, portable CI/CPU-policy/broker compilation and six exact named
model-free routing, gate and request-policy regressions. A missing named case fails.
The read-only workflow runs no complete suite, model, benchmark or self-hosted job;
it qualifies neither native Windows behavior nor actual-model/device acceptance.

[Foundation](../.github/workflows/foundation.yml) runs on `main` pushes/manual
dispatch, skipping root-Markdown/`docs/`-only pushes. Its [Rust planner](../tools/ci/README.md)
reads [impact routing](../.github/ci/impact.json), selects changed packages plus
transitive workspace consumers, and requires every selected lane/app group to
pass. Only planned skips are accepted. The [package registry](../config/rust-packages.json)
contains application packages, not upstream runtime catalogs.

| Group | Coverage |
| --- | --- |
| Broker, UI, speech, maintenance | Affected packages/consumers with independent caches |
| Native integration | Released GGUF/Python integration and admission-only checks; Rust maintenance tests own provisioning, plans, supervision and package lifecycle/tamper/path checks |
| Released runtime | Archive/install/profile/adapter/admission policy, GGUF wrappers, help switching and actual pinned Rust inference provisioning with `includeCapabilities: true` |

For example, a broker edit selects broker checks; shared Windows-boundary edits
also select broker/CLI consumers. CLI edits select CLI/app integration. Missing/unclassified diffs or changes to
workspace/lockfile/toolchain/CI inputs select full coverage. Manual dispatch
selects everything. Each workflow compares with its newest successful main-push
ancestor, covering failed/cancelled intervening changes. New automatic pushes
supersede older Foundation push runs; manual dispatch groups remain independent.
Planner/gate regressions run first; the tested cached Linux planner executable
is handed to aggregation, avoiding a second compiler install/build or Cargo metadata.

Maintenance owns affected Windows Job tests and produces a CLI plus feature-gated
Rust completion probe. Consumers verify revision, both digests and ARM64 identities;
lifecycle fixtures retain production headless mitigations. Script/released-runtime
lanes start after maintenance without waiting for independent broker/UI/speech.
The PowerShell/C# bootstrap, script tests, analyzer job and .NET SDK pin were
retired with their Rust replacements.

ASR/TTS use separate lanes and the checked CLI for pinned source preparation.
TTS caches four native static libraries plus digest receipt through
`BONSAI_TTS_NATIVE_BUILD_DIR`, keyed by build inputs/runner image; all Rust checks
still run. Missing caches rebuild; identity/digest mismatches fail. Speech
Cargo/native caches also bind the checked native build-host hash in every restore
prefix. Version-two library receipts bind full host/source hashes and each library
SHA-256; each lane retains its model-free host report, including cache hits.

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

Upstream-only coverage runs in Prism's [conformance](https://github.com/symbiogenesis/prism-inference/blob/main/.github/workflows/runtime-conformance.yml),
[Foundation](https://github.com/symbiogenesis/prism-inference/blob/main/.github/workflows/foundation.yml),
[standards](https://github.com/symbiogenesis/prism-inference/blob/main/.github/workflows/standards.yml)
and [compiler](https://github.com/symbiogenesis/prism-inference/blob/main/.github/workflows/rust-nightly.yml)
workflows, including source/backend catalogs, baseline/codec corpora, overlays and
focused upstream diagnostics. Runner's optional [LoRA device workflow](../.github/workflows/swift-lora-qualification.yml)
keeps its standalone tool build because it qualifies the selected application
profile; it requires reviewed artifacts, owner-managed hardware and explicit dispatch.

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
[Hosted resource evidence](performance-expectations.md#q4-speech-resource-comparison)
describes that CPU alone; Snapdragon conclusions need its runner and quality
still needs independent listening.

## Standards

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
See the Rust team's [Polonius Alpha announcement](https://blog.rust-lang.org/2026/08/04/enabling-polonius-alpha-on-nightly/)
and [trait solver announcement](https://blog.rust-lang.org/2026/08/21/enabling-next-solver-on-nightly/).

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

The 2026-11-01 focused Windows ARM64 screen completed in 87.6 seconds on
`rustc 1.101.0-nightly (1a2c4797e 2026-10-21)`. Both compiler regressions passed;
disabling Polonius produced E0499 and disabling the global next solver produced
E0369, confirming the gates distinguish the required analyses. Changed-source
formatting, Release all-target Clippy for artifacts and CI tooling, the exact
interrupted-read/cancellation regression and a production-source tokenizer Clippy
check passed. The tokenizer check isolated native linkage; complete owning
speech fixtures, native linking and the remaining workspace checks stay in
hosted Foundation. This screen ran no model, benchmark or complete suite.
The compiler and dependency pins subsequently advanced on `main`; this local
result retains the compiler identity above, with the combined revision requiring
its own hosted Foundation result.

The first combined [hosted run](https://github.com/symbiogenesis/SwiftBonsai2Runner/actions/runs/41384479280)
passed compiler behavioral gates on Linux and Windows and all selected test,
documentation and executable-build phases, including native ASR/TTS fixtures
and linking. New-nightly Clippy identified two additional updates: explicitly
take/drop oversized speech scratch to preserve allocation release, and make the
queue's `pending` accessor `const fn`. Both are corrected without weakening
lints. A source-bound replay of the existing scratch-retention regression and
new-nightly Clippy passed in a further two seconds. The corrected source is
`d2a2962`; [PLAN.md](../PLAN.md#runtime-and-correctness) owns its pending combined
hosted result. No model or performance qualification is transferred.

[Compiler probes](https://github.com/symbiogenesis/prism-inference/blob/main/tools/rust-trial/README.md)
retain stable as an isolated control. Release overflow checks stay enabled and
debug assertions disabled to preserve
[subtle's release contract](https://github.com/dalek-cryptography/subtle/blob/2.6.1/src/lib.rs#L50-L53).

The lending-storage refactor passes owning broker and speech-service Release
Clippy checks with all targets. Its bounded local screen passes 42 source-bound
model-free regressions: HTTP/SSE/PCM framing and borrowing, cache ownership,
packed replay, broker buffered caps/deadlines, speech request/storage reuse and
waveform-owner cleanup. Transcript/Find test sources also type-check and pass
focused Clippy checks; their executable build exceeded its local allowance.
Source-bound harnesses retain production logic while isolating native linkage;
these checks do not replace owning UI execution, native ASR/TTS build/link,
rustdoc, actual-model acceptance or performance measurement. The initial broader
analyzer capture failed draining compiler output, and linked broker test
compilation also exceeded its bounded allowance. [PLAN.md](../PLAN.md) retains
the remaining hosted gates.

Shared `doc` planning uses `cargo doc --no-deps -Z unstable-options --output-format json`
for selected packages/features in Release, through `bonsai-ci rust-check` locally and in
Foundation. Missing docs, intra-doc links, markup and Rust-block lints still fail.
JSON emits no rustdoc HTML/JS, avoiding the generated `trait.Send.js` quarantine
trigger; this does not determine whether Defender's detection was false.
Existing HTML caches/machine settings are untouched. Recheck the [unstable output option](https://doc.rust-lang.org/nightly/cargo/commands/cargo-doc.html#option-cargo-doc---output-format)
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
`cargo test --package bonsai-tools --lib NAME -- --exact` and external target/working
directories. `--full-tests` is reserved for main-branch GitHub Actions; benchmarks
are always CI-only.

`window::interaction_tests::` drives egui input/accessibility with inert channels:
Enter/Shift+Enter, streaming selection and startup drafting, with no GPU/model/audio
required. Set external absolute `BONSAI_UI_CAPTURE_DIRECTORY` and select
`window::interaction_tests::capture_chat_layouts_when_requested` for PNGs of wide,
narrow, empty and maximum-draft/minimum-window layouts. Captures do not qualify
native screen readers, actual-model streaming or audio.

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
| TTS [`build_overlay`](../src/speech-tts/build_overlay.rs) | Compiles the actual generated decode block with the provisioned compiler; checks cache hits, window/batch replacement, allocation failure, completed-graph lifetime/teardown, immutable source and drift rejection. |
| Transcript [`cache_tests`](../src/chat-ui/src/window/transcript/cache_tests.rs), [`search` work tests](../src/chat-ui/src/window/transcript/search/performance_tests.rs) and [`shortcuts`](../src/chat-ui/src/window/transcript/search/shortcuts.rs) | Fixed highlight identities at 10,000 results; navigation, same-length replacements/queries, Find reopen/eviction and no duplicated range vectors. Appends scan/copy only new bytes/results with one continuation, query-sized scratch and folded-Unicode oracle checks; saturated prefixes skip tail work while preserving lookahead, selection and navigation. Accounting checks exact totals and a combined count/byte eviction prefix at the shared 512-message live/restore limit, without visiting completed messages on ordinary append. Keyboard/IME/Escape precedence needs no service. |

For promotion tooling, the Rust `release_lock` integration test (also in
Foundation's maintenance crate checks) uses synthetic bundles without a model. Candidate generation
cannot promote a pin; follow the [lock-update review](development-guide.md#prepare-a-runtime-lock-update).

## Acceptance limits

| Boundary | What focused/native evidence establishes |
| --- | --- |
| TTS bootstrap | `voice_bootstrap` checks native reference pointer identity/limits and handoff through Design teardown/Base initialization/extraction; failure releases once and retains at most one model. |
| Rust speech helpers | `native::tests::rust_parameter_defaults_match_every_pinned_cpp_field` compares every ABI-v6 default against the linked pinned C++ functions, with exact float bits. `native::cpu_threads::tests::` covers fallback/cast edges, all 1–256 overrides and rejected updates; `thread_overlay_routes_native_queries_to_rust_and_rejects_source_drift` checks policy replacement/source drift. See [helper ports](../src/speech-tts/README.md#rust-parameter-and-cpu-thread-helpers). |
| Rust speech sampler | `philox::tests::`, `native::philox_tests::rust_philox_scalar_abi_matches_captured_bits`: independent block/f32 answers and 4,411 frozen native observations, with exact fixture/input identity and output bits. Scalar replay starts no compiler or native reference process. Source-identity/overlay checks retain both call replacements and reject drift. Remaining sampler/tokenizer/native integration checks still require the provisioned compiler/environment; see [sampler port](../src/speech-tts/README.md#rust-sampler-port) and [retirement contract](cpp-retirement.md). |
| ISA discovery | `native::isa::tests::native_isa_queries_neon_dotprod_and_neon_i8mm_independently`: exact Windows feature IDs through an injected query, without executing optional kernels. |
| X2 conditions | `cpu_evaluation::conditions::tests::condition_comparison_rejects_known_drift_and_unknowns_without_treating_charge_as_energy`; `condition_comparison_binds_exact_native_host_even_when_core_counts_match`; `host_cpu::tests::host_identity_retains_native_cpu_kernel_and_feature_facts_separately`: reject topology/power/registry/kernel/feature drift, malformed/missing identities, legacy receipts and cross-sample changes; separate identity from live availability. |
| OpenCL inventory | `opencl_inventory_rejects_unbounded_or_malformed_native_properties` (process crate), `opencl_inventory_rejects_overrides_and_unqualified_claim_changes` (tools): property/count bounds without a driver. `host-opencl` only queries bounded information; no executable GPU workload or inference verdict. See [feasibility](adreno.md#opencl-access-and-source-audit). |
| Native containment | Main-only [sandbox qualification](../.github/workflows/sandbox-qualification.yml) runs explicitly ignored PSEC fixtures on hosted ARM64/selected owner hardware, retaining revision/SDK/OS and synthetic logs. Checks file/temp authority, external-network denial, broader host-loopback scope, process limits, artifact/key validation and abrupt-owner cleanup. Foundation excludes these cases; no model/benchmark runs. |

The X2 screen records CPU/power before/after each sample outside process wall
time and rejects changed/unknown observations within or between paired boundaries,
even if the batch returns to its initial mode. There is no inside-sample monitoring
or energy verdict. Its retained legacy headless startup failed DLL initialization;
inspection success does not qualify worker startup. See [X2 evidence](cpu-kernels.md#x2-cpu-thread-screen).

Synthetic checks establish the tested policy/parser/storage/process properties;
native checks establish only the tested Windows behavior. Neither establishes
actual-model/daily acceptance, broker-only/same-user bypass prevention, quality,
throughput, sustained comfort, physical audio-driver behavior or speech/chat overlap.
In particular, untimed storage checks do not qualify the 15.5 GiB admission/11.5 GiB
Job budget. Source checks do not qualify a provisioned binary; release acceptance
requires independently reviewed artifacts and promotion evidence.

## Evidence retention

Device acceptance, sandbox, LoRA, PTQ1, X2 CPU and TTS workflows request 90-day
artifact retention. This extends the review window; Actions artifacts remain
temporary, and repository or organization policy can shorten their availability.
Before using a capture for promotion or a published claim, preserve its existing
credential-free upload selection in durable owner-managed storage. Keep the exact
files and their SHA-256 digests, repository revision, run URL/attempt, artifact
identity, device/build identities and independent reviews, including losing/no-go
results. Verify the archived bytes against the recorded digests before review.
Do not expand upload selections to include credentials, private conversations,
model weights or adapters. Run URLs and expired artifacts alone cannot reproduce
evidence; unavailable evidence leaves the associated gate pending.

The native [`evidence-archive` command](../tools/bonsai/README.md#command-boundary)
seals an explicit reviewed file/length/SHA-256 selection with its exact identity
records, then verifies or restores it using a separately retained manifest digest.
It rejects missing, changed, additional or redirected entries and never overwrites
an archive. This supplies local byte-preserving archive/retrieval mechanics;
deployment to durable owner-managed storage and capture before artifact expiry
remain independent requirements. Integrity is separate from execution, provenance,
independent review and promotion.

## Promotion criteria

Rust/PQ2 with four row workers is the owner-directed development default, with
an explicit disclaimer and manual publisher rollback. That selection makes no
full-qualification claim and changes none of the evaluator gates below. PTQ1
promotion and removal of the qualification disclaimer still require the complete
reviewed evidence.

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
