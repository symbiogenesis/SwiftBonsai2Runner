//! Fixed archived publisher startup probes and bounded original-PQ2 smoke.
#![cfg_attr(
    not(any(windows, test)),
    expect(
        dead_code,
        reason = "Native execution is Windows-only; portable policy is exercised by tests."
    )
)]

use std::{collections::BTreeSet, fs, path::Path, time::Duration};

use serde_json::{Value, json};

const PIN: &str = include_str!("../tests/fixtures/publisher-startup-v1.json");
const MODEL: &str = include_str!("../../../config/swift-assessment.lock.json");

#[cfg(windows)]
mod windows;

fn external(path: &Path) -> crate::Result<()> {
    bonsai_artifacts::validate_external_path(path).map_err(|error| error.to_string())
}

fn pinned() -> crate::Result<Value> {
    let pin: Value = serde_json::from_str(PIN).map_err(|error| error.to_string())?;
    if pin["schemaVersion"] != 1
        || pin["kind"] != "publisher-headless-hosted-model-free-startup"
        || pin["results"]["allProbesPassed"].as_bool() != Some(true)
        || pin["execution"]["hosted"].as_bool() != Some(true)
        || pin["results"]["packageFiles"].as_array().map(Vec::len) != Some(14)
    {
        return Err("Incomplete archived hosted publisher startup evidence.".into());
    }
    Ok(pin)
}

fn cases() -> Vec<Value> {
    let mut cases = vec![
        json!({"Name":"completion-version", "Executable":"llama-completion.exe", "Arguments":["--version"]}),
        json!({"Name":"server-version", "Executable":"llama-server.exe", "Arguments":["--version"]}),
        json!({"Name":"bench-help", "Executable":"llama-bench.exe", "Arguments":["--help"]}),
    ];
    for (index, vector) in [
        "",
        "日本語 🪴 café",
        "C:\\notes dir\\ \"quoted\" tail\\\\",
        "line1\tline2\nline3",
        "*.gguf",
        "  leading and trailing  ",
        "slashes\\\\\\\"quote",
    ]
    .into_iter()
    .enumerate()
    {
        cases.push(json!({"Name":format!("argv-{index}"), "Executable":"test-headless-argv.exe",
            "Arguments":["--model", "模型/test.gguf", "--seed", index.to_string(), "--prompt", vector]}));
    }
    cases
}

fn inventory(bundle: &Path, files: &Value) -> crate::Result<Value> {
    external(bundle)?;
    let files = files
        .as_array()
        .filter(|files| (4..=32).contains(&files.len()))
        .ok_or("Invalid candidate package inventory.")?;
    let mut names = BTreeSet::new();
    let mut artifacts = Vec::new();
    for file in files {
        let name = file["Name"]
            .as_str()
            .filter(|name| {
                !name.is_empty()
                    && name
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
            })
            .ok_or("Invalid candidate filename.")?;
        if matches!(name, "." | "..") || !names.insert(name.to_ascii_lowercase()) {
            return Err("Duplicate or redirected candidate filename.".into());
        }
        let length = file["Bytes"]
            .as_u64()
            .filter(|bytes| *bytes > 0)
            .ok_or("Invalid candidate length.")?;
        let digest = file["Sha256"]
            .as_str()
            .filter(|hash| crate::identity::is_lower_hex(hash, 64))
            .ok_or("Invalid candidate digest.")?;
        let path = bundle.join(name);
        external(&path)?;
        let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        if !metadata.is_file()
            || bonsai_artifacts::is_redirect(&metadata)
            || metadata.len() != length
        {
            return Err("A pinned candidate file is missing, redirected or changed length.".into());
        }
        artifacts.push(json!({"path":path, "bytes":length, "sha256":digest}));
    }
    let actual = fs::read_dir(bundle)
        .map_err(|error| error.to_string())?
        .take(33)
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(|error| error.to_string())?;
    if actual.len() != names.len()
        || actual.iter().any(|entry| {
            !entry
                .file_name()
                .to_str()
                .is_some_and(|name| names.contains(&name.to_ascii_lowercase()))
        })
    {
        return Err("Candidate bundle contains unpinned entries.".into());
    }
    for required in [
        "llama-completion.exe",
        "llama-server.exe",
        "llama-bench.exe",
        "test-headless-argv.exe",
    ] {
        if !names.contains(required) {
            return Err("Missing required candidate executable.".into());
        }
    }
    Ok(json!(artifacts))
}

fn allowance(budget: u64, elapsed: Duration) -> crate::Result<u64> {
    Duration::from_secs(budget)
        .saturating_sub(elapsed)
        .as_secs()
        .checked_sub(10)
        .filter(|remaining| *remaining >= 24)
        .ok_or_else(|| "Candidate startup budget exhausted; remaining probes stay pending.".into())
}

fn smoke_deadline(remaining: u64) -> crate::Result<u64> {
    if remaining < 60 {
        return Err("Model smoke lacks verification and teardown reserve.".into());
    }
    Ok(30.min(remaining - 36))
}

fn unique_argument(arguments: &[String], flag: &str) -> crate::Result<usize> {
    let indices: Vec<_> = arguments
        .iter()
        .enumerate()
        .filter_map(|(index, argument)| (argument == flag).then_some(index))
        .collect();
    if indices.len() != 1 || indices[0] + 1 >= arguments.len() {
        return Err("Missing or duplicated candidate smoke control.".into());
    }
    Ok(indices[0] + 1)
}

fn smoke_plan(cpu: &Value, model: &Value) -> crate::Result<Value> {
    let policy = &cpu["Policy"];
    if policy["Name"] != "swift-pq2-assessment"
        || !cpu["AdapterLock"].is_null()
        || policy["WeightRepresentation"] != "PQ2_0"
        || policy["ServiceCeilingBytes"] != 12_348_030_976_u64
        || policy["RequiredAvailableBytes"] != 16_642_998_272_u64
        || policy["AvailableMemoryFloorBytes"] != 4_294_967_296_u64
        || cpu["Artifact"]["Bytes"] != model["model"]["bytes"]
        || cpu["Artifact"]["Sha256"] != model["model"]["sha256"]
        || cpu["Lock"]["runtime"]["commit"] != model["runtime"]["commit"]
    {
        return Err("Candidate model smoke requires the exact original PQ2 control and unchanged memory policy.".into());
    }
    for field in [
        "repository",
        "revision",
        "filename",
        "bytes",
        "sha256",
        "architecture",
    ] {
        if cpu["Lock"]["model"][field] != model["model"][field] {
            return Err("CPU model differs from the pinned control.".into());
        }
    }
    let path = cpu["ModelPath"].as_str().ok_or("Missing CPU model path.")?;
    external(Path::new(path))?;
    let mut arguments: Vec<String> =
        serde_json::from_value(cpu["Arguments"].clone()).map_err(|error| error.to_string())?;
    if arguments[unique_argument(&arguments, "--model")?] != path
        || arguments
            .iter()
            .any(|argument| matches!(argument.as_str(), "--lora" | "--lora-scaled"))
    {
        return Err("Candidate smoke must consume only the pinned original model.".into());
    }
    for (flag, before, after) in [("--n-predict", "16", "4"), ("--reasoning", "on", "off")] {
        let index = unique_argument(&arguments, flag)?;
        if arguments[index] != before {
            return Err("Review candidate smoke after a compiled CPU policy change.".into());
        }
        arguments[index] = after.into();
    }
    for flag in ["--no-perf", "--no-display-prompt"] {
        if arguments
            .iter()
            .filter(|argument| *argument == flag)
            .count()
            != 1
        {
            return Err("Missing unique candidate diagnostic controls.".into());
        }
    }
    arguments.retain(|argument| argument != "--no-perf");
    arguments.push("--verbose-prompt".into());
    Ok(
        json!({"Name":"pq2-four-token-smoke", "Executable":"llama-completion.exe", "Arguments":arguments,
        "Artifact":{"path":path, "bytes":model["model"]["bytes"], "sha256":model["model"]["sha256"]}}),
    )
}

fn decimal(text: &str) -> crate::Result<u64> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("Invalid inference counter.".into());
    }
    text.parse()
        .map_err(|error: std::num::ParseIntError| error.to_string())
}

fn logger_line(line: &str) -> &str {
    let Some((stamp, rest)) = line.split_once(" I ") else {
        return line;
    };
    let parts: Vec<_> = stamp.split('.').collect();
    if parts.len() == 4
        && !parts[0].is_empty()
        && parts[1].len() == 2
        && parts[2].len() == 3
        && parts[3].len() == 3
        && parts
            .iter()
            .all(|part| part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        rest
    } else {
        line
    }
}

fn observation(stdout: &str, stderr: &str) -> crate::Result<Value> {
    if stdout.len() > 65_536 || stderr.len() > 65_536 || stdout.trim().is_empty() {
        return Err("Invalid bounded smoke output.".into());
    }
    let mut prompt = Vec::new();
    let mut sampled = Vec::new();
    let mut decoded = Vec::new();
    for line in stderr.lines().map(logger_line) {
        if let Some(count) = line
            .strip_prefix("main: number of tokens in prompt = ")
            .or_else(|| line.strip_prefix("llama_completion: number of tokens in prompt = "))
        {
            prompt.push(decimal(count)?);
        }
        let Some(counter) = line.strip_prefix("common_perf_print:") else {
            continue;
        };
        if !counter.starts_with([' ', '\t']) {
            continue;
        }
        let counter = counter.trim_start_matches([' ', '\t']);
        let (target, remaining, units) = if let Some(rest) = counter.strip_prefix("samplers time") {
            (&mut sampled, rest, " tokens")
        } else if let Some(rest) = counter.strip_prefix("eval time") {
            (&mut decoded, rest, " runs")
        } else {
            continue;
        };
        let remaining = remaining
            .trim_start_matches([' ', '\t'])
            .strip_prefix('=')
            .ok_or("Malformed inference timing counter.")?;
        let (_, value) = remaining
            .rsplit_once('/')
            .ok_or("Missing inference counter denominator.")?;
        let (count, suffix) = value
            .trim_start_matches([' ', '\t'])
            .split_once(units)
            .ok_or("Missing inference counter units.")?;
        if units == " tokens" {
            if !suffix.trim_matches([' ', '\t']).is_empty() {
                return Err("Invalid sample counter suffix.".into());
            }
        } else if !suffix.starts_with([' ', '\t'])
            || !suffix.trim_start_matches([' ', '\t']).starts_with('(')
            || !suffix.ends_with(')')
        {
            return Err("Invalid decode counter suffix.".into());
        }
        target.push(decimal(count)?);
    }
    if prompt.len() != 1 || sampled.len() != 1 || decoded.len() != 1 {
        return Err("Smoke lacks unique prompt, sampler and decode evidence.".into());
    }
    let generated = sampled[0]
        .checked_sub(prompt[0])
        .ok_or("Invalid sampled count.")?;
    if !(2..=8192).contains(&prompt[0])
        || !(1..=4).contains(&generated)
        || !(1..=4).contains(&decoded[0])
        || generated != decoded[0] + 1
    {
        return Err("Candidate smoke counters do not prove bounded inference.".into());
    }
    Ok(
        json!({"PromptTokens":prompt[0], "SampledOutputTokens":generated, "DecodeEvaluationRuns":decoded[0],
        "VisibleOutputBytes":stdout.len(), "PerformanceVerdict":"not-evaluated"}),
    )
}

/// Run fixed archived publisher startup probes under one complete owner allowance.
///
/// # Errors
/// Rejects unsafe paths, substituted candidates, invalid hosts/controls, caller death,
/// expired budgets, incomplete inference evidence and failed owned processes.
pub fn run(value: Value) -> crate::Result<Value> {
    #[cfg(windows)]
    {
        windows::run(value)
    }
    #[cfg(not(windows))]
    {
        let _ = value;
        Err("Publisher startup requires native ARM64 Windows.".into())
    }
}

#[cfg(test)]
mod tests;
