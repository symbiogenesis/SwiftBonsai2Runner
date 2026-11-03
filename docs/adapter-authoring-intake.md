# Adapter authoring record contract

`qualification` operation `basis-intake` uses a schema-2 envelope described in
[the adapter guide](swift-lora.md#authoring-evidence-intake). Its three authoring
manifests are schema-1 JSON objects with the exact fields below; unknown fields
reject. Records describe the actual original authoring process. Do not substitute
a later reconstruction for a missing original identity.

All artifact references use `{ "file": "NAME", "bytes": LENGTH, "sha256": "SHA256" }`.
Names are individual filenames; digests are lowercase SHA-256. `source` records
use `{ "repository": "OWNER/REPOSITORY", "revision": "IMMUTABLE_REVISION" }`,
with a complete lowercase 40- or 64-digit revision, never a moving branch or tag.
These are declarations for independent authentication, not proof of source access.

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
equal the shape times the dtype width and fit the declared shard. The tensor
digest identifies raw original stored tensor bytes, without a safetensors header.
Independent reproduction must document any conversion to F32.

The index uses the standard `weight_map` plus optional `metadata` fields, at most
8192 tensor names and 64 metadata entries. Every indexed shard must be declared,
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
