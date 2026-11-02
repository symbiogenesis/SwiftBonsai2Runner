//! Bounded byte intake and independent all-site signed-permutation comparison.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read as _;
use std::path::{Component, Path, PathBuf};

use bonsai_artifacts::open_locked_file;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::{decode, hash, require, string};
use crate::Result;
use crate::identity::sha256;

mod direction;
mod header;
#[cfg(test)]
mod tests;

const CHANNELS: usize = 6144;
const VECTOR_BYTES: u64 = 24_576;
const MANIFEST_LIMIT: u64 = 1_048_576;
const ADAPTER_BYTES: u64 = 9_682_464;
const ADAPTER_SHA256: &str = "f1669534803d340a496015f5c45125f3437b4d13ec764f40e34488ce83967f42";
const MODEL_SHA256: &str = "e658f0f0736a03a4549535a439f9b07254ebf8ab88ae27cbe6e52d098ca5d1f5";

pub(super) use direction::run as check_direction;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    evidence_directory: PathBuf,
    manifest_path: PathBuf,
    manifest_sha256: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Artifact {
    file: String,
    bytes: u64,
    sha256: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Site {
    block: u32,
    authoring_a: Artifact,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Intake {
    schema_version: u32,
    model_sha256: String,
    runtime_commit: String,
    adapter: Artifact,
    checkpoint_manifest: Artifact,
    conversion_manifest: Artifact,
    toolchain_manifest: Artifact,
    direction: Artifact,
    normalization: String,
    mapping: Mapping,
    absolute_tolerance: f64,
    relative_tolerance: f64,
    ssm_sites: Vec<Site>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Mapping {
    // native[channel] = signs[channel] * authoring[nativeToAuthoring[channel]].
    native_to_authoring: Vec<usize>,
    signs: Vec<i8>,
}

fn read(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let mut file = open_locked_file(path).map_err(|error| error.to_string())?;
    let length = file.metadata().map_err(|error| error.to_string())?.len();
    require(length <= limit, "Basis input exceeds its byte bound.")?;
    let mut bytes = Vec::new();
    (&mut file)
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    require(
        u64::try_from(bytes.len()).map_err(|error| error.to_string())? == length,
        "Basis input changed or exceeded its byte bound.",
    )?;
    Ok(bytes)
}

fn artifact(directory: &Path, reference: &Artifact, limit: u64) -> Result<Vec<u8>> {
    hash(&reference.sha256)?;
    string(&reference.file, 128)?;
    let mut components = Path::new(&reference.file).components();
    require(
        matches!(components.next(), Some(Component::Normal(_)))
            && components.next().is_none()
            && !reference.file.contains([':', '/', '\\']),
        "Basis artifacts must name individual files inside the evidence directory.",
    )?;
    require(
        (1..=limit).contains(&reference.bytes),
        "Basis artifact has an invalid byte length.",
    )?;
    let bytes = read(&directory.join(&reference.file), limit)?;
    require(
        u64::try_from(bytes.len()).map_err(|error| error.to_string())? == reference.bytes
            && sha256(&bytes) == reference.sha256,
        "Basis artifact differs from its declared raw-byte identity.",
    )?;
    Ok(bytes)
}

fn validate(manifest: &Intake) -> Result<()> {
    let runtime: Value =
        serde_json::from_str(include_str!("../../../../config/prism-inference.lock.json"))
            .map_err(|error| error.to_string())?;
    require(
        manifest.schema_version == 1
            && manifest.model_sha256 == MODEL_SHA256
            && manifest.runtime_commit == runtime["commit"]
            && manifest.adapter.bytes == ADAPTER_BYTES
            && manifest.adapter.sha256 == ADAPTER_SHA256,
        "Basis intake must bind the approved adapter, current PTQ1 and pinned runtime.",
    )?;
    string(&manifest.normalization, 4096)?;
    for tolerance in [manifest.absolute_tolerance, manifest.relative_tolerance] {
        require(
            tolerance.is_finite() && (0.0..=0.000_01).contains(&tolerance),
            "Basis tolerances must be finite, nonnegative and at most 1e-5.",
        )?;
    }
    require(
        manifest.mapping.native_to_authoring.len() == CHANNELS
            && manifest.mapping.signs.len() == CHANNELS
            && manifest
                .mapping
                .signs
                .iter()
                .all(|sign| matches!(sign, -1 | 1))
            && manifest
                .mapping
                .native_to_authoring
                .iter()
                .copied()
                .collect::<BTreeSet<_>>()
                == (0..CHANNELS).collect(),
        "Basis mapping must be a complete signed permutation of 6144 channels.",
    )?;
    let blocks: BTreeSet<_> = manifest.ssm_sites.iter().map(|site| site.block).collect();
    require(
        manifest.ssm_sites.len() == 48
            && blocks == (0..64).filter(|block| (block + 1) % 4 != 0).collect(),
        "Basis intake requires exactly all 48 distinct Swift SSM sites.",
    )
}

fn vector(bytes: &[u8]) -> Result<Vec<f64>> {
    require(
        bytes.len() == CHANNELS * 4,
        "Basis A vectors must contain exactly 6144 little-endian F32 values.",
    )?;
    let (chunks, _) = bytes.as_chunks::<4>();
    chunks
        .iter()
        .map(|chunk| {
            let value = f64::from(f32::from_le_bytes(*chunk));
            require(
                value.is_finite(),
                "Basis vectors must contain finite values.",
            )?;
            Ok(value)
        })
        .collect()
}

fn compare(
    shipped: &[u8],
    authoring: &[u8],
    mapping: &Mapping,
    absolute: f64,
    relative: f64,
) -> Result<(u64, f64)> {
    let actual = vector(shipped)?;
    let reference = vector(authoring)?;
    let mut mismatches = 0;
    let mut maximum: f64 = 0.0;
    for (channel, actual) in actual.into_iter().enumerate() {
        let expected =
            f64::from(mapping.signs[channel]) * reference[mapping.native_to_authoring[channel]];
        let difference = (actual - expected).abs();
        maximum = maximum.max(difference);
        if difference > relative.mul_add(expected.abs(), absolute) {
            mismatches += 1;
        }
    }
    Ok((mismatches, maximum))
}

pub(super) fn run(request: Map<String, Value>) -> Result<Value> {
    let request: Request = decode(Value::Object(request))?;
    let directory = super::validate_path(&request.evidence_directory)?;
    require(directory.is_dir(), "Basis evidence directory is missing.")?;
    let path = super::validate_path(&request.manifest_path)?;
    hash(&request.manifest_sha256)?;
    let bytes = read(&path, MANIFEST_LIMIT)?;
    require(
        sha256(&bytes) == request.manifest_sha256,
        "Basis manifest differs from its selected raw-byte digest.",
    )?;
    let manifest: Intake = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    validate(&manifest)?;
    let mut inputs = BTreeMap::new();
    for (name, reference) in [
        ("checkpointManifest", &manifest.checkpoint_manifest),
        ("conversionManifest", &manifest.conversion_manifest),
        ("toolchainManifest", &manifest.toolchain_manifest),
        ("direction", &manifest.direction),
    ] {
        artifact(&directory, reference, MANIFEST_LIMIT)?;
        inputs.insert(name, reference.sha256.as_str());
    }
    let adapter = artifact(&directory, &manifest.adapter, ADAPTER_BYTES)?;
    let vectors = header::ssm_vectors(&adapter)?;
    let mut reports = Vec::new();
    let mut passed = true;
    for site in &manifest.ssm_sites {
        let authoring = artifact(&directory, &site.authoring_a, VECTOR_BYTES)?;
        let shipped = vectors
            .get(&site.block)
            .ok_or("Approved adapter lacks a required SSM A vector.")?;
        let (mismatches, maximum) = compare(
            shipped,
            &authoring,
            &manifest.mapping,
            manifest.absolute_tolerance,
            manifest.relative_tolerance,
        )?;
        passed &= mismatches == 0;
        reports.push(
            json!({"block": site.block, "authoringASha256": site.authoring_a.sha256,
            "shippedASha256": sha256(shipped), "comparedElements": CHANNELS,
            "mismatchedElements": mismatches, "maximumAbsoluteDifference": maximum}),
        );
    }
    Ok(
        json!({"schemaVersion": 1, "kind": "adapter-authoring-basis-intake",
        "manifestSha256": request.manifest_sha256, "adapterSha256": manifest.adapter.sha256,
        "modelSha256": manifest.model_sha256, "runtimeCommit": manifest.runtime_commit,
        "status": if passed {"intake-complete"} else {"rejected-vector-comparison"},
        "inputSha256": inputs, "ssmVectorComparisonPassed": passed, "sites": reports,
        "writerBasisQualified": false, "defaultMayChange": false,
        "pending": ["Independent authentication of checkpoint/conversion/direction provenance and reproduction", "Independent native writer-input/correction validation of the declared mapping", "No-adapter/loaded-zero identity and nonzero logits/live-state application", "Scalar/explicit-NEON and application promotion gates"],
        "scope": "Raw-byte intake and complete signed-permutation A-vector comparison only. Checkpoint payloads are not loaded or hashed. Caller-supplied manifests and vectors do not authenticate their authoring provenance or qualify the native writer basis."}),
    )
}
