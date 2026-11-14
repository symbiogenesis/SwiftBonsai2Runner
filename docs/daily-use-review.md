# Daily-use review packet

This packet defines review work for A1/R1, D1 and Q1 in [PLAN.md](../PLAN.md).
It contains task specifications, not captured results or an acceptance receipt.
[Validation](validation.md) owns execution, power, cleanup and promotion rules;
[performance measurement](performance-expectations.md) owns timing and listening
interpretation. Full matrices remain in their owning CI.

## Source and acceptance handoff

Finish local integration before selecting a candidate. Record the full Runner
and Prism commits, compiler/channel/target, runtime archive and executable hashes,
model/representation, adapter hash and scale, selected workers, cache/context,
effective placement and device/SKU/driver identities. Use the exact-source
package; do not substitute a previously built helper after source changes.

The owner publishes reviewed local `main` and operates Source checks, Foundation
and dependency auditing for C1/L1. The following are owner commands, run from the
reviewed Runner checkout after publication:

```powershell
gh workflow run source-checks.yml --ref main
gh workflow run foundation.yml --ref main
gh workflow run native-runtime-acceptance.yml --ref main -f run_device=false -f profile=pq2-adapter-row4
```

Inspect the preparation run's successful strict Release analyzer, production-link,
workflow-contract and exact-inventory receipts before device work. A bundle-only
Prism run does not qualify Foundation or supply Runner's acceptance package.

Select one feasible application workload before dispatch. The minimal production
adapter profile below requests exactly one four-token stream with original PQ2,
adapter 1.3 and four workers, readiness and cleanup. It does not exercise full
Stop/Retry, long histories, complete-answer quality or performance. Its dispatch
prepares its own same-run package rather than consuming the preceding run.

```powershell
gh workflow run native-runtime-acceptance.yml --ref main -f run_device=true -f profile=pq2-adapter-row4 -f owner_budget_minutes=5
```

Do not use that command until a conservative whole-job estimate covers setup,
intake, hashes, model loading, output, power observations, teardown and retention
within five minutes. If it cannot fit, leave it pending or predeclare the smallest
justified supported extension under the existing budget contract. Never use
separate profiles or repeated dispatches to continue an incomplete matrix.
Retain aggregate AC/saver/charge observations before setup and after retention,
plus each arm's endpoints; unknown conditions stay unknown.

Actual native UI acceptance independently checks: launch with explicit DX12;
draft while preparation runs; send one admitted prompt; Stop an active reply;
confirm model/broker acknowledgment, empty Jobs and released listeners; explicitly
Reload model and Retry response; preserve drafts/history across switching and
recovery. Reload alone must never resend. Use a distinct predeclared feasible
workload when this work is selected, without restarting an earlier trial clock.

## Portable setup and update review

Use the source-bound single executable on native Windows ARM64, with an
independently selected fresh/custom data directory. Select a feasible subset
before device work; this inventory is not one authorized laptop matrix.

| Area | Required observation |
| --- | --- |
| Fresh launch | Double-click opens the native window; startup errors remain visible; no separately installed VC runtime or TTS inference DLL is required. |
| Setup | Text readiness remains independent of speech; pause/retry preserves drafts and launch options; interrupted downloads resume verified original bytes. |
| Repair | Corrupt originals and unrelated files survive until a verified atomic replacement; insufficient disk, network and permission failures leave a recoverable state. |
| Cancellation | A stalled response body stops and its worker joins promptly after cancellation, before the operation deadline; close leaves no owned descendants or listener. |
| Updates | Download and verification precede the prompt; Later leaves the Settings badge; deferred close replaces without reopening; immediate replacement restarts with original options. |
| Failure/rollback | Tampered bytes, changed publication, sharing/read-only failures and unknown teardown preserve the original executable and user data; record restart failure separately. |

Schannel/system trust, proxy and ignored-Range behavior require their own owning
integration controls. Loopback cancellation and source fixtures do not establish
Internet/proxy, clean-machine or distribution acceptance.

## Creative writing packet

Use complete answers with the pinned standard adapter at 1.3. Freeze the exact
prompt bytes, rendered/template identity, sampler/seed, Thinking, Extra effort,
output limit, context and branch history before capture. Choose the production
settings being reviewed; do not inherit a historical fixed-greedy verdict.
Reasoning and intermediate revisions count toward the declared workload but are
distinct from the final answer. A token-cap or interrupted ending is incomplete.

The following original tasks are held out from fitting and candidate selection.
If any task has already informed a candidate, replace it and freeze its replacement
before capture. Select one independently useful bounded capture or use owning CI
for a declared collection; do not split a matrix into repeated laptop trials.

| Task | Exact user prompt | Objective review |
| --- | --- | --- |
| Scene | Write a complete 120 to 170 word scene. Inez repairs clocks but cannot hear their chimes. Her apprentice brings a clock that has stopped at tomorrow's sunrise. Include a tactile detail, a line of dialogue and a concrete decision by Inez. Keep her hearing limitation consistent. End with an action, not an explanation of the mechanism. | Word range; hearing limitation; tactile detail; dialogue; decision; completed ending. |
| Continuity | Canon: Mara cannot swim. Yesterday the town's only bridge washed away. Her brother waits across the river with a lantern. Continue in 120 to 170 words without changing those facts or introducing a new bridge. Give Mara a plausible next step. End with a question spoken by Mara. | All canon remains true; no unsupported crossing or new bridge; plausible action; final speaker/question; word range. |
| Revision | Revise this passage into 90 to 130 words, preserving every event and its order. Use close third person from Ada's perspective, avoid adding a motive for the visitor, and replace abstract emotion with observable detail. Passage: Ada locked the shop at dusk. A visitor knocked three times. She hid a blue envelope beneath the till. She opened the door. The visitor returned her lost key and left without speaking. | Lock, three knocks, hidden blue envelope, opened door, returned key and silent departure stay in order; perspective and word range; no invented visitor motive. |
| Practical ideation | Design an inexpensive two-hour neighborhood skill exchange. Give it a memorable name, a schedule totaling exactly 120 minutes, three concrete skill examples, and a specific way for a shy newcomer to participate without speaking to the whole group. Use 120 to 180 words. | Exact duration; three practical examples; low-cost feasibility; concrete newcomer option; word range. |

Count words as nonempty runs separated by Unicode whitespace in the complete
final answer, including any headings; punctuation within a run does not split it.
Record the count and independent reviewer's identity with each exact answer hash.
An independent human reviews the exact hashed final answer. Score each applicable
dimension from 0 to 4: 0 unusable, 1 major defects, 2 material defects, 3 useful with
minor defects, 4 fully satisfies the task with no identified defect. Record a
reason and supporting passage for every score; unknown or missing review remains
pending. For this baseline packet, every applicable dimension must reach 3 and
every objective check must pass. This is a declared packet threshold, not a change
to independent LoRA/default-promotion gates.

| Dimension | Question |
| --- | --- |
| Instructions | Does the complete answer satisfy the task's content, format and length constraints? |
| Local prose | Are details specific, sentences readable, dialogue purposeful and repetition controlled? |
| Continuity | Do character abilities, chronology, world facts and causal actions remain consistent throughout the supplied context and answer? |
| Usefulness | Does the answer make a concrete creative or practical contribution and finish the requested task? |

Flag generic filler, invented canon, unwanted refusals, abrupt truncation and
contradictions separately; a high average cannot erase a failed task/dimension.
A scene packet does not qualify whole-story coherence. For a story claim the
reviewer needs the complete relevant narrative and its unresolved promises.
When comparing candidates, blind the labels, predeclare order and tolerances,
and preserve losing/no-go outcomes. Keep private writing local.

## Speech listening packet

Keep Q8/Q4, voice description, seed/sampling, thread count and exact model/runtime
identities separate. Use these fixed public synthetic texts:

1. `At six, the station clock rang twice. "Wait," said Lena. "The last train is early." Outside, rain softened the chalk arrows on the platform.`
2. `First, fold the blue card. Next, place it beside the cup. Pause here. Then read the final line quietly: the room is ready.`

Preserve original PCM/WAV bytes, sample rate/channels, sample count and SHA-256.
For live read-aloud also retain the exact paragraph/chunk boundaries, their
generation order and completion state; file continuity cannot prove audible
continuity. Distinguish voice bootstrap, first PCM, first audible sound, warm
synthesis, playback gaps and terminal cleanup. Never infer Snapdragon latency
from a hosted capture. These texts require a separately prepared, source-bound
capture; the current TTS workflow has no text input and does not generate them.
Its existing fixed-text comparison (`The quick brown fox crossed the quiet garden.`)
remains hosted-only and supplies a separate listening control:

```powershell
gh workflow run tts-evaluation.yml --ref main -f target=hosted-arm64 -f repetitions=1 -f response_format=pcm
```

An independent listener reviews the identified complete clip for intelligibility,
voice consistency, pronunciation, punctuation/prosody and audible cuts/gaps,
using the same 0..4 anchors and a reason for each dimension. Every applicable
dimension must reach 3 for this packet; record dropouts, missing words and
truncation separately. Listener/device, playback level and review order must be
identified. Loudness-matched copies require a declared gain and separate hashes;
original samples remain the numerical authority. No loudness helper is required
or presumed installed by this packet.

## Retained result

Keep one concise summary binding the selected task, exact identities/settings,
complete output/audio hashes, objective checks, independent reviewer scores and
reasons, termination, timing/resource observations, budget, power and cleanup.
Retain the selected answer/audio bytes needed by that review and any independent
replay inputs required by the owning gate. Failed, skipped, unknown and losing
results stay visible. Do not create empty scorecards, placeholder success receipts
or duplicate evidence bundles. Source/model/device changes require a new scope
review; they do not inherit an old verdict.
