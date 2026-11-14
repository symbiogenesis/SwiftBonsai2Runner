//! Fixed historical artifact admission and integrity replay without producer execution.

use super::{decode, digest, integer, path, read_json, require, text, write_json};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read as _, Seek as _, SeekFrom, Write as _},
    path::Path,
    time::Duration,
};

pub(super) const REPOSITORY: &str = "symbiogenesis/SwiftBonsai2Runner";
pub(super) const REVISION: &str = "469f57b233898fd10b97d5ebc10aaef78b940e30";
pub(super) const RUNTIME: &str = "cae16fa6998dac58a947eba9ae4398ed6a9fb863";
pub(super) const RUN: u64 = 41_568_281_287;
pub(super) const ARTIFACT: u64 = 13_200_218_351;
pub(super) const ARCHIVE_BYTES: u64 = 294_092_332;
pub(super) const ARCHIVE_SHA: &str =
    "c31de3e837f4d4d43df5d144892c0b5c4b36d747ab763799abe0556b419af8d8";
pub(super) const COMPARATOR_SHA: &str =
    "b8372072b3d77a918c5c6fa7038da95f51a50701f6927fb4b4931a591b056c08";
pub(super) const WORKFLOW: &str = ".github/workflows/native-ptq1-numerics.yml";

pub(super) fn producer(run: &Value, artifact: &Value, jobs: &Value) -> crate::Result<()> {
    require(
        run["id"] == RUN
            && run["head_sha"] == REVISION
            && run["head_branch"] == "main"
            && run["event"] == "workflow_dispatch"
            && run["run_attempt"] == 1
            && run["path"] == WORKFLOW
            && run["status"] == "completed"
            && run["conclusion"] == "success"
            && run["repository"]["full_name"] == REPOSITORY,
        "inexact original producer",
    )?;
    require(
        artifact["id"] == ARTIFACT
            && artifact["name"] == format!("native-ptq1-numerics-{RUN}-1")
            && artifact["size_in_bytes"] == ARCHIVE_BYTES
            && artifact["expired"] == false
            && artifact["digest"] == format!("sha256:{ARCHIVE_SHA}")
            && artifact["workflow_run"]["id"] == RUN
            && artifact["workflow_run"]["head_sha"] == REVISION
            && artifact["workflow_run"]["head_branch"] == "main"
            && artifact["workflow_run"]["repository_id"] == run["repository"]["id"],
        "inexact original artifact",
    )?;
    let items = jobs["jobs"]
        .as_array()
        .filter(|a| a.len() == 2)
        .ok_or("original job inventory differs")?;
    require(
        jobs["total_count"] == 2
            && items
                .iter()
                .filter_map(|j| j["name"].as_str())
                .collect::<BTreeSet<_>>()
                == BTreeSet::from(["numerics", "prepare"]),
        "original job inventory differs",
    )?;
    for item in items {
        require(
            item["status"] == "completed"
                && item["conclusion"] == "success"
                && item["run_id"] == RUN
                && item["head_sha"] == REVISION,
            "original jobs did not complete successfully",
        )?;
        if item["name"] == "numerics" {
            require(
                item["labels"] == json!([format!("ptq1-numerics-{RUN}")]),
                "inexact original owner label",
            )?;
        }
    }
    Ok(())
}

pub(super) fn redirect(url: &str) -> crate::Result<()> {
    crate::https_transfer::validate_uri(url)?;
    let parsed: ureq::http::Uri = url.parse().map_err(|_| "invalid retained redirect")?;
    let host = parsed
        .host()
        .ok_or("retained redirect host missing")?
        .to_ascii_lowercase();
    require(
        host.ends_with(".blob.core.windows.net") || host.ends_with(".githubusercontent.com"),
        "unapproved artifact redirect",
    )
}

fn endpoint(endpoint: &str, token: &str, destination: Option<&Path>) -> crate::Result<Value> {
    let config = ureq::Agent::config_builder()
        .https_only(true)
        .http_status_as_error(false)
        .max_redirects(0)
        .timeout_global(Some(Duration::from_secs(120)));
    #[cfg(windows)]
    let config = config.tls_config(
        ureq::tls::TlsConfig::builder()
            .provider(ureq::tls::TlsProvider::NativeTls)
            .root_certs(ureq::tls::RootCerts::PlatformVerifier)
            .build(),
    );
    let agent = config.build().new_agent();
    let mut response = agent
        .get(format!(
            "https://api.github.com/repos/{REPOSITORY}/{endpoint}"
        ))
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", "bonsai-retained-reference")
        .call()
        .map_err(|error| error.to_string())?;
    if [301, 302, 303, 307, 308].contains(&response.status().as_u16()) {
        require(destination.is_some(), "unexpected metadata redirect")?;
        let target = response
            .headers()
            .get("location")
            .and_then(|h| h.to_str().ok())
            .ok_or("artifact redirect missing")?
            .to_owned();
        redirect(&target)?;
        // A fresh credential-free request, with automatic redirects disabled.
        response = agent
            .get(target)
            .call()
            .map_err(|error| error.to_string())?;
    }
    require(
        response.status() == 200,
        "retained endpoint response differs",
    )?;
    let maximum = if destination.is_some() {
        ARCHIVE_BYTES
    } else {
        4_194_304
    };
    if let Some(target) = destination {
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(target)
            .map_err(|error| error.to_string())?;
        let copied = std::io::copy(
            &mut response.body_mut().as_reader().take(maximum + 1),
            &mut output,
        )
        .map_err(|error| error.to_string())?;
        output.sync_all().map_err(|error| error.to_string())?;
        require(
            copied == maximum && digest(target)? == ARCHIVE_SHA,
            "archive raw length/hash differs",
        )?;
        Ok(Value::Null)
    } else {
        let mut raw = Vec::new();
        response
            .body_mut()
            .as_reader()
            .take(maximum + 1)
            .read_to_end(&mut raw)
            .map_err(|error| error.to_string())?;
        require(
            u64::try_from(raw.len()).map_err(|error| error.to_string())? <= maximum,
            "retained response exceeds byte bound",
        )?;
        decode(&raw)
    }
}

fn central(archive: &Path) -> crate::Result<Vec<(String, u64)>> {
    let mut file = File::open(archive).map_err(|error| error.to_string())?;
    let length = file.metadata().map_err(|error| error.to_string())?.len();
    let tail_len = length.min(65_557);
    file.seek(SeekFrom::Start(length - tail_len))
        .map_err(|error| error.to_string())?;
    let mut tail = vec![0; usize::try_from(tail_len).map_err(|error| error.to_string())?];
    file.read_exact(&mut tail)
        .map_err(|error| error.to_string())?;
    let u16_at = |bytes: &[u8], at: usize| -> crate::Result<u16> {
        Ok(u16::from_le_bytes(
            bytes
                .get(at..at + 2)
                .ok_or("truncated ZIP integer")?
                .try_into()
                .map_err(|_| "truncated ZIP integer")?,
        ))
    };
    let u32_at = |bytes: &[u8], at: usize| -> crate::Result<u32> {
        Ok(u32::from_le_bytes(
            bytes
                .get(at..at + 4)
                .ok_or("truncated ZIP integer")?
                .try_into()
                .map_err(|_| "truncated ZIP integer")?,
        ))
    };
    let end = (0..tail.len().saturating_sub(21))
        .rev()
        .find(|&at| {
            u32_at(&tail, at).ok() == Some(0x0605_4b50)
                && u16_at(&tail, at + 20).is_ok_and(|n| at + 22 + usize::from(n) == tail.len())
        })
        .ok_or("unsupported ZIP directory")?;
    require(
        u16_at(&tail, end + 4)? == 0 && u16_at(&tail, end + 6)? == 0,
        "unsupported ZIP directory",
    )?;
    let count = u16_at(&tail, end + 10)?;
    require(
        count == u16_at(&tail, end + 8)? && count < u16::MAX,
        "unsupported ZIP member count",
    )?;
    let bytes = u64::from(u32_at(&tail, end + 12)?);
    let offset = u64::from(u32_at(&tail, end + 16)?);
    require(
        offset + bytes
            == length - tail_len + u64::try_from(end).map_err(|error| error.to_string())?,
        "ZIP directory extent differs",
    )?;
    file.seek(SeekFrom::Start(offset))
        .map_err(|error| error.to_string())?;
    let mut records = Vec::new();
    for _ in 0..count {
        let mut header = [0; 46];
        file.read_exact(&mut header)
            .map_err(|error| error.to_string())?;
        require(
            u32_at(&header, 0)? == 0x0201_4b50,
            "ZIP central record differs",
        )?;
        let flags = u16_at(&header, 8)?;
        let method = u16_at(&header, 10)?;
        let size = u64::from(u32_at(&header, 24)?);
        let attributes = u32_at(&header, 38)?;
        require(
            flags & 1 == 0
                && [0, 8].contains(&method)
                && size <= 536_870_912
                && [0, 0x8000].contains(&((attributes >> 16) & 0xf000))
                && attributes & 0x10 == 0,
            "invalid ZIP encryption/method/extent",
        )?;
        let mut name = vec![0; usize::from(u16_at(&header, 28)?)];
        file.read_exact(&mut name)
            .map_err(|error| error.to_string())?;
        let name = String::from_utf8(name).map_err(|error| error.to_string())?;
        file.seek(SeekFrom::Current(
            i64::from(u16_at(&header, 30)?) + i64::from(u16_at(&header, 32)?),
        ))
        .map_err(|error| error.to_string())?;
        records.push((name, size));
    }
    require(
        file.stream_position().map_err(|error| error.to_string())? == offset + bytes,
        "ZIP central directory trailing bytes",
    )?;
    Ok(records)
}

pub(super) fn extract(
    archive: &Path,
    destination: &Path,
    expected: &Value,
) -> crate::Result<Value> {
    let expected = expected.as_object().ok_or("invalid retained inventory")?;
    let metadata = central(archive)?;
    require(
        metadata.len() == expected.len(),
        "archive member count differs",
    )?;
    let mut zip = zip::ZipArchive::new(File::open(archive).map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())?;
    require(zip.len() == expected.len(), "archive member count differs")?;
    let mut seen = BTreeSet::new();
    let mut manifest = BTreeMap::new();
    for (index, (central_name, central_size)) in metadata.iter().enumerate() {
        let mut member = zip.by_index(index).map_err(|error| error.to_string())?;
        let name = member
            .name()
            .map_err(|error| error.to_string())?
            .into_owned();
        require(
            name == *central_name
                && member.size() == *central_size
                && name.len() <= 200
                && !name.is_empty()
                && !name.contains([':', '\\'])
                && name
                    .split('/')
                    .all(|part| !part.is_empty() && part != "." && part != "..")
                && expected.contains_key(&name)
                && seen.insert(name.to_lowercase()),
            "unsafe unexpected or duplicate archive member",
        )?;
        let mode = member.unix_mode().unwrap_or(0) & 0xf000;
        let record = &expected[&name];
        require(
            [0, 0x8000].contains(&mode)
                && !member.is_dir()
                && member.size() == integer(record, "bytes")?
                && member.size() <= 536_870_912,
            "invalid archive member type or extent",
        )?;
        let target = destination.join(&name);
        fs::create_dir_all(target.parent().ok_or("missing archive member parent")?)
            .map_err(|error| error.to_string())?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
            .map_err(|error| error.to_string())?;
        let mut hash = crate::identity::Hasher::new()?;
        let mut total = 0;
        let mut buffer = vec![0; 1_048_576];
        loop {
            let count = member
                .read(&mut buffer)
                .map_err(|error| error.to_string())?;
            if count == 0 {
                break;
            }
            total += u64::try_from(count).map_err(|error| error.to_string())?;
            require(
                total <= member.size(),
                "expanded member exceeds declared extent",
            )?;
            hash.update(&buffer[..count])?;
            output
                .write_all(&buffer[..count])
                .map_err(|error| error.to_string())?;
        }
        let sha = hash.finalize_hex()?;
        require(
            total == member.size() && sha == text(record, "sha256")?,
            "retained member hash differs",
        )?;
        manifest.insert(
            name.clone(),
            json!({"path":name,"bytes":total,"sha256":sha}),
        );
    }
    require(seen.len() == expected.len(), "missing retained member")?;
    Ok(manifest.into_values().collect::<Vec<_>>().into())
}

pub(super) fn replay(root: &Path, comparator: &Path) -> crate::Result<Value> {
    require(
        digest(comparator)? == COMPARATOR_SHA,
        "original comparator source differs",
    )?;
    let original = read_json(&root.join("comparison.json"), 1_048_576)?;
    let owner = read_json(&root.join("numerics.json"), 1_048_576)?;
    require(
        owner["Revision"] == REVISION
            && (owner["RunAttempt"] == 1 || owner["RunAttempt"] == "1")
            && text(&owner, "RunUrl")?.ends_with(&format!("/actions/runs/{RUN}"))
            && owner["Status"] == "completed-one-token-smoke"
            && owner["Phase"] == "completed"
            && owner["Comparison"] == original
            && owner["DefaultMayChange"] == false
            && owner["Plan"]["RustSource"]["commit"] == RUNTIME
            && owner["Plan"]["RustBackend"] == "neon"
            && owner["Plan"]["RustRowWorkers"] == 4
            && owner["Plan"]["AdapterCondition"] == "standard"
            && owner["Plan"]["AdapterScale"].as_f64() == Some(1.3),
        "retained owner/comparison binding differs",
    )?;
    require(
        original["status"] == "passed-bounded-one-token-smoke"
            && original["files"] == 129
            && original["elements"] == 39_504_384
            && original["byteExactFiles"] == 129
            && original["failedElements"] == 0
            && original["defaultMayChange"] == false
            && original["backendRequested"] == "neon"
            && original["rustRowWorkers"] == 4
            && original["adapterCondition"] == "standard"
            && original["adapterScale"].as_f64() == Some(1.3),
        "retained selected comparison differs",
    )?;
    let recomputed = super::native::compare(
        &json!({"native":root.join("native"),"rust":root.join("neon"),"backend":"neon","rowWorkers":4,"adapterCondition":"standard"}),
    )?;
    require(
        recomputed == original,
        "replayed comparison differs from original report",
    )?;
    Ok(recomputed)
}

#[expect(
    clippy::too_many_lines,
    reason = "One failure-retaining operation encloses hosted admission, exact artifact intake, replay and scratch cleanup."
)]
pub(super) fn audit(request: &Value) -> crate::Result<Value> {
    let comparator = path(request, "comparator")?;
    let output = path(request, "outputDirectory")?;
    let repository = path(request, "repository")?;
    require(!output.exists(), "retained audit output must be fresh")?;
    fs::create_dir_all(&output).map_err(|error| error.to_string())?;
    let mut report = json!({"schemaVersion":2,"status":"failed","stage":"hosted-guard","sourceRunId":RUN.to_string(),"sourceRevision":REVISION,"artifactId":ARTIFACT.to_string(),"artifactSha256":ARCHIVE_SHA,"comparatorSourceSha256":COMPARATOR_SHA,"replayImplementationSha256":digest(&repository.join("tools/bonsai/src/native_reference/native.rs"))?,"originalComparatorExecuted":false,"auditRevision":std::env::var("GITHUB_SHA").unwrap_or_default(),"auditRunId":std::env::var("GITHUB_RUN_ID").unwrap_or_default(),"ownerDeviceExecutedByAudit":false,"modelExecutedByAudit":false,"defaultMayChange":false});
    let mut scratch = None;
    let result = (|| -> crate::Result<()> {
        for (key, value) in [
            ("GITHUB_ACTIONS", "true"),
            ("GITHUB_REPOSITORY", REPOSITORY),
            ("GITHUB_REF", "refs/heads/main"),
            ("RUNNER_ENVIRONMENT", "github-hosted"),
        ] {
            require(
                std::env::var(key).ok().as_deref() == Some(value),
                "hosted main only",
            )?;
        }
        let token = std::env::var("GH_TOKEN")
            .ok()
            .filter(|s| !s.is_empty())
            .ok_or("artifact credential unavailable")?;
        report["stage"] = "producer-metadata".into();
        let run = endpoint(&format!("actions/runs/{RUN}"), &token, None)?;
        let artifact = endpoint(&format!("actions/artifacts/{ARTIFACT}"), &token, None)?;
        let jobs = endpoint(
            &format!("actions/runs/{RUN}/jobs?filter=all&per_page=100"),
            &token,
            None,
        )?;
        producer(&run, &artifact, &jobs)?;
        write_json(
            &output.join("producer.json"),
            &json!({"run":run,"artifact":artifact,"jobs":jobs}),
        )?;
        let root = std::path::PathBuf::from(
            std::env::var_os("RUNNER_TEMP").ok_or("runner temporary directory absent")?,
        )
        .join(format!(
            "ptq1-retained-{}",
            crate::startup_process::unique()
        ));
        fs::create_dir(&root).map_err(|error| error.to_string())?;
        scratch = Some(root.clone());
        let archive = root.join("evidence.zip");
        let extracted = root.join("extracted");
        report["stage"] = "archive-download".into();
        endpoint(
            &format!("actions/artifacts/{ARTIFACT}/zip"),
            &token,
            Some(&archive),
        )?;
        report["stage"] = "retained-member-integrity".into();
        let inventory = read_json(
            &repository.join(format!(
                "tools/native-reference/fixtures/retained-{RUN}.inventory.json"
            )),
            1_048_576,
        )?;
        let files = inventory["files"]
            .as_array()
            .filter(|a| a.len() == 155)
            .ok_or("expected inventory producer/extent differs")?;
        require(
            inventory["sourceRunId"].as_str() == Some("41568281287")
                && inventory["sourceRevision"] == REVISION
                && files
                    .iter()
                    .map(|f| integer(f, "bytes"))
                    .collect::<crate::Result<Vec<_>>>()?
                    .iter()
                    .sum::<u64>()
                    == 316_267_936,
            "expected inventory producer/extent differs",
        )?;
        let mut expected = serde_json::Map::new();
        for file in files {
            require(
                expected
                    .insert(text(file, "path")?.into(), file.clone())
                    .is_none(),
                "duplicate expected inventory member",
            )?;
        }
        let manifest = extract(&archive, &extracted, &Value::Object(expected))?;
        write_json(
            &output.join("manifest.json"),
            &json!({"sourceRunId":RUN.to_string(),"sourceRevision":REVISION,"files":manifest}),
        )?;
        report["stage"] = "replacement-comparator-replay".into();
        let comparison = replay(&extracted, &comparator)?;
        fs::copy(
            extracted.join("comparison.json"),
            output.join("original-comparison.json"),
        )
        .map_err(|error| error.to_string())?;
        write_json(&output.join("replayed-comparison.json"), &comparison)?;
        report["retainedFiles"] = 155.into();
        report["retainedBytes"] = 316_267_936.into();
        for (destination, source) in [
            ("comparedFiles", "files"),
            ("comparedValues", "elements"),
            ("byteExactFiles", "byteExactFiles"),
            ("modelSha256", "modelSha256"),
            ("adapterSha256", "adapterSha256"),
            ("nativeCaptureSha256", "nativeCaptureSha256"),
            ("nativeStateSha256", "nativeStateSha256"),
            ("rustStdoutSha256", "rustStdoutSha256"),
        ] {
            report[destination] = comparison[source].clone();
        }
        report["runtimeRevision"] = RUNTIME.into();
        report["originalExecutionUnchanged"] = true.into();
        report["fullContextQualified"] = false.into();
        report["scope"]="Retained byte integrity and replacement-comparator replay only; no new model/device execution, promotion, provenance, quality or full-context acceptance.".into();
        report["status"] = "passed-retained-capture-integrity-and-replay".into();
        report["stage"] = "complete".into();
        Ok(())
    })();
    if let Err(error) = result {
        report["failureClass"] = "native-validation-or-execution".into();
        report["failureReason"] = error.into();
    }
    if let Some(scratch) = scratch {
        fs::remove_dir_all(scratch).map_err(|error| error.to_string())?;
    }
    write_json(&output.join("audit.json"), &report)?;
    Ok(report)
}
