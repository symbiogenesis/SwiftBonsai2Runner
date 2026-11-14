//! Independent bounded layout and source-derived payload controls.
use crate::{Result, bounded_input, identity};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io::Write as _,
    path::{Path, PathBuf},
};

mod contracts;
mod fixtures;
mod ledgers;
mod pq2;
#[cfg(test)]
mod tests;
mod tiling;

const ROOT: &str = "tools/bonsai/src/hexagon_policy/audit/";
const OPERATION: &str = "docs/evidence/hexagon-swift-operation-ledger.json";
const TRAFFIC: &str = "docs/evidence/hexagon-swift-traffic-ledger.json";
const BUDGET: &str = "docs/evidence/hexagon-session-operation-budget.json";
const RESIDENT: &str = "docs/evidence/hexagon-resident-host-24000-ledger.json";
const TILING: &str = "docs/evidence/hexagon-k-tiling-and-reduction-audit.json";
const NATIVE: &str = "73bfe6cc485554822f80c473a3c0c4f25e80f41b";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    operation: String,
    repository_directory: PathBuf,
    #[serde(default)]
    metadata: Option<PathBuf>,
    #[serde(default)]
    census: Option<PathBuf>,
    #[serde(default)]
    payload: Option<PathBuf>,
    #[serde(default)]
    columns: u64,
    #[serde(default = "one")]
    rows: u64,
    #[serde(default)]
    emit: bool,
    #[serde(default)]
    write: bool,
    #[serde(default)]
    name: String,
}
const fn one() -> u64 {
    1
}

fn bytes(path: &Path, maximum: u64) -> Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !metadata.is_file() || bonsai_artifacts::is_redirect(&metadata) {
        return Err("Audit input must be an ordinary file.".into());
    }
    let input = File::open(path).map_err(|error| error.to_string())?;
    bounded_input::read(input, maximum)
        .map_err(|error| error.into_message("Input exceeds bounded audit size."))
}
fn read(root: &Path, name: &str) -> Result<Value> {
    let raw = bytes(&root.join(name), 1_048_576)?;
    super::json::parse_bounded(
        std::str::from_utf8(&raw).map_err(|error| error.to_string())?,
        1_048_576,
        64,
    )
}
fn source(root: &Path, name: &str) -> Result<Vec<u8>> {
    let raw = bytes(&root.join(name), 1_048_576)?;
    Ok(std::str::from_utf8(&raw)
        .map_err(|error| error.to_string())?
        .replace("\r\n", "\n")
        .into_bytes())
}
fn sources(root: &Path, names: &[&str]) -> Result<Value> {
    names
        .iter()
        .map(|name| Ok(json!({"path":name,"sha256":identity::sha256(&source(root,name)?)})))
        .collect::<Result<Vec<_>>>()
        .map(Value::Array)
}
fn integer(value: &Value, name: &str) -> Result<u64> {
    value[name]
        .as_u64()
        .ok_or_else(|| format!("Invalid exact integer field: {name}"))
}
fn require(valid: bool, message: &str) -> Result<()> {
    if valid {
        Ok(())
    } else {
        Err(message.to_owned())
    }
}
fn add(left: u64, right: u64) -> Result<u64> {
    left.checked_add(right)
        .ok_or_else(|| "Audit extent overflows u64.".into())
}
fn mul(left: u64, right: u64) -> Result<u64> {
    left.checked_mul(right)
        .ok_or_else(|| "Audit extent overflows u64.".into())
}
fn align(value: u64, alignment: u64) -> Result<u64> {
    mul(value.div_ceil(alignment), alignment)
}
fn contract(text: &str) -> Result<Value> {
    serde_json::from_str(text).map_err(|error| error.to_string())
}
fn publish(root: &Path, path: &str, expected: Value, emit: bool, write: bool) -> Result<Value> {
    require(!(emit && write), "Emit and write are mutually exclusive.")?;
    if write {
        if let Ok(metadata) = fs::symlink_metadata(root.join(path)) {
            require(
                metadata.is_file() && !bonsai_artifacts::is_redirect(&metadata),
                "Audit output must be an ordinary file.",
            )?;
        }
        let mut file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .open(root.join(path))
            .map_err(|error| error.to_string())?;
        serde_json::to_writer_pretty(&mut file, &expected).map_err(|error| error.to_string())?;
        file.write_all(b"\n")
            .and_then(|()| file.sync_all())
            .map_err(|error| error.to_string())?;
    } else if !emit {
        require(
            read(root, path)? == expected,
            "Retained audit differs from native source-derived report.",
        )?;
    }
    if emit {
        Ok(expected)
    } else {
        Ok(
            json!({"Valid":true,"Written":write,"DeviceOpened":false,"ModelLoaded":false,"ProfileEligible":false}),
        )
    }
}

pub(super) fn run(request: Value) -> Result<Value> {
    let request: Request = serde_json::from_value(request).map_err(|error| error.to_string())?;
    let root = &request.repository_directory;
    require(
        root.is_absolute() && root.join("tools/hexagon").is_dir(),
        "Select the expanded Runner root.",
    )?;
    match request.operation.as_str() {
        "regression" => {
            let selected = fixtures::CASES
                .iter()
                .filter(|(name, _)| request.name.is_empty() || *name == request.name)
                .collect::<Vec<_>>();
            require(
                !selected.is_empty(),
                "Unknown native Hexagon fixture selection.",
            )?;
            for (_, fixture) in &selected {
                fixture(root)?;
            }
            Ok(
                json!({"Passed":true,"Cases":selected.iter().map(|(name,_)|name).collect::<Vec<_>>(),"Count":selected.len(),"DeviceOpened":false,"ModelLoaded":false,"ProfileEligible":false}),
            )
        }
        "pq2-bridge" => pq2::report(root, &request),
        "packed-fixture" => ledgers::packed(root),
        "operation-traffic" => publish(
            root,
            TRAFFIC,
            ledgers::traffic(root)?,
            request.emit,
            request.write,
        ),
        "operation-budget" => publish(
            root,
            BUDGET,
            ledgers::budget(root)?,
            request.emit,
            request.write,
        ),
        "resident-traffic" => publish(
            root,
            RESIDENT,
            ledgers::resident(root)?,
            request.emit,
            request.write,
        ),
        "k-tiling" => publish(
            root,
            TILING,
            tiling::report(root)?,
            request.emit,
            request.write,
        ),
        _ => Err("Unknown native Hexagon audit operation.".into()),
    }
}
