//! Independent generated-C and Rust wire receipt controls.
use super::{
    Control, OsString, Path, PathBuf, Result, Value, copy, fields_equal, flags_false, fs,
    hexagon_policy, identity, json, pinned, require, string,
};
pub(super) const SHARED: &[&str] = &[
    "schemaVersion",
    "testVectorId",
    "handleHex",
    "start",
    "stop",
];
pub(super) fn layout() -> Value {
    json!({"charBits":8,"pointerBytes":8,"sizeTBytes":8,"uint32Bytes":4,"uint64Bytes":8,"remoteHandle64Bytes":8,"remoteBufferBytes":16,"remoteBufferAlignment":8,"remoteBufferPointerOffset":0,"remoteBufferLengthOffset":8,"remoteArgBytes":16,"remoteArgAlignment":8,"littleEndian":true})
}
pub(super) fn capture(actual: &Value, expected: &Value) -> Result<()> {
    fields_equal(actual, expected, SHARED)?;
    require(
        actual["layout"] == layout()
            && actual["callCount"] == 2
            && actual["unexpectedCalls"] == 0
            && actual["returnCodes"] == json!({"start":0,"stop":0}),
        "Generated C wire failed fixed call/layout checks.",
    )
}
pub(super) fn parse(text: &str) -> Result<Value> {
    require(
        text.len() <= 16384,
        "Wire capture receipt exceeds fixed bound.",
    )?;
    hexagon_policy::data_json(text)
}
pub(super) fn rust_receipt(actual: &Value, captured: &Value) -> Result<()> {
    let fields = [
        "schemaVersion",
        "testVectorId",
        "handleHex",
        "start",
        "stop",
        "driverLoaded",
        "deviceExecuted",
        "sdkCompatibilityQualified",
        "profileEligible",
        "kind",
    ];
    let actual_keys = actual
        .as_object()
        .ok_or("Rust wire receipt must be an object.")?;
    require(
        actual_keys.len() == fields.len()
            && fields.iter().all(|key| actual_keys.contains_key(*key))
            && actual["kind"] == "public-idl-rust-encoding-control",
        "Rust encoding emitted unsupported receipt shape.",
    )?;
    flags_false(
        actual,
        &[
            "driverLoaded",
            "deviceExecuted",
            "sdkCompatibilityQualified",
            "profileEligible",
        ],
    )?;
    fields_equal(actual, captured, SHARED)
}
pub(super) fn compare_rust(
    control: &mut Control,
    pin: &Value,
    report: &mut Value,
    rustc: &Path,
) -> Result<()> {
    let selection = &pin["rustWireControl"];
    flags_false(selection, &["deviceInvocationAuthorized"])?;
    require(
        report["wireCapture"]["passed"] == true,
        "Independent generated-C wire capture must pass first.",
    )?;
    let (compiler, original, sysroot, drivers) = rust_compiler(control, selection, rustc)?;
    let target = string(selection, "target")?;
    let source = control.source(string(&selection["source"], "name")?);
    let original_source = pinned(&source, &selection["source"])?;
    let retained = control.review.join(string(&selection["source"], "name")?);
    copy(&source, &retained)?;
    let linker = PathBuf::from(string(&report["wireCapture"]["compiler"], "path")?);
    pinned(&linker, &report["wireCapture"]["compiler"])?;
    let flags: Vec<OsString> = vec![
        "--edition=2024".into(),
        "--target".into(),
        target.into(),
        "-D".into(),
        "warnings".into(),
        "-C".into(),
        "opt-level=2".into(),
        "-C".into(),
        "overflow-checks=yes".into(),
        "-C".into(),
        format!("linker={}", linker.display()).into(),
    ];
    let executable = control.output.join("public-idl-rust-control");
    let tests = control.output.join("public-idl-rust-contract");
    let mut args = vec![compiler.clone().into_os_string()];
    args.extend(flags.clone());
    args.extend([
        source.clone().into_os_string(),
        "-o".into(),
        executable.clone().into_os_string(),
    ]);
    control.run("wire-rust-build", args, 30)?;
    let mut args = vec![compiler.clone().into_os_string()];
    args.extend(flags);
    args.extend([
        "--test".into(),
        source.clone().into_os_string(),
        "-o".into(),
        tests.clone().into_os_string(),
    ]);
    control.run("wire-rust-test-build", args, 30)?;
    require(
        identity(&compiler)? == original && identity(&source)? == original_source,
        "Rust compiler or encoder changed during bounded build.",
    )?;
    let exe_pin = identity(&executable)?;
    let test_pin = identity(&tests)?;
    control.run(
        "wire-rust-contract",
        vec![tests.clone().into_os_string()],
        5,
    )?;
    let text = control.run(
        "wire-rust-encoding",
        vec![executable.clone().into_os_string()],
        5,
    )?;
    require(
        identity(&executable)? == exe_pin && identity(&tests)? == test_pin,
        "Rust control bytes changed during execution.",
    )?;
    let actual = parse(&text)?;
    rust_receipt(&actual, &report["wireCapture"]["observed"])?;
    report["rustWireControl"] = json!({"kind":"independent-Rust-public-wire-encoding-control","passed":true,"compiler":original,"toolchain":selection["toolchain"],"target":target,"sysroot":sysroot,"identityScope":"selected rustc/driver files and verbose identity; not-complete-OS-closure","selectedCompilerDrivers":drivers,"source":identity(&retained)?,"executable":exe_pin,"contractExecutable":test_pin,"testExecutionAttempted":true,"testExecutionCompleted":true,"encodingExecutionAttempted":true,"encodingExecutionCompleted":true,"observed":actual,"captureReceipt":identity(&control.review.join("wire-rust-encoding.log"))?,"generatedCWireMatched":true,"driverLoaded":false,"deviceExecuted":false,"sdkCompatibilityQualified":false,"profileEligible":false});
    report["stage"] = json!("independent-public-C-Rust-wire-matched");
    report["outcome"] = json!("public-generated-C-and-Rust-wire-cross-checked-only");
    Ok(())
}

fn rust_compiler(
    control: &mut Control,
    selection: &Value,
    rustc: &Path,
) -> Result<(PathBuf, Value, PathBuf, Vec<Value>)> {
    let compiler = fs::canonicalize(rustc).map_err(|e| e.to_string())?;
    let original = identity(&compiler)?;
    let banner = control.run(
        "wire-rustc-version",
        vec![compiler.clone().into_os_string(), "-vV".into()],
        30,
    )?;
    let target = string(selection, "target")?;
    require(
        banner
            .lines()
            .any(|line| line.trim_end() == format!("host: {target}")),
        "Rust wire control requires the fixed native Linux target.",
    )?;
    let sysroot = control.run(
        "wire-rustc-sysroot",
        vec![
            compiler.clone().into_os_string(),
            "--print".into(),
            "sysroot".into(),
        ],
        30,
    )?;
    let sysroot = PathBuf::from(sysroot.trim());
    require(
        sysroot.file_name().and_then(|s| s.to_str()) == Some(string(selection, "toolchain")?)
            && compiler.parent().and_then(Path::parent) == Some(sysroot.as_path()),
        "Rust compiler resolved outside the selected dated toolchain.",
    )?;
    let mut drivers = Vec::new();
    for entry in fs::read_dir(sysroot.join("lib")).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        if name.starts_with("librustc_driver-") && path.extension().is_some_and(|ext| ext == "so") {
            let mut item = identity(&path)?;
            item["path"] = json!(path);
            drivers.push(item);
            require(
                drivers.len() <= 8,
                "Rust compiler driver count exceeds bound.",
            )?;
        }
    }
    Ok((compiler, original, sysroot, drivers))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn public_wire_receipts_reject_changed_geometry_calls_and_authority() -> Result<()> {
        let mut expected = hexagon_policy::data_json(include_str!(
            "../../../../hexagon/public-qaic-wire.expected.json"
        ))?;
        expected["layout"] = layout();
        expected["callCount"] = json!(2);
        expected["unexpectedCalls"] = json!(0);
        expected["returnCodes"] = json!({"start":0,"stop":0});
        capture(&expected, &expected)?;
        for (pointer, value) in [
            ("/layout/remoteArgBytes", json!(8)),
            ("/callCount", json!("2")),
            ("/unexpectedCalls", json!(1)),
            ("/returnCodes/stop", json!(1)),
        ] {
            let mut changed = expected.clone();
            *changed
                .pointer_mut(pointer)
                .ok_or("Wire fixture field missing.")? = value;
            assert!(capture(&changed, &expected).is_err());
        }
        let mut rust = json!({"kind":"public-idl-rust-encoding-control","driverLoaded":false,"deviceExecuted":false,"sdkCompatibilityQualified":false,"profileEligible":false});
        for field in SHARED {
            rust[*field] = expected[*field].clone();
        }
        rust_receipt(&rust, &expected)?;
        for field in [
            "driverLoaded",
            "deviceExecuted",
            "sdkCompatibilityQualified",
            "profileEligible",
        ] {
            let mut changed = rust.clone();
            changed[field] = json!(true);
            assert!(rust_receipt(&changed, &expected).is_err());
        }
        rust["unreviewed"] = json!(true);
        assert!(rust_receipt(&rust, &expected).is_err());
        assert!(parse(&" ".repeat(16385)).is_err());
        Ok(())
    }
}
