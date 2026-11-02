//! Independent agreement of every adapter B factor with a selected unit direction.

use std::path::PathBuf;

use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::{ADAPTER_BYTES, ADAPTER_SHA256, decode, hash, header, read, require, sha256};
use crate::Result;

#[cfg(test)]
mod tests;

const CHANNELS: usize = 5120;
const VECTOR_BYTES: u64 = 20_480;
const ABSOLUTE_TOLERANCE: f64 = 0.000_001;
const RELATIVE_TOLERANCE: f64 = 0.000_001;
const NORM_TOLERANCE: f64 = 0.000_01;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    adapter_path: PathBuf,
    direction_path: PathBuf,
    direction_sha256: String,
}

fn vector(bytes: &[u8]) -> Result<Vec<f64>> {
    require(
        bytes.len() == CHANNELS * 4,
        "Direction and B vectors require exactly 5120 little-endian F32 values.",
    )?;
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|chunk| {
            let value = f64::from(f32::from_le_bytes(*chunk));
            require(value.is_finite(), "Direction and B vectors must be finite.")?;
            Ok(value)
        })
        .collect()
}

fn unit_direction(bytes: &[u8]) -> Result<(Vec<f64>, f64)> {
    let direction = vector(bytes)?;
    let norm = direction
        .iter()
        .fold(0.0_f64, |sum, value| value.mul_add(*value, sum))
        .sqrt();
    require(
        (norm - 1.0).abs() <= NORM_TOLERANCE,
        "Selected direction must already be normalized to unit L2 norm within 1e-5.",
    )?;
    Ok((direction, norm))
}

fn compare(bytes: &[u8], direction: &[f64]) -> Result<(usize, f64)> {
    let actual = vector(bytes)?;
    require(direction.len() == CHANNELS, "Direction width mismatch.")?;
    let mut mismatches = 0;
    let mut maximum = 0.0_f64;
    for (actual, direction) in actual.into_iter().zip(direction) {
        let expected = -direction;
        let difference = (actual - expected).abs();
        maximum = maximum.max(difference);
        if difference > RELATIVE_TOLERANCE.mul_add(expected.abs(), ABSOLUTE_TOLERANCE) {
            mismatches += 1;
        }
    }
    Ok((mismatches, maximum))
}

fn report(adapter: &[u8], direction: &[u8]) -> Result<Value> {
    let (values, norm) = unit_direction(direction)?;
    let vectors = header::direction_vectors(adapter)?;
    let mut passed = true;
    let mut sites = Vec::new();
    for (site, vector) in vectors {
        let (mismatches, maximum) = compare(vector, &values)?;
        passed &= mismatches == 0;
        sites.push(json!({"site":site, "shippedBSha256":sha256(vector),
            "comparedElements":CHANNELS, "mismatchedElements":mismatches,
            "maximumAbsoluteDifference":maximum}));
    }
    Ok(
        json!({"schemaVersion":1, "kind":"adapter-direction-consistency",
        "adapterSha256":sha256(adapter), "directionSha256":sha256(direction),
        "directionL2Norm":norm, "directionL2NormTolerance":NORM_TOLERANCE,
        "absoluteTolerance":ABSOLUTE_TOLERANCE, "relativeTolerance":RELATIVE_TOLERANCE,
        "status":if passed {"direction-consistent"} else {"rejected-direction-comparison"},
        "directionComparisonPassed":passed, "sites":sites,
        "authoringProvenanceQualified":false, "writerBasisQualified":false,
        "defaultMayChange":false,
        "scope":"All 129 B factors compared with the negative of the selected already-normalized direction. Does not authenticate the direction source, reproduce A factors, establish native SSM input order or qualify model behavior."}),
    )
}

pub(in crate::qualification) fn run(request: Map<String, Value>) -> Result<Value> {
    let request: Request = decode(Value::Object(request))?;
    let adapter_path = super::super::validate_path(&request.adapter_path)?;
    let direction_path = super::super::validate_path(&request.direction_path)?;
    hash(&request.direction_sha256)?;
    let adapter = read(&adapter_path, ADAPTER_BYTES)?;
    require(
        u64::try_from(adapter.len()).map_err(|error| error.to_string())? == ADAPTER_BYTES
            && sha256(&adapter) == ADAPTER_SHA256,
        "Direction verification requires the exact approved adapter bytes.",
    )?;
    let direction = read(&direction_path, VECTOR_BYTES)?;
    require(
        sha256(&direction) == request.direction_sha256,
        "Direction input differs from its selected raw-byte identity.",
    )?;
    report(&adapter, &direction)
}
