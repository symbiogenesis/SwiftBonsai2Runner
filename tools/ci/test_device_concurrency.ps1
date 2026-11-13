#Requires -Version 7.6
param([string]$Case)
. (Join-Path $PSScriptRoot 'test_support.ps1')
$repository=Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$workflows=@{}
foreach ($name in @('native-runtime-acceptance','native-ptq1-numerics','x2-cpu-evaluation','cpu-bundle-comparison','tts-evaluation')) {
    $workflows[$name]=[IO.File]::ReadAllText((Join-Path $repository ".github/workflows/$name.yml")).Replace("`r`n","`n")
}
function Assert-DeviceLane([string]$Workflow) {
    $header=($Workflow -split '(?m)^jobs:\s*$',2)[0]
    # Only an explicit device request can take the common mutex. Hosted-only
    # dispatches receive a run-specific group and cannot supersede one another.
    Assert-Ci ($header -cmatch '(?m)^  group: \$\{\{ inputs\.run_device && ''swift-lora-snapdragon'' \|\| format\(''[a-z0-9-]+-\{0\}'', github\.run_id\) \}\}$') 'Expected conditional device group and run-specific hosted fallback.'
    Assert-Ci ($header -cmatch '(?m)^  cancel-in-progress: false$') 'Device captures must not be cancelled by another dispatch.'
}
$cases=[ordered]@{
 hosted_preparation_never_reserves_device={
    foreach ($name in @('native-runtime-acceptance','native-ptq1-numerics','x2-cpu-evaluation','cpu-bundle-comparison')) {
        $workflow=$workflows[$name]
        Assert-DeviceLane $workflow
        Assert-Ci ($workflow -cmatch '(?s)      run_device:\n.*?        type: boolean\n        default: false\n') "$name requires an explicit device request."
        Assert-CiReject { Assert-DeviceLane ($workflow -replace '(?m)^  group: .*$', '  group: swift-lora-snapdragon') } 'Expected conditional device group'
        Assert-CiReject { Assert-DeviceLane ($workflow.Replace('github.run_id','github.ref')) } 'Expected conditional device group'
        Assert-CiReject { Assert-DeviceLane ($workflow.Replace('  cancel-in-progress: false','  cancel-in-progress: true')) } 'must not be cancelled'
    }
 }
 device_jobs_still_require_explicit_selection={
    foreach ($workflowName in @('native-runtime-acceptance','native-ptq1-numerics','x2-cpu-evaluation','cpu-bundle-comparison')) {
        $jobs=Get-CiJobBlocks $workflows[$workflowName]
        # Preparation may run without a device. Every selected device lane must
        # still wait for preparation and test the dispatch boolean itself.
        $device=@($jobs.Keys | Where-Object { $jobs[$_] -cmatch '(?m)^    if: .*inputs\.run_device' })
        Assert-Ci ($device.Count -gt 0) ($workflowName+' has no explicitly selected device job.')
        foreach ($name in $device) { Assert-Ci ('prepare' -cin (Get-CiDependencies $jobs[$name])) ($workflowName+' device job must consume preparation.') }
    }
 }
 rejected_tts_target_stays_hosted={
    $workflow=$workflows['tts-evaluation']; $header=($workflow -split '(?m)^jobs:\s*$',2)[0]
    Assert-Ci (-not $header.Contains('swift-lora-snapdragon')) 'A rejected TTS option cannot reserve the physical device.'
    Assert-Ci ($header.Contains('  group: tts-evaluation-hosted-arm64'))
    $jobs=Get-CiJobBlocks $workflow
    Assert-Ci ($jobs.evaluate.Contains('runs-on: windows-11-arm'))
    Assert-Ci ($jobs.evaluate.IndexOf('Reject the unbounded owner-device matrix before setup') -lt $jobs.evaluate.IndexOf('uses: actions/checkout@'))
    Assert-Ci ($jobs.evaluate.Contains('$env:EVALUATION_TARGET -cne ''hosted-arm64'''))
 }
}
Invoke-CiCases $cases $Case