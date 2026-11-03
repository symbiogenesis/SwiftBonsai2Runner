//! Bounded consistency checks for declared original authoring records.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};

use super::{Artifact, Intake, MANIFEST_LIMIT, artifact, hash, require, string};
use crate::Result;
use crate::identity::is_lower_hex;

const MAX_SHARDS: usize = 256;
const MAX_INDEX_TENSORS: usize = 8192;
const MAX_SHARD_BYTES: u64 = 64 * 1024 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Source {
    repository: String,
    revision: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Checkpoint {
    schema_version: u32,
    source: Source,
    index: Artifact,
    shards: Vec<Artifact>,
    ssm_tensors: Vec<Tensor>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Tensor {
    block: u32,
    name: String,
    shard: String,
    dtype: String,
    shape: [u64; 2],
    bytes: u64,
    sha256: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Exporter {
    source: Source,
    path: String,
    bytes: u64,
    sha256: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Conversion {
    schema_version: u32,
    exporter: Exporter,
    invocation: Vec<String>,
    checkpoint_manifest_sha256: String,
    toolchain_manifest_sha256: String,
    direction_sha256: String,
    output_adapter_sha256: String,
    base_model: Artifact,
    normalization: String,
    preprocessing: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Toolchain {
    schema_version: u32,
    python: String,
    numpy: String,
    safetensors: String,
    gguf_writer: String,
    matrix_multiplication: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Index {
    #[serde(default, deserialize_with = "unique_metadata")]
    metadata: Option<BTreeMap<String, Value>>,
    #[serde(deserialize_with = "unique_map")]
    weight_map: BTreeMap<String, String>,
}

fn unique_map<'de, D, T>(deserializer: D) -> std::result::Result<BTreeMap<String, T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct Unique<T>(std::marker::PhantomData<T>);
    impl<'de, T: Deserialize<'de>> serde::de::Visitor<'de> for Unique<T> {
        type Value = BTreeMap<String, T>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("an object with distinct keys")
        }

        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            mut access: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            let mut values = BTreeMap::new();
            while let Some((key, value)) = access.next_entry::<String, T>()? {
                if values.insert(key, value).is_some() {
                    return Err(serde::de::Error::custom(
                        "Duplicate original checkpoint index key.",
                    ));
                }
            }
            Ok(values)
        }
    }
    deserializer.deserialize_map(Unique(std::marker::PhantomData))
}

fn unique_metadata<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<BTreeMap<String, Value>>, D::Error>
where
    D: Deserializer<'de>,
{
    unique_map(deserializer).map(Some)
}

fn source(source: &Source) -> Result<()> {
    string(&source.repository, 512)?;
    require(
        is_lower_hex(&source.revision, 40) || is_lower_hex(&source.revision, 64),
        "Authoring sources require an immutable lowercase 40/64-digit revision.",
    )
}

fn filename(name: &str) -> Result<()> {
    string(name, 128)?;
    require(
        name != "." && name != ".." && !name.contains([':', '/', '\\', '\0']),
        "Authoring file references must be individual filenames.",
    )
}

fn declared_artifact(reference: &Artifact, maximum: u64) -> Result<()> {
    filename(&reference.file)?;
    hash(&reference.sha256)?;
    require(
        (1..=maximum).contains(&reference.bytes),
        "Declared authoring artifact has an invalid byte length.",
    )
}

fn checkpoint(checkpoint: &Checkpoint, index: &Index) -> Result<()> {
    require(
        checkpoint.schema_version == 1,
        "Unknown checkpoint manifest schema.",
    )?;
    source(&checkpoint.source)?;
    declared_artifact(&checkpoint.index, MANIFEST_LIMIT)?;
    require(
        !checkpoint.shards.is_empty() && checkpoint.shards.len() <= MAX_SHARDS,
        "Checkpoint manifest requires a bounded nonempty shard inventory.",
    )?;
    let mut shards = BTreeMap::new();
    for shard in &checkpoint.shards {
        declared_artifact(shard, MAX_SHARD_BYTES)?;
        require(
            shard.file.ends_with(".safetensors")
                && shards.insert(shard.file.as_str(), shard.bytes).is_none(),
            "Checkpoint shards must be distinct safetensors files.",
        )?;
    }
    require(
        !index.weight_map.is_empty()
            && index.weight_map.len() <= MAX_INDEX_TENSORS
            && index
                .metadata
                .as_ref()
                .is_none_or(|metadata| metadata.len() <= 64),
        "Checkpoint index exceeds its bounded tensor/metadata inventory.",
    )?;
    let mut indexed_shards = BTreeSet::new();
    for (name, shard) in &index.weight_map {
        string(name, 512)?;
        require(
            shards.contains_key(shard.as_str()),
            "Index names an undeclared checkpoint shard.",
        )?;
        indexed_shards.insert(shard.as_str());
    }
    require(
        indexed_shards == shards.keys().copied().collect(),
        "Checkpoint shard inventory differs from its original index.",
    )?;
    let mut blocks = BTreeSet::new();
    let mut declared_bytes = BTreeMap::<&str, u64>::new();
    for tensor in &checkpoint.ssm_tensors {
        let name = format!(
            "model.language_model.layers.{}.linear_attn.out_proj.weight",
            tensor.block
        );
        let width = match tensor.dtype.as_str() {
            "F16" | "BF16" => 2,
            "F32" => 4,
            _ => return Err("Original SSM tensor dtype must be F16, BF16 or F32.".into()),
        };
        hash(&tensor.sha256)?;
        require(
            tensor.name == name
                && tensor.shape == [5120, 6144]
                && tensor.bytes == 5120 * 6144 * width
                && shards
                    .get(tensor.shard.as_str())
                    .is_some_and(|bytes| *bytes >= tensor.bytes)
                && index.weight_map.get(&tensor.name) == Some(&tensor.shard)
                && blocks.insert(tensor.block),
            "Original SSM tensor name, extent, shard or index mapping is inconsistent.",
        )?;
        let total = declared_bytes.entry(tensor.shard.as_str()).or_default();
        *total = total
            .checked_add(tensor.bytes)
            .ok_or("Declared SSM tensor byte sum overflow.")?;
        require(
            shards
                .get(tensor.shard.as_str())
                .is_some_and(|bytes| *total <= *bytes),
            "Declared SSM tensor payloads collectively exceed their checkpoint shard.",
        )?;
    }
    require(
        checkpoint.ssm_tensors.len() == 48
            && blocks == (0..64).filter(|block| (block + 1) % 4 != 0).collect(),
        "Checkpoint manifest requires exactly the 48 original Swift SSM tensors.",
    )
}

fn conversion(manifest: &Intake, conversion: &Conversion, toolchain: &Toolchain) -> Result<()> {
    require(
        conversion.schema_version == 1 && toolchain.schema_version == 1,
        "Unknown conversion or toolchain manifest schema.",
    )?;
    source(&conversion.exporter.source)?;
    hash(&conversion.exporter.sha256)?;
    string(&conversion.exporter.path, 512)?;
    require(
        (1..=MANIFEST_LIMIT).contains(&conversion.exporter.bytes)
            && !conversion.exporter.path.starts_with('/')
            && !conversion.exporter.path.contains([':', '\\', '\0'])
            && conversion
                .exporter
                .path
                .split('/')
                .all(|part| !matches!(part, "" | "." | "..")),
        "Exporter must identify a bounded source file at a repository-relative path.",
    )?;
    require(
        !conversion.invocation.is_empty()
            && conversion.invocation.len() <= 128
            && !conversion.preprocessing.is_empty()
            && conversion.preprocessing.len() <= 64,
        "Conversion requires bounded exact argv and explicit preprocessing records.",
    )?;
    for value in conversion
        .invocation
        .iter()
        .chain(&conversion.preprocessing)
    {
        string(value, 4096)?;
    }
    for value in [
        &toolchain.python,
        &toolchain.numpy,
        &toolchain.safetensors,
        &toolchain.gguf_writer,
        &toolchain.matrix_multiplication,
    ] {
        string(value, 4096)?;
    }
    declared_artifact(&conversion.base_model, MAX_SHARD_BYTES)?;
    require(
        conversion.checkpoint_manifest_sha256 == manifest.checkpoint_manifest.sha256
            && conversion.toolchain_manifest_sha256 == manifest.toolchain_manifest.sha256
            && conversion.direction_sha256 == manifest.direction.sha256
            && conversion.output_adapter_sha256 == manifest.adapter.sha256
            && conversion.normalization == manifest.normalization,
        "Conversion record differs from the selected checkpoint/toolchain/direction/adapter/normalization.",
    )
}

pub(super) fn validate(directory: &Path, manifest: &Intake) -> Result<Value> {
    let parse = |reference: &Artifact| artifact(directory, reference, MANIFEST_LIMIT);
    let checkpoint: Checkpoint = serde_json::from_slice(&parse(&manifest.checkpoint_manifest)?)
        .map_err(|error| format!("Invalid checkpoint manifest: {error}"))?;
    let index: Index = serde_json::from_slice(&parse(&checkpoint.index)?)
        .map_err(|error| format!("Invalid checkpoint index: {error}"))?;
    let conversion_record: Conversion =
        serde_json::from_slice(&parse(&manifest.conversion_manifest)?)
            .map_err(|error| format!("Invalid conversion manifest: {error}"))?;
    let toolchain: Toolchain = serde_json::from_slice(&parse(&manifest.toolchain_manifest)?)
        .map_err(|error| format!("Invalid toolchain manifest: {error}"))?;
    self::checkpoint(&checkpoint, &index)?;
    conversion(manifest, &conversion_record, &toolchain)?;
    Ok(json!({
        "status": "declared-records-consistent", "checkpointIndexSha256": checkpoint.index.sha256,
        "declaredCheckpointShards": checkpoint.shards.len(), "declaredSsmTensors": 48,
        "originalCheckpointPayloadsVerified": false, "authoringProvenanceQualified": false,
        "aFactorReproductionQualified": false,
        "scope": "Raw index bytes and manifest consistency only. No checkpoint shard or tensor payload was read; immutable source, toolchain and invocation declarations remain unauthenticated."
    }))
}

#[cfg(test)]
mod tests;
