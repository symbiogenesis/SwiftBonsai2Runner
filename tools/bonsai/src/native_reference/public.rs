//! Hosted public reconstruction orchestration with one original absolute budget.

use super::{
    decode, digest, formats, integer, path, read, read_json, read_match, require, text, write_json,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{Read as _, Seek as _, SeekFrom},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub(super) const ARITHMETIC: &str = "public-mlx-f16-inverse1024-f16-rust-scalar-f32-dot-v2";
pub(super) const ORIGINAL_SOURCES: &[&str] = &[
    "config/swift-lora.lock.json",
    "config/swift-lora.direction.json",
    "tools/native-reference/public-authoring-pins.json",
    "tools/native-reference/public_pack_reference.ps1",
    "tools/native-reference/public_pack_formats.ps1",
    "tools/native-reference/audit_public_reference.ps1",
    "tools/native-reference/reference_numerics.rs",
];
pub(super) const NATIVE_SOURCES: &[&str] = &[
    "tools/bonsai/src/native_reference.rs",
    "tools/bonsai/src/native_reference/formats.rs",
    "tools/bonsai/src/native_reference/public.rs",
    "tools/bonsai/src/native_reference/audit.rs",
    "tools/bonsai/src/native_reference/native.rs",
    "tools/bonsai/src/native_reference/row.rs",
];
pub(super) fn blocks() -> impl Iterator<Item = u64> {
    (0..64).filter(|block| block % 4 != 3)
}

pub(super) fn hosted() -> crate::Result<()> {
    for (name, expected) in [
        ("GITHUB_ACTIONS", "true"),
        ("RUNNER_ENVIRONMENT", "github-hosted"),
        ("RUNNER_OS", "Windows"),
        ("RUNNER_ARCH", "ARM64"),
        ("GITHUB_REPOSITORY", "symbiogenesis/SwiftBonsai2Runner"),
        ("GITHUB_REF", "refs/heads/main"),
        ("GITHUB_EVENT_NAME", "workflow_dispatch"),
    ] {
        require(
            std::env::var(name).ok().as_deref() == Some(expected),
            "public reconstruction requires main-only GitHub-hosted Windows ARM64",
        )?;
    }
    require(
        std::env::var("GITHUB_SHA").is_ok_and(|sha| crate::identity::is_lower_hex(&sha, 40)),
        "source revision absent",
    )
}

fn remaining(until: Instant) -> crate::Result<Duration> {
    until
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
        .ok_or_else(|| "hosted reference absolute allowance expired".into())
}

pub(super) fn config_sites(raw: &[u8]) -> crate::Result<Value> {
    let config = decode(raw)?;
    require(
        config["schema_version"] == 2
            && config["model_type"] == "prism_hadamard_qwen35"
            && config["base_model_type"] == "qwen3_5"
            && config["tensor_namespace"] == "mlx-vlm-qwen3_5"
            && config["gdn_activation_layout"] == "grouped"
            && config["quantization"] == json!({"bits":2,"group_size":128,"mode":"affine"}),
        "public model layout/schema differs",
    )?;
    let modules = config["modules"]
        .as_array()
        .filter(|a| a.len() <= 1024)
        .ok_or("packed module inventory differs")?;
    let mut seen = std::collections::BTreeSet::new();
    let mut selected = std::collections::BTreeSet::new();
    for record in modules {
        let name = text(record, "path")?;
        require(seen.insert(name), "duplicate packed module")?;
        if name.ends_with(".linear_attn.out_proj") {
            require(
                record["block"] == 1024
                    && record["dtype"] == "float16"
                    && record["embedding"] == false,
                "exact SSM module geometry differs",
            )?;
            selected.insert(name.to_owned());
        }
    }
    require(
        selected
            == blocks()
                .map(|block| format!("model.layers.{block}.linear_attn.out_proj"))
                .collect(),
        "exact SSM module set differs",
    )?;
    Ok(blocks()
        .map(|block| {
            Value::from(format!(
                "language_model.model.layers.{block}.linear_attn.out_proj"
            ))
        })
        .collect::<Vec<_>>()
        .into())
}

fn safe_new_path(path: &Path) -> crate::Result<()> {
    require(path.is_absolute(), "reference directory must be absolute")?;
    require(
        !path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir)),
        "reference path traversal",
    )?;
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) => require(
                metadata.is_dir() && !super::redirected(&metadata),
                "reference directory path contains a redirected or non-directory component",
            )?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(())
}

pub(super) fn directories(data: &Path, evidence: &Path, temporary: &Path) -> crate::Result<()> {
    safe_new_path(temporary)?;
    safe_new_path(data)?;
    safe_new_path(evidence)?;
    let temporary = temporary.to_string_lossy();
    let data_text = data.to_string_lossy();
    let evidence_text = evidence.to_string_lossy();
    let normalize = |text: &str| {
        let text = text.replace('\\', "/").trim_end_matches('/').to_owned();
        if cfg!(windows) {
            text.to_lowercase()
        } else {
            text
        }
    };
    let prefix = normalize(&temporary) + "/";
    let normalized_data = normalize(&data_text);
    let normalized_evidence = normalize(&evidence_text);
    require(
        normalized_data.starts_with(&prefix)
            && normalized_evidence.starts_with(&prefix)
            && normalized_data != normalized_evidence
            && !data.exists(),
        "reference data/output must be fresh separate external runner-temp directories",
    )
}

fn pin(pin: &Value) -> crate::Result<()> {
    let object = pin.as_object().ok_or("invalid reference input pin")?;
    require(
        object.len() == 4
            && ["filename", "bytes", "sha256", "url"]
                .iter()
                .all(|key| object.contains_key(*key)),
        "input identity differs",
    )?;
    let filename = text(pin, "filename")?;
    let bytes = integer(pin, "bytes")?;
    require(
        !filename.is_empty()
            && filename != "."
            && filename != ".."
            && filename
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
            && (1..=9_000_000_000).contains(&bytes)
            && crate::identity::is_lower_hex(text(pin, "sha256")?, 64),
        "input identity differs",
    )?;
    crate::https_transfer::validate_uri(text(pin, "url")?)
}

fn receive(directory: &Path, pin_value: &Value, until: Instant) -> crate::Result<PathBuf> {
    remaining(until)?;
    pin(pin_value)?;
    let target = directory.join(text(pin_value, "filename")?);
    let partial = target.with_extension("partial");
    require(
        !target.exists() && !partial.exists(),
        "reference input must be fresh",
    )?;
    let mut output = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&partial)
        .map_err(|error| error.to_string())?;
    crate::https_transfer::download_fresh(
        text(pin_value, "url")?,
        &mut output,
        integer(pin_value, "bytes")?,
        until,
        &mut || remaining(until).map(|_| ()),
    )?;
    output.sync_all().map_err(|error| error.to_string())?;
    output
        .seek(SeekFrom::Start(0))
        .map_err(|error| error.to_string())?;
    require(
        output.metadata().map_err(|error| error.to_string())?.len() == integer(pin_value, "bytes")?
            && super::digest_reader(&mut output, &mut || remaining(until).map(|_| ()))?
                == text(pin_value, "sha256")?,
        "complete input raw-byte identity differs",
    )?;
    drop(output);
    fs::rename(&partial, &target).map_err(|error| error.to_string())?;
    Ok(target)
}

fn numeric(tool: &Path, arguments: &[String], work: &Path, until: Instant) -> crate::Result<Value> {
    let log = work.join("numeric.json");
    let arguments = arguments
        .iter()
        .map(std::ffi::OsString::from)
        .collect::<Vec<_>>();
    let result = crate::startup_process::run_with_checkpoint(
        tool.as_os_str(),
        &arguments,
        work,
        &BTreeMap::new(),
        remaining(until)?,
        &log,
        || remaining(until).map(|_| ()),
    )?;
    fs::remove_file(&log).map_err(|error| error.to_string())?;
    require(result.code == 0, "bounded numerical helper rejected inputs")?;
    decode(result.stdout.as_bytes())
}

fn metrics(
    tool: &Path,
    expected: &Path,
    actual: &Path,
    count: u64,
    tolerance: &str,
    work: &Path,
    until: Instant,
) -> crate::Result<Value> {
    let metric = numeric(
        tool,
        &[
            "compare".into(),
            expected.to_string_lossy().into(),
            actual.to_string_lossy().into(),
            "f32".into(),
            "0".into(),
            count.to_string(),
            "4".into(),
            tolerance.into(),
            tolerance.into(),
            "fma".into(),
        ],
        work,
        until,
    )?;
    Ok(
        json!({"comparedElements":metric["elements"],"mismatchedElements":metric["failedElements"],"maximumAbsoluteDifference":metric["maximumAbsoluteError"]}),
    )
}

pub(super) fn save_sources(
    repository: &Path,
    evidence: &Path,
    revision: &str,
    legacy: bool,
) -> crate::Result<Value> {
    let bundle = evidence.join("replay-source");
    fs::create_dir(&bundle).map_err(|error| error.to_string())?;
    let mut files = Vec::new();
    let sources = ORIGINAL_SOURCES
        .iter()
        .chain(if legacy { &[][..] } else { NATIVE_SOURCES });
    for relative in sources {
        let raw = read_match(&repository.join(relative), 1_048_576, None, None)?;
        require(!raw.is_empty(), "replay source is empty")?;
        let target = bundle.join(relative);
        fs::create_dir_all(target.parent().ok_or("missing replay source parent")?)
            .map_err(|error| error.to_string())?;
        fs::write(&target, &raw).map_err(|error| error.to_string())?;
        files.push(
            json!({"path":relative,"bytes":raw.len(),"sha256":crate::identity::sha256(&raw)}),
        );
    }
    let manifest = bundle.join("source-manifest.json");
    write_json(
        &manifest,
        &json!({"schemaVersion":if legacy{1}else{2},"kind":"public-reference-replay-source","sourceRevision":revision,"files":files}),
    )?;
    Ok(
        json!({"replaySourceManifestFile":"replay-source/source-manifest.json","replaySourceManifestSha256":digest(&manifest)?}),
    )
}

fn available_space(path: &Path) -> crate::Result<u64> {
    #[cfg(windows)]
    {
        bonsai_windows_process::available_disk_bytes(path).map_err(|error| error.to_string())
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err("public reconstruction requires native Windows storage admission".into())
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "One original absolute allowance and terminal attempt guard enclose the fixed all-48 hosted reconstruction stages."
)]
pub(super) fn produce(request: &Value) -> crate::Result<Value> {
    let until = Instant::now()
        .checked_add(Duration::from_mins(40))
        .ok_or("invalid hosted budget")?;
    hosted()?;
    let data = path(request, "dataDirectory")?;
    let evidence = path(request, "evidenceDirectory")?;
    let repository = path(request, "repository")?;
    let tool = path(request, "toolPath")?;
    require(
        tool.is_absolute() && tool.is_file(),
        "prebuilt reference numerical helper is required",
    )?;
    let temporary =
        PathBuf::from(std::env::var_os("RUNNER_TEMP").ok_or("runner temporary directory absent")?);
    directories(&data, &evidence, &temporary)?;
    fs::create_dir_all(&data)
        .and_then(|()| fs::create_dir_all(&evidence))
        .map_err(|error| error.to_string())?;
    let attempt = evidence.join("attempt.json");
    require(!attempt.exists(), "retain earlier reference attempts")?;
    let pins_raw = read(
        &repository.join("tools/native-reference/public-authoring-pins.json"),
        65_536,
    )?;
    let pins = decode(&pins_raw)?;
    require(
        pins["schemaVersion"] == 2
            && pins["arithmeticId"] == ARITHMETIC
            && pins["absoluteTolerance"].as_f64() == Some(0.00001)
            && pins["relativeTolerance"].as_f64() == Some(0.00001)
            && pins["normalization"] == "scalar-f64-sum-sqrt-rounded-f32"
            && pins["reduction"] == "scalar-sequential-f32-no-fma",
        "reference recipe or fixed bounds differ",
    )?;
    for (name, key) in [
        ("swift-lora.lock.json", "adapter"),
        ("swift-lora.direction.json", "direction"),
    ] {
        let lock = read_json(&repository.join("config").join(name), 1_048_576)?;
        for field in ["filename", "bytes", "sha256", "url"] {
            require(
                pins[key][field] == lock[key][field],
                "approved adapter/direction lock differs",
            )?;
        }
    }
    let env = |key: &str| std::env::var(key).unwrap_or_default();
    let mut report = json!({"schemaVersion":2,"kind":"independent-public-pack-A-reference","status":"running","stage":"inputs","arithmeticId":ARITHMETIC,"referencePinsSha256":crate::identity::sha256(&pins_raw),"inputPins":pins,"repository":env("GITHUB_REPOSITORY"),"workflowPath":".github/workflows/ptq1-public-authoring-reference.yml","sourceRevision":env("GITHUB_SHA"),"runId":env("GITHUB_RUN_ID"),"runAttempt":env("GITHUB_RUN_ATTEMPT"),"sites":[],"runnerEnvironment":env("RUNNER_ENVIRONMENT"),"authorOriginalProvenanceQualified":false,"nativeWriterBasisQualified":false,"logitsOrStateQualified":false,"defaultMayChange":false,"ownerDeviceTrials":0,"modelInferenceCommands":0,"fullCheckpointWritten":false,"all48PublicAComparisonPassed":false});
    let work = data.join("numeric-work");
    fs::create_dir(&work).map_err(|error| error.to_string())?;
    let result = (|| -> crate::Result<Value> {
        write_json(&attempt, &report)?;
        for (key, value) in save_sources(
            &repository,
            &evidence,
            text(&report, "sourceRevision")?,
            false,
        )?
        .as_object()
        .ok_or("invalid source binding")?
        {
            report[key] = value.clone();
        }
        let compiler = crate::startup_process::resolve(std::ffi::OsStr::new("rustc.exe"))?;
        let rust_log = work.join("rust-version.txt");
        let result = crate::startup_process::run(
            compiler.as_os_str(),
            &["--version".into(), "--verbose".into()],
            &work,
            &BTreeMap::new(),
            remaining(until)?,
            &rust_log,
        )?;
        require(result.code == 0, "reference compiler identity unavailable")?;
        let rust = result.stdout;
        fs::remove_file(rust_log).map_err(|error| error.to_string())?;
        let source_hash =
            |name: &str| digest(&repository.join("tools/native-reference").join(name));
        write_json(
            &evidence.join("toolchain.json"),
            &json!({"producerImplementation":"native-rust-v1","rust":rust.trim_end(),"referenceScriptSha256":source_hash("public_pack_reference.ps1")?,"formatScriptSha256":source_hash("public_pack_formats.ps1")?,"numericSourceSha256":source_hash("reference_numerics.rs")?,"numericExecutableSha256":digest(&tool)?,"policyExecutableSha256":digest(&std::env::current_exe().map_err(|error|error.to_string())?)?,"reduction":pins["reduction"],"normalization":pins["normalization"],"threads":1}),
        )?;
        let mut paths = BTreeMap::new();
        for name in ["config", "adapter", "direction"] {
            report["stage"] = format!("{name}-intake").into();
            write_json(&attempt, &report)?;
            paths.insert(name, receive(&data, &pins[name], until)?);
        }
        let config_raw = read_match(
            &paths["config"],
            integer(&pins["config"], "bytes")?,
            None,
            None,
        )?;
        let sites = config_sites(&config_raw)?;
        fs::write(evidence.join("config.json"), config_raw).map_err(|error| error.to_string())?;
        let normalized = work.join("normalized.f32le");
        let normal = numeric(
            &tool,
            &[
                "normalize".into(),
                paths["direction"].to_string_lossy().into(),
                normalized.to_string_lossy().into(),
            ],
            &work,
            until,
        )?;
        let direction = read_match(&normalized, 20_480, Some(20_480), None)?;
        let mut negative = direction.clone();
        for chunk in negative.as_chunks_mut::<4>().0 {
            chunk[3] ^= 0x80;
        }
        let negative_path = work.join("negative.f32le");
        fs::write(&negative_path, negative).map_err(|error| error.to_string())?;
        let factors = formats::adapter(&read_match(
            &paths["adapter"],
            integer(&pins["adapter"], "bytes")?,
            Some(integer(&pins["adapter"], "bytes")?),
            Some(text(&pins["adapter"], "sha256")?),
        )?)?;
        let factors = factors.as_object().ok_or("invalid adapter factors")?;
        let factor_path = work.join("factor.f32le");
        let mut b_results = Vec::new();
        for (name, factor) in factors {
            if name.ends_with(".lora_b") {
                let raw = super::audit::unhex(factor.as_str().ok_or("invalid factor bytes")?)?;
                fs::write(&factor_path, &raw).map_err(|error| error.to_string())?;
                let mut metric = metrics(
                    &tool,
                    &negative_path,
                    &factor_path,
                    5120,
                    "0.000_001",
                    &work,
                    until,
                )?;
                metric["tensor"] = name.clone().into();
                metric["sha256"] = crate::identity::sha256(&raw).into();
                b_results.push(metric);
            }
        }
        write_json(
            &evidence.join("direction-comparison.json"),
            &json!({"sites":b_results}),
        )?;
        require(
            b_results.len() == 129 && b_results.iter().all(|r| r["mismatchedElements"] == 0),
            "normalized direction differs from approved B factors",
        )?;
        report["directionF32NormBeforeNormalization"] = normal["norm"].clone();
        report["normalizedDirectionSha256"] = crate::identity::sha256(&direction).into();
        report["all129BComparisonsPassed"] = true.into();
        let available = available_space(&data)?;
        require(
            available >= integer(&pins["model"], "bytes")? + 1_073_741_824,
            "hosted storage cannot retain model plus reserve",
        )?;
        report["stage"] = "model-intake".into();
        write_json(&attempt, &report)?;
        let model_path = receive(&data, &pins["model"], until)?;
        report["stage"] = "model-full-rehash-and-header".into();
        write_json(&attempt, &report)?;
        let mut model =
            bonsai_artifacts::open_locked_file(&model_path).map_err(|error| error.to_string())?;
        let before = model.metadata().map_err(|error| error.to_string())?;
        require(
            before.len() == integer(&pins["model"], "bytes")?
                && super::digest_reader(&mut model, &mut || remaining(until).map(|_| ()))?
                    == text(&pins["model"], "sha256")?,
            "pre-read full input hash differs",
        )?;
        remaining(until)?;
        let header = formats::safetensors_header(&model_path)?;
        let header_raw = text(&header, "raw")?.as_bytes();
        fs::write(evidence.join("model-header.json"), header_raw)
            .map_err(|error| error.to_string())?;
        report["modelHeaderSha256"] = crate::identity::sha256(header_raw).into();
        report["modelHeaderBytes"] = header_raw.len().into();
        report["modelHeaderFile"] = "model-header.json".into();
        report["modelTensorCount"] = header["descriptors"]
            .as_object()
            .ok_or("invalid model descriptors")?
            .len()
            .into();
        let mut site_results = Vec::new();
        for (index, block) in blocks().enumerate() {
            remaining(until)?;
            let site = sites[index].as_str().ok_or("invalid public site")?;
            report["stage"] = format!("reconstruct-ssm-{block}").into();
            write_json(&attempt, &report)?;
            let mut wanted = json!({});
            let mut hashes = json!({});
            for suffix in ["weight", "scales", "biases", "signs"] {
                let descriptor = &header["descriptors"][format!("{site}.{suffix}")];
                super::audit::site_descriptor(descriptor, suffix)?;
                let begin = integer(descriptor, "absoluteByteBegin")?;
                let end = integer(descriptor, "absoluteByteEnd")?;
                require(
                    end >= begin && end - begin <= 128 * 1024 * 1024,
                    "model tensor extent exceeds bound",
                )?;
                model
                    .seek(SeekFrom::Start(begin))
                    .map_err(|error| error.to_string())?;
                let mut raw =
                    vec![0; usize::try_from(end - begin).map_err(|error| error.to_string())?];
                model
                    .read_exact(&mut raw)
                    .map_err(|error| error.to_string())?;
                wanted[suffix] = descriptor.clone();
                hashes[suffix] = crate::identity::sha256(&raw).into();
            }
            let name = format!("ssm-{block:02}.f32le");
            let vector = evidence.join(&name);
            let matrix = work.join("candidate.f16le");
            let mut arguments = vec!["reconstruct".into(), model_path.to_string_lossy().into()];
            for suffix in ["weight", "scales", "biases", "signs"] {
                arguments.push(integer(&wanted[suffix], "absoluteByteBegin")?.to_string());
            }
            arguments.extend([
                "5120".into(),
                "6144".into(),
                normalized.to_string_lossy().into(),
                vector.to_string_lossy().into(),
                matrix.to_string_lossy().into(),
                text(&wanted["signs"], "dtype")?.into(),
            ]);
            numeric(&tool, &arguments, &work, until)?;
            let matrix_digest = digest(&matrix)?;
            fs::remove_file(&matrix).map_err(|error| error.to_string())?;
            let shipped = super::audit::unhex(
                factors[&format!("blk.{block}.ssm_out.weight.lora_a")]
                    .as_str()
                    .ok_or("missing shipped factor")?,
            )?;
            fs::write(&factor_path, &shipped).map_err(|error| error.to_string())?;
            let mut metric = metrics(&tool, &vector, &factor_path, 6144, "0.00001", &work, until)?;
            for (key, value) in [
                ("block", json!(block)),
                ("publicTensorPrefix", json!(site)),
                ("candidateF16MatrixSha256", json!(matrix_digest)),
                ("authoringAFile", json!(name)),
                ("authoringASha256", json!(digest(&vector)?)),
                ("authoringABytes", json!(24_576)),
                ("shippedASha256", json!(crate::identity::sha256(&shipped))),
                ("inputTensorDescriptors", wanted),
                ("inputTensorSha256", hashes),
            ] {
                metric[key] = value;
            }
            site_results.push(metric);
            report["sites"] = site_results.clone().into();
            write_json(&attempt, &report)?;
        }
        let after = fs::metadata(&model_path).map_err(|error| error.to_string())?;
        require(
            before.len() == after.len() && before.modified().ok() == after.modified().ok(),
            "input endpoint identity changed",
        )?;
        remaining(until)?;
        let passed =
            site_results.len() == 48 && site_results.iter().all(|r| r["mismatchedElements"] == 0);
        report["all48PublicAComparisonPassed"] = passed.into();
        report["status"] = if passed {
            "complete-public-reference-match"
        } else {
            "rejected-public-A-comparison"
        }
        .into();
        report["stage"] = "complete".into();
        write_json(&evidence.join("report.json"), &report)?;
        Ok(report.clone())
    })();
    if result.is_err() {
        report["status"] = "failed".into();
        report["failureClass"] = "native-validation-or-execution".into();
    }
    write_json(&attempt, &report)?;
    result
}
