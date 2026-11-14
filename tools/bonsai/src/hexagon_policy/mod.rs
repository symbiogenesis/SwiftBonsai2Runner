//! Offline Hexagon evidence policy; validation never grants device admission.

use serde::Deserialize;
use serde_json::{Value, json};

use crate::Result;
use crate::identity::is_lower_hex;

mod dispatch;
mod json;
mod session;
#[cfg(test)]
mod tests;

// Build receipts and their verifier share one compiler/source inventory contract.
pub(super) const TOOLCHAIN: &str = "nightly-2026-11-12-aarch64-pc-windows-msvc";
pub(super) const SOURCE_NAMES: &[&str] = &[
    "lib.rs",
    "windows.rs",
    "mapping.rs",
    "session.rs",
    "session_native.rs",
    "session_mapping.rs",
    "session_mapping_native.rs",
    "compute.rs",
    "dispatch.rs",
    "compute_native.rs",
    "projection.rs",
    "projection_native.rs",
    "q8.rs",
    "q8_dispatch.rs",
    "q8_dispatch_tests.rs",
    "q8_native.rs",
    "q8_native_tests.rs",
    "q8_oracle.rs",
    "q8_oracle_tests.rs",
    "q8_projection.rs",
    "query.rs",
    "sources.json",
];

#[derive(Deserialize)]
#[serde(
    tag = "operation",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum Request {
    DispatchPlan {
        maximum_seconds: u32,
    },
    SessionPlan {
        maximum_seconds: u32,
        reference_initialization: bool,
    },
    ComputePlan {
        maximum_seconds: u32,
        case: String,
    },
    Environment {
        runtime_directory: String,
        system_directory: String,
    },
    Json {
        json: String,
    },
    DriverIdentity {
        json: String,
    },
    DispatchBuild {
        json: String,
        expected_runner_revision: String,
        source_sha256: String,
        manifest_sha256: String,
        lock_sha256: String,
    },
    SessionBuild {
        json: String,
        expected_runner_revision: String,
        source_digests: Value,
        manifest_sha256: String,
        lock_sha256: String,
    },
    DispatchResult {
        json: String,
        exit_code: i32,
    },
    DispatchWorker {
        json: String,
        expected_runner_revision: String,
    },
    SessionResult {
        json: String,
        exit_code: i32,
        reference_initialization: bool,
    },
    SessionWorker {
        json: String,
        expected_runner_revision: String,
        reference_initialization: bool,
    },
    ComputeResult {
        json: String,
        exit_code: i32,
        case: String,
    },
}

struct Record<'a>(&'a Value);

impl<'a> Record<'a> {
    fn get(&self, name: &str) -> Result<&'a Value> {
        self.0
            .get(name)
            .ok_or_else(|| format!("Missing Hexagon evidence field: {name}"))
    }
    fn string(&self, name: &str) -> Result<&'a str> {
        self.get(name)?
            .as_str()
            .ok_or_else(|| format!("Hexagon evidence requires a string: {name}"))
    }
    fn boolean(&self, name: &str) -> Result<bool> {
        self.get(name)?
            .as_bool()
            .ok_or_else(|| format!("Hexagon evidence requires a boolean: {name}"))
    }
    fn integer(&self, name: &str) -> Result<i64> {
        self.get(name)?
            .as_i64()
            .ok_or_else(|| format!("Hexagon evidence requires an exact signed integer: {name}"))
    }
    fn optional_integer(&self, name: &str) -> Result<Option<i64>> {
        if self.get(name)?.is_null() {
            Ok(None)
        } else {
            self.integer(name).map(Some)
        }
    }
    fn array(&self, name: &str) -> Result<&'a [Value]> {
        self.get(name)?
            .as_array()
            .map(Vec::as_slice)
            .ok_or_else(|| format!("Hexagon evidence requires an array: {name}"))
    }
    fn equal(&self, name: &str, expected: &str) -> Result<()> {
        require(
            self.string(name)? == expected,
            "Hexagon evidence string identity changed.",
        )
    }
    fn fixed(&self, name: &str, expected: i64) -> Result<()> {
        require(
            self.integer(name)? == expected,
            "Hexagon evidence fixed integer changed.",
        )
    }
    fn flags(&self, names: &[&str], expected: bool) -> Result<()> {
        for name in names {
            require(
                self.boolean(name)? == expected,
                "Hexagon evidence claimed an incomplete or independent gate.",
            )?;
        }
        Ok(())
    }
    fn label(&self, name: &str, maximum: usize) -> Result<()> {
        require(
            label(self.string(name)?, maximum),
            "Invalid Hexagon evidence stage or failure label.",
        )
    }
    fn optional_label(&self, name: &str, maximum: usize) -> Result<()> {
        if !self.get(name)?.is_null() {
            self.label(name, maximum)?;
        }
        Ok(())
    }
    fn digest(&self, name: &str) -> Result<()> {
        require(
            is_lower_hex(self.string(name)?, 64),
            "Invalid Hexagon source digest.",
        )
    }
    fn optional_digests(&self, names: &[&str], required: bool) -> Result<()> {
        for name in names {
            if self.get(name)?.is_null() {
                require(
                    !required,
                    "Completed Hexagon evidence omitted its source binding.",
                )?;
            } else {
                self.digest(name)?;
            }
        }
        Ok(())
    }
}

fn require(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}
fn label(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && value.as_bytes()[0].is_ascii_lowercase()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}
fn windows_absolute(value: &str) -> bool {
    let bytes = value.as_bytes();
    (bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\'))
        || value.starts_with("\\\\")
}
fn driver_path(value: &str) -> bool {
    windows_absolute(value)
        && value
            .rsplit(['/', '\\'])
            .next()
            .is_some_and(|name| name.eq_ignore_ascii_case("libcdsprpc.dll"))
}

fn driver_identity(value: &Value) -> Result<()> {
    let r = Record(value);
    require(
        driver_path(r.string("Path")?),
        "Invalid FastRPC driver path.",
    )?;
    require(
        (1..=32 * 1024 * 1024).contains(&r.integer("Bytes")?),
        "Invalid FastRPC driver byte count.",
    )?;
    r.digest("Sha256")?;
    r.equal("SignatureStatus", "Valid")?;
    r.equal(
        "Publisher",
        "Microsoft Windows Hardware Compatibility Publisher",
    )?;
    let thumbprint = r.string("SignerThumbprint")?;
    require(
        (40..=64).contains(&thumbprint.len()) && is_lower_hex(thumbprint, thumbprint.len()),
        "Invalid FastRPC signer thumbprint.",
    )?;
    r.flags(&["FileHeldAgainstMutation"], true)
}

fn build(
    value: &Value,
    revision: &str,
    manifest: &str,
    lock: &str,
    executable: &str,
) -> Result<()> {
    require(
        is_lower_hex(revision, 40) && is_lower_hex(manifest, 64) && is_lower_hex(lock, 64),
        "Invalid Hexagon build source binding.",
    )?;
    let r = Record(value);
    r.fixed("SchemaVersion", 1)?;
    r.equal("RunnerRevision", revision)?;
    r.equal("RustToolchain", TOOLCHAIN)?;
    r.equal("Target", "aarch64-pc-windows-msvc")?;
    r.equal("ManifestSha256", manifest)?;
    r.equal("LockSha256", lock)?;
    r.digest("CompilerIdentitySha256")?;
    r.flags(&["HostedBuild"], true)?;
    let exe = Record(r.get("Executable")?);
    exe.equal("Name", executable)?;
    require(
        (1..=16 * 1024 * 1024).contains(&exe.integer("Bytes")?),
        "Invalid Hexagon executable size.",
    )?;
    exe.digest("Sha256")
}

fn session_build(
    value: &Value,
    revision: &str,
    sources: &Value,
    manifest: &str,
    lock: &str,
) -> Result<()> {
    build(value, revision, manifest, lock, "native-hexagon-query.exe")?;
    let sources = sources
        .as_object()
        .ok_or("Invalid Hexagon source digest set.")?;
    require(
        sources.len() == SOURCE_NAMES.len()
            && sources.values().all(|value| {
                value
                    .as_str()
                    .is_some_and(|digest| is_lower_hex(digest, 64))
            }),
        "Invalid Hexagon source digest set.",
    )?;
    let r = Record(value);
    for (name, maximum) in [("RunId", 32), ("RunAttempt", 8)] {
        let value = r.string(name)?;
        require(
            !value.is_empty()
                && value.len() <= maximum
                && value.bytes().all(|byte| byte.is_ascii_digit()),
            "Invalid hosted Hexagon run identity.",
        )?;
    }
    r.flags(
        &["DeviceCodeLoaded", "DspCodeLoaded", "ProfileEligible"],
        false,
    )?;
    let files = r.array("SourceFiles")?;
    require(
        files.len() == SOURCE_NAMES.len(),
        "Hexagon source count changed.",
    )?;
    let mut seen = std::collections::BTreeSet::new();
    for entry in files {
        let entry = Record(entry);
        let name = entry.string("Name")?;
        require(
            SOURCE_NAMES.contains(&name)
                && seen.insert(name)
                && sources.get(name).and_then(Value::as_str) == Some(entry.string("Sha256")?),
            "Hexagon source set or digest changed.",
        )?;
    }
    Ok(())
}

fn plan(operation: &str, maximum: u32, reference: bool, case: &str) -> Result<Value> {
    require(
        (1..=240).contains(&maximum),
        "Hexagon plan allowance must be between 1 and 240 seconds.",
    )?;
    match operation {
        "dispatch" => Ok(
            json!({"SchemaVersion":1,"Kind":"runner-hexagon-dispatch-plan","MaximumSeconds":maximum,"CleanupReserveSeconds":75,"ModelLoaded":false,"CpuFallbackAllowed":false,"OwnedJobRequired":true,"CatalogMemberVerificationRequired":true,"SourceBoundHostedFixtureRequired":true,"AsInvokerManifestRequired":true,"DriverInstallationAllowed":false,"Cases":[{"Columns":128,"Rows":32,"Batch":1},{"Columns":128,"Rows":32,"Batch":32}],"Qualification":"device-access-only","ProfileEligible":false}),
        ),
        "session" => Ok(
            json!({"SchemaVersion":1,"Kind":"runner-hexagon-session-plan","MaximumSeconds":maximum,"CleanupReserveSeconds":75,"Bytes":4096,"MapFlags":2,"SessionId":0,"OwnedJobRequired":true,"SourceBoundHostedFixtureRequired":true,"CatalogMemberVerificationRequired":true,"IndependentDriverIdentityRequired":true,"DriverInstallationAllowed":false,"TrustChangesAllowed":false,"ComputeExecuted":false,"ModelLoaded":false,"ProfileEligible":false,"Qualification":"native-signed-session-FD-mapping-only","ReferenceProcessInitializationSelected":reference,"ReferenceProcessInitializationControl":if reference {"DSPRPC_CONTROL_UNSIGNED_MODULE-enable1-default-session-effective-domain-process-only"} else {"not-selected"},"AutomaticFallbackAllowed":false,"AutomaticRetryAllowed":false}),
        ),
        "compute" => {
            compute_kind(case)?;
            Ok(
                json!({"SchemaVersion":1,"Kind":"runner-hexagon-compute-plan","MaximumSeconds":maximum,"CleanupReserveSeconds":75,"QueueSlots":1,"Operations":1,"Package":"GenieX-v0.10.0","Columns":if case.starts_with("GuardedQ2") {256} else {32},"Rows":32,"Batch":2,"ComputeCase":case,"ModelLoaded":false,"ProfileEligible":false,"OwnedJobRequired":true,"IndependentWindowsAbiRequired":true,"CatalogVerificationRequired":true,"SourceBoundHostedFixtureRequired":true,"TrustChangesAllowed":false,"AutomaticRetryAllowed":false}),
            )
        }
        _ => Err("Unknown Hexagon plan.".into()),
    }
}

fn compute_kind(case: &str) -> Result<&'static str> {
    match case {
        "F32" => Ok("native-fastrpc-queue-compute-probe"),
        "GuardedQ2" => Ok("native-fastrpc-queue-q2-probe"),
        "F32NoBlock" => Ok("native-fastrpc-queue-noblock-probe"),
        "GuardedQ2NoBlock" => Ok("native-fastrpc-queue-q2-noblock-probe"),
        _ => Err("Select one fixed Hexagon compute case.".into()),
    }
}

fn dispatch_build(
    json: &str,
    revision: &str,
    source_digest: &str,
    manifest_digest: &str,
    lock_digest: &str,
) -> Result<Value> {
    let value = json::parse(json)?;
    build(
        &value,
        revision,
        manifest_digest,
        lock_digest,
        "hexagon-dispatch-fixture.exe",
    )?;
    require(
        is_lower_hex(source_digest, 64),
        "Invalid dispatch source identity.",
    )?;
    Record(&value).equal("SourceSha256", source_digest)?;
    Ok(json!({"Valid":true}))
}

/// Validate bounded native evidence or return a fixed model-free plan.
///
/// # Errors
/// Rejects ambiguous JSON, coerced fields, incomplete source bindings and
/// contradictory state or qualification claims. This never opens a device.
pub fn run(request: Value) -> Result<Value> {
    let request: Request = serde_json::from_value(request).map_err(|error| error.to_string())?;
    match request {
        Request::DispatchPlan { maximum_seconds } => plan("dispatch", maximum_seconds, false, ""),
        Request::SessionPlan {
            maximum_seconds,
            reference_initialization,
        } => plan("session", maximum_seconds, reference_initialization, ""),
        Request::ComputePlan {
            maximum_seconds,
            case,
        } => plan("compute", maximum_seconds, false, &case),
        Request::Environment {
            runtime_directory,
            system_directory,
        } => Ok(
            json!({"RemovePrefixes":["GGML_","GENIEX_","QNN_","QAIRT_","HEXAGON_","ADSP_","CDSP_","FASTRPC_","RPCMEM_"],"RemoveNames":["ACTIONS_RUNTIME_TOKEN","ACTIONS_ID_TOKEN_REQUEST_TOKEN","ACTIONS_ID_TOKEN_REQUEST_URL","GITHUB_TOKEN","GH_TOKEN","ACTIONS_RUNNER_INPUT_TOKEN","LD_LIBRARY_PATH","LD_PRELOAD"],"Set":{"PATH":format!("{runtime_directory};{system_directory}"),"ADSP_LIBRARY_PATH":runtime_directory}}),
        ),
        Request::Json { json } => json::parse(&json),
        Request::DriverIdentity { json } => {
            driver_identity(&json::parse(&json)?)?;
            Ok(json!({"Valid":true}))
        }
        Request::DispatchBuild {
            json,
            expected_runner_revision,
            source_sha256,
            manifest_sha256,
            lock_sha256,
        } => dispatch_build(
            &json,
            &expected_runner_revision,
            &source_sha256,
            &manifest_sha256,
            &lock_sha256,
        ),
        Request::SessionBuild {
            json,
            expected_runner_revision,
            source_digests,
            manifest_sha256,
            lock_sha256,
        } => {
            session_build(
                &json::parse(&json)?,
                &expected_runner_revision,
                &source_digests,
                &manifest_sha256,
                &lock_sha256,
            )?;
            Ok(json!({"Valid":true}))
        }
        Request::DispatchResult { json, exit_code } => {
            let value = json::parse(&json)?;
            dispatch::fixture(&value, exit_code)?;
            Ok(value)
        }
        Request::DispatchWorker {
            json,
            expected_runner_revision,
        } => {
            let value = json::parse(&json)?;
            dispatch::worker(&value, &expected_runner_revision)?;
            Ok(value)
        }
        Request::SessionResult {
            json,
            exit_code,
            reference_initialization,
        } => {
            let value = json::parse(&json)?;
            session::probe(&value, exit_code, reference_initialization)?;
            Ok(value)
        }
        Request::SessionWorker {
            json,
            expected_runner_revision,
            reference_initialization,
        } => {
            let value = json::parse(&json)?;
            session::worker(&value, &expected_runner_revision, reference_initialization)?;
            Ok(value)
        }
        Request::ComputeResult {
            json,
            exit_code,
            case,
        } => {
            let value = json::parse(&json)?;
            dispatch::compute(&value, exit_code, &case)?;
            Ok(value)
        }
    }
}
