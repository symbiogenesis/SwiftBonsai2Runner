#requires -Version 7.6
<# .SYNOPSIS
Rejects incomplete or coerced native computation evidence without opening a device.
#>
[CmdletBinding()]
[OutputType([string])]
param([Parameter(Mandatory)][ValidateSet('Plan','Result')][string]$Case)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.TestSupport.psm1')
Import-Module (Join-Path $PSScriptRoot '../scripts/Bonsai.HexagonCompute.psm1')
if ($Case -ceq 'Plan') {
    $computeModule = Get-Module -Name Bonsai.HexagonCompute
    $helpers = @(& $computeModule {
        foreach ($name in @('Assert-Arm64PE','Assert-BonsaiHexagonSessionBuild','Get-BonsaiHexagonDispatchFile','Get-BonsaiHexagonDriverGuard','Invoke-BonsaiHexagonBoundedCapture')) {
            (Get-Command -Name $name -CommandType Function -ErrorAction Stop).Name
        }
    })
    Assert ($helpers.Count -eq 5) 'The owned worker cannot resolve a required verification helper.'
    $plan = & (Join-Path $PSScriptRoot '../scripts/Invoke-HexagonComputeProbe.ps1') -PlanOnly
    Assert ($plan.MaximumSeconds -eq 120 -and $plan.CleanupReserveSeconds -eq 75 -and $plan.Operations -eq 1 -and
        $plan.QueueSlots -eq 1 -and $plan.OwnedJobRequired -and $plan.IndependentWindowsAbiRequired -and
        $plan.CatalogVerificationRequired -and $plan.SourceBoundHostedFixtureRequired) 'Compute plan lost its fixed operation or owned deadline.'
    Assert (-not $plan.ModelLoaded -and -not $plan.ProfileEligible -and -not $plan.TrustChangesAllowed -and
        -not $plan.AutomaticRetryAllowed) 'Compute plan authorized an independent gate or replay.'
    $q2Plan = & (Join-Path $PSScriptRoot '../scripts/Invoke-HexagonComputeProbe.ps1') -PlanOnly -Case GuardedQ2
    Assert ($q2Plan.ComputeCase -ceq 'GuardedQ2' -and $q2Plan.Columns -eq 256 -and $q2Plan.Rows -eq 32 -and
        $q2Plan.Batch -eq 2 -and $q2Plan.Operations -eq 1 -and -not $q2Plan.ProfileEligible) 'Guarded Q2 plan changed its fixed primitive.'
} else {
    $native = @{
        schemaVersion = 1; kind = 'native-fastrpc-queue-compute-probe'; driverDll = 'C:\Windows\System32\DriverStore\FileRepository\fixture\libcdsprpc.dll'
        passed = $true; stage = 'queue-compute-complete'; architecture = 81; effectiveDomain = 3
        uriVerified = $true; unsignedProcessRequested = $true; handleObserved = $true
        responseValidated = $true; numericValidated = $true; freeCompleted = $true; ownerPinnedUntilExit = $true
        poisoned = $false; computeSubmitted = $true; computeExecuted = $true; profileEligible = $false
        sessionTeardownVerified = $false
    }
    $steps = @('domainResult','uriResult','processInitializationResult','moduleOpenResult','queueCreateResult',
        'queueExportResult','queueStartResult','queueWriteResult','queueReadResult','queueStopResult',
        'queueCloseResult','mappingResult','unmappingResult','moduleCloseResult')
    foreach ($step in $steps) { $native[$step] = 0 }
    $result = Get-BonsaiHexagonComputeResult -Json ($native | ConvertTo-Json -Compress) -ExitCode 0
    Assert $result.passed 'Complete fixed computation was rejected.'
    foreach ($step in $steps) {
        foreach ($value in @($null, '0', 1, 0.0)) {
            $changed = $native.Clone(); $changed[$step] = $value
            Assert-Failure { Get-BonsaiHexagonComputeResult -Json ($changed | ConvertTo-Json -Compress) -ExitCode 0 } 'Coerced or missing native lifecycle result admitted.'
        }
    }
    foreach ($flag in @('passed','responseValidated','numericValidated','freeCompleted','computeSubmitted','computeExecuted','ownerPinnedUntilExit')) {
        $changed = $native.Clone(); $changed[$flag] = 'true'
        Assert-Failure { Get-BonsaiHexagonComputeResult -Json ($changed | ConvertTo-Json -Compress) -ExitCode 0 } 'Coerced native computation flag admitted.'
    }
    foreach ($flag in @('responseValidated','numericValidated','freeCompleted','computeSubmitted','ownerPinnedUntilExit')) {
        $changed = $native.Clone(); $changed[$flag] = $false
        Assert-Failure { Get-BonsaiHexagonComputeResult -Json ($changed | ConvertTo-Json -Compress) -ExitCode 0 } 'Incomplete native computation claimed success.'
    }
    foreach ($flag in @('poisoned','profileEligible','sessionTeardownVerified')) {
        $changed = $native.Clone(); $changed[$flag] = $true
        Assert-Failure { Get-BonsaiHexagonComputeResult -Json ($changed | ConvertTo-Json -Compress) -ExitCode 0 } 'Uncertain owner or unrelated qualification admitted.'
    }
    $partial = $native.Clone(); $partial.passed = $false; $partial.computeExecuted = $false
    $partial.computeSubmitted = $true; $partial.numericValidated = $false; $partial.responseValidated = $false
    $partial.freeCompleted = $false; $partial.poisoned = $true; $partial.queueReadResult = 39
    $result = Get-BonsaiHexagonComputeResult -Json ($partial | ConvertTo-Json -Compress) -ExitCode 0
    Assert (-not $result.passed -and $result.poisoned) 'An uncertain submitted buffer did not remain a partial observation.'
    Assert-Failure { Get-BonsaiHexagonComputeResult -Json ($native | ConvertTo-Json -Compress) -ExitCode 1 } 'A failed child became successful evidence.'
    Assert-Failure { Get-BonsaiHexagonComputeResult -Json '{"schemaVersion":1,"schemaVersion":1}' -ExitCode 0 } 'Duplicate evidence keys admitted.'
    $q2 = $native.Clone(); $q2.kind = 'native-fastrpc-queue-q2-probe'
    $q2.columns = 256; $q2.rows = 32; $q2.batch = 2; $q2.outputValuesChecked = 64
    $result = Get-BonsaiHexagonComputeResult -Json ($q2 | ConvertTo-Json -Compress) -ExitCode 0 -Case GuardedQ2
    Assert $result.passed 'Complete fixed guarded Q2 receipt was rejected.'
    Assert-Failure { Get-BonsaiHexagonComputeResult -Json ($q2 | ConvertTo-Json -Compress) -ExitCode 0 } 'Q2 receipt supplied the F32 gate.'
    Assert-Failure { Get-BonsaiHexagonComputeResult -Json ($native | ConvertTo-Json -Compress) -ExitCode 0 -Case GuardedQ2 } 'F32 receipt supplied the Q2 gate.'
    foreach ($field in @('columns','rows','batch','outputValuesChecked')) {
        $changed = $q2.Clone(); $changed[$field] = $null
        Assert-Failure { Get-BonsaiHexagonComputeResult -Json ($changed | ConvertTo-Json -Compress) -ExitCode 0 -Case GuardedQ2 } 'Incomplete guarded Q2 geometry admitted.'
    }
    $noBlock = $native.Clone(); $noBlock.kind = 'native-fastrpc-queue-noblock-probe'; $noBlock.writeMode = 'nonblocking'
    $result = Get-BonsaiHexagonComputeResult -Json ($noBlock | ConvertTo-Json -Compress) -ExitCode 0 -Case F32NoBlock
    Assert $result.passed 'Explicit nonblocking control was rejected.'
    Assert-Failure { Get-BonsaiHexagonComputeResult -Json ($noBlock | ConvertTo-Json -Compress) -ExitCode 0 } 'A nonblocking observation supplied the timed-write gate.'
    $changed = $noBlock.Clone(); $changed.writeMode = 'timed'
    Assert-Failure { Get-BonsaiHexagonComputeResult -Json ($changed | ConvertTo-Json -Compress) -ExitCode 0 -Case F32NoBlock } 'An ambiguous write mode supplied the nonblocking gate.'
    $q2NoBlock = $q2.Clone(); $q2NoBlock.kind = 'native-fastrpc-queue-q2-noblock-probe'; $q2NoBlock.writeMode = 'nonblocking'
    $result = Get-BonsaiHexagonComputeResult -Json ($q2NoBlock | ConvertTo-Json -Compress) -ExitCode 0 -Case GuardedQ2NoBlock
    Assert $result.passed 'Explicit guarded Q2 nonblocking control was rejected.'
    Assert-Failure { Get-BonsaiHexagonComputeResult -Json ($q2NoBlock | ConvertTo-Json -Compress) -ExitCode 0 -Case GuardedQ2 } 'Nonblocking Q2 supplied the timed Q2 gate.'
    $drained = $noBlock.Clone(); $drained.passed = $false; $drained.queueWriteResult = 14
    $drained.poisoned = $true; $drained.freeCompleted = $false
    foreach ($name in @('queueStopResult','queueCloseResult','unmappingResult','moduleCloseResult')) { $drained[$name] = $null }
    $result = Get-BonsaiHexagonComputeResult -Json ($drained | ConvertTo-Json -Compress) -ExitCode 0 -Case F32NoBlock
    Assert ($result.computeExecuted -and -not $result.passed -and $result.poisoned) 'A verified error-drain reply lost its independent numerical observation.'
    foreach ($name in @('queueStopResult','queueCloseResult','unmappingResult','moduleCloseResult')) {
        $changed = $drained.Clone(); $changed[$name] = 0
        Assert-Failure { Get-BonsaiHexagonComputeResult -Json ($changed | ConvertTo-Json -Compress) -ExitCode 0 -Case F32NoBlock } 'An error-drain reply admitted normal cleanup.'
    }
    $changed = $drained.Clone(); $changed.queueReadResult = $null
    Assert-Failure { Get-BonsaiHexagonComputeResult -Json ($changed | ConvertTo-Json -Compress) -ExitCode 0 -Case F32NoBlock } 'A numeric error-drain claim omitted its completed read.'
}
return 'Native queue compute evidence controls passed without opening a device.'
