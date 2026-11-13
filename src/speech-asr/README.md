# Native Qwen3-ASR recognition

This Rust library runs the selected **Qwen3-ASR 1.7B** checkpoint
through the CPU-only `transcribe-cpp` 0.4.1 binding. Its packaged native sources
come from `handy-computer/transcribe.cpp` commit
`4d7c1ccefd39e5a9ea36cca5f8fefc6dcc14b99b`.

Before `Recognizer::load`, the service must verify and retain the Q8_0 file in
`model.lock.json`. The library maps it, verifies architecture `qwen3_asr`, variant
`qwen3-asr-1.7b` and audio capabilities, then reuses one four-thread session with
an 8,192-token decoder context. It accepts up to thirty seconds of normalized
mono 16 kHz audio, detects language automatically and returns complete text.
Cancellation discards partial text. The backend checks cancellation before
inference; owned process teardown, deadlines and memory limits bound active
inference because it does not check between decoder steps.

Windows ARM64 native builds require clang-cl and Ninja, the ARM64 MSVC libraries,
and Windows SDK. ggml's ARM backend does not support MSVC cl.exe. Build with CPU
features only, `GGML_NATIVE=OFF`, and OpenMP disabled; the launcher/build tooling
owns that explicit child environment. Upstream build scripts were reviewed:
they compile/install vendored C++ sources and create a verified short build
junction outside Git, without downloading or executing speech models.

Host-bound source builds explicitly enable DOTPROD and I8MM independently when
Windows reports support, retaining ARMv8-A for unavailable observations. The
selected architecture partitions the native build cache. Portable ASR candidates
keep ARMv8-A regardless of their build host. This selects existing quantized CPU
kernels; actual-model transcript parity and latency still require qualification.

The original Qwen model and converted GGUF are **Apache-2.0** licensed; their
immutable source and conversion revisions are recorded in the lock.
The selected Q8_0 file is 2,185,030,624 bytes. Its revision, size and SHA-256
come from the converter publisher's public Hugging Face file metadata.
This is the transcribe.cpp conversion, not a llama.cpp multimodal split model.
transcribe.cpp and its Rust bindings are **MIT** licensed.

Native device qualification of loading, transcription, microphone lifecycle,
quality, accuracy, performance and resources remains pending. Historical Canary
fixtures do not qualify Qwen3-ASR. Model-free regressions cover family/variant/audio
identity rejection, bounded input and cancellation; compilation and protocol checks
do not establish model qualification.
