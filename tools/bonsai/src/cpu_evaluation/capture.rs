//! Original-deadline X2 capture policy and independent pair reconciliation.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
#[cfg(any(windows, test))]
use std::{fs, io::Write as _, time::Duration};

#[cfg(test)]
mod tests;
#[cfg(windows)]
mod windows;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CimProcessor {
    name: String,
    logical_processors: u16,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[cfg_attr(
    not(windows),
    expect(dead_code, reason = "Capture request fields are used only on Windows.")
)]
struct Request {
    operation: String,
    data_directory: PathBuf,
    cim_processors: Vec<CimProcessor>,
    #[serde(default)]
    observe_output_progress: bool,
    #[serde(default = "budget")]
    work_budget_seconds: u64,
    work_deadline_tick: Option<u64>,
    parent_process_id: Option<u32>,
    parent_creation_time_file_time: Option<u64>,
}

const fn budget() -> u64 {
    240
}

fn require(condition: bool, message: &str) -> crate::Result<()> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

fn external(path: &Path) -> crate::Result<()> {
    bonsai_artifacts::validate_external_path(path).map_err(|error| error.to_string())
}

fn observed_plan(request: &Request, native: &Value) -> crate::Result<Value> {
    let [cim] = request.cim_processors.as_slice() else {
        return Err("X2 screening requires one independent CIM processor observation.".into());
    };
    require(
        native["status"] == "observed"
            && native["architecture"] == "aarch64"
            && native["topology"]["logical_processors"] == cim.logical_processors,
        "CPU evaluation requires matching observed CIM and native CPU topology.",
    )?;
    let conditions = super::conditions::compare(
        json!({"operation":"compare-conditions", "before":native, "after":native}),
    )?;
    require(
        conditions["Unknown"]
            .as_array()
            .is_some_and(|items| !items.iter().any(|item| item == "host-identity-unavailable"))
            && native["host_identity"]["processor_name"]
                .as_str()
                .is_some_and(|name| name.trim() == cim.name.trim()),
        "CPU evaluation requires complete native host identity and matching CIM/native processor names.",
    )?;
    let mut plan = super::run(
        json!({"operation":"plan", "dataDirectory":request.data_directory,
        "processorName":cim.name, "logicalProcessors":cim.logical_processors}),
    )?;
    plan["HostIdentity"] = native["host_identity"].clone();
    plan["InstrumentedAttribution"] = request.observe_output_progress.into();
    Ok(plan)
}

#[cfg(any(windows, test))]
fn allowance(
    cap: u64,
    elapsed: Duration,
    deadline: u64,
    now: u64,
    frequency: u64,
) -> crate::Result<u64> {
    require(
        (1..=240).contains(&cap) && frequency != 0 && deadline != 0,
        "Invalid original X2 workload deadline.",
    )?;
    Duration::from_secs(cap)
        .saturating_sub(elapsed)
        .as_secs()
        .min(deadline.saturating_sub(now) / frequency)
        .checked_sub(20)
        .filter(|seconds| *seconds > 0)
        .ok_or_else(|| {
            "The complete X2 CPU work budget is exhausted; qualification remains pending.".into()
        })
}

#[cfg(any(windows, test))]
fn batch_allowance(remaining: u64, plan: &Value) -> crate::Result<u64> {
    require(
        plan["MaximumSamples"] == 4
            && plan["MaximumModelExecutionSeconds"] == 180
            && plan["PerSampleTimeoutSeconds"] == 45
            && plan["Schedule"].as_array().map(Vec::len) == Some(4),
        "The X2 workload must remain the complete fixed four-sample screen.",
    )?;
    require(
        remaining >= 201,
        "Setup left insufficient room for the complete four-sample X2 screen and teardown; qualification remains pending.",
    )?;
    Ok(remaining - 10)
}

#[cfg(any(windows, test))]
fn maintenance_identity<'a>(
    receipt: &'a Value,
    revision: &str,
    length: u64,
) -> crate::Result<&'a str> {
    require(
        receipt["SchemaVersion"] == 3
            && receipt["Revision"] == revision
            && crate::identity::is_lower_hex(revision, 40)
            && (1..=134_217_728).contains(&length),
        "Maintenance tool differs from the checked Runner revision.",
    )?;
    receipt["Sha256"]
        .as_str()
        .filter(|digest| crate::identity::is_lower_hex(digest, 64))
        .ok_or_else(|| "Missing or invalid maintenance executable digest.".into())
}

#[cfg(any(windows, test))]
fn read_json(path: &Path, maximum: u64) -> crate::Result<Value> {
    let file = bonsai_artifacts::open_locked_file(path).map_err(|error| error.to_string())?;
    require(
        file.metadata().map_err(|error| error.to_string())?.len() <= maximum,
        "X2 evidence exceeds its pre-read bound.",
    )?;
    let bytes = crate::bounded_input::read(file, maximum)
        .map_err(|error| error.into_message("X2 evidence grew beyond its byte bound."))?;
    serde_json::from_slice(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes))
        .map_err(|error| error.to_string())
}

#[cfg(any(windows, test))]
fn save(path: &Path, value: &Value) -> crate::Result<()> {
    external(path)?;
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    require(
        bytes.len() <= 16 * 1024 * 1024,
        "X2 evidence exceeds its publication bound.",
    )?;
    let temporary = path.with_extension(format!("{}.tmp", crate::startup_process::unique()));
    let result = (|| {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt as _;
            options.share_mode(0);
        }
        let mut file = options
            .open(&temporary)
            .map_err(|error| error.to_string())?;
        file.write_all(&bytes).map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
        drop(file);
        fs::rename(&temporary, path).map_err(|error| error.to_string())
    })();
    if external(&temporary).is_ok() {
        let _ = fs::remove_file(temporary);
    }
    result
}

#[cfg(any(windows, test))]
fn record(trial: &Value, sample: &Value, captured: &Value, cap: &Value) -> crate::Result<Value> {
    require(
        sample["Id"] == trial["Id"]
            && sample["Status"] == "completed"
            && captured["SchemaVersion"] == 1
            && captured["Text"].is_string()
            && captured["Sha256"].as_str()
                == captured["Text"]
                    .as_str()
                    .map(|text| crate::identity::sha256(text.as_bytes()))
                    .as_deref(),
        "Completed CPU sample or exact response identity is missing.",
    )?;
    let mut result = json!({});
    for field in [
        "Id",
        "CandidateId",
        "Workload",
        "Repetition",
        "Role",
        "PairPosition",
        "DecodeThreads",
        "PrefillThreads",
    ] {
        result[field] = trial[field].clone();
    }
    for field in [
        "Timing",
        "Counts",
        "WallMilliseconds",
        "PeakJobMemoryBytes",
        "MinimumAvailablePhysicalBytes",
        "ProcessMemory",
        "CpuConditions",
    ] {
        result[field] = sample[field].clone();
    }
    result["ResponseSha256"] = captured["Sha256"].clone();
    result["ReachedTokenCap"] = (sample["Counts"]["GeneratedSampledTokens"] == *cap).into();
    Ok(result)
}

#[cfg(any(windows, test))]
fn positive(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .filter(|number| number.is_finite() && *number > 0.0)
}

#[cfg(any(windows, test))]
fn compare_pair(pair: &[Value; 2], execution: &Value, instrumented: bool) -> crate::Result<Value> {
    let (baseline, candidate) = if pair[0]["Role"] == "baseline" {
        (&pair[0], &pair[1])
    } else {
        (&pair[1], &pair[0])
    };
    require(
        baseline["Role"] == "baseline"
            && candidate["Role"] == "candidate"
            && baseline["CandidateId"] == candidate["CandidateId"]
            && baseline["Workload"] == candidate["Workload"]
            && baseline["Repetition"] == candidate["Repetition"],
        "X2 comparison pair identities disagree.",
    )?;
    let matching = baseline["ReachedTokenCap"] == true
        && candidate["ReachedTokenCap"] == true
        && ["PromptTokens", "DecodeTokens"].iter().all(|key| {
            baseline["Timing"][key].as_u64().is_some()
                && baseline["Timing"][key] == candidate["Timing"][key]
        });
    let conditions =
        super::conditions::compare_samples(json!({"operation":"compare-sample-conditions",
        "baseline":baseline["CpuConditions"], "candidate":candidate["CpuConditions"]}))?;
    let ratios = [
        (
            &candidate["Timing"]["PromptMilliseconds"],
            &baseline["Timing"]["PromptMilliseconds"],
        ),
        (
            &candidate["Timing"]["DecodeMilliseconds"],
            &baseline["Timing"]["DecodeMilliseconds"],
        ),
        (
            &candidate["WallMilliseconds"],
            &baseline["WallMilliseconds"],
        ),
    ]
    .map(|(candidate, baseline)| {
        let ratio = positive(candidate)? / positive(baseline)?;
        ratio.is_finite().then_some(ratio)
    });
    let comparable = !instrumented
        && matching
        && execution["Comparable"] == true
        && conditions["Comparable"] == true
        && ratios.iter().all(Option::is_some);
    Ok(
        json!({"CandidateId":candidate["CandidateId"], "Workload":candidate["Workload"], "Repetition":candidate["Repetition"],
        "MatchingTokenCountsAtFixedCap":matching, "ExecutionConditionsStatus":execution["Status"],
        "PairExecutionConditions":conditions, "ComparableAtFixedTokenCap":comparable, "InstrumentedAttribution":instrumented,
        "CandidateOverBaselinePromptMilliseconds":if comparable {ratios[0]} else {None},
        "CandidateOverBaselineDecodeMilliseconds":if comparable {ratios[1]} else {None},
        "CandidateOverBaselineWallMilliseconds":if comparable {ratios[2]} else {None},
        "ExactResponseMatch":baseline["ResponseSha256"] == candidate["ResponseSha256"]}),
    )
}

pub(super) fn run(value: Value) -> crate::Result<Value> {
    let request: Request = serde_json::from_value(value).map_err(|error| error.to_string())?;
    external(&request.data_directory)?;
    require(
        (1..=240).contains(&request.work_budget_seconds),
        "X2 work allowance must be in 1..240 seconds.",
    )?;
    if request.operation == "observed-plan" {
        return observed_plan(&request, &crate::host_cpu::run(&json!({}))?);
    }
    require(
        request.operation == "capture",
        "Unknown X2 capture operation.",
    )?;
    #[cfg(windows)]
    {
        windows::run(&request)
    }
    #[cfg(not(windows))]
    {
        Err("X2 capture requires native Windows ARM64 owner CI.".into())
    }
}
