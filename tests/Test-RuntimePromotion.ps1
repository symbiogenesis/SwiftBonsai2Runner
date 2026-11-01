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
$ptq1 = Invoke-BonsaiTool -Command runtime-promotion -ToolPath $ToolPath -Request @{
    operation = 'plan'; runnerRevision = $revision; nativeRowWorkers = 1
    weightRepresentation = 'current-repacked-PTQ1_0'
}
if ($ptq1.policy.profile -cne 'swift-ptq1-standard-lora-1.3' -or
    $ptq1.policy.identities.modelSha256 -cne 'e658f0f0736a03a4549535a439f9b07254ebf8ab88ae27cbe6e52d098ca5d1f5' -or
    $ptq1.policy.identities.modelBytes -ne 5946648928 -or
    $ptq1.policy.identities.ptq1ExportReceiptSha256 -cnotmatch '^[0-9a-f]{64}$' -or
    $ptq1.policy.identities.rollback.modelSha256 -cne $plan.policy.identities.modelSha256 -or
    $ptq1.policy.identities.rollback.runtimeLockSha256 -cne $plan.policy.identities.runtimeLockSha256 -or
    $ptq1.policy.identities.admissionBytes -ne $plan.policy.identities.admissionBytes -or
    $ptq1.policy.requiredGates.Count -ne ($plan.policy.requiredGates.Count + 2) -or
    @($ptq1.policy.requiredGates | Where-Object id -CEQ 'current-weight-ptq1-packing').Count -ne 1 -or
    @($ptq1.policy.requiredGates | Where-Object id -CEQ 'original-pq2-rollback').Count -ne 1 -or
    $ptq1.policySha256 -ceq $plan.policySha256) {
    throw 'PTQ1 promotion plan lost its packing, original-PQ2 rollback or independent-gate identities.'
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
$rejected = $false
try {
    $null = Invoke-BonsaiTool -Command runtime-promotion -ToolPath $ToolPath -Request @{
        operation = 'file-score'; runnerRevision = $revision; evidencePath = $evidence
        weightRepresentation = 'current-repacked-PTQ1_0'
    }
} catch { $rejected = $true }
if (-not $rejected) { throw 'PQ2 evidence qualified a different PTQ1 promotion policy.' }
$ptq1.evidenceTemplate | ConvertTo-Json -Depth 64 |
    Set-Content -LiteralPath $evidence -Encoding utf8NoBOM
$ptq1Score = Invoke-BonsaiTool -Command runtime-promotion -ToolPath $ToolPath -Request @{
    operation = 'file-score'; runnerRevision = $revision; evidencePath = $evidence
    weightRepresentation = 'current-repacked-PTQ1_0'
}
if ($ptq1Score.promotionReady -or $ptq1Score.status -cne 'pending' -or
    $ptq1Score.gates.Count -ne $ptq1.policy.requiredGates.Count) {
    throw 'Uncollected PTQ1 evidence incorrectly qualified default promotion.'
}
'Runtime promotion command preserves exact policy and pending evidence.'
