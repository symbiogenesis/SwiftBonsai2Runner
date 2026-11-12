# Performance expectations and benchmark context

The [README charts](../README.md#performance-expectations) reproduce publisher results on
zero-based axes. They are reference points, not Runner measurements.

## Published reference results

The speed chart uses base Bonsai 2 PQ2 `llama-bench` TG128: 128 generated tokens, batch 1, depth 0,
no vision tower. The quality chart uses the publisher's 14-benchmark thinking-mode average,
evaluated with EvalScope/vLLM on H100; it includes vision tasks. Neither chart evaluates Swift or
our Qwen Q3 reference. Source: [Prism's model
card](https://huggingface.co/prism-ml/Ternary-Bonsai-2-27B-gguf#cross-platform-throughput), checked
November 9, 2026. The [source refresh](evidence/publisher-reference-20261109.json)
confirmed that all seven plotted reference values remain unchanged.

Other-hardware throughput and general benchmark averages predict neither Snapdragon speed
nor prose quality. Measure useful-answer time under the protocol below.

## Current Swift evidence

The [Prism 0.1.25 registry delivery](evidence/prism-0125-publish-20261112/summary.json)
and binary repin supply no new model timings. Fresh bounded acceptance remains
in [the plan](../PLAN.md).
The measurements below keep their original 0.1.23 identities and values.

The [November 9 Prism 0.1.23 capture](evidence/readme-performance-0123-20261109/summary.json)
followed actual crates.io publication, the verified GitHub development prerelease
and Runner's binary repin to `055c743`. It measured one request per representation
through public broker HTTP/SSE on the Snapdragon X2 Elite Extreme, with four observed
row workers, standard adapter scale 1.3, seed 42, Thinking off and an eight-token cap:

| Model | Runtime | First-request TTFT (s) | Generation tokens/second | Model file | Peak Job commit | Available RAM admission |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| **Swift Bonsai 2 `PQ2_0` (development default)** | **Rust 0.1.23; default acceleration policy** | **8.23** | **2.66** | **6.71 GiB** | **7.07 GiB** | **15.5 GiB** |
| **Swift Bonsai 2 `PTQ1_0` (experimental opt-in)** | **Rust 0.1.23; CPU** | **15.68** | **1.41** | **5.54 GiB** | **5.89 GiB** | **15.5 GiB** |

Both fresh owners returned `1, 2, 3,` for `Count from 1 to 100.`, with eight
reported completion tokens and `length`. Eight nonempty ASCII deltas and the
reviewed pinned non-speculative callback support seven positive token intervals.
TTFT begins at client request start after HTTP readiness and can include remaining
asynchronous model/device preparation. Generation is seven divided by the time
between the first and last token receipts. Broker forwarding, exact-count admission,
receive coalescing and observer receipt I/O remain included.

Peak Job commit was `7,588,093,952` bytes for PQ2 and `6,326,587,392` bytes for PTQ1,
a `1.17487 GiB` difference. It includes startup and descendants, excludes mapped-file
residency and device allocations, and does not establish total RAM use or a resource
benefit. PQ2 used the default X2 Hexagon policy with CPU fallback; effective NPU
execution remains **unknown** because authenticated readiness does not expose it.
Both owners confirmed empty Jobs and released listeners.

All 90 power observations, including 81 periodic samples and separate aggregate,
per-arm and post-retention endpoints, reported AC connected, saver off and 99%
charge. Sampling does not prove uninterrupted stability. Cache-only provisioning,
one offline Release build, four focused controls, both requests, cleanup and
evidence/manifest retention finished in **87.649 s** of the predeclared seven-minute
allowance (380-second conservative estimate), without a retry or CI action.
The raw receipts and independently recomputed metrics are retained. These single
short requests do not establish sustained speed, repeated benefit, quality,
application acceptance or a controlled cross-version speedup. Keep the earlier
observations below separate; their original clocks remain closed.

### Earlier 0.1.22 capture

The [November 9 Prism 0.1.22 capture](evidence/readme-performance-20261109-attempt2/summary.json)
measured two short requests through the public broker HTTP/SSE API on the Snapdragon
X2 Elite Extreme, with four observed row workers, standard adapter scale 1.3,
seed 42, Thinking off and an eight-token cap:

| Model | Runtime | First-request TTFT (s) | Generation tokens/second | Model file | Peak Job commit | Available RAM admission |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| **Swift Bonsai 2 `PQ2_0` (development default)** | **Rust 0.1.22; default acceleration policy** | **6.63** | **3.16** | **6.71 GiB** | **7.06 GiB** | **15.5 GiB** |
| **Swift Bonsai 2 `PTQ1_0` (experimental opt-in)** | **Rust 0.1.22; CPU** | **20.08** | **1.59** | **5.54 GiB** | **5.89 GiB** | **15.5 GiB** |

Each fresh owner received `Count from 1 to 100.` and returned `1, 2, 3,` with
eight reported completion tokens and `length`. Exact ASCII deltas, monotonic receive
timestamps and the pinned non-speculative per-committed-token callback establish
the seven post-first-token intervals. First-request TTFT begins after verified
HTTP readiness and can include remaining asynchronous model/device preparation.
Generation is `(N - 1) / (last token receipt - first token receipt)`, excluding TTFT.
Public broker forwarding, exact-count admission, receive coalescing and observer
receipt I/O remain included. These are short-window client observations.

PQ2 used the default policy that attempts X2 Hexagon acceleration with CPU fallback;
authenticated readiness does not expose effective placement, so actual NPU use is
**unknown**. Peak Job commit was `7,576,604,672` bytes for PQ2 and `6,320,873,472`
bytes for PTQ1, a `1.16949 GiB` difference. This counter excludes mapped-file
residency and device allocations; it is not total RAM use or a qualified resource benefit.
Both owners confirmed empty Jobs and released listeners. All 57 periodic power
samples and aggregate, per-arm and post-retention endpoints observed AC connected,
saver off and 98% charge; sampling does not prove uninterrupted stability.
One cached offline Release build, two synthetic controls, both requests, cleanup
and evidence/manifest retention finished in **59.250 s** of a predeclared ten-minute
allowance (365-second conservative estimate). No further retry or CI action occurred.
Sustained speed, repeated benefit, quality and application acceptance remain pending.

This 0.1.22 attempt was explicitly authorized after the
[earlier preparation failure](evidence/readme-performance-20261109/summary.json).
That original 69.432-second clock remains closed; its original receipts remain
unchanged in Git history. No
cross-version speedup is inferred: the November 3 observations below used direct
sandboxed backend HTTP, while the new capture includes the public broker.

| Publisher evidence | What it establishes |
| --- | --- |
| Current merged PQ2 | IFBench and AIME were refreshed; AIME final-answer-only ties the base. |
| Historical runtime correction | GPQA/C-Eval were not rerun on current merged weights. |
| Evaluation configuration | Supports the [sampler baseline](inference-defaults.md#applied-defaults), without a sampler/effort/prompt sweep or creative-writing comparison. |

Small score differences do not establish gains; token counts do not measure latency.
The publisher reports uneven instruction following, tool use and open-ended coding. See
[Swift's evaluation and limitations](https://ukisai.com/swift-bonsai-2#evaluation) and its
[evaluation artifact](https://huggingface.co/ukisai/Swift-Bonsai-2-GGUF/blob/main/benchmark_results.json).

The [memory guide](swift-memory.md) records 16-token Prism CPU decode and PTQ1 results;
the [Rust runtime contract](native-rust-engine.md) owns NEON/hybrid-graph evidence.
Neither establishes complete-answer performance. The historical
[2026-11-03 separated timing capture](evidence/token-timing-20261103/summary.json)
measured seven installed Rust profiles on the Snapdragon X2 Elite Extreme:

| Model | Runtime | Time to first token (s) | Generation tokens/second | Model file | Job commit cap / measured peak | Available RAM admission | Restore copy saved vs. Rust 0.1.4 |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Swift Bonsai 2 `PQ2_0` | Rust 0.1.14 release (4 workers) + LoRA 1.3 | 16.64 | 1.34 | 6.71 GiB | 11.5 GiB / 7.20 GiB (short request) | 15.5 GiB | 149.625 MiB |
| Swift Bonsai 2 `PTQ1_0` | Rust 0.1.14 release (4 workers) + LoRA 1.3 | 29.50 | 0.75 | 5.54 GiB | 11.5 GiB / 6.02 GiB (short request) | 15.5 GiB | 149.625 MiB |
| Swift Bonsai 2 `PQ2_0` | Rust 0.1.13 release (4 workers) + LoRA 1.3 | 16.45 | 1.32 | 6.71 GiB | 11.5 GiB / 7.20 GiB (short request) | 15.5 GiB | 149.625 MiB |
| Swift Bonsai 2 `PTQ1_0` | Rust 0.1.13 release (4 workers) + LoRA 1.3 | 28.69 | 0.75 | 5.54 GiB | 11.5 GiB / 6.02 GiB (short request) | 15.5 GiB | 149.625 MiB |
| Swift Bonsai 2 `PQ2_0` | Rust 0.1.10 release (4 workers) + LoRA 1.3 | 16.98 | 1.34 | 6.71 GiB | 11.5 GiB / 7.20 GiB (short request) | 15.5 GiB | 149.625 MiB |
| Swift Bonsai 2 `PQ2_0` | Rust 0.1.10 release (1 worker) + LoRA 1.3 | 35.59 | 0.64 | 6.71 GiB | 11.5 GiB / 7.20 GiB (short request) | 15.5 GiB | 149.625 MiB |
| Swift Bonsai 2 `PQ2_0` | Rust 0.1.9 release (4 workers) + LoRA 1.3 | 23.00 | 1.32 | 6.71 GiB | 11.5 GiB / 7.20 GiB (short request) | 15.5 GiB | 149.625 MiB |
| Swift Bonsai 2 `PTQ1_0` | PrismML C++ CPU | Startup failed | Startup failed | 5.54 GiB | 11.5 GiB / — | 15.5 GiB | — |
| Swift Bonsai 2 `PQ2_0` (explicit publisher rollback) | PrismML C++ CPU | Startup failed | Startup failed | 6.71 GiB | 11.5 GiB / — | 15.5 GiB | — |
| Qwen3.8-27B `UD-Q3_K_XL` | llama.cpp reference | Not installed | Not installed | 12.24 GiB | 18 GiB / — | 22 GiB | — |

The historical release profiles above used four CPU row workers unless explicitly
labelled otherwise. Comparing the
`0.1.10` single-worker row (`35.59 s` TTFT, `0.64` generation tokens/s) against
the four-worker row (`16.98 s` TTFT, `1.34` generation tokens/s) gives observed
ratios of **2.10x** for first-token time and **2.09x** for generation rate.
Four-worker TTFT was `23.00 s` on `0.1.9`, `16.98 s` on `0.1.10` and
`16.45–16.64 s` on `0.1.13`–`0.1.14`. These single fixed-order observations do
not isolate the cause or qualify a repeated-trial speedup. Short-request peak Job
commit measured **7.20 GiB** (`7,726,850,048–7,727,276,032` bytes) across all five
`PQ2_0` profiles and **6.02 GiB** (`6,465,683,456–6,465,744,896` bytes) across both
`PTQ1_0` profiles (**1.17 GiB lower peak Job commit** on short requests).
Each fresh owner used the fixed prompt `Count from 1 to 100.`, thinking off,
seed 42 and the standard Swift adapter at scale 1.3. All seven measured responses
reported eight completion tokens and returned `1, 2, 3,` before hitting the cap.
The historical **Rust Prism `0.1.23`** control (`055c743`, building on `0.1.22` `6b4f452`)
includes the `0.1.15`–`0.1.22` CPU NEON, 18-worker row parallelism (`MAX_ROW_WORKERS = 18`),
`TL1` nibble lookup tables (`Tl1BlockLut`), `TQ1_0`/`TQ2_0` LUT + NEON SIMD, direct
`PTQ1 × Q8_K` dot kernels, multi-worker GDN/attention/conv/SwiGLU, `4x4`/`8x8` tiled-prefill
GEMM, training-free suffix speculative decoding (`SuffixDecoding`) with transactional block
verification, and Snapdragon X2 6-arena 168-projection (`blk.0..=blk.26`,
`2,888,663,040` resident weight bytes / `2,754.84 MiB`) `native-q2` resident NPU optimizations
(including 8-op `dspqueue` batching and fused `ffn_gate + ffn_up + HTP_OP_GLU_SWIGLU` VTCM
chaining). Version `0.1.23` also includes direct floating projections, compact rollback
checkpoints, validated-logit sampling and recurrent staging. The latest November 9
capture above records short requests on `0.1.23`; sustained performance and matched
repeated benefit remain pending.
See [Hexagon execution contracts](hexagon.md#snapdragon-x2-hexagon-npu-v81-projection-and-layer-timings)
for the supervised 3-token (`42, 43, 128`) X2 device control timings (`0.64–0.98 ms` for 1
projection, `1.08–1.44 ms` for 2 projections, `2.27–2.87 ms` for 6 recurrent projections in `blk.0`,
`4.43–4.87 ms` with `422.49–446.20 ms` warm token evaluation for 13 projections in `blk.0 + blk.3`,
and `10.05–10.91 ms` for all 25 projections of the 4-layer repeating block `blk.0..=blk.3`).

For the November 3 capture, **TTFT** runs from client request start through the first output token,
including required exact token-count admission, template processing, prefill and HTTP
transport; model verification/loading/startup is excluded. **Generation tokens/s**
is `(N - 1) / (last token time - first token time)`, using reported completion
usage `N`, so it excludes TTFT. Incremental receive timestamps and source-verified
ASCII token boundaries support these seven-token intervals; SSE event counts
alone are not token counts. These are **client-observed short-window rates** over
direct authenticated sandboxed backend HTTP, not isolated kernel decode TPS,
sustained generation or a qualified speedup comparison.

Both publisher options exited before HTTP readiness, leaving their timings
unavailable; the cause remains unknown. The pinned Qwen model was absent from
the selected installed data, so no download or model attempt was made. Every
started owner confirmed an empty Job, released its listener and left no model
processes or trial credentials. This validates the measurement owner's cleanup;
the installed application broker's earlier graceful-shutdown failure remains open.

AC was connected and battery saver off at all **336 per-arm power samples** and
the aggregate endpoints; charge rose from **43% to 46%**. Sampling does not prove
uninterrupted stability. Setup, the streaming check, one offline Release build,
all nine started arms, cleanup and final evidence retention shared one
predeclared fourteen-minute ceiling and finished in **473.890 s**. No arm was
retried. The [earlier whole-request observations](evidence/local-throughput-20261103/summary.json)
and [historical one-worker attempt](evidence/native-request-speed.json) keep their
original intervals and closed clocks; their request rates cannot supply TTFT or
generation speed. [PLAN.md](../PLAN.md) owns pending acceptance under
[validation](validation.md) and the independent [LoRA gates](swift-lora.md#standard-profile-qualification).

## X1/X2 configuration evidence

[Hardware policy](hardware-capabilities.md#automatic-profile-selection) owns selection, admission, phase handoff and qualified CPU
fallback/refusal. A sustained X2 speedup and qualified automatic selector remain
unaccepted; the fixed [X2 NPU development default](hexagon.md) is separate.
Family/core counts, NPU TOPS and
bandwidth bounds cannot forecast TPS or energy: prefill can reuse weight tiles, while single-chat
decode pays bandwidth, unpack/transform and dispatch costs with little reuse. Whole-model expansion
can erase packing benefits; simultaneous processors share bandwidth and power.

Compare CPU/Adreno/Hexagon and phase combinations separately for each SKU/driver/runtime, using
the measurement protocol below. Interactive/battery rankings require matching-device evidence;
focused untimed discovery/selection checks cannot establish them.

Shared CPU work reductions use the
[cross-generation promotion rule](cpu-kernels.md#cross-generation-cpu-promotion):
matched CPU-only whole-request X2 benefit plus a passing X1 applicability review
can authorize common deployment without an X1 benchmark. X1 speedup figures and
family-specific tuned defaults still need X1 evidence; accelerator rankings and
complete device-profile qualification remain separate.

## 2026 literature: measurement and comparison

The [systems catalog](research-register.md#2026-systems-measurement-and-output-budget-work) records
sources reviewed **2026-10-29** and their platform limits:

| Source | Measurement lesson and limit |
| --- | --- |
| MLPerf Client v2.0 | Separate TTFT/token rate, vary prompt/output lengths and retain accuracy gates. Windows Arm/X/X2 coverage uses other models/quantizers; experimental power tools and general accuracy do not qualify Swift creative quality. |
| Sequential Isolation Protocol; MLSys deployment search | Repeat controlled phase-specific comparisons. A100/vLLM variance and deployment results supply neither laptop thresholds nor a universal batch size. |
| Edge-continuum study | Compare latency/energy tradeoffs; its Jetson/server frontier is not a Snapdragon CPU verdict. |

For `N >= 2` output tokens, define the interval rate as `r = (N-1)/(t_last-t_first)` over a positive
measured interval at the same observation boundary. Then `last-token latency = TTFT + (N-1)/r` is an
identity; arithmetic averages of per-token or per-run rates need not satisfy it. For one token,
last-token latency is TTFT. Terminal framing, flushing and teardown may add time afterward and have
separate endpoints. Reasoning tokens, buffered delivery and speculative bursts make *first output*,
*first visible answer* and *first useful answer* distinct observations. A client burst rate is not
the device decode rate.

| Comparison requirement | Record |
| --- | --- |
| Paired repeated trials | Reverse or balance order; identify cold/warm/prefix-cache state; hold artifacts, adapter, sampler and numerical contracts fixed. Retain complete output or label truncation. |
| Phase and task costs | Load/initialization, repack/compile, migration, prefill, first useful answer, decode and complete task. Charge each cost to the workload that pays it. Use profiled runs for attribution and unprofiled runs for request timing. |
| Memory | Persistent payload, temporary peak, private/commit, file-backed residency, device memory and machine headroom. Identify excluded reconstruction scratch or retained high-precision copies before comparing compression ratios. |
| Energy and comfort | Observable energy per useful completed answer, output/retry/revision counts, desktop responsiveness and idle power. Cheaper tokens can cost more per useful answer. |

Stable TPS can coexist with worse foreground behavior under shared bandwidth/power contention;
see [mobile bandwidth research](hexagon.md). Keep current admission limits;
timing/profile captures and benchmarks remain [CI-only](validation.md).

## Q4 speech resource comparison

The [hosted ARM64 comparison](https://github.com/symbiogenesis/SwiftBonsai2Runner/actions/runs/40445129576)
at Runner `90db7f71033adeb2e262ec6f97f7dc360fceb9c8` covers eight fresh-process sessions
and sixteen bootstrap/warm WAV records on a four-core Cobalt 100. Every clip passes
independent framing checks. Workers exit only after supervisor stop; none exits spontaneously.

Positive differences below mean lower Q4 usage. Counter domains are separate.

| Threads | Repetition | Q8 minus Q4 peak Job commit | Q8 minus Q4 peak working set |
| ---: | ---: | ---: | ---: |
| 2 | 1 | 0.734196 GiB | 1.532074 GiB |
| 2 | 2 | 0.734276 GiB | 1.532158 GiB |
| 4 | 1 | 0.734226 GiB | 1.532124 GiB |
| 4 | 2 | 0.734653 GiB | 1.532566 GiB |
| Overall median | | **0.734251 GiB** | **1.532141 GiB** |

The active talker file difference is 860,203,008 bytes (0.801126 GiB). Design and Base talkers are
sequential, so their reductions count once for active weights. The Q8 codec is unchanged. The larger
observed working-set reduction does not identify its allocation cause. Job commit excludes mapped
residency; neither counter is added to the other or treated as complete system RAM.

Private/resident sampling at 25 ms can miss brief allocations. Minimum sampled system
availability is 7.015308 GiB, above the 5 GiB reserve; admission is 9 GiB and the commit
ceiling 16 GiB. These hosted-CPU, short-fixture results do not qualify Snapdragon,
long clips or independent listening. Q8 remains default; [PLAN.md](../PLAN.md) owns
qualification.

The report SHA-256 is `9b9aecf03a2c0a2467a3b3c8f316c4ee0c2c237dbe67e22975fa5c9b07285eb4`. The TTS
executable was 1,914,368 bytes, SHA-256
`2ac52dddc913ca4e193a2614262d154bd63632cc12a23521c8f2cdfc9e4999d8`. Artifact `12708687596` retains
the exact model identities, individual observations, failure diagnostics and synthetic listening
clips.

## 2026 speech inference research

The [mobile/speech catalog](research-register.md#2026-mobile-acceleration-and-speech) records
sources reviewed **2026-10-29**. Their implications below retain the
[speech contract](../tools/qwen-tts/README.md): sequential VoiceDesign/Base, reused conditioning,
bounded playback and existing admission/reserve. No model, quantization, thread or accelerator
change is qualified by these papers.

| Lead | Decision boundary |
| --- | --- |
| Qwen3-TTS report | Separate talker, residual-code predictor and causal codec invocation rates/first-packet prerequisites. Publisher 97 ms is not this 1.7B GGML CPU path, fixed-reference bootstrap or audible playback; 12Hz/25Hz tokenizers differ. |
| RVQ position-aware speculation | Frozen heads plus 3,072 learned offsets/sparse verification introduce artifacts. Reported 2–2.2× RVQ-token speed is 0.6B CustomVoice/Apple, not 1.7B VoiceDesign/Base GGML/full readout; distributional equality is not seeded-audio identity. |
| One Megakernel Is Not Enough | Producer/codec rates, registers, retained resources and first-audio dependencies guide fusion boundaries. H100/CUDA measured-plan choices are not a portable kernel or laptop forecast. |
| On-device streaming ASR | Chunk/right-context, caching, selective quantization/fusion trade latency/quality/storage. 32-core EPYC, Nemotron/Canary-1B-v2 results are not Canary-Qwen-2.5B; streaming needs compatible trained state, and smaller files do not prove smaller processes. |
| Qwen3-TTS conditioning/failure study | Listening must evaluate timbre, timing, continuity and prosody; automatic metrics/WER can disagree. Cloning results do not qualify description-designed references. Keep reference/seed fixed while isolating optimization. |

Attribute voice bootstrap, warm talker prefill, residual-code steps, codec chunks and playback
before optimizing. Conditioning reuse and bounded retained decode graphs already exist.
Workspace/fusion changes retain cancellation, PCM framing and voice identity; speculation needs
accepted-prefix state, sampling correction and bounded scratch. Even a tiny trained drafter is a
separate artifact.

Measure first generated PCM separately from first audible output, chunk supply gaps/underruns,
long-read peak memory and combined chat/speech use in target-device CI. Average
faster-than-real-time synthesis can still stall individual chunks, while overlap contends with
Swift/native rendering. Framing/ownership checks do not replace these measurements or independent
listening.
