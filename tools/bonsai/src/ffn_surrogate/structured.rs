//! Paired structured-FFN compensation; supplied capture claims remain unauthenticated.

mod application;
#[cfg(test)]
mod tests;

use super::{Controls, Site, Source, Split, WIDTH, hash, identifier, solver};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    time::{Duration, Instant},
};

const STUDENT_PROFILE: &str = "experimental-ffn-group-omission-cpu-scalar-unfused-v1";
const TRANSFORM: &str = "independent-contiguous-1024-signed-hadamard-v1";
const ADAPTER_POLICY: &str = "zero-omitted-original-hidden-before-standard-adapter";
const GROUP_WIDTH: usize = 1_024;
const HIDDEN_WIDTH: usize = 17_408;
const APPLICATION_ARITHMETIC: &str = "rank-major-source-order-f32-dot-add-v1";

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StudentExecution {
    execution_profile: String,
    graph_identity: String,
    prefix_policy_sha256: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SignIdentity {
    full_signs_sha256: String,
    down_sign_offset: u32,
    down_sign_count: u32,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Omission {
    group_width: usize,
    original_hidden_width: usize,
    omitted_groups: Vec<usize>,
    transform: String,
    retained_adapter_policy: String,
    sign_identity: SignIdentity,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Row {
    sample_id: String,
    group_id: String,
    token_index: u32,
    split: Split,
    input: Vec<f32>,
    teacher_target: Vec<f32>,
    retained_branch: Vec<f32>,
    original_prefix_teacher_target: Vec<f32>,
    shared_input_sha256: String,
    student_prefix_sha256: String,
    original_prefix_sha256: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    schema_version: u32,
    source: Source,
    student_execution: StudentExecution,
    capture_sha256: String,
    site: Site,
    omission: Omission,
    rows: Vec<Row>,
    #[serde(default)]
    controls: Controls,
    output_path: Option<PathBuf>,
}

fn identity(value: &str, width: usize) -> crate::Result<()> {
    if !crate::identity::is_lower_hex(value, width) {
        return Err("Noncanonical structured-compensation identity.".into());
    }
    Ok(())
}

fn vector_hash(values: &[f32]) -> crate::Result<String> {
    let mut digest = crate::identity::Hasher::new()?;
    for value in values {
        digest.update(&value.to_le_bytes())?;
    }
    digest.finalize_hex()
}

fn sign_hash(request: &Request) -> crate::Result<String> {
    let mut digest = crate::identity::Hasher::new()?;
    digest.update(b"bonsai-ffn-model-bound-signs-v1\0")?;
    digest.update(request.source.model_sha256.as_bytes())?;
    digest.update(&request.site.layer.to_le_bytes())?;
    digest.update(request.omission.sign_identity.full_signs_sha256.as_bytes())?;
    digest.update(
        &request
            .omission
            .sign_identity
            .down_sign_offset
            .to_le_bytes(),
    )?;
    digest.update(&request.omission.sign_identity.down_sign_count.to_le_bytes())?;
    digest.finalize_hex()
}

impl Request {
    fn validate_source(&self) -> crate::Result<()> {
        let source = &self.source;
        for (value, width) in [
            (&source.model_sha256, 64),
            (&source.runtime_sha256, 64),
            (&source.runtime_source_commit, 40),
            (&source.adapter_scale_bits, 8),
            (&self.capture_sha256, 64),
            (&self.student_execution.prefix_policy_sha256, 64),
            (&self.omission.sign_identity.full_signs_sha256, 64),
        ] {
            identity(value, width)?;
        }
        if let Some(adapter) = &source.adapter_sha256 {
            identity(adapter, 64)?;
        }
        let scale = f32::from_bits(
            u32::from_str_radix(&source.adapter_scale_bits, 16)
                .map_err(|error| error.to_string())?,
        );
        if self.schema_version != 2
            || !(3..=super::MAX_ROWS).contains(&self.rows.len())
            || !scale.is_finite()
            || !(0.0..=2.0).contains(&scale)
            || (source.adapter_sha256.is_none() && source.adapter_scale_bits != "00000000")
            || !matches!(
                source.weight_representation.as_str(),
                "PQ2_0" | "current-repacked-PTQ1_0"
            )
            || source.execution_profile != super::PROFILE
            || !identifier(&source.graph_identity)
            || self.student_execution.execution_profile != STUDENT_PROFILE
            || !identifier(&self.student_execution.graph_identity)
            || self.student_execution.graph_identity == source.graph_identity
            || self.site.layer >= 64
            || self.site.input_width != WIDTH
            || self.site.output_width != WIDTH
            || self.site.input_basis != "original-ffn-normalized-input"
            || self.site.target_basis != "teacher-minus-retained-ffn-branch"
        {
            return Err("Unsupported paired teacher/student execution, basis or shape.".into());
        }
        Ok(())
    }

    fn validate_omission(&self) -> crate::Result<()> {
        let omission = &self.omission;
        if omission.group_width != GROUP_WIDTH
            || omission.original_hidden_width != HIDDEN_WIDTH
            || omission.omitted_groups.is_empty()
            || omission.omitted_groups.len() >= HIDDEN_WIDTH / GROUP_WIDTH
            || omission.omitted_groups.iter().any(|&group| group >= 17)
            || omission
                .omitted_groups
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
            || omission.transform != TRANSFORM
            || omission.retained_adapter_policy != ADAPTER_POLICY
            || omission.sign_identity.down_sign_offset != 11_264
            || omission.sign_identity.down_sign_count != 17_408
        {
            return Err(
                "Require sorted distinct whole Hadamard groups and original sign/adapter indexing."
                    .into(),
            );
        }
        Ok(())
    }

    fn validate_rows(&self, deadline: Instant, work: &mut u64) -> crate::Result<()> {
        let mut groups = BTreeMap::new();
        let mut samples = BTreeMap::new();
        let mut inputs = BTreeMap::new();
        let mut student_prefixes = BTreeMap::new();
        let mut original_prefixes = BTreeMap::new();
        let mut tokens = BTreeSet::new();
        let mut fit_count = 0;
        let mut development_count = 0;
        for row in &self.rows {
            if !identifier(&row.sample_id)
                || !identifier(&row.group_id)
                || !tokens.insert((&row.sample_id, row.token_index))
                || [
                    &row.input,
                    &row.teacher_target,
                    &row.retained_branch,
                    &row.original_prefix_teacher_target,
                ]
                .into_iter()
                .any(|vector| {
                    vector.len() != WIDTH || vector.iter().any(|value| !value.is_finite())
                })
            {
                return Err(
                    "Paired rows require unique tokens and finite matching vectors.".into(),
                );
            }
            for value in [
                &row.shared_input_sha256,
                &row.student_prefix_sha256,
                &row.original_prefix_sha256,
            ] {
                identity(value, 64)?;
            }
            solver::charge(work, WIDTH * 5, deadline)?;
            if vector_hash(&row.input)? != row.shared_input_sha256 {
                return Err("sharedInputSha256 does not bind the supplied F32-le input.".into());
            }
            for (map, key) in [
                (&mut groups, &row.group_id),
                (&mut samples, &row.sample_id),
                (&mut inputs, &row.shared_input_sha256),
                (&mut student_prefixes, &row.student_prefix_sha256),
                (&mut original_prefixes, &row.original_prefix_sha256),
            ] {
                if map
                    .insert(key, row.split)
                    .is_some_and(|split| split != row.split)
                {
                    return Err(
                        "A sample, content group, input or prefix crosses fit/development splits."
                            .into(),
                    );
                }
            }
            match row.split {
                Split::Fit => fit_count += 1,
                Split::Development => development_count += 1,
            }
        }
        if fit_count < 2 || development_count == 0 || self.controls.rank > fit_count {
            return Err("Require two fit rows, a development row and rank <= fit rows.".into());
        }
        Ok(())
    }

    fn validate(&self, deadline: Instant, work: &mut u64) -> crate::Result<()> {
        self.validate_source()?;
        self.validate_omission()?;
        self.validate_rows(deadline, work)?;
        if let Some(path) = &self.output_path {
            bonsai_artifacts::validate_external_path(path).map_err(|error| error.to_string())?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Default)]
struct Totals {
    rows: u32,
    teacher_energy: f64,
    original_teacher_energy: f64,
    uncompensated_error: f64,
    compensated_error: f64,
    prefix_drift: f64,
    original_prefix_error: f64,
}

fn ratio(error: f64, energy: f64) -> Option<f64> {
    (energy > 0.0).then_some(error / energy)
}

fn diagnostics(
    request: &Request,
    solution: &solver::Solution,
    deadline: Instant,
    work: &mut u64,
) -> crate::Result<([Totals; 2], Value)> {
    let mut totals = [Totals::default(); 2];
    for row in &request.rows {
        let prediction = solver::prediction(&row.input, &solution.a, &solution.b, deadline, work)?;
        let total = &mut totals[usize::from(row.split == Split::Development)];
        total.rows += 1;
        for (((&teacher, &retained), &original), &correction) in row
            .teacher_target
            .iter()
            .zip(&row.retained_branch)
            .zip(&row.original_prefix_teacher_target)
            .zip(&prediction)
        {
            let teacher = f64::from(teacher);
            let retained = f64::from(retained);
            let original = f64::from(original);
            let missing = teacher - retained;
            total.teacher_energy += teacher.powi(2);
            total.original_teacher_energy += original.powi(2);
            total.uncompensated_error += missing.powi(2);
            total.compensated_error += (missing - correction).powi(2);
            total.prefix_drift += (teacher - original).powi(2);
            total.original_prefix_error += (retained + correction - original).powi(2);
        }
        solver::charge(work, WIDTH * 6, deadline)?;
    }
    let result = summarize(&totals)?;
    Ok((totals, result))
}

fn summarize(totals: &[Totals; 2]) -> crate::Result<Value> {
    let mut result = serde_json::Map::new();
    for (name, total) in ["fit", "development"].into_iter().zip(totals) {
        if [
            total.teacher_energy,
            total.original_teacher_energy,
            total.uncompensated_error,
            total.compensated_error,
            total.prefix_drift,
            total.original_prefix_error,
        ]
        .into_iter()
        .any(|value| !value.is_finite())
        {
            return Err("Nonfinite structured exported-factor diagnostics.".into());
        }
        result.insert(name.into(), json!({"rows":total.rows,
            "teacherSquaredEnergy":total.teacher_energy,
            "originalPrefixTeacherSquaredEnergy":total.original_teacher_energy,
            "uncompensatedSquaredError":total.uncompensated_error,
            "compensatedSquaredError":total.compensated_error,
            "uncompensatedRelativeSquaredError":ratio(total.uncompensated_error,total.teacher_energy),
            "relativeSquaredError":ratio(total.compensated_error,total.teacher_energy),
            "teacherPrefixDriftSquaredError":total.prefix_drift,
            "teacherPrefixDriftRelativeSquaredError":ratio(total.prefix_drift,total.original_teacher_energy),
            "correctedBranchVersusOriginalPrefixRelativeSquaredError":ratio(total.original_prefix_error,total.original_teacher_energy)}));
    }
    Ok(result.into())
}

fn screen(totals: &[Totals; 2]) -> Value {
    let [fit, development] = totals;
    let mut reasons = Vec::new();
    if totals
        .iter()
        .any(|total| total.teacher_energy <= 0.0 || total.original_teacher_energy <= 0.0)
    {
        reasons.push("nonzero-teacher-energy-required-in-each-split");
    }
    if let (Some(fit), Some(development)) = (
        ratio(fit.compensated_error, fit.teacher_energy),
        ratio(development.compensated_error, development.teacher_energy),
    ) {
        if development > 0.1 {
            reasons.push("development-relative-squared-error-exceeds-limit");
        }
        if development - fit > 0.05 {
            reasons.push("development-minus-fit-relative-squared-error-exceeds-limit");
        }
    }
    if development.compensated_error > development.uncompensated_error {
        reasons.push("compensation-worsens-uncompensated-development-error");
    }
    json!({"kind":"algebraic-development-screen-only","passed":reasons.is_empty(),
        "maximumDevelopmentRelativeSquaredError":0.1,
        "maximumDevelopmentMinusFitRelativeSquaredError":0.05,
        "requireNoWorseThanUncompensatedDevelopment":true,"rejectionReasons":reasons,
        "thresholdsAreDesignChoices":true,"answerQualityQualified":false,
        "changedPrefixDriftRecoveryQualified":false})
}

fn geometry(request: &Request, rank: usize) -> Value {
    let omitted = request.omission.omitted_groups.len();
    let factor_bytes = rank * WIDTH * 2 * 4;
    let factor_macs = rank * WIDTH * 2;
    let block_bytes = if request.source.weight_representation == "PQ2_0" {
        34
    } else {
        28
    };
    let packed_group_bytes = 3 * WIDTH * GROUP_WIDTH / 128 * block_bytes;
    let gross_bytes = omitted * packed_group_bytes;
    let removed_macs = omitted * 3 * WIDTH * GROUP_WIDTH;
    json!({"weightRepresentation":request.source.weight_representation,
        "packedBlockCoefficients":128,"packedBlockBytes":block_bytes,
        "perSite":{"grossOmittedPackedPayloadBytes":gross_bytes,
        "newFactorPayloadBytes":factor_bytes,"netPackedMinusFactorPayloadBytes":gross_bytes-factor_bytes,
        "originalBaseProjectionMultiplyAccumulatesPerToken":3*WIDTH*HIDDEN_WIDTH,
        "omittedBaseProjectionMultiplyAccumulatesPerToken":removed_macs,
        "newFactorProjectionMultiplyAccumulatesPerToken":factor_macs,
        "retainedBasePlusFactorProjectionMultiplyAccumulatesPerToken":3*WIDTH*HIDDEN_WIDTH-removed_macs+factor_macs},
        "samePlanAtAll64Sites":{"grossOmittedPackedPayloadBytes":gross_bytes*64,
            "newFactorPayloadBytes":factor_bytes*64,"netPackedMinusFactorPayloadBytes":(gross_bytes-factor_bytes)*64},
        "all64SitesIsGeometryOnly":true,"selectiveLoadingImplemented":false,
        "excludes":"norms, nonlinearities, adapters, signs/metadata, scratch, allocator capacity, cache/state and kernel costs",
        "processMemoryQualified":false,"performanceQualified":false})
}

pub(super) fn run(value: Value, started: Instant) -> crate::Result<Value> {
    let request: Request = serde_json::from_value(value).map_err(|error| error.to_string())?;
    if !(1..=super::MAX_RANK).contains(&request.controls.rank)
        || !request.controls.ridge.is_finite()
        || !(1e-8..=1e6).contains(&request.controls.ridge)
        || !(1..=30_000).contains(&request.controls.time_budget_milliseconds)
    {
        return Err("Structured controls exceed finite rank, ridge or time limits.".into());
    }
    let deadline = started
        .checked_add(Duration::from_millis(
            request.controls.time_budget_milliseconds,
        ))
        .ok_or("Structured compensation deadline overflow.")?;
    let mut work = 0;
    request.validate(deadline, &mut work)?;
    let mut rows = Vec::new();
    for row in request.rows.iter().filter(|row| row.split == Split::Fit) {
        solver::charge(&mut work, WIDTH, deadline)?;
        rows.push(solver::FitRow {
            input: &row.input,
            target: row
                .teacher_target
                .iter()
                .zip(&row.retained_branch)
                .map(|(&teacher, &retained)| f64::from(teacher) - f64::from(retained))
                .collect(),
        });
    }
    let solution = solver::fit_data(
        &rows,
        request.controls.rank,
        request.controls.ridge,
        deadline,
        &mut work,
    )?;
    let (totals, diagnostics) = diagnostics(&request, &solution, deadline, &mut work)?;
    let reconstruction_screen = screen(&totals);
    let (application_totals, application_diagnostics) =
        application::diagnostics(&request, &solution, deadline, &mut work)?;
    let application_screen = screen(&application_totals);
    let screening = application::combined_screen(&reconstruction_screen, &application_screen)?;
    let passed = screening["passed"] == true;
    let rank = solution.a.len();
    let request_bytes = serde_json::to_vec(&request).map_err(|error| error.to_string())?;
    let fit_input_sha256 = hash(&request_bytes)?;
    let model_bound_sign_sha256 = sign_hash(&request)?;
    let resource_estimates = geometry(&request, rank);
    let mut artifact = Value::Null;
    let mut artifact_sha256 = None;
    if passed {
        artifact = json!({"schemaVersion":2,"kind":"experimental-structured-ffn-compensation-v2",
            "enabled":false,"qualification":"unqualified-research-only","source":request.source,
            "studentExecution":request.student_execution,"captureSha256":request.capture_sha256,
            "site":request.site,"omission":request.omission,"modelBoundSignSha256":model_bound_sign_sha256,
            "signPayloadArithmetic":"complete metadata sign_values array in original order, I32-le +/-1",
            "signBindingRecipe":"SHA256(domain bonsai-ffn-model-bound-signs-v1 NUL || modelSha256 ASCII || layer U32-le || fullSignsSha256 ASCII || downSignOffset U32-le || downSignCount U32-le)",
            "composition":"retained-ffn-branch-plus-normalized-input-low-rank-compensation",
            "teacherAndRetainedTargetsIncludeStandardAdapter":true,"applyStandardAdapterAgain":false,
            "formula":"retained_branch + sum_k b[k] * dot(a[k], normalized_original_basis_student_input)",
            "factorApplicationArithmetic":APPLICATION_ARITHMETIC,
            "fitTargetArithmetic":"F64(teacherTarget F32) - F64(retainedBranch F32)",
            "fitInputSha256":fit_input_sha256,"controls":request.controls,"rank":rank,
            "factors":{"layout":"rank-major-f32-le","a":solution.a,"b":solution.b,
                "aSha256":super::factor_hash(&solution.a)?,"bSha256":super::factor_hash(&solution.b)?},
            "developmentScreen":screening,
            "exportedFactorF64DevelopmentScreen":reconstruction_screen,
            "sourceOrderF32DevelopmentScreen":application_screen,"sourceAuthenticated":false,
            "changedPrefixDriftRecoveryQualified":false,"runtimeImplemented":false,
            "runtimeImplementedScope":"released-serving-and-selective-loading","behaviorQualified":false});
        let bytes = serde_json::to_vec_pretty(&artifact).map_err(|error| error.to_string())?;
        artifact_sha256 = Some(hash(&bytes)?);
        solver::charge(&mut work, 0, deadline)?;
        if let Some(path) = &request.output_path {
            crate::weight_patch::publish(path, &bytes)?;
        }
    }
    if !passed {
        solver::charge(&mut work, 0, deadline)?;
    }
    Ok(
        json!({"schemaVersion":2,"status":if passed {"development-screen-passed-unqualified"} else {"development-rejected"},
        "artifact":artifact,"artifactSha256":artifact_sha256,"outputPath":if passed {request.output_path.as_ref()} else {None},
        "fitInputSha256":fit_input_sha256,"diagnostics":diagnostics,"developmentScreen":screening,
        "factorApplicationArithmetic":APPLICATION_ARITHMETIC,
        "sourceOrderF32Diagnostics":application_diagnostics,
        "exportedFactorF64DevelopmentScreen":reconstruction_screen,
        "sourceOrderF32DevelopmentScreen":application_screen,
        "diagnosticArithmetic":"F64 reductions of exported F32 factors; not a native runtime parity oracle",
        "driftScope":"Local teacher reconstruction on changed-prefix inputs does not recover original-prefix state or qualify end-to-end answer quality",
        "requestedRank":request.controls.rank,"effectiveRank":rank,"resourceEstimates":resource_estimates,
        "limits":{"maxRows":super::MAX_ROWS,"maxRank":super::MAX_RANK,"maxCellVisits":solver::MAX_WORK,
            "timeBudgetMilliseconds":request.controls.time_budget_milliseconds,"cellVisits":work,
            "timeBudgetScope":"typed-admission-through-export-preparation; last-check-before-publication",
            "requiresOriginalAggregateSupervisorDeadlineFor":["CLI-intake","synchronous-publication-and-cleanup","CLI-output"]},
        "developmentUsedForFit":false,"finalHeldoutUsed":false,"sourceAuthenticated":false,
        "captureAndPrefixDeclarationsAuthenticated":false,"signPayloadAuthenticated":false,
        "runtimeImplemented":false,"behaviorQualified":false,"performanceQualified":false,
        "runtimeImplementedScope":"released-serving-and-selective-loading",
        "processMemoryQualified":false,"installed":false}),
    )
}
