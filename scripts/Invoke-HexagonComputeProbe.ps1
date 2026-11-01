#requires -Version 7.6
<# .SYNOPSIS
Runs one fixed native Rust matrix operation through the verified newer signed DSP package.
.DESCRIPTION
Intake must come from a trusted hosted run after independent queue and wire ABI controls.
The caller owns same-run artifact provenance. This supervisor binds its source and package,
reserves 75 seconds for owned Job teardown, and retains partial credential-free receipts.
It requests the separately reviewed reference DSP process class without changing OS trust.
No model, automatic retry, CPU replay or profile promotion is performed.
#>
[CmdletBinding(DefaultParameterSetName = 'Execute')]
[OutputType([Collections.Specialized.OrderedDictionary])]
param(
    [Parameter(Mandatory, ParameterSetName = 'Execute')][string]$PackageDirectory,
    [Parameter(Mandatory, ParameterSetName = 'Execute')][string]$FixtureDirectory,
    [Parameter(Mandatory, ParameterSetName = 'Execute')][string]$OutputDirectory,
    [Parameter(Mandatory, ParameterSetName = 'Execute')][string]$SigntoolPath,
    [Parameter(Mandatory, ParameterSetName = 'Execute')][ValidatePattern('^[a-f0-9]{40}$')][string]$ExpectedRunnerRevision,
    [ValidateRange(1,240)][int]$MaximumSeconds = 120,
    [ValidateSet('F32','GuardedQ2','F32NoBlock','GuardedQ2NoBlock')][string]$Case = 'F32',
    [Parameter(Mandatory, ParameterSetName = 'Plan')][switch]$PlanOnly
)
$clock = [Diagnostics.Stopwatch]::StartNew()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.HexagonCompute.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.HexagonDispatch.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Native.psm1')
if ($PlanOnly) {
    Import-Module (Join-Path $PSScriptRoot 'Bonsai.HexagonPolicy.psm1')
    return Invoke-BonsaiHexagonPolicy -Request @{ operation = 'compute-plan'; maximumSeconds = $MaximumSeconds; case = $Case }
}
if (-not $IsWindows -or [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture -cne 'Arm64' -or
    [Runtime.InteropServices.RuntimeInformation]::OSArchitecture -cne 'Arm64') { throw 'Native Windows ARM64 is required.' }
$PackageDirectory = Resolve-ExternalPath $PackageDirectory
$FixtureDirectory = Resolve-ExternalPath $FixtureDirectory
$SigntoolPath = Resolve-ExternalPath $SigntoolPath
$OutputDirectory = Resolve-ExternalPath $OutputDirectory
if (-not [IO.Directory]::Exists($PackageDirectory) -or -not [IO.Directory]::Exists($FixtureDirectory) -or
    -not [IO.File]::Exists($SigntoolPath) -or [IO.Directory]::Exists($OutputDirectory) -or [IO.File]::Exists($OutputDirectory)) {
    throw 'Use existing checked inputs and a new external receipt directory.'
}
[void][IO.Directory]::CreateDirectory($OutputDirectory)
$report = [ordered]@{
    SchemaVersion = 1; Kind = 'runner-hexagon-compute'; ComputeCase = $Case; Outcome = 'pending'; Failure = $null
    ExpectedRunnerRevision = $ExpectedRunnerRevision; MaximumSeconds = $MaximumSeconds; CleanupReserveSeconds = 75
    ElapsedMilliseconds = 0; WorkerAttempted = $false; OwnedJobTeardown = 'not-started'; Worker = $null
    ComputeExecuted = $false; ModelLoaded = $false; SwiftQualified = $false; ProfileEligible = $false
    TrustChanged = $false
}
try {
    $allowance = [int][Math]::Floor($MaximumSeconds - $clock.Elapsed.TotalSeconds - 75)
    if ($allowance -lt 1) { $report.Failure = 'insufficient-owned-cleanup-reserve'; return $report }
    $repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
    $request = @{
        PackageDirectory = $PackageDirectory; FixtureDirectory = $FixtureDirectory; SigntoolPath = $SigntoolPath
        ExpectedRunnerRevision = $ExpectedRunnerRevision; ComputeCase = $Case; LockPath = Join-Path $repo 'config/hexagon-dispatch.lock.json'
        CandidateLockPath = Join-Path $repo 'config/hexagon-signed-q2-candidate.lock.json'
        NativeSourceDirectory = Join-Path $repo 'tools/hexagon/native_rpc'
        ManifestPath = Join-Path $repo 'tools/hexagon/diagnostic.manifest'
        WorkerReceiptPath = Join-Path $OutputDirectory 'worker.json'
    }
    $requestPath = Join-Path $OutputDirectory 'worker-request.json'
    [IO.File]::WriteAllText($requestPath, ($request | ConvertTo-Json -Depth 5))
    $module = Join-Path $PSScriptRoot 'Bonsai.HexagonCompute.psm1'
    $command = "`$ErrorActionPreference = 'Stop'; Import-Module '" + $module.Replace("'", "''") + "'; " +
        "`$r = ConvertFrom-Json -InputObject ([IO.File]::ReadAllText('" + $requestPath.Replace("'", "''") + "')) -AsHashtable -NoEnumerate -DateKind String; " +
        'Invoke-BonsaiHexagonComputeWorker -Request $r | ConvertTo-Json -Depth 20 -Compress'
    $report.WorkerAttempted = $true; $report.OwnedJobTeardown = 'unconfirmed'
    $owned = Invoke-BonsaiNativeProcess -Executable (Join-Path $PSHOME 'pwsh.exe') -ArgumentList @('-NoLogo','-NoProfile','-NonInteractive','-EncodedCommand',
        [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($command))) -WorkingDirectory $OutputDirectory -TimeoutSeconds $allowance -OwnDescendants
    $report.OwnedJobTeardown = 'confirmed-empty'
    if ($owned.ExitCode -ne 0 -or $owned.StandardOutput.Length -gt 65536 -or $owned.StandardError.Length -gt 65536) { throw 'Owned compute worker failed its output contract.' }
    $worker = ConvertFrom-BonsaiHexagonDispatchJson -Json $owned.StandardOutput
    if ($worker.Kind -cne 'runner-hexagon-compute-worker' -or $worker.ComputeCase -cne $Case -or $worker.SourceRevision -cne $ExpectedRunnerRevision -or
        $worker.Outcome -cnotin @('passed','pending') -or $worker.ModelLoaded -cne $false -or
        $worker.SwiftQualified -cne $false -or $worker.ProfileEligible -cne $false -or $worker.TrustChanged -cne $false) {
        throw 'Invalid owned compute worker result.'
    }
    if ($worker.Outcome -ceq 'passed') {
        $native = Get-BonsaiHexagonComputeResult -Json ($worker.NativeResult | ConvertTo-Json -Depth 10 -Compress) -ExitCode 0 -Case $Case
        if (-not $native.passed -or $worker.SignedPackageVerified -cne $true -or $worker.RuntimeFilesVerified -ne 17 -or $worker.ComputeExecuted -cne $true) {
            throw 'Incomplete verified computation.'
        }
    }
    $report.Worker = $worker; $report.Outcome = $worker.Outcome; $report.Failure = $worker.Failure
    $report.ComputeExecuted = $worker.ComputeExecuted
} catch {
    $report.Failure = 'owned-queue-compute-incomplete'
} finally {
    $report.ElapsedMilliseconds = $clock.ElapsedMilliseconds
    Save-BonsaiHexagonDispatchWorkerReceipt -Report $report -Path (Join-Path $OutputDirectory 'compute.json')
}
return $report
