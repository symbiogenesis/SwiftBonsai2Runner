# Native CPU bundle comparison lane

The manual [workflow](../.github/workflows/cpu-bundle-comparison.yml) prepares
matched native CPU collectors on hosted Windows ARM64. It compares the complete
integrated serving bundle from original Prism `0.1.23` against reviewed registry
runtime `0.1.25`, using one collector source with explicit FP16 KV, the approved
adapter at scale 1.3, four CPU workers, seed 42 and fixed public messages.
The exact published Prism **tooling revision** is separate from the registry
candidate's authenticated source identity. Both Runner and the new Prism tooling
must be owner-published before this workflow can consume them. No push is part
of preparation or capture.

The default `run_device: false` performs hosted preparation only. Opting into
device capture declares one fixed ten-minute workload. Each of four ABBA arms
has a 60-second collector deadline and a 90-second completion allowance; its
outer helper is bounded at 95 seconds, with ten seconds for condition checks and
five seconds for metadata. Four 110-second reservations, sixty seconds for
bootstrap/intake and one hundred seconds for cleanup/evaluation/upload fit the
600-second maximum. The planning estimate is five to eight minutes, without an
empirical timing prediction from earlier mixed-backend observations.
Bootstrap, checkout, tool and artifact intake, hashing,
power observations, all four arms, cleanup and evidence upload share the original
600-second deadline. Refuse work that no longer fits; never retry or resume it.
The 32-token reservation and fixed input are never reduced to make a failed arm
fit. The earlier model-free preflight is closed and supplies no comparison allowance.

Before registering or starting the ephemeral runner for the actual dispatched
run, the owner creates its original deadline and native power receipt from native
PowerShell 7.6+. Use the genuine run ID and published Runner revision:

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

Carry that environment variable into the existing owner-managed runner bootstrap.
Register it as ephemeral with only this run's `cpu-bundle-<run-id>` qualification
label; a persistent generic runner must not accept queued historical workloads.
The receipt must originate on that same physical machine before bootstrap; a
missing or foreign receipt stops capture. The script does not register runners,
create GitHub credentials or impersonate CI.

Each collector runs through the existing checked completion supervisor and its
owned Windows Job. Collector JSON and supervisor streams use separate directories.
Only verified, completed cleanup permits the next arm. Tokens are removed from
the entire supervisor/collector process tree. The final selection retains named
JSON/replay inputs and relevant nonempty failure diagnostics; successful empty
stdout/stderr remain intermediate files. A post-upload check enforces the same
original deadline and records final power in the owning job summary. The local
artifact alone does not prove that final job gate passed.

The evaluator reports only two reversed short-request pairs for the integrated
bundle. It never changes a runtime pin or default and leaves full promotion,
individual optimization attribution, uncertainty, state/quality and sustained
acceptance unqualified. Exact LUT, tiled/triangular prefill and latest-prefix
variants retain their separate consumers and gates in [PLAN.md](../PLAN.md#cpu-kernels).
