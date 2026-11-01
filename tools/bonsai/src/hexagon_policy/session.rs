//! Native session state transitions remain separate from device qualification.

use super::{Record, Result, driver_identity, driver_path, require};
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
#[expect(
    clippy::struct_excessive_bools,
    reason = "These are independent flags in the existing native observation ABI, not mutable application state."
)]
struct Native {
    passed: bool,
    stage: String,
    metadata_stage: String,
    metadata_prepared: bool,
    uri_verified: bool,
    handle_observed: bool,
    remote_module_opened: bool,
    allocation_succeeded: bool,
    descriptor_observed: bool,
    cpu_roundtrip_verified: bool,
    mapping_attempted: bool,
    free_completed: bool,
    poisoned: bool,
    module_close_completed: bool,
    owner_pinned_until_exit: bool,
    unsigned_process_requested: bool,
    reference_process_initialization_selected: bool,
    architecture_raw: u32,
    #[serde(deserialize_with = "Option::deserialize")]
    architecture: Option<i64>,
    #[serde(deserialize_with = "Option::deserialize")]
    domain_result: Option<i32>,
    #[serde(deserialize_with = "Option::deserialize")]
    effective_domain: Option<i32>,
    #[serde(deserialize_with = "Option::deserialize")]
    uri_result: Option<i32>,
    #[serde(deserialize_with = "Option::deserialize")]
    module_open_result: Option<i32>,
    #[serde(deserialize_with = "Option::deserialize")]
    descriptor_error: Option<i32>,
    #[serde(deserialize_with = "Option::deserialize")]
    mapping_result: Option<i32>,
    #[serde(deserialize_with = "Option::deserialize")]
    unmapping_result: Option<i32>,
    #[serde(deserialize_with = "Option::deserialize")]
    module_close_result: Option<i32>,
    #[serde(deserialize_with = "Option::deserialize")]
    process_initialization_result: Option<i32>,
    #[serde(deserialize_with = "Option::deserialize")]
    process_initialization_domain_after: Option<i32>,
    #[serde(deserialize_with = "Option::deserialize")]
    process_initialization_enable_after: Option<i32>,
}

fn metadata(n: &Native, reference: bool) -> Result<bool> {
    let low = n.architecture_raw & 255;
    let decoded = i64::from((low >> 4) * 10 + (low & 15));
    let supported = (1..=9).contains(&(low >> 4)) && low & 15 <= 9 && [73, 81].contains(&decoded);
    require(
        n.architecture == supported.then_some(decoded),
        "Native architecture contradicts the raw capability.",
    )?;
    let domain = supported
        && n.domain_result == Some(0)
        && n.effective_domain.is_some_and(|domain| domain >= 0);
    let metadata = domain && n.uri_result == Some(0) && n.uri_verified;
    require(
        n.metadata_prepared == metadata
            && (!metadata || n.metadata_stage == "session-metadata-prepared"),
        "Native session metadata is contradictory.",
    )?;
    let initialization = n.process_initialization_result.is_some();
    require(
        initialization == n.process_initialization_domain_after.is_some()
            && initialization == n.process_initialization_enable_after.is_some(),
        "Process initialization returned-field presence contradicts its request.",
    )?;
    require(
        n.reference_process_initialization_selected == reference
            && n.unsigned_process_requested == initialization
            && (!initialization || (reference && domain))
            && (!reference || n.uri_result.is_none() || n.process_initialization_result == Some(0))
            && (!n.uri_verified || (n.uri_result == Some(0) && domain))
            && (!initialization
                || n.process_initialization_result == Some(0)
                || (n.uri_result.is_none() && !n.uri_verified))
            && (!reference
                || n.module_open_result.is_none()
                || n.process_initialization_result == Some(0)),
        "Native reference-process initialization selection or outcome is contradictory.",
    )?;
    Ok(metadata)
}

fn mapping(n: &Native, metadata: bool) -> Result<bool> {
    let open = n.module_open_result.is_some();
    let opened = n.module_open_result == Some(0) && n.handle_observed && metadata;
    require(
        n.remote_module_opened == opened
            && n.owner_pinned_until_exit == (open || n.process_initialization_result.is_some())
            && (!open || metadata)
            && (!n.handle_observed || open)
            && (!n.allocation_succeeded || opened),
        "Native session open evidence is contradictory.",
    )?;
    require(
        (!n.descriptor_observed || n.allocation_succeeded)
            && n.descriptor_error
                .is_none_or(|error| error < 0 && n.allocation_succeeded && !n.descriptor_observed)
            && (n.allocation_succeeded && !n.descriptor_observed) == n.descriptor_error.is_some()
            && (!n.cpu_roundtrip_verified || n.descriptor_observed),
        "Native allocation evidence is contradictory.",
    )?;
    require(
        n.mapping_attempted == n.mapping_result.is_some()
            && (!n.mapping_attempted
                || (opened && n.cpu_roundtrip_verified && n.descriptor_observed))
            && (n.unmapping_result.is_none() || n.mapping_result == Some(0))
            && (!n.free_completed
                || (n.allocation_succeeded
                    && (!n.mapping_attempted || n.unmapping_result == Some(0)))),
        "Native session mapping evidence is contradictory.",
    )?;
    let uncertain = n.mapping_attempted
        && (n.mapping_result != Some(0) || n.unmapping_result.is_some_and(|code| code != 0));
    require(
        n.poisoned == uncertain
            && (!uncertain || (!n.free_completed && n.module_close_result.is_none()))
            && (n.module_close_result.is_none()
                || (opened && (!n.allocation_succeeded || n.free_completed)))
            && n.module_close_completed == (n.module_close_result == Some(0)),
        "Native session cleanup evidence is contradictory.",
    )?;
    Ok(opened)
}

fn terminal(n: &Native, reference: bool, metadata: bool, opened: bool) -> Result<()> {
    let initialization = n.process_initialization_result.is_some();
    let open = n.module_open_result.is_some();
    let passed = (!reference || n.process_initialization_result == Some(0))
        && metadata
        && opened
        && n.allocation_succeeded
        && n.descriptor_observed
        && n.cpu_roundtrip_verified
        && n.mapping_attempted
        && n.mapping_result == Some(0)
        && n.unmapping_result == Some(0)
        && n.free_completed
        && !n.poisoned
        && n.module_close_completed
        && n.owner_pinned_until_exit;
    require(
        n.passed == passed && passed == (n.stage == "session-fd-cycle-completed-owner-pinned"),
        "Native session completion is contradictory.",
    )?;
    let matches = match n.stage.as_str() {
        "session-metadata-incomplete" => {
            !metadata && !open && (!initialization || n.process_initialization_result == Some(0))
        }
        "reference-process-init-refused-pinned" => {
            reference
                && initialization
                && n.process_initialization_result != Some(0)
                && n.uri_result.is_none()
                && !n.uri_verified
                && !open
        }
        "module-open-refused-pinned" => open && n.module_open_result != Some(0) && !opened,
        "module-handle-not-written-pinned" => {
            n.module_open_result == Some(0) && !n.handle_observed && !opened
        }
        "allocation-refused" => {
            opened && !n.allocation_succeeded && n.module_close_result.is_some()
        }
        "descriptor-refused" => {
            n.allocation_succeeded
                && !n.descriptor_observed
                && n.descriptor_error.is_some_and(|error| error < 0)
                && n.free_completed
                && n.module_close_result.is_some()
        }
        "allocation-not-page-aligned" | "cpu-roundtrip-failed" => {
            n.allocation_succeeded
                && n.descriptor_observed
                && !n.cpu_roundtrip_verified
                && !n.mapping_attempted
                && n.free_completed
                && n.module_close_result.is_some()
        }
        "fd-mapping-failed-pinned" => {
            n.mapping_attempted && n.mapping_result != Some(0) && n.poisoned
        }
        "fd-unmapping-failed-pinned" => {
            n.mapping_attempted
                && n.mapping_result == Some(0)
                && n.unmapping_result.is_some_and(|code| code != 0)
                && n.poisoned
        }
        "session-fd-cycle-completed-owner-pinned" => passed,
        "module-close-refused-pinned" => {
            n.free_completed
                && n.mapping_attempted
                && n.unmapping_result == Some(0)
                && n.module_close_result.is_some_and(|code| code != 0)
        }
        _ => false,
    };
    require(
        matches,
        "Native session terminal stage contradicts its raw outcomes.",
    )
}

pub(super) fn probe(value: &Value, exit_code: i32, reference: bool) -> Result<()> {
    require(exit_code == 0, "Native session probe process failed.")?;
    let r = Record(value);
    r.fixed("schemaVersion", 1)?;
    r.equal("kind", "native-fastrpc-session-mapping-probe")?;
    require(
        driver_path(r.string("driverDll")?),
        "Invalid native session driver path.",
    )?;
    r.label("metadataStage", 64)?;
    r.flags(
        &[
            "signedDeploymentVerified",
            "computeExecuted",
            "dmaExecuted",
            "sessionTeardownVerified",
            "profileEligible",
        ],
        false,
    )?;
    r.flags(&["driverLoaded"], true)?;
    for (name, fixed) in [
        ("sessionId", 0),
        ("domainRequestId", 14),
        ("uriRequestId", 15),
        ("bytes", 4096),
        ("heapId", 25),
        ("allocationFlags", 1),
        ("mapFlags", 2),
        ("processInitializationRequestId", 2),
        ("processInitializationEnable", 1),
        ("processInitializationStructureBytes", 8),
    ] {
        r.fixed(name, fixed)?;
    }
    let native: Native =
        serde_json::from_value(value.clone()).map_err(|error| error.to_string())?;
    let metadata = metadata(&native, reference)?;
    let opened = mapping(&native, metadata)?;
    terminal(&native, reference, metadata, opened)
}

fn worker_reference(r: &Record<'_>, reference: bool, driver_verified: bool) -> Result<()> {
    require(
        r.boolean("ReferenceProcessInitializationSelected")? == reference,
        "Native worker reference selection differs.",
    )?;
    let request = r.get("UnsignedProcessRequested")?;
    let requested = if request.is_null() {
        None
    } else {
        Some(r.boolean("UnsignedProcessRequested")?)
    };
    let result = r.optional_integer("ProcessInitializationResult")?;
    require(
        result.is_none_or(|code| i32::try_from(code).is_ok()),
        "Invalid native worker initialization result.",
    )?;
    if !reference {
        require(
            requested == Some(false)
                && result.is_none()
                && r.string("LastNativeStage")? != "reference-process-init-entered",
            "Default session claimed an unselected reference request.",
        )?;
    }
    if reference && requested != Some(false) {
        require(
            driver_verified
                && ["session-mapping", "session-mapping-complete"].contains(&r.string("Stage")?),
            "Reference request escaped signed and guarded execution.",
        )?;
    }
    require(
        result.is_none() || requested == Some(true),
        "Native initialization outcome omitted its request.",
    )
}

fn worker_identity(r: &Record<'_>, revision: &str) -> Result<()> {
    r.fixed("SchemaVersion", 1)?;
    r.equal("Kind", "runner-hexagon-session-worker")?;
    r.equal("SourceRevision", revision)?;
    require(
        ["passed", "pending"].contains(&r.string("Outcome")?),
        "Invalid session worker outcome.",
    )?;
    require(
        [
            "source-identity",
            "runtime-identity",
            "catalog-verification",
            "driver-inspection",
            "session-mapping",
            "session-mapping-complete",
        ]
        .contains(&r.string("Stage")?),
        "Invalid session worker stage.",
    )?;
    require(
        [
            "not-started",
            "reference-process-init-entered",
            "module-open-entered",
            "allocation-entered",
            "descriptor-entered",
            "fd-mapping-entered",
            "fd-unmapping-entered",
            "free-entered",
            "module-close-entered",
        ]
        .contains(&r.string("LastNativeStage")?),
        "Invalid native session progress marker.",
    )?;
    r.optional_label("Failure", 96)?;
    r.flags(
        &[
            "ModelLoaded",
            "SwiftQualified",
            "ProfileEligible",
            "DriverInstalled",
            "TrustChanged",
            "ComputeExecuted",
        ],
        false,
    )?;
    Ok(())
}

pub(super) fn worker(value: &Value, revision: &str, reference: bool) -> Result<()> {
    let r = Record(value);
    worker_identity(&r, revision)?;
    let signed = r.boolean("SignedPackageVerified")?;
    let driver = r.boolean("DriverIdentityVerified")?;
    let passed = r.boolean("MappingCycleVerified")?;
    worker_reference(&r, reference, driver)?;
    let files = r.integer("RuntimeFilesVerified")?;
    require(
        (0..=16).contains(&files) && (!signed || files == 16),
        "Invalid native package verification count.",
    )?;
    if driver {
        require(signed, "Verified driver omitted signed-package evidence.")?;
        driver_identity(r.get("DriverIdentity")?)?;
    } else {
        require(
            r.get("DriverIdentity")?.is_null(),
            "Unverified native driver identity was retained.",
        )?;
    }
    r.optional_digests(
        &[
            "ExecutableSha256",
            "BuildReceiptSha256",
            "ManifestSha256",
            "LockSha256",
        ],
        signed,
    )?;
    let native = r.get("NativeResult")?;
    if native.is_null() {
        require(
            !passed
                && r.get("ProcessInitializationResult")?.is_null()
                && r.get("UnsignedProcessRequested")?.as_bool() != Some(true),
            "Native mapping or initialization claimed without complete result.",
        )?;
    } else {
        probe(native, 0, reference)?;
        let native = Record(native);
        require(
            driver
                && native.string("driverDll")?
                    == Record(r.get("DriverIdentity")?).string("Path")?
                && passed == native.boolean("passed")?
                && !r.get("UnsignedProcessRequested")?.is_null()
                && r.boolean("UnsignedProcessRequested")?
                    == native.boolean("unsignedProcessRequested")?
                && r.get("ProcessInitializationResult")?
                    == native.get("processInitializationResult")?,
            "Native result contradicts guarded driver evidence.",
        )?;
    }
    require(
        passed == (r.string("Outcome")? == "passed")
            && (!passed
                || (r.string("Stage")? == "session-mapping-complete"
                    && r.get("Failure")?.is_null())),
        "Native session worker completion contradicts its evidence.",
    )
}
