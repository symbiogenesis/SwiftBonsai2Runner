# Five-minute PTQ1 correctness smoke

The owner-laptop trial compares one native reference and one Rust process on
the single ID `[248045]`. Select one Rust backend (`auto`,
`scalar` or `neon`), one row-worker count (`1`, the default, or `4`), and one
`adapter_condition`: `standard` (default, approved adapter at 1.3), `loaded-zero`
(approved adapter loaded at zero) or `no-adapter` (adapter loading omitted).
Both processes use the same condition; no worker, backend or adapter matrix runs.
**The entire owner job, including setup, loading, comparison, cleanup and
artifact upload, has a five-minute hard limit.** Its clock starts before
checkout/tool intake. The checked budget tool charges setup against the
at-most-240-second work allowance, honors earlier owner deadlines and preserves
separate teardown/upload reserves. Missing or out-of-range allowances reject
before owner/model preparation. Rust starts its work clock before preparation
and caps external calls by the remaining time. The parent passes the original
monotonic deadline to the child and recalculates the watchdog after process
startup. Receipts, startup and handoff cannot renew either deadline. Save active
capture/comparison phases before external work; never split omitted controls
into repeated jobs.

Cross-condition loaded-zero identity, changed nonzero logits, scalar, explicit NEON,
writer authoring basis, quality, performance, full context and service remain
pending. A passing result covers only one supplied token with the exact
selected adapter condition, backend and worker count. One condition cannot prove
identity or application relative to another condition. The comparator requires the reported worker
count to match and requires positive parallel-operation counts for four workers
(zero for one); these counters do not establish a performance gain.
Backend requests do not provide operation-level dispatch counters. No default or
admission change is authorized.

The application, ordinary inference service and reference collector are Rust.
The isolated collector resolves the unchanged released native DLL API without the
CLI's shell dependency. Its private `repr(C)` declarations are bound to the exact
pinned public header. Compile-time guards check all 62 field offsets, three sizes
and alignments before any native structure return is read. The collector compiles
no inference implementation; the reference DLL engine remains native.

`fixtures/collector-abi-v1.txt` retains 369 independently captured native layout,
type, enum, default/configured parameter and returned-aggregate observations.
Adjacent provenance binds the exact headers, compiler, DLLs, original driver and
license. Rust computes observations from its own declarations and the actual DLLs;
expected records are not used to generate observations. The original C++ driver
is removed. Focused tests retain success, every native failure phase,
cleanup order, bounded output, create-new collisions and partial evidence.
Returned native errors release acquired resources in order; an unexpected foreign
exception aborts the isolated collector before damaged native state can be reused.

`pins.json` binds native and Rust source archives and the native header. Both
preparation and capture require the Rust source's commit, archive size, digest
and URL to equal the current runtime lock; historical checkers cannot qualify a
new release. Capture
tools must be built in hosted CI and delivered with `capture-tools.json`:
`SchemaVersion: 3`, `CheckerFeatures: ["parallel-rows"]`,
`SupportedRowWorkers: [1,4]`, `RunnerRevision`, `NativeSource`, `RustSource`,
`CollectorLanguage: rust`, `CollectorSourcePath`, `CollectorSourceSha256`,
`AbiControlSha256`, `AbiProvenanceSha256`, `Compiler`, `RustToolchain`, and exactly two `Files`
entries (`Name`, `Bytes`, lowercase `Sha256`) for `swift-ptq1-reference.exe` and
`swift_model_check.exe`. Rust checks the current Runner revision, source
pins, Rust source/control digests, binary hashes/sizes and ARM64 PE architecture. Both binaries
and their receipt are guarded during execution. A historical schema-2 receipt is
accepted only for one worker and cannot acquire parallel capabilities through
injected fields. There is no owner build fallback.
Run the separately hosted-prepared maintenance executable directly. Its adjacent
`receipt.json` must bind its complete digest to the current Runner revision.

Hosted preparation uses the pinned Rust compiler/LLD for the collector and its
model-free tests; it no longer downloads native source headers or invokes a C++
compiler for this tool. The native source pins remain provenance for the DLL ABI.
Model-free collector controls distinguish omitted adapter calls from an adapter
loaded and applied at exact positive zero, retain dependency release order and
reject unsupported scale spellings before native loading.
Before owner model admission, both the original JSON ABI probe and the complete
live 369-record observation set must match the frozen native control. Duplicate,
missing, extra, changed or overflowing records reject the trial, as do changed
expected control bytes after receipt validation. These checks share the existing
240-second work allowance and do not replace the logits/state comparison.

```text
bonsai-tools native-qualification --request C:\Evidence\native-plan.json
```

```json
{"operation":"plan","kind":"ptq1","repositoryDirectory":"C:\\Source\\SwiftBonsai2Runner","rustBackend":"auto","rowWorkers":1,"adapterCondition":"standard"}
```

The owning workflow uses `operation: "capture"` and adds existing
`dataDirectory`, `captureToolsDirectory`, `workBudgetSeconds` and the original
`workDeadlineTick`. Source pins, hosted tool receipts, native ABI checks, model
admission, each owned capture and bounded atomic partial receipts live in Rust.
The independent PowerShell 7.6 full-state decoder and packaged Rust arithmetic helper remain owned,
deadline-bound children of the Rust command. The hosted-prepared helper is required; no compiler
runs
inside a device trial.

Actual execution belongs only to the main-only manual `native-ptq1-numerics.yml` workflow on a
native ARM64 Snapdragon ephemeral runner with only the `ptq1-numerics-<run_id>` label,
`run_device: true`, its physical-runner concurrency group and credential-clean child. The default
`run_device: false` prepares and tests the source-bound bundle on hosted CI only. It retains bounded
compiler/test log tails and a build receipt, including failed preparation; it does not start an
owner-device or model process. The exact PTQ1 export and native package must already be provisioned;
loaded conditions also require the approved adapter. Both numerical workloads refuse an existing
Bonsai app or project model process before model work. One model runs at a time. The shared policy
retains 15.5 GiB admission, 11.5 GiB Job commit and 4 GiB physical reserve. Source preparation and
compilation run in hosted CI; all owner preparation is included in its 240-second work budget.
Snapshots occupy approximately 301 MiB before metadata/compression; the Rust owner requires 1 GiB
external disk headroom. The upload selection excludes weights, adapters and build caches. Archive
credential-free evidence durably before temporary artifacts expire.

The collector saves one logits row and complete native sequence-zero state. Its
expected artifact identity is meaningful together with the Rust owner's retained
guards; the standalone collector does not attest authenticity. All writes are
create-new and each state snapshot is capped at 512 MiB. It samples no token.
The native sequence header must be magic `0xaf143cd8`, then sequence ID zero.
The strict decoder then requires the pinned single-stream text IMRoPE, non-replay
hybrid layout: 16 FP16 K/V layers and 48 F32 convolution/recurrent layers, exact
positions/types/extents and complete consumption. Control JSON, stdout and logits
are bounded before reads. Every value must be finite and satisfy
`1e-3 + 1e-3 * abs(reference)`; greedy-ID equality cannot override a value failure.
The complete one-token comparison is 129 files / 39,504,384 values.

Model-free synthetic checks cover transposed V, recurrent inventory, the native
magic/sequence header, truncated/tailing input, finite values, exact extents,
the unchanged tolerance, control binding and pre-read limits. No actual-model
smoke is established by adding this tool. Writer authoring basis remains pending
even if all numerical values match; native writer inputs/corrections are absent.

```text
pwsh -NoLogo -NoProfile -File tools/native-reference/test_compare.ps1 -Name test_transposed_v_and_complete_recurrent_inventory_match_logical_rows
pwsh -NoLogo -NoProfile -File tools/native-reference/test_compare.ps1 -Name test_state_decoder_rejects_truncation_trailing_bytes_and_changed_schema
pwsh -NoLogo -NoProfile -File tools/native-reference/test_compare.ps1 -Name test_comparator_enforces_finite_values_exact_extents_and_original_tolerance
pwsh -NoLogo -NoProfile -File tools/native-reference/test_compare.ps1 -Name test_control_binding_rejects_changed_adapter_scale_tokens_and_backend
pwsh -NoLogo -NoProfile -File tools/native-reference/test_compare.ps1 -Name test_capture_pre_read_bounds_reject_oversized_json_and_logits
```

The retained-file auditor is fixed to numerical run `41568281287` at Runner
`469f57b`. The auditor downloads the original evidence archive,
verifies its authenticated producer and artifact digest, and checks every unique
regular ZIP member against the 155 previously recorded file identities. The
tracked inventory is copied producer metadata; it supplies expected identities
for the cloud check and does not itself prove that retained payloads match.

After checking the original comparator source as immutable data by SHA-256, the
audit replays the Rust/PowerShell replacement against retained native/Rust data
and requires its complete report to
equal the original comparison, including payload, native-state, control/stdout
hashes and the explicit NEON/four-worker/standard-1.3 condition. Its artifact
contains the audit, file manifest, producer metadata and original/replayed
comparison reports. Focused synthetic controls cover archive and producer rejection.
No model, reference executable or checker runs.

A successful cloud audit preserves the original runtime/source execution;
it supplies no new device trial, renewed owner allowance, full-context, adapter
provenance, quality or default-promotion qualification. This fixed auditor does
not qualify later numerical captures.

The separate [public authoring reference](../../docs/adapter-public-reference.md)
has one predeclared all 48 A-vector reconstruction from the immutable public MLX
pack. Its main-only GitHub-hosted Windows ARM64 workflow defaults to small
independent synthetic controls; actual 8.6 GB intake requires explicit dispatch.
It retains complete A files, metrics and toolchain identities, without inference
or a full checkpoint write. A numeric match cannot authenticate the author's
original inputs or qualify native writer mappings, logits/state or promotion.

The current public reference uses PowerShell 7.6 or newer for bounded intake and
strict JSON/GGUF/safetensors descriptors, plus the dependency-free Rust
`bonsai-reference-numerics` executable for affine decoding, inverse Hadamard,
binary16 storage rounding and scalar binary32 dot products. Its independent
offline replay computes A/B comparison metrics in PowerShell and has separate
GGUF and retained-header readers. No Python runtime, package installer or numeric
wheel is required by either path.

This migration creates schema 2 and arithmetic identity
`public-mlx-f16-inverse1024-f16-rust-scalar-f32-dot-v2`: the direction norm uses a
sequential binary64 sum/square root rounded once to binary32; each dot product
uses a sequential binary32 sum with separate multiply and add operations. The
fixed tolerances and immutable public input pins remain unchanged. Historical
NumPy/BLAS recipe-v1 outcomes keep their original identities; their arithmetic
and qualification do not transfer to this replacement. The public workflow must
qualify the newly declared recipe with its hosted all-48 reconstruction and
independent replay. Local synthetic controls establish no actual public-pack
agreement or model qualification.

For focused local controls, provide a prebuilt helper with
`BONSAI_REFERENCE_NUMERIC_TOOL` or `-ToolPath` and select one named case. Omitting
the selection runs all cases only under GitHub Actions. The public workflow
builds the std-only helper with the pinned compiler; owner numerical trials
consume its source-bound packaged executable.
