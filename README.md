<div align="center">

# 🌱 Swift Bonsai 2 - Runner

**A local runner for the Swift Bonsai 2 model on Windows ARM64.**

Creative writing · Brainstorming · Art direction

![Platform: Windows ARM64](https://img.shields.io/badge/Windows-ARM64-0078D4?style=flat-square)
![UI: native Rust](https://img.shields.io/badge/UI-native_Rust-50855B?style=flat-square)
[![License: Apache 2.0](https://img.shields.io/badge/license-Apache_2.0-50855B?style=flat-square)](LICENSE)

[Get started](docs/releases.md#download-and-open) · [Documentation](#documentation) · [TODO](PLAN.md)

</div>

---

Swift Bonsai 2 is a compact language model by [UkisAI](https://ukisai.com/swift-bonsai-2),
built on [PrismML's Bonsai 2](https://huggingface.co/prism-ml/Ternary-Bonsai-2-27B-gguf)
and [Alibaba's Qwen3.8-27B](https://huggingface.co/Qwen/Qwen3.8-27B).

This creative-writing and brainstorming app runs locally on **Windows ARM64
laptops with Snapdragon X and X2 processors**. Prompts and conversations stay on
your device.

The app downloads the prebuilt [Prism Inference](https://github.com/symbiogenesis/prism-inference)
Rust engine automatically. It runs on the CPU, with experimental Hexagon NPU
acceleration on supported Snapdragon X2 installations.
[Broader hardware tuning and GPU/NPU support](docs/hardware-capabilities.md#automatic-profile-selection)
are in progress.

Download the portable Windows ARM64 executable from
[GitHub releases](https://github.com/symbiogenesis/SwiftBonsai2Runner/releases),
then double-click it. The app prepares its models and engines inside the window;
there are no PowerShell setup commands. See [first launch](docs/releases.md#first-launch-and-in-app-setup).

The [Windows release guide](docs/releases.md) covers the portable executable,
setup and updates.
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
- **Portable updates:** The app downloads and verifies newer Windows ARM64 releases.
  Update behavior depends on the installed version; see
  [Windows releases](docs/releases.md#update-and-remove). Model downloads and
  conversations use a separate data folder.
- **Thinking and Extra effort:** **Thinking** is off by default; enable it under
  **Settings → Response** for reasoning before answers. **Extra effort** plans,
  drafts and revises a reply. Each conversation remembers its choice.
- **Response tuning:** Set the style adapter's strength for the next reply in
  Settings: **1.3** by default, **0** to disable. Its effect on Swift is experimental.
- **Voice and dictation:** Chat by typing, hear answers read aloud in a voice you
  describe in plain words, or record a message with Qwen3-ASR 1.7B. Finish recording
  to transcribe and send it to Swift. The microphone stays off until you turn it
  on and grant Windows permission. **Always Read Aloud** speaks new replies while
  Swift writes, and each reply's speaker button lets you replay it anytime.

The app prepares the pinned Swift model, adapter, inference runtime and speech
models automatically, with progress and in-app retry. Text chat can work while
speech finishes preparing. The [application guide](docs/development-guide.md#native-chat-app)
explains its controls; contributor builds and model-free previews have separate
instructions. See the
[UI architecture](docs/ui-architecture.md) and
[local API](docs/broker-policy.md#public-request-contract) for technical details.

## At a glance

| | Current target |
| --- | --- |
| **Hardware** | Windows ARM64 laptops with Snapdragon X and X2 processors |
| **Model** | Swift Bonsai 2 (6.71 GiB download) |
| **Memory needed** | Selected at model startup from available RAM, with 4 GiB reserved for Windows and other apps |
| **Conversation size** | One active reply at a time · context selected from memory and shared by history, prompt, reply and template reserve |
| **Engine** | [Prism Inference](https://github.com/symbiogenesis/prism-inference) (Rust) on CPU, with experimental X2 Hexagon acceleration |
| **Roadmap** | [Central TODO](PLAN.md) |

## Performance expectations

The current runtime still needs matched device measurements and creative-writing
review. Speed depends on hardware, prompt length, Thinking and available memory;
no sustained-speed or full-context quality claim is made. Older runtime timings
and base-model scores do not qualify this app. See the
[measurement requirements](docs/performance-expectations.md) and
[pending acceptance work](PLAN.md).

## Documentation

Start with [download and setup](docs/releases.md), [validation](docs/validation.md)
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
