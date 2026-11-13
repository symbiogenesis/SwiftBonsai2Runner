# Adapter authoring evidence intake

`qualification` operation `basis-intake` verifies bounded evidence and compares
all 48 shipped SSM A vectors. It loads no checkpoint and runs no inference.
The [LoRA guide](swift-lora.md#structural-compatibility-and-unresolved-basis)
defines the unresolved writer-basis gate.

## Intake envelope

Supply absolute external `evidenceDirectory` and `manifestPath` plus an
independently selected raw-file `manifestSha256`. The schema-2 envelope contains
`modelSha256` (current PTQ1 export), `runtimeCommit`, `adapter`,
`checkpointManifest`, `conversionManifest`, `toolchainManifest`, `direction`,
nonempty `normalization`, `mapping`, `absoluteTolerance`, `relativeTolerance`
and `ssmSites`. Only schema 2 is accepted.

Artifact references are `{ "file": "NAME", "bytes": LENGTH, "sha256": "SHA256" }`.
Files must be individual ordinary files in the evidence directory; lengths and
lowercase SHA-256 digests are verified. Source records use
`{ "repository": "OWNER/REPOSITORY", "revision": "IMMUTABLE_REVISION" }` with a
complete lowercase 40- or 64-digit revision.

`mapping.nativeToAuthoring` permutes all 6144 channels; `mapping.signs` contains
6144 values of -1 or 1. The equation is
`native[c] = signs[c] * authoring[nativeToAuthoring[c]]`. Other transforms are
unsupported. `ssmSites` supplies exactly 48 `{ "block": N, "authoringA": REF }`
records for blocks 0..63 excluding 3, 7, ... 63. Each A file holds 6144 finite
little-endian F32 values. Intake compares every mapped channel against the named
`blk.N.ssm_out.weight.lora_a` payload in the approved, digest-verified adapter.
Both tolerances are at most 1e-5; the comparison is
`abs(actual - expected) <= absoluteTolerance + relativeTolerance * abs(expected)`,
with the bound evaluated using F64 fused multiply-add. Retain each vector's
digest, mismatch count and maximum absolute difference.

The three authoring manifests below are strict schema-1 JSON objects; unknown
fields reject. Describe the actual original process. Declarations and later
reconstructions cannot authenticate missing original inputs or invocations.

## Checkpoint manifest

| Field | Contract |
| --- | --- |
| `schemaVersion` | `1` |
| `source` | Original unfolded checkpoint repository and immutable revision. |
| `index` | Artifact reference to the original `model.safetensors.index.json` retained in the evidence directory. At most 1 MiB; its actual bytes and digest are verified. |
| `shards` | Complete distinct safetensors shard artifact references named by the index, at most 256, each at most 64 GiB. Shard declarations are checked, but shard payloads are not opened or hashed by intake. |
| `ssmTensors` | Exactly 48 records, one for every block 0..63 except 3, 7, ... 63. |

Each SSM record contains `block`, `name`, `shard`, `dtype`, `shape`, `bytes` and
`sha256`. The original tensor name is
`model.language_model.layers.N.linear_attn.out_proj.weight`, shape `[5120, 6144]`.
Supported original storage is `F16`, `BF16` or `F32`; declared tensor bytes must
equal the shape times the dtype width. All declared SSM payloads assigned to one
shard must collectively fit its declared size, with checked addition. The tensor
digest identifies raw original stored tensor bytes, without a safetensors header.
Independent reproduction must document any conversion to F32.

The index uses the standard `weight_map` plus optional `metadata` fields, at most
8192 tensor names and 64 metadata entries. Duplicate raw tensor-map or metadata
keys reject rather than silently replacing an earlier entry. Every indexed shard must be declared,
every declared shard must be indexed, and all 48 original SSM names must resolve
to their declared shard. Checking this relationship does not verify payload
offsets, tensor bytes or the checkpoint's authenticity.

## Conversion manifest

| Field | Contract |
| --- | --- |
| `schemaVersion` | `1` |
| `exporter` | `source`, repository-relative `path`, raw source-file `bytes` and `sha256`. Source-file declarations are bounded to 1 MiB; source bytes are not fetched. |
| `invocation` | Exact nonempty original argv array, at most 128 arguments of at most 4096 UTF-16 units each. |
| `checkpointManifestSha256` | Must equal the selected checkpoint manifest's raw-file digest. |
| `toolchainManifestSha256` | Must equal the selected toolchain manifest's raw-file digest. |
| `directionSha256` | Must equal the actual selected direction artifact's raw-file digest. |
| `outputAdapterSha256` | Must equal the approved adapter digest from the envelope. |
| `baseModel` | Artifact declaration for the actual base GGUF used by the original invocation. This need not be the later Swift export. Its bytes are not loaded by intake. |
| `normalization` | Exact same nonempty normalization description as the envelope. |
| `preprocessing` | Explicit nonempty ordered records of original unfolding, reordering, transforms/signs or an explicit declaration that none occurred; at most 64 records of 4096 UTF-16 units each. |

Do not rewrite an original direction digest to match the selected published
direction. A mismatch is evidence requiring investigation, not an intake success.

## Toolchain manifest

`schemaVersion` is `1`. Required nonempty text fields are `python`, `numpy`,
`safetensors`, `ggufWriter` and `matrixMultiplication`, each at most 4096 UTF-16
units. Retain exact original build/version/source identities and the F32 matrix
multiplication implementation, including the BLAS provider and build when used.
An unrecorded original version remains missing evidence; a guessed value cannot
qualify provenance.

## Receipt interpretation

Successful intake reports `authoringManifestConsistency.status` as
`declared-records-consistent`, the actual index digest and declared shard/site
counts, before comparing every independently supplied A vector. It keeps
`originalCheckpointPayloadsVerified`, `authoringProvenanceQualified`,
`aFactorReproductionQualified`, `writerBasisQualified` and `defaultMayChange`
false. This is an integrity and consistency operation, without tensor execution
or implicit promotion. Independent original-input authentication, reproduction
and native writer validation still own their respective qualification decisions.
