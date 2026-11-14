//! One original native owner deadline, four owned arms and selected replay bytes.

use super::{
    Path, PathBuf, Result, SCHEDULE, Value, bytes, digest, float, fs, hex, integer, json,
    json_bytes, matches, member, native_conditions, power, read, reject_redirects, require, subset,
    validate_arm, validate_preparation,
};
use bonsai_windows_process::{OwnedProcess, ToolProcessSpec};
use std::{
    collections::BTreeMap,
    io::Write as _,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

fn path(request: &Value, key: &str) -> Result<PathBuf> {
    let result = PathBuf::from(
        request[key]
            .as_str()
            .ok_or_else(|| format!("Missing {key}."))?,
    );
    external(&result)?;
    Ok(result)
}
fn external(path: &Path) -> Result<()> {
    require(
        path.to_str()
            .is_some_and(|text| !text.contains(['\r', '\n', '\0'])),
        "Capture paths require single-line expanded Unicode names.",
    )?;
    bonsai_artifacts::validate_external_path(path).map_err(|e| e.to_string())?;
    reject_redirects(path)?;
    require(
        !path
            .ancestors()
            .any(|ancestor| ancestor.join(".git").exists()),
        "Capture paths must be outside Git checkouts.",
    )
}
fn save(path: &Path, value: &Value, new: bool) -> Result<()> {
    external(path)?;
    let mut options = fs::OpenOptions::new();
    options.write(true);
    if new {
        options.create_new(true);
    } else {
        options.create(true).truncate(true);
    }
    let mut file = options.open(path).map_err(|e| e.to_string())?;
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    file.write_all(&bytes)
        .and_then(|()| file.sync_all())
        .map_err(|e| e.to_string())
}
fn append_env(name: &str, line: &str) -> Result<()> {
    if let Some(path) = std::env::var_os(name) {
        fs::OpenOptions::new()
            .append(true)
            .open(path)
            .and_then(|mut file| writeln!(file, "{line}"))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn ticks() -> Result<(u64, u64)> {
    bonsai_windows_process::monotonic_ticks().map_err(|e| e.to_string())
}
fn unix() -> Result<u64> {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_millis(),
    )
    .map_err(|e| e.to_string())
}
fn variable(name: &str) -> String {
    std::env::var(name).unwrap_or_default()
}
fn decimal(value: &str) -> bool {
    !value.is_empty() && !value.starts_with('0') && value.bytes().all(|b| b.is_ascii_digit())
}
fn ci_guard() -> Result<()> {
    require(
        cfg!(target_arch = "aarch64"),
        "Use native Windows ARM64 tools.",
    )?;
    for (key, value) in [
        ("GITHUB_ACTIONS", "true"),
        ("GITHUB_REF", "refs/heads/main"),
        ("GITHUB_REPOSITORY", "symbiogenesis/SwiftBonsai2Runner"),
        ("RUNNER_ARCH", "ARM64"),
        ("RUNNER_ENVIRONMENT", "self-hosted"),
    ] {
        require(
            variable(key) == value,
            "Capture requires owning main-only Windows ARM64 CI.",
        )?;
    }
    require(
        decimal(&variable("GITHUB_RUN_ID"))
            && !variable("RUNNER_NAME").is_empty()
            && crate::identity::is_lower_hex(&variable("GITHUB_SHA"), 40),
        "Identified source-bound owner execution required.",
    )
}
fn tool_identity(revision: &str) -> Result<(PathBuf, String)> {
    let tool = std::env::current_exe().map_err(|e| e.to_string())?;
    external(&tool)?;
    let receipt = read(&tool.with_file_name("receipt.json"))?;
    subset(
        &receipt,
        &json!({"SchemaVersion":3,"Revision":revision,"Sha256":digest(&tool)?,"NumericSha256":digest(&tool.with_file_name("bonsai-reference-numerics.exe"))?}),
    )?;
    Ok((
        tool,
        receipt["Sha256"]
            .as_str()
            .ok_or("Missing checked supervisor identity.")?
            .into(),
    ))
}
fn create(request: &Value) -> Result<Value> {
    let (started, frequency) = ticks()?;
    let started_unix = unix()?;
    require(
        cfg!(target_arch = "aarch64"),
        "Use native Windows ARM64 tools.",
    )?;
    let destination = path(request, "startReceipt")?;
    let runner = hex(&request["runnerCommit"], 40)?;
    let candidate = hex(&request["candidateCommit"], 40)?;
    require(
        request["runId"].as_str().is_some_and(decimal),
        "Use actual dispatched run ID.",
    )?;
    tool_identity(runner)?;
    let result = json!({"schema_version":1,"kind":"prism-cpu-bundle-owner-start","run_id":request["runId"],"runner_commit":runner,"candidate_commit":candidate,"clock_frequency":frequency,"started_tick":started,"deadline_tick":started.checked_add(600*frequency).ok_or("Owner clock overflow.")?,"started_unix_milliseconds":started_unix,"deadline_unix_milliseconds":started_unix+600_000,"power_before":crate::host_cpu::power_snapshot(),"budget_seconds":600,"conservative_estimate_seconds":600,"planning_estimate_seconds":[300,480],"schedule":SCHEDULE,"fixed_work":"One bundle-only ABBA comparison with 32 output tokens reserved per arm: each collector has 60 seconds, each supervised helper at most 95 seconds, with 10 seconds condition checks and 5 seconds metadata. Reserve 60 seconds bootstrap/intake and 100 seconds cleanup/evaluation/upload; 600 seconds maximum aggregate. Planning estimate 5-8 minutes is not an empirical timing prediction. Stop once complete or infeasible; no retry or split matrix."});
    save(&destination, &result, true)?;
    Ok(result)
}
struct Context {
    original: Value,
    deadline: u64,
    frequency: u64,
    tool: PathBuf,
    tool_digest: String,
    working: PathBuf,
    retained: PathBuf,
    parent: bonsai_windows_process::ParentProcess,
}
impl Context {
    fn remaining(&self) -> Result<u64> {
        self.parent.ensure_alive().map_err(|e| e.to_string())?;
        let (now, frequency) = ticks()?;
        require(
            frequency == self.frequency,
            "Original clock frequency changed.",
        )?;
        Ok(self.deadline.saturating_sub(now) / frequency)
    }
    fn window(&self) -> Result<()> {
        append_env(
            "GITHUB_ENV",
            &format!(
                "BONSAI_CPU_BUNDLE_UPLOAD_ALLOWED={}",
                self.remaining()? >= 70
            ),
        )
    }
    fn tool(&self, command: &str, request: &Value, maximum: u64, reserve: u64) -> Result<Value> {
        require(
            digest(&self.tool)? == self.tool_digest,
            "Checked supervisor changed.",
        )?;
        let limit = self.remaining()?.saturating_sub(reserve).min(maximum);
        require(
            limit > 0,
            "Original aggregate budget cannot fit another helper.",
        )?;
        let serial = crate::startup_process::unique();
        let stdout = self.working.join(format!("helper-{serial}.stdout"));
        let stderr = self.working.join(format!("helper-{serial}.stderr"));
        let removed = std::env::vars_os()
            .filter_map(|(name, _)| {
                let name = name.to_string_lossy().into_owned();
                let upper = name.to_ascii_uppercase();
                (upper.contains("TOKEN")
                    || upper.contains("SECRET")
                    || upper.contains("PASSWORD")
                    || upper.contains("CREDENTIAL")
                    || [
                        "ACTIONS_RUNTIME_URL",
                        "ACTIONS_RESULTS_URL",
                        "ACTIONS_CACHE_URL",
                        "ACTIONS_ID_TOKEN_REQUEST_URL",
                    ]
                    .contains(&upper.as_str()))
                .then_some(name)
            })
            .collect::<Vec<_>>();
        let process = OwnedProcess::start_tool_with_removed(
            &ToolProcessSpec {
                executable: self.tool.clone(),
                arguments: vec![
                    command.into(),
                    "--json".into(),
                    serde_json::to_string(request)
                        .map_err(|e| e.to_string())?
                        .into(),
                ],
                working_directory: self.working.clone(),
                stdout: stdout.clone(),
                stderr: stderr.clone(),
                environment: BTreeMap::new(),
            },
            &removed,
        )
        .map_err(|e| e.to_string())?;
        let began = Instant::now();
        let result: Result<()> = (|| {
            loop {
                for file in [&stdout, &stderr] {
                    require(
                        fs::metadata(file).map_err(|e| e.to_string())?.len() <= 4_194_304,
                        "Supervisor diagnostic output exceeded bound.",
                    )?;
                }
                if let Some(code) = process
                    .wait_for_exit(Duration::from_millis(20))
                    .map_err(|e| e.to_string())?
                {
                    require(
                        code == 0,
                        "Checked native helper failed; selected failure diagnostics retained.",
                    )?;
                    break;
                }
                require(
                    began.elapsed() < Duration::from_secs(limit) && self.remaining()? > reserve,
                    "Supervised helper exceeded original remaining allowance.",
                )?;
            }
            Ok(())
        })();
        let cleanup = process
            .stop(Duration::from_secs(10))
            .map_err(|e| e.to_string());
        if result.is_err() || cleanup.is_err() {
            for (source, label) in [(&stdout, "stdout"), (&stderr, "stderr")] {
                let raw = bytes(source, 4_194_304)?;
                if raw.iter().any(|b| !b.is_ascii_whitespace()) {
                    fs::write(
                        self.retained
                            .join(format!("failed-helper-{serial}.{label}")),
                        raw,
                    )
                    .map_err(|e| e.to_string())?;
                }
            }
        }
        let response = if result.is_ok() && cleanup.is_ok() {
            json_bytes(&bytes(&stdout, 4_194_304)?)
        } else {
            Err("Native helper failed or cleanup could not be confirmed.".into())
        };
        fs::remove_file(stdout).map_err(|e| e.to_string())?;
        fs::remove_file(stderr).map_err(|e| e.to_string())?;
        cleanup?;
        result?;
        response
    }
}
fn original(request: &Value) -> Result<Value> {
    ci_guard()?;
    let original = read(&path(request, "startReceipt")?)?;
    let (now, frequency) = ticks()?;
    subset(
        &original,
        &json!({"schema_version":1,"kind":"prism-cpu-bundle-owner-start","run_id":variable("GITHUB_RUN_ID"),"runner_commit":variable("GITHUB_SHA"),"clock_frequency":frequency,"budget_seconds":600}),
    )?;
    let started = integer(&original["started_tick"], 0, now.saturating_sub(1))?;
    let deadline = integer(&original["deadline_tick"], 1, i64::MAX.unsigned_abs())?;
    require(
        deadline
            == started
                .checked_add(600 * frequency)
                .ok_or("Owner deadline overflow.")?,
        "Original owner allowance malformed.",
    )?;
    let utc_started = integer(
        &original["started_unix_milliseconds"],
        1,
        i64::MAX.unsigned_abs(),
    )?;
    let utc_deadline = integer(
        &original["deadline_unix_milliseconds"],
        1,
        i64::MAX.unsigned_abs(),
    )?;
    require(
        utc_deadline == utc_started + 600_000,
        "Original UTC anchor malformed.",
    )?;
    let elapsed_millis = u128::from(now - started) * 1000 / u128::from(frequency);
    require(
        (i128::from(unix()?)
            - i128::from(utc_started)
            - i128::try_from(elapsed_millis).map_err(|e| e.to_string())?)
        .abs()
            <= 2000,
        "Original monotonic and UTC deadline anchors disagree.",
    )?;
    Ok(original)
}
fn finalize(request: &Value) -> Result<Value> {
    let original = original(request)?;
    let after = crate::host_cpu::power_snapshot();
    let (now, frequency) = ticks()?;
    let matched = power(&after)
        .ok()
        .zip(power(&original["power_before"]).ok())
        .is_some_and(|(a, b)| a == b);
    let result = json!({"kind":"prism-cpu-bundle-post-upload-budget","run_id":variable("GITHUB_RUN_ID"),"original_deadline_tick":original["deadline_tick"],"completed_tick":now,"elapsed_seconds":float(now-integer(&original["started_tick"],0,now)?)?/float(frequency)?,"power_after":after,"power_endpoints_match":matched,"full_promotion_qualified":false});
    append_env(
        "GITHUB_STEP_SUMMARY",
        &serde_json::to_string(&result).map_err(|e| e.to_string())?,
    )?;
    require(
        now < integer(&original["deadline_tick"], 1, i64::MAX.unsigned_abs())?,
        "Original allowance expired through upload/final recording.",
    )?;
    require(
        matched,
        "Final power unknown or mismatched; qualification pending.",
    )?;
    Ok(result)
}
fn proof(root: &Path, relative: &str) -> Result<Value> {
    Ok(json!({"path":relative,"sha256":digest(&root.join(relative))?}))
}

#[expect(
    clippy::too_many_lines,
    reason = "The one-shot capture owns intake, selected retention and failure publication under the same original allowance."
)]
fn capture(request: &Value) -> Result<Value> {
    let original = original(request)?;
    let candidate = hex(&request["candidateCommit"], 40)?;
    let tooling = hex(&request["toolingCommit"], 40)?;
    let preparation_hash = hex(&request["expectedPacketSha256"], 64)?;
    matches(&original["candidate_commit"], &json!(candidate))?;
    let packet_root = path(request, "packetDirectory")?;
    let preparation_path = packet_root.join("preparation.json");
    matches(&json!(digest(&preparation_path)?), &json!(preparation_hash))?;
    let packet = read(&preparation_path)?;
    validate_preparation(&packet, &preparation_path)?;
    subset(
        &packet["orchestrator"],
        &json!({"repository":variable("GITHUB_REPOSITORY"),"revision":variable("GITHUB_SHA"),"run_id":variable("GITHUB_RUN_ID"),"run_attempt":variable("GITHUB_RUN_ATTEMPT")}),
    )?;
    subset(
        &packet["tooling_source"],
        &json!({"revision":tooling,"clean":true}),
    )?;
    matches(
        &packet["arms"]["candidate"]["source"]["revision"],
        &json!(candidate),
    )?;
    for role in ["baseline", "candidate"] {
        let arm = &packet["arms"][role];
        for value in [
            &arm["driver_manifest_sha256"],
            &arm["driver_lock_sha256"],
            &arm["source"]["manifest_sha256"],
            &arm["source"]["lock_sha256"],
        ] {
            hex(value, 64)?;
        }
        let executable = member(&packet_root, &arm["executable"]["path"])?;
        matches(
            &json!(fs::metadata(&executable).map_err(|e| e.to_string())?.len()),
            &arm["executable"]["bytes"],
        )?;
        matches(&json!(digest(&executable)?), &arm["executable"]["sha256"])?;
    }
    let evidence = if request["evidenceDirectory"]
        .as_str()
        .is_some_and(|s| !s.is_empty())
    {
        path(request, "evidenceDirectory")?
    } else {
        PathBuf::from(std::env::var_os("RUNNER_TEMP").ok_or("Missing RUNNER_TEMP.")?).join(format!(
            "CpuBundle-Evidence-{}-{}",
            variable("GITHUB_RUN_ID"),
            variable("GITHUB_RUN_ATTEMPT")
        ))
    };
    external(&evidence)?;
    require(
        !evidence.exists(),
        "Use new evidence root; previous workloads cannot resume.",
    )?;
    let (tool, tool_digest) = tool_identity(&variable("GITHUB_SHA"))?;
    let parent_id = u32::try_from(integer(
        &request["parentProcessId"],
        1,
        u64::from(u32::MAX),
    )?)
    .map_err(|e| e.to_string())?;
    let parent_creation = integer(&request["parentCreationTimeFileTime"], 1, u64::MAX)?;
    let parent = bonsai_windows_process::ParentProcess::open(parent_id, parent_creation)
        .map_err(|e| e.to_string())?;
    let context = Context {
        deadline: integer(&original["deadline_tick"], 1, i64::MAX.unsigned_abs())?,
        frequency: integer(&original["clock_frequency"], 1, 1_000_000_000_000)?,
        original,
        tool,
        tool_digest,
        working: evidence.join("working"),
        retained: evidence.join("retained"),
        parent,
    };
    fs::create_dir_all(&context.working)
        .and_then(|()| fs::create_dir_all(&context.retained))
        .map_err(|e| e.to_string())?;
    append_env(
        "GITHUB_ENV",
        &format!("BONSAI_CPU_BUNDLE_EVIDENCE={}", context.retained.display()),
    )?;
    append_env("GITHUB_ENV", "BONSAI_CPU_BUNDLE_UPLOAD_ALLOWED=false")?;
    let mut owner = json!({"schema_version":1,"kind":"prism-cpu-bundle-owner","runner_commit":variable("GITHUB_SHA"),"run_id":variable("GITHUB_RUN_ID"),"run_attempt":variable("GITHUB_RUN_ATTEMPT"),"preparation_sha256":preparation_hash,"workload_predeclared":true,"completed":false,"budget_seconds":600,"clock_frequency":context.frequency,"started_tick":context.original["started_tick"],"deadline_tick":context.deadline,"original_deadline_unix_milliseconds":context.original["deadline_unix_milliseconds"],"power_before":context.original["power_before"],"arms":[],"failure":null,"retention_scope":"Selected local evidence stage; owning job must separately pass its post-upload original-deadline check.","full_promotion_qualified":false});
    let capture_result = capture_arms(
        &context,
        &packet,
        &packet_root,
        &preparation_path,
        preparation_hash,
        &mut owner,
    );
    if let Err(error) = &capture_result {
        owner["failure"] = error.clone().into();
    }
    match context.tool("host-cpu", &json!({}), 5, 85) {
        Ok(after) => {
            owner["power_after"] = after["power_snapshot"].clone();
            owner["host_cpu_after"] = after;
        }
        Err(error) => {
            owner["completed"] = false.into();
            owner["power_after"] = bonsai_cpu_policy::power::unknown_snapshot();
            if owner["failure"].is_null() {
                owner["failure"] = error.into();
            }
        }
    }
    retain(&context, &packet_root, &packet, capture_result.is_err())?;
    owner["retention_completed_tick"] = ticks()?.0.into();
    save(&context.retained.join("owner.json"), &owner, false)?;
    context.window()?;
    capture_result?;
    require(owner["completed"] == true, "Final power capture failed.")?;
    require(
        context.remaining()? >= 85,
        "Insufficient original budget for evaluation/upload.",
    )?;
    let compared=context.tool("cpu-evaluation",&json!({"operation":"bundle-evaluate","preparationPath":context.retained.join("packet/preparation.json"),"ownerPath":context.retained.join("owner.json")}),15,70);
    match &compared {
        Ok(report) => {
            save(&context.retained.join("comparison.json"), report, false)?;
            append_env(
                "GITHUB_STEP_SUMMARY",
                &serde_json::to_string(report).map_err(|e| e.to_string())?,
            )?;
        }
        Err(error) => {
            save(
                &context.retained.join("comparison-rejection.json"),
                &json!({"status":"rejected","reason":error,"completed_tick":ticks()?.0,"full_promotion_qualified":false}),
                false,
            )?;
        }
    }
    context.window()?;
    compared
}

#[expect(
    clippy::too_many_lines,
    reason = "Keep fixed four-arm supervision, artifact controls and power boundaries in one original-deadline workload."
)]
fn capture_arms(
    context: &Context,
    packet: &Value,
    packet_root: &Path,
    preparation: &Path,
    preparation_hash: &str,
    owner: &mut Value,
) -> Result<()> {
    require(
        context.remaining()? >= 540,
        "Bootstrap/intake cannot fit complete fixed workload.",
    )?;
    let before = context.tool("host-cpu", &json!({}), 5, 100)?;
    native_conditions(&before, &before["host_identity"])?;
    require(
        before["host_identity"]["processor_name"]
            .as_str()
            .is_some_and(|v| v.contains("Elite") && v.contains("Extreme")),
        "Selected X2 Elite Extreme not observed.",
    )?;
    require(
        power(&before["power_snapshot"])? == power(&context.original["power_before"])?,
        "Initial setup power changed or unknown.",
    )?;
    owner["host_identity"] = before["host_identity"].clone();
    owner["host_cpu_before"] = before.clone();
    for role in ["baseline", "candidate"] {
        context.tool("artifact",&json!({"operation":"arm64","path":member(packet_root,&packet["arms"][role]["executable"]["path"])?}),2,100)?;
    }
    let data = PathBuf::from(std::env::var_os("LOCALAPPDATA").ok_or("Missing LOCALAPPDATA.")?)
        .join("SwiftBonsai2");
    let plan=context.tool("cpu-check-plan",&json!({"operation":"plan","dataDirectory":data,"profile":"swift-pq2-assessment","useAbliterateLora":true,"useRepackedPtq1":false}),5,100)?;
    subset(
        &plan["Policy"],
        &json!({"ServiceCeilingBytes":12_348_030_976u64,"RequiredAvailableBytes":16_642_998_272u64,"AvailableMemoryFloorBytes":4_294_967_296u64}),
    )?;
    matches(&packet["model_sha256"], &plan["Artifact"]["Sha256"])?;
    matches(&packet["model_bytes"], &plan["Artifact"]["Bytes"])?;
    matches(
        &packet["adapter_sha256"],
        &plan["AdapterLock"]["adapter"]["sha256"],
    )?;
    matches(
        &packet["adapter_bytes"],
        &plan["AdapterLock"]["adapter"]["bytes"],
    )?;
    let model = path(&json!({"path":plan["ModelPath"]}), "path")?;
    let adapter = path(&json!({"path":plan["AdapterPath"]}), "path")?;
    let messages = member(packet_root, &packet["messages"]["path"])?;
    owner["model_sha256"] = plan["Artifact"]["Sha256"].clone();
    owner["adapter_sha256"] = plan["AdapterLock"]["adapter"]["sha256"].clone();
    save(&context.retained.join("cpu-plan.json"), &plan, false)?;
    save(
        &context.retained.join("owner-start.json"),
        &context.original,
        false,
    )?;
    for (index, role) in SCHEDULE.into_iter().enumerate() {
        owner["failed_arm"] = json!(index + 1);
        require(
            context.remaining()?
                >= u64::try_from(4 - index).map_err(|e| e.to_string())? * 110 + 100,
            "Remaining complete comparison cannot fit original allowance.",
        )?;
        require(
            digest(preparation)? == preparation_hash,
            "Preparation changed between arms.",
        )?;
        let relative = format!("arms/arm-{}", index + 1);
        let lane = context.working.join(&relative);
        let collector = lane.join("collector");
        let supervisor_lane = lane.join("supervision");
        fs::create_dir_all(&collector)
            .and_then(|()| fs::create_dir_all(&supervisor_lane))
            .map_err(|e| e.to_string())?;
        let begin = ticks()?.0;
        let execution = begin + 60 * context.frequency;
        let unix_deadline = integer(&context.original["deadline_unix_milliseconds"], 1, u64::MAX)?
            - u64::try_from(
                u128::from(context.deadline - execution) * 1000 / u128::from(context.frequency),
            )
            .map_err(|e| e.to_string())?;
        let executable = member(packet_root, &packet["arms"][role]["executable"]["path"])?;
        let request = json!({"schemaVersion":1,"executable":executable,"arguments":[model.to_string_lossy(),adapter.to_string_lossy(),collector.to_string_lossy(),messages.to_string_lossy(),"32",unix_deadline.to_string(),packet["arms"][role]["source"]["revision"].as_str().ok_or("Missing source revision.")?],"workingDirectory":lane,"evidenceDirectory":supervisor_lane,"artifacts":[{"path":model,"bytes":plan["Artifact"]["Bytes"],"sha256":plan["Artifact"]["Sha256"]},{"path":adapter,"bytes":plan["AdapterLock"]["adapter"]["bytes"],"sha256":plan["AdapterLock"]["adapter"]["sha256"]},{"path":executable,"bytes":packet["arms"][role]["executable"]["bytes"],"sha256":packet["arms"][role]["executable"]["sha256"]},{"path":messages,"bytes":fs::metadata(&messages).map_err(|e|e.to_string())?.len(),"sha256":packet["messages"]["sha256"]}],"memoryLimitBytes":12_348_030_976u64,"requiredAvailableBytes":16_642_998_272u64,"physicalReserveBytes":4_294_967_296u64,"timeoutSeconds":60,"totalTimeoutSeconds":90,"stdoutLimitBytes":1_048_576,"stderrLimitBytes":1_048_576,"requireNonemptyStdout":false,"ciOnly":false,"observeCpuConditions":true,"parentProcessId":std::process::id(),"parentCreationTimeFileTime":bonsai_windows_process::current_process_creation_time().map_err(|e|e.to_string())?});
        save(
            &context
                .retained
                .join(format!("completion-request-{}.json", index + 1)),
            &request,
            false,
        )?;
        let supervision = context.tool("completion", &request, 95, 100)?;
        subset(
            &supervision,
            &json!({"Status":"completed","ExitCode":0,"ModelProcessStarted":true}),
        )?;
        require(
            supervision.get("CancellationFailure").is_none(),
            "Owned Job teardown failed.",
        )?;
        let receipt = read(&collector.join("cpu-bundle.json"))?;
        validate_arm(&receipt, owner, packet, role, unix_deadline)?;
        for point in ["Before", "After"] {
            matches(
                &native_conditions(
                    &supervision["CpuConditions"][point],
                    &owner["host_identity"],
                )?,
                &native_conditions(&before, &owner["host_identity"])?,
            )?;
        }
        let stdout = bytes(&supervisor_lane.join("stdout.txt"), 1_048_576)?;
        let stderr = bytes(&supervisor_lane.join("stderr.txt"), 1_048_576)?;
        let arm = json!({"id":format!("arm-{}",index+1),"role":role,"started_tick":begin,"execution_deadline_tick":execution,"collector_deadline_unix_milliseconds":unix_deadline,"cleanup_completed_tick":ticks()?.0,"host_identity":supervision["CpuConditions"]["Before"]["host_identity"],"power_before":supervision["CpuConditions"]["Before"]["power_snapshot"],"power_after":supervision["CpuConditions"]["After"]["power_snapshot"],"owned_job_empty":true,"cleanup_succeeded":true,"stdout_bytes":stdout.len(),"stderr_bytes":stderr.len(),"stdout_nonempty":stdout.iter().any(|b|!b.is_ascii_whitespace()),"stderr_nonempty":stderr.iter().any(|b|!b.is_ascii_whitespace()),"receipt":proof(&context.working,&format!("{relative}/collector/cpu-bundle.json"))?,"started_receipt":proof(&context.working,&format!("{relative}/collector/cpu-bundle-started.json"))?,"supervision":proof(&context.working,&format!("{relative}/supervision/supervision.json"))?});
        owner["arms"]
            .as_array_mut()
            .ok_or("Missing owner arms.")?
            .push(arm);
    }
    owner["failed_arm"] = Value::Null;
    owner["completed"] = true.into();
    Ok(())
}
fn retain(context: &Context, packet: &Path, packet_data: &Value, failure: bool) -> Result<()> {
    for index in 1..=4 {
        let relative = format!("arms/arm-{index}");
        let lane = context.working.join(&relative);
        for name in [
            "collector/cpu-bundle-started.json",
            "collector/cpu-bundle.json",
            "supervision/supervision.json",
        ] {
            let source = lane.join(name);
            if source.exists() {
                let destination = context.retained.join(&relative).join(name);
                fs::create_dir_all(destination.parent().ok_or("Missing retained parent.")?)
                    .and_then(|()| fs::copy(source, destination).map(|_| ()))
                    .map_err(|e| e.to_string())?;
            }
        }
        if failure {
            for name in ["stdout.txt", "stderr.txt"] {
                let source = lane.join("supervision").join(name);
                if source.exists() {
                    let raw = bytes(&source, 1_048_576)?;
                    if raw.iter().any(|b| !b.is_ascii_whitespace()) {
                        let target = context
                            .retained
                            .join(&relative)
                            .join("supervision")
                            .join(name);
                        fs::create_dir_all(target.parent().ok_or("Missing retained parent.")?)
                            .and_then(|()| fs::write(target, raw))
                            .map_err(|e| e.to_string())?;
                    }
                }
            }
        }
    }
    fs::create_dir_all(context.retained.join("packet")).map_err(|e| e.to_string())?;
    for name in super::array(&packet_data["retained_files"])? {
        let name = name.as_str().ok_or("Missing retained packet member.")?;
        fs::copy(
            packet.join(name),
            context.retained.join("packet").join(name),
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}
pub(super) fn run(request: &Value) -> Result<Value> {
    match request["operation"].as_str() {
        Some("bundle-owner-start") => create(request),
        Some("bundle-owner-finalize") => finalize(request),
        Some("bundle-capture") => capture(request),
        _ => Err("Unknown owner operation.".into()),
    }
}
