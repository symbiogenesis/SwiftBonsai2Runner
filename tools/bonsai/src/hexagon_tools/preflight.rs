//! Model-free native device and bounded top-level runtime inventory.

use super::{plain, write_json};
use crate::{Result, startup_process};
use bonsai_artifacts::validate_external_path;
#[cfg(any(windows, test))]
use bonsai_artifacts::{hash_reader, is_redirect};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
#[cfg(any(windows, test))]
use std::fs::{File, OpenOptions};
#[cfg(any(windows, test))]
use std::io::Seek as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[cfg(any(windows, test))]
fn eligible(path: &Path) -> bool {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if ["dll", "exe", "so", "cat", "inf"].contains(&extension.as_str()) {
        return true;
    }
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_uppercase();
    let (stem, suffix) = name
        .split_once('.')
        .map_or((name.as_str(), None), |(stem, suffix)| (stem, Some(suffix)));
    ["LICENSE", "NOTICE"].contains(&stem)
        && suffix.is_none_or(|suffix| {
            !suffix.is_empty()
                && suffix
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
        })
}
#[cfg(any(windows, test))]
fn signature(file: &File, path: &Path) -> Value {
    #[cfg(windows)]
    if let Ok(signer) = bonsai_windows_process::verify_authenticode(file, path) {
        return json!({"Status":"Valid","SignerThumbprint":signer.thumbprint,"Scope":if path.extension().is_some_and(|extension| extension.to_str().is_some_and(|extension| extension.eq_ignore_ascii_case("cat"))) {"catalog-signature-only; member binding unverified"} else {"file-signature-observation-only"}});
    }
    #[cfg(not(windows))]
    let _ = (file, path);
    json!({"Status":"Unverified","SignerThumbprint":null,"Scope":"signature-observation-only; trust and member binding unknown"})
}
#[cfg(any(windows, test))]
fn package(path: Option<&Path>) -> Result<Value> {
    let Some(root) = path else {
        return Ok(json!({"Status":"not-selected","Files":[],"TotalBytes":0}));
    };
    plain(root)?;
    if !root.is_dir() {
        return Err("Select an existing runtime package directory.".into());
    }
    let mut files = Vec::new();
    let mut total = 0_u64;
    for (index, entry) in fs::read_dir(root)
        .map_err(|error| error.to_string())?
        .enumerate()
    {
        if index >= 128 {
            return Err("Package inventory exceeds 128 top-level entries.".into());
        }
        let path = entry.map_err(|error| error.to_string())?.path();
        let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        if is_redirect(&metadata) {
            return Err("Package inventory contains a redirect.".into());
        }
        if metadata.is_dir() || !eligible(&path) {
            continue;
        }
        if files.len() >= 64 || !metadata.is_file() {
            return Err("Package inventory exceeds its file bound.".into());
        }
        let bytes = metadata.len();
        total = total.checked_add(bytes).ok_or("Package length overflow.")?;
        if bytes > 128 * 1024 * 1024 || total > 256 * 1024 * 1024 {
            return Err("Package hashing exceeds its per-file or aggregate byte bound.".into());
        }
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt as _;
            options.share_mode(1).custom_flags(0x0020_0000);
        }
        let mut file = options.open(&path).map_err(|error| error.to_string())?;
        let digest = hash_reader(&mut file, bytes, &mut vec![0; 65_536], &mut || Ok(()))
            .map_err(|error| error.to_string())?;
        file.rewind().map_err(|error| error.to_string())?;
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let signed = if ["dll", "exe", "cat"].contains(&extension.as_str()) {
            signature(&file, &path)
        } else {
            Value::Null
        };
        files.push(json!({"Name":path.file_name().map(|name| name.to_string_lossy()),"Bytes":bytes,"Sha256":digest,"Signature":signed}));
    }
    Ok(json!({"Status":"inventoried","Files":files,"TotalBytes":total}))
}

pub(super) fn inventory(path: Option<&Path>) -> Result<Value> {
    if !cfg!(windows) {
        return Err("Hexagon inventory requires Windows.".into());
    }
    #[cfg(windows)]
    {
        let host =
            bonsai_windows_process::native_build_host().map_err(|error| error.to_string())?;
        let drivers=bonsai_windows_process::device_inventory("ComputeAccelerator").map_err(|error| error.to_string())?.into_iter().map(|driver| json!({"DeviceName":driver.name,"Manufacturer":driver.manufacturer,"DriverVersion":driver.driver_version,"DriverDate":null,"DriverDateObservation":driver.driver_date,"IsSigned":null,"InfName":driver.inf_name,"DeviceID":driver.device_id,"Source":"SetupAPI-present-device-and-driver-registry; signature-unknown"})).collect::<Vec<_>>();
        Ok(
            json!({"OsArchitecture":if cfg!(target_arch="aarch64") {"Arm64"} else {"unknown"},"ProcessArchitecture":if cfg!(target_arch="aarch64") {"Arm64"} else {"unknown"},"Processors":[{"Name":host.processor_name,"Manufacturer":host.vendor_identifier,"Architecture":if cfg!(target_arch="aarch64") {12} else {-1}}],"Drivers":drivers,"Package":package(path)?}),
        )
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err("Hexagon inventory requires Windows.".into())
    }
}

fn receipt(inventory: Option<&Value>, status: &str, maximum: u32, elapsed: u128) -> Value {
    let mut blockers = Vec::new();
    match inventory {
        None => {
            blockers.push("Inventory unavailable; device and deployment identity remain unknown.");
        }
        Some(value) => {
            if value["OsArchitecture"] != "Arm64" || value["ProcessArchitecture"] != "Arm64" {
                blockers.push("Native Windows ARM64 execution has not been observed.");
            }
            if value["Drivers"].as_array().is_none_or(Vec::is_empty) {
                blockers.push("No ComputeAccelerator driver was observed.");
            }
            if value["Package"]["Status"] != "inventoried" {
                blockers.push("No explicit runtime package directory was selected.");
            } else if value["Package"]["Files"]
                .as_array()
                .is_none_or(Vec::is_empty)
            {
                blockers.push("The selected directory contained no eligible runtime, DSP, catalog or notice files.");
            }
        }
    }
    blockers.extend(["Exact source/license/package ABI and DSP catalog membership/installed trust require independent verification.","Device mapping and supported-type dispatch have not executed; DMA64 and effective VTCM remain unknown.","Swift packed arithmetic, full state, lifecycle, resource and paired benefit gates remain pending."]);
    #[cfg(windows)]
    let utc = bonsai_windows_process::utc_now();
    #[cfg(not(windows))]
    let utc = Value::Null;
    json!({"SchemaVersion":1,"Kind":"runner-hexagon-preflight","CollectedAtUtc":utc,"CollectionStatus":status,"MaximumSeconds":maximum,"ElapsedMilliseconds":elapsed,"Inventory":inventory,"Execution":"not-attempted","Qualification":"pending","ProfileEligible":false,"TrustChanged":false,"DriverInstalled":false,"ModelLoaded":false,"Blockers":blockers})
}

pub(super) fn supervise(
    package_path: Option<&Path>,
    output_path: Option<PathBuf>,
    maximum: u32,
) -> Result<Value> {
    let clock = Instant::now();
    if !cfg!(windows) || !(5..=300).contains(&maximum) {
        return Err(
            "Hexagon preflight requires Windows and a 5..300 second remaining allowance.".into(),
        );
    }
    if let Some(path) = package_path {
        plain(path)?;
        if !path.is_dir() {
            return Err("Select an existing package directory.".into());
        }
    }
    if let Some(path) = &output_path {
        validate_external_path(path).map_err(|error| error.to_string())?;
        if path.exists() || path.parent().is_none_or(|parent| !parent.is_dir()) {
            return Err("Receipt must be new external storage with an existing parent.".into());
        }
    }
    let scratch = std::env::temp_dir().join(format!(
        "bonsai-hexagon-preflight-{}",
        startup_process::unique()
    ));
    validate_external_path(&scratch).map_err(|error| error.to_string())?;
    fs::create_dir(&scratch).map_err(|error| error.to_string())?;
    let request = scratch.join("request.json");
    write_json(
        &request,
        &json!({"operation":"preflight-worker","packagePath":package_path}),
    )?;
    // Native process cleanup has a five-second hard bound; reserve another
    // second for receipt publication. A smaller remaining allowance stays pending.
    let allowance = Duration::from_secs(u64::from(maximum))
        .saturating_sub(clock.elapsed())
        .saturating_sub(Duration::from_secs(6));
    let (inventory, status) = if allowance < Duration::from_millis(1) {
        (None, "deadline-expired")
    } else {
        let current = std::env::current_exe().map_err(|error| error.to_string())?;
        match startup_process::run(
            current.as_os_str(),
            &[
                "hexagon-tools".into(),
                "--request".into(),
                request.into_os_string(),
            ],
            &scratch,
            &BTreeMap::new(),
            allowance,
            &scratch.join("worker.txt"),
        ) {
            Ok(result)
                if result.code == 0
                    && result.stdout.len() <= 262_144
                    && result.stderr.is_empty() =>
            {
                crate::hexagon_policy::run(json!({"operation":"json","json":result.stdout}))
                    .map_or((None, "collection-failed"), |inventory| {
                        (Some(inventory), "collected")
                    })
            }
            Err(_) if clock.elapsed() >= allowance => (None, "deadline-expired"),
            _ => (None, "collection-failed"),
        }
    };
    let report = receipt(
        inventory.as_ref(),
        status,
        maximum,
        clock.elapsed().as_millis(),
    );
    if let Some(path) = output_path {
        write_json(&path, &report)?;
    }
    // Only this newly created, validated scratch directory is removed.
    let _ = fs::remove_dir_all(scratch);
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inventory_never_grants_execution_or_deployment() {
        let unknown = receipt(None, "deadline-expired", 5, 3000);
        assert_eq!(unknown["Inventory"], Value::Null);
        assert_eq!(unknown["ProfileEligible"], false);
        assert_eq!(unknown["Execution"], "not-attempted");
        assert_eq!(unknown["Blockers"].as_array().map(Vec::len), Some(4));
        let facts = json!({"OsArchitecture":"Arm64","ProcessArchitecture":"Arm64","Drivers":[{"IsSigned":true}],"Package":{"Status":"inventoried","Files":[{"Name":"libggml-htp-v81.so","Signature":{"Status":"Valid"}}]}});
        let observed = receipt(Some(&facts), "collected", 5, 1);
        assert_eq!(observed["ProfileEligible"], false);
        assert_eq!(observed["Blockers"].as_array().map(Vec::len), Some(3));
    }
    #[test]
    fn package_filters_models_and_nested_entries() -> Result<()> {
        let scratch = std::env::temp_dir().join(format!(
            "bonsai-hexagon-inventory-test-{}",
            startup_process::unique()
        ));
        fs::create_dir(&scratch).map_err(|error| error.to_string())?;
        fs::write(scratch.join("libggml-htp-v81.so"), [1, 2, 3])
            .map_err(|error| error.to_string())?;
        fs::write(scratch.join("model.gguf"), b"must-not-hash")
            .map_err(|error| error.to_string())?;
        fs::create_dir(scratch.join("nested")).map_err(|error| error.to_string())?;
        fs::write(scratch.join("nested/hidden.so"), [4]).map_err(|error| error.to_string())?;
        let report = package(Some(&scratch))?;
        assert_eq!(report["TotalBytes"], 3);
        assert_eq!(report["Files"].as_array().map(Vec::len), Some(1));
        assert_eq!(
            report["Files"][0]["Sha256"],
            "039058c6f2c0cb492c533b0a4d14ef77cc0f78abccced5287d84a1a2011cfb81"
        );
        assert_eq!(report["Files"][0]["Signature"], Value::Null);
        for index in 0..129 {
            fs::write(scratch.join(format!("ignored-{index}")), [])
                .map_err(|error| error.to_string())?;
        }
        assert!(package(Some(&scratch)).is_err());
        fs::remove_dir_all(&scratch).map_err(|error| error.to_string())?;
        Ok(())
    }
}
