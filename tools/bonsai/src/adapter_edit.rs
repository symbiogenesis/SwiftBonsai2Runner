//! Deterministic weight edits to a separately published rank-one adapter.

use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{Read as _, Write as _},
    path::{Path, PathBuf},
};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::{Result, identity::Hasher};

#[cfg(test)]
mod tests;

const ADAPTER_BYTES: u64 = 9_682_464;
const ADAPTER_SHA256: &str = "f1669534803d340a496015f5c45125f3437b4d13ec764f40e34488ce83967f42";
const ALGORITHM: &str = "linear-a-magnitude-prune-stable-index-v1";

#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Operation {
    Inspect,
    Create,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    schema_version: u32,
    operation: Operation,
    adapter_path: PathBuf,
    keep_numerator: usize,
    keep_denominator: usize,
    output_path: Option<PathBuf>,
}

fn digest(bytes: &[u8]) -> Result<String> {
    let mut hash = Hasher::new()?;
    hash.update(bytes)?;
    hash.finalize_hex()
}

fn validate(request: &Request) -> Result<()> {
    if request.schema_version != 1
        || request.keep_numerator == 0
        || request.keep_numerator > request.keep_denominator
        || request.keep_denominator > 1024
    {
        return Err("Use schema 1 and a positive keep fraction at most one, denominator <=1024.".into());
    }
    bonsai_artifacts::validate_external_path(&request.adapter_path)
        .map_err(|error| error.to_string())?;
    match (&request.operation, &request.output_path) {
        (Operation::Inspect, None) => Ok(()),
        (Operation::Create, Some(path)) => {
            bonsai_artifacts::validate_external_path(path).map_err(|error| error.to_string())?;
            if path == &request.adapter_path
                || fs::exists(path).map_err(|error| error.to_string())?
                || path.parent().is_none_or(|parent| !parent.is_dir())
            {
                return Err("Output must be a new file in an existing external directory.".into());
            }
            if request.keep_numerator == request.keep_denominator {
                return Err("Creating an unchanged adapter is not a weight edit.".into());
            }
            Ok(())
        }
        _ => Err("Inspect forbids outputPath; create requires outputPath.".into()),
    }
}

fn read_source(path: &Path) -> Result<Vec<u8>> {
    let mut file = bonsai_artifacts::open_locked_file(path).map_err(|error| error.to_string())?;
    if file.metadata().map_err(|error| error.to_string())?.len() != ADAPTER_BYTES {
        return Err("Adapter editing requires the exact approved source length.".into());
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(ADAPTER_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if u64::try_from(bytes.len()).map_err(|error| error.to_string())? != ADAPTER_BYTES
        || digest(&bytes)? != ADAPTER_SHA256
    {
        return Err("Adapter editing requires the exact approved source digest.".into());
    }
    Ok(bytes)
}

struct Pruned {
    bytes: Vec<u8>,
    original_nonzero: usize,
    retained_nonzero: usize,
    changed: usize,
    original_energy: f64,
    removed_energy: f64,
}

#[derive(Default)]
struct Family {
    sites: usize,
    coefficients: usize,
    retained_nonzero: usize,
    original_energy: f64,
    removed_energy: f64,
}

fn prune(bytes: &[u8], numerator: usize, denominator: usize) -> Result<Pruned> {
    let (chunks, tail) = bytes.as_chunks::<4>();
    if chunks.is_empty() || !tail.is_empty() || chunks.len() > 65_536 {
        return Err("Linear A factors require 1..65536 F32 coefficients.".into());
    }
    let values: Vec<_> = chunks.iter().map(|chunk| f32::from_le_bytes(*chunk)).collect();
    if values.iter().any(|value| !value.is_finite()) {
        return Err("Adapter coefficients must be finite.".into());
    }
    let keep = (values.len() * numerator).div_ceil(denominator);
    let mut indices: Vec<_> = (0..values.len()).collect();
    indices.sort_unstable_by(|&left, &right| {
        values[right]
            .abs()
            .total_cmp(&values[left].abs())
            .then_with(|| left.cmp(&right))
    });
    let mut retained = vec![false; values.len()];
    for &index in &indices[..keep] {
        retained[index] = true;
    }
    let mut result = Pruned {
        bytes: bytes.to_vec(),
        original_nonzero: 0,
        retained_nonzero: 0,
        changed: 0,
        original_energy: 0.0,
        removed_energy: 0.0,
    };
    for (index, &value) in values.iter().enumerate() {
        let energy = f64::from(value) * f64::from(value);
        result.original_energy += energy;
        result.original_nonzero += usize::from(value != 0.0);
        if retained[index] {
            result.retained_nonzero += usize::from(value != 0.0);
        } else {
            result.removed_energy += energy;
            if value != 0.0 {
                result.bytes[index * 4..index * 4 + 4].copy_from_slice(&0.0_f32.to_le_bytes());
                result.changed += 1;
            }
        }
    }
    Ok(result)
}

fn retained_energy(original: f64, removed: f64) -> f64 {
    if original == 0.0 { 1.0 } else { ((original - removed) / original).clamp(0.0, 1.0) }
}

fn edited(source: &[u8], numerator: usize, denominator: usize) -> Result<(Vec<u8>, Value)> {
    let mut candidate = source.to_vec();
    let ranges = crate::qualification::linear_a_ranges(source)?;
    let mut sites = Vec::new();
    let mut coefficients = 0_usize;
    let mut original_nonzero = 0_usize;
    let mut retained_nonzero = 0_usize;
    let mut changed = 0_usize;
    let mut original_energy = 0.0;
    let mut removed_energy = 0.0;
    let mut minimum_retained_fraction = 1.0_f64;
    let mut families: BTreeMap<&str, Family> = BTreeMap::new();
    for (site, range) in ranges {
        let factors = &source[range.clone()];
        let pruned = prune(factors, numerator, denominator)?;
        let fraction = retained_energy(pruned.original_energy, pruned.removed_energy);
        candidate[range].copy_from_slice(&pruned.bytes);
        coefficients += factors.len() / 4;
        original_nonzero += pruned.original_nonzero;
        retained_nonzero += pruned.retained_nonzero;
        changed += pruned.changed;
        original_energy += pruned.original_energy;
        removed_energy += pruned.removed_energy;
        minimum_retained_fraction = minimum_retained_fraction.min(fraction);
        let family_name = if site.ends_with(".ffn_down.weight") {
            "ffn_down"
        } else if site.ends_with(".attn_output.weight") {
            "attn_output"
        } else {
            "ssm_out"
        };
        let family = families.entry(family_name).or_default();
        family.sites += 1;
        family.coefficients += factors.len() / 4;
        family.retained_nonzero += pruned.retained_nonzero;
        family.original_energy += pruned.original_energy;
        family.removed_energy += pruned.removed_energy;
        sites.push(json!({"site":site,"coefficients":factors.len()/4,
            "originalNonzero":pruned.original_nonzero,"retainedNonzero":pruned.retained_nonzero,
            "changedCoefficients":pruned.changed,"retainedSquaredL2Fraction":fraction,
            "relativeRankOneFrobeniusError":(1.0-fraction).sqrt(),
            "originalASha256":digest(factors)?,"candidateASha256":digest(&pruned.bytes)?}));
    }
    let families: Vec<_> = families.into_iter().map(|(name, family)| {
        let fraction = retained_energy(family.original_energy, family.removed_energy);
        json!({"family":name,"sites":family.sites,"coefficients":family.coefficients,
            "retainedNonzero":family.retained_nonzero,"retainedSquaredL2Fraction":fraction,
            "relativeAggregateAFactorL2Error":(1.0-fraction).sqrt()})
    }).collect();
    let fraction = retained_energy(original_energy, removed_energy);
    let summary = json!({"linearSites":sites.len(),"coefficients":coefficients,
        "originalNonzero":original_nonzero,"retainedNonzero":retained_nonzero,
        "changedCoefficients":changed,"retainedSquaredL2Fraction":fraction,
        "relativeAggregateAFactorL2Error":(1.0-fraction).sqrt(),
        "minimumSiteRetainedSquaredL2Fraction":minimum_retained_fraction,
        "denseLinearAFactorPayloadBytes":coefficients*4,
        "sparseU16IndexF32ValuePayloadBytes":retained_nonzero*6,
        "families":families,"sites":sites});
    Ok((candidate, summary))
}

fn publish(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut staging_name = path.file_name().ok_or("Output requires a file name.")?.to_os_string();
    staging_name.push(format!(".pending-{}", std::process::id()));
    let staging = path.with_file_name(staging_name);
    bonsai_artifacts::validate_external_path(&staging).map_err(|error| error.to_string())?;
    let mut file = OpenOptions::new().write(true).create_new(true).open(&staging)
        .map_err(|error| format!("Cannot create adapter staging file: {error}"))?;
    let published = file.write_all(bytes).and_then(|()| file.sync_all())
        .and_then(|()| fs::hard_link(&staging, path));
    drop(file);
    let cleanup = fs::remove_file(&staging);
    match (published, cleanup) {
        (Ok(()), Ok(())) => Ok(()),
        (Ok(()), Err(error)) => Err(format!("Adapter published; staging cleanup failed: {error}")),
        (Err(error), Ok(())) => Err(format!("Cannot publish new adapter: {error}")),
        (Err(error), Err(cleanup)) => Err(format!("Cannot publish new adapter: {error}; staging cleanup failed: {cleanup}")),
    }
}

/// Inspects a deterministic magnitude edit or publishes its separately named GGUF.
///
/// # Errors
/// Rejects unsupported requests, unauthenticated source bytes, invalid factor
/// inventories, nonfinite coefficients, no-op edits and unsafe/existing outputs.
pub fn run(request: Value) -> Result<Value> {
    let request: Request = serde_json::from_value(request).map_err(|error| error.to_string())?;
    validate(&request)?;
    let source = read_source(&request.adapter_path)?;
    let (candidate, summary) = edited(&source, request.keep_numerator, request.keep_denominator)?;
    if request.operation == Operation::Create && summary["changedCoefficients"] == 0 {
        return Err("Selected edit changes no coefficients.".into());
    }
    let candidate_sha256 = digest(&candidate)?;
    if let Some(output) = &request.output_path {
        publish(output, &candidate)?;
    }
    Ok(json!({"schemaVersion":1,"kind":"experimental-edited-rank-one-adapter",
        "algorithm":ALGORITHM,"sourceAdapterSha256":ADAPTER_SHA256,
        "candidateSha256":candidate_sha256,"candidateBytes":candidate.len(),
        "sourceAdapterPath":request.adapter_path,"outputPath":request.output_path,
        "keepNumerator":request.keep_numerator,"keepDenominator":request.keep_denominator,
        "published":request.operation==Operation::Create,"weightEdit":summary,
        "all129SitesRetained":true,"bFactorsUnchanged":true,"embeddingFactorsUnchanged":true,
        "metadataAndLayoutUnchanged":true,"diskSizeReduced":false,
        "behaviorQualified":false,"performanceQualified":false,"installed":false,
        "scope":"Edits only the existing adapter's linear F32 A coefficients; retains its rank, directions, embedding, metadata and all sites. Sparse payload estimates exclude container, allocator and runtime overhead. Weight-space error is not an accuracy bound."}))
}
