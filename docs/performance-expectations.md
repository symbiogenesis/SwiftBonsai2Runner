# Performance measurement

The current development profile is defined by the [runtime lock](../config/prism-inference.lock.json),
[inference defaults](inference-defaults.md) and [memory policy](swift-memory.md#memory-selected-serving-context).
There is no qualified sustained-performance figure for this profile.
[PLAN.md](../PLAN.md) tracks acceptance. Older runtimes' short-request observations
cannot predict its speed, memory use, creative quality or effective accelerator placement.

## Current Swift evidence

Loading, short decode and synthetic controls do not establish a useful completed
answer, full-context behavior or repeated performance benefit. A release or repin
also supplies no model timing. Keep model-free, actual-model, resource, quality
and device acceptance separate under [validation](validation.md).

Publisher measurements for base Bonsai, Qwen or other hardware have different
weights, arithmetic, workloads and observation boundaries. They are not Runner
or Swift measurements and cannot serve as a Snapdragon forecast.

## Measurement and comparison

Use the same observation boundary for both arms. Record request start, first output,
first visible/useful answer, last token, terminal framing and teardown separately.
Reasoning, buffered delivery and speculative bursts can separate those events.

For `N >= 2` output tokens over a positive observed interval, define
`r = (N - 1) / (t_last - t_first)`. Then
`last-token latency = TTFT + (N - 1) / r` is an identity at that boundary.
For one token, last-token latency equals TTFT. An arithmetic average of rates need
not preserve the identity. SSE events or buffered bursts are not token counts;
client-observed delivery rate is not isolated kernel decode throughput.

| Comparison requirement | Record |
| --- | --- |
| Paired repeated trials | Balance/reverse order; identify cold/warm/prefix-cache state. Hold model, adapter, template, sampler, context, output and numerical contracts fixed. Retain complete output or label truncation. |
| Actual placement | Bind effective CPU/GPU/NPU execution, device/SKU, driver, source/build and immutable artifacts. Requested acceleration or readiness alone does not prove offload. |
| Complete task | Charge initialization, loading, repack/compile, migration, prefill, decode, retries and revisions to the workload that pays them. Separate profiled attribution from unprofiled timing. |
| Memory | Separate persistent payload, temporary peak, private/Job commit, file-backed residency, device allocations and machine headroom. Charge scales, scratch, snapshots and retained high-precision copies. Never add overlapping counters. |
| Power and comfort | Record aggregate/per-arm AC, saver and charge observations; missing/mismatched conditions disqualify performance comparisons. Matching endpoints do not prove continuous stability or energy use. Measure useful-answer energy only with valid telemetry. Retain responsiveness, heat/fan and idle observations. |

Two short reversed pairs can supply a smoke comparison, not statistical or sustained
qualification. Stable TPS can coexist with worse desktop responsiveness under
shared memory/power contention. Include uncertainty and losing/no-go results;
use predeclared acceptance limits and [promotion criteria](validation.md#promotion-criteria).

Benchmarks and complete suites remain CI-only. Owner-device trials share one
predeclared allowance across setup, intake, loading, every arm, power, cleanup and
retention under [AGENTS.md](../AGENTS.md#owner-device-execution-budgets).
Do not retry, reopen a closed clock or split an infeasible matrix across repeated runs.

## Creative review packets

The [daily-use review packet](daily-use-review.md#creative-writing-packet) supplies
fixed original writing tasks, objective checks and independent prose/continuity
rubrics. Its [speech packet](daily-use-review.md#speech-listening-packet) defines
complete synthetic listening texts and sample/reviewer identities. These are
capture specifications; missing captures or human reviews remain pending.

The [Longform Writing Bench rubric organization](https://github.com/EQ-bench/longform-writing-bench/tree/main/data)
is a candidate source for independent creative review. Separate scene/chapter prose
quality from whole-narrative coherence: character consistency, chronology, world
rules and unresolved plot promises need the complete relevant story. Declare the
criteria, weights, negative criteria and review scope before comparing complete
held-out answers, including Thinking and Extra effort settings. Retain independent
human review and distinguish prose quality from token count or completion speed.

Adapted short packets are not leaderboard-equivalent. Its Python harness and
automatic external judging are outside this proposal; private prompts and prose
stay local unless the owner separately authorizes an external review. Source and
reuse limits live in the [speech and creative review references](research-register.md#speech-and-creative-review-references).
Existing trial budgets, per-arm power eligibility and evidence retention still
govern any selected capture.

## X1/X2 configuration evidence

[Hardware policy](hardware-capabilities.md#automatic-profile-selection) owns
selection, admission, phase handoff and qualified fallback/refusal. Family/core
counts, NPU TOPS and bandwidth bounds predict neither TPS nor energy: decode,
prefill and concurrent processors pay different bandwidth/preparation costs.

Compare each backend and phase combination at exact device/driver/runtime
identities. Shared CPU changes may follow the [cross-generation applicability rule](cpu-kernels.md#cross-generation-cpu-promotion),
without relabelling X2 measurements as X1 results. Family-specific tuned defaults,
accelerator rankings and complete device profiles retain independent acceptance.

## Q4 speech resource comparison

Q4 talker weights remain a separate experiment from the default Q8 speech profile.
Compare exact model/codec, voice reference, seed and thread settings with fresh
processes and complete clips. VoiceDesign and Base run sequentially; their active
weight reductions cannot be counted twice. Smaller files or lower short-fixture
working sets do not establish the cause, total-system memory savings or Snapdragon
benefit. Keep Job commit and mapped residency distinct and retain the physical reserve.

Measure voice bootstrap, warm talker prefill, residual-code steps, codec chunks
and playback. First generated PCM and first audible sound are separate endpoints.
Include chunk supply gaps/underruns, long-read memory and simultaneous chat/speech.
Average faster-than-real-time synthesis can still stall playback. Framing/ownership
checks cannot replace real-device audio or independent listening for timbre,
continuity, timing and prosody. See [speech controls](../tools/qwen-tts/README.md).

The Rust [ebur128 analyzer](https://github.com/sdroege/ebur128) is a candidate
review helper for integrated/short-term loudness, loudness range and true peak.
If loudness matching is used for listening, apply the declared gain only to
separately identified listening copies and record their relationship to the
original samples. Original generated PCM remains the authority for numerical,
streaming and continuity comparisons. Record unavailable measurements explicitly;
this proposal sets no loudness threshold and changes no serving PCM. Matched
loudness controls a listening confound, but establishes neither numerical equality
nor intelligibility, voice identity or audible quality. Follow the
[source and adoption limits](research-register.md#speech-and-creative-review-references)
before introducing the helper.
