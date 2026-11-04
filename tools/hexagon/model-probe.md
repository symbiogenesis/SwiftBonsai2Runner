# Local actual-model Hexagon probe

`bonsai-tools hexagon-tools --request ABSOLUTE_JSON_PATH` accepts the separate
`model-device` operation below. This local diagnostic evaluates one or two explicit
original-PQ2 tokens with `blk.0.ffn_up.weight` on the selected guarded HVX or HMX
bitplane path, compares every returned integer limb or plane and the complete
graph logits/state against its CPU shadow after each token, and requires explicit
native cleanup. Other projections stay on CPU. It enables no
production backend and establishes no inference speedup or adapter qualification.

```json
{
  "operation": "model-device",
  "request": {
    "repositoryDirectory": "C:\\source\\SwiftBonsai2Runner",
    "prismRepositoryDirectory": "C:\\source\\prism-inference",
    "expectedPrismRevision": "EXACT_LOWERCASE_40_HEX_REVISION",
    "executablePath": "C:\\external\\prism-hexagon-model-probe.exe",
    "executableSha256": "EXACT_LOWERCASE_64_HEX_DIGEST",
    "buildReceiptPath": "C:\\external\\local-model-build.json",
    "modelPath": "C:\\external\\Swift-Bonsai-2-PQ2_0.gguf",
    "packageDirectory": "C:\\external\\verified-v0.10.0-runtime",
    "signtoolPath": "C:\\external\\signtool.exe",
    "outputDirectory": "C:\\external\\new-model-probe-evidence",
    "tokenId": 42,
    "tokenIds": [42, 43],
    "executionBackend": "hmx-bitplane",
    "maximumSeconds": 300
  }
}
```

Use a fresh output directory. The fixed child gets 195 seconds including model
admission, the CPU arm and a native session of at most 120 seconds; the outer
supervisor reserves another 75 seconds. Intake refuses before model execution
when the complete child allowance no longer fits. The entire owner workload,
including builds, intake, power snapshots and archival, must independently fit
its predeclared aggregate budget. Historical investigations remain closed.

`tokenId` remains required. Omitting `tokenIds` selects that single token; an
explicit sequence must contain one or two vocabulary IDs and start with
`tokenId`. Omitting `executionBackend` selects `guarded-hvx`; the only other
accepted value is `hmx-bitplane`. This preserves the original single-token
request. The current probe executable emits explicit backend and sequence
evidence even for that legacy request; older executable output without those
fields cannot qualify the extended supervisor.

Both selected paths admit all 40 operations per token before the first native
graph evaluation. A two-token control therefore requires exactly 80 completed
operations and two completed projections in the same bounded session. Guarded
HVX compares 1,392,640 returned integers per token; HMX compares 5,570,560 returned
binary-plane values per token. The eight HMX planes decompose one token's
quantized activation; they do not represent eight prefill tokens. The supervisor
requires the selected backend, the exact ordered `tokenIds`, every per-token
logit/state comparison and hash, final hashes matching the last comparison,
complete native cleanup and an empty owned Job. HMX evidence must also carry the
source-bound `nativeHmxBitplaneControl` result. Operation, session, child and
cleanup limits are unchanged. No adapter is loaded by this control.

The local receipt has this closed schema:

```json
{
  "schemaVersion": 1,
  "kind": "local-prism-hexagon-model-probe-build",
  "hostedBuild": false,
  "buildSucceeded": true,
  "cargoLocked": true,
  "target": "aarch64-pc-windows-msvc",
  "prismRevision": "EXACT_LOWERCASE_40_HEX_REVISION",
  "compiler": {"path":"ABSOLUTE_RUSTC_PATH", "bytes":1, "sha256":"EXACT_SHA256"},
  "executable": {"bytes":1, "sha256":"EXACT_SHA256"},
  "sources": [{"path":"Cargo.toml", "bytes":1, "sha256":"EXACT_SHA256"}]
}
```

Generate that receipt only after observing a successful local `cargo build
--locked --release --target aarch64-pc-windows-msvc -p bonsai-hexagon-runtime
--bin prism-hexagon-model-probe`, with its target directory outside the source
closure. Preserve the actual compiler/executable lengths and SHA-256 values.
Enumerate **every regular file** below Prism `src/` and `tools/`, excluding
directories named `target`, `.git` or `.worktrees`; add root `Cargo.toml`,
`Cargo.lock`, and existing `build.rs`, `rust-toolchain.toml`, `.cargo/config.toml`.
Use normalized relative paths with forward slashes, actual lengths and lowercase
SHA-256 digests, including the SHA-256 of empty files. Sort for reproducibility.
PowerShell `Get-FileHash -Algorithm SHA256` and `Get-Item.Length` provide those
values. The supervisor independently enumerates the closure and holds every
checked source, executable, package and model against mutation through execution.
The receipt is explicitly local provenance; it cannot replace a hosted receipt
in the existing projection verifier.

Intake verifies all seventeen signed-package files, five catalog members,
the catalog tool and Microsoft-signed installed driver; model SHA-256 admission
occurs inside the exact source-bound probe while the supervisor holds the model.
At least 18 GiB of physical memory must be available before model loading.
The child runs in a strict owned Job with inherited vendor overrides cleared
through child-only overlays. This diagnostic does not qualify service isolation
or device allocations outside process accounting.

`model-device.json` retains the result, per-arm and workload power observations,
owned-process teardown and local identities. CPU/NPU snapshots may be unknown;
equal endpoints do not establish uninterrupted power stability. A failed child
retains its raw output and completed arm observations without publishing parity.
Do not retry or claim performance from this bounded correctness workload.

The [2026-11-03 X2 AC receipt](../../docs/evidence/hexagon-model-device-20261103-ac/summary.json)
passes this fixed token-42 control: all 40 submissions, 1,392,640 independent
integer comparisons, full logits/state bits and explicit terminal cleanup.
Both comparison arms observed AC at their endpoints with battery saver off.
The supervisor's 51,689 ms includes intake, model admission and both arms;
it is not kernel timing, decode throughput or evidence of a speedup. Wider
placement, adapters, persistent service and production admission remain pending.
