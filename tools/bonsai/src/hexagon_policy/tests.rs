use super::*;

const DRIVER: &str = "C:\\Windows\\System32\\DriverStore\\FileRepository\\fixture\\libcdsprpc.dll";

fn native() -> Result<Value> {
    serde_json::from_str(include_str!("../../tests/fixtures/hexagon-session-v1.json"))
        .map_err(|error| error.to_string())
}
fn dispatch_record() -> Result<Value> {
    serde_json::from_str(include_str!(
        "../../tests/fixtures/hexagon-dispatch-v1.json"
    ))
    .map_err(|error| error.to_string())
}
fn compute_record() -> Result<Value> {
    serde_json::from_str(include_str!("../../tests/fixtures/hexagon-compute-v1.json"))
        .map_err(|error| error.to_string())
}
fn session_check(value: &Value, reference: bool) -> Result<Value> {
    run(
        json!({"operation":"session-result","json":value.to_string(),"exitCode":0,"referenceInitialization":reference}),
    )
}
fn dispatch_check(value: &Value, exit_code: i32) -> Result<Value> {
    run(json!({"operation":"dispatch-result","json":value.to_string(),"exitCode":exit_code}))
}
fn compute_check(value: &Value, case: &str) -> Result<Value> {
    run(json!({"operation":"compute-result","json":value.to_string(),"exitCode":0,"case":case}))
}
fn changed(value: &Value, edits: &[(&str, Value)]) -> Value {
    let mut value = value.clone();
    for (name, replacement) in edits {
        value[*name] = replacement.clone();
    }
    value
}
fn rejects(
    value: &Value,
    names: &[&str],
    replacement: &Value,
    check: impl Fn(&Value) -> Result<Value>,
) {
    for name in names {
        assert!(
            check(&changed(value, &[(name, replacement.clone())])).is_err(),
            "Accepted invalid {name}"
        );
    }
}

#[test]
fn strict_json_rejects_duplicate_aliases_coercions_size_depth_and_trailing_input() -> Result<()> {
    for text in [
        "{\"Outcome\":1,\"Outcome\":2}",
        "{\"outcome\":1,\"Outcome\":2}",
        "{\"x\":[{\"Rows\":1,\"Rows\":2}]}",
        "{\"\\u0052ows\":1,\"Rows\":2}",
        "{\"非\":1}",
        "[]",
        "null",
        "{} {}",
    ] {
        assert!(json::parse(text).is_err());
    }
    assert!(json::parse(&format!("{{\"x\":\"{}\"}}", "x".repeat(65536))).is_err());
    assert!(json::parse(&format!("{{\"x\":{}0{}}}", "[".repeat(31), "]".repeat(31))).is_ok());
    assert!(json::parse(&format!("{{\"x\":{}0{}}}", "[".repeat(32), "]".repeat(32))).is_err());
    let record = dispatch_record()?;
    for value in [json!(true), json!(1.0), json!("1")] {
        rejects(&record, &["SchemaVersion"], &value, |record| {
            dispatch_check(record, 0)
        });
    }
    let raw = record.to_string().replace(
        "\"Outcome\":\"passed\"",
        "\"outcome\":\"pending\",\"Outcome\":\"passed\"",
    );
    assert!(run(json!({"operation":"dispatch-result","json":raw,"exitCode":0})).is_err());
    Ok(())
}

#[test]
fn plans_and_environment_preserve_fixed_bounds_and_no_admission() -> Result<()> {
    let dispatch = run(json!({"operation":"dispatch-plan","maximumSeconds":120}))?;
    assert_eq!(dispatch["MaximumSeconds"], 120);
    assert_eq!(dispatch["CleanupReserveSeconds"], 75);
    assert_eq!(
        dispatch["Cases"],
        json!([{"Columns":128,"Rows":32,"Batch":1},{"Columns":128,"Rows":32,"Batch":32}])
    );
    Record(&dispatch).flags(
        &[
            "OwnedJobRequired",
            "CatalogMemberVerificationRequired",
            "SourceBoundHostedFixtureRequired",
            "AsInvokerManifestRequired",
        ],
        true,
    )?;
    Record(&dispatch).flags(
        &[
            "ModelLoaded",
            "CpuFallbackAllowed",
            "ProfileEligible",
            "DriverInstallationAllowed",
        ],
        false,
    )?;
    Ok(())
}

#[test]
fn session_plans_keep_reference_selection_separate_from_admission() -> Result<()> {
    for reference in [false, true] {
        let session = run(
            json!({"operation":"session-plan","maximumSeconds":120,"referenceInitialization":reference}),
        )?;
        for (name, value) in [
            ("MaximumSeconds", 120),
            ("CleanupReserveSeconds", 75),
            ("Bytes", 4096),
            ("MapFlags", 2),
            ("SessionId", 0),
        ] {
            assert_eq!(session[name], value);
        }
        Record(&session).flags(
            &[
                "OwnedJobRequired",
                "CatalogMemberVerificationRequired",
                "SourceBoundHostedFixtureRequired",
                "IndependentDriverIdentityRequired",
            ],
            true,
        )?;
        Record(&session).flags(
            &[
                "DriverInstallationAllowed",
                "TrustChangesAllowed",
                "ComputeExecuted",
                "ModelLoaded",
                "ProfileEligible",
                "AutomaticFallbackAllowed",
                "AutomaticRetryAllowed",
            ],
            false,
        )?;
        assert_eq!(session["ReferenceProcessInitializationSelected"], reference);
        assert_eq!(
            session["ReferenceProcessInitializationControl"],
            if reference {
                "DSPRPC_CONTROL_UNSIGNED_MODULE-enable1-default-session-effective-domain-process-only"
            } else {
                "not-selected"
            }
        );
    }
    Ok(())
}

#[test]
fn compute_plans_and_environment_keep_fixed_cases_and_no_credentials() -> Result<()> {
    for case in ["F32", "GuardedQ2", "F32NoBlock", "GuardedQ2NoBlock"] {
        let compute = run(json!({"operation":"compute-plan","maximumSeconds":120,"case":case}))?;
        for (name, value) in [
            ("MaximumSeconds", 120),
            ("CleanupReserveSeconds", 75),
            ("QueueSlots", 1),
            ("Operations", 1),
            ("Rows", 32),
            ("Batch", 2),
        ] {
            assert_eq!(compute[name], value);
        }
        assert_eq!(
            compute["Columns"],
            if case.starts_with("GuardedQ2") {
                256
            } else {
                32
            }
        );
        Record(&compute).flags(
            &[
                "OwnedJobRequired",
                "IndependentWindowsAbiRequired",
                "CatalogVerificationRequired",
                "SourceBoundHostedFixtureRequired",
            ],
            true,
        )?;
        Record(&compute).flags(
            &[
                "ModelLoaded",
                "ProfileEligible",
                "TrustChangesAllowed",
                "AutomaticRetryAllowed",
            ],
            false,
        )?;
    }
    for maximum in [0, 241] {
        assert!(run(json!({"operation":"dispatch-plan","maximumSeconds":maximum})).is_err());
    }
    let environment = run(
        json!({"operation":"environment","runtimeDirectory":"C:\\runtime","systemDirectory":"C:\\Windows\\System32"}),
    )?;
    assert_eq!(
        environment["Set"],
        json!({"PATH":"C:\\runtime;C:\\Windows\\System32","ADSP_LIBRARY_PATH":"C:\\runtime"})
    );
    for prefix in [
        "GGML_", "GENIEX_", "FASTRPC_", "RPCMEM_", "HEXAGON_", "ADSP_",
    ] {
        assert!(
            Record(&environment)
                .array("RemovePrefixes")?
                .contains(&json!(prefix))
        );
    }
    for name in ["ACTIONS_RUNTIME_TOKEN", "GITHUB_TOKEN"] {
        assert!(
            Record(&environment)
                .array("RemoveNames")?
                .contains(&json!(name))
        );
    }
    Ok(())
}

fn build_record(session: bool) -> Value {
    let mut value = json!({"SchemaVersion":1,"RunnerRevision":"1".repeat(40),"RustToolchain":"nightly-2026-11-02-aarch64-pc-windows-msvc","SourceSha256":"2".repeat(64),"ManifestSha256":"4".repeat(64),"LockSha256":"5".repeat(64),"HostedBuild":true,"Target":"aarch64-pc-windows-msvc","CompilerIdentitySha256":"3".repeat(64),"Executable":{"Name":if session {"native-hexagon-query.exe"} else {"hexagon-dispatch-fixture.exe"},"Bytes":65536,"Sha256":"6".repeat(64)}});
    if session {
        value["RunId"] = json!("1234");
        value["RunAttempt"] = json!("1");
        value["SourceFiles"] = json!(
            [
                "lib.rs",
                "windows.rs",
                "mapping.rs",
                "session.rs",
                "session_native.rs",
                "session_mapping.rs",
                "session_mapping_native.rs",
                "compute.rs",
                "compute_native.rs",
                "query.rs",
                "sources.json"
            ]
            .map(|name| json!({"Name":name,"Sha256":"2".repeat(64)}))
        );
        for name in ["DeviceCodeLoaded", "DspCodeLoaded", "ProfileEligible"] {
            value[name] = false.into();
        }
    }
    value
}

fn check_build(record: &Value, session: bool) -> Result<Value> {
    let mut request = json!({"operation":if session {"session-build"} else {"dispatch-build"},"json":record.to_string(),"expectedRunnerRevision":"1".repeat(40),"manifestSha256":"4".repeat(64),"lockSha256":"5".repeat(64)});
    if session {
        let sources: serde_json::Map<_, _> = [
            "lib.rs",
            "windows.rs",
            "mapping.rs",
            "session.rs",
            "session_native.rs",
            "session_mapping.rs",
            "session_mapping_native.rs",
            "compute.rs",
            "compute_native.rs",
            "query.rs",
            "sources.json",
        ]
        .into_iter()
        .map(|name| (name.into(), json!("2".repeat(64))))
        .collect();
        request["sourceDigests"] = sources.into();
    } else {
        request["sourceSha256"] = json!("2".repeat(64));
    }
    run(request)
}

#[test]
fn hosted_build_requires_exact_source_set_types_and_executable_identity() -> Result<()> {
    for session in [false, true] {
        let record = build_record(session);
        check_build(&record, session)?;
        rejects(
            &record,
            &[
                "RunnerRevision",
                "HostedBuild",
                "ManifestSha256",
                "LockSha256",
                "Target",
                "CompilerIdentitySha256",
            ],
            &Value::Null,
            |value| check_build(value, session),
        );
        for schema in [json!(true), json!(1.0)] {
            rejects(&record, &["SchemaVersion"], &schema, |value| {
                check_build(value, session)
            });
        }
        for name in ["RustToolchain", "ManifestSha256", "LockSha256", "Target"] {
            rejects(&record, &[name], &json!([record[name]]), |value| {
                check_build(value, session)
            });
        }
        let mut bad = record.clone();
        bad["Executable"]["Name"] = json!("../native-hexagon-query.exe");
        assert!(check_build(&bad, session).is_err());
        if session {
            rejects(
                &record,
                &["RunId", "RunAttempt", "SourceFiles"],
                &Value::Null,
                |value| check_build(value, true),
            );
            rejects(
                &record,
                &["DeviceCodeLoaded", "DspCodeLoaded", "ProfileEligible"],
                &json!(0),
                |value| check_build(value, true),
            );
            bad = record.clone();
            bad["SourceFiles"] = vec![record["SourceFiles"][0].clone(); 11].into();
            assert!(check_build(&bad, true).is_err());
        } else {
            rejects(&record, &["SourceSha256"], &json!("changed"), |value| {
                check_build(value, false)
            });
        }
    }
    Ok(())
}

fn driver() -> Value {
    json!({"Path":DRIVER,"Bytes":65536,"Sha256":"7".repeat(64),"SignatureStatus":"Valid","Publisher":"Microsoft Windows Hardware Compatibility Publisher","SignerThumbprint":"8".repeat(40),"FileHeldAgainstMutation":true})
}
fn dispatch_worker(record: &Value) -> Value {
    json!({"SchemaVersion":1,"Kind":"runner-hexagon-dispatch-worker","SourceRevision":"1".repeat(40),"Outcome":"passed","Stage":"numeric-dispatch-complete","Failure":null,"LastFixtureStage":"readback","CatalogMemberVerified":true,"CatalogMembersVerified":4,"CatalogMemberNames":["libggml-htp-v73.so","libggml-htp-v75.so","libggml-htp-v79.so","libggml-htp-v81.so"],"SelectedDspArchitecture":"unknown","DeviceDispatchVerified":true,"FixtureResult":record,"LoadedDriverObservation":"not-observed","LoadedDriverIdentity":null,"RuntimeFilesVerified":16,"FixtureSha256":"2".repeat(64),"FixtureSourceSha256":"3".repeat(64),"ManifestSha256":"4".repeat(64),"LockSha256":"5".repeat(64),"SigntoolSha256":"6".repeat(64),"ModelLoaded":false,"SwiftQualified":false,"ProfileEligible":false,"DriverInstalled":false,"TrustChanged":false})
}
fn worker_check(record: &Value, session: bool, reference: bool) -> Result<Value> {
    let mut request = json!({"operation":if session {"session-worker"} else {"dispatch-worker"},"json":record.to_string(),"expectedRunnerRevision":"1".repeat(40)});
    if session {
        request["referenceInitialization"] = reference.into();
    }
    run(request)
}

#[test]
fn dispatch_partial_cases_and_loaded_driver_cannot_become_independent_gates() -> Result<()> {
    let record = dispatch_record()?;
    dispatch_check(&record, 0)?;
    assert!(dispatch_check(&record, 2).is_err());
    rejects(
        &record,
        &[
            "CpuFallbackAllowed",
            "ModelLoaded",
            "SwiftQualified",
            "ProfileEligible",
            "SelfTest",
        ],
        &json!(true),
        |value| dispatch_check(value, 0),
    );
    for cases in [
        json!([record["Cases"][0]]),
        json!([record["Cases"][0], record["Cases"][0]]),
    ] {
        rejects(&record, &["Cases"], &cases, |value| {
            dispatch_check(value, 0)
        });
    }
    let pending = changed(
        &record,
        &[
            ("Outcome", json!("pending")),
            ("Stage", json!("allocation")),
            ("Failure", json!("hexagon-buffer-allocation-failed")),
            ("DeviceDispatchVerified", json!(false)),
            ("Cases", json!([record["Cases"][0]])),
        ],
    );
    assert_eq!(
        dispatch_check(&pending, 2)?["Cases"],
        json!([record["Cases"][0]])
    );
    let mut bad = pending.clone();
    bad["Cases"][0]["OutputValuesChecked"] = json!(1);
    assert!(dispatch_check(&bad, 2).is_err());
    for code in [json!(true), json!(1.5), json!(-1), json!(4_294_967_296_u64)] {
        rejects(&pending, &["NativeCode"], &code, |value| {
            dispatch_check(value, 2)
        });
    }
    dispatch_workers(&record, &pending)
}

fn dispatch_workers(record: &Value, pending: &Value) -> Result<()> {
    let worker = dispatch_worker(record);
    worker_check(&worker, false, false)?;
    let observed = changed(
        &worker,
        &[
            ("LoadedDriverObservation", json!("observed-verified")),
            ("LoadedDriverIdentity", driver()),
        ],
    );
    worker_check(&observed, false, false)?;
    for name in ["Sha256", "FileHeldAgainstMutation", "Publisher"] {
        let mut bad = observed.clone();
        bad["LoadedDriverIdentity"][name] = Value::Null;
        assert!(worker_check(&bad, false, false).is_err());
    }
    rejects(
        &observed,
        &["LoadedDriverObservation"],
        &json!("not-observed"),
        |value| worker_check(value, false, false),
    );
    rejects(
        &worker,
        &[
            "ModelLoaded",
            "SwiftQualified",
            "ProfileEligible",
            "DriverInstalled",
            "TrustChanged",
        ],
        &json!(0),
        |value| worker_check(value, false, false),
    );
    rejects(
        &worker,
        &[
            "SourceRevision",
            "CatalogMembersVerified",
            "RuntimeFilesVerified",
            "FixtureSha256",
            "DeviceDispatchVerified",
            "FixtureResult",
        ],
        &Value::Null,
        |value| worker_check(value, false, false),
    );
    rejects(&worker, &["FixtureResult"], pending, |value| {
        worker_check(value, false, false)
    });
    let partial = changed(
        &worker,
        &[
            ("Outcome", json!("pending")),
            ("DeviceDispatchVerified", json!(false)),
            ("Stage", json!("device-dispatch")),
            ("FixtureResult", Value::Null),
        ],
    );
    for stage in ["readback", "context"] {
        assert_eq!(
            worker_check(
                &changed(&partial, &[("LastFixtureStage", json!(stage))]),
                false,
                false
            )?["LastFixtureStage"],
            stage
        );
    }
    Ok(())
}

fn reference_record(native: &Value) -> Value {
    changed(
        native,
        &[
            ("referenceProcessInitializationSelected", json!(true)),
            ("processInitializationResult", json!(0)),
            ("unsignedProcessRequested", json!(true)),
            ("processInitializationDomainAfter", json!(3)),
            ("processInitializationEnableAfter", json!(1)),
        ],
    )
}
fn refused(native: &Value) -> Value {
    changed(
        native,
        &[
            ("passed", json!(false)),
            ("stage", json!("fd-mapping-failed-pinned")),
            ("mappingResult", json!(512)),
            ("unmappingResult", Value::Null),
            ("freeCompleted", json!(false)),
            ("poisoned", json!(true)),
            ("moduleCloseResult", Value::Null),
            ("moduleCloseCompleted", json!(false)),
        ],
    )
}
fn open_refused(native: &Value) -> Value {
    changed(
        &refused(native),
        &[
            ("stage", json!("module-open-refused-pinned")),
            ("moduleOpenResult", json!(512)),
            ("handleObserved", json!(false)),
            ("remoteModuleOpened", json!(false)),
            ("allocationSucceeded", json!(false)),
            ("descriptorObserved", json!(false)),
            ("cpuRoundtripVerified", json!(false)),
            ("mappingAttempted", json!(false)),
            ("mappingResult", Value::Null),
            ("poisoned", json!(false)),
        ],
    )
}

#[test]
fn session_mapping_refusals_preserve_uncertain_owners_and_do_not_qualify_compute() -> Result<()> {
    let native = native()?;
    session_check(&native, false)?;
    rejects(
        &native,
        &[
            "profileEligible",
            "computeExecuted",
            "dmaExecuted",
            "unsignedProcessRequested",
            "signedDeploymentVerified",
            "sessionTeardownVerified",
        ],
        &json!(true),
        |value| session_check(value, false),
    );
    rejects(
        &native,
        &[
            "mapFlags",
            "bytes",
            "moduleCloseCompleted",
            "uriVerified",
            "mappingAttempted",
            "ownerPinnedUntilExit",
        ],
        &json!(0),
        |value| session_check(value, false),
    );
    rejects(&native, &["architecture"], &json!(73), |value| {
        session_check(value, false)
    });
    let refused = refused(&native);
    session_check(&refused, false)?;
    rejects(&refused, &["freeCompleted"], &json!(true), |value| {
        session_check(value, false)
    });
    for stage in [
        "session-fd-cycle-completed-owner-pinned",
        "fd-unmapping-failed-pinned",
    ] {
        rejects(&refused, &["stage"], &json!(stage), |value| {
            session_check(value, false)
        });
    }
    session_check(&open_refused(&native), false)?;
    let raw = native
        .to_string()
        .replace("\"passed\":true", "\"Passed\":false,\"passed\":true");
    assert!(run(json!({"operation":"session-result","json":raw,"exitCode":0,"referenceInitialization":false})).is_err());
    Ok(())
}

#[test]
fn reference_initialization_preserves_selection_returned_bytes_and_prerequisites() -> Result<()> {
    let native = native()?;
    let reference = reference_record(&native);
    session_check(&reference, true)?;
    assert!(session_check(&reference, false).is_err());
    assert!(session_check(&native, true).is_err());
    for name in [
        "processInitializationDomainAfter",
        "processInitializationEnableAfter",
    ] {
        for invalid in [
            Value::Null,
            json!(true),
            json!(1.5),
            json!(2_147_483_648_i64),
        ] {
            rejects(&reference, &[name], &invalid, |value| {
                session_check(value, true)
            });
        }
        let mut missing = reference.clone();
        missing.as_object_mut().ok_or("fixture")?.remove(name);
        assert!(session_check(&missing, true).is_err());
        rejects(&native, &[name], &json!(3), |value| {
            session_check(value, false)
        });
    }
    let returned = changed(
        &reference,
        &[
            (
                "processInitializationDomainAfter",
                json!(-2_147_483_648_i64),
            ),
            ("processInitializationEnableAfter", json!(2_147_483_647_i64)),
        ],
    );
    session_check(&returned, true)?;
    rejects(
        &reference,
        &[
            "processInitializationRequestId",
            "processInitializationEnable",
            "processInitializationStructureBytes",
        ],
        &json!(0),
        |value| session_check(value, true),
    );
    reference_refusals(&native)
}

fn reference_refusals(native: &Value) -> Result<()> {
    let refused = changed(
        &reference_record(&open_refused(native)),
        &[
            ("processInitializationResult", json!(14)),
            ("stage", json!("reference-process-init-refused-pinned")),
            ("metadataPrepared", json!(false)),
            ("metadataStage", json!("effective-domain-prepared")),
            ("uriResult", Value::Null),
            ("uriVerified", json!(false)),
            ("moduleOpenResult", Value::Null),
        ],
    );
    session_check(&refused, true)?;
    rejects(
        &refused,
        &["domainResult", "effectiveDomain"],
        &Value::Null,
        |value| session_check(value, true),
    );
    for (field, value) in [
        ("domainResult", json!(14)),
        ("effectiveDomain", json!(-1)),
        ("uriResult", json!(0)),
        ("uriVerified", json!(true)),
        ("ownerPinnedUntilExit", json!(false)),
    ] {
        rejects(&refused, &[field], &value, |value| {
            session_check(value, true)
        });
    }
    assert!(
        session_check(
            &changed(
                &refused,
                &[
                    ("architectureRaw", json!(0x82)),
                    ("architecture", Value::Null)
                ]
            ),
            true
        )
        .is_err()
    );
    let uri_refused = changed(
        &refused,
        &[
            ("stage", json!("session-metadata-incomplete")),
            ("metadataStage", json!("session-uri-refused")),
            ("processInitializationResult", json!(0)),
            ("uriResult", json!(14)),
        ],
    );
    session_check(&uri_refused, true)?;
    let domain_refused = changed(
        &refused,
        &[
            ("stage", json!("session-metadata-incomplete")),
            ("metadataStage", json!("effective-domain-refused")),
            ("unsignedProcessRequested", json!(false)),
            ("processInitializationResult", Value::Null),
            ("processInitializationDomainAfter", Value::Null),
            ("processInitializationEnableAfter", Value::Null),
            ("ownerPinnedUntilExit", json!(false)),
            ("domainResult", json!(14)),
            ("effectiveDomain", Value::Null),
        ],
    );
    session_check(&domain_refused, true)?;
    Ok(())
}

#[test]
fn session_worker_binds_signed_inputs_and_distinguishes_entered_from_completed_calls() -> Result<()>
{
    let native = native()?;
    let worker = json!({"SchemaVersion":1,"Kind":"runner-hexagon-session-worker","SourceRevision":"1".repeat(40),"Outcome":"passed","Stage":"session-mapping-complete","Failure":null,"LastNativeStage":"module-close-entered","ReferenceProcessInitializationSelected":false,"UnsignedProcessRequested":false,"ProcessInitializationResult":null,"SignedPackageVerified":true,"RuntimeFilesVerified":16,"DriverIdentityVerified":true,"DriverIdentity":driver(),"MappingCycleVerified":true,"NativeResult":native,"ExecutableSha256":"4".repeat(64),"BuildReceiptSha256":"5".repeat(64),"ManifestSha256":"6".repeat(64),"LockSha256":"7".repeat(64),"ModelLoaded":false,"SwiftQualified":false,"ProfileEligible":false,"DriverInstalled":false,"TrustChanged":false,"ComputeExecuted":false});
    worker_check(&worker, true, false)?;
    rejects(
        &worker,
        &[
            "DriverIdentity",
            "NativeResult",
            "ExecutableSha256",
            "SignedPackageVerified",
        ],
        &Value::Null,
        |value| worker_check(value, true, false),
    );
    let partial = changed(
        &worker,
        &[
            ("Outcome", json!("pending")),
            ("Stage", json!("session-mapping")),
            ("MappingCycleVerified", json!(false)),
            ("NativeResult", Value::Null),
            ("LastNativeStage", json!("fd-mapping-entered")),
        ],
    );
    worker_check(&partial, true, false)?;
    let reference = changed(
        &worker,
        &[
            ("ReferenceProcessInitializationSelected", json!(true)),
            ("UnsignedProcessRequested", json!(true)),
            ("ProcessInitializationResult", json!(0)),
            ("NativeResult", reference_record(&native)),
        ],
    );
    worker_check(&reference, true, true)?;
    assert!(worker_check(&reference, true, false).is_err());
    let interrupted = changed(
        &reference,
        &[
            ("Outcome", json!("pending")),
            ("Stage", json!("session-mapping")),
            ("MappingCycleVerified", json!(false)),
            ("NativeResult", Value::Null),
            ("LastNativeStage", json!("reference-process-init-entered")),
            ("UnsignedProcessRequested", Value::Null),
            ("ProcessInitializationResult", Value::Null),
        ],
    );
    worker_check(&interrupted, true, true)?;
    rejects(
        &interrupted,
        &["UnsignedProcessRequested"],
        &json!(true),
        |value| worker_check(value, true, true),
    );
    Ok(())
}

#[test]
fn compute_results_require_every_step_and_keep_error_drain_ownership() -> Result<()> {
    let native = compute_record()?;
    compute_check(&native, "F32")?;
    let steps = [
        "domainResult",
        "uriResult",
        "processInitializationResult",
        "moduleOpenResult",
        "queueCreateResult",
        "queueExportResult",
        "queueStartResult",
        "queueWriteResult",
        "queueReadResult",
        "queueStopResult",
        "queueCloseResult",
        "mappingResult",
        "unmappingResult",
        "moduleCloseResult",
    ];
    for invalid in [Value::Null, json!("0"), json!(1), json!(0.0)] {
        rejects(&native, &steps, &invalid, |value| {
            compute_check(value, "F32")
        });
    }
    rejects(
        &native,
        &[
            "passed",
            "responseValidated",
            "numericValidated",
            "freeCompleted",
            "computeSubmitted",
            "computeExecuted",
            "ownerPinnedUntilExit",
        ],
        &json!("true"),
        |value| compute_check(value, "F32"),
    );
    rejects(
        &native,
        &[
            "responseValidated",
            "numericValidated",
            "freeCompleted",
            "computeSubmitted",
            "ownerPinnedUntilExit",
        ],
        &json!(false),
        |value| compute_check(value, "F32"),
    );
    rejects(
        &native,
        &["poisoned", "profileEligible", "sessionTeardownVerified"],
        &json!(true),
        |value| compute_check(value, "F32"),
    );
    let partial = changed(
        &native,
        &[
            ("passed", json!(false)),
            ("computeExecuted", json!(false)),
            ("numericValidated", json!(false)),
            ("responseValidated", json!(false)),
            ("freeCompleted", json!(false)),
            ("poisoned", json!(true)),
            ("queueReadResult", json!(39)),
        ],
    );
    compute_check(&partial, "F32")?;
    assert!(run(json!({"operation":"compute-result","json":native.to_string(),"exitCode":1,"case":"F32"})).is_err());
    compute_variants(&native)
}

fn compute_variants(native: &Value) -> Result<()> {
    for (case, kind) in [
        ("GuardedQ2", "native-fastrpc-queue-q2-probe"),
        ("F32NoBlock", "native-fastrpc-queue-noblock-probe"),
        ("GuardedQ2NoBlock", "native-fastrpc-queue-q2-noblock-probe"),
    ] {
        let mut record = changed(native, &[("kind", json!(kind))]);
        if case.starts_with("GuardedQ2") {
            for (field, value) in [
                ("columns", 256),
                ("rows", 32),
                ("batch", 2),
                ("outputValuesChecked", 64),
            ] {
                record[field] = value.into();
            }
        }
        if case.ends_with("NoBlock") {
            record["writeMode"] = json!("nonblocking");
        }
        compute_check(&record, case)?;
        assert!(compute_check(&record, "F32").is_err());
        assert!(compute_check(native, case).is_err());
        if case.starts_with("GuardedQ2") {
            rejects(
                &record,
                &["columns", "rows", "batch", "outputValuesChecked"],
                &Value::Null,
                |value| compute_check(value, case),
            );
        }
        if case.ends_with("NoBlock") {
            rejects(&record, &["writeMode"], &json!("timed"), |value| {
                compute_check(value, case)
            });
            let drained = changed(
                &record,
                &[
                    ("passed", json!(false)),
                    ("queueWriteResult", json!(14)),
                    ("poisoned", json!(true)),
                    ("freeCompleted", json!(false)),
                    ("queueStopResult", Value::Null),
                    ("queueCloseResult", Value::Null),
                    ("unmappingResult", Value::Null),
                    ("moduleCloseResult", Value::Null),
                ],
            );
            assert_eq!(compute_check(&drained, case)?["computeExecuted"], true);
            rejects(
                &drained,
                &[
                    "queueStopResult",
                    "queueCloseResult",
                    "unmappingResult",
                    "moduleCloseResult",
                ],
                &json!(0),
                |value| compute_check(value, case),
            );
            rejects(&drained, &["queueReadResult"], &Value::Null, |value| {
                compute_check(value, case)
            });
        }
    }
    Ok(())
}
