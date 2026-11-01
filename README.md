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
the inference engine. Provision it with `bonsai-tools provision --json` using operation `inference` and an absolute `dataDirectory`; see the [setup commands](docs/development-guide.md#native-chat-app).

Once you've [set up the model and runtime](docs/development-guide.md#native-chat-app),
run `bonsai-tools chat --native-engine` to select the Rust engine, or
`bonsai-tools chat` for the publisher C++ default. Updating the Rust runtime
pin alone does not change that selection. Both open the native Windows chat app.

The convenience launcher `./scripts/Start-Chat.ps1` uses the same prebuilt Rust
tool; add `-NativeEngine` to select the Rust inference engine.

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
| **Outstanding work** | [Central TODO](PLAN.md) |

## Performance

| Model | Runtime | Tokens/second | Model file | Job commit cap / measured peak | Available RAM admission | Restore copy saved vs. Rust 0.1.4 |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| **Swift Bonsai 2 `PTQ1_0` (experimental opt-in)** | **Rust 0.1.12 release + LoRA 1.3** | **Not measured** | **5.54 GiB** | **11.5 GiB / not recorded** | **15.5 GiB** | **149.625 MiB** |
| Swift Bonsai 2 `PQ2_0` (experimental Rust) | Rust 0.1.10 prerelease, one worker + LoRA 1.3 | 0.105 (truncated request) | 6.71 GiB | 11.5 GiB / not recorded | 15.5 GiB | 149.625 MiB |
| Swift Bonsai 2 `PQ2_0` (experimental Rust) | Rust 0.1.10 prerelease, four workers + LoRA 1.3 | Pending (deadline) | 6.71 GiB | 11.5 GiB / not recorded | 15.5 GiB | 149.625 MiB |
| Swift Bonsai 2 `PQ2_0` (captured source diagnostic) | Rust 0.1.9, four workers + LoRA 1.3 | Not measured | 6.71 GiB | 11.5 GiB / 7.04 GiB | 15.5 GiB | 149.625 MiB |
| Swift Bonsai 2 `PTQ1_0` | PrismML C++ CPU | Not measured | 5.54 GiB | 11.5 GiB / — | 15.5 GiB | — |
| Swift Bonsai 2 `PQ2_0` (current default) | PrismML C++ CPU | Not measured | 6.71 GiB | 11.5 GiB / — | 15.5 GiB | — |
| Qwen3.8-27B `UD-Q3_K_XL` | llama.cpp reference | Not measured | 12.24 GiB | 18 GiB / — | 22 GiB | — |

The [bounded X2 Elite Extreme attempt](docs/evidence/native-request-speed.json)
measured eight reported output tokens in 76.21 s: **0.105 request tokens/second**,
including prefill and HTTP delivery through connection closure, excluding startup.
The eight-token cap truncated the answer (`finish_reason: length`); this measures
a short request, not decode TPS or a complete answer. The four-worker request
reached the shared soft deadline; its rate and the paired speedup remain pending.
Sustained decode TPS and PTQ1 speed remain unmeasured.

The [verified PTQ1 export](config/swift.ptq1-export.json) saves **1,259,520,000 bytes
(1.173 GiB)** on disk. Current-release PTQ1 peak memory and device benefit remain
unmeasured; the packing saving alone does not qualify resources or performance.

> [!NOTE]
> **PTQ1 is an experimental opt-in; qualification for the app default is pending.**
> Select the verified current-weight export with
> `bonsai-tools chat --native-engine --use-repacked-ptq1` after
> [provisioning it](docs/development-guide.md#runtime-selection).
> The bold row names the pinned Rust 0.1.12 profile; historical measurements do not
> transfer to it. The [0.1.7 source diagnostic](docs/evidence/ptq1-rust-smoke.json)
> measured **5.85 GiB peak Job commit** for one supplied token at adapter scale 1.3:
> all **129 files / 39,504,384 logits and live-state values** matched the native
> reference byte for byte with Auto requested. The separate [0.1.8 sandboxed service
> smoke](docs/evidence/ptq1-native-service.json) passed startup, one short scale-1.3
> stream and cleanup, without a memory peak. Broader PTQ1 service acceptance,
> scalar/explicit NEON and zero-scale controls, full-context/4K-output, adapter basis,
> quality and benefit remain unqualified. PQ2 remains the app default; the
> [release decision](docs/swift-memory.md#release-decision) retains earlier failures.
>
> The [current 0.1.10 PTQ1 attempt](docs/evidence/ptq1-0110-service-attempt.json)
> reached verified readiness, but its initial scale-1.3 request did not complete
> within the request allowance. Intake, cleanup and receipt retention took
> **168.70 s**; that duration is not a request-speed measurement. Partial response
> progress is unknown, and zero-scale/rejection/history checks remain pending.
>
> The later [0.1.10 PQ2 lifecycle check](docs/evidence/prism-0110-lifecycle-service.json)
> passed readiness, scales 1.3/zero/1.3, invalid-scale rejection, static supplied
> history and owned cleanup in **124.781 s** from artifact intake through cleanup.
> Its three streams reported **one, one and two output tokens** with four configured
> workers. This bounded lifecycle pass does not qualify PTQ1, the separate one-worker
> policy, active graph cancellation, memory peaks or performance.
>
> The [original-PQ2 row-worker diagnostic](docs/evidence/row-parallel-smoke.json)
> matches all 39,504,384 logits/state values byte for byte. One fixed forward
> evaluation took 1.55 s with one worker and 0.58 s with four; loading and capture
> I/O are excluded. This is not application TPS or full performance qualification.
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
rate remains unmeasured; the table above records one short-request rate.**
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

---

## License

Original project code, documentation, and configuration are licensed under the
[Apache License 2.0](LICENSE). Third-party code retains its own terms, including
the MIT-licensed Prism/ggml portions identified in [NOTICE](NOTICE).

Model weights and adapters are separate downloads governed by their publishers'
licenses. See the [license decision and dependency review](docs/licensing.md)
for scope, attribution, and redistribution requirements.
