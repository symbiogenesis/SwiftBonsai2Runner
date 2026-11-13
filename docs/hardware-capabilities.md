# Hardware discovery and profile policy

Runner's development default is Rust Prism 0.1.27 CPU inference with the fixed
[X2 Hexagon policy](hexagon.md) when its signed package and driver are available.
Discovery supplies host/runtime inventory and checked compatibility/evidence records;
**automatic ranking and selection of qualified profiles is not connected to serving**.
The optional model-free probe verifies the runtime against the immutable
[release lock](../config/prism-inference.lock.json).
The [selection contract](#automatic-profile-selection) defines eligibility;
[PLAN.md](../PLAN.md#hardware-selection) owns outstanding work.

## Host inventory

`bonsai-tools doctor --json '{}'` returns a v2 native report containing
`Capabilities` (`schema_version: 1`, `kind: runner-host-capabilities`), the CPU
compatibility identity and native observations. Rust owns discovery, projection,
bounded tool probes and optional create-new JSON publication. Invoke a trusted
prebuilt executable; inventory never builds or installs dependencies.

`dataPath` selects an existing external data directory; `outputPath` selects a
new external report file with an existing parent. `nativeGraphics: true` adds
the separately bounded Vulkan/OpenCL probes. Exact tool paths and retained
version-probe logs need review before sharing.

| Section | Observations | Unknowns |
| --- | --- | --- |
| `operating_system`, `process` | Native OS/current-process architectures, available OS version/build, emulation | Unavailable facts |
| `cpu.identities` | Registry names/vendors and native CPU-set core/logical counts | Unavailable topology; observations are current-process scoped |
| `cpu.isa` | Native Windows AdvSimd/dot-product flags | Native eligibility from emulation |
| `memory` | SMBIOS installed bytes and OS-usable/available physical bytes | System-wide committed usage; `Memory` separately labels effective process/system commit limits |
| `gpu` | SetupAPI device/vendor and driver-registry version/date | Driver signatures, API features/device budgets |
| `npu` | SetupAPI ComputeAccelerator device/vendor and driver-registry version/date | Driver signatures, architecture/firmware/backend readiness |

Each section records its source and `observed`/`partial`/`unknown` status, using null
for unknown values. Empty lists mean successful queries. Native inventories allow
16 devices per class; owned version-probe processes share a 90-second allowance.
BIOS manufacturer/model exclude serials; reports also omit hostnames, device-instance
IDs and user/network inventory.
SMBIOS installed bytes and OS-usable bytes keep distinct sources; live memory does
not replace admission. Family names and driver signatures do not prove ISA,
execution or deployment trust; emulated-process ISA cannot qualify a native runtime.

### Native CPU observations

`bonsai-tools doctor --json '{}'` includes Rust `host-cpu`'s v1
`runner-native-cpu-observations`. The pure `host-capabilities` command still accepts
CIM facts; the native Doctor labels its current
registry/CPU-set sources explicitly.

| Observation | Contract |
| --- | --- |
| CPU sets | Group-relative topology, raw efficiency/scheduling classes and current-process allocation/parking; static topology/live availability remain separate |
| Bounds/failure | Bounded allocation, records and retries; malformed/unavailable results stay unknown, without guessed core counts |
| `power_snapshot` | `GetSystemPowerStatus` AC/battery, battery saver and charge with unknown sentinels |
| `power_policy_snapshot` | [Active scheme GUID][scheme], [effective mode][power-mode] via version-two notification, one-second initial wait, registration removed before successful return; unknowns suppress X2 comparisons |
| `isa` | Native ARM64 [Windows feature query][windows-features]: NEON/AdvSimd 19, dot product 43, **NEON I8MM 66**; SVE I8MM 57 is distinct |
| `host_identity` | Checked speech-build host query: fixed hardware-registry CPU name/architecture/family/model/revision/vendor, `RtlGetVersion` kernel and sorted available feature IDs in `0..256` |

Windows CPU sets use [group-relative indices/classes][cpu-sets]; IDs/classes establish neither
persistent/process-affinity eligibility, optional ISA nor core names. Power snapshots observe
settings, not energy; equal values cannot exclude intervening or in-scheme edits.
ISA observations are independent of CPU-set success/.NET digest; non-ARM64 processes report null
ISA even on ARM hardware. `dispatch_qualified` stays false. False Windows flags mean unsupported
**or HAL-undetected**. I8MM detection arrived in Windows 11 24H2 / Server 2025; the pinned bindings
lack its constant, so the native boundary explicitly retains SDK 26100 value 66. Discovery executes
no optional instruction and changes no dispatch/selection.

Malformed, missing or oversized registry values make the whole host identity unknown. Registry
name is an independent observation, not verified silicon SKU/firmware. Identity is separate from
live availability/power. The [X2 screen](cpu-kernels.md#x2-cpu-thread-screen) retains
exact identity in plans/receipts. Two registry-derived observations are not independent identity checks.
Unknown/changed identity, including receipts lacking it, suppresses timing ratios. Kernel/feature changes require fresh captures; compare the
whole bounded feature list, not descriptive `isa` flags. No new digest/backend permission follows.

### Native Vulkan observations

`nativeGraphics: true` adds `NativeGpuObservations` from `host-gpu`. The native
ARM64 child queries absolute system `vulkan-1.dll` under ordinary headless
mitigations in a 512 MiB one-process Job, with bounded output and a ten-second
deadline. `VK_*` overrides reject; no weaker-policy retry is allowed. Installed
loader/ICD code remains a trust boundary with unverified signatures/dependencies.

Only an instance is created, without requested layers/extensions, logical device,
submission, surface, model or device allocation. Successful empty enumeration is
observed; failures return `status: unknown`, `adapters: null`.

| Observation | Bounds and meaning |
| --- | --- |
| Adapters | 16 adapters, 64 queue families each, 1,024 extensions. Incomplete/oversized enumeration rejects. Retain names, raw vendor/device/driver IDs, API versions, compute queues and heaps. Driver encoding is vendor-specific; CPU/virtual adapters have `hardware_gpu: false`. |
| Feature queries | Application API is loader API capped at 1.3; core eligibility uses the lower application/device version. Eligible queries include subgroup width/operations/stages, FP16 and integer-dot features. Unqueried fields stay null. |
| Failures | Fixed `failure.stage`, raw Vulkan result when supplied, and observed loader/application API versions. `query-failed-device-support-unknown` identifies no failing DLL, mitigation or root cause. Supervision/malformed reports use `worker-supervision-or-report`. Arbitrary driver text/paths are excluded. |
| Execution policy | `required_execution_policy` describes the query contract; only the owning wrapper adds `execution_policy: headless-worker` after supervised exit. Retain bounded child JSON on failure. |
| Module snapshots | Immediately before/after the single `vkCreateInstance` call: one fixed 256-handle [enumeration][process-modules], at most 260 UTF-16 units per [basename][module-basename], no retries/growth. Malformed, truncated, oversized or path/control-containing results make the snapshot unknown. Report the difference only when both are observed. |
| On-disk hashes | After snapshots, prioritize the loader then new module basenames; at most sixteen files, 16 MiB each/64 MiB total, 64 KiB read buffer and 500 ms shared deadline. Unknown before-snapshot permits only the loader. Fixed 32,768-unit path queries reject truncation. Local-file guards reject redirects, deny writer/delete sharing and recheck metadata. Failed reads consume the byte budget and never emit prefix digests. |
| Graphics count | After instance failure, one count-only [`D3DKMTEnumAdapters2`][graphics-count] call resolves the fixed export from already loaded `gdi32.dll`. No adapter handles or extra loader are requested. Retain raw `NTSTATUS`; count is defined only on success. Module/export lookup failures retain immediate Win32 error; unattempted fields stay unknown. |

The ten-second supervisor also bounds blocking driver/filesystem calls. Module
snapshots omit transient/failed/data-file loads; an empty difference does not
exclude an attempted ICD. File hashes identify bytes at current paths, not earlier
mapped images. Basenames establish neither file identity nor trust. The graphics
count can narrow diagnosis but cannot prove a Vulkan-capable device, an attempted
ICD or the cause of instance failure. [Windows loader source][loader-windows] is
a diagnostic lead, not installed-byte provenance.

Heap capacity is not free memory; count shared physical storage once. Cooperative
matrix arithmetic, live budgets, exact SKU, signed-driver readiness and complete
dependencies remain unqualified. `backend_ready` stays false and CPU digest/launch
policy is unchanged. Failed queries leave access unknown. See Khronos
[properties][properties], [properties2][properties2], [application API][application-api]
and the locked [ash API][ash].
### Native OpenCL observations

`nativeGraphics: true` adds `NativeOpenClObservations` from bounded `host-opencl`: GPU information
through the absolute system loader under unchanged headless policy, with no context, queue,
program, kernel or device allocation. It retains architecture/platform/device/driver/compiler
facts and bounded loader size/hash; provenance stays unverified, `backend_ready: false`.
CPU digests/launch policy ignore it.
[OpenCL access and source audit](adreno.md#opencl-access-and-source-audit)
owns exact bounds, identities and missing pinned PQ2/PTQ1 projections. Its pilot remains in PLAN;
OpenCL observations do not resolve Vulkan worker access.

## Runtime inventory

`prism-inference --capabilities` emits one v1 `prism-runtime-capabilities` record: package/version/
compiler target, scalar support, production NEON build predicate, exact graph identity suffixes
and unavailable Vulkan/Hexagon. `qualification` is always `unqualified`.
This describes the generic runtime executable, not the separate server/worker's
Hexagon support or effective request placement. The compiled release lock owns
the current version. Do not interpret the generic accelerator fields as proof
that the server used CPU or NPU.

Provision/probe with the Rust `provision` command using
`{"operation":"inference","dataDirectory":"C:\\BonsaiData","includeCapabilities":true}`;
add `verifyOnly: true` to require existing files. Results add `Capabilities`
/`CapabilityVerification`,
runtime file/archive identities and external diagnostics. Verification guards stay held through
exit. Custom locks reject; reported version/native target must match the compiled release pin.

The frozen example is identical in Prism's `tests/fixtures/runtime-capabilities-v1.json` and
Runner's `src/cpu-policy/tests/fixtures/runtime-capabilities-v1.json`; wire changes update both
copies/checks. Example version/target are sample facts, not pins. In `bonsai_cpu_policy::hardware`,
`RuntimeCapabilities::from_json` accepts at most 16 KiB, checking exact fields/types/schema,
bounded unique graph IDs and consistent target/build claims. Unknown versions/fields, accelerator
claims and asserted qualification reject. The parser does not duplicate eligibility rules;
graph suffixes prove neither artifact hash, portable state nor numerical/device qualification.

## Profile and evidence keys

`ProfileCompatibilityKey::from_json` requires every identity, rejecting case-insensitive reserved
`unknown` as concrete target/graph/state identity:

| Field | Binding |
| --- | --- |
| `objective` | `interactive` or `battery` |
| `model_sha256` | Complete exact weights, including derived export |
| `adapter` | Exactly `{"kind":"none"}` or `{"kind":"artifact","sha256":"<lowercase SHA-256>"}` |
| `runtime` | `archive_sha256`, full `source_revision`, exact `target` architecture/OS/environment |
| `capability_class_sha256`, `backend_driver_sha256` | Independently reviewed static capability/backend-driver records |
| `graph_contract`, `state_layout`, `settings_sha256` | Exact numerical/state identity and all settings: phase placement, threads, batching, precision/migration |
| `workload` | `context_tokens`, `minimum_prompt_tokens`, `maximum_prompt_tokens`, `maximum_output_tokens`, `state` (`fresh`/`reused`); maximum prompt plus full output fits context |

SHA-256 values are 64 lowercase hex digits; Git revisions are 40. Static capability records bind
observed facts, excluding timestamps, paths and live RAM/power; marketing family names are
insufficient.

Doctor's `CpuCompatibilityIdentity` retains canonical UTF-8 JSON/lowercase SHA-256.
`host-capabilities` with `{"operation":"identity","capabilities":...}` reproduces it without probes.
Scope `native-windows-arm64-cpu-observations-v1` binds native architecture, ordinally sorted CPU
names/vendors/core counts and AdvSimd/dot-product flags; CPU backend binds OS version/build.
Duplicate processors retain multiplicity. Paths/timestamps/live RAM/power/accelerators do not
affect CPU digests. Missing/emulated/contradictory facts return `unknown-capability` and null
records/digests; qualification stays `unqualified`. These exclude full topology, unobserved ISA,
accelerator/driver identities, catalog admission and selection. Runtime archive/source/target
remain separate; hashing the whole Doctor report is not compatibility policy.

`ProfileEvidence::from_json` binds the full key to `tested_main_revision`, decimal-string
`ci_run_id`, retained `capture_sha256`, `verdict` and `energy_verdict`: `unknown`, `passed`,
`failed` or `no-go`. Battery pass requires energy pass. `applies_to` compares exact identity,
including objective/workload, without trusting asserted verdicts or authorizing execution.
Source ancestry/build provenance/run-capture existence/numerical/quality/resource/security
acceptance still need review.

Reserved value-free `FallbackReason` codes are `unknown-capability`, `unavailable-backend`,
`incompatible-runtime`, `incompatible-driver`, `stale-evidence`, `insufficient-memory` and
`strict-backend-unavailable`. The selection contract below defines their use; no selector emits them
yet.

## Automatic profile selection

This is the contract for a qualified profile selector, not enabled serving behavior.
Runner owns discovery, selection,
verification and admission. [Prism Inference][prism] owns kernels, graph/device/state execution
and effective-placement reporting. Their versioned handshake keeps kernel eligibility rules
out of the broker.

### Device classes and observed identity

Target identities must come from the selected native inventory and matching qualification receipts;
family names alone establish no kernel or backend readiness.

| Target | Candidates | Required independent evidence |
| --- | --- | --- |
| X1: initially X1E-78-100 / Adreno X1-85 / Hexagon v73 | Packed scalar/NEON CPU, Vulkan, viable packed Hexagon regions | Windows ISA/driver, signed NPU access, mapping costs, exact Swift graph/state and device results |
| X2: initially Elite Extreme / Adreno X2-90 / Hexagon v81 | Packed CPU/Vulkan, batched HMX prefill, packed HVX decode | Actual SKU/capabilities, signed package, extended DMA and separate phase/sustained results |
| Unknown SKU or invalidated/unavailable accelerator | Reviewed compatible CPU fallback | Native ARM64, baseline ISA, verified artifacts and ordinary memory/context admission; otherwise refuse execution |

Family rules cover only independently qualified SKU/capability/driver classes. The
[X2E-94-100/X2E-96-100 product brief][x2-specs] advertises up to 228 GB/s system bandwidth and 80
INT8 TOPS; neither predicts Swift throughput or exclusive bandwidth, and other SKUs differ.

[DMA64][dma64] permits v81+ buffers beyond the older 32-bit mapping window only with the actual
supported Windows runtime/driver/DMA path; X1 mapping/tiling remains separate. Address windows are
not physical RAM budgets. [Q1/Q2 Hadamard support][q2-hadamard] does not establish PQ2/PTQ1
activation arithmetic, grouped recurrent transforms, adapters or signed distribution. A catalog for
existing binaries cannot sign rebuilt code.

### Eligibility, ranking and admission

Selection is a bounded deterministic lookup/ranking over versioned qualified profiles; ordinary
startup neither benchmarks nor tunes. Explicit CPU controls remain reproducible.

A profile binds exact model/derived artifact, adapter, runtime/build/backend, static hardware/driver
compatibility, numerical/state/settings contract, shapes, context/full output reservation, objective
and fresh/reused workload bucket. Required unknown, mismatched or stale facts exclude it. Discovery,
family matches and asserted receipts cannot authorize execution; signed backend readiness is
independent.

| Objective | Ranking basis |
| --- | --- |
| Interactive | Complete-task latency within qualified quality/resource/security constraints |
| Battery | Measured energy per useful task within accepted latency/quality/resource limits; battery operation alone proves no saving |

Retained device evidence, versioned thresholds and deterministic tie-breaking include losing/no-go
entries. Placement, phase threads/waits, batches/tiles, sustained power policy and state layout
belong to profile identity. Four threads and all cores are controls, not universal optima.

Recheck current RAM/commit/device budgets and backend readiness immediately before admission. An
unavailable preferred accelerator may yield an eligible qualified CPU fallback; an explicit
strict-backend diagnostic fails. Never silently change model, sampler, adapter, precision, context
or full output reservation to fit a profile.

Diagnostics report requested/effective profile, device class, phase placement, build/evidence
identity and a value-free fallback reason, excluding prompts/tensors. The [native Rust
UI](ui-architecture.md) presents a simple automatic choice; tuning stays in trusted configuration,
without arbitrary public arguments/paths or inherited environment overrides.

### Prefill, decode and complete state

The pure Rust [offline ranking policy](../src/cpu-policy/src/hardware/ranking.rs)
compares independently reviewed same-workload observations across CPU, Adreno,
Hexagon and qualified mixed prefill/decode plans. It makes no family or TOPS
assumption: either X1 or X2 may retain CPU, select GPU or recommend NPU according
to its own evidence. Interactive candidates must clear ten percent lower
complete-task latency using a reviewed conservative upper estimate; battery
candidates must clear ten percent lower task energy without a latency regression.
Missing energy telemetry excludes battery ranking. This is a conservative initial
screen, not statistical qualification or the separate capacity-benefit exception.

Exact current keys must match individually reviewed receipts. A shared comparison
digest binds artifacts, sampler/scale/precision, actual token boundaries, power,
cold/warm treatment and complete setup/transfer/transition cost accounting across
different backend builds. Callers must verify that measurements, placement and
comparison digest bind to the same archived captures; parsing a receipt does not
establish that trust. Stable ties prefer fewer accelerator phases, then backend
order and profile ID. Mixed-phase observations require reviewed complete-state
handoff; adding independently fast phase timings is insufficient.

The result contains only a provisional profile/placement recommendation. It opens
no device, supplies no launch arguments and is not connected to serving admission.
Signed device execution, qualification, live budgets and safe request-boundary
selection remain pending in [PLAN.md](../PLAN.md#hexagon-npu).

Batched prefill reuses weight tiles across tokens; sequential blocks alone do not. Packed decode has
little reuse and pays bandwidth, unpacking, transforms and dispatch/submission costs. [Algorithm
contracts](inference-algorithms.md) separately govern tiled projections and chunkwise GDN.

Keep PQ2/PTQ1 packed storage and bounded local tiles as the control. Charge load/repack/compile,
synchronization, migrations and cache misses as well as storage. Residency, contiguous regions and
reusable submissions require correct ownership. Shared bandwidth/power makes simultaneous execution
a candidate to measure, not an assumed gain.

Plans remain stable during a request. Reevaluate power, memory, runtime or driver changes only at
safe boundaries. Backend/phase transitions require qualified complete KV/recurrent/convolution/
checkpoint migration with declared numerical/layout identity and cost, or verified teardown/
rebuild. Otherwise retain the eligible plan or return a bounded recovery result; never continue
partial work on another backend. Driver changes invalidate affected accelerator evidence/caches, not
an independently compatible CPU control.

Every route preserves the [broker contract](broker-policy.md): one active conversation,
authentication, exact counts, deadlines, cancellation, admission and daily isolation. Rust
acceleration also preserves the
[runtime's safe device/replacement boundaries](native-rust-engine.md).

### Accelerator storage and lifetime

The GPU/NPU resource ledger includes packed/repacked or expanded weights, duplicate CPU/device/
adapter storage, mappings, staging/RPC buffers, scratch, state/checkpoints, compilation caches and
peak load overlap. Count unique physical bytes once; shared memory does not prove zero-copy.
Address windows and heap capacities are separate from physical RAM and live free-memory budgets.
Job commit omits some mapped/device allocations, so admission also charges resources outside it.

Submitted work retains buffers until device use ends. Allocation/mapping refusal, timeout,
unsupported shapes, device loss, driver changes or cancellation must preserve complete committed
state or close admission through verified teardown/reset; hidden replay and partial CPU continuation
are invalid. Qualified fallback follows the request-boundary rules above.

Private bounded repack/compiled caches bind artifacts, source/options/toolchain, kernel/runtime/ABI,
observable driver/firmware, device, shape, precision and placement. Check metadata and invalidate
affected entries on changes. Diagnostics exclude prompts and tensor payloads.

### Qualification scope

Receipts bind actual SKU/capabilities, OS/driver/backend, artifacts/profile, settled main revision,
CI run, raw captures and uncertainty. Workloads distinguish cold/warm/reused/edited state, short/
long prefill, fixed/natural outputs, AC/battery, pressure and sustained use. Report first useful
token, complete task, decode, peak memory, responsiveness, thermals/comfort and observable energy.
One family/device does not qualify another; missing valid power telemetry excludes energy ranking.

Selector checks cover unknown/misidentified devices, missing ISA/signed backend, stale runtime/
driver/evidence/cache, budgets/workload boundaries, deterministic ranking, objective changes, CPU
fallback, strict rejection and complete-state handoff/failure/cancellation. Simulate negative ISA
cases; execute optional instructions/accelerators only on eligible hardware.

Promotion follows the [validation criteria](validation.md#promotion-criteria), including
independent [LoRA TPS/quality](swift-lora.md#standard-profile-qualification),
installation/rollback and affected-upgrade requalification. Qualified families may coexist with
unsupported classes and qualified CPU fallbacks. [Validation policy](validation.md) requires
main-only
matching-device CI for full suites, benchmarks and performance captures; local builds/focused
untimed checks are allowed.

## Focused checks

From an external temp working directory, use absolute checkout/build paths:

```text
cargo test --manifest-path <repo>/Cargo.toml --target-dir <temp>/target --locked --offline --release -p bonsai-tools --lib host_capabilities::tests
cargo test --manifest-path <repo>/Cargo.toml --target-dir <temp>/target --locked --offline --release -p bonsai-tools --lib doctor::tests
cargo test --manifest-path <repo>/Cargo.toml --target-dir <temp>/target --locked --offline --release -p bonsai-cpu-policy hardware::tests
```

Coverage: malformed/missing probes, emulation, failed/empty inventories, memory units/overflow,
strict wire parsing, identity invalidation and output reservation. Full suites/benchmarks remain
[CI-only](validation.md).

[cpu-sets]: https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-system_cpu_set_information
[scheme]: https://learn.microsoft.com/en-us/windows/win32/api/powersetting/nf-powersetting-powergetactivescheme
[power-mode]: https://learn.microsoft.com/en-us/windows/win32/api/powersetting/nf-powersetting-powerregisterforeffectivepowermodenotifications
[windows-features]: https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-isprocessorfeaturepresent
[properties]: https://docs.vulkan.org/refpages/latest/refpages/source/VkPhysicalDeviceProperties.html
[properties2]: https://docs.vulkan.org/refpages/latest/refpages/source/VkPhysicalDeviceProperties2.html
[application-api]: https://docs.vulkan.org/refpages/latest/refpages/source/VkApplicationInfo.html
[ash]: https://docs.rs/ash/0.38.0+1.3.281/ash/struct.Entry.html
[x2-specs]: https://www.qualcomm.com/content/dam/qcomm-martech/dm-assets/documents/Snapdragon-X2-Elite-Product-Brief.pdf
[process-modules]: https://learn.microsoft.com/en-us/windows/win32/api/psapi/nf-psapi-enumprocessmodules
[module-basename]: https://learn.microsoft.com/en-us/windows/win32/api/psapi/nf-psapi-getmodulebasenamew
[graphics-count]: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/d3dkmthk/nf-d3dkmthk-d3dkmtenumadapters2
[loader-windows]: https://github.com/KhronosGroup/Vulkan-Loader/blob/main/loader/loader_windows.c
[dma64]: https://github.com/ggml-org/llama.cpp/pull/29197
[q2-hadamard]: https://github.com/ggml-org/llama.cpp/pull/30627
[prism]: https://github.com/symbiogenesis/prism-inference
