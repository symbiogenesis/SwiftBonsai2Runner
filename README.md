# Swift Bonsai 2 Runner

A local creative-writing and brainstorming app for Windows ARM64 laptops with
Snapdragon X and X2 processors. It uses a native Rust UI and
[Prism Inference](https://github.com/symbiogenesis/prism-inference) on CPU, with
experimental Hexagon acceleration on supported X2 installations.

[Swift Bonsai 2](https://ukisai.com/swift-bonsai-2) is UkisAI's model based on
[PrismML's Bonsai 2](https://huggingface.co/prism-ml/Ternary-Bonsai-2-27B-gguf)
and [Qwen3.8-27B](https://huggingface.co/Qwen/Qwen3.8-27B).
Prompts and conversations stay on your device.

**Unreleased.** Build a portable executable with the
[development guide](docs/development-guide.md#build-a-single-executable).
Clean-machine setup, actual-model behavior, quality, performance and isolation
still need [acceptance testing](PLAN.md).

## Features

- Separate conversations and drafts, local search, and encrypted workspace
  storage that can be disabled in Settings.
- Optional Thinking, Extra effort planning and revision, and adjustable style
  adapter strength.
- Local dictation, audio-file transcription, and read-aloud with a voice
  described in plain words. Microphone capture requires explicit permission.
- In-app model/runtime setup, progress, pause and retry. Text chat can work while
  speech finishes preparing.
- Verified portable updates with immediate or deferred installation.

Swift weights are a 6.71 GiB download. Startup selects context and its memory
budget from available RAM, reserving 4 GiB for Windows and other apps. One reply
runs at a time. Setup and startup failures report errors without silently
changing engines or model files.

## Documentation

- [Application controls and contributor builds](docs/development-guide.md)
- [Portable distribution, setup and updates](docs/releases.md)
- [Technical contracts and tooling](docs/README.md)
- [Validation policy](docs/validation.md) and [outstanding work](PLAN.md)

## License

Original code, documentation and configuration use [Apache 2.0](LICENSE).
Third-party code retains its own terms; see [NOTICE](NOTICE).
Model weights and adapters are separate downloads with publisher licenses.
The [licensing guide](docs/licensing.md) covers redistribution and notices.
