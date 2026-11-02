//! Fixed native numerical/row capture plans and deadline-bound owner orchestration.

#[cfg(any(windows, test))]
use std::fs;
use std::fs::OpenOptions;
use std::io::Read as _;
#[cfg(any(windows, test))]
use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{Value, json};

#[cfg(windows)]
mod windows;

#[cfg(any(windows, test))]
const LIMIT: u64 = 1_048_576;
#[cfg(windows)]
const CHANNEL: &str = "nightly-2026-11-02-aarch64-pc-windows-msvc";
const PENDING: &[&str] = &[
    "no-adapter",
    "loaded-zero-identity",
    "nonzero-change",
    "scalar",
    "explicit-neon",
    "writer-authoring-basis",
    "quality",
    "performance",
    "full-context",
    "service",
];

#[derive(Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Kind {
    #[default]
    Ptq1,
    Rows,
}

#[derive(Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum AdapterCondition {
    #[default]
    Standard,
    LoadedZero,
    NoAdapter,
}

impl AdapterCondition {
    const fn name(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::LoadedZero => "loaded-zero",
            Self::NoAdapter => "no-adapter",
        }
    }

    const fn scale(self) -> f64 {
        match self {
            Self::Standard => 1.3,
            Self::LoadedZero | Self::NoAdapter => 0.0,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Operation {
    Plan,
    Capture,
    SourcePins,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[cfg_attr(
    not(windows),
    expect(
        dead_code,
        reason = "Capture requests are decoded consistently, but execution is Windows-only."
    )
)]
struct Request {
    operation: Operation,
    #[serde(default)]
    kind: Kind,
    repository_directory: PathBuf,
    #[serde(default = "auto_backend")]
    rust_backend: String,
    #[serde(default = "one_worker")]
    row_workers: u8,
    #[serde(default)]
    adapter_condition: AdapterCondition,
    data_directory: Option<PathBuf>,
    capture_tools_directory: Option<PathBuf>,
    work_budget_seconds: Option<u64>,
    work_deadline_tick: Option<u64>,
    parent_process_id: Option<u32>,
    parent_creation_time_file_time: Option<u64>,
}

fn auto_backend() -> String {
    "auto".into()
}
const fn one_worker() -> u8 {
    1
}

fn read_bounded(path: &Path, limit: u64) -> crate::Result<Vec<u8>> {
    if !path.is_absolute() {
        return Err("Qualification paths must be expanded and absolute.".into());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        options.share_mode(1).custom_flags(0x0020_0000);
    }
    let file = options.open(path).map_err(|error| error.to_string())?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file() || bonsai_artifacts::is_redirect(&metadata) || metadata.len() > limit {
        return Err("Qualification input exceeds its ordinary-file byte bound.".into());
    }
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if u64::try_from(bytes.len()).map_err(|error| error.to_string())? > limit {
        return Err("Qualification input grew beyond its byte bound.".into());
    }
    Ok(bytes)
}

fn read_json(path: &Path, limit: u64) -> crate::Result<Value> {
    serde_json::from_slice(&read_bounded(path, limit)?).map_err(|error| error.to_string())
}

fn source_pins(repository: &Path, kind: Kind) -> crate::Result<Value> {
    let pins = read_json(
        &repository.join(match kind {
            Kind::Ptq1 => "tools/native-reference/pins.json",
            Kind::Rows => "tools/row-parallel/pins.json",
        }),
        65_536,
    )?;
    let source = &pins["rustSource"];
    let commit = source["commit"].as_str().ok_or("Missing source commit.")?;
    if !crate::identity::is_lower_hex(commit, 40)
        || !source["sha256"]
            .as_str()
            .is_some_and(|value| crate::identity::is_lower_hex(value, 64))
        || !source["bytes"]
            .as_u64()
            .is_some_and(|value| (1..=67_108_864).contains(&value))
        || source["url"]
            != format!("https://github.com/symbiogenesis/prism-inference/archive/{commit}.zip")
    {
        return Err("Reviewed exact Prism source archive pins are required.".into());
    }
    if kind == Kind::Ptq1 {
        let runtime = read_json(&repository.join("config/prism-inference.lock.json"), 65_536)?;
        require_runtime_source(source, &runtime)?;
    }
    Ok(pins)
}

fn require_runtime_source(source: &Value, runtime: &Value) -> crate::Result<()> {
    if source["commit"] != runtime["commit"]
        || source["commit"] != runtime["Source"]["Commit"]
        || source["url"] != runtime["Source"]["Url"]
        || source["bytes"] != runtime["Source"]["Bytes"]
        || source["sha256"] != runtime["Source"]["Sha256"]
    {
        return Err("PTQ1 numerical source differs from the pinned Rust runtime; update and review capture pins first.".into());
    }
    Ok(())
}

fn plan(request: &Request, pins: &Value) -> Value {
    let mut value = json!({"Status": "plan-only", "ModelProcessStarted": false,
        "SuppliedTokens": [248_045], "AdapterScale": request.adapter_condition.scale(), "RustBackend": request.rust_backend,
        "WorkBudgetSeconds": 240, "WholeJobMinutes": 5, "RequiredFreeDiskBytes": 1_073_741_824,
        "ContextTokens": 8192, "JobBytes": 12_348_030_976_u64, "AdmissionBytes": 16_642_998_272_u64,
        "PhysicalReserveBytes": 4_294_967_296_u64, "RustSource": pins["rustSource"],
        "WriterBasisQualified": false, "DefaultMayChange": false });
    if request.kind == Kind::Ptq1 {
        value["Representation"] = json!("current-repacked-PTQ1_0");
        value["RustRowWorkers"] = json!(request.row_workers);
        value["AdapterCondition"] = json!(request.adapter_condition.name());
        value["AdapterLoaded"] = json!(request.adapter_condition != AdapterCondition::NoAdapter);
        value["NativeSamples"] = json!(1);
        value["RustSamples"] = json!(1);
        value["EstimatedSnapshotBytes"] = json!(315_971_136);
        value["NativeSource"] = pins["nativeSource"].clone();
        value["PendingControls"] = json!(PENDING);
    } else {
        value["Representation"] = json!("PQ2_0");
        value["Conditions"] =
            json!([{"Name": "serial", "RowWorkers": 1}, {"Name": "parallel", "RowWorkers": 4}]);
        value["EstimatedSnapshotBytes"] = json!(316_035_072);
        value["SourcePinsAvailable"] = json!(true);
        value["AppTpsQualified"] = json!(false);
    }
    value
}

#[cfg(any(windows, test))]
fn sample_arguments(
    request: &Request,
    admission: &Value,
    runtime: Option<&Value>,
    lane: &Path,
    workers: u8,
) -> crate::Result<Value> {
    let standard = request.adapter_condition == AdapterCondition::Standard;
    let loaded = request.adapter_condition != AdapterCondition::NoAdapter;
    if workers == 0 {
        return Ok(json!([
            "capture",
            runtime.ok_or("Missing reference runtime.")?["Runtime"]["runtimeDirectory"],
            admission["ModelPath"],
            lane,
            if loaded {
                admission["AdapterPath"].clone()
            } else {
                json!("none")
            },
            if standard { "1.3" } else { "0" }
        ]));
    }
    let mut arguments = if request.kind == Kind::Ptq1 {
        vec![json!("--use-repacked-ptq1")]
    } else {
        vec![]
    };
    arguments.extend([json!("--row-workers"), json!(workers.to_string())]);
    if loaded {
        arguments.extend([
            json!("--rank-one-adapter"),
            admission["AdapterPath"].clone(),
            json!(if standard { "1.3" } else { "0" }),
        ]);
    }
    arguments.extend([
        json!("forward"),
        admission["ModelPath"].clone(),
        json!(lane),
        json!(request.rust_backend),
        json!("248045"),
    ]);
    Ok(json!(arguments))
}

#[cfg(any(windows, test))]
pub(crate) fn save_report(path: &Path, report: &Value) -> crate::Result<()> {
    bonsai_artifacts::validate_external_path(path).map_err(|error| error.to_string())?;
    let bytes = serde_json::to_vec_pretty(report).map_err(|error| error.to_string())?;
    if u64::try_from(bytes.len()).map_err(|error| error.to_string())? > LIMIT {
        return Err("Numerical qualification receipt exceeds its fixed byte bound.".into());
    }
    let temporary = path.with_extension(format!("{}.tmp", crate::native_startup::nonce()?));
    let result = (|| {
        let mut options = OpenOptions::new();
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
    if temporary.exists() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(any(windows, test))]
fn remaining_seconds(
    cap: std::time::Duration,
    elapsed: std::time::Duration,
    deadline: u64,
    now: u64,
    frequency: u64,
) -> crate::Result<u64> {
    if frequency == 0 {
        return Err("Invalid monotonic frequency.".into());
    }
    let remaining = cap
        .saturating_sub(elapsed)
        .as_secs()
        .min(deadline.saturating_sub(now) / frequency);
    if remaining == 0 {
        return Err("The original complete numerical qualification budget is exhausted.".into());
    }
    Ok(remaining)
}

/// Plan or execute one fixed PTQ1/row qualification, or review current source pins.
///
/// Captures require source-bound hosted tools, main-only owner CI and an original
/// monotonic deadline. The command never builds tools or downloads models.
///
/// # Errors
/// Rejects stale pins, invalid options, untrusted bundles, missing owner clocks,
/// exhausted budgets, failed owned processes and incomplete comparisons.
pub fn run(value: Value) -> crate::Result<Value> {
    let request: Request = serde_json::from_value(value).map_err(|error| error.to_string())?;
    if !["auto", "scalar", "neon"].contains(&request.rust_backend.as_str())
        || !matches!(request.row_workers, 1 | 4)
        || (request.kind == Kind::Rows
            && (request.rust_backend != "auto"
                || request.row_workers != 1
                || request.adapter_condition != AdapterCondition::Standard))
    {
        return Err("Select one supported backend and exact one/four-worker PTQ1 policy; row qualification fixes both worker conditions.".into());
    }
    let pins = source_pins(&request.repository_directory, request.kind)?;
    match request.operation {
        Operation::SourcePins => Ok(pins),
        Operation::Plan => Ok(plan(&request, &pins)),
        Operation::Capture => {
            #[cfg(windows)]
            {
                windows::capture(&request, &pins)
            }
            #[cfg(not(windows))]
            {
                Err("Native qualification requires Windows ARM64 owner CI.".into())
            }
        }
    }
}

#[cfg(test)]
mod tests;
