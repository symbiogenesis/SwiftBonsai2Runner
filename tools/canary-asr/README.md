# Native Qwen3-ASR speech recognition

The app uses the selected **Qwen3-ASR 1.7B** model through the
[native Rust companion](../../src/speech-asr/README.md) and the CPU-only
transcribe.cpp engine.
The first-party UI remains Rust and native; inference includes a reviewed C++
GGML core. This is not a claim of pure-Rust tensor inference.

The [speech setup and operation guide](../../docs/development-guide.md#speech-setup-and-operation)
owns app provisioning, microphone permission/capture, prompt submission,
ASR/TTS exclusion, service lifetime and resource limits.

## Artifacts

The native [model lock](../../src/speech-asr/model.lock.json) pins the conversion,
source model revision, exact size and SHA-256. Qwen3-ASR and this conversion are
Apache-2.0 licensed; retain attribution and the model license. The legacy
`model.lock.json` and `tokenizer.lock.json` here describe original reference
snapshots for the explicit Rust `provision` operation `speech-reference`; the app does not load
them, and existing downloaded snapshots are preserved. The `canary` directory,
`--canary-key-file`/port options, `Canary` service kind and `CanaryModelPath`
output name remain compatibility names; they select Qwen3-ASR, not Canary.

## Developer-managed service

Ordinary app launches prepare and own this service automatically. The following
interface is for developers integrating a separately managed companion. Run the
built executable from an external working directory with absolute verified model
and protected key-file paths:

```text
bonsai-speech-asr.exe --model C:\Speech\qwen3-asr.gguf --token-file C:\Speech\asr.key --port 8766
```

Then launch with `bonsai-tools chat --canary-key-file C:\Speech\asr.key` as described
in the setup guide. A key contains exactly 64 lowercase hex bytes without a newline,
protected for the owner and LocalSystem.
Never pass its value through arguments, environment variables or logs.

## HTTP contract

The service binds exclusively to `127.0.0.1` after loading and retaining verified
model files. All requests require exact `Host: 127.0.0.1:<port>`, bearer auth,
HTTP/1.1 and `Connection: close`. Authenticated `GET /health` reports model,
ready, `sample_rate:16000`, `max_clip_seconds:30`, `channels:1` and
`input_format:"pcm_s16le_wav"`.

`POST /v1/audio/transcriptions` accepts bounded multipart fields `model` (exact
`Qwen/Qwen3-ASR-1.7B`), `file` (mono 16 kHz signed PCM16 RIFF WAV), and optional
`language:"Auto"`. Omission also selects automatic language detection; explicit
language hints are unsupported. The complete body is at most 1 MiB; clips are
at most 30 seconds.
A successful response is `{"text":"...","model":"Qwen/Qwen3-ASR-1.7B"}`.
Unknown or duplicate fields, aliases, Origin/forwarding headers, chunked/encoded
uploads and connection reuse are rejected. Errors contain only bounded codes.

The [shared Rust service](../../src/speech-service/src/lib.rs) implements HTTP,
authentication, validation, PCM handling and deadlines. One request runs at a
time. Cancellation, disconnect or the 120-second inference deadline terminates
only the separately owned speech process. Model input and audio remain in memory
and are not logged or persisted. No cloud fallback is configured.

## Qualification

Qwen3-ASR native loading, actual-microphone transcription, quality, lifecycle and
resource qualification remain pending. Earlier Canary build/fixture results do
not qualify this replacement. GPU/NPU acceleration and performance remain
unqualified. Full correctness suites and every benchmark run only in CI.
