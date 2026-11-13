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

Swift Bonsai 2 is a compact language model by [UkisAI](https://ukisai.com/swift-bonsai-2),
built on [PrismML's Bonsai 2](https://huggingface.co/prism-ml/Ternary-Bonsai-2-27B-gguf)
and [Alibaba's Qwen3.8-27B](https://huggingface.co/Qwen/Qwen3.8-27B).

This creative-writing and brainstorming app runs locally on **Windows ARM64
laptops with Snapdragon X and X2 processors**. Prompts and conversations stay on
your device.

The app downloads the prebuilt [Prism Inference](https://github.com/symbiogenesis/prism-inference)
Rust engine automatically. It runs on the CPU and can use the Hexagon NPU on
Snapdragon X2 with its driver installed.
[Broader hardware tuning and GPU/NPU support](docs/hardware-capabilities.md#automatic-profile-selection)
are in progress.

After [setup](docs/development-guide.md#native-chat-app), launch with
`bonsai-tools chat` or `./scripts/Start-Chat.ps1`. Add `--publisher-engine`
(or `-PublisherEngine` with the script) to use the original publisher C++ engine.

The [Windows release guide](docs/releases.md) covers the portable executable and
the prepared [v0.3.0 experimental release](docs/releases/v0.3.0.md). Publication
requires the release workflow to finish.
[Build locally](docs/development-guide.md#build-a-single-executable)
to create the same single-file distribution containing all five application tools.

> [!NOTE]
> **Work in progress.** Rust is the development default. Long conversations,
> writing quality, sustained speed, isolation and clean-machine setup still need
> acceptance testing. Startup failures stop the app with an error; they never
> silently switch engines or model files.

## Features

- **Conversations and search:** Keep separate conversations and unsent drafts, rename
  your chats, and find text with **Ctrl+F**. Your workspace is saved automatically
  and encrypted for your Windows user account. Disable **Save conversations on this
  device** in Settings to delete the saved copy and keep future chats unsaved;
  that choice survives restarts.
- **Portable updates:** Download and verify the newer Windows ARM64 executable,
  close Swift, then replace your copy. Keep the same data directory to preserve
  model downloads and conversations; see [Windows releases](docs/releases.md).
- **Reviewed source excerpts:** Use **Sources** in the composer to search pasted
  text locally, select exact passages and review them in your draft before sending.
  Each excerpt records its source title, version, line range and complete-source hash.
- **Thinking and Extra effort:** **Thinking** is off by default; enable it under
  **Settings → Response** for reasoning before answers. **Extra effort** plans,
  drafts and revises a reply. Each conversation remembers its choice.
- **Response tuning:** Set the style adapter's strength for the next reply in
  Settings: **1.3** by default, **0** to disable. Its effect on Swift is experimental.
- **Voice and dictation:** Chat by typing, hear answers read aloud in a voice you
  describe in plain words, or record a message with Qwen3-ASR 1.7B. Finish recording
  to transcribe and send it to Swift. The microphone stays off until you turn it
  on and grant Windows permission. **Read
  aloud as responses arrive** speaks new replies while Swift writes, and each
  reply's speaker button lets you replay it anytime.

The launcher prepares the pinned Swift model, adapter, runtime and speech models
automatically, with progress and in-app retry. The [setup guide](docs/development-guide.md)
explains how to skip them or preview the app without a model. See the
[UI architecture](docs/ui-architecture.md) and
[local API](docs/broker-policy.md#public-request-contract) for technical details.

## At a glance

| | Current target |
| --- | --- |
| **Hardware** | Windows ARM64 laptops with Snapdragon X and X2 processors |
| **Model** | Swift Bonsai 2 (6.71 GiB download) |
| **Memory needed** | 15.5 GiB of free RAM (keeps 4 GiB reserved for Windows and other apps) |
| **Conversation size** | One active reply at a time · 8,192 tokens shared by history, prompt, reply and template reserve |
| **Engine** | [Prism Inference](https://github.com/symbiogenesis/prism-inference) (Rust) on CPU, with Hexagon NPU acceleration on Snapdragon X2 |
| **Roadmap** | [Central TODO](PLAN.md) |

## Performance

The latest [recorded measurements](docs/evidence/readme-performance-0123-20261109/summary.json)
are from **November 9, 2026**, using **Prism 0.1.23**, four CPU workers and the
standard adapter at strength 1.3 on a **Snapdragon X2 Elite Extreme** laptop.
AC power was connected and battery saver was off at every recorded power sample.

Runner's development build uses published [Prism 0.1.25](https://github.com/symbiogenesis/prism-inference/releases/tag/v0.1.25)
with verified files, K4/V4 cache and explicit FP16 recovery. Its performance
measurements remain [pending](PLAN.md); the timings below apply to **0.1.23**.

| Model option | Time to first token | Generation speed | Model file size | Peak Job commit (short reply) | Free RAM required |
| --- | ---: | ---: | ---: | ---: | ---: |
| **Standard model (PQ2, default)** | **8.23 s** | **2.66 tokens/s** | **6.71 GiB** | **7.07 GiB** | **15.5 GiB** |
| **Extra-compact model (PTQ1, experimental)** | **15.68 s** | **1.41 tokens/s** | **5.54 GiB** | **5.89 GiB** | **15.5 GiB** |

Each option was measured once with `Count from 1 to 100.`, Thinking off and an
eight-token reply limit through the local API. First-token timing starts after
HTTP readiness and can include remaining model/device preparation; generation
speed excludes that initial wait. These short-reply observations do not establish
sustained speed. The default acceleration policy was used, but actual NPU usage
was not verified. Peak Job commit excludes mapped-file residency and NPU
allocations; it is not total RAM use.

The optional `--use-repacked-ptq1` experiment saves about **1.17 GiB** on disk
without changing weights. In these short requests it also reduced peak Job commit
by about **1.17 GiB**, but generated tokens more slowly.

For detailed benchmarks, NPU layer timings, and memory breakdowns, see:

- [Performance measurements and speech benchmarks](docs/performance-expectations.md)
- [Snapdragon Hexagon NPU benchmarks](docs/hexagon.md#snapdragon-x2-hexagon-npu-v81-projection-and-layer-timings)
- [Model memory and compact storage details](docs/swift-memory.md)

## Performance expectations

### Response speed

Reply speed depends on processor, engine and prompt length. **Snapdragon
measurements cover short replies; sustained long-conversation speed is still
being tested.** For context, PrismML publishes these speeds for **base Bonsai 2**
on other hardware:

![Base Bonsai 2 PQ2 generation speed: Apple M5 Pro 28.1, RTX 4090 81.2 and RTX 5090 129.9 tokens per second.](docs/assets/reference-throughput.svg)

[Source: Prism throughput benchmarks](https://huggingface.co/prism-ml/Ternary-Bonsai-2-27B-gguf#cross-platform-throughput).
The published figures were checked again on **November 9, 2026** and are unchanged.
These are reference points from other machines, not a speed forecast for Swift on
Snapdragon laptops. Turning on **Thinking** or sending long prompts also increases the
wait before an answer finishes.

### Answer quality

PrismML reports an average score of **84.78** for base Bonsai 2 versus **86.32** for
the full-size Qwen3.8-27B model across 14 reasoning benchmarks. The chart shows how
that compares with other compressed versions of the same Qwen model:

![Published benchmark averages: Qwen FP16 86.32, Qwen Q4 85.18, base Bonsai 2 84.78 and Qwen IQ2 72.59, on a zero-to-100 scale.](docs/assets/answer-quality.svg)

[Source: Prism benchmark results](https://huggingface.co/prism-ml/Ternary-Bonsai-2-27B-gguf#benchmarks).
The published scores were checked again on **November 9, 2026** and are unchanged.
These scores cover **base Bonsai, not Swift**, and do not measure creative writing.
Swift's publisher notes [uneven general-purpose behavior](https://ukisai.com/swift-bonsai-2#research-status-and-limitations),
and we are still evaluating its creative-writing quality for everyday use. See
[benchmark context and current limits](docs/performance-expectations.md).

## Documentation

Start with [setup](docs/development-guide.md), [validation](docs/validation.md)
or the [project TODO](PLAN.md). The [documentation index](docs/README.md) covers
[application design](docs/README.md#application-contracts),
[models and runtime](docs/README.md#models-memory-and-runtime), and
[hardware and inference](docs/README.md#hardware-and-inference).

## License

Original project code, documentation, and configuration are licensed under the
[Apache License 2.0](LICENSE). Third-party code keeps its own license terms,
including the MIT-licensed Prism and ggml components listed in [NOTICE](NOTICE).

Model weights and adapters are separate downloads governed by their publishers'
licenses. See the [licensing guide](docs/licensing.md) for details.
