# Independent public-pack authoring reference

The immutable public pack supplies a concrete reconstruction candidate for all
48 SSM A vectors. This is source preparation, with no successful reconstruction
claimed. The existing authenticated-original-input intake remains separate.

The publisher describes its evaluation checkpoint as an
[FP16 expansion of the packed ternary representation](https://github.com/Continuum-AI-Corp/OrcaBonsai-27B-Uncensored/blob/703028bb7fba03496e796b144839b161cf0f5409/README.md#L237-L241)
and says that the [unfolded checkpoint is not shipped](https://github.com/Continuum-AI-Corp/OrcaBonsai-27B-Uncensored/blob/703028bb7fba03496e796b144839b161cf0f5409/README.md#L680-L683).
Those facts permit an independent public reconstruction attempt; they do not
authenticate the author's original checkpoint bytes or conversion invocation.

`tools/native-reference/public_pack_reference.ps1` and the dependency-free Rust
`bonsai-reference-numerics` helper implement exactly one declared recipe,
`public-mlx-f16-inverse1024-f16-rust-scalar-f32-dot-v2`, before seeing payload results.
Schema-2 packets bind this exact arithmetic identity.

1. Verify the complete immutable 8,595,477,990-byte public MLX safetensors file's
   SHA-256 before reading its bounded header, then rehash a read-only handle held
   throughout numerical reconstruction. Require distinct tensor names, exact
   nonoverlapping extents, canonical 48 locations, U32 weights `[5120,384]`, F16
   scales/biases `[5120,48]`, binary signs `[6144]` and declared grouped layout.
2. Decode little-endian two-bit lanes with group128 affine arithmetic. Require
   codes 0, 1 or 2, exact raw F16 `bias=-scale`, finite `abs(scale)<=32752`.
   Evaluate in F16, then widen to F32. The immutable MLX
   [dtype selection](https://github.com/ml-explore/mlx/blob/7a1d4f5c12ac82f4b4d0a6e71538d89ca0605247/mlx/ops.cpp#L5105)
   and [affine kernel](https://github.com/ml-explore/mlx/blob/7a1d4f5c12ac82f4b4d0a6e71538d89ca0605247/mlx/backend/metal/kernels/quantized.h#L2701-L2760)
   support the F16 output stage. In this restricted ternary domain, products and
   sums are exactly representable; fused versus split affine evaluation cannot
   select a different numeric value. The fixtures check every accepted F16 scale,
   including signed zero, against an independent FMA oracle. Code3 is rejected.
3. Apply F32 increasing-span block1024 Hadamard butterflies, normalization 1/32,
   then signs, following the public [inverse transform](https://huggingface.co/prism-ml/Ternary-Bonsai-2-27B-mlx-2bit/blob/fcba37d2117a7077eac6b613b2668d14d9779edd/runtime/runtime.py#L16-L28)
   and pinned MLX [CPU transform](https://github.com/ml-explore/mlx/blob/7a1d4f5c12ac82f4b4d0a6e71538d89ca0605247/mlx/backend/cpu/hadamard.cpp).
   Round the reconstructed matrix to F16 storage, then widen to F32. This final
   storage round and the exact transform/reduction order are explicit candidate
   assumptions; the public exporter does not identify the original serialization,
   MLX version or BLAS build. No alternative precision or channel permutation runs.
4. Verify the approved adapter and selected public direction against repository
   locks. Normalize that F32 direction using a sequential F64 sum/square root
   rounded once to F32, and check all 129 B factors separately.
   Compute F32 `r @ W`, as in the publisher's
   [A/B export](https://github.com/Continuum-AI-Corp/OrcaBonsai-27B-Uncensored/blob/703028bb7fba03496e796b144839b161cf0f5409/scripts/export_gguf_lora.py#L254-L269),
   with a sequential F32 multiply/add reduction without FMA in the pinned Rust
   helper. Compare every one of
   all 48 A vectors under the unchanged `1e-5 + 1e-5 * abs(reference)` bound. Finish
   all numeric comparisons and retain disagreements; never search variants or
   enlarge a tolerance after observing results.

The [hosted workflow](../.github/workflows/ptq1-public-authoring-reference.yml)
runs only on GitHub-hosted `windows-11-arm`, main, with no owner runner label.
Relevant source pushes and default `run_reconstruction: false` dispatches build
the Rust helper and check small independent math/parser/admission
fixtures, retaining a separate preparation receipt. Set the dispatch boolean true
for the one full public-input attempt. The auditor also enforces its lane and an
absolute 40-minute deadline between bounded operations. It imports no downloaded model code,
instantiates no model, executes no inference and writes no full checkpoint.
Weights stay in external runner-temp storage and are excluded from uploads.

Retained evidence includes exact source/input identities, PowerShell/compiler
and helper source/executable identities, the declared normalization/reduction,
per-site input and candidate-F16 matrix
hashes, the bounded raw model header, actual per-site dtype/shape/absolute byte
extents, all 48 independently produced little-endian F32 A files, shipped-factor
hashes and complete metrics. A failure retains its stage and partial results.
Archive and independently review the exact raw output before artifact expiration.

The producer also retains `replay-source/source-manifest.json` and seven selected
raw source files: the producer, format reader, independent auditor, recipe pins
and approved adapter/direction locks, plus the Rust numerical source. Its terminal
report binds the manifest's SHA-256. The upload includes that finite source subtree,
so later checkout line endings do not destroy replayability. Validation failures retain a bounded fixed
reason; arbitrary HTTP, OS and library exception text is excluded.

`tools/native-reference/audit_public_reference.ps1` independently replays the
retained comparisons with PowerShell 7.6. It verifies the complete
approved adapter and direction bytes, producer scripts, selected source/run,
raw configuration/header, exact all-48 inventory and source tensor descriptors,
every retained A file and all 129 B factors. Its separate GGUF reader and scalar
comparison loop check every reported metric under the unchanged bounds. Valid
losing results remain losing results. A missing complete result cannot pass.
The hosted workflow retains this replay after a complete match or disagreement.

An independent reviewer can repeat it without a Python runtime, a model download or another
model workload. The current trusted auditor automatically checks the retained
source bundle as data, including its exact finite inventory, types, lengths and
digests; it executes none of the archived producer code. Parsed retained recipe
pins must equal the reviewer's independently selected recipe, and adapter/direction
locks and payload identities still need to match. Require the retained source bundle and matching raw producer bytes, including
line endings:

```text
pwsh -NoLogo -NoProfile -File tools/native-reference/audit_public_reference.ps1 -EvidenceDirectory ABSOLUTE_RETAINED_DIRECTORY -Adapter VERIFIED_APPROVED_ADAPTER -Direction VERIFIED_PUBLIC_DIRECTION -ExpectedSourceRevision FULL_REVISION -ExpectedRunId RUN_ID -ExpectedRunAttempt ATTEMPT
```

Select the expected producer from separately authenticated artifact records;
copying those fields from an untrusted report does not authenticate the producer.
The replay does not reopen the public model pack or independently reproduce the
matrix reconstruction. Input tensor and candidate-matrix digests remain
declarations tied to that separately reviewed producer. Automatic scalar replay
is evidence for the independent review and does not itself perform that review.

A complete all 48 match would establish reproducibility of this public-input
candidate at the fixed bound. It would leave authenticated original-run
provenance, authoring-to-native writer mappings and residual captures,
loaded-zero/nonzero logits/state, human quality, performance and promotion gates
pending. The report keeps those qualification/default flags false even on a
numeric match. Owner-device execution is outside this hosted reconstruction lane.

Reproduce only through GitHub-hosted CI:

```text
gh workflow run ptq1-public-authoring-reference.yml --ref main -f run_reconstruction=false
gh workflow run ptq1-public-authoring-reference.yml --ref main -f run_reconstruction=true
```
