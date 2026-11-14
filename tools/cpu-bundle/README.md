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
schema-2 candidate receipts are **not admitted by this frozen lane**. A checked
current-source preparation and comparison lane remains outstanding in
[PLAN.md](../../PLAN.md#cpu-kernels); source collection alone supplies no X1
performance or promotion result.

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
