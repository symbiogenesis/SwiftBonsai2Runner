#requires -Version 7.6
<#
.SYNOPSIS
Checks the real PowerShell-to-Rust promotion plan and pending evidence handoff.
.DESCRIPTION
Uses the packaged maintenance tool without loading models or running measurements.
The synthetic review revision never authorizes a serving default.
#>
[CmdletBinding()]
[OutputType([string])]
param([string]$ToolPath)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot '../scripts/Bonsai.Tools.psm1')
$revision = 'a' * 40
$plan = Invoke-BonsaiTool -Command runtime-promotion -ToolPath $ToolPath -Request @{
    operation = 'plan'; runnerRevision = $revision; nativeRowWorkers = 1
}
if ($plan.policy.identities.runnerRevision -cne $revision -or
    $plan.policy.schemaVersion -ne 2 -or $plan.evidenceTemplate.schemaVersion -ne 2 -or
    $plan.policy.identities.adapterScale -ne 1.3 -or
    $plan.policy.identities.nativeRowWorkers -ne 1 -or
    $plan.policy.identities.servingStatePolicy -cne 'reset-every-request' -or
    $plan.policy.requiredGates.Count -lt 17) {
    throw 'Promotion command did not preserve the complete standard-profile policy.'
}
$harness = Invoke-BonsaiTool -Command runtime-promotion -ToolPath $ToolPath -Request @{
    operation = 'plan'; runnerRevision = $revision; nativeRowWorkers = 1; serviceHarnessRevision = ('b' * 40)
}
if ($harness.policy.identities.serviceHarnessRevision -cne ('b' * 40) -or
    $harness.policy.identities.runtimeRevision -cne $plan.policy.identities.runtimeRevision -or
    $harness.policySha256 -ceq $plan.policySha256 -or
    $harness.policy.identities.workerSha256 -cnotmatch '^[0-9a-f]{64}$') {
    throw 'Promotion command did not separate the exact released runtime and selected service harness.'
}
$parallel = Invoke-BonsaiTool -Command runtime-promotion -ToolPath $ToolPath -Request @{
    operation = 'plan'; runnerRevision = $revision; nativeRowWorkers = 4
}
if ($parallel.policy.identities.nativeRowWorkers -ne 4 -or
    $parallel.policySha256 -ceq $plan.policySha256) {
    throw 'Promotion command did not bind the selected row-worker policy.'
}
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('Runtime-Promotion-Bridge-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($scratch)
$evidence = Join-Path $scratch 'evidence.json'
$plan.evidenceTemplate | ConvertTo-Json -Depth 64 |
    Set-Content -LiteralPath $evidence -Encoding utf8NoBOM
$score = Invoke-BonsaiTool -Command runtime-promotion -ToolPath $ToolPath -Request @{
    operation = 'file-score'; runnerRevision = $revision; nativeRowWorkers = 1; evidencePath = $evidence
}
if ($score.promotionReady -or $score.status -cne 'pending' -or
    @($score.gates | Where-Object status -CNE 'pending').Count -ne 0) {
    throw 'Uncollected evidence incorrectly qualified the default runtime.'
}
'Runtime promotion command preserves exact policy and pending evidence.'
