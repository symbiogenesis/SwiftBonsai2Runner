//! Model-free native host and tool inventory with explicit observation sources.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::{Read as _, Seek as _, SeekFrom, Write as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use bonsai_artifacts::validate_external_path;
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    data_path: Option<PathBuf>,
    output_path: Option<PathBuf>,
    #[serde(default)]
    native_graphics: bool,
}

fn architecture(path: &Path) -> crate::Result<&'static str> {
    let mut file = fs::File::open(path).map_err(|error| error.to_string())?;
    let length = file.metadata().map_err(|error| error.to_string())?.len();
    let mut header = [0; 64];
    file.read_exact(&mut header)
        .map_err(|error| error.to_string())?;
    let offset = u64::from(u32::from_le_bytes([
        header[60], header[61], header[62], header[63],
    ]));
    if &header[..2] != b"MZ" || offset < 64 || offset.checked_add(24).is_none_or(|end| end > length)
    {
        return Err("Invalid native executable header.".into());
    }
    file.seek(SeekFrom::Start(offset))
        .map_err(|error| error.to_string())?;
    let mut signature = [0; 6];
    file.read_exact(&mut signature)
        .map_err(|error| error.to_string())?;
    if &signature[..4] != b"PE\0\0" {
        return Err("Invalid PE signature.".into());
    }
    Ok(match u16::from_le_bytes([signature[4], signature[5]]) {
        0xaa64 => "Arm64",
        0xa641 => "Arm64EC",
        0xa64e => "Arm64X",
        0x8664 => "X64",
        0x014c => "X86",
        _ => "unknown",
    })
}

fn program_directories() -> Vec<PathBuf> {
    ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"]
        .into_iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .collect()
}

fn visual_studio_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for directory in program_directories() {
        let Ok(years) = fs::read_dir(directory.join("Microsoft Visual Studio")) else {
            continue;
        };
        for year in years.take(32).filter_map(Result::ok) {
            let Ok(editions) = fs::read_dir(year.path()) else {
                continue;
            };
            for edition in editions.take(16).filter_map(Result::ok) {
                if edition.path().join("Common7").is_dir() {
                    roots.push(edition.path());
                }
            }
        }
    }
    roots
}

fn find_tool(executable: &str, visual_studio: &[PathBuf]) -> Option<PathBuf> {
    if let Ok(path) = crate::startup_process::resolve(OsStr::new(executable)) {
        return Some(path);
    }
    let relative = match executable {
        "git.exe" => "Git/cmd/git.exe",
        "cmake.exe" => "CMake/bin/cmake.exe",
        _ => "",
    };
    if !relative.is_empty()
        && let Some(path) = program_directories()
            .iter()
            .map(|root| root.join(relative))
            .find(|path| path.is_file())
    {
        return Some(path);
    }
    for root in visual_studio {
        if executable == "cmake.exe" {
            let path =
                root.join("Common7/IDE/CommonExtensions/Microsoft/CMake/CMake/bin/cmake.exe");
            if path.is_file() {
                return Some(path);
            }
        }
        if executable == "git.exe" {
            let path = root.join("Common7/IDE/CommonExtensions/Microsoft/TeamFoundation/Team Explorer/Git/cmd/git.exe");
            if path.is_file() {
                return Some(path);
            }
        }
        if executable == "cl.exe"
            && let Ok(versions) = fs::read_dir(root.join("VC/Tools/MSVC"))
        {
            let mut versions: Vec<_> = versions
                .take(32)
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .collect();
            versions.sort();
            for version in versions.into_iter().rev() {
                for lane in [
                    "Hostarm64/arm64",
                    "Hostx64/arm64",
                    "Hostx64/x64",
                    "Hostx86/x86",
                ] {
                    let path = version.join("bin").join(lane).join("cl.exe");
                    if path.is_file() {
                        return Some(path);
                    }
                }
            }
        }
    }
    None
}

fn tool(
    name: &str,
    executable: &str,
    visual_studio: &[PathBuf],
    working: &Path,
    deadline: Instant,
) -> Value {
    let Some(path) = find_tool(executable, visual_studio) else {
        return json!({"Name": name, "Present": false, "Path": null,
            "Architecture": null, "Version": null, "VersionStatus": "not-found"});
    };
    let remaining = deadline
        .saturating_duration_since(Instant::now())
        .min(Duration::from_secs(10));
    let output = if remaining.is_zero() {
        None
    } else {
        let arguments = if executable == "cl.exe" {
            Vec::new()
        } else {
            vec!["--version".into()]
        };
        crate::startup_process::run(
            path.as_os_str(),
            &arguments,
            working,
            &BTreeMap::new(),
            remaining,
            &working.join(format!("{name}.log")),
        )
        .ok()
    };
    let version = output
        .as_ref()
        .filter(|output| output.code == 0 || executable == "cl.exe")
        .and_then(|output| {
            output
                .stdout
                .lines()
                .chain(output.stderr.lines())
                .find(|line| !line.trim().is_empty())
        })
        .map(|version| version.chars().take(1024).collect::<String>());
    json!({"Name": name, "Present": true, "Path": path,
        "Architecture": architecture(&path).unwrap_or("unknown"), "Version": version,
        "VersionStatus": if version.is_some() { "reported" } else { "command-failed" }})
}

#[cfg(windows)]
fn memory() -> Value {
    bonsai_windows_process::memory_status().map_or_else(
        |_| json!({"status": "unknown", "source": "GlobalMemoryStatusEx"}),
        |memory| {
            json!({"status": "observed", "source": "GlobalMemoryStatusEx",
            "installed_physical_bytes": memory.installed_physical_bytes,
            "usable_physical_bytes": memory.total_physical_bytes,
            "available_physical_bytes": memory.available_physical_bytes,
            "effective_commit_limit_bytes": memory.commit_limit_bytes,
            "available_commit_bytes": memory.available_commit_bytes,
            "commit_scope": "current-process-or-system-limit-whichever-is-smaller"})
        },
    )
}

#[cfg(any(windows, test))]
fn driver_date(value: &str) -> Option<String> {
    let parts: Vec<_> = value.split(['-', '/']).collect();
    if parts.len() != 3 {
        return None;
    }
    let month = parts[0].parse::<u32>().ok()?;
    let day = parts[1].parse::<u32>().ok()?;
    let year = parts[2].parse::<u32>().ok()?;
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year.is_multiple_of(400) || year.is_multiple_of(4) && !year.is_multiple_of(100) => 29,
        2 => 28,
        _ => return None,
    };
    (year > 0 && year < 10_000 && day > 0 && day <= days)
        .then(|| format!("{year:04}-{month:02}-{day:02}T00:00:00.0000000Z"))
}

fn devices(class: &str) -> Value {
    #[cfg(windows)]
    if let Ok(devices) = bonsai_windows_process::device_inventory(class) {
        return json!({"Status": "observed", "Source": "SetupAPI;device driver registry metadata", "Devices": devices.iter().map(|device|
            json!({"Name": device.name, "DeviceName": device.name,
                "Manufacturer": device.manufacturer, "AdapterCompatibility": device.manufacturer,
                "DriverVersion": device.driver_version,
                "DriverDate": device.driver_date.as_deref().and_then(driver_date),
                "RegistryDriverDate": device.driver_date, "InfName": device.inf_name, "IsSigned": null}))
            .collect::<Vec<_>>()});
    }
    json!({"Status": "unknown", "Class": class, "Source": "SetupAPI;device driver registry metadata", "Devices": null})
}

fn computer() -> Value {
    #[cfg(windows)]
    if let Ok(identity) = bonsai_windows_process::computer_identity() {
        return json!({"Manufacturer": identity.manufacturer, "Model": identity.model,
            "Source": "HKLM/HARDWARE/DESCRIPTION/System/BIOS"});
    }
    Value::Null
}

#[cfg(not(windows))]
fn memory() -> Value {
    json!({"status": "unknown", "source": "GlobalMemoryStatusEx"})
}

fn capabilities(cpu: &Value, memory: &Value, device_inventory: &Value) -> crate::Result<Value> {
    let host = &cpu["host_identity"];
    let topology = &cpu["topology"];
    let version = host["windows_version"].as_array();
    let host_observed = host["status"] == "observed";
    let architecture = if host_observed {
        json!("arm64")
    } else {
        Value::Null
    };
    let version_text = version.map(|parts| format!("{}.{}.{}", parts[0], parts[1], parts[2]));
    let build = version.and_then(|parts| parts.get(2)).map(Value::to_string);
    let cpu_observed = host_observed && cpu["status"] == "observed";
    let processors = if cpu_observed {
        json!([{
            "Name": host["processor_name"], "Manufacturer": host["vendor_identifier"],
            "NumberOfCores": topology["physical_cores"], "NumberOfLogicalProcessors": topology["logical_processors"]
        }])
    } else {
        json!([])
    };
    let mut projected = crate::host_capabilities::run(&json!({"operation": "project", "facts": {
        "OsArchitecture": architecture, "ProcessArchitecture": std::env::consts::ARCH.replace("aarch64", "arm64"),
        "OperatingSystem": {"Version": version_text, "BuildNumber": build,
            "TotalVisibleMemorySize": memory["usable_physical_bytes"].as_u64().map(|bytes| bytes / 1024)},
        "Processors": processors, "Graphics": device_inventory["Display"]["Devices"],
        "NpuDrivers": device_inventory["ComputeAccelerator"]["Devices"],
        "Computer": {"TotalPhysicalMemory": memory["installed_physical_bytes"]},
        "Memory": {"AvailableBytes": memory["available_physical_bytes"]},
        "Isa": {"AdvSimd": cpu["isa"]["advsimd"], "DotProd": cpu["isa"]["dotprod"]},
        "Probes": {"OperatingSystem": host_observed, "Computer": !memory["installed_physical_bytes"].is_null(),
            "Processors": cpu_observed, "Graphics": device_inventory["Display"]["Status"] == "observed",
            "NpuDrivers": device_inventory["ComputeAccelerator"]["Status"] == "observed",
            "Memory": memory["status"] == "observed", "Isa": cpu["isa"]["status"] == "observed"}
    }}))?;
    projected["operating_system"]["source"] = json!("RtlGetVersion;native-process-architecture");
    projected["process"]["source"] = json!("Rust compiled target architecture");
    projected["cpu"]["source"] = json!("CPU registry identity;GetSystemCpuSetInformation");
    projected["cpu"]["isa"]["source"] = json!("IsProcessorFeaturePresent");
    projected["memory"]["source"] = json!(
        "GlobalMemoryStatusEx;GetPhysicallyInstalledSystemMemory;system-wide committed usage unavailable"
    );
    projected["gpu"]["source"] = json!("SetupAPI;device driver registry metadata");
    projected["npu"]["source"] =
        json!("SetupAPI;device driver registry metadata;signature unavailable");
    Ok(projected)
}

/// Collect native CPU/memory observations and bounded tool versions without setup.
///
/// `nativeGraphics` explicitly requests the separately owned Vulkan/OpenCL probes.
/// Unavailable manufacturer, driver-signature and physical-memory observations stay
/// unknown. Optional report publication uses create-new external storage.
///
/// # Errors
/// Rejects malformed requests, unsafe storage, existing outputs, failed report
/// publication and unsupported capability observations. Never loads a model.
pub fn run(value: Value) -> crate::Result<Value> {
    let request: Request = serde_json::from_value(value).map_err(|error| error.to_string())?;
    if let Some(data) = &request.data_path {
        validate_external_path(data).map_err(|error| error.to_string())?;
        if !data.is_dir() {
            return Err("DataPath must name an existing external directory.".into());
        }
    }
    if let Some(output) = &request.output_path {
        validate_external_path(output).map_err(|error| error.to_string())?;
        if output.exists() || output.parent().is_none_or(|parent| !parent.is_dir()) {
            return Err("OutputPath must name a new external file with an existing parent.".into());
        }
    }
    let started = Instant::now();
    let deadline = started + Duration::from_secs(90);
    let working = std::env::temp_dir().join(format!(
        "Bonsai-Doctor-{}",
        crate::startup_process::unique()
    ));
    validate_external_path(&working).map_err(|error| error.to_string())?;
    fs::create_dir(&working).map_err(|error| error.to_string())?;
    let cpu = crate::host_cpu::run(&json!({}))?;
    let memory = memory();
    let device_inventory =
        json!({"Display": devices("Display"), "ComputeAccelerator": devices("ComputeAccelerator")});
    let capabilities = capabilities(&cpu, &memory, &device_inventory)?;
    let identity = crate::host_capabilities::run(
        &json!({"operation": "identity", "capabilities": capabilities}),
    )?;
    let visual_studio = visual_studio_roots();
    let tools: Vec<_> = [
        ("Git", "git.exe"),
        ("CMake", "cmake.exe"),
        ("MSVC", "cl.exe"),
        ("Clang", "clang.exe"),
        ("Rust", "rustc.exe"),
        ("Cargo", "cargo.exe"),
        ("PowerShell", "pwsh.exe"),
    ]
    .into_iter()
    .map(|(name, executable)| tool(name, executable, &visual_studio, &working, deadline))
    .collect();
    let gpu = if request.native_graphics {
        crate::host_gpu::run(&json!({}))?
    } else {
        Value::Null
    };
    let opencl = if request.native_graphics {
        crate::host_opencl::run(&json!({}))?
    } else {
        Value::Null
    };
    #[cfg(windows)]
    let available_disk = request
        .data_path
        .as_ref()
        .and_then(|path| bonsai_windows_process::available_disk_bytes(path).ok());
    #[cfg(not(windows))]
    let available_disk: Option<u64> = None;
    let mut gaps = vec![
        "Device/driver names are observations; driver signatures, system-wide committed usage and backend readiness remain unverified.",
    ];
    if tools
        .iter()
        .any(|tool| tool["Present"] != true || tool["VersionStatus"] != "reported")
    {
        gaps.push("Some tools or versions are unavailable; no dependency was installed.");
    }
    if !request.native_graphics {
        gaps.push("Native graphics probes were not selected.");
    }
    let report = json!({"SchemaVersion": 2, "Kind": "runner-native-doctor", "Capabilities": capabilities,
        "OperatingSystem": capabilities["operating_system"], "Runtime": {
            "ProcessArchitecture": capabilities["process"]["architecture"],
            "IsNativeProcess": capabilities["process"]["is_native"], "Implementation": "native Rust"},
        "Processors": capabilities["cpu"]["identities"], "Graphics": capabilities["gpu"]["devices"],
        "NativeCpuObservations": cpu, "NativeGpuObservations": gpu, "NativeOpenClObservations": opencl,
        "Memory": memory, "Computer": computer(), "NativeDeviceObservations": device_inventory,
        "CpuCompatibilityIdentity": identity, "Tools": tools,
        "Storage": {"Selected": request.data_path.is_some(), "Path": request.data_path, "AvailableBytes": available_disk},
        "EvidenceDirectory": working, "ElapsedMilliseconds": started.elapsed().as_millis(), "Gaps": gaps,
        "Qualification": "Inventory only; engine correctness, backend readiness, performance and daily security remain unqualified."});
    if let Some(path) = request.output_path {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|error| error.to_string())?;
        serde_json::to_writer_pretty(&mut file, &report).map_err(|error| error.to_string())?;
        file.write_all(b"\n").map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_driver_dates_reject_invalid_calendar_values_and_keep_utc_transport_form() {
        assert_eq!(
            driver_date("2-29-2024").as_deref(),
            Some("2024-02-29T00:00:00.0000000Z")
        );
        for invalid in [
            "2-29-2025",
            "13-1-2026",
            "0-0-0",
            "1-1-10000",
            "2026-01-01",
            "arbitrary",
        ] {
            assert!(driver_date(invalid).is_none());
        }
    }

    #[test]
    fn doctor_does_not_invent_cpu_or_commit_identity_from_missing_observations() -> crate::Result<()>
    {
        let result = capabilities(
            &json!({"status": "unknown"}),
            &json!({"status": "unknown"}),
            &json!({}),
        )?;
        assert_eq!(result["cpu"]["status"], "unknown");
        assert!(
            result["cpu"]["identities"]
                .as_array()
                .is_some_and(Vec::is_empty)
        );
        assert!(result["memory"]["committed_bytes"].is_null());
        assert_eq!(result["npu"]["status"], "unknown");
        Ok(())
    }

    #[test]
    fn doctor_native_identity_preserves_cpu_counts_and_known_sources() -> crate::Result<()> {
        let result = capabilities(
            &json!({"status": "observed", "host_identity": {
            "status": "observed", "processor_name": "Snapdragon X2", "vendor_identifier": "Qualcomm",
            "windows_version": [10, 0, 26300]}, "topology": {"physical_cores": 18, "logical_processors": 18},
            "isa": {"status": "observed", "advsimd": true, "dotprod": true}}),
            &json!({"status": "observed", "usable_physical_bytes": 68719476736_u64, "available_physical_bytes": 42949672960_u64}),
            &json!({}),
        )?;
        assert_eq!(result["cpu"]["identities"][0]["physical_cores"], 18);
        assert_eq!(result["operating_system"]["version"], "10.0.26300");
        assert_eq!(result["memory"]["usable_physical_bytes"], 68719476736_u64);
        assert!(result["memory"]["reported_total_physical_bytes"].is_null());
        Ok(())
    }

    #[test]
    fn doctor_rejects_existing_report_before_any_probe() -> crate::Result<()> {
        let root = std::env::temp_dir().join(format!(
            "Bonsai-DoctorOutput-{}",
            crate::startup_process::unique()
        ));
        fs::create_dir(&root).map_err(|error| error.to_string())?;
        let path = root.join("report.json");
        fs::write(&path, b"existing report").map_err(|error| error.to_string())?;
        assert!(run(json!({"outputPath": path})).is_err());
        assert_eq!(
            fs::read(&path).map_err(|error| error.to_string())?,
            b"existing report"
        );
        fs::remove_file(path).map_err(|error| error.to_string())?;
        fs::remove_dir(root).map_err(|error| error.to_string())
    }
}
