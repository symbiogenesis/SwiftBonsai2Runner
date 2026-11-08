<div align="center">

# 🌱 Swift Bonsai 2 - Runner

**A local runner for the Swift Bonsai 2 model on Windows ARM64.**

Creative writing · Brainstorming · Art direction

![Platform: Windows ARM64](https://img.shields.io/badge/Windows-ARM64-0078D4?style=flat-square)
![UI: native Rust](https://img.shields.io/badge/UI-native_Rust-50855B?style=flat-square)
[![License: Apache 2.0](https://img.shields.io/badge/license-Apache_2.0-50855B?style=flat-square)](LICENSE)

[Get started](docs/development-guide.md) · [Documentation](#documentation) · [TODO](PLAN.md)

</div>

---

Swift Bonsai 2 is a language model by [UkisAI](https://ukisai.com/swift-bonsai-2),
built on [PrismML's Bonsai 2](https://huggingface.co/prism-ml/Ternary-Bonsai-2-27B-gguf),
which derives from [Alibaba's Qwen3.8-27B](https://huggingface.co/Qwen/Qwen3.8-27B).

This project provides a local creative assistant for **Windows ARM64 laptops
with Snapdragon X and X2 processors**. It currently runs on the CPU, with
[automatic hardware tuning and GPU/NPU support](docs/hardware-capabilities.md#automatic-profile-selection)
planned.

The native Rust engine lives in [Prism Inference](https://github.com/symbiogenesis/prism-inference).
This project consumes a pinned prebuilt server; ordinary builds do not compile
the inference engine. Native source launch prepares a missing pinned runtime after
the window opens. Explicit provisioning uses `bonsai-tools provision` with operation
`inference` and an absolute `dataDirectory`; see the [setup commands](docs/development-guide.md#native-chat-app).

Once you've [set up the model and runtime](docs/development-guide.md#native-chat-app), run `bonsai-tools chat`. The development default is Rust
Prism 0.1.19 with the original Swift PQ2 weights, four CPU row workers and the standard adapter at
scale 1.3, and default Snapdragon X2 Hexagon NPU `native-q2` resident acceleration
(`blk.0..=blk.26`, 168 projections across 6 arenas). Use `bonsai-tools chat --publisher-engine` for
the explicit publisher C++ rollback. `--native-engine` remains accepted. Both open the native
Windows chat app.

The convenience launcher `./scripts/Start-Chat.ps1` uses the same prebuilt Rust
tool and defaults to Rust; add `-PublisherEngine` for the publisher rollback.

> [!NOTE]
> **Rust is the development default.** Full-context/output, creative quality,
> sustained performance, daily-service isolation and clean-machine installation
> qualification remain pending. This choice makes no speed claim; PTQ1 remains
> an explicit experimental option. Startup failure never switches engines or weights.

Keep separate conversations and unsent drafts, rename your chats, and find text
with **Ctrl+F**. Enable **Save conversations on this device** in Settings to
recover your workspace after closing; saved conversations use Windows user
encryption. Saving is off by default.

Thinking is off by default. Enable **Thinking** under **Settings → Response**
for the next response, including every Extra effort pass. Each conversation
keeps its own choice; optional conversation saving restores it.

Settings also adjusts the pinned abliteration adapter's strength for the next
response. The default scale is **1.3**; **0** disables its correction. Its effect
and output quality on Swift remain experimental.

You can chat with Swift, hear answers read aloud in a voice you describe, or
dictate a message to review before sending. The microphone stays off until you
turn it on and grant Windows permission. **Read aloud as responses arrive** reads
new answers while Swift writes; each response's speaker button remains available
for independent replay.

The launcher downloads speech models automatically. The setup guide explains
how to skip those downloads or preview the app without loading a model.
For developer details, see the [native Rust UI architecture](docs/ui-architecture.md)
and [local API contract](docs/broker-policy.md#public-request-contract).

## At a glance

| | Current target |
| --- | --- |
| **Hardware** | Snapdragon X and X2 · Windows ARM64 |
| **Model** | Swift Bonsai 2 · `PQ2_0` |
| **CPU profile** | One active request · 8K-token context · FP16 attention caches |
| **Engine** | Rust Prism 0.1.19; four CPU row workers; Snapdragon X2 Hexagon NPU `native-q2` resident acceleration (`blk.0..=blk.26`, 168 projections across 6 arenas); adapter scale 1.3 |
| **Outstanding work** | [Central TODO](PLAN.md) |

## Performance

| Model | Runtime | Time to first token (s) | Generation tokens/second | Model file | Job commit cap / measured peak | Available RAM admission | Restore copy saved vs. Rust 0.1.4 |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| **Swift Bonsai 2 `PQ2_0` (development default; pinned at Rust 0.1.19)** | **Rust 0.1.14 release (4 workers) + LoRA 1.3** | **16.64** | **1.34** | **6.71 GiB** | **11.5 GiB / 7.20 GiB (short request)** | **15.5 GiB** | **149.625 MiB** |
| **Swift Bonsai 2 `PTQ1_0` (experimental opt-in; pinned at Rust 0.1.19)** | **Rust 0.1.14 release (4 workers) + LoRA 1.3** | **29.50** | **0.75** | **5.54 GiB** | **11.5 GiB / 6.02 GiB (short request)** | **15.5 GiB** | **149.625 MiB** |
| Swift Bonsai 2 `PQ2_0` | Rust 0.1.13 release (4 workers) + LoRA 1.3 | 16.45 | 1.32 | 6.71 GiB | 11.5 GiB / 7.20 GiB (short request) | 15.5 GiB | 149.625 MiB |
| Swift Bonsai 2 `PTQ1_0` | Rust 0.1.13 release (4 workers) + LoRA 1.3 | 28.69 | 0.75 | 5.54 GiB | 11.5 GiB / 6.02 GiB (short request) | 15.5 GiB | 149.625 MiB |
| Swift Bonsai 2 `PQ2_0` | Rust 0.1.10 release (4 workers) + LoRA 1.3 | 16.98 | 1.34 | 6.71 GiB | 11.5 GiB / 7.20 GiB (short request) | 15.5 GiB | 149.625 MiB |
| Swift Bonsai 2 `PQ2_0` | Rust 0.1.10 release (1 worker) + LoRA 1.3 | 35.59 | 0.64 | 6.71 GiB | 11.5 GiB / 7.20 GiB (short request) | 15.5 GiB | 149.625 MiB |
| Swift Bonsai 2 `PQ2_0` | Rust 0.1.9 release (4 workers) + LoRA 1.3 | 23.00 | 1.32 | 6.71 GiB | 11.5 GiB / 7.20 GiB (short request) | 15.5 GiB | 149.625 MiB |
| Swift Bonsai 2 `PTQ1_0` | PrismML C++ CPU | Startup failed | Startup failed | 5.54 GiB | 11.5 GiB / — | 15.5 GiB | — |
| Swift Bonsai 2 `PQ2_0` (explicit publisher rollback) | PrismML C++ CPU | Startup failed | Startup failed | 6.71 GiB | 11.5 GiB / — | 15.5 GiB | — |
| Qwen3.8-27B `UD-Q3_K_XL` | llama.cpp reference | Not installed | Not installed | 12.24 GiB | 18 GiB / — | 22 GiB | — |

The [2026-11-03 separated timing capture](docs/evidence/token-timing-20261103/summary.json)
measured seven installed Rust profiles on the Snapdragon X2 Elite Extreme.
The current release profiles use the default four CPU row workers; comparing the
`0.1.10` single-worker row (`35.59 s` TTFT, `0.64` generation tokens/s) against
the four-worker row (`16.98 s` TTFT, `1.34` generation tokens/s) shows a **2.10x
faster TTFT** and **2.09x faster generation rate** from four-worker row parallelism,
while the `0.1.10` startup/prefill changes reduced four-worker TTFT from `23.00 s`
(`0.1.9`) to `16.98 s` (`16.45–16.64 s` in `0.1.13`–`0.1.14`). Short-request peak Job
commit measured **7.20 GiB** (`7,726,850,048–7,727,276,032` bytes) across all five
`PQ2_0` profiles and **6.02 GiB** (`6,465,683,456–6,465,744,896` bytes) across both
`PTQ1_0` profiles (**1.17 GiB lower peak Job commit** on short requests).
Each fresh owner used the fixed prompt `Count from 1 to 100.`, thinking off,
seed 42 and the standard Swift adapter at scale 1.3. All seven measured responses
reported eight completion tokens and returned `1, 2, 3,` before hitting the cap.
Runner now pins **Rust Prism `0.1.19`** (`951cc88`), which adds the `0.1.15`–`0.1.19`
CPU NEON, multi-worker GDN/attention/conv/SwiGLU, `4x4`/`8x8` tiled-prefill GEMM,
and Snapdragon X2 6-arena 168-projection `native-q2` resident NPU optimizations;
an updated end-to-end HTTP timing capture on `0.1.19` remains pending.

### Snapdragon X2 Hexagon NPU (`v81`) projection and layer timings

In `Prism 0.1.19`, `prism-inference-server` pre-warms **`DEFAULT_NATIVE_Q2_PROJECTIONS = 168`**
(`blk.0..=blk.26`, **`2,888,663,040` resident weight bytes / `2,754.84 MiB`** across six alternating
`<= 480 MiB` `FastRPC` weight arenas
`[502_824_960, 460_062_720, 502_824_960, 460_062_720, 502_824_960, 460_062_720]` within CDSP
`v-size = 3,000 MiB`, with scoped `<= 92 KiB` active-prefix `dspqueue` cache flushes and batched
2/3-op sibling plus 4-op multi-token tiled-prefill submissions) by default on Snapdragon X2 when the
verified signed GenieX `v0.10.0` runtime (`libggml-htp-v81.so`) and CDSP driver are present, falling
back to 4-worker NEON CPU otherwise. Losslessly transcoding `PQ2_0` weights into tiled
`HTP_TYPE_Q2_0` reduces shared-memory output traffic per `[5120, 17408]` projection by **320x**
(`69,632` bytes vs. `22.28 MB` of 8-plane `FP32` bitplanes) and eliminates host bitplane
reconstruction.

The table below summarizes the supervised 3-token (`42, 43, 128`) Snapdragon X2 Elite
Extreme device controls (`LoRA 1.3`, 4 CPU row workers; see [Hexagon execution contracts](docs/hexagon.md)
for full receipts):

| Placement / backend | Projections (`PQ2_0`) | Resident weight payload (`FastRPC` mapped) | NPU submit & wait / token (ms) | Per-projection NPU time (ms) | Token evaluation: NPU vs. 4-worker NEON CPU (ms) | Numerical verification | Power condition |
| --- | --- | ---: | ---: | ---: | ---: | --- | --- |
| [**1-op `native-q2` resident (`ccb1339`)**](docs/evidence/npu-native-q2-1op-20261105/summary.json) | 1 (`blk.0.ffn_up`, `[5120, 17408]`) | 23.91 MiB (47.90 MiB) | **0.64–0.98** | **0.64–0.98** | 520.84–561.49 vs. 515.92–589.57 | Cosine `0.999908–0.999972`, RMSE `0.0221–0.0286`, top-1 `3/3`, top-5 `5/5` | AC (`99%`, saver off) |
| [**1-op `native-q2` resident (`ccb1339`)**](docs/evidence/npu-native-q2-1op-20261105/summary.json) | 2 (`blk.0.ffn_up + ffn_gate`) | 47.81 MiB (71.81 MiB) | **1.08–1.44** | **0.54–0.72** | **506.15**–567.14 vs. 516.49–591.33 | Cosine `0.999906–0.999974`, RMSE `0.0226–0.0284`, top-1 `3/3`, top-5 `5/5` | AC (`99%`, saver off) |
| [**Full recurrent layer `native-q2` (`a383e92`)**](docs/evidence/npu-full-layer-native-q2-a383e92-20261105/summary.json) | 6 (`blk.0` all recurrent projections) | 102.66 MiB (126.65 MiB) | **2.27–2.87** | **0.38–0.48** | 395.80–603.33 vs. 547.11–628.76 | Cosine `0.999893–0.999973`, RMSE `0.0249–0.0306`, top-1 `3/3`, top-5 `5/5` | Battery (`90%`, saver off; diagnostic) |
| [**2-layer `native-q2` unscoped flush (`a383e92`)**](docs/evidence/npu-full-layer-native-q2-a383e92-20261105/summary.json) | 13 (`blk.0` recurrent + `blk.3` full-attn) | 202.50 MiB (226.50 MiB) | **4.38–5.46** | **0.34–0.42** | 496.00–586.48 vs. 544.23–604.43 | Cosine `0.999902–0.999977`, RMSE `0.0228–0.0290`, top-1 `3/3`, top-5 `5/5` | Battery (`90%`, saver off; diagnostic) |
| [**2-layer `native-q2` scoped `<= 92 KiB` flush (`e2349af`)**](docs/evidence/npu-4layer-25proj-native-q2-20261105/summary.json) | 13 (`blk.0` recurrent + `blk.3` full-attn) | 202.50 MiB (226.50 MiB) | **4.43–4.87** | **0.34–0.37** | **422.49–446.20** (warm) vs. 463.97–478.07 (`1,386.00` vs. `1,392.70` total) | Cosine `0.999902–0.999977`, RMSE `0.0228–0.0290`, top-1 `3/3`, top-5 `5/5` | Battery (`76%`, saver off; diagnostic) |
| [**4-layer block `native-q2` scoped flush (`e2349af`)**](docs/evidence/npu-4layer-25proj-native-q2-20261105/summary.json) | 25 (`blk.0..=blk.3` complete repeating block) | 407.81 MiB (431.81 MiB) | **10.05–10.91** | **0.40–0.44** | 807.50–1,166.27 vs. 425.83–970.25 | Cosine `0.999895–0.999972`, RMSE `0.0217–0.0316`, top-1 `3/3`, top-5 `5/5` | Battery (`76%`, saver off; diagnostic) |
| [**8-HVX / 2 MiB VTCM / Burst `batch10` HMX (`9f61217`)**](docs/evidence/npu-hmx-8hvx-2mib-vtcm-20261105/summary.json) | 1 (`blk.0.ffn_up`, 40 ops in 4 writes/tok) | 23.91 MiB (35.24 MiB) | **8.18–13.28** | **8.18–13.28** | 320.31–536.98 vs. 294.61–342.29 | **0-ULP bitwise parity** (logits & state) | AC (`95%`, saver off) |
| [**8-HVX / 2 MiB VTCM / Burst `batch10` HMX (`9f61217`)**](docs/evidence/npu-hmx-8hvx-2mib-vtcm-20261105/summary.json) | 2 (`blk.0.ffn_up + ffn_gate`, 80 ops in 8 writes/tok) | 47.81 MiB (59.15 MiB) | **15.60–26.77** | **7.80–13.39** | **457.34–464.30** (warm) vs. 552.44–553.09 | **0-ULP bitwise parity** (logits & state) | AC (`95%`, saver off) |
| [**4-worker L1-tiled `batch10` HMX (`5136bd1`)**](docs/evidence/npu-batch10-fast-queue-20261105/summary.json) | 1–2 (`blk.0.ffn_up`, `+ ffn_gate`) | 23.91–47.81 MiB | 22.21–23.48 (1p) / 44.08–46.26 (2p) | 22.04–23.48 | 487.91–529.85 (1p) / 513.61–537.70 (2p) | **0-ULP bitwise parity** (logits & state) | Battery (`89%`, saver off; diagnostic) |
| [**Fused host-reconstruct `batch4` HMX (`ebc22c2`)**](docs/evidence/npu-multi-projection-20261104/summary.json) | 1–2 (`blk.0.ffn_up`, `+ ffn_gate`) | 23.91–47.81 MiB | 32.53–34.22 (1p) / 64.66–66.22 (2p) | 32.33–34.22 | 432.66–487.59 (1p) / 514.98–553.37 (2p) | **0-ULP bitwise parity** (logits & state) | AC (`100%`, saver off) |

Battery-operated diagnostic runs (`a383e92`, `e2349af`, `5136bd1`) are kept separate from
AC-connected runs (`ccb1339`, `9f61217`, `ebc22c2`) and are not treated as qualified
cross-power speed comparisons. Full-model 6-arena (`168`-projection) AC-connected
end-to-end service qualification remains pending in [PLAN.md](PLAN.md#hexagon-npu).

**TTFT** runs from client request start through the first output token, including
required exact token-count admission, template processing, prefill and HTTP
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
retried. The [earlier whole-request capture](docs/evidence/local-throughput-20261103/summary.json)
and [historical one-worker attempt](docs/evidence/native-request-speed.json) retain
their original clocks and broader timing intervals; their request rates are not
generation rates.

The [verified PTQ1 export](config/swift.ptq1-export.json) saves **1,259,520,000 bytes
(1.173 GiB)** on disk. The released service's full-context peak memory and paired benefit remain
unmeasured; the packing saving alone does not qualify resources or performance.

> [!NOTE]
> **PTQ1 is an experimental opt-in; qualification for the app default is pending.**
> Select the verified current-weight export with
> `bonsai-tools chat --native-engine --use-repacked-ptq1 --native-row-workers 4`
> after [provisioning it](docs/development-guide.md#runtime-selection).
> The [0.1.13 four-worker promotion score](docs/evidence/ptq1-promotion-bb020f4-service-90a62b9-review.json)
> retains **6 of 19 gates passed**, with 13 pending for frozen candidate `bb020f4`
> and separately reviewed service harness `90a62b9`.
>
> The [0.1.13 original-PQ2 rollback gate](docs/evidence/ptq1-rollback-41573164864/independent-review.json)
> passed with that released runtime, standard adapter, four-worker readiness,
> complete response, owned cleanup and verification of the retained original file.
> Both models returned `Oak` in the fixed-order comparison. Request times were
> **18.38 s for PQ2 and 35.73 s for PTQ1**; peak Job commit was **7.20 GiB and
> 6.02 GiB**, respectively. AC/battery status was not recorded, so these remain
> uncontrolled observations, with no performance or full-resource qualification.
>
> The [latest bounded four-worker service trial](docs/evidence/ptq1-service-41577882160/independent-review.json)
> passed three one-token-cap streams at scales 1.3, negative zero and 2.0, plus
> invalid-scale rejection. Its context-error check received no complete HTTP
> header and stopped before cancellation or fresh-owner replay. Parent-owned
> cleanup completed within **119.737 charged seconds**. AC was connected at the
> parent endpoints; the missing child after-power observation stays unknown.
> These partial checks do not qualify the full service gate, quality or throughput.
>
> The [published Prism 0.1.14 release](https://github.com/symbiogenesis/prism-inference/releases/tag/v0.1.14)
> includes the typed context-error fix. The [Runner pin preparation](docs/evidence/prism-0114-pin-preparation.json)
> and [publication review](docs/evidence/ptq1-context-publication-014/independent-review.json)
> bind its exact source and archive identities; owning Runner integration checks
> remain pending.
> The [combined service and original-PQ2 rollback profile](docs/validation.md#actual-model-application-acceptance)
> is prepared for one bounded allowance with adapter 1.3 and four workers.
> Actual 0.1.14 model/service/rollback acceptance remains pending. The earlier
> 0.1.13 measurements and gate passes retain their original identities.
>
> The [fresh hosted packing check](docs/evidence/ptq1-hosted-packing-b833d6b.json)
> independently matched all **209,920,000 blocks**, their original symbols and
> scales, and the preserved metadata and other tensor bytes. It reproduced the exact
> export with the pinned 0.1.13 exporter. Packing parity alone does not establish
> runtime numerical parity, memory use or speed.
>
> The [retained 0.1.13 source diagnostic](docs/evidence/ptq1-numerics-41568281287.json)
> matched all **39,504,384 logits and live-state values** byte for byte for one
> supplied token on X2 Elite Extreme, with explicit NEON, four row workers and
> adapter scale 1.3. Its **5.85 GiB peak Job commit** is specific to the correctness
> checker; it does not measure the released service's memory or throughput.
>
> Full-context/4K-output acceptance, scalar/explicit-NEON trajectories,
> loaded-zero identity, adapter authoring basis, paired complete-answer benefit,
> independent quality review and daily-service acceptance remain pending in
> [the plan](PLAN.md#actual-model-acceptance). PQ2 remains the app default and
> explicit rollback; the [release decision](docs/swift-memory.md#release-decision)
> records a **no-go for default promotion** under the current evidence and retains
> the historical results and failures. PTQ1 remains experimental opt-in.
>
> Admission values are configured budgets including a 4 GiB physical reserve.
> The restore-copy figure describes a transient allocation lifetime. Mapped weight
> residency requires separate accounting from Job commit, and Windows, other
> applications, caching and speech need additional headroom.

See the [Swift memory assessment](docs/swift-memory.md) and
[Qwen reference accounting](docs/model-selection.md#source-only-compatibility-and-memory-sanity-check).

## Performance expectations

### Response speed

Compact weights can reduce memory traffic, but response speed also depends on
the hardware, runtime and prompt length. **Swift's sustained Snapdragon decode
rate remains unmeasured; the table above records short-request observations.**
For context, these are Prism's published results for
the **base Bonsai 2 model**, using PQ2 weights on other hardware:

![Base Bonsai 2 PQ2 generation speed: Apple M5 Pro 28.1, RTX 4090 81.2 and RTX 5090 129.9 tokens per second.](docs/assets/reference-throughput.svg)

[Source: Prism throughput benchmarks](https://huggingface.co/prism-ml/Ternary-Bonsai-2-27B-gguf#cross-platform-throughput).
These are reference points, not a Swift or Snapdragon speed forecast. Thinking
time and long prompts also affect how soon a useful answer arrives.

### Answer quality

Prism reports **84.78** for base Bonsai 2 versus **86.32** for full-precision
Qwen3.8-27B across 14 thinking-mode benchmarks. The chart shows how that compares
with conventional compressed versions of the same Qwen base:

![Published benchmark averages: Qwen FP16 86.32, Qwen Q4 85.18, base Bonsai 2 84.78 and Qwen IQ2 72.59, on a zero-to-100 scale.](docs/assets/answer-quality.svg)

[Source: Prism benchmark results](https://huggingface.co/prism-ml/Ternary-Bonsai-2-27B-gguf#benchmarks).
These scores cover **base Bonsai, not Swift**, and do not measure creative-writing
quality. Swift's own publisher reports [uneven general-purpose behavior](https://ukisai.com/swift-bonsai-2#research-status-and-limitations).
Creative-writing quality and daily-service isolation still need qualification.
See [benchmark context and current limits](docs/performance-expectations.md).

## Documentation

The [documentation index](docs/README.md) maps every guide and technical contract.
Start with [setup and development](docs/development-guide.md),
[validation](docs/validation.md), or the [outstanding work](PLAN.md).
For design and research, use [application contracts](docs/README.md#application-contracts),
[models and runtime](docs/README.md#models-memory-and-runtime), and
[hardware and inference](docs/README.md#hardware-and-inference).

## License

Original project code, documentation, and configuration are licensed under the
[Apache License 2.0](LICENSE). Third-party code retains its own terms, including
the MIT-licensed Prism/ggml portions identified in [NOTICE](NOTICE).

Model weights and adapters are separate downloads governed by their publishers'
licenses. See the [license decision and dependency review](docs/licensing.md)
for scope, attribution, and redistribution requirements.
