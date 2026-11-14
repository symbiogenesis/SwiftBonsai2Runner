# Native CPU bundle comparison

The owner-operated [workflow](../../.github/workflows/cpu-bundle-comparison.yml)
builds one collector against fixed Prism **0.1.23** and registry **0.1.25** on
hosted Windows ARM64. The exact published Prism tooling revision supplies the
collector; the registry runtime has its own authenticated identity.

The fixed workload uses `SwiftSession` directly, CPU only, four workers, FP16 KV,
approved adapter 1.3, seed 42 and public messages. Request timing includes
preparation and generation; load/process timing remains separate. UI, broker,
speech and the application's K4/V4 default are outside scope. Two reversed short
pairs do not qualify individual optimizations, uncertainty, sustained behavior,
state/quality, promotion or a runtime pin.

Prism's separate `cpu_candidate_check` collector can describe newer source
profiles with FP16/K4/V4 and explicit ordinary/8/16/32-token ingestion. Its distinct
schema-2 candidate receipts are **not admitted by this frozen lane**. The separate
[Rust source-comparison evaluator](#current-source-offline-admission) accepts a
reviewed checked-build packet and complete owner evidence. Automated checked-build
preparation/device capture remains outstanding in [PLAN.md](../../PLAN.md#cpu-kernels);
source collection alone supplies no performance or promotion result.

## Current-source offline admission

Pass a JSON request on stdin to `bonsai-tools cpu-evaluation --request -`:

```json
{
  "operation": "evaluate-source-comparison",
  "preparationPath": "C:\\packet\\preparation.json",
  "preparationSha256": "<independently reviewed lowercase SHA-256>",
  "ownerPath": "C:\\capture\\owner.json",
  "ownerSha256": "<independently reviewed lowercase SHA-256>"
}
```

The current-source protocol is independent of `cpu_bundle.ps1`. The source
contract is [source_comparison.rs](../bonsai/src/cpu_evaluation/source_comparison.rs)
and its [focused fixtures](../bonsai/src/cpu_evaluation/source_comparison/tests.rs).
All envelopes reject unknown fields, all nested evidence rejects duplicate JSON
fields, and every retained member has a confined relative path and SHA-256.
Reads are bounded before parsing. Digests bind bytes; they authenticate neither
a build producer nor that producer's successful-check declarations. Independently
verify the reviewed clean source/build provenance before supplying the packet
digest. This command performs no build, model execution, device capture or CI work.

The schema-1 `prism-cpu-source-comparison-preparation` contains:

- `runner_revision`, the exact executing maintenance `evaluator_sha256`,
  `messages` (`path`, `sha256`), `model_sha256`, `adapter_sha256`, `output_tokens`
  fixed at 32, `schedule` fixed at baseline/candidate/candidate/baseline and `arms`.
- One shared `profile`: `kv_cache` (`f16` or `k4-v4`), `context_tokens` 8192,
  `cpu_prefill_tokens` 1/8/16/32 and `cpu_prefill_scratch_bytes`. Ordinary ingestion
  uses 1/0; explicit tiles require a nonzero reserve at most 256 MiB. Precision,
  context, tile and the full reserve match across both sources.
- Each named build arm supplies `source_revision`, `clean: true`, exact matching
  `compiler`, native pinned `toolchain`, `rustc_sha256`, `cargo_sha256`,
  `helper_source_sha256` keyed by both exact helper package names,
  `target: aarch64-pc-windows-msvc`, ordered `features`
  (`cli`, `experimental-neon`, `parallel-rows`), `executable_sha256`,
  `executable_bytes`, `inputs` and `checks`. `inputs` binds `Cargo.toml`,
  `Cargo.lock`, `rust-toolchain.toml`, both helper manifests (`tools/neon-dot`, `tools/native-hash`),
  `examples/cpu_candidate_check.rs` and the shared
  `examples/cpu_bundle_check/{collector,candidate_tests}.rs`. Collector bytes must
  match across sources; the source revisions must differ. Source digests are
  declarations covered by the independently reviewed build packet; copied source
  files are not required replay artifacts.
- Ordered checks are `source_analysis`, `neon_helper_analysis`,
  `hash_helper_analysis`, `portable_library`, `state_fixture`, `collector_analysis`,
  `collector_controls`, `collector_link`. Every check retains `command`,
  `exit_code: 0`, `passed` names and zero `failed`, `ignored`, `measured` counts.
  Commands must match the evaluator's exact phase templates, beginning with
  `cargo +<toolchain>`; execution-suppressing or extra flags reject admission.
  The templates require locked/offline Release execution, explicit package selection
  and strict all-target Clippy for runtime/helper analysis, strict collector-example
  Clippy, scalar library checking,
  the exact named graph state control, all three current-source collector tests
  and production collector linking. Test names use their sorted full Rust names.

The public messages are exactly the fixed count-to-ten fixture above. Sources
must support the same production sampler/model stops, seed 42, disabled thinking,
approved adapter 1.3, four effective workers, no prompt reuse or external executor.
All four outputs, exact UTF-8 digests, rendered prompt/output counts and terminal
reasons must match. The collector limits remain 128 prompt and 32 output tokens;
this evaluator supplies no long-prefill qualification.

The schema-1 `prism-cpu-source-comparison-owner` contains `preparation_sha256`,
`workload_predeclared: true`, `completed: true`, `budget_seconds: 600`, nonblank
`fixed_work` and `budget_reason`, conservative `estimated_seconds` (301..600),
`clock_frequency`, `started_tick`, `deadline_tick`,
`original_deadline_unix_milliseconds`, `retention_completed_tick`, aggregate
`power_before`/`power_after` and four ordered `arms`. This fixed ten-minute
workload is a protocol bound, not authorization to start a trial; all owner
budget and power requirements still apply.

Each arm retains `id` (`arm-1` through `arm-4`), `role`, `started_tick`,
`execution_deadline_tick` (at most sixty seconds after start),
`collector_deadline_unix_milliseconds` derived from the original aggregate clock,
`cleanup_completed_tick`, `power_before`/`power_after`, `owned_job_empty: true`,
`cleanup_succeeded: true`, `minimum_admission_bytes` (15.5 GiB plus full scratch),
`maximum_job_memory_bytes` (11.5 GiB plus full scratch), and unique SHA-bound
`receipt`, `started_receipt`, `supervision` references. Retain these twelve raw
proof files for independent replay. Confirm cleanup before the next arm; admit
neither missing power/CPU conditions nor failed/partial collector records.

The result retains losing/no-go timing outcomes as admitted comparisons. Both
reversed load-inclusive process-wall ratios must be at most 0.90, and owned-arm
ratios at most 1.10, to report an observed short-request benefit. Equal power
endpoints do not prove uninterrupted stability. State/logits/adapters, uncertainty,
quality, comfort, sustained behavior, producer authentication and promotion remain
independent gates; no runtime pin or default changes follow from this evaluator.

Focused hosted command: `cargo test --locked --offline --release -p bonsai-tools
--lib source_comparison_`. It exercises file-bound replay, losing outcomes,
legacy/profile rejection, helper/positive-control binding, power/resource/deadline/
cleanup failures, path escape, duplicate JSON and changed raw proof bytes.

## Hosted preparation and offline replay

[`cpu_bundle.ps1`](cpu_bundle.ps1) verifies registry archives, locked dependency
subsets and positive controls. Preparation requires published Runner/Prism sources;
it performs no push and refuses local preparation before Cargo/model execution.

- `-Operation prepare -BaselineRoot ABSOLUTE_PATH -ToolingRoot ABSOLUTE_PATH
  -CandidateRegistryVersion 0.1.25 -RunnerRevision EXACT_SHA -Output NEW_ABSOLUTE_PATH`
- `-Operation evaluate -Preparation ABSOLUTE_JSON -Owner ABSOLUTE_JSON`

Schema-2 packets retain `preparation.json`, `messages.json` and `evaluator.ps1`,
including evaluator SHA-256 and PowerShell language identity. Replay uses
`pwsh -NoLogo -NoProfile -File evaluator.ps1 -Operation evaluate
-Preparation ABSOLUTE_PREPARATION_JSON -Owner ABSOLUTE_OWNER_JSON`.
The bounded .NET tar reader compares extracted Cargo files with authenticated
archive bytes without extracting archive paths. Admission validates the complete
ABBA set, source/artifact identities, outputs, original deadline, host/power,
resources and cleanup. Hosted preparation and device acceptance remain separate.

A focused local check is
`pwsh -NoLogo -NoProfile -File tools/cpu-bundle/test_cpu_bundle.ps1 -Test archive`.
The full inventory belongs to the owning workflow.

## Owner-device workload

`run_device: false` prepares only. Device capture declares **one fixed ten-minute
workload**, estimated at five to eight minutes: four ABBA arms, each reserving a
60-second collector deadline, 90-second completion allowance, 95-second outer
helper, ten seconds for conditions and five for metadata. Four 110-second
reservations plus sixty seconds for bootstrap/intake and one hundred seconds for
cleanup/evaluation/upload fit the 600-second ceiling.

Bootstrap, checkout, tool/artifact intake, hashing, per-arm power observations,
all arms, cleanup and upload share that original deadline under
[AGENTS.md](../../AGENTS.md#owner-device-execution-budgets). Refuse work that no
longer fits; never retry, resume or shrink the fixed input/32-token reservation.

Before starting the ephemeral runner, create the deadline and native power receipt
on the same physical machine using PowerShell 7.6+ and the actual dispatched run:

```powershell
$runId = '<actual-dispatched-run-id>'
$runnerRevision = '<exact-published-Runner-main-SHA>'
$ownerReceipt = Join-Path $env:TEMP ('cpu-bundle-owner-' + $runId + '.json')
./scripts/Invoke-CpuBundleComparison.ps1 -CreateOwnerStartReceipt `
    -RunId $runId -RunnerCommit $runnerRevision `
    -CandidateCommit 99bab71270b1e6a6fb6d47eec2e34b8097cdcd4c `
    -StartReceipt $ownerReceipt
$env:CPU_BUNDLE_OWNER_START_RECEIPT = $ownerReceipt
```

Carry that variable into owner-managed bootstrap. Register an ephemeral runner
with only `cpu-bundle-<run-id>`; missing/foreign receipts reject capture. The
script does not register runners or create credentials.

Each collector uses the checked completion supervisor and an owned Windows Job,
with separate collector JSON/supervisor streams and tokens removed from the
process tree. Verified cleanup is required before the next arm. Retain only named
replay inputs and useful nonempty failure diagnostics. The owning job separately
checks the original deadline and final power after upload; local artifacts do not
prove that gate passed. The owner controls dispatch, execution and CI verdicts.
