use super::*;
use crate::hardware::{Objective, ProfileCompatibilityKey, ProfileEvidence};
use serde_json::json;

const GIB: u64 = 1024 * 1024 * 1024;

struct Evidence {
    key: ProfileCompatibilityKey,
    receipt: ProfileEvidence,
    comparison: String,
}

impl Evidence {
    fn new() -> Result<Self, String> {
        let key = ProfileCompatibilityKey::from_json(
            &json!({"schema_version":1, "objective":Objective::Interactive,
                "model_sha256":"a".repeat(64), "adapter":{"kind":"none"},
                "runtime":{"archive_sha256":"b".repeat(64),"source_revision":"c".repeat(40),
                    "target":{"architecture":"aarch64","os":"windows","environment":"msvc"}},
                "capability_class_sha256":"d".repeat(64),"backend_driver_sha256":"e".repeat(64),
                "graph_contract":"swift-pq2","state_layout":"hybrid-fp32-fp16",
                "settings_sha256":"f".repeat(64),
                "workload":{"context_tokens":8192,"minimum_prompt_tokens":1,
                    "maximum_prompt_tokens":4096,"maximum_output_tokens":4096,"state":"fresh"}})
            .to_string(),
        )?;
        let receipt = ProfileEvidence::from_json(
            &json!({"schema_version":1,"compatibility":key,
                "tested_main_revision":"1".repeat(40),"ci_run_id":"123",
                "capture_sha256":"2".repeat(64),"verdict":"passed","energy_verdict":"unknown"})
            .to_string(),
        )?;
        Ok(Self {
            key,
            receipt,
            comparison: "3".repeat(64),
        })
    }

    fn profile(&self, id: &'static str, backend: Backend, latency: u64) -> Profile<'_> {
        Profile {
            observation: Observation {
                id,
                current: &self.key,
                evidence: &self.receipt,
                independently_reviewed: true,
                ready_and_admitted: true,
                placement: Placement {
                    prefill: backend,
                    decode: backend,
                },
                phase_handoff_reviewed: false,
                comparison_sha256: &self.comparison,
                complete_task_ns: latency,
                upper_task_ns: latency,
                energy_uj: None,
                upper_energy_uj: None,
            },
            resources: ResourceCharge {
                job_commit_bytes: 8 * GIB,
                system_commit_bytes: 9 * GIB,
                physical_bytes: if backend == Backend::Cpu {
                    10 * GIB
                } else {
                    14 * GIB
                },
                adreno_bytes: if backend == Backend::Adreno {
                    4 * GIB
                } else {
                    0
                },
                hexagon_bytes: if backend == Backend::Hexagon {
                    4 * GIB
                } else {
                    0
                },
            },
            backend: BackendStatus::Ready,
        }
    }
}

fn budgets() -> LiveBudgets {
    LiveBudgets {
        available_physical_bytes: Some(24 * GIB),
        available_commit_bytes: Some(20 * GIB),
        job_limit_bytes: 12 * GIB,
        adreno_available_bytes: Some(5 * GIB),
        hexagon_available_bytes: Some(5 * GIB),
    }
}

fn reservation() -> Reservation {
    Reservation {
        prompt_tokens: 100,
        output_tokens: 4096,
        state: StateUse::Fresh,
    }
}

#[test]
fn changed_live_budgets_choose_eligible_plan_without_reducing_output() -> Result<(), String> {
    let fixture = Evidence::new()?;
    let cpu = fixture.profile("cpu", Backend::Cpu, 100);
    let npu = fixture.profile("npu", Backend::Hexagon, 60);
    let gpu = fixture.profile("gpu", Backend::Adreno, 70);
    let selection = select(
        &cpu,
        &[npu, gpu],
        reservation(),
        budgets(),
        Intent::Automatic,
    )?;
    assert_eq!(selection.recommendation.profile_id, "npu");
    assert_eq!(selection.reservation.output_tokens, 4096);
    assert_eq!(selection.resources, npu.resources);
    assert_eq!(selection.fallback_reason, None);

    let changed = LiveBudgets {
        hexagon_available_bytes: None,
        ..budgets()
    };
    let selection = select(&cpu, &[npu, gpu], reservation(), changed, Intent::Automatic)?;
    assert_eq!(selection.recommendation.profile_id, "gpu");
    assert_eq!(
        selection.fallback_reason,
        Some(FallbackReason::InsufficientMemory)
    );
    let changed = LiveBudgets {
        available_physical_bytes: Some(17 * GIB),
        ..changed
    };
    let selection = select(&cpu, &[gpu, npu], reservation(), changed, Intent::Automatic)?;
    assert_eq!(selection.recommendation.profile_id, "cpu");
    assert_eq!(selection.reservation, reservation());
    Ok(())
}

#[test]
fn outside_job_commit_physical_reserve_and_unknowns_are_independent() -> Result<(), String> {
    let fixture = Evidence::new()?;
    let cpu = fixture.profile("cpu", Backend::Cpu, 100);
    let npu = fixture.profile("npu", Backend::Hexagon, 60);
    for insufficient in [
        LiveBudgets {
            available_commit_bytes: None,
            ..budgets()
        },
        LiveBudgets {
            available_physical_bytes: None,
            ..budgets()
        },
        LiveBudgets {
            available_commit_bytes: Some(8 * GIB),
            ..budgets()
        },
        LiveBudgets {
            job_limit_bytes: 7 * GIB,
            ..budgets()
        },
        LiveBudgets {
            available_physical_bytes: Some(14 * GIB - 1),
            ..budgets()
        },
    ] {
        assert!(select(&cpu, &[npu], reservation(), insufficient, Intent::Automatic).is_err());
    }
    let boundary = LiveBudgets {
        available_commit_bytes: Some(9 * GIB),
        available_physical_bytes: Some(14 * GIB),
        job_limit_bytes: 8 * GIB,
        ..budgets()
    };
    assert_eq!(
        select(&cpu, &[], reservation(), boundary, Intent::Automatic)?.resources,
        cpu.resources
    );
    let mut overflow = cpu;
    overflow.resources.physical_bytes = u64::MAX;
    assert!(select(&overflow, &[], reservation(), budgets(), Intent::Automatic).is_err());
    Ok(())
}

#[test]
fn strict_backend_and_changed_identity_refuse_silent_fallback() -> Result<(), String> {
    let fixture = Evidence::new()?;
    let cpu = fixture.profile("cpu", Backend::Cpu, 100);
    let npu = fixture.profile("npu", Backend::Hexagon, 60);
    let intent = Intent::Strict(npu.observation.placement);
    assert_eq!(
        select(&cpu, &[npu], reservation(), budgets(), intent)?
            .recommendation
            .profile_id,
        "npu"
    );
    for (backend, reason) in [
        (BackendStatus::Unknown, FallbackReason::UnknownCapability),
        (
            BackendStatus::Unavailable,
            FallbackReason::UnavailableBackend,
        ),
        (
            BackendStatus::RuntimeChanged,
            FallbackReason::IncompatibleRuntime,
        ),
        (
            BackendStatus::DriverChanged,
            FallbackReason::IncompatibleDriver,
        ),
    ] {
        let changed = Profile { backend, ..npu };
        assert_eq!(
            select(&cpu, &[changed], reservation(), budgets(), intent),
            Err("strict-backend-unavailable".into())
        );
        let selection = select(
            &cpu,
            &[changed],
            reservation(),
            budgets(),
            Intent::Automatic,
        )?;
        assert_eq!(selection.recommendation.profile_id, "cpu");
        assert_eq!(selection.fallback_reason, Some(reason));
    }
    Ok(())
}

#[test]
fn workload_and_accelerator_ledger_cannot_be_omitted_or_shrunk() -> Result<(), String> {
    let fixture = Evidence::new()?;
    let cpu = fixture.profile("cpu", Backend::Cpu, 100);
    for request in [
        Reservation {
            output_tokens: 4097,
            ..reservation()
        },
        Reservation {
            output_tokens: 0,
            ..reservation()
        },
        Reservation {
            prompt_tokens: 4097,
            ..reservation()
        },
        Reservation {
            state: StateUse::Reused,
            ..reservation()
        },
    ] {
        assert!(select(&cpu, &[], request, budgets(), Intent::Automatic).is_err());
    }
    let mut npu = fixture.profile("npu", Backend::Hexagon, 60);
    npu.resources.hexagon_bytes = 0;
    assert_eq!(
        select(&cpu, &[npu], reservation(), budgets(), Intent::Automatic)?
            .recommendation
            .profile_id,
        "cpu"
    );
    npu.resources.hexagon_bytes = 4 * GIB;
    npu.resources.system_commit_bytes = 1;
    assert_eq!(
        select(&cpu, &[npu], reservation(), budgets(), Intent::Automatic)?
            .recommendation
            .profile_id,
        "cpu"
    );
    Ok(())
}

#[test]
fn unreviewed_fast_profiles_and_duplicate_ids_remain_rejected() -> Result<(), String> {
    let fixture = Evidence::new()?;
    let cpu = fixture.profile("cpu", Backend::Cpu, 100);
    let mut npu = fixture.profile("npu", Backend::Hexagon, 1);
    npu.observation.independently_reviewed = false;
    assert_eq!(
        select(&cpu, &[npu], reservation(), budgets(), Intent::Automatic)?
            .recommendation
            .profile_id,
        "cpu"
    );
    npu.observation.independently_reviewed = true;
    assert!(
        select(
            &cpu,
            &[npu, npu],
            reservation(),
            budgets(),
            Intent::Automatic
        )
        .is_err()
    );
    assert!(
        select(
            &cpu,
            &[npu; 33],
            reservation(),
            budgets(),
            Intent::Automatic
        )
        .is_err()
    );
    Ok(())
}
