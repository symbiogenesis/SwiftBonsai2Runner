//! Original-clock owner CI admission and receipts. No clock restart or performance promotion.
#![cfg(any(windows, test))]
#[cfg(windows)]
use super::{device, hold, plain, preflight, write_json};
use crate::Result;
use serde::Deserialize;
use serde_json::Value;
#[cfg(windows)]
use serde_json::json;
#[cfg(windows)]
use std::{fs, io::Read as _, path::PathBuf, time::Instant};

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Kind {
    Inventory,
    Session,
    Dispatch,
}

fn allowance(elapsed_ticks: u64, kind: Kind) -> Result<u32> {
    let remaining = (270_u64 * 10_000_000).saturating_sub(elapsed_ticks) / 10_000_000;
    let maximum = match kind {
        Kind::Inventory => 45,
        _ => 120,
    };
    let value = u32::try_from(remaining.min(maximum)).map_err(|error| error.to_string())?;
    let minimum = match kind {
        Kind::Inventory => 5,
        _ => 76,
    };
    if value < minimum {
        return Err("Original owner setup exhausted the fixed cleanup/evidence reserve; work remains pending.".into());
    }
    Ok(value)
}

fn intake_identity(receipt: &Value, revision: &str, run: &str, attempt: &str) -> Result<()> {
    if receipt["RunnerRevision"] != revision
        || receipt["RunId"] != run
        || receipt["RunAttempt"] != attempt
    {
        return Err(
            "Fixture intake differs from this exact trusted hosted run and revision.".into(),
        );
    }
    Ok(())
}

#[cfg(windows)]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Request {
    kind: Kind,
    repository_directory: PathBuf,
    output_directory: PathBuf,
    #[serde(default)]
    fixture_directory: Option<PathBuf>,
    #[serde(default)]
    package_directory: Option<PathBuf>,
    #[serde(default)]
    signtool_path: Option<PathBuf>,
    #[serde(default)]
    reference_initialization: Option<String>,
    whole_job_maximum_seconds: u32,
}
#[cfg(windows)]
struct Clock {
    original: u64,
    entry_ticks: u64,
    entry: Instant,
    started: String,
}
#[cfg(windows)]
impl Clock {
    fn new() -> Result<Self> {
        let started = std::env::var("BONSAI_HEXAGON_JOB_STARTED_UTC")
            .map_err(|_| "Original owner job clock is missing.")?;
        let original = crate::native_acceptance_budget::utc_ticks(&started)?;
        let entry_ticks =
            crate::native_acceptance_budget::utc_ticks(&bonsai_windows_process::utc_now())?;
        if entry_ticks < original {
            return Err("Original owner clock cannot begin in the future.".into());
        }
        Ok(Self {
            original,
            entry_ticks,
            entry: Instant::now(),
            started,
        })
    }
    fn elapsed(&self) -> Result<u64> {
        let wall = crate::native_acceptance_budget::utc_ticks(&bonsai_windows_process::utc_now())?
            .checked_sub(self.original)
            .ok_or("Original owner wall clock moved before its start.")?;
        let monotonic = u64::try_from(self.entry.elapsed().as_nanos() / 100)
            .map_err(|error| error.to_string())?;
        let charged = self
            .entry_ticks
            .checked_sub(self.original)
            .and_then(|value| value.checked_add(monotonic))
            .ok_or("Owner elapsed clock overflow.")?;
        Ok(wall.max(charged))
    }
}
#[cfg(windows)]
fn power() -> Value {
    let utc = bonsai_windows_process::utc_now();
    bonsai_windows_process::power_status().map_or_else(|_|json!({"ObservedUtc":utc,"Status":"unknown","Source":"GetSystemPowerStatus","AcOnline":null,"BatterySaver":null,"BatteryPercent":null}),|power|json!({"ObservedUtc":utc,"Status":"observed","Source":"GetSystemPowerStatus","AcOnline":power.ac_online,"BatterySaver":power.battery_saver,"BatteryPercent":power.battery_percent}))
}
#[cfg(windows)]
fn digest(path: &std::path::Path) -> Result<String> {
    hold(path, 4 * 1024 * 1024).map(|(_, digest)| digest)
}
#[cfg(windows)]
fn same_run(request: &Request, revision: &str, run: &str, attempt: &str) -> Result<()> {
    if matches!(request.kind, Kind::Inventory) {
        return Ok(());
    }
    let bundle = request
        .fixture_directory
        .as_ref()
        .ok_or("Exact same-run fixture bundle required.")?;
    let name = match request.kind {
        Kind::Session => "native-build.json",
        _ => "fixture-build.json",
    };
    let (mut input, _) = hold(&bundle.join(name), 1_048_576)?;
    let mut raw = String::new();
    input
        .read_to_string(&mut raw)
        .map_err(|error| error.to_string())?;
    let receipt = crate::hexagon_policy::data_json(&raw)?;
    intake_identity(&receipt, revision, run, attempt)
}
#[cfg(windows)]
fn selected(request: &Request, maximum: u32, revision: &str) -> Result<Value> {
    if matches!(request.kind, Kind::Inventory) {
        fs::create_dir(&request.output_directory).map_err(|error| error.to_string())?;
        return preflight::supervise(
            request.package_directory.as_deref(),
            Some(request.output_directory.join("preflight.json")),
            maximum,
        );
    }
    let reference = match request.reference_initialization.as_deref() {
        Some("true") if matches!(request.kind, Kind::Session) => true,
        Some("false") => false,
        None if matches!(request.kind, Kind::Dispatch) => false,
        _ => return Err(
            "Reference initialization selection must be explicit and apply only to session probes."
                .into(),
        ),
    };
    let probe:device::Probe=serde_json::from_value(json!({"kind":match request.kind{Kind::Session=>"session",_=>"dispatch"},"repositoryDirectory":request.repository_directory,"packageDirectory":request.package_directory.clone().unwrap_or_default(),"fixtureDirectory":request.fixture_directory,"outputDirectory":request.output_directory,"signtoolPath":request.signtool_path,"expectedRunnerRevision":revision,"maximumSeconds":maximum,"referenceInitialization":reference})).map_err(|error|error.to_string())?;
    device::supervise(&probe)
}
#[cfg(windows)]
pub(super) fn run(request: &Request) -> Result<Value> {
    if !cfg!(target_arch = "aarch64")
        || request.whole_job_maximum_seconds != 300
        || std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
        || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("self-hosted")
    {
        return Err(
            "Owner operation requires the fixed native Windows ARM64 self-hosted 300-second lane."
                .into(),
        );
    }
    let clock = Clock::new()?;
    plain(&request.repository_directory)?;
    bonsai_artifacts::validate_external_path(&request.output_directory)
        .map_err(|error| error.to_string())?;
    if request.output_directory.exists() {
        return Err("Owner evidence directory must be new external storage.".into());
    }
    let revision = std::env::var("GITHUB_SHA").unwrap_or_default();
    let run = std::env::var("GITHUB_RUN_ID").unwrap_or_default();
    let attempt = std::env::var("GITHUB_RUN_ATTEMPT").unwrap_or_default();
    if !crate::identity::is_lower_hex(&revision, 40)
        || [&run, &attempt].iter().any(|value| {
            value.is_empty() || value.len() > 32 || !value.bytes().all(|byte| byte.is_ascii_digit())
        })
    {
        return Err("Exact owner source/run identities required.".into());
    }
    same_run(request, &revision, &run, &attempt)?;
    let before = power();
    let preparation = clock.elapsed()?;
    let maximum = allowance(preparation, request.kind)?;
    let result = selected(request, maximum, &revision);
    let after = power();
    let elapsed = clock.elapsed();
    if !request.output_directory.exists() {
        fs::create_dir(&request.output_directory).map_err(|error| error.to_string())?;
    }
    let mut host = json!({"SchemaVersion":1,"RunnerRevision":revision,"RunId":run,"RunAttempt":attempt,"JobStartedUtc":clock.started,"WholeJobMaximumSeconds":300,"PreparationElapsedMilliseconds":preparation.div_ceil(10000),"PostProcessReserveSeconds":30,"TrustedSameRunIntake":!matches!(request.kind,Kind::Inventory),"PowerBefore":before,"PowerAfter":after,"PowerAtOriginalJobStart":"unknown; native helper was unavailable before checkout/intake","PowerContinuity":"endpoint snapshots do not prove uninterrupted stability","SwiftQualified":false,"ProfileEligible":false,"PerformanceComparisonQualified":false,"Outcome":"pending","ElapsedMilliseconds":elapsed.as_ref().ok().map(|ticks|ticks.div_ceil(10000)),"OwnerSupervisorSha256":digest(&request.repository_directory.join("tools/bonsai/src/hexagon_tools/owner.rs"))?});
    match request.kind {
        Kind::Inventory => {
            host["SourceRevision"] = json!(revision);
            host["InventoryAllowanceSeconds"] = json!(maximum);
            host["ExecutionQualified"] = json!(false);
            host["SupervisorSha256"] = json!(digest(
                &request
                    .repository_directory
                    .join("tools/bonsai/src/hexagon_tools/preflight.rs")
            )?);
            host["ExperimentLockSha256"] = json!(digest(
                &request
                    .repository_directory
                    .join("config/hexagon-experiment.lock.json")
            )?);
            if request.output_directory.join("preflight.json").is_file() {
                host["ReceiptSha256"] =
                    json!(digest(&request.output_directory.join("preflight.json"))?);
            }
        }
        Kind::Session => {
            host["ProbeAggregateAllowanceSeconds"] = json!(maximum);
            host["ReferenceInitializationSelected"] =
                json!(request.reference_initialization.as_deref() == Some("true"));
        }
        Kind::Dispatch => {
            host["DispatchAggregateAllowanceSeconds"] = json!(maximum);
        }
    }
    if let Ok(value) = &result {
        if let Some(outcome) = value["Outcome"].as_str() {
            host["Outcome"] = json!(outcome);
        }
        host["CollectionStatus"] = value["CollectionStatus"].clone();
        host["DeviceDispatchVerified"] = value["DeviceDispatchVerified"].clone();
    } else if let Err(error) = &result {
        host["Outcome"] = json!("failed");
        host["Failure"] = json!(error.chars().take(512).collect::<String>());
    }
    if let Err(error) = &elapsed {
        host["ClockFailure"] = json!(error);
    }
    write_json(&request.output_directory.join("host.json"), &host)?;
    if elapsed? > 300 * 10_000_000 {
        return Err(
            "Original owner aggregate allowance expired; no workload retry is authorized.".into(),
        );
    }
    let value = result?;
    let passed = match request.kind {
        Kind::Inventory => value["CollectionStatus"] == "collected",
        Kind::Session => value["Outcome"] == "passed",
        Kind::Dispatch => value["Outcome"] == "passed" && value["DeviceDispatchVerified"] == true,
    };
    if !passed {
        return Err("Selected owner access/inventory gate remains pending; inspect retained stage evidence.".into());
    }
    Ok(host)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn owner_budget_keeps_original_fractional_setup_and_fixed_reserves() -> Result<()> {
        assert_eq!(allowance(0, Kind::Inventory)?, 45);
        assert_eq!(allowance(0, Kind::Session)?, 120);
        assert_eq!(allowance(194 * 10_000_000, Kind::Dispatch)?, 76);
        assert!(allowance(194 * 10_000_000 + 1, Kind::Dispatch).is_err());
        assert_eq!(allowance(265 * 10_000_000, Kind::Inventory)?, 5);
        assert!(allowance(265 * 10_000_000 + 1, Kind::Inventory).is_err());
        assert!(allowance(u64::MAX, Kind::Session).is_err());
        Ok(())
    }
    #[test]
    fn same_run_admission_rejects_changed_and_coerced_identities() -> Result<()> {
        let receipt = json!({"RunnerRevision":"a".repeat(40),"RunId":"123","RunAttempt":"1"});
        intake_identity(&receipt, &"a".repeat(40), "123", "1")?;
        for (field, value) in [
            ("RunnerRevision", json!("b".repeat(40))),
            ("RunId", json!(124)),
            ("RunAttempt", json!(true)),
        ] {
            let mut changed = receipt.clone();
            changed[field] = value;
            assert!(intake_identity(&changed, &"a".repeat(40), "123", "1").is_err());
        }
        Ok(())
    }
}
