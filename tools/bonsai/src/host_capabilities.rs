//! Pure host-inventory projection and version-one CPU observation identities.

use serde::Serialize;
use serde_json::{Value, json};

use crate::Result;

const SCOPE: &str = "native-windows-arm64-cpu-observations-v1";

/// Projects supplied facts or derives an offline CPU identity without probing the host.
///
/// # Errors
/// Rejects invalid fact types, oversized inventories and unsupported input schemas.
pub fn run(request: &Value) -> Result<Value> {
    let object = request
        .as_object()
        .ok_or("Capability request must be an object.")?;
    match request["operation"].as_str() {
        Some("project") if object.len() == 2 && object.contains_key("facts") => {
            project(&request["facts"])
        }
        Some("identity") if object.len() == 2 && object.contains_key("capabilities") => {
            identity(&request["capabilities"])
        }
        _ => Err("Unsupported capability operation or request fields.".into()),
    }
}

fn text(value: &Value) -> Result<Option<&str>> {
    if value.is_null() {
        return Ok(None);
    }
    let value = value.as_str().ok_or("Invalid capability text fact.")?;
    if value.encode_utf16().count() > 512
        || value
            .chars()
            .any(|character| character <= '\u{1f}' || character == '\u{7f}')
    {
        return Err("Invalid capability text fact.".into());
    }
    let trimmed = value.trim();
    Ok((!trimmed.is_empty()).then_some(trimmed))
}

fn count(value: &Value) -> Result<Option<u64>> {
    if value.is_null() {
        return Ok(None);
    }
    value
        .as_u64()
        .map(Some)
        .ok_or_else(|| "Capability counts and bytes must be unsigned integer facts.".into())
}

fn flag(value: &Value) -> Result<Option<bool>> {
    if value.is_null() {
        return Ok(None);
    }
    value
        .as_bool()
        .map(Some)
        .ok_or_else(|| "Capability flags must be boolean facts.".into())
}

const fn status(succeeded: bool, incomplete: bool) -> &'static str {
    if !succeeded {
        "unknown"
    } else if incomplete {
        "partial"
    } else {
        "observed"
    }
}

fn architecture(value: &Value) -> Result<Option<String>> {
    Ok(text(value)?
        .map(str::to_ascii_lowercase)
        .filter(|value| matches!(value.as_str(), "arm64" | "x64" | "x86" | "arm")))
}

fn inventory<'a>(facts: &'a Value, probes: &Value, name: &str) -> Result<&'a [Value]> {
    if probes[name] != true || facts[name].is_null() {
        return Ok(&[]);
    }
    let values = facts[name]
        .as_array()
        .ok_or("Capability inventories must be arrays.")?;
    if values.len() > 64 || values.iter().any(|value| !value.is_object()) {
        return Err("Invalid or oversized capability inventory.".into());
    }
    Ok(values)
}

fn date(value: &Value) -> Result<Option<&str>> {
    let Some(value) = text(value)? else {
        return Ok(None);
    };
    let bytes = value.as_bytes();
    // The PowerShell fact bridge normalizes real DateTime/DateTimeOffset values.
    // Require that fixed UTC transport form instead of accepting arbitrary text.
    if !((bytes.len() == 28 && bytes[27] == b'Z')
        || (bytes.len() == 33 && &bytes[27..] == b"+00:00"))
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes[19] != b'.'
        || bytes[..27].iter().enumerate().any(|(index, byte)| {
            !matches!(index, 4 | 7 | 10 | 13 | 16 | 19) && !byte.is_ascii_digit()
        })
    {
        return Err("Invalid normalized capability date fact.".into());
    }
    let digits = |start: usize, end: usize| {
        bytes[start..end]
            .iter()
            .fold(0_u32, |sum, byte| sum * 10 + u32::from(byte - b'0'))
    };
    let year = digits(0, 4);
    let month = digits(5, 7);
    let day = digits(8, 10);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year.is_multiple_of(400) || year.is_multiple_of(4) && !year.is_multiple_of(100) => 29,
        2 => 28,
        _ => 0,
    };
    if year == 0
        || day == 0
        || day > days
        || digits(11, 13) > 23
        || digits(14, 16) > 59
        || digits(17, 19) > 59
    {
        return Err("Invalid normalized capability date fact.".into());
    }
    Ok(Some(value))
}

fn project(facts: &Value) -> Result<Value> {
    if !facts.is_object() {
        return Err("Capability facts must be an object.".into());
    }
    let probes = &facts["Probes"];
    if !probes.is_null() && !probes.is_object() {
        return Err("Invalid capability probe results.".into());
    }
    if let Some(probes) = probes.as_object() {
        for value in probes.values() {
            let _ = flag(value)?;
        }
    }
    let observed = |name: &str| {
        if probes[name] == true {
            &facts[name]
        } else {
            &Value::Null
        }
    };
    let os = observed("OperatingSystem");
    let computer = observed("Computer");
    let memory = observed("Memory");
    let features = observed("Isa");
    let os_architecture = architecture(&facts["OsArchitecture"])?;
    let process_architecture = architecture(&facts["ProcessArchitecture"])?;
    let native = os_architecture
        .as_ref()
        .zip(process_architecture.as_ref())
        .map(|(os, process)| os == process);
    let version = text(&os["Version"])?;
    let build = text(&os["BuildNumber"])?;
    let processors = inventory(facts, probes, "Processors")?.iter().map(|cpu| Ok(json!({
        "name": text(&cpu["Name"])?, "manufacturer": text(&cpu["Manufacturer"])?,
        "physical_cores": count(&cpu["NumberOfCores"])?, "logical_processors": count(&cpu["NumberOfLogicalProcessors"])?
    }))).collect::<Result<Vec<_>>>()?;
    let graphics = inventory(facts, probes, "Graphics")?.iter().map(|gpu| Ok(json!({
        "name": text(&gpu["Name"])?, "vendor": text(&gpu["AdapterCompatibility"])?,
        "driver_version": text(&gpu["DriverVersion"])?, "driver_date": date(&gpu["DriverDate"])?
    }))).collect::<Result<Vec<_>>>()?;
    let npus = inventory(facts, probes, "NpuDrivers")?.iter().map(|npu| Ok(json!({
        "name": text(&npu["DeviceName"])?, "vendor": text(&npu["Manufacturer"])?,
        "driver_version": text(&npu["DriverVersion"])?, "driver_date": date(&npu["DriverDate"])?,
        "driver_signed": flag(&npu["IsSigned"])?
    }))).collect::<Result<Vec<_>>>()?;
    let total = count(&computer["TotalPhysicalMemory"])?;
    let usable = count(&os["TotalVisibleMemorySize"])?
        .map(|kib| {
            kib.checked_mul(1024)
                .ok_or("Capability memory conversion exceeds its byte bound.")
        })
        .transpose()?;
    let available = count(&memory["AvailableBytes"])?;
    let commit_limit = count(&memory["CommitLimit"])?;
    let committed = count(&memory["CommittedBytes"])?;
    if commit_limit
        .zip(committed)
        .is_some_and(|(limit, used)| used > limit)
    {
        return Err("Capability committed memory exceeds its observed limit.".into());
    }
    let advsimd = flag(&features["AdvSimd"])?;
    let dotprod = flag(&features["DotProd"])?;
    Ok(json!({
        "schema_version": 1, "kind": "runner-host-capabilities",
        "operating_system": {
            "architecture": os_architecture, "version": version, "build_number": build,
            "source": "RuntimeInformation.OSArchitecture;Win32_OperatingSystem",
            "status": status(os_architecture.is_some(), version.is_none() || build.is_none())
        },
        "process": { "architecture": process_architecture, "is_native": native,
            "source": "RuntimeInformation.ProcessArchitecture", "status": status(process_architecture.is_some(), native.is_none()) },
        "cpu": { "identities": processors, "source": "Win32_Processor",
            "status": status(probes["Processors"] == true, processors.is_empty() || processors.iter().any(|cpu| cpu["name"].is_null() || cpu["physical_cores"].is_null() || cpu["logical_processors"].is_null())),
            "topology": { "processor_groups": null, "efficiency_classes": null, "status": "unknown" },
            "isa": { "architecture": process_architecture, "scope": "current-process", "advsimd": advsimd, "dotprod": dotprod,
                "source": ".NET System.Runtime.Intrinsics.Arm.AdvSimd/Dp.IsSupported",
                "status": status(probes["Isa"] == true, advsimd.is_none() || dotprod.is_none() || process_architecture.is_none()) }
        },
        "memory": { "reported_total_physical_bytes": total, "usable_physical_bytes": usable, "available_physical_bytes": available,
            "commit_limit_bytes": commit_limit, "committed_bytes": committed,
            "source": "Win32_ComputerSystem;Win32_OperatingSystem;Win32_PerfFormattedData_PerfOS_Memory",
            "status": status(probes["Computer"] == true || probes["OperatingSystem"] == true || probes["Memory"] == true,
                total.is_none() || usable.is_none() || available.is_none() || commit_limit.is_none() || committed.is_none()) },
        "gpu": { "devices": graphics, "source": "Win32_VideoController",
            "status": status(probes["Graphics"] == true, graphics.iter().any(|gpu| gpu["name"].is_null() || gpu["driver_version"].is_null())),
            "api_features": null, "device_budget_bytes": null },
        "npu": { "devices": npus, "source": "Win32_PnPSignedDriver:DeviceClass=ComputeAccelerator",
            "status": status(probes["NpuDrivers"] == true, npus.iter().any(|npu| npu["name"].is_null() || npu["driver_version"].is_null() || npu["driver_signed"].is_null())),
            "architecture": null, "firmware_version": null, "backend_readiness": null }
    }))
}

#[derive(Serialize)]
struct Processor<'a> {
    name: &'a str,
    manufacturer: &'a str,
    physical_cores: u64,
    logical_processors: u64,
}
#[derive(Serialize)]
struct Isa {
    advsimd: bool,
    dotprod: bool,
}
#[derive(Serialize)]
struct CapabilityClass<'a> {
    schema_version: u8,
    kind: &'static str,
    scope: &'static str,
    architecture: &'static str,
    identities: Vec<Processor<'a>>,
    isa: Isa,
}
#[derive(Serialize)]
struct OperatingSystem<'a> {
    architecture: &'static str,
    version: &'a str,
    build_number: &'a str,
}
#[derive(Serialize)]
struct DriverRecord<'a> {
    schema_version: u8,
    kind: &'static str,
    scope: &'static str,
    backend: &'static str,
    operating_system: OperatingSystem<'a>,
}

fn canonical_json(value: &impl Serialize) -> Result<String> {
    // Newtonsoft's default JSON escaping, used by the original PowerShell
    // records, always escapes these three JavaScript line separators.
    serde_json::to_string(value)
        .map(|json| {
            json.replace('\u{85}', "\\u0085")
                .replace('\u{2028}', "\\u2028")
                .replace('\u{2029}', "\\u2029")
        })
        .map_err(|error| error.to_string())
}

fn processors(identities: &[Value]) -> Result<Option<Vec<Processor<'_>>>> {
    if identities.len() > 64 {
        return Err("Too many CPU compatibility identities.".into());
    }
    if identities.is_empty() {
        return Ok(None);
    }
    let mut processors = Vec::with_capacity(identities.len());
    for cpu in identities {
        let name = text(&cpu["name"])?;
        let manufacturer = text(&cpu["manufacturer"])?;
        let cores = count(&cpu["physical_cores"])?;
        let logical = count(&cpu["logical_processors"])?;
        let (Some(name), Some(manufacturer), Some(cores), Some(logical)) =
            (name, manufacturer, cores, logical)
        else {
            return Ok(None);
        };
        if name.eq_ignore_ascii_case("unknown")
            || manufacturer.eq_ignore_ascii_case("unknown")
            || cores == 0
            || logical < cores
        {
            return Ok(None);
        }
        let processor = Processor {
            name,
            manufacturer,
            physical_cores: cores,
            logical_processors: logical,
        };
        let canonical = canonical_json(&processor)?;
        processors.push((canonical, processor));
    }
    // Match .NET ordinal UTF-16 order, including non-BMP processor names.
    processors.sort_by(|(left, _), (right, _)| left.encode_utf16().cmp(right.encode_utf16()));
    Ok(Some(
        processors
            .into_iter()
            .map(|(_, processor)| processor)
            .collect(),
    ))
}

fn identity(capabilities: &Value) -> Result<Value> {
    if capabilities["schema_version"].as_u64() != Some(1)
        || capabilities["kind"] != "runner-host-capabilities"
    {
        return Err(
            "CPU compatibility identity requires the version-one host capability projection."
                .into(),
        );
    }
    let mut result = json!({
        "schema_version": 1, "kind": "runner-cpu-compatibility-identity", "scope": SCOPE,
        "status": "unknown", "fallback_reason": "unknown-capability", "qualification": "unqualified",
        "capability_class_json": null, "capability_class_sha256": null, "backend_driver_json": null, "backend_driver_sha256": null
    });
    let os = &capabilities["operating_system"];
    let process = &capabilities["process"];
    let cpu = &capabilities["cpu"];
    let isa = &cpu["isa"];
    if [os, process, cpu, isa]
        .iter()
        .any(|value| value["status"] != "observed")
    {
        return Ok(result);
    }
    let os_architecture = text(&os["architecture"])?;
    let process_architecture = text(&process["architecture"])?;
    let isa_architecture = text(&isa["architecture"])?;
    let native = flag(&process["is_native"])?;
    if os_architecture != Some("arm64")
        || process_architecture != Some("arm64")
        || isa_architecture != Some("arm64")
        || native != Some(true)
        || isa["scope"] != "current-process"
    {
        return Ok(result);
    }
    let version = text(&os["version"])?;
    let build = text(&os["build_number"])?;
    let advsimd = flag(&isa["advsimd"])?;
    let dotprod = flag(&isa["dotprod"])?;
    let (Some(version), Some(build), Some(advsimd), Some(dotprod)) =
        (version, build, advsimd, dotprod)
    else {
        return Ok(result);
    };
    if version.eq_ignore_ascii_case("unknown")
        || build.eq_ignore_ascii_case("unknown")
        || dotprod && !advsimd
    {
        return Ok(result);
    }
    let Some(identities) = cpu["identities"].as_array() else {
        return Ok(result);
    };
    let Some(processors) = processors(identities)? else {
        return Ok(result);
    };
    let capability = canonical_json(&CapabilityClass {
        schema_version: 1,
        kind: "runner-cpu-capability-class",
        scope: SCOPE,
        architecture: "arm64",
        identities: processors,
        isa: Isa { advsimd, dotprod },
    })?;
    let backend = canonical_json(&DriverRecord {
        schema_version: 1,
        kind: "runner-cpu-backend-driver",
        scope: SCOPE,
        backend: "cpu",
        operating_system: OperatingSystem {
            architecture: "arm64",
            version,
            build_number: build,
        },
    })?;
    result["status"] = json!("observed");
    result["fallback_reason"] = Value::Null;
    result["capability_class_sha256"] = json!(crate::identity::sha256(capability.as_bytes()));
    result["backend_driver_sha256"] = json!(crate::identity::sha256(backend.as_bytes()));
    result["capability_class_json"] = json!(capability);
    result["backend_driver_json"] = json!(backend);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> Value {
        json!({
            "OsArchitecture": "Arm64", "ProcessArchitecture": "Arm64",
            "OperatingSystem": { "Version": "10.0.26100", "BuildNumber": "26100", "TotalVisibleMemorySize": 33_554_432_u64 },
            "Computer": { "TotalPhysicalMemory": 34_359_738_368_u64 },
            "Processors": [{ "Name": "Snapdragon X2E-96-100", "Manufacturer": "Qualcomm", "NumberOfCores": 18, "NumberOfLogicalProcessors": 18 }],
            "Graphics": [], "NpuDrivers": [], "Memory": { "AvailableBytes": 17_179_869_184_u64, "CommitLimit": 68_719_476_736_u64, "CommittedBytes": 8_589_934_592_u64 },
            "Isa": { "AdvSimd": false, "DotProd": false },
            "Probes": { "OperatingSystem": true, "Computer": true, "Processors": true, "Graphics": true, "NpuDrivers": true, "Memory": true, "Isa": true }
        })
    }

    #[test]
    fn frozen_powershell_cpu_records() -> Result<()> {
        let projected = project(&facts())?;
        assert_eq!(
            projected["memory"]["usable_physical_bytes"],
            34_359_738_368_u64
        );
        let result = identity(&projected)?;
        let capability = r#"{"schema_version":1,"kind":"runner-cpu-capability-class","scope":"native-windows-arm64-cpu-observations-v1","architecture":"arm64","identities":[{"name":"Snapdragon X2E-96-100","manufacturer":"Qualcomm","physical_cores":18,"logical_processors":18}],"isa":{"advsimd":false,"dotprod":false}}"#;
        let backend = r#"{"schema_version":1,"kind":"runner-cpu-backend-driver","scope":"native-windows-arm64-cpu-observations-v1","backend":"cpu","operating_system":{"architecture":"arm64","version":"10.0.26100","build_number":"26100"}}"#;
        assert_eq!(result["capability_class_json"], capability);
        assert_eq!(result["backend_driver_json"], backend);
        assert_eq!(
            result["capability_class_sha256"],
            crate::identity::sha256(capability.as_bytes())
        );
        assert_eq!(
            result["backend_driver_sha256"],
            crate::identity::sha256(backend.as_bytes())
        );
        assert_eq!(result["qualification"], "unqualified");
        Ok(())
    }

    #[test]
    fn failed_empty_and_emulated_observations() -> Result<()> {
        let unknown = project(&json!({}))?;
        assert_eq!(unknown["cpu"]["status"], "unknown");
        assert!(unknown["cpu"]["isa"]["advsimd"].is_null());
        assert_eq!(identity(&unknown)?["status"], "unknown");
        let mut input = facts();
        assert_eq!(project(&input)?["npu"]["status"], "observed");
        input["Probes"]["NpuDrivers"] = json!(false);
        assert_eq!(project(&input)?["npu"]["status"], "unknown");
        input["ProcessArchitecture"] = json!("X64");
        let emulated = project(&input)?;
        assert_eq!(emulated["process"]["is_native"], false);
        assert_eq!(identity(&emulated)?["status"], "unknown");
        input["Probes"]["Isa"] = json!(false);
        assert!(project(&input)?["cpu"]["isa"]["advsimd"].is_null());
        Ok(())
    }

    #[test]
    fn strict_fact_types_and_bounds() -> Result<()> {
        for invalid in [json!(-1), json!("1"), json!(1.0), json!(true)] {
            let mut input = facts();
            input["Memory"]["AvailableBytes"] = invalid;
            assert!(project(&input).is_err());
        }
        let mut input = facts();
        input["OperatingSystem"]["TotalVisibleMemorySize"] = json!(u64::MAX);
        assert!(project(&input).is_err());
        input = facts();
        input["Memory"]["CommittedBytes"] = json!(u64::MAX);
        assert!(project(&input).is_err());
        input = facts();
        input["Processors"] = json!(vec![json!({}); 65]);
        assert!(project(&input).is_err());
        input = facts();
        input["Processors"][0]["Name"] = json!("🦀".repeat(257));
        assert!(project(&input).is_err());
        input["Processors"][0]["Name"] = json!("bad\nname");
        assert!(project(&input).is_err());
        input = facts();
        input["Isa"]["AdvSimd"] = json!("false");
        assert!(project(&input).is_err());
        input["Probes"]["Isa"] = json!(false);
        assert!(project(&input)?["cpu"]["isa"]["advsimd"].is_null());
        Ok(())
    }

    #[test]
    fn identity_scope_and_inventory_order() -> Result<()> {
        let mut projected = project(&facts())?;
        let original = identity(&projected)?;
        projected["memory"]["available_physical_bytes"] = json!(0);
        assert_eq!(identity(&projected)?, original);
        projected["operating_system"]["build_number"] = json!("26101");
        let updated = identity(&projected)?;
        assert_eq!(
            updated["capability_class_sha256"],
            original["capability_class_sha256"]
        );
        assert_ne!(
            updated["backend_driver_sha256"],
            original["backend_driver_sha256"]
        );
        let second = json!({ "name": "Other CPU", "manufacturer": "Vendor", "physical_cores": 2, "logical_processors": 4 });
        let first = projected["cpu"]["identities"][0].clone();
        projected["cpu"]["identities"] = json!([first, second]);
        let ordered = identity(&projected)?;
        projected["cpu"]["identities"] = json!([second, first]);
        assert_eq!(identity(&projected)?, ordered);
        projected["cpu"]["identities"] = json!([second, first, first]);
        assert_ne!(
            identity(&projected)?["capability_class_sha256"],
            ordered["capability_class_sha256"]
        );
        projected["cpu"]["isa"]["dotprod"] = json!(true);
        assert_eq!(identity(&projected)?["status"], "unknown");
        projected["schema_version"] = json!(1.0);
        assert!(identity(&projected).is_err());
        Ok(())
    }

    #[test]
    fn incomplete_identity_facts_remain_unknown() -> Result<()> {
        let projected = project(&facts())?;
        for (pointer, invalid) in [
            ("/operating_system/status", json!("partial")),
            ("/process/status", json!("unknown")),
            ("/cpu/status", json!("unknown")),
            ("/cpu/isa/status", json!("unknown")),
            ("/process/is_native", json!(false)),
            ("/cpu/isa/scope", json!("other-process")),
            ("/cpu/isa/architecture", json!("x64")),
            ("/operating_system/version", json!("UNKNOWN")),
            ("/cpu/identities/0/name", Value::Null),
            ("/cpu/identities/0/manufacturer", json!("unknown")),
            ("/cpu/identities/0/physical_cores", json!(0)),
            ("/cpu/identities/0/logical_processors", json!(1)),
            ("/cpu/isa/advsimd", Value::Null),
            ("/cpu/isa/dotprod", json!(true)),
            ("/cpu/identities", json!([])),
        ] {
            let mut input = projected.clone();
            *input
                .pointer_mut(pointer)
                .ok_or("Missing test fixture field.")? = invalid;
            let result = identity(&input)?;
            assert_eq!(result["status"], "unknown", "{pointer}");
            assert!(result["capability_class_json"].is_null(), "{pointer}");
            assert!(result["backend_driver_sha256"].is_null(), "{pointer}");
        }
        for schema in [json!(2), json!("1"), Value::Null] {
            let mut input = projected.clone();
            input["schema_version"] = schema;
            assert!(identity(&input).is_err());
        }
        for kind in [
            json!([]),
            json!(["runner-host-capabilities"]),
            json!(1),
            Value::Null,
        ] {
            let mut input = projected.clone();
            input["kind"] = kind;
            assert!(identity(&input).is_err());
        }
        Ok(())
    }

    #[test]
    fn unicode_canonical_records_preserve_powershell_escaping_and_order() -> Result<()> {
        let mut projected = project(&facts())?;
        projected["cpu"]["identities"][0]["name"] = json!("CPU\u{85}\u{2028}\u{2029}🦀");
        let result = identity(&projected)?;
        let record = result["capability_class_json"]
            .as_str()
            .ok_or("Missing CPU record.")?;
        assert!(record.contains(r"CPU\u0085\u2028\u2029🦀"));
        let original = projected["cpu"]["identities"][0].clone();
        let mut bmp = original.clone();
        bmp["name"] = json!("\u{e000}");
        let mut non_bmp = original;
        non_bmp["name"] = json!("🦀");
        projected["cpu"]["identities"] = json!([bmp, non_bmp]);
        let result = identity(&projected)?;
        let record = result["capability_class_json"]
            .as_str()
            .ok_or("Missing CPU record.")?;
        let canonical: Value = serde_json::from_str(record).map_err(|error| error.to_string())?;
        assert_eq!(canonical["identities"][0]["name"], "🦀");
        Ok(())
    }

    #[test]
    fn command_and_device_transport_boundaries() -> Result<()> {
        assert!(run(&json!({ "operation": "project", "facts": {}, "extra": true })).is_err());
        assert!(run(&json!({ "operation": "project", "facts": [] })).is_err());
        assert!(run(&json!({ "operation": "identity", "capabilities": [] })).is_err());
        let mut input = facts();
        input["Graphics"] = json!([{ "Name": "Adreno", "AdapterCompatibility": "Qualcomm", "DriverVersion": "1.2.3", "DriverDate": "2026-01-01T00:00:00.0000000Z" }]);
        let result = run(&json!({ "operation": "project", "facts": input }))?;
        assert_eq!(result["gpu"]["status"], "observed");
        assert_eq!(
            result["gpu"]["devices"][0]["driver_date"],
            "2026-01-01T00:00:00.0000000Z"
        );
        input["Graphics"][0]["DriverDate"] = json!("not-a-date");
        assert!(project(&input).is_err());
        input["Probes"]["Graphics"] = json!(false);
        assert_eq!(project(&input)?["gpu"]["status"], "unknown");
        input["Probes"]["Graphics"] = json!("false");
        assert!(project(&input).is_err());
        Ok(())
    }
}
