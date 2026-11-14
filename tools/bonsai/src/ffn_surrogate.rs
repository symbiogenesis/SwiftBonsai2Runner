//! Offline whole-FFN replacement and structured-compensation fitting.
//!
//! Structured factors have a separate source-only Prism replay consumer;
//! whole-FFN replacement, selective loading and released serving stay unsupported.
//!
//! Capture identities are declarations, not authentication of supplied vectors.

mod solver;
mod structured;
#[cfg(test)]
mod tests;

use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const WIDTH: usize = 5_120;
const MAX_ROWS: usize = 64;
const MAX_RANK: usize = 16;
const PROFILE: &str = "experimental-ffn-cpu-scalar-unfused-v1";

#[derive(Deserialize, Serialize)]
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

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Site {
    layer: u32,
    input_basis: String,
    target_basis: String,
    input_width: usize,
    output_width: usize,
}

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Split {
    Fit,
    Development,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Row {
    sample_id: String,
    group_id: String,
    token_index: u32,
    split: Split,
    input: Vec<f32>,
    target: Vec<f32>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
struct Controls {
    rank: usize,
    ridge: f64,
    time_budget_milliseconds: u64,
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            rank: 8,
            ridge: 0.001,
            time_budget_milliseconds: 5_000,
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    schema_version: u32,
    source: Source,
    site: Site,
    rows: Vec<Row>,
    #[serde(default)]
    controls: Controls,
    output_path: Option<PathBuf>,
}

fn identifier(value: &str) -> bool {
    !value.is_empty() && value.len() <= 1024 && !value.chars().any(char::is_control)
}

fn validate_source_identity(source: &Source) -> crate::Result<f32> {
    for (value, width) in [
        (&source.model_sha256, 64),
        (&source.runtime_sha256, 64),
        (&source.runtime_source_commit, 40),
        (&source.adapter_scale_bits, 8),
    ] {
        if !crate::identity::is_lower_hex(value, width) {
            return Err("Noncanonical source identity.".into());
        }
    }
    if source
        .adapter_sha256
        .as_ref()
        .is_some_and(|value| !crate::identity::is_lower_hex(value, 64))
    {
        return Err("Noncanonical teacher adapter identity.".into());
    }
    Ok(f32::from_bits(
        u32::from_str_radix(&source.adapter_scale_bits, 16).map_err(|error| error.to_string())?,
    ))
}

impl Request {
    fn validate(&self) -> crate::Result<()> {
        let source = &self.source;
        let scale = validate_source_identity(source)?;
        if self.schema_version != 1
            || !(3..=MAX_ROWS).contains(&self.rows.len())
            || !scale.is_finite()
            || !(0.0..=2.0).contains(&scale)
            || (source.adapter_sha256.is_none() && source.adapter_scale_bits != "00000000")
            || !matches!(
                source.weight_representation.as_str(),
                "PQ2_0" | "current-repacked-PTQ1_0"
            )
            || source.execution_profile != PROFILE
            || !identifier(&source.graph_identity)
            || self.site.layer >= 64
            || self.site.input_width != WIDTH
            || self.site.output_width != WIDTH
            || self.site.input_basis != "original-ffn-normalized-input"
            || self.site.target_basis != "complete-ffn-branch-delta"
        {
            return Err(
                "Unsupported teacher identity, whole-FFN basis, shape or row count.".into(),
            );
        }
        let controls = &self.controls;
        if !(1..=MAX_RANK).contains(&controls.rank)
            || !controls.ridge.is_finite()
            || !(1e-8..=1e6).contains(&controls.ridge)
            || !(1..=30_000).contains(&controls.time_budget_milliseconds)
        {
            return Err("Surrogate controls exceed rank, finite ridge or time limits.".into());
        }
        let mut groups = BTreeMap::new();
        let mut samples = BTreeMap::new();
        let mut inputs = BTreeMap::new();
        let mut tokens = BTreeSet::new();
        let mut fit_count = 0;
        let mut development_count = 0;
        for row in &self.rows {
            if !identifier(&row.sample_id)
                || !identifier(&row.group_id)
                || row.input.len() != WIDTH
                || row.target.len() != WIDTH
                || row
                    .input
                    .iter()
                    .chain(&row.target)
                    .any(|value| !value.is_finite())
                || !tokens.insert((&row.sample_id, row.token_index))
            {
                return Err(
                    "Rows need unique tokens and finite matching input/target vectors.".into(),
                );
            }
            for (map, key) in [(&mut groups, &row.group_id), (&mut samples, &row.sample_id)] {
                if map
                    .insert(key, row.split)
                    .is_some_and(|split| split != row.split)
                {
                    return Err("A sample or content group crosses fit/development splits.".into());
                }
            }
            // Renaming a sample/group must not admit an exact training input
            // as development evidence. Keep F32 identities exact, including
            // signed zero, as in the paired fitter's F32-le input digests.
            let input: Vec<_> = row.input.iter().map(|value| value.to_bits()).collect();
            if inputs
                .insert(input, row.split)
                .is_some_and(|split| split != row.split)
            {
                return Err("An identical F32 input crosses fit/development splits.".into());
            }
            match row.split {
                Split::Fit => fit_count += 1,
                Split::Development => development_count += 1,
            }
        }
        if fit_count < 2 || development_count == 0 || controls.rank > fit_count {
            return Err(
                "Require at least two fit rows, one development row and rank <= fit rows.".into(),
            );
        }
        self.validate_output()
    }

    fn validate_output(&self) -> crate::Result<()> {
        if let Some(path) = &self.output_path {
            bonsai_artifacts::validate_external_path(path).map_err(|error| error.to_string())?;
        }
        Ok(())
    }
}

fn hash(bytes: &[u8]) -> crate::Result<String> {
    let mut digest = crate::identity::Hasher::new()?;
    digest.update(bytes)?;
    digest.finalize_hex()
}

fn factor_hash(values: &[Vec<f32>]) -> crate::Result<String> {
    let mut digest = crate::identity::Hasher::new()?;
    for value in values.iter().flatten() {
        digest.update(&value.to_le_bytes())?;
    }
    digest.finalize_hex()
}

/// Fits a bounded replacement from teacher inputs and complete FFN branch outputs.
///
/// # Errors
/// Rejects unsupported identities/bases, malformed vectors, split leakage,
/// ill-conditioned solves, exhausted time/work, and existing or unsafe outputs.
/// There is no final-heldout split: final answer evaluation must remain external.
/// The numerical deadline is checked before synchronous publication. Native
/// process supervision must enforce the original aggregate deadline across CLI
/// intake, publication, output and cleanup; this function cannot preempt file I/O.
pub fn run(request: Value) -> crate::Result<Value> {
    let started = Instant::now();
    if request["schemaVersion"].as_u64() == Some(2) {
        return structured::run(request, started);
    }
    let request: Request = serde_json::from_value(request).map_err(|error| error.to_string())?;
    request.validate()?;
    let deadline = started
        .checked_add(Duration::from_millis(
            request.controls.time_budget_milliseconds,
        ))
        .ok_or("Surrogate deadline overflow.")?;
    let rows: Vec<_> = request
        .rows
        .iter()
        .filter(|row| row.split == Split::Fit)
        .collect();
    let solution = solver::fit(
        &rows,
        request.controls.rank,
        request.controls.ridge,
        deadline,
    )?;
    let mut work = solution.work;
    let diagnostics =
        solver::diagnostics(&request.rows, &solution.a, &solution.b, deadline, &mut work)?;
    let request_bytes = serde_json::to_vec(&request).map_err(|error| error.to_string())?;
    let effective_rank = solution.a.len();
    let artifact = json!({"schemaVersion":1,"kind":"experimental-whole-ffn-surrogate-v1",
        "enabled":false,"qualification":"unqualified-research-only","source":request.source,
        "site":request.site,"composition":"replace-complete-ffn-branch",
        "teacherAdapterIncludedInTarget":true,"applySkippedSiteAdapterAgain":false,
        "formula":"y = sum_k b[k] * dot(a[k], normalized_original_ffn_input)",
        "controls":request.controls,"rank":effective_rank,"fitInputSha256":hash(&request_bytes)?,
        "factors":{"layout":"rank-major-f32-le","a":solution.a,"b":solution.b,
            "aSha256":factor_hash(&solution.a)?,"bSha256":factor_hash(&solution.b)?},
        "runtimeImplemented":false,"behaviorQualified":false});
    let bytes = serde_json::to_vec_pretty(&artifact).map_err(|error| error.to_string())?;
    // Finish fallible digest work before publishing the immutable candidate.
    let artifact_sha256 = hash(&bytes)?;
    solver::charge(&mut work, 0, deadline)?;
    if let Some(path) = &request.output_path {
        crate::weight_patch::publish(path, &bytes)?;
    }
    Ok(
        json!({"schemaVersion":1,"status":"fitted-unqualified","artifact":artifact,
        "artifactSha256":artifact_sha256,"outputPath":request.output_path,
        "diagnostics":diagnostics,"diagnosticArithmetic":"F64 reduction of exported F32 factors; not a runtime parity oracle",
        "requestedRank":request.controls.rank,"effectiveRank":effective_rank,
        "factorPayloadBytes":effective_rank * WIDTH * 2 * 4,
        "estimatedProjectionMultiplyAccumulatesPerToken":effective_rank * WIDTH * 2,
        "originalFfnProjectionMultiplyAccumulatesPerToken":3 * WIDTH * 17_408,
        "estimatesExclude":"normalization, nonlinearities, packing, dispatch, allocation, cache and adapter costs",
        "limits":{"maxRows":MAX_ROWS,"maxRank":MAX_RANK,"maxCellVisits":solver::MAX_WORK,
            "timeBudgetMilliseconds":request.controls.time_budget_milliseconds,"cellVisits":work,
            "timeBudgetScope":"typed-admission-through-export-preparation; last-check-before-publication",
            "requiresOriginalAggregateSupervisorDeadlineFor":["CLI-intake","synchronous-publication-and-cleanup","CLI-output"]},
        "developmentUsedForFit":false,"finalHeldoutUsed":false,"sourceAuthenticated":false,
        "runtimeImplemented":false,"behaviorQualified":false,"performanceQualified":false,
        "processMemoryQualified":false,"installed":false}),
    )
}
