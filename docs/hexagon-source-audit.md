# Hexagon source and ABI boundaries

[Hexagon deployment and execution](hexagon.md) owns current serving behavior;
[PLAN.md](../PLAN.md#hexagon-npu) owns qualification work. Runner's active
[runtime lock](../config/prism-inference.lock.json) pins the Rust engine. The separate
[signed-Q2 candidate lock](../config/hexagon-signed-q2-candidate.lock.json) binds
package/source inputs and retained ABI controls; its old observations do not qualify
the current runtime, a full device profile or automatic profile ranking.

## Pinned packages and source decisions

The active X2 path uses the verified signed GenieX v0.10.0 DSP package and installed
CDSP driver. Package, selected DSP, catalog membership, installed driver and Rust
host executable have separate identities. Source availability is neither a callable
Windows ABI nor permission to replace signed package members. Rebuilt DSP code requires
its own supported signing and distribution route; do not change OS trust policy.

Native Rust integration preserves the model loader, packed arithmetic, transforms,
adapters, sampler and transactional graph state. A small audited vendor RPC/mapping
boundary may use system bindings with safe ownership; it adds no C/C++ inference
library dependency. [Native RPC tooling](../tools/hexagon/native_rpc/README.md) owns
its CLI and operation bounds. Module open, FD mapping and capability queries do not
establish DSP computation, DMA visibility or vendor teardown.

## Native Rust integration and placement

[Current placement](hexagon.md) selects 168 `PQ2_0` projections across the first 27
layers. All other graph regions need an explicit execution owner. Model metadata in
[`swift.metadata.json`](../config/swift.metadata.json) describes the 64-layer graph,
including full-attention and recurrent layers. PQ2 uses Q8_K for widths divisible by
256 and Q8_0 otherwise; PTQ1's independently pinned Q8_0 rounding and accumulation
cannot substitute for the PQ2 oracle.

Transform signs, normalization, direction and grouped recurrent basis are artifact
identity. LoRA and stateful graph work keep their own acceptance gates. Device
projection completion does not commit token state: the CPU graph reserves appends,
stages deltas and commits only after the whole token succeeds. Failure and
cancellation must drain or tear down in-use buffers, discard provisional output and
preserve the complete prior state. Migration includes attention, convolution and
recurrent state; partial continuation or automatic replay is forbidden.

Splitting each original 128-weight PQ2 block into two 64-weight Q2 blocks adds
5.88235% packed storage while preserving decoded weights. It does not establish
activation, scale/reduction-order, logits or state parity. Charge repacking, mappings,
VTCM, DMA, scratch, driver allocations and storage outside Job commit against the
[selected application budget](swift-memory.md); shared physical storage counts once.

## Native Rust computation requires another reviewed boundary

A compute control must bind generated-IDL marshalling, create/export one bounded queue,
start, register checked buffers/descriptors, submit, verify response extent/buffer
count/sequence and poisoned numeric output, then stop/drain/close or retain every
uncertain owner through verified teardown. Synchronous waiting does not remove
asynchronous device ownership or permit post-submit free.

The pinned newer `htp_iface.idl` adds VTCM-size and power-mode arguments to `start`.
Its wire layout must not inherit an older package's five-argument layout or catalog.
The public FastRPC queue header defines a four-u32/pointer-or-u64 record with expected
64-bit C offsets 0/4/8/12/16, size 24 and alignment 8. Declarations alone establish
neither installed Windows ARM64 layout nor calling convention.

Queue export requires later DSP import; close can fail while imported. References
require a prior same-FD mapping and an in-range subsection, but accounting covers the
whole mapping. Reference/dereference and sender/recipient cache flags differ.
`DSPQUEUE_TIMEOUT_NONE` is infinite (`0xffffffff`); the reviewed Windows wrapper routes
explicit zero to nonblocking read. Neither supplies an aggregate deadline. Keep the
native supervisor's owned deadline and uncertain-buffer retention. Preserve u64
`remote_handle64_close`; do not copy a host wrapper's narrowing 32-bit close typedef.

### SDK inputs, recipe and terms

The [SDK inventory lock](../tools/hexagon/sdk-inventory.lock.json) binds SDK archive,
selected inputs, recipe and lexical/declaration controls. The retained
[archive inventory](evidence/hexagon-sdk-inventory-41168523349/provenance.json),
[recipe observation](evidence/hexagon-sdk-inventory-41168872514/provenance.json) and
[functional comparison](evidence/hexagon-sdk-functional-41171339745/provenance.json)
retain producer provenance for those controls. The older SDK/generator selections
contain provenance summaries without their complete original raw payloads; they cannot
independently replay or close a gate. Required unavailable bytes leave acceptance pending.
The latest wire selection below retains its live ABI replay inputs. None supplies a
current installed-driver verdict.

The selected recipe uses SDK 6.7.0.0/tools 21.0.04. Its `qaic.exe` is x64, so local
ARM64 execution would emulate. The public generator and SDK generator are different
binaries. SDK and public whole-header streams differ even where selected prototypes
or comment-aware IDL token sequences match. Lexical comparison evaluates no
preprocessor conditions, imports or types. Effective Windows header closure, ABI,
callback/FD/cache/import-close lifetimes and driver binding need independent controls.

Reproduction preserves generator identity, flags, working directory, ordered include
closure, generated header/stub hashes and wire captures. The reviewed `build_idl`
recipe selects `-mdll -o BINARY_DIR`, SDK include directories and target includes in
order. Duplicate include directories are meaningful. A predicted filename or recipe
inspection does not prove resolved imports or generated bytes.

The retained legal scan does not establish SDK license applicability, redistribution
rights or generator-output terms. SDK execution and output distribution remain
unqualified until the applicable agreement is reviewed. Public QAIC terms do not
establish SDK terms. Further SDK preparation belongs on hosted CI under owner control.

### Public QAIC generation and wire control

The [public QAIC lock](../tools/hexagon/public-qaic.lock.json) freezes generator source,
Haskell dependency resolution, FastRPC imports and generation inputs. Preserve its
project/freeze/plan and generated bytes as data. Generation uses checked
`htp_iface.idl` with pinned public `remote.idl`/`AEEStdDef.idl`, documented flags and
the internal preprocessor. It executes no SDK generator or device code.

The [retained passing wire control](evidence/hexagon-public-qaic-41171859264/provenance.json)
is a direct replay input for `windows_queue_abi.ps1`. Its fixed vector uses a synthetic
u64 handle, start scalar `0x02010000`, one 48-byte input with fields at
0/8/16/20/24/32/40 and zero padding 4–7/44–47; stop scalar `0x03000000` has null
arguments. Linux LP64 C and independent Rust agreement qualifies only that fixed
vector. Preserve earlier generator/compiler no-go observations referenced by the
candidate lock; they diagnose the exact captured public-input closure.

The Windows control compiles unchanged generated start/stop code with native ARM64
MSVC and the explicitly selected public non-Windows header branch. This establishes
neither the absent Windows-specific header closure nor SDK equivalence. Other methods,
queue/cache/callback ownership, signed computation and model/profile qualification
remain independent.

Captured C/header/Rust files are immutable `.txt` data with hashes and notices, not
production build inputs. Retain the complete QAIC/FastRPC/llama licenses, IDL/version
notices and template provenance. QAIC's BSD-3-Clause-Clear terms exclude a patent
grant. Artifact-service ZIP digests and locally verified retained-file hashes identify
different objects; integrity alone proves neither provenance nor execution.

## Packed arithmetic controls

The [packed projection guide](../tools/hexagon/packed-projection.md) defines original
weight/sign extraction, independent activation inputs, per-limb/plane comparisons and
ordered CPU FMA. The guarded primitive's 576-byte host tile and 640-byte aligned VTCM
destination stride are different layouts. Full-M projection preparation uses forty
original-block operations; derived work counts establish no measured performance.

The exact HMX bitplane bridge and the current tolerance-based `native-q2` serving path
are separate arithmetic contracts. The bridge's terminal cap is 24,000 attempted
operations; reset cannot replenish it. Reserve complete prompt/output/attempted work
before admission. Batching existing operations changes transport, not their charged
logical count. Persistent admission needs an explicitly reviewed partial placement
or supported lifetime design; one successful synthetic operation supplies no serving
capacity, quality or complete-request benefit verdict.
