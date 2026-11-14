# Standalone original-PQ2 OpenCL projection pilot

`bonsai-tools opencl-projection --request <absolute-request.json>` implements one
real Swift FFN-gate geometry (`K=5120`, `M=17408`). It consumes an exact raw
original-PQ2 matrix and an already CPU-prepared Q8_K vector. These are independently
SHA-256-locked inputs; a tensor-shaped fixture does not establish model provenance.
There is no GGUF/model loading, Hadamard/adapter application, graph-state integration
or application admission. [PLAN](../PLAN.md#adreno-vulkan) retains qualification.

The request contains `weights` and `q8k` objects with absolute `path` and lowercase
`sha256`, the reviewed `kernelSha256`, a new absolute `outputDirectory`, and `device`
with exact `platformName`, `deviceName`, `driverVersion` and `vendorId`. Unknown
fields reject. The selected device must uniquely report Adreno X2, availability,
an online compiler, 64-bit addresses and sufficient maximum-allocation capacity.
There is no first-device or changed-driver fallback. The fixed source is exposed
by `bonsai_windows_process::opencl_pq2_kernel_source()` for source-bound preparation.

The embedded OpenCL C 1.2 kernel reads unchanged 34-byte PQ2 records and computes
one exact signed integer dot per original 128-weight block. It requires no FP16,
subgroup, cooperative-matrix or integer-dot extension. The independent scalar
control traverses packed bytes and all four symbols, retains signed Q8_K extremes,
validates every Q8_K block sum, and compares all 696,320 returned dots. Original
half scales, original Q8_K scales and per-128 ordered FMA remain on CPU. Its
2,785,280-byte integer readback is charged explicitly; this primitive establishes
no complete-request speed benefit.

The wrapper uses the unchanged headless worker mitigations, a one-process 1 GiB
Job, ten seconds of child work and ten seconds for cleanup. Cold driver/kernel
compilation can exhaust this fixed bound; there is no automatic retry. Explicit
device buffers total 26,465,280 bytes. Native driver/compiler allocations remain
unknown and are not covered by Job commit or reported capacity. Blocking readback
and a terminal queue fence precede host-storage release. Unknown terminal ownership
retains the library and all buffers until supervised exit. Seven native release
results and outer Job cleanup remain distinct observations.

The selected output inventory is `request.json`, `receipt.json`, raw
`device-dots.i32le` for independent replay, and `projection.f32le` only after complete
integer parity. Useful bounded failure diagnostics are retained; empty logs and
successful duplicate stdout are removed. Preserve the exact locked raw input
bytes for replay rather than copying a working tree. Loader hash/size observations
do not verify mapped bytes, signatures or ICD dependencies. Aggregate and separate
CPU/GPU power snapshots retain unknowns explicitly; equal endpoints do not prove
uninterrupted stability. Performance comparison eligibility and backend qualification
always remain false.

Power observations are synced and atomically replaced in a bounded progress
checkpoint before and after each arm. After owned child exit the parent merges
that checkpoint into the single receipt, preserving observed CPU endpoints and
GPU-before even when a driver call times out. Unobserved endpoints remain unknown.
Interrupted or invalid checkpoint publication remains selected failure evidence.

This source preparation executes no device workload. Run a source-bound pilot only
in its owning CI or a separately predeclared eligible owner workload, charging
preparation, both controls, power, cleanup and retention to the original allowance.
Actual-device arithmetic, policy access, resources, graph/adapters and complete
request benefit remain open. Follow [Adreno policy](adreno.md) and
[validation](validation.md) before selecting execution.

The native ABI follows the Khronos [OpenCL headers](https://github.com/KhronosGroup/OpenCL-Headers/blob/main/CL/cl.h)
and [runtime ownership and command contracts](https://github.com/KhronosGroup/OpenCL-Docs/blob/main/api/opencl_runtime_layer.asciidoc).
