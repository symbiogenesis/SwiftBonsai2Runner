# Native Qwen3-ASR recognition

This Rust library runs the selected **Qwen3-ASR 1.7B** checkpoint
through the CPU-only `transcribe-cpp` 0.4.1 binding. Its packaged native sources
come from `handy-computer/transcribe.cpp` commit
`4d7c1ccefd39e5a9ea36cca5f8fefc6dcc14b99b`.

The service must verify and retain the exact Q8_0 file in `model.lock.json`
before calling `Recognizer::load`. The library maps that immutable file,
verifies `qwen3_asr` architecture, `qwen3-asr-1.7b` variant and audio capabilities,
and reuses one
four-thread session with an 8,192-token decoder context. It accepts normalized
mono 16 kHz samples for at most thirty seconds and returns complete text only.
Language is detected by the model rather than forcing English. Request
cancellation discards partial text. This pinned Qwen3-ASR backend observes the
native cancellation token before inference, not between decoder steps. The
service's owned process teardown therefore bounds cancellation during active
inference, along with its process deadline and memory limits.

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
transcribe.cpp and its Rust bindings are **MIT** licensed. Recognition
quality, accuracy and performance still require explicit native device
qualification; compilation and protocol tests alone do not establish them.

Qwen3-ASR actual-model loading, transcription, microphone lifecycle, accuracy and
resource qualification remain pending. Historical Canary fixture results do not
qualify this model. Focused model-free regressions cover family/variant/audio
identity rejection, bounded input and cancellation; they establish no model
quality or performance result.
