# Retained evidence

Keep inputs and records required by the consumers below under the
[retention policy](../validation.md#evidence-retention).

| Retained material | Current consumer |
| --- | --- |
| `headless-publisher-startup.json` | [Headless candidate](../runtime-candidates/README.md) and its [frozen fixture](../../tools/bonsai/tests/fixtures/README.md) |
| `cpp-retirement-model-free.json` | [TTS decode-graph fixture provenance](../../src/speech-tts/tests/fixtures/decode-graph-v1.provenance.json) |
| `hexagon-public-qaic-*` and `hexagon-sdk-*` | [Signed-Q2 source lock](../../config/hexagon-signed-q2-candidate.lock.json), independent SDK/wire review and [Windows ABI replay inputs](../../tools/hexagon/windows_queue_abi.ps1) |
| Hexagon operation, traffic, session-budget and K-tiling ledgers | [Static policy verifiers](../../tools/hexagon/README.md) |
| Hexagon signed-package, projection-binding, FD/session and advance-control records | [Experiment lock](../../config/hexagon-experiment.lock.json), [signed-Q2 lock](../../config/hexagon-signed-q2-candidate.lock.json) and their archive manifest |
| `hexagon-adapter-session-20261103/model-device-request.json` | Request template consumed by [model-control preparation](../../tools/hexagon/prepare_model_control.ps1) |

`hexagon-public-qaic-41171859264/` contains the raw QAIC wire/ABI replay selection.
Other QAIC/SDK directories supply provenance referenced by the signed-Q2 lock;
their summaries cannot replace missing payloads. Preserve required replay bytes,
notices and integrity records. An observation qualifies only its recorded source,
tools and workload.
