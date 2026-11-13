# Retained evidence

Keep inputs and records required by the consumers below under the
[retention policy](../validation.md#evidence-retention).

| Retained material | Current consumer |
| --- | --- |
| `cpp-retirement-model-free.json` | [TTS decode-graph fixture provenance](../../src/speech-tts/tests/fixtures/decode-graph-v1.provenance.json) |
| `hexagon-public-qaic-41171859264/` | [Signed-Q2 source lock](../../config/hexagon-signed-q2-candidate.lock.json), public-wire review and [Windows ABI replay inputs](../../tools/hexagon/windows_queue_abi.ps1) |
| `hexagon-public-qaic-41169481882/provenance.json` | Supplemental template notices referenced by the current [QAIC provenance](hexagon-public-qaic-41171859264/provenance.json) |
| Hexagon operation, traffic, session-budget and K-tiling ledgers | [Static policy verifiers](../../tools/hexagon/README.md) |
| Hexagon signed-package, projection-binding, FD/session and advance-control records | [Experiment lock](../../config/hexagon-experiment.lock.json), [signed-Q2 lock](../../config/hexagon-signed-q2-candidate.lock.json) and their archive manifest |
| `hexagon-adapter-session-20261103/model-device-request.json` | Request template consumed by [model-control preparation](../../tools/hexagon/prepare_model_control.ps1) |

Preserve required replay bytes, notices and integrity records. Supplemental
QAIC provenance supplies template notices, not missing payloads or an independent
replay. An observation qualifies only its recorded source, tools and workload.
