//! Offline direction estimation and certified sparse-objective FFN patch fitting.
//!
//! Source hashes bind declared capture identities. They do not authenticate the
//! supplied activations or establish held-out independence or behavioral benefit.

mod solver;
#[cfg(test)]
mod tests;

use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::Write as _,
    path::{Path, PathBuf},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const INPUT_WIDTH: usize = 17_408;
const OUTPUT_WIDTH: usize = 5_120;
const MAX_ROWS: usize = 256;
const EXECUTION_PROFILE: &str = "experimental-ffn-cpu-scalar-unfused-v1";

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Source {
    model_sha256: String,
    runtime_sha256: String,
    runtime_source_commit: String,
    adapter_sha256: Option<String>,
    adapter_scale_bits: String,
    weight_representation: String,
    execution_profile: String,
    graph_identity: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Site {
    tensor: String,
    input_basis: String,
    input_width: usize,
    output_width: usize,
}

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Split {
    Fit,
    Holdout,
}

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Label {
    Target,
    Preserve,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Row {
    sample_id: String,
    group_id: String,
    token_index: u32,
    split: Split,
    label: Label,
    activation: Vec<f32>,
    #[serde(default)]
    residual: Option<Vec<f32>>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
struct Controls {
    lambda: f64,
    target_penalty: f64,
    preserve_penalty: f64,
    delta: f64,
    epsilon: f64,
    max_iterations: u32,
    relative_gap_tolerance: f64,
    time_budget_milliseconds: u64,
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            lambda: 1.0,
            target_penalty: 10.0,
            preserve_penalty: 10.0,
            delta: 1.0,
            epsilon: 0.01,
            max_iterations: 10_000,
            relative_gap_tolerance: 0.0001,
            time_budget_milliseconds: 5_000,
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Operation {
    Direction,
    Fit,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    operation: Operation,
    schema_version: u32,
    source: Source,
    site: Site,
    rows: Vec<Row>,
    #[serde(default)]
    direction: Option<Vec<f32>>,
    #[serde(default)]
    direction_source_sha256: Option<String>,
    #[serde(default)]
    controls: Controls,
    #[serde(default)]
    output_path: Option<PathBuf>,
}

fn hex(value: &str, length: usize) -> crate::Result<()> {
    if !crate::identity::is_lower_hex(value, length) {
        return Err(format!(
            "Expected a canonical lowercase {length}-digit identity."
        ));
    }
    Ok(())
}

fn identifier(value: &str) -> bool {
    !value.is_empty() && value.len() <= 1024 && !value.chars().any(char::is_control)
}

impl Source {
    fn validate(&self) -> crate::Result<()> {
        hex(&self.model_sha256, 64)?;
        hex(&self.runtime_sha256, 64)?;
        hex(&self.runtime_source_commit, 40)?;
        hex(&self.adapter_scale_bits, 8)?;
        if let Some(adapter) = &self.adapter_sha256 {
            hex(adapter, 64)?;
        }
        let scale = f32::from_bits(
            u32::from_str_radix(&self.adapter_scale_bits, 16).map_err(|error| error.to_string())?,
        );
        if !scale.is_finite()
            || !(0.0..=2.0).contains(&scale)
            || (self.adapter_sha256.is_none() && self.adapter_scale_bits != "00000000")
            || !matches!(
                self.weight_representation.as_str(),
                "PQ2_0" | "current-repacked-PTQ1_0"
            )
            || self.execution_profile != EXECUTION_PROFILE
            || !identifier(&self.graph_identity)
        {
            return Err("Unsupported capture execution identity or adapter scale.".into());
        }
        Ok(())
    }
}

impl Site {
    fn validate(&self) -> crate::Result<()> {
        let layer = self
            .tensor
            .strip_prefix("blk.")
            .and_then(|name| name.strip_suffix(".ffn_down.weight"))
            .and_then(|name| name.parse::<u32>().ok())
            .filter(|&layer| layer < 64)
            .ok_or("Only a named Swift FFN-down site is supported.")?;
        if self.tensor != format!("blk.{layer}.ffn_down.weight")
            || self.input_basis != "original-ffn-hidden"
            || self.input_width != INPUT_WIDTH
            || self.output_width != OUTPUT_WIDTH
        {
            return Err("Unsupported FFN activation basis or dimensions.".into());
        }
        Ok(())
    }
}

impl Controls {
    fn validated(&self) -> crate::Result<solver::Controls> {
        let positive = |value: f64| value.is_finite() && (1e-12..=1e6).contains(&value);
        if !positive(self.lambda)
            || !positive(self.target_penalty)
            || !positive(self.preserve_penalty)
            || !positive(self.delta)
            || !self.epsilon.is_finite()
            || !(0.0..=1e6).contains(&self.epsilon)
            || !(1..=20_000).contains(&self.max_iterations)
            || !self.relative_gap_tolerance.is_finite()
            || !(1e-8..=0.01).contains(&self.relative_gap_tolerance)
            || !(1..=30_000).contains(&self.time_budget_milliseconds)
        {
            return Err("LP controls exceed finite numerical, iteration or time bounds.".into());
        }
        Ok(solver::Controls {
            lambda: self.lambda,
            target_penalty: self.target_penalty,
            preserve_penalty: self.preserve_penalty,
            delta: self.delta,
            epsilon: self.epsilon,
            max_iterations: self.max_iterations,
            relative_gap_tolerance: self.relative_gap_tolerance,
            budget: Duration::from_millis(self.time_budget_milliseconds),
        })
    }
}

impl Request {
    fn validate(&self) -> crate::Result<()> {
        if self.schema_version != 1 || !(2..=MAX_ROWS).contains(&self.rows.len()) {
            return Err("Unsupported weight-patch schema or row count.".into());
        }
        self.source.validate()?;
        self.site.validate()?;
        self.controls.validated()?;
        let mut groups = BTreeMap::new();
        let mut samples = BTreeMap::new();
        let mut tokens = BTreeSet::new();
        let mut fit_target = false;
        let mut fit_preserve = false;
        for row in &self.rows {
            if !identifier(&row.sample_id)
                || !identifier(&row.group_id)
                || row.activation.len() != self.site.input_width
                || row.activation.iter().any(|value| !value.is_finite())
                || row.residual.as_ref().is_some_and(|residual| {
                    residual.len() != self.site.output_width
                        || residual.iter().any(|value| !value.is_finite())
                })
                || !tokens.insert((&row.sample_id, row.token_index))
            {
                return Err(
                    "Capture rows require unique tokens and finite matching vectors.".into(),
                );
            }
            for (map, key) in [(&mut groups, &row.group_id), (&mut samples, &row.sample_id)] {
                if map
                    .insert(key, row.split)
                    .is_some_and(|split| split != row.split)
                {
                    return Err("A sample or trace group crosses the fit/holdout boundary.".into());
                }
            }
            if row.split == Split::Fit {
                match row.label {
                    Label::Target => fit_target = true,
                    Label::Preserve => fit_preserve = true,
                }
            }
        }
        if !fit_target || !fit_preserve {
            return Err("Both target and preserve fit rows are required.".into());
        }
        if let Some(path) = &self.output_path {
            bonsai_artifacts::validate_external_path(path).map_err(|error| error.to_string())?;
        }
        Ok(())
    }
}

fn factor_hash(values: &[f32]) -> crate::Result<String> {
    let mut hasher = crate::identity::Hasher::new()?;
    for value in values {
        hasher.update(&value.to_le_bytes())?;
    }
    hasher.finalize_hex()
}

fn request_hash(request: &Request) -> crate::Result<String> {
    let mut hash = crate::identity::Hasher::new()?;
    hash.update(b"bonsai-weight-patch-expanded-request-v1\0")?;
    hash.update(&serde_json::to_vec(request).map_err(|error| error.to_string())?)?;
    hash.finalize_hex()
}

fn unit_direction(direction: &[f32], width: usize) -> crate::Result<()> {
    if direction.len() != width || direction.iter().any(|value| !value.is_finite()) {
        return Err("Direction dimensions or finite values do not match the residual site.".into());
    }
    let norm = direction
        .iter()
        .map(|&value| f64::from(value).powi(2))
        .sum::<f64>()
        .sqrt();
    if (norm - 1.0).abs() > 1e-5 {
        return Err("Direction must already have unit L2 norm within 1e-5.".into());
    }
    Ok(())
}

#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "Normalized F64 direction components are narrowed to F32 and revalidated."
)]
fn direction(request: &Request) -> crate::Result<Value> {
    if request.direction.is_some() || request.direction_source_sha256.is_some() {
        return Err("Direction estimation takes fit residuals, not an existing direction.".into());
    }
    if request.output_path.is_some() {
        return Err(
            "Direction estimation returns JSON; outputPath is reserved for certified factors."
                .into(),
        );
    }
    let mut target = vec![0.0; request.site.output_width];
    let mut preserve = target.clone();
    let mut target_count = 0_u32;
    let mut preserve_count = 0_u32;
    let mut hash = crate::identity::Hasher::new()?;
    hash.update(b"bonsai-weight-patch-fit-residual-direction-v1\0")?;
    hash.update(&serde_json::to_vec(&request.source).map_err(|error| error.to_string())?)?;
    hash.update(&serde_json::to_vec(&request.site).map_err(|error| error.to_string())?)?;
    for row in request.rows.iter().filter(|row| row.split == Split::Fit) {
        let residual = row
            .residual
            .as_ref()
            .ok_or("Direction estimation needs every fit residual.")?;
        let (sum, count, label) = match row.label {
            Label::Target => (&mut target, &mut target_count, b"target".as_slice()),
            Label::Preserve => (&mut preserve, &mut preserve_count, b"preserve".as_slice()),
        };
        *count += 1;
        for (sum, &value) in sum.iter_mut().zip(residual) {
            *sum += f64::from(value);
        }
        // Framed identities and exact residual bytes bind the fitting recipe.
        for part in [row.group_id.as_bytes(), row.sample_id.as_bytes(), label] {
            hash.update(
                &u64::try_from(part.len())
                    .map_err(|error| error.to_string())?
                    .to_le_bytes(),
            )?;
            hash.update(part)?;
        }
        hash.update(&row.token_index.to_le_bytes())?;
        for value in residual {
            hash.update(&value.to_le_bytes())?;
        }
    }
    let difference: Vec<_> = target
        .into_iter()
        .zip(preserve)
        .map(|(target, preserve)| {
            target / f64::from(target_count) - preserve / f64::from(preserve_count)
        })
        .collect();
    let norm = difference
        .iter()
        .map(|value| value.powi(2))
        .sum::<f64>()
        .sqrt();
    if !norm.is_finite() || norm <= 1e-12 {
        return Err("Fit residual means have no finite nonzero separating direction.".into());
    }
    let direction: Vec<_> = difference
        .into_iter()
        .map(|value| (value / norm) as f32)
        .collect();
    unit_direction(&direction, request.site.output_width)?;
    Ok(
        json!({"schemaVersion":1,"status":"direction-estimated","source":request.source,
        "site":request.site,"direction":direction,"directionSha256":factor_hash(&direction)?,
        "directionSourceSha256":hash.finalize_hex()?,"fitTargetRows":target_count,
        "fitPreserveRows":preserve_count,"holdoutUsed":false,"behaviorQualified":false}),
    )
}

fn diagnostics(request: &Request, factors: &[f32]) -> Vec<Value> {
    request
        .rows
        .iter()
        .map(|row| {
            let push = row
                .activation
                .iter()
                .zip(factors)
                .map(|(&activation, &factor)| f64::from(activation) * f64::from(factor))
                .sum::<f64>();
            let violation = match row.label {
                Label::Target => (push + request.controls.delta).max(0.0),
                Label::Preserve => (push.abs() - request.controls.epsilon).max(0.0),
            };
            json!({"sampleId":row.sample_id,"groupId":row.group_id,"tokenIndex":row.token_index,
            "split":if row.split == Split::Fit {"fit"} else {"holdout"},
            "label":if row.label == Label::Target {"target"} else {"preserve"},
            "scalarPush":push,"constraintViolation":violation})
        })
        .collect()
}

pub(crate) fn publish(path: &Path, bytes: &[u8]) -> crate::Result<()> {
    let mut name = path
        .file_name()
        .ok_or("Patch output needs a file name.")?
        .to_os_string();
    name.push(format!(".pending-{}", std::process::id()));
    let temporary = path.with_file_name(name);
    bonsai_artifacts::validate_external_path(&temporary).map_err(|error| error.to_string())?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| format!("Cannot create patch staging file: {error}"))?;
    let result = output
        .write_all(bytes)
        .and_then(|()| output.sync_all())
        .and_then(|()| fs::hard_link(&temporary, path));
    drop(output);
    let cleanup = fs::remove_file(&temporary);
    match (result, cleanup) {
        (Ok(()), Ok(())) => Ok(()),
        (Ok(()), Err(error)) => Err(format!(
            "Patch published but staging cleanup failed: {error}"
        )),
        (Err(error), Ok(())) => Err(format!("Cannot atomically publish a new patch: {error}")),
        (Err(error), Err(cleanup)) => Err(format!(
            "Cannot atomically publish a new patch: {error}; staging cleanup failed: {cleanup}"
        )),
    }
}

fn fit(request: &Request) -> crate::Result<Value> {
    let direction = request
        .direction
        .as_ref()
        .ok_or("Fit requires an independently selected unit direction.")?;
    unit_direction(direction, request.site.output_width)?;
    let direction_source = request
        .direction_source_sha256
        .as_ref()
        .ok_or("Fit requires directionSourceSha256.")?;
    hex(direction_source, 64)?;
    let rows: Vec<_> = request
        .rows
        .iter()
        .filter(|row| row.split == Split::Fit)
        .map(|row| solver::Row {
            target: row.label == Label::Target,
            activation: &row.activation,
        })
        .collect();
    let solution = solver::solve(&rows, request.controls.validated()?)?;
    let mut report = json!({"schemaVersion":1,"status":solution.status,
        "iterations":solution.iterations,"optimizationCertificate":{
            "kind":"conservative-floating-point-primal-dual-gap",
            "certified":solution.status == "converged",
            "primalObjective":solution.certificate.primal,"dualLowerBound":solution.certificate.dual,
            "relativeGap":solution.certificate.relative_gap,
            "relativeGapTolerance":request.controls.relative_gap_tolerance,
            "dualGradientMaximum":solution.certificate.dual_gradient_max,
            "dualWeights":solution.certificate.dual_weights,
            "evaluatedFactors":"F32-le","lambda":request.controls.lambda},
        "rows":diagnostics(request,&solution.factors),"holdoutUsedForFit":false,
        "sourceAuthenticated":false,"behaviorQualified":false,"installed":false,
        "limits":{"maxRows":MAX_ROWS,"timeBudgetMilliseconds":request.controls.time_budget_milliseconds,
            "maxCellVisits":500_000_000},"artifact":null});
    if solution.status != "converged" {
        return Ok(report);
    }
    let artifact = json!({"schemaVersion":1,"kind":"experimental-ffn-rank-one-patch",
        "enabled":false,"qualification":"unqualified-research-only","source":request.source,
        "site":request.site,"compositionOrder":"after-standard-adapter","scaleBits":"3f800000",
        "fitInputSha256":request_hash(request)?,"controls":request.controls,
        "factors":{"a":solution.factors,"b":direction,
            "aSha256":factor_hash(&solution.factors)?,"bSha256":factor_hash(direction)?},
        "directionSourceSha256":direction_source,"optimizationCertificate":report["optimizationCertificate"],
        "behaviorQualified":false});
    if let Some(path) = &request.output_path {
        let bytes = serde_json::to_vec_pretty(&artifact).map_err(|error| error.to_string())?;
        publish(path, &bytes)?;
        report["outputPath"] = json!(path);
        let mut hash = crate::identity::Hasher::new()?;
        hash.update(&bytes)?;
        report["artifactSha256"] = json!(hash.finalize_hex()?);
    }
    report["artifact"] = artifact;
    Ok(report)
}

/// Estimates a fit-only residual direction or fits a bounded rank-one FFN patch.
///
/// # Errors
/// Rejects malformed identities, unsupported sites/bases, split leakage, nonfinite
/// vectors, oversized work and unsafe or existing output paths. Optimization
/// exhaustion is an explicit report without an artifact, rather than success.
pub fn run(request: Value) -> crate::Result<Value> {
    let request: Request = serde_json::from_value(request).map_err(|error| error.to_string())?;
    request.validate()?;
    match request.operation {
        Operation::Direction => direction(&request),
        Operation::Fit => fit(&request),
    }
}
