# Local actual-model Hexagon probe

`bonsai-tools hexagon-tools --request ABSOLUTE_JSON_PATH` accepts the separate
`model-device` operation below. This local diagnostic evaluates one to three explicit
original-PQ2 tokens with `blk.0.ffn_up.weight` on the selected guarded HVX or HMX
bitplane path, compares every returned integer limb or plane and the complete
graph logits/state against its CPU shadow after each token, and requires explicit
native cleanup. Other projections stay on CPU. It enables no
production backend and establishes no inference speedup or broader adapter qualification.

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
    "residentHostWeights": false,
    "adapterCase": "standard-scale-1.3",
    "adapterPath": "C:\\external\\bonsai-abliterate-lora.gguf",
    "cpuBackend": "neon",
    "rowWorkers": 4,
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

`tokenId` remains required. Omitting `tokenIds` selects that single token; the
wire format permits one to eight vocabulary IDs and requires the sequence to start with
`tokenId`. Omitting `executionBackend` selects `guarded-hvx`; the only other
accepted value is `hmx-bitplane`. This preserves the original single-token
request. The current probe executable emits explicit backend and sequence
evidence even for that legacy request; older executable output without those
fields cannot qualify the extended supervisor.

`residentHostWeights` defaults to false. Setting it to true requires the explicit
HMX backend and selects one fixed mapped host-weight arena: 25,067,520 bytes of
prepared weight payload in a 26,259,456-byte native mapping, with at most 65,536
bytes of host metadata. The first token prepares one tensor; later tokens must
hit that cache. This mode reports `integerAudit: "graph-parity-only"` and zero
per-integer comparisons while preserving every token's complete CPU-shadow
logits/state comparison, operation accounting and explicit native cleanup.
The normal path retains `integerAudit: "all-returned-integers"` and its full
integer oracle. The supervisor binds the selected mode and actual cache counters;
it accepts missing mode/audit fields only for historical normal-path receipts.
Mapped host reuse establishes no DSP residency or production eligibility.

The unchanged 536,870,912-byte control-state budget admits at most three tokens.
The pinned model's complete logical snapshot after token position `i` contains
156,893,184 bytes of fixed recurrent state plus `131,072 * i` bytes of KV state.
Because the CPU arm retains every snapshot for the later native comparisons,
an `n`-token sequence requires
`156,893,184 * n + 131,072 * n * (n + 1) / 2` bytes. Three snapshots require
471,465,984 bytes; four require 628,883,456 bytes and eight require
1,259,864,064 bytes. The supervisor rejects excess state before source/package
intake or model launch, with the exact required and allowed sizes. The child
independently checks the same pinned geometry before model loading. This does
not extend state, device, session or workload allowances.

`adapterCase` defaults to `none`, which requires omitting `adapterPath` (or setting
it to null). `loaded-zero` and `standard-scale-1.3` require the exact approved
9,682,464-byte adapter with SHA-256
`f1669534803d340a496015f5c45125f3437b4d13ec764f40e34488ce83967f42`.
The supervisor verifies and holds the adapter against mutation throughout both
arms. The source-bound child independently admits its identity. `loaded-zero`
compares an adapter-free CPU graph against a native graph with the adapter loaded
at scale zero; `standard-scale-1.3` uses the adapter at scale 1.3 in both graphs.
Final policy evidence binds the case, loaded status, adapter digest and scale bits,
including the CPU baseline's loaded status and scale.

`cpuBackend` accepts `scalar` (default) or explicit `neon`; `rowWorkers` accepts
one (default) or four. NEON and parallel rows require the corresponding Prism
build features; unavailable explicit selections fail before inference. Both CPU
and native graph arms use the selected CPU policy for their CPU-owned work.
The supervisor requires matching terminal policy fields. These options do not
change the projection placement or qualify the selected policy for daily service.

Both selected paths admit all 40 operations per token before the first native
graph evaluation. A two-token control therefore requires exactly 80 completed
operations and two completed projections in the same bounded session. Guarded
HVX compares 1,392,640 returned integers per token; HMX compares 5,570,560 returned
binary-plane values per token. The eight HMX planes decompose one token's
quantized activation; they do not represent eight prefill tokens. The supervisor
requires the selected backend, the exact ordered `tokenIds`, every per-token
logit/state comparison and hash, final hashes matching the last comparison,
complete native cleanup and an empty owned Job. HMX evidence must also carry the
source-bound `nativeHmxBitplaneControl` result. Serialized cleanup must report
successful queue stop/close, unmapping and module close, completed free, and no
poisoned allocation. `ownerPinnedUntilExit` must remain true: the native policy
retains driver/module/catalog owners until process exit after successful
unmapping/free; it does not report an outstanding allocation. The supervisor
separately requires an empty owned Job after the process exits.
Resource observations retain unknown effective HMX
placement and VTCM; they cannot qualify physical placement or available capacity.
Operation, session, child and
cleanup limits are unchanged. Numerical hashes and comparisons come from the
source-bound worker; complete raw numerical arrays are not retained for an
independent offline arithmetic replay.

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
closure. Add `--features experimental-neon,parallel-rows` for the explicit NEON,
four-worker example above. Preserve the actual compiler/executable lengths and SHA-256 values.
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

`model-device.json` retains the result, the observed child exit code when capture
returns, per-arm and workload power observations,
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
