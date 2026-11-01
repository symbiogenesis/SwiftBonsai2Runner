//! Fixed dispatch and queued-computation evidence invariants.

use super::{Record, Result, compute_kind, driver_identity, require, windows_absolute};
use serde_json::Value;

fn exit(outcome: &str) -> Result<i32> {
    match outcome {
        "passed" => Ok(0),
        "pending" => Ok(2),
        "failed" => Ok(1),
        _ => Err("Invalid Hexagon outcome.".into()),
    }
}

pub(super) fn fixture(value: &Value, exit_code: i32) -> Result<()> {
    let r = Record(value);
    r.fixed("SchemaVersion", 1)?;
    r.equal("Kind", "runner-hexagon-dispatch-fixture")?;
    r.equal("SourceRevision", "94256114c229674ef96e76eb2dea596e65b43818")?;
    r.equal(
        "DispatchEvidence",
        "explicit-hexagon-compute-sync-and-full-output-oracle",
    )?;
    r.equal("SessionCleanup", "supervised-process-exit-required")?;
    r.label("Stage", 64)?;
    r.optional_label("Failure", 96)?;
    r.flags(
        &[
            "SelfTest",
            "CpuFallbackAllowed",
            "ModelLoaded",
            "SwiftQualified",
            "ProfileEligible",
        ],
        false,
    )?;
    let outcome = r.string("Outcome")?;
    let passed = outcome == "passed";
    require(
        exit_code == exit(outcome)? && r.boolean("DeviceDispatchVerified")? == passed,
        "Dispatch exit status and device result disagree.",
    )?;
    if let Some(code) = r.optional_integer("NativeCode")? {
        require(
            u32::try_from(code).is_ok(),
            "Invalid native dispatch error code.",
        )?;
    }
    if !r.get("BackendName")?.is_null() {
        r.equal("BackendName", "HTP0")?;
    }
    let cases = r.array("Cases")?;
    require(cases.len() <= 2, "Dispatch exceeded its fixed case count.")?;
    if passed {
        require(
            cases.len() == 2 && r.get("Failure")?.is_null() && r.get("NativeCode")?.is_null(),
            "Completed dispatch omitted fixed cases or retained failure.",
        )?;
        r.equal("BackendName", "HTP0")?;
        r.equal("Stage", "numeric-dispatch-complete")?;
    } else {
        require(
            !r.get("Failure")?.is_null(),
            "Incomplete dispatch omitted its failure reason.",
        )?;
    }
    let mut batches = std::collections::BTreeSet::new();
    for case in cases {
        let case = Record(case);
        r.equal("BackendName", "HTP0")?;
        case.fixed("Columns", 128)?;
        case.fixed("Rows", 32)?;
        let batch = case.integer("Batch")?;
        require(
            [1, 32].contains(&batch) && batches.insert(batch),
            "Dispatch batch identity changed.",
        )?;
        case.fixed("OutputValuesChecked", 32 * batch)?;
        case.flags(&["MatmulParity", "ExecutedOnHexagon"], true)?;
    }
    Ok(())
}

fn catalog(r: &Record<'_>) -> Result<bool> {
    let verified = r.boolean("CatalogMemberVerified")?;
    let count = r.integer("CatalogMembersVerified")?;
    let names = r.array("CatalogMemberNames")?;
    if verified {
        require(
            count == 4 && names.len() == 4,
            "Dispatch omitted reviewed catalog members.",
        )?;
        for (actual, expected) in names.iter().zip([
            "libggml-htp-v73.so",
            "libggml-htp-v75.so",
            "libggml-htp-v79.so",
            "libggml-htp-v81.so",
        ]) {
            require(
                actual.as_str() == Some(expected),
                "Dispatch catalog member identity changed.",
            )?;
        }
    } else {
        require(
            count == 0 && names.is_empty(),
            "Unverified catalog membership was counted.",
        )?;
    }
    Ok(verified)
}

pub(super) fn worker(value: &Value, revision: &str) -> Result<()> {
    let r = Record(value);
    r.fixed("SchemaVersion", 1)?;
    r.equal("Kind", "runner-hexagon-dispatch-worker")?;
    r.equal("SourceRevision", revision)?;
    let outcome = r.string("Outcome")?;
    exit(outcome)?;
    r.label("Stage", 64)?;
    require(
        [
            "not-started",
            "load",
            "registry",
            "backend-init",
            "context",
            "allocation",
            "transfer",
            "dispatch",
            "readback",
        ]
        .contains(&r.string("LastFixtureStage")?),
        "Invalid fixture progress stage.",
    )?;
    r.optional_label("Failure", 96)?;
    r.equal("SelectedDspArchitecture", "unknown")?;
    let observed = r.string("LoadedDriverObservation")?;
    require(
        [
            "not-started",
            "not-observed",
            "observed-verified",
            "observation-rejected",
        ]
        .contains(&observed),
        "Invalid loaded-driver observation stage.",
    )?;
    if observed == "observed-verified" {
        driver_identity(r.get("LoadedDriverIdentity")?)?;
    } else {
        require(
            r.get("LoadedDriverIdentity")?.is_null(),
            "Unverified loaded driver identity was retained.",
        )?;
    }
    r.flags(
        &[
            "ModelLoaded",
            "SwiftQualified",
            "ProfileEligible",
            "DriverInstalled",
            "TrustChanged",
        ],
        false,
    )?;
    let passed = r.boolean("DeviceDispatchVerified")?;
    require(
        passed == (outcome == "passed"),
        "Dispatch completion differs from outcome.",
    )?;
    let files = r.integer("RuntimeFilesVerified")?;
    require((0..=16).contains(&files), "Invalid dispatch runtime count.")?;
    let verified = catalog(&r)?;
    r.optional_digests(
        &[
            "FixtureSha256",
            "FixtureSourceSha256",
            "ManifestSha256",
            "LockSha256",
            "SigntoolSha256",
        ],
        passed,
    )?;
    let native = r.get("FixtureResult")?;
    if native.is_null() {
        require(
            !passed && outcome != "failed",
            "Dispatch claimed execution without fixture evidence.",
        )?;
    } else {
        fixture(native, exit(Record(native).string("Outcome")?)?)?;
        for name in ["Outcome", "Stage", "Failure"] {
            require(
                r.get(name)? == Record(native).get(name)?,
                "Dispatch worker and fixture disagree.",
            )?;
        }
    }
    require(
        !passed || (verified && files == 16),
        "Completed dispatch omitted verified runtime inputs.",
    )
}

pub(super) fn compute(value: &Value, exit_code: i32, case: &str) -> Result<()> {
    let r = Record(value);
    require(exit_code == 0, "Native computation process failed.")?;
    r.fixed("schemaVersion", 1)?;
    r.equal("kind", compute_kind(case)?)?;
    r.label("stage", 96)?;
    require(
        windows_absolute(r.string("driverDll")?),
        "Invalid native computation driver path.",
    )?;
    if case.ends_with("NoBlock") {
        r.equal("writeMode", "nonblocking")?;
    }
    if case.starts_with("GuardedQ2") {
        for (name, fixed) in [
            ("columns", 256),
            ("rows", 32),
            ("batch", 2),
            ("outputValuesChecked", 64),
        ] {
            r.fixed(name, fixed)?;
        }
    }
    for name in [
        "passed",
        "uriVerified",
        "unsignedProcessRequested",
        "handleObserved",
        "responseValidated",
        "numericValidated",
        "freeCompleted",
        "ownerPinnedUntilExit",
        "poisoned",
        "computeSubmitted",
        "computeExecuted",
        "profileEligible",
        "sessionTeardownVerified",
    ] {
        r.boolean(name)?;
    }
    r.flags(&["profileEligible", "sessionTeardownVerified"], false)?;
    if r.boolean("computeExecuted")? {
        r.flags(
            &["computeSubmitted", "numericValidated", "responseValidated"],
            true,
        )?;
        r.fixed("queueReadResult", 0)?;
        r.integer("queueWriteResult")?;
    }
    if r.boolean("computeSubmitted")?
        && r.optional_integer("queueWriteResult")?
            .is_some_and(|code| code != 0)
    {
        r.flags(&["poisoned", "ownerPinnedUntilExit"], true)?;
        r.flags(&["freeCompleted"], false)?;
        for name in [
            "queueStopResult",
            "queueCloseResult",
            "unmappingResult",
            "moduleCloseResult",
        ] {
            require(
                r.get(name)?.is_null(),
                "A write-error observation attempted normal cleanup.",
            )?;
        }
    }
    if r.boolean("passed")? {
        complete_compute(&r)?;
    }
    Ok(())
}

fn complete_compute(r: &Record<'_>) -> Result<()> {
    r.flags(
        &[
            "uriVerified",
            "unsignedProcessRequested",
            "handleObserved",
            "responseValidated",
            "numericValidated",
            "freeCompleted",
            "ownerPinnedUntilExit",
            "computeSubmitted",
            "computeExecuted",
        ],
        true,
    )?;
    for name in [
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
    ] {
        r.fixed(name, 0)?;
    }
    r.fixed("architecture", 81)?;
    r.fixed("effectiveDomain", 3)?;
    r.flags(&["poisoned"], false)?;
    Ok(())
}
