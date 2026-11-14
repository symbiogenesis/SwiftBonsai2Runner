# Hexagon packed-weight audit

`pq2_bridge.ps1` forwards to a bounded, model-free Rust audit for the first Hexagon
projection experiment. It never loads an inference engine, opens a device or
rewrites a GGUF file. Native inference kernels and device ownership belong in
Prism; this tool prepares an independent layout control.

Run the inventory audit with PowerShell 7.6 or later:

```powershell
pwsh -NoProfile -File tools/hexagon/pq2_bridge.ps1
```

The complete regression suite runs on hosted CI through
[Hexagon prerequisites](../../.github/workflows/hexagon-qualification.yml) and
Foundation. A focused local case is
`pwsh -NoProfile -File tools/hexagon/test_pq2_bridge.ps1 -Test test_known_byte_fixture_splits_at_element_64_and_keeps_fourth_symbol`.
Follow the [local-check and device-budget policy](../../docs/validation.md).

These compatibility entry points require the checked native `bonsai-tools`
executable through `BONSAI_TOOLS_PATH` or PATH. They do not compile inside an
owner workload. `hexagon-policy`'s `audit` request owns PQ2 binary conversion,
strict inventory admission, retained independent packed-fixture verification,
checked integer operation/traffic/resident accounting, binary16 counterexamples
and VTCM solver policy. `test_pq2_bridge.ps1 -Test NAME` and
`test_k_tiling_and_reduction_audit.ps1 -Test NAME` select native fixtures from the
same checked executable. The Rust harness also exercises these fixtures in CI.
Static ledger values preserve the prior derivation, while their source inventories
now bind the Rust policy and fixed descriptive contracts. The historical original
operation ledger and independent capture bytes remain unchanged.

[Hexagon qualification](../../.github/workflows/hexagon-qualification.yml) owns
current native fixture preparation, model-free policy controls and the independent
`public-native-abi` hosted job. That job checks native queue layouts and the retained
generated start/stop wire control without depending on device preparation or admission.
SDK and public-QAIC preparation helpers retain exact pins and
[source/ABI boundaries](../../docs/hexagon-source-audit.md#sdk-inputs-recipe-and-terms).
New SDK or wire coverage needs a separately reviewed hosted workload. Existing
provenance snapshots cannot replace unavailable replay bytes or close the Windows,
SDK or device gates in PLAN.

Hosted diagnostic compilation selects `rust-lld.exe` from the pinned Rust
toolchain, matching the packed-artifact builder. Fresh build receipts bind its
ARM64 image, size and SHA-256, and native process ownership still rejects and
stops surviving descendants. The focused
`hexagon_tools::tests::hexagon_hosted_compiler_closes_its_owned_process_tree`
regression compiles the actual dispatch fixture through that strict runner and
executes only its model-free self-test. A direct compiler/self-test check does
not establish that regression or cold Cargo host/target process-tree acceptance.

An optional raw PQ2 fixture can be checked with
`-Payload ABSOLUTE_FIXTURE_PATH -Columns 384 -Rows 2`. The fixture must contain
exactly the declared complete rows, at most 1 MiB. This is a raw packed fixture,
not an entire model. JSON inventory inputs are individually capped at 1 MiB;
all reads enforce the limit even if the file grows. The tool prints JSON to
stdout and writes no converted weights.

`sdk_inventory.ps1` forwards to the standalone Rust package in `sdk-reader/`.
Rust owns the pinned XZ/tar inventory, strict JSON and identity validation,
C/IDL lexical and declaration analysis, CMake token review, legal text decoding,
bounded excerpts and final receipt. Its separate exact dependency lock uses a pure Rust XZ decoder, with a
256 MiB decoder-memory limit and fixed archive-entry, byte and selection bounds.
The native inventory consumes selected bytes directly; it extracts no SDK paths
and executes no SDK code. The legacy two-argument reader still emits the bounded
data stream. Hosted inventory builds it with `cargo build --locked`
into external runner scratch. `-Reader` can select an already prepared executable;
the receipt records that executable's bytes and digest. Source/compiler identity
and hosted replay remain required for a new acceptance claim. `bonsai-tools`
invokes this native inventory directly under its process supervisor, without a
PowerShell child. Native-source and executable digests identify migrated receipts;
historical SDK receipts keep their original source identities.

The SDK lexical fixtures live in `sdk-reader/src/functional.rs`; the historical
`test_sdk_functional_review.ps1 -Test NAME` command selects the corresponding
Rust test. SDK metadata still establishes no license grant, SDK/public ABI
equivalence, Windows compatibility or device invocation authority.

Inventory JSON has a 64-container nesting limit and rejects duplicate names and
nonfinite JSON constants. Counts and derived storage extents must fit unsigned
64-bit values; tensor counts are positive and type IDs are exact integers.

The layout is pinned to the unchanged native oracle
[73bfe6cc](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-common.h)
and its
[dequantizers](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-quants.c)
.
PQ2 uses 128 weights/34 bytes and Q2_0 uses 64 weights/18 bytes: two little-endian
FP16 scale bytes followed by adjacent low-to-high two-bit symbols. Codes 0, 1,
2 and 3 decode to -1, 0, +1 and +2. Split each 32-byte PQ2 symbol block in half
and duplicate its original scale bytes. There is no quantizer, scale conversion,
ternary-only restriction or symbol reorder. The inverse rejects unequal scale
bits, including +0 versus -0. Nonfinite scales and partial rows reject.

The Swift inventory has 209,920,000 PQ2 blocks. This bridge increases
packed weight storage from 7,137,280,000 to **7,557,120,000 bytes**, an additional
**419,840,000 bytes (5.88235%)**. The arithmetic-only artifact estimate is
7,626,008,928 bytes before changed tensor alignment. Retaining both full layouts
simultaneously costs both allocations; this estimate excludes staging, VTCM,
activation/state buffers and device mappings. A future tiling prototype should
bridge bounded tiles rather than persist a second whole model.

Layout controls cover independent split/order bytes, every packed byte, fourth
symbols, finite scale extremes/zeros, awkward extents and malformed inputs.
Decoded f32 checks include signed zero. Inventory reconciles artifact identity,
histograms, geometry and symbols; metadata digests do not bind omitted sign arrays.
These controls qualify layout only. Activation quantization, accumulation,
transforms, LoRA, complete state and signed execution require
[separate acceptance](../../docs/hexagon.md#qualification-evidence).

The [native Rust driver boundary](native_rpc/README.md) and
[signed-package numeric fixture](dispatch_fixture.rs) cover separate access
questions. Metadata, mapping and compute are separate qualification boundaries; none alone qualifies
the production serving path.

The separate [packed projection operation control](packed-projection.md) keeps
original PQ2 blocks and pinned Swift Q8_K/FMA arithmetic in a bounded CPU tile
schedule. Retained native captures, Walsh ordering and rollback controls prepare
an oracle for future signed kernels; they establish no DSP execution or speedup.
Its [proposed tile wire contract](packed-projection.md#proposed-signed-kernel-tile-protocol)
serializes bounded original-PQ2/Q8_K work units and strictly replays them on CPU,
including sequencing, corruption and cancellation controls. It is an application
protocol for a future separately signed kernel, not an existing vendor operation.

The [resident host accounting](resident-host.md) separately derives the optional
Prism HMX arena's repeated CPU packing bounds, unchanged kernel payloads and
expanded queue-descriptor coverage. Its static ledger establishes no device
weight residency, cache-traffic reduction, full-model admission or speedup.
