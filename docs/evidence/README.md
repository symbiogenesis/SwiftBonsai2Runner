# Retained evidence

This directory contains inputs and records with current consumers. Completed
work, superseded release measurements and trial diaries belong in Git history.
See the [retention policy](../validation.md#evidence-retention) before adding or
removing evidence.

| Retained material | Current consumer |
| --- | --- |
| `windows-release-030-20261113/` | [Published release identities and verification limits](../releases.md) |
| `cpp-retirement-model-free.json` | [TTS decode-graph fixture provenance](../../src/speech-tts/tests/fixtures/decode-graph-v1.provenance.json) |
| `hexagon-public-qaic-41171859264/` | [Signed-Q2 source lock](../../config/hexagon-signed-q2-candidate.lock.json), public-wire review and [Windows ABI replay inputs](../../tools/hexagon/windows_queue_abi.ps1) |
| `hexagon-public-qaic-41169481882/provenance.json` | Supplemental template notices referenced by the current [QAIC provenance](hexagon-public-qaic-41171859264/provenance.json) |
| Hexagon operation, traffic, session-budget and K-tiling ledgers | [Static policy verifiers](../../tools/hexagon/README.md) |
| Hexagon signed-package, projection-binding, FD/session and advance-control records | [Experiment lock](../../config/hexagon-experiment.lock.json), [signed-Q2 lock](../../config/hexagon-signed-q2-candidate.lock.json) and their archive manifest |
| `hexagon-adapter-session-20261103/model-device-request.json` | Request template consumed by [model-control preparation](../../tools/hexagon/prepare_model_control.ps1) |

The current raw QAIC wire/ABI selection is `hexagon-public-qaic-41171859264/`.
The older QAIC provenance remains solely for its supplemental notices; its listed
historical payloads are absent from this checkout. It cannot supply independent
replay or close a gate that requires those missing bytes.

Original observations retain their source, tool names, results and limitations.
They do not qualify a newer runtime or authorize reopening a closed trial. The
current raw replay selection, license notices and integrity records remain
byte-for-byte; historical tool names inside them are provenance, not current
dependencies.
