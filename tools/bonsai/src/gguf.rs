//! Native GGUF inspection/export entry points and hosted independent fixtures.

use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use bonsai_artifacts::{VerifiedFile, validate_external_path};
use serde::Deserialize;
use serde_json::Value;

mod checks;

const REFERENCE: &str = include_str!("../../../config/runtime.lock.json");
const SWIFT: &str = include_str!("../../../config/swift-assessment.lock.json");
const INFERENCE: &str = include_str!("../../../config/prism-inference.lock.json");
const PRISM: &str = "Prism-73bfe6cc485554822f80c473a3c0c4f25e80f41b";

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "kebab-case", deny_unknown_fields)]
enum Request {
    Inspect(Inspect),
    ExportPtq1(Export),
    Check(checks::Request),
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Inspect {
    model_path: PathBuf,
    output_path: Option<PathBuf>,
    expected_bytes: Option<u64>,
    expected_sha256: Option<String>,
    #[serde(default = "stock")]
    dialect: String,
    #[serde(default)]
    census_pq2: bool,
    gguf_tool_path: Option<PathBuf>,
    data_directory: Option<PathBuf>,
    #[serde(default = "inspection_timeout")]
    timeout_seconds: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Export {
    model_path: PathBuf,
    output_path: PathBuf,
    gguf_tool_path: Option<PathBuf>,
    data_directory: Option<PathBuf>,
    #[serde(default = "export_timeout")]
    timeout_seconds: u64,
}

fn stock() -> String {
    "Stock".into()
}
const fn inspection_timeout() -> u64 {
    300
}
const fn export_timeout() -> u64 {
    1800
}

fn external(path: &Path) -> crate::Result<()> {
    validate_external_path(path).map_err(|error| error.to_string())
}

fn new_output(path: &Path) -> crate::Result<()> {
    external(path)?;
    if path.try_exists().map_err(|error| error.to_string())? {
        return Err("Output must name a new external file.".into());
    }
    if !path.parent().is_some_and(Path::is_dir) {
        return Err("The external output directory must already exist.".into());
    }
    Ok(())
}

fn pin(source: &str) -> crate::Result<Value> {
    serde_json::from_str(source).map_err(|error| error.to_string())
}

fn text<'a>(value: &'a Value, name: &str) -> crate::Result<&'a str> {
    value[name]
        .as_str()
        .ok_or_else(|| format!("Missing compiled GGUF pin: {name}"))
}

fn count(value: &Value, name: &str) -> crate::Result<u64> {
    value[name]
        .as_u64()
        .ok_or_else(|| format!("Missing compiled GGUF count: {name}"))
}

fn artifact(
    bytes: Option<u64>,
    digest: Option<&str>,
    default: &str,
) -> crate::Result<(u64, String)> {
    let (bytes, digest) = match (bytes, digest) {
        (Some(bytes), Some(digest)) => (bytes, digest.to_ascii_lowercase()),
        (None, None) => {
            let value = pin(default)?;
            (
                count(&value["model"], "bytes")?,
                text(&value["model"], "sha256")?.into(),
            )
        }
        _ => return Err("Specify both expectedBytes and expectedSha256, or neither.".into()),
    };
    if bytes == 0 || bytes > i64::MAX.unsigned_abs() || !crate::identity::is_lower_hex(&digest, 64)
    {
        return Err("A positive artifact length and exact SHA-256 are required.".into());
    }
    Ok((bytes, digest))
}

fn inspect_arguments(request: &Inspect) -> crate::Result<Vec<OsString>> {
    external(&request.model_path)?;
    if request.dialect != "Stock" && request.dialect != PRISM {
        return Err("Use Stock or the explicitly pinned Prism dialect.".into());
    }
    if request.census_pq2 && request.dialect == "Stock" {
        return Err("PQ2 census requires the explicit pinned Prism dialect.".into());
    }
    let (bytes, digest) = artifact(
        request.expected_bytes,
        request.expected_sha256.as_deref(),
        REFERENCE,
    )?;
    let mut arguments = vec![
        "inspect".into(),
        "--model".into(),
        request.model_path.as_os_str().to_owned(),
        "--expected-bytes".into(),
        bytes.to_string().into(),
        "--expected-sha256".into(),
        digest.into(),
        "--dialect".into(),
        request.dialect.clone().into(),
    ];
    if request.census_pq2 {
        arguments.push("--census-pq2".into());
    }
    if let Some(output) = &request.output_path {
        new_output(output)?;
        arguments.extend(["--output".into(), output.as_os_str().to_owned()]);
    }
    Ok(arguments)
}

fn export_arguments(request: &Export) -> crate::Result<Vec<OsString>> {
    external(&request.model_path)?;
    new_output(&request.output_path)?;
    let (bytes, digest) = artifact(None, None, SWIFT)?;
    Ok(vec![
        "export-ptq1".into(),
        "--model".into(),
        request.model_path.as_os_str().to_owned(),
        "--output".into(),
        request.output_path.as_os_str().to_owned(),
        "--expected-bytes".into(),
        bytes.to_string().into(),
        "--expected-sha256".into(),
        digest.into(),
        "--timeout-seconds".into(),
        request.timeout_seconds.to_string().into(),
    ])
}

struct Tool {
    path: PathBuf,
    _guards: Vec<VerifiedFile>,
}

fn verify(path: &Path, value: &Value) -> crate::Result<VerifiedFile> {
    VerifiedFile::verify(path, count(value, "bytes")?, text(value, "sha256")?)
        .map_err(|error| error.to_string())
}

fn resolve(explicit: Option<PathBuf>, data: Option<PathBuf>) -> crate::Result<Tool> {
    if let Some(path) = explicit.or_else(|| {
        std::env::var_os("BONSAI_GGUF_TOOL")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    }) {
        external(&path)?;
        if !path.is_file() {
            return Err("ggufToolPath must name an existing expanded absolute executable.".into());
        }
        return Ok(Tool {
            path,
            _guards: Vec::new(),
        });
    }
    let data = data
        .or_else(|| std::env::var_os("BONSAI_DATA_DIRECTORY").map(PathBuf::from))
        .or_else(|| {
            std::env::var_os("LOCALAPPDATA").map(|path| PathBuf::from(path).join("SwiftBonsai2"))
        })
        .ok_or("Select dataDirectory or an explicit GGUF executable.")?;
    external(&data)?;
    let runtime = pin(INFERENCE)?;
    let directory = data
        .join("inference")
        .join(text(&runtime, "commit")?)
        .join(text(&runtime, "target")?);
    external(&directory)?;
    let mut guards = vec![verify(
        &data
            .join("downloads")
            .join(text(&runtime["asset"], "filename")?),
        &runtime["asset"],
    )?];
    let mut names = BTreeSet::new();
    for file in runtime["files"]
        .as_array()
        .ok_or("Missing compiled inference inventory.")?
    {
        let name = text(file, "name")?;
        if !names.insert(name) || Path::new(name).components().count() != 1 {
            return Err("Invalid compiled inference inventory.".into());
        }
        let path = directory.join(name);
        guards.push(verify(&path, file)?);
        if Path::new(name).extension().is_some_and(|extension| extension.eq_ignore_ascii_case("exe")) {
            crate::startup_cache::assert_arm64(&path)?;
        }
    }
    let entries = fs::read_dir(&directory)
        .map_err(|error| error.to_string())?
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(|error| error.to_string())?;
    if entries.len() != names.len()
        || entries.iter().any(|entry| {
            entry
                .file_name()
                .to_str()
                .is_none_or(|name| !names.contains(name))
        })
    {
        return Err("Installed inference files differ from the locked inventory.".into());
    }
    if !names.contains("bonsai-gguf.exe") {
        return Err("The pinned inference release has no GGUF executable.".into());
    }
    Ok(Tool {
        path: directory.join("bonsai-gguf.exe"),
        _guards: guards,
    })
}

fn invoke(
    tool: &Path,
    arguments: &[OsString],
    work: &Path,
    seconds: u64,
) -> crate::Result<crate::startup_process::ProcessOutput> {
    crate::startup_process::run(
        tool.as_os_str(),
        arguments,
        work,
        &BTreeMap::new(),
        Duration::from_secs(seconds),
        &work.join(format!("gguf-{}.log", crate::startup_process::unique())),
    )
}

fn decode(output: &crate::startup_process::ProcessOutput) -> crate::Result<Value> {
    if output.code != 0 {
        return Err(format!(
            "GGUF tool exited with code {}: {}{}",
            output.code, output.stderr, output.stdout
        ));
    }
    let value: Value = serde_json::from_str(&output.stdout).map_err(|error| error.to_string())?;
    if !value.is_object() {
        return Err("The GGUF tool did not return a JSON object.".into());
    }
    Ok(value)
}

fn execute(
    tool: Option<PathBuf>,
    data: Option<PathBuf>,
    arguments: &[OsString],
    seconds: u64,
) -> crate::Result<Value> {
    if !(1..=3600).contains(&seconds) {
        return Err("GGUF process timeout must be between 1 and 3600 seconds.".into());
    }
    let tool = resolve(tool, data)?;
    let work = std::env::temp_dir().join(format!(
        "SwiftBonsai-Gguf-{}",
        crate::startup_process::unique()
    ));
    external(&work)?;
    fs::create_dir(&work).map_err(|error| error.to_string())?;
    decode(&invoke(&tool.path, arguments, &work, seconds)?)
}

/// Inspect verified metadata, count pinned PQ2 symbols, export the pinned Swift
/// artifact or execute explicit hosted synthetic integration controls.
///
/// # Errors
/// Rejects incomplete identities, unsafe paths, unsupported dialects, existing
/// outputs, missing tools, changed installations and failed or expired processes.
pub fn run(value: Value) -> crate::Result<Value> {
    match serde_json::from_value(value).map_err(|error| error.to_string())? {
        Request::Inspect(request) => {
            let arguments = inspect_arguments(&request)?;
            execute(
                request.gguf_tool_path,
                request.data_directory,
                &arguments,
                request.timeout_seconds,
            )
        }
        Request::ExportPtq1(request) => {
            let arguments = export_arguments(&request)?;
            let mut result = execute(
                request.gguf_tool_path,
                request.data_directory,
                &arguments,
                request.timeout_seconds,
            )?;
            result["SourceRevision"] = pin(SWIFT)?["model"]["revision"].clone();
            Ok(result)
        }
        Request::Check(request) => checks::run(&request),
    }
}

#[cfg(test)]
mod tests;
