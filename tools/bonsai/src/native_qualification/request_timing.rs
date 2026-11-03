//! Fixed source-session attribution selection and fail-closed receipt binding.

use serde_json::{Value, json};

use super::Request;

pub(super) fn plan(request: &Request, pins: &Value) -> Value {
    json!({"Status":"plan-only", "CaptureMode":"request-timing", "ModelProcessStarted":false,
        "Representation":"current-repacked-PTQ1_0", "AdapterCondition":"standard", "AdapterScale":1.3,
        "RustBackend":request.rust_backend,"RustRowWorkers":request.row_workers,
        "Messages":messages(),"OutputTokens":1,"MaximumRenderedPromptTokens":128,
        "NativeSamples":0,"RustSamples":1,"RustSource":pins["rustSource"],
        "WorkBudgetSeconds":240,"WholeJobMinutes":5,"CleanupAndVerificationReserveSeconds":32,
        "JobBytes":12_348_030_976_u64,"AdmissionBytes":16_642_998_272_u64,
        "PhysicalReserveBytes":4_294_967_296_u64,
        "Scope":"one source-built production SwiftSession request; no-op text consumer; no HTTP or released-binary execution",
        "WriterBasisQualified":false,"AppTpsQualified":false,"DefaultMayChange":false})
}

pub(super) fn messages() -> Value {
    json!([{"role":"user","content":"Name one tree. Answer with only its name."}])
}

#[cfg(any(windows, test))]
pub(super) fn deadline(remaining: u64, unix_milliseconds: u64) -> crate::Result<u64> {
    let work = remaining
        .checked_sub(32)
        .filter(|seconds| (1..=208).contains(seconds))
        .ok_or("Request timing needs the original allowance plus its 32-second cleanup reserve.")?;
    unix_milliseconds
        .checked_add(work * 1000)
        .ok_or_else(|| "Request timing absolute deadline overflowed.".into())
}

#[cfg(any(windows, test))]
pub(super) fn arguments(
    request: &Request,
    checked: &Value,
    lane: &std::path::Path,
    messages: &std::path::Path,
    deadline: u64,
    source: &Value,
) -> Value {
    json!([
        "--use-repacked-ptq1",
        "--row-workers",
        request.row_workers.to_string(),
        "--rank-one-adapter",
        checked["Report"]["AdapterPath"],
        "1.3",
        "request-timing",
        checked["Report"]["ModelPath"],
        lane,
        request.rust_backend,
        messages,
        "1",
        deadline.to_string(),
        source["commit"]
    ])
}

#[cfg(any(windows, test))]
pub(super) fn expected(
    request: &Request,
    pins: &Value,
    checked: &Value,
    executable: &Value,
    input: &Value,
    deadline: u64,
) -> crate::Result<Value> {
    let runtime: Value =
        serde_json::from_str(include_str!("../../../../config/prism-inference.lock.json"))
            .map_err(|error| error.to_string())?;
    let backend = match request.rust_backend.as_str() {
        "auto" => "Auto",
        "scalar" => "Scalar",
        "neon" => "ExperimentalNeon",
        _ => return Err("Request timing requires one explicit supported backend.".into()),
    };
    Ok(
        json!({"schema_version":1,"kind":"swift-source-request-timing","capture_status":"returned",
        "source_commit_declaration":pins["rustSource"]["commit"],"executable_sha256":executable,
        "package_version":runtime["version"],"input_bytes_sha256":input,
        "weight_representation":"current-repacked-PTQ1_0","model_sha256":checked["Artifact"]["Sha256"],
        "selected_adapter_sha256":checked["AdapterLock"]["adapter"]["sha256"],"adapter_scale_bits":"3fa66666",
        "configured_row_workers":request.row_workers,"effective_row_workers":request.row_workers,
        "backend_requested":backend,
        "seed":42,"thinking_enabled":false,"prompt_reuse":"disabled",
        "sampler":"production SwiftSampler and model-declared native stops",
        "maximum_prompt_tokens":128,"output_tokens_reserved":1,"original_deadline_unix_milliseconds":deadline,
        "stage":"complete","model_loaded":true,"request_prepared":true,"diagnostic_succeeded":true,
        "session_succeeded":true,"runtime_entered":true,"runtime_succeeded":true,"reused_tokens":0}),
    )
}

#[cfg(any(windows, test))]
pub(super) fn validate(receipt: &Value, expected: &Value) -> crate::Result<()> {
    let identity = expected
        .as_object()
        .ok_or("Missing request timing identity.")?;
    if identity
        .iter()
        .any(|(field, value)| value.is_null() || receipt[field] != *value)
    {
        return Err("Request timing receipt differs from the admitted source, artifacts, settings or successful terminal state.".into());
    }
    for field in [
        "elapsed_milliseconds",
        "session_milliseconds",
        "load_milliseconds",
        "preparation_milliseconds",
        "runtime_milliseconds",
    ] {
        if receipt[field]
            .as_f64()
            .is_none_or(|value| !value.is_finite() || value <= 0.0)
        {
            return Err(
                "Request timing receipt is missing a positive finite operation duration.".into(),
            );
        }
    }
    let duration = |field: &str| receipt[field].as_f64().unwrap_or_default();
    if duration("runtime_milliseconds") > duration("session_milliseconds")
        || duration("load_milliseconds")
            + duration("preparation_milliseconds")
            + duration("session_milliseconds")
            > duration("elapsed_milliseconds")
    {
        return Err("Request timing operation durations exceed their enclosing brackets.".into());
    }
    let prompt = receipt["prompt_tokens"]
        .as_u64()
        .filter(|n| (1..=128).contains(n))
        .ok_or("Request timing receipt has no bounded prompt count.")?;
    let output = receipt["session_committed_generated_tokens"]
        .as_u64()
        .filter(|n| *n <= 1)
        .ok_or("Request timing receipt has no bounded committed-output count.")?;
    if receipt["runtime_committed_generated_tokens"].as_u64() != Some(output) {
        return Err("Request timing receipt has unbound prompt or committed-output counts.".into());
    }
    validate_phases(receipt, prompt, output)
}

#[cfg(any(windows, test))]
fn validate_phases(receipt: &Value, prompt: u64, output: u64) -> crate::Result<()> {
    let phases = receipt["phases"]
        .as_array()
        .ok_or("Missing request timing phases.")?;
    let names = [
        "sampler-snapshot",
        "rollback-snapshot",
        "prefix-preparation",
        "prefill-ingestion",
        "prefill-evaluation",
        "decode-evaluation",
        "sampling",
        "callback",
        "completion-snapshot",
        "rollback-restore",
        "sampler-restore",
    ];
    if phases.len() != names.len() {
        return Err("Request timing phase inventory is incomplete.".into());
    }
    for (phase, name) in phases.iter().zip(names) {
        let calls = phase["attempted_calls"].as_u64();
        let completed = phase["completed_calls"].as_u64();
        let tokens = phase["attempted_tokens"].as_u64();
        let completed_tokens = phase["completed_tokens_lower_bound"].as_u64();
        if phase["phase"] != name
            || calls.is_none()
            || completed.is_none()
            || completed != calls
            || tokens.is_none()
            || completed_tokens.is_none()
            || completed_tokens != tokens
            || phase["partial_progress_unknown"] != false
            || phase["milliseconds"]
                .as_f64()
                .is_none_or(|ms| !ms.is_finite() || ms < 0.0)
        {
            return Err(
                "Request timing phases contain missing, inconsistent or failed work.".into(),
            );
        }
    }
    // Only a fully returned successful request permits this binding. An early
    // sampled stop may commit zero tokens despite completing one selection.
    let work = [
        (1, 0),
        (1, 0),
        (1, 0),
        ((prompt - 1).div_ceil(64), prompt - 1),
        (1, 1),
        (output, output),
        (1, 1),
        (output, output),
        (0, 0),
        (0, 0),
        (0, 0),
    ];
    for (phase, (calls, tokens)) in phases.iter().zip(work) {
        if phase["completed_tokens_lower_bound"] != tokens
            || phase["completed_calls"] != calls
            || (calls == 0 && phase["milliseconds"].as_f64().is_none_or(|ms| ms > 0.0))
        {
            return Err(
                "Request timing phase units do not match successful prompt/output work.".into(),
            );
        }
    }
    let phase_milliseconds: f64 = phases
        .iter()
        .map(|phase| phase["milliseconds"].as_f64().unwrap_or_default())
        .sum();
    if phase_milliseconds > receipt["runtime_milliseconds"].as_f64().unwrap_or_default() {
        return Err("Request timing phase durations exceed the runtime bracket.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
