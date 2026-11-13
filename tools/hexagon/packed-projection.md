# Packed first-projection operation control

[`packed_projection.rs`](packed_projection.rs) is a std-only safe Rust CPU
control for a future signed Hexagon projection. It opens no driver, loads no
model and dispatches no DSP work. It is an operation oracle and scheduling
candidate; inference integration belongs in Prism. The
[source/capture manifest](packed-sources.json) binds its arithmetic to the
pinned Prism control revision and independent native observations. That
manifest is not the active [runtime release lock](../../config/prism-inference.lock.json).

The control keeps original 34-byte PQ2 blocks, including all four symbols and
raw finite half-scale bits. It stages complete packed tiles without a Q2 copy,
prepares Q8_K once and carries each row's f32 reduction across column tiles.
Each original 128-weight block applies its own scale in original order.
Cancellation before a tile or final publication discards every provisional
output. Input validation and numerical errors also leave the caller's output
empty. A future asynchronous implementation must additionally retain in-use
device buffers until verified completion/teardown; this synchronous CPU control
does not prove that lifetime gate.

The selected mode is **pinned Swift binary arithmetic**. In the pinned source,
`Arithmetic::PinnedSwift` selects `with_pinned_backend` for the exact verified
Swift artifact identity. It uses fused magic-add Q8_K rounding and ordered
per-PQ2-block FMA, with scalar or eligible NEON integer dots. Generic/source
arithmetic uses rounded-product ties-to-even preparation and nonfused adds;
the two are distinct contracts. The retained 584-byte native Q8_K capture
distinguishes codes 71/72 and -71/-72 at half boundaries. A separate retained
native PQ2 observation distinguishes output bits `c1f90b01` from source-order
`c1f90b02`. Tests run those original inputs through preparation and the packed
tile loop; they never launch the old DLL or regenerate golden bytes.

The optional CPU transform takes exact explicit sign bytes, applies signs
before each 1024-element normalized Sylvester Walsh-Hadamard block and keeps
the input-last-dimension basis. Sign values/extent are checked;
[verified extraction](../../docs/evidence/hexagon-swift-projection-binding.json)
binds the original model's signs. Their consumption through this control remains
pending; synthetic sign tests establish axis/order only. PTQ1/Q8_0 has a separate
[native-lane control](#ptq1q8_0-native-lane-control) below. Odd-128-group PQ2, grouped recurrent permutations,
LoRA/rank-one corrections and batched HMX execution are outside this control.
All adapters and committed graph state remain CPU-owned; no-LoRA and approved
adapter forward paths still require separate qualification.

## PTQ1/Q8_0 native-lane control

[`ptq1_hmx.rs`](ptq1_hmx.rs) consumes original 28-byte/128-weight PTQ1 blocks
and externally prepared 34-byte/32-code Q8_0 records. It preserves every native
fractional trit byte, including aliases; canonical export validation remains a
separate boundary. It does not quantize activations, load a driver or admit a
model. The injectable synchronous submission boundary enables signed-device
comparison after independent resource and lifetime qualification.

Pinned PTQ1 retains four floating accumulators over the whole row. Within each
Q8_0 group, lane `i` sums its four products at `i*4` and four at `i*4+16`.
Each group applies the original weight/activation scale product with lane FMA;
the final reduction is `(lane0+lane1)+(lane2+lane3)`. Combining these integers
before scaling changes retained native results.

The control creates four sparse unit-scale ordinary-Q2 rows for one Q8_0 group
and shares eight binary activation planes across them. Each row preserves its
eight original lane positions and zeroes every other weight. One K128/N8
operation returns 32 plane/lane dots; four operations cover one original PTQ1
block. Plane dots lie in -8..8 and reconstructed lane sums in -1024..1024.
The host stage contains two 576-byte source-native tiles, padded to 32 rows.
Every padding row has unit scales and zero symbols. Native submission returns
256 values for its 32 padded rows; an adapter must extract `[bit*32+lane]` into
the control callback's compact 32-value `[bit*4+lane]` result.
Neither padding nor bitplanes represent independent prefill tokens. The extra
rows, operations, return traffic and persistent lane accumulators require their
own whole-model resource/work ledger; the PQ2 ledger does not apply.

The two immutable independent ARM64 native-dot records cover widths 128/256
and distinguish native lane/FMA order from grouped scalar accumulation. Their
508 bytes and original provenance are hash-checked by
[`verify_packed_fixture.ps1`](verify_packed_fixture.ps1). Exhaustive byte controls
exercise native aliases, sparse packed traversal and signed codes; failure and
cancellation controls leave no published result. These CPU controls establish
neither signed HMX arithmetic nor actual-model logits/state, adapter parity,
asynchronous ownership or performance. Full validation stays in hosted CI.

For `blk.0.ffn_up.weight`, K=5120/M=17408/N=1, 32-row tiles and four Q8_K groups
per column tile, the derived ledger is:

| Storage or traffic | Bytes |
| --- | ---: |
| Borrowed original PQ2 weights | 23,674,880 |
| Prepared serialized Q8_K activations, once | 5,840 |
| Private complete f32 output | 69,632 |
| One packed CPU weight staging tile | 8,704 |
| Proposed device double weight stages + activation tile + row sums, 64-aligned | 18,752 |
| Proposed complete weight transfer, without repeated reads | 23,674,880 |
| Proposed activation transfer, once per 32-row tile | 3,176,960 |
| Proposed complete output return | 69,632 |

The CPU uses one staging tile; proposed device double buffering and DMA remain
unimplemented. Activation traffic charges every row tile without assumed
residency. Add descriptors, queues, mappings, allocator/stack and synchronization
costs, including resources outside Job commit. Observed VTCM page size does not
establish allocation. `output.clear()` retains caller capacity: charge it beside
the private pending result. Existing F32 input/weights, optional transform vector
(20,480 bytes at K5120), sign arrays and allocator retention are separate charges.

Hosted ARM64 CI owns full controls:

```powershell
pwsh -NoProfile -File tools/hexagon/verify_packed_fixture.ps1
rustc --edition=2024 --test -D warnings -O tools/hexagon/packed_projection.rs -o ABSOLUTE_EXTERNAL_BUILD/packed-projection-tests.exe
& ABSOLUTE_EXTERNAL_BUILD/packed-projection-tests.exe
```

Controls cover independent native preparation/FMA captures, full-range packed/i8
arithmetic, scales/ties, awkward/unaligned tiles, reduction carry, malformed input,
cancellation and atomic publication. The expanded reference shares quantization,
half expansion and FMA routines; it does not independently qualify them. Walsh
coordinate-37 impulse checks use `popcount(i & 37)` parity to bind interior order.
Artifact consumption, signed execution, state/adapters and whole-request benefit
retain the [Hexagon qualification gates](../../docs/hexagon.md#qualification-evidence).

## Proposed signed-kernel tile protocol

[`packed_tiles.rs`](packed_tiles.rs) defines an application-owned version 1
payload contract and a strict **CPU-only** receiver. This is not a Qualcomm
RPC ABI or a supported operation in the signed GenieX bundle. A future separately
reviewed and signed packed kernel must implement the protocol before device
submission can use it. No native submission or asynchronous buffer lifetime is
implemented by this control.

Each little-endian payload consists of a 64-byte header, complete original PQ2
row fragments and complete pinned prepared Q8_K records. The eight-byte magic is
`PQ2Q8K01`. Fourteen u32 fields follow: version (1), header bytes (64), projection
columns and rows, maximum tile rows and Q8_K groups, starting row and group,
active rows and groups, zero-based sequence, flags, packed weight bytes and
activation bytes. Flags are bit 0 for the first column tile of a row tile, bit 1
for its last column tile and bit 2 for the final projection tile. Other bits
reject. Packed weights remain row-major; each Q8_K group consumes two original
PQ2 blocks. Q8_K records follow weights with their original scale, 256 codes and
sixteen signed i16 sums. No native struct layout or padding crosses this boundary.

`TileSource` prepares activations once and lends each payload from one source-owned
staging vector. The slice prevents source reuse or destruction during synchronous
consumption; asynchronous device consumers still need independently verified
ownership through completion. Its input has already passed the graph's CPU transform.
The receiver derives the exact expected tile from the admitted geometry and
sequence, compares every header byte and checks exact payload length before
accessing records. Unknown versions, changed flags or geometry, duplicates,
reordering, truncation and trailing bytes reject. It verifies finite scales and
Q8_K sums, retains one CPU copy of prepared activations to reject changes across
row tiles, and carries per-row FMA reduction across column tiles. Any arithmetic,
protocol or cancellation error terminally discards provisional output. The
consuming final commit requires every tile and a final cancellation/deadline
check. No partial result is exposed.

For the first projection's 32-row/four-group schedule, there are 2720 payloads,
each at most **9936 bytes**, with **174,080 header bytes** and **27,025,920 total
payload bytes**. Those totals include repeated activation traffic and the
original 23,674,880 packed weight bytes. Vendor RPC, queue and DMA-descriptor
framing, padding, mapping and output-return costs are additional. These are
derived wire extents, not observed transfers. CPU replay additionally retains
5840 bytes of activation identity, its complete private output and caller-owned
staging capacity. It does not change the proposed VTCM reservation or prove
device-resident activation reuse.

The session owner separately authenticates artifact/sign/input identity, kernel
signature, geometry, mappings and resources. CPU corruption/sequence/cancellation
controls do not establish device completion or asynchronous buffer ownership.

## Actual artifact extraction candidate

[`packed_artifact.rs`](packed_artifact.rs) is a narrow Rust export tool using
the existing `bonsai_gguf::VerifiedModel` API from exact Prism revision
`7aca2251f09acb345d4afb53d5ce411f81017745`. It adds no GGUF parser, inference
dependency or alternate model admission. Hosted preparation verifies that
source checkout and builds the exact extractor as a temporary Cargo example,
using its `bonsai_gguf` and `sha2` dependencies with default features disabled.
Tracked dependency source remains unchanged; Cargo owns metadata/link resolution. The tool's
model-free `--self-test` checks width selection, complete-array sign validation
and packed-scale rejection without opening a model or device. Building or
executing the candidate alone does not establish actual artifact evidence.

The only extraction invocation is:

```powershell
& ABSOLUTE_HOSTED_BUILD/hexagon-packed-artifact.exe --model ABSOLUTE_SWIFT_GGUF --output-directory NEW_EXTERNAL_DIRECTORY
```

An owned supervisor must enforce the remaining aggregate owner-device deadline
around the entire invocation. The command hashes all **7,206,168,928 bytes** of
the fixed Swift artifact against SHA-256
`5912bb739217cf25b4283a098c7b893d9baff36bdf2e72d3dbf4cb999d5633d5`.
There is no cached verification receipt or caller-supplied identity override.
The verified handle denies writes/deletion on Windows and remains alive through
descriptor, metadata and payload access. Parsing uses the existing bounded
reader with a 128 MiB retained-header allowance. Payload retention adds the
23,674,880-byte projection and a 5120-byte extracted sign vector; parser/hash
buffers and allocator overhead remain separate charges.

The command requires original PQ2 type 142 and `[5120,17408]` geometry for
`blk.0.ffn_up.weight`, checks all raw half scales and retains original packed
bytes. It requires the exact version-1 forward input transform declaration,
block size 1024, explicit signs and widths `[5120,6144,17408]`. All 28,672 sign
values must be integer +1/-1 and their canonical little-endian i32 SHA-256 must
match Prism's existing `3d1ea9136a6044cd680cc1e92cde65e9dd7238e1288fe2effabec5d201c48bed`
control. The first 5120 entries are therefore the exact width-5120 sign vector,
exported losslessly as two's-complement i8 bytes. It does not generate signs,
execute the transform or infer a basis from dimensions alone.

The new external directory receives `weights.pq2`, `signs.i8` and an at-most
8192-byte `binding.json` recording the full artifact, library revision, extractor
source digest, separate output digests, geometry and transform/sign definition.
Existing directories, linked/reparse storage and Git ancestors reject. Files
use exclusive creation and remain held against mutation through final manifest
creation. Failures can retain bounded partial files; only a successful owned
process receipt plus independently verified output digests permits intake.
The tool reads no other tensor and writes no complete-model conversion.

The [extraction binding](../../docs/evidence/hexagon-swift-projection-binding.json)
provides independent original-weight/sign pins. It does not authenticate
activations or qualify consumption, full state/adapters or signed execution.
Full-file verification must fit one aggregate allowance; never split it across runs.

## Conditional signed Q2 integer primitive

[`q2_integer.rs`](q2_integer.rs) adds a **CPU-only** operation control for using
the Q2_0/Q8_1 arithmetic in the newer signed-package lead as an integer-dot
primitive. Its source identity is llama.cpp
[`19cce6919992fdffd92e0ac152caaf1e2b8f254e`](https://github.com/ggml-org/llama.cpp/tree/19cce6919992fdffd92e0ac152caaf1e2b8f254e),
bundled by GenieX v0.10.0. That source is separate from the v0.8.0 access pin.
Catalog membership and execution of that exact signed binary require separate controls.

The source's
[Q2 dot](https://github.com/ggml-org/llama.cpp/blob/19cce6919992fdffd92e0ac152caaf1e2b8f254e/ggml/src/ggml-hexagon/htp/hvx-mm-kernels-tiled.h#L1543-L1567)
multiplies unsigned two-bit symbols and subtracts two Q8_1 32-code block sums
per 64-weight block. Both activation scales and sums are stored as FP16. Its
[ordinary activation path](https://github.com/ggml-org/llama.cpp/blob/19cce6919992fdffd92e0ac152caaf1e2b8f254e/ggml/src/ggml-hexagon/htp/matmul-ops.c#L681-L704)
quantizes F32 inputs internally. No supported prepared-Q8_1 upload is assumed.
Simply converting original full-range Q8_K codes to scale-one Q8_1 fails to prove
parity: some 32-code sums exceed 2048 and lose odd integer bits in FP16.

The conditional adapter keeps the original CPU Q8_K quantizer and its scale.
For each original PQ2 128-weight block it performs the following bounded work:

1. Split each already prepared i8 code `q` into integer `q / 2` (truncating
   toward zero) and `q - q / 2`. Both limbs lie within -64 through +64, with no
   quantization, discarded bits or alternative activation scale.
2. Preserve every original two-bit symbol. Place at most 31 real lanes in each
   32-lane activation group and add one guard with zero weight (symbol 1).
   Choose the guard as -127 for a nonnegative real-code sum, otherwise +127.
   Pad to K=256 with zero-weight lanes and a guard in every group. Each complete
   group has maximum absolute value exactly 127 and sum magnitude at most 1857.
3. Use logical Q2 scales of one for all four 64-weight blocks and two F32 rows
   for the limbs. The intended Q8_1 scale/inverse is one. All input integers and
   complete block sums are exactly representable in FP16. Each zero-weight guard
   cancels through the unsigned-dot-minus-sum expression; all integer-dot and
   F32 addition intermediates remain well inside the exact 24-bit integer range.
4. Return the two integer dots for that original 128-weight block, add them
   exactly on CPU, and apply `(original_half_scale * original_Q8_K_scale).mul_add`
   into the original CPU accumulator in original block order. No original scale
   or complete projection reduction is delegated to a different floating order.

The actual
[quantizer](https://github.com/ggml-org/llama.cpp/blob/19cce6919992fdffd92e0ac152caaf1e2b8f254e/ggml/src/ggml-hexagon/htp/hvx-mm-kernels-tiled.h#L1-L138)
converts inputs/maxima to FP16, forms its half scale using reciprocal-127 bits
`0x2008`, calls a polynomial FP16 inverse and uses HVX rounding/conversion
intrinsics. For maximum 127, the mathematical half-scale product is
0.99993896484375, which rounds to half one; the separate F32 scale used for sums
also rounds to one. **The CPU control does not emulate or qualify the vendor
inverse polynomial and conversion intrinsics.** Exact guarded input codes,
half scales, half offsets and final integer outputs must be independently checked
against the signed binary before this becomes an exact device adapter.

Source
[shape checks](https://github.com/ggml-org/llama.cpp/blob/19cce6919992fdffd92e0ac152caaf1e2b8f254e/ggml/src/ggml-hexagon/htp-kparams.cpp#L971-L995)
admit contiguous ordinary Q2 weights with K divisible by 64, F32/F16 activation
and F32 output, subject to later solver/resource checks. K=256 also respects the
activation quantizer's 128-element padding. N=2 is a proposed HVX case: the source
HMX condition excludes N at or below four. This does not prove that a selected
signed deployment admits the constructed operation or that native Rust can
submit its supported entry correctly.

The block control exposes ordinary logical Q2 records and F32 rows. The separate
[`q2_projection.rs`](q2_projection.rs) preparation/reduction API packs the
source-native host layout and accepts a synchronous submitter. Each host tile
for 32 rows and K=64 is **576 bytes**: 512 symbol bytes and 64 scale bytes. DMA
copies those 576 bytes into a **640-byte aligned VTCM destination**; host tiles
are not padded to that destination stride. The K=256 primitive therefore has
2304 host weight bytes and 2560 VTCM weight bytes per 32 rows, before additional
kernel scratch. Activation inputs occupy 2048 F32 bytes, with eight returned
limb bytes per original row. Full first-projection derived costs are:

| Proposed schedule cost | M=32 tiles | Full M=17408 |
| --- | ---: | ---: |
| Operations preserving original per-128 reduction | 21,760 | 40 |
| Repacked host weight traffic | 50,135,040 bytes | 50,135,040 bytes |
| F32 activation traffic without assumed reuse | 44,564,480 bytes | 81,920 bytes |
| Returned F32 limb dots | 5,570,560 bytes | 5,570,560 bytes |
| Complete CPU output | 69,632 bytes | 69,632 bytes |

These are conservative derived extents, not observed transfers, native request
counts or admitted memory. The full-M plan bounds its owned payload at 1,470,160
bytes, including 1,253,376 host staging bytes, original Q8_K preparation, guarded
activation rows, limb outputs and the private final output. The submitter must
independently qualify solver support and actual completion/ownership. Reuse
or residency cannot reduce the charged traffic without matching ownership and
device evidence. Charge internal Q8_1 quantization, padded activation tiles,
output padding, original scales, DMA, queues, mappings, synchronization and
resources outside the Job separately. The construction uses bounded temporary
packing and never justifies a persistent expanded whole-model copy.

Model-free controls cover packed/i8 bounds, guards, scale-one records, native FMA,
576-/640-byte layouts, full-projection replay, awkward tails and failed/poisoned/
cancelled submissions. The conditional primitive needs independently qualified
signed quantizer/integer results and lifetime/resource acceptance before use;
extra work and traffic may erase any benefit.

## Artifact-bound signed projection diagnostic

[`projection_probe.rs`](projection_probe.rs) connects the original artifact's
verified extraction to the native Rust signed-Q2 submitter. It supports one
selected prefix of `blk.0.ffn_up.weight`, K=5120 and M divisible by 32 up to
17408. One full-M operation per original 128-column block needs 40 submissions;
a 32-row schedule needs 21,760. The signed boundary keeps K=256/N=2 and admits
only M divisible by 32. CPU-only awkward-tail controls remain separate.

The request is a UTF-8 JSON file of at most 8192 bytes. Its complete schema is:

```json
{
  "bindingDirectory": "C:\\external\\swift-projection",
  "activationFile": "C:\\external\\activation.f32le",
  "activationSha256": "EXACT_64_LOWERCASE_HEXADECIMAL_DIGITS",
  "outputDirectory": "C:\\external\\new-projection-result",
  "rows": 17408,
  "tileRows": 17408,
  "maximumSeconds": 120,
  "execution": "device-guarded-q2",
  "packageDirectory": "C:\\external\\verified-signed-package"
}
```

The binding directory contains `binding.json`, `weights.pq2` and `signs.i8`
from the [verified extraction](#actual-artifact-extraction-candidate). The
complete 23,674,880-byte projection and 5120-byte sign vector must match the
independent immutable pins retained in
[the extraction evidence](../../docs/evidence/hexagon-swift-projection-binding.json).
The diagnostic validates the manifest's original artifact, shape, representation
and transform declarations; it never accepts replacement weight/sign digests
from the caller. Files remain open against writes and deletion on Windows
through result publication.

The activation file contains exactly 5120 finite IEEE F32 values in little-endian
order, in the input-last-dimension basis **before** the original Hadamard signs
and rotation. The caller supplies its SHA-256. That digest identifies bytes;
it does not authenticate their origin or qualify actual-model activations. The
CPU applies the exact extracted signs/normalized 1024-wide transform, original
Q8_K preparation and original ordered per-128-block FMA. Every returned limb is
also checked against an independent CPU decode of the transmitted tiles, and
the final selected projection must match the original PQ2 CPU oracle bitwise.

The device path requires the trusted Runner supervisor's source-bound hosted
bundle, seventeen runtime-file and five catalog-member checks, retained driver
identity, exact child-only package search directory, aggregate process deadline
and verified owned-process cleanup. Its native boundary keeps one session,
queue and mapped allocation across the bounded synchronous operation stream.
Only a complete correlated response permits reuse; uncertain submission,
correlation, numerical, callback or deadline failure pins all uncertain owners
until supervised process death. Native stop, queue close, unmap, module close
and free must complete before the diagnostic publishes a projection result.
The API requests the reviewed reference DSP process class; Windows package
catalog checks remain mandatory and no OS trust settings change.

`maximumSeconds` is an absolute allowance from diagnostic entry, including input
verification and the CPU controls. Device requests permit 4 through 120 seconds;
the outer supervisor still owns the single five-minute aggregate owner budget
and cleanup reserve. Smaller prefix-row trials are explicit, and cannot be
stitched together into full-model qualification. Full controls stay in hosted
CI. Ordinary startup and chat never run this diagnostic.

For `execution: "cpu-control"`, omit `packageDirectory`; the same request can
replay the guarded schedule on CPU without opening the driver. This mode allows
1 through 225 seconds and supplies no device evidence. `--self-test` performs
small model-free parser/identity controls. `--inspect` emits the installed driver
path without loading it, for the trusted supervisor's independent verification.

A sibling `.pending` directory stages the complete output; its atomic rename
publishes the new external result directory and `receipt.json` together. A
passed attempt also contains `result.f32le`, bound by exact length and SHA-256
in that receipt; a failed attempt publishes no projection result. The receipt
separates source identities, externally supplied activation bytes, conservative
payload/transfer bounds, submitted/completed/checked operation counts, numerical
parity and explicit native cleanup. These diagnostic comparisons do not measure
inference speed. Actual activation provenance, logits, complete state, adapters,
selected DSP bytes, complete vendor teardown, performance and production
admission remain independent gates; every receipt keeps `profileEligible:false`.

## HMX integer-bitplane preparation

[`hmx_projection.rs`](hmx_projection.rs) provides a separate pure Rust packing
and reduction control for source-eligible K=128/N=8 HMX operations. It preserves
the original PQ2 symbols, prepares the pinned Q8_K codes once and splits each
signed code into eight binary activation planes with coefficients
`[1, 2, 4, 8, 16, 32, 64, -128]`. Two tightly packed 576-byte host weight tiles
cover each 32-row operation tile. Every returned plane must be finite, integral
and between -128 and 256 before reconstruction and the original per-128 CPU FMA.
Failed submission, malformed output or cancellation leaves no partial projection.

The full first-projection plan uses forty operations with 626,688 bytes of peak
weight staging and 1,263,312 bytes of owned host payload. Without assumed reuse,
its derived transfers are 25,067,520 weight bytes, 163,840 activation bytes and
22,282,240 returned plane bytes. These exclude device FP16 expansion, N=8-to-32
padding, VTCM, DMA, mappings, queue storage and allocator/stack overhead.

The submitter is injected; this control supplies no native HMX session/packet.
The signed projection diagnostic uses guarded HVX. Hosted receipts bind HMX
source and model-free controls; signed integer parity, effective placement,
full state/adapters and benefit retain the
[separate HMX gates](../../docs/hexagon-source-audit.md#packed-arithmetic-controls).
