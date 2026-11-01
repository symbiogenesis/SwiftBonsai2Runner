//! Windows-owned X2 workload, using one original monotonic deadline.

use super::{
    Request, allowance, batch_allowance, compare_pair, external, maintenance_identity,
    observed_plan, read_json, record, require, save,
};
use bonsai_artifacts::{ArtifactError, VerifiedFile};
use bonsai_windows_process::ParentProcess;
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read as _, Write as _},
    path::{Path, PathBuf},
    time::Instant,
};

struct Context<'a> {
    request: &'a Request,
    started: Instant,
    deadline: u64,
    parent: ParentProcess,
    parent_id: u32,
    parent_creation: u64,
    evidence: PathBuf,
    plan: Value,
    native: Value,
    report: Value,
}

fn ci_guard() -> crate::Result<()> {
    require(
        cfg!(target_arch = "aarch64"),
        "X2 capture requires native Windows ARM64.",
    )?;
    for (name, expected) in [
        ("GITHUB_ACTIONS", "true"),
        ("GITHUB_REF", "refs/heads/main"),
        ("GITHUB_REPOSITORY", "symbiogenesis/SwiftBonsai2Runner"),
        ("RUNNER_ARCH", "ARM64"),
        ("RUNNER_ENVIRONMENT", "self-hosted"),
    ] {
        require(
            std::env::var(name).as_deref() == Ok(expected),
            "X2 capture requires this repository main on the identified owner CI lane.",
        )?;
    }
    let run = std::env::var("GITHUB_RUN_ID").unwrap_or_default();
    require(
        crate::identity::is_lower_hex(&std::env::var("GITHUB_SHA").unwrap_or_default(), 40)
            && !run.is_empty()
            && run.bytes().all(|byte| byte.is_ascii_digit())
            && !std::env::var("RUNNER_NAME")
                .unwrap_or_default()
                .trim()
                .is_empty(),
        "X2 capture requires source-bound CI identity.",
    )?;
    for name in [
        "ACTIONS_RUNTIME_TOKEN",
        "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
        "GITHUB_TOKEN",
        "GH_TOKEN",
        "ACTIONS_RUNNER_INPUT_TOKEN",
    ] {
        require(
            std::env::var_os(name).is_none(),
            "X2 capture requires CI credentials omitted from its process tree.",
        )?;
    }
    bonsai_cpu_policy::server::validate_environment(
        std::env::vars_os().map(|(name, _)| name.to_string_lossy().into_owned()),
    )
}

fn publish_evidence(path: &Path) -> crate::Result<()> {
    let destination =
        PathBuf::from(std::env::var_os("GITHUB_ENV").ok_or("Expected the CI environment file.")?);
    external(&destination)?;
    let path = path
        .to_str()
        .filter(|value| !value.contains(['\r', '\n']))
        .ok_or("Expected a single-line Unicode evidence path.")?;
    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(destination)
        .map_err(|error| error.to_string())?;
    writeln!(file, "BONSAI_X2_CPU_EVIDENCE={path}").map_err(|error| error.to_string())
}

impl Context<'_> {
    fn remaining(&self) -> crate::Result<u64> {
        self.parent
            .ensure_alive()
            .map_err(|error| error.to_string())?;
        let (now, frequency) =
            bonsai_windows_process::monotonic_ticks().map_err(|error| error.to_string())?;
        allowance(
            self.request.work_budget_seconds,
            self.started.elapsed(),
            self.deadline,
            now,
            frequency,
        )
    }
    fn checkpoint(&self) -> crate::Result<()> {
        self.remaining().map(|_| ())
    }
    fn save(&self) -> crate::Result<()> {
        save(&self.evidence.join("cpu-evaluation.json"), &self.report)
    }
    fn phase(&mut self, phase: &str) -> crate::Result<()> {
        self.checkpoint()?;
        self.report["Phase"] = phase.into();
        self.save()
    }
    fn verify(&self, path: &Path, bytes: u64, digest: &str) -> crate::Result<VerifiedFile> {
        VerifiedFile::verify_with(path, bytes, digest, || {
            self.checkpoint().map_err(|_| ArtifactError::Cancelled)
        })
        .map_err(|error| error.to_string())
    }
    fn lane(&self, name: &str) -> crate::Result<PathBuf> {
        let path = self.evidence.join(name);
        external(&path)?;
        fs::create_dir(&path).map_err(|error| error.to_string())?;
        Ok(path)
    }
    fn supervision(&self, runtime: &Value, lane: &Path, model: bool) -> crate::Result<Value> {
        let remaining = self.remaining()?;
        let policy = &self.plan["CpuPlan"]["Policy"];
        Ok(json!({"schemaVersion":1,
            "executable":PathBuf::from(runtime["Runtime"]["runtimeDirectory"].as_str().ok_or("Missing runtime directory.")?).join("llama-completion.exe"),
            "arguments":if model {json!([])} else {json!(["--version"])}, "runtime":runtime["Runtime"],
            "workingDirectory":lane, "evidenceDirectory":lane, "artifacts":[],
            "memoryLimitBytes":if model {policy["ServiceCeilingBytes"].clone()} else {json!(536_870_912)},
            "requiredAvailableBytes":policy["RequiredAvailableBytes"], "physicalReserveBytes":policy["AvailableMemoryFloorBytes"],
            "timeoutSeconds":if model {45} else {10}, "totalTimeoutSeconds":if model {batch_allowance(remaining,&self.plan)?} else {remaining},
            "stdoutLimitBytes":if model {4_194_304} else {65_536}, "stderrLimitBytes":if model {67_108_864} else {65_536},
            "requireNonemptyStdout":model, "ciOnly":model, "observeCpuConditions":model,
            "observeOutputProgress":model && self.request.observe_output_progress,
            "parentProcessId":self.parent_id, "parentCreationTimeFileTime":self.parent_creation}))
    }
}

fn maintenance(context: &Context<'_>) -> crate::Result<(fs::File, VerifiedFile)> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let receipt = executable
        .parent()
        .ok_or("Missing tool directory.")?
        .join("receipt.json");
    let held = bonsai_artifacts::open_locked_file(&receipt).map_err(|error| error.to_string())?;
    let value = read_json(&receipt, 65_536)?;
    let length = fs::metadata(&executable)
        .map_err(|error| error.to_string())?
        .len();
    let revision = std::env::var("GITHUB_SHA").unwrap_or_default();
    let executable = context.verify(
        &executable,
        length,
        maintenance_identity(&value, &revision, length)?,
    )?;
    crate::startup_cache::assert_arm64(executable.path())?;
    save(&context.evidence.join("maintenance-tools.json"), &value)?;
    Ok((held, executable))
}

fn runtime(context: &mut Context<'_>) -> crate::Result<Value> {
    context.phase("admission-and-runtime-verification")?;
    let admission = crate::completion::cpu_check_report(
        json!({"operation":"plan", "dataDirectory":context.request.data_directory,
        "profile":"swift-pq2-assessment", "useAbliterateLora":true, "useRepackedPtq1":false}),
    )?;
    require(
        admission["Report"]["Status"] == "admission-passed",
        "The unchanged 15.5 GiB memory admission requirement was not met.",
    )?;
    context.report["Admission"] = admission["Report"].clone();
    let cpu = &context.plan["CpuPlan"];
    let spec = crate::cpu_check::runtime_spec(&context.request.data_directory, &cpu["Lock"])?;
    let runtime = crate::runtime::run(
        json!({"schemaVersion":1, "runtime":spec, "commit":cpu["Lock"]["runtime"]["commit"],
        "includeHelp":false, "timeoutSeconds":context.remaining()?,
        "parentProcessId":context.parent_id,"parentCreationTimeFileTime":context.parent_creation}),
    )?;
    require(
        runtime["Commit"] == cpu["Lock"]["runtime"]["commit"] && runtime["Runtime"] == spec,
        "Verified runtime differs from the compiled X2 plan.",
    )?;
    context.report["RuntimeVerification"] = runtime.clone();
    context.phase("model-free-worker-preflight")?;
    let lane = context.lane("worker-preflight")?;
    context.report["WorkerPreflight"]["Status"] = "running".into();
    context.save()?;
    let result = crate::completion::run(context.supervision(&runtime, &lane, false)?)?;
    context.report["WorkerPreflight"]["Status"] = "passed".into();
    context.report["WorkerPreflight"]["Supervision"] = result;
    context.save()?;
    Ok(runtime)
}

fn adapter_copy(context: &Context<'_>) -> crate::Result<PathBuf> {
    let cpu = &context.plan["CpuPlan"];
    let source = Path::new(cpu["AdapterPath"].as_str().ok_or("Missing adapter path.")?);
    let artifact = &cpu["AdapterLock"]["adapter"];
    let expected = artifact["bytes"]
        .as_u64()
        .ok_or("Missing adapter length.")?;
    let _guard = context.verify(
        source,
        expected,
        artifact["sha256"]
            .as_str()
            .ok_or("Missing adapter digest.")?,
    )?;
    let destination = context.evidence.join("bonsai-abliterate-lora.gguf");
    external(&destination)?;
    let mut input =
        bonsai_artifacts::open_locked_file(source).map_err(|error| error.to_string())?;
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&destination)
        .map_err(|error| error.to_string())?;
    let mut buffer = [0; 65_536];
    let mut total = 0_u64;
    loop {
        context.checkpoint()?;
        let count = input.read(&mut buffer).map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        total += u64::try_from(count).map_err(|error| error.to_string())?;
        require(total <= expected, "Adapter grew during copying.")?;
        output
            .write_all(&buffer[..count])
            .map_err(|error| error.to_string())?;
    }
    require(total == expected, "Adapter changed during copying.")?;
    output.sync_all().map_err(|error| error.to_string())?;
    Ok(destination)
}

fn samples(context: &mut Context<'_>, runtime: &Value) -> crate::Result<Value> {
    context.phase("adapter-copy-and-fixed-trials")?;
    let adapter = adapter_copy(context)?;
    let schedule = context.plan["Schedule"]
        .as_array()
        .ok_or("Missing schedule.")?;
    let mut samples = Vec::new();
    for trial in schedule {
        let lane = context.lane(trial["Id"].as_str().ok_or("Missing trial identity.")?)?;
        save(&lane.join("arguments.json"), &trial["Arguments"])?;
        samples.push(
            json!({"id":trial["Id"],"arguments":trial["Arguments"],"evidenceDirectory":lane}),
        );
    }
    let mut request = context.supervision(runtime, &context.evidence, true)?;
    let cpu = &context.plan["CpuPlan"];
    request["samples"] = samples.into();
    request["artifacts"] = json!([
        {"path":cpu["ModelPath"],"bytes":cpu["Artifact"]["Bytes"],"sha256":cpu["Artifact"]["Sha256"]},
        {"path":adapter,"bytes":cpu["AdapterLock"]["adapter"]["bytes"],"sha256":cpu["AdapterLock"]["adapter"]["sha256"]}]);
    context.report["Status"] = "running".into();
    context.report["ModelTrialsRequested"] = true.into();
    context.save()?;
    crate::completion::run(request)
}

fn reconcile(context: &mut Context<'_>, supervision: &Value) -> crate::Result<()> {
    context.phase("reconcile-fixed-pairs")?;
    let schedule = context.plan["Schedule"]
        .as_array()
        .ok_or("Missing schedule.")?;
    let samples = supervision["Samples"]
        .as_array()
        .ok_or("Missing sample receipts.")?;
    require(
        supervision["Status"] == "completed" && samples.len() == 4,
        "Incomplete X2 batch receipts.",
    )?;
    let mut records = Vec::new();
    for trial in schedule {
        context.checkpoint()?;
        let selected = samples
            .iter()
            .filter(|sample| sample["Id"] == trial["Id"])
            .collect::<Vec<_>>();
        let [sample] = selected.as_slice() else {
            return Err("Missing or duplicate completed CPU sample.".into());
        };
        let path = context
            .evidence
            .join(trial["Id"].as_str().ok_or("Missing trial ID.")?)
            .join("capture.json");
        records.push(record(
            trial,
            sample,
            &read_json(&path, 25 * 1024 * 1024)?,
            &context.plan["GeneratedTokenCap"],
        )?);
        context.report["Records"] = json!(records);
        context.save()?;
    }
    let after = crate::host_cpu::run(&json!({}))?;
    let execution = super::super::conditions::compare(
        json!({"operation":"compare-conditions","before":context.native,"after":after}),
    )?;
    let mut comparisons = Vec::new();
    for pair in records.as_chunks::<2>().0 {
        context.checkpoint()?;
        comparisons.push(compare_pair(
            pair,
            &execution,
            context.request.observe_output_progress,
        )?);
    }
    context.report["NativeCpuAfter"] = after;
    context.report["ExecutionConditions"] = execution;
    context.report["Comparisons"] = comparisons.into();
    context.checkpoint()
}

fn process_isa() -> Value {
    #[cfg(target_arch = "aarch64")]
    {
        json!({"Source":"Rust is_aarch64_feature_detected", "Scope":"current-process",
        "AdvSimd":std::arch::is_aarch64_feature_detected!("neon"),"DotProd":std::arch::is_aarch64_feature_detected!("dotprod")})
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        json!({"Source":"Rust is_aarch64_feature_detected", "Scope":"current-process", "AdvSimd":null, "DotProd":null})
    }
}

fn report(request: &Request, native: &Value, plan: &Value) -> Value {
    json!({"SchemaVersion":1,"Status":"preparing","Phase":"maintenance-intake",
        "Revision":std::env::var("GITHUB_SHA").unwrap_or_default(),
        "RunUrl":format!("https://github.com/symbiogenesis/SwiftBonsai2Runner/actions/runs/{}",std::env::var("GITHUB_RUN_ID").unwrap_or_default()),
        "RunAttempt":std::env::var("GITHUB_RUN_ATTEMPT").ok(),"Runner":std::env::var("RUNNER_NAME").unwrap_or_default(),
        "ProcessorName":plan["ProcessorName"],"DeviceModel":plan["DeviceModel"],
        "OperatingSystem":native["host_identity"]["windows_version"],"OsArchitecture":"Arm64","ProcessArchitecture":"Arm64",
        "ProcessIsa":process_isa(),"StartedUtc":bonsai_windows_process::utc_now(),"NativeCpu":native,"CimProcessors":request.cim_processors,"Plan":plan,
        "TimingDefinitions":{
            "PromptMilliseconds":"Pinned native prompt eval; actual prompt token count retained.",
            "DecodeMilliseconds":"Pinned native eval time; count excludes first sampled token.",
            "WallMilliseconds":"Owned process creation through exit and final supervision; includes model load; excludes artifact verification and CPU/power boundary observations.",
            "Memory":"ProcessMemory retains sampled private/current working-set maxima and OS lifetime peak working set from existing one-second safety queries plus final exit. Optional OutputProgress partitions samples around observed capped stdout and root exit, not startup/prefill/decode phases or first-token latency. Sampling can miss brief peaks. Working set and Job commit overlap and must not be added. No energy measurement."},
        "Qualification":"screen-only","DefaultProfileChanged":false,"InstrumentedAttribution":request.observe_output_progress,
        "ModelTrialsRequested":false,"WorkBudgetSeconds":request.work_budget_seconds,"WorkDeadlineTick":request.work_deadline_tick,
        "WorkerPreflight":{"Kind":"model-free-worker-startup","Status":"pending","Arguments":["--version"],"ModelLoaded":false,"WorkerPolicy":"required-headless"},
        "Records":[],"Comparisons":[]})
}

pub(super) fn run(request: &Request) -> crate::Result<Value> {
    let started = Instant::now();
    ci_guard()?;
    let deadline = request
        .work_deadline_tick
        .filter(|tick| *tick != 0)
        .ok_or("Capture requires the original workDeadlineTick.")?;
    let (id, creation) = match (
        request.parent_process_id,
        request.parent_creation_time_file_time,
    ) {
        (Some(id), Some(creation)) if id != 0 && creation != 0 => (id, creation),
        _ => return Err("Capture requires the owning caller creation identity.".into()),
    };
    let parent = ParentProcess::open(id, creation).map_err(|error| error.to_string())?;
    let (now, frequency) =
        bonsai_windows_process::monotonic_ticks().map_err(|error| error.to_string())?;
    allowance(
        request.work_budget_seconds,
        started.elapsed(),
        deadline,
        now,
        frequency,
    )?;
    let native = crate::host_cpu::run(&json!({}))?;
    let plan = observed_plan(request, &native)?;
    let evidence = std::env::temp_dir().join(format!(
        "Bonsai-X2CpuEvaluation-{}",
        crate::startup_process::unique()
    ));
    external(&evidence)?;
    fs::create_dir(&evidence).map_err(|error| error.to_string())?;
    publish_evidence(&evidence)?;
    let report = report(request, &native, &plan);
    let mut context = Context {
        request,
        started,
        deadline,
        parent,
        parent_id: id,
        parent_creation: creation,
        evidence,
        plan,
        native,
        report,
    };
    context.save()?;
    let result = (|| {
        let _guards = maintenance(&context)?;
        save(&context.evidence.join("plan.json"), &context.plan)?;
        save(&context.evidence.join("native-cpu.json"), &context.native)?;
        let runtime = runtime(&mut context)?;
        let supervision = samples(&mut context, &runtime)?;
        reconcile(&mut context, &supervision)
    })();
    context.report["ElapsedMilliseconds"] = json!(context.started.elapsed().as_millis());
    if let Err(error) = &result {
        context.report["Status"] = "failed".into();
        if context.report["WorkerPreflight"]["Status"] == "running" {
            context.report["WorkerPreflight"]["Status"] = "failed".into();
        }
        context.report["Failure"] = error.clone().into();
    } else {
        context.report["Status"] = "completed".into();
        context.report["CompletedUtc"] = bonsai_windows_process::utc_now().into();
    }
    context.save()?;
    result.map_err(|error| format!("{error} Evidence: {}", context.evidence.display()))?;
    Ok(
        json!({"Status":"completed","ReportPath":context.evidence.join("cpu-evaluation.json"),"EvidenceDirectory":context.evidence,
        "Samples":4,"Qualification":"screen-only","DefaultProfileChanged":false}),
    )
}
