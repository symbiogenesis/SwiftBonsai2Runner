param(
    [Parameter(Mandatory=$true)][string]$Repository,
    [Parameter(Mandatory=$true)][string]$Revision,
    [Parameter(Mandatory=$true)][string]$OutputDirectory,
    [Parameter(Mandatory=$true)][string]$ModelCase,
    [Parameter(Mandatory=$true)][string]$ControlFilter
)

$trialStartedTick = [Diagnostics.Stopwatch]::GetTimestamp()
$trialStartedUtc = [DateTime]::UtcNow.ToString('o')
$trialClock = [Diagnostics.Stopwatch]::StartNew()
$ErrorActionPreference = 'Stop'
$maximumSeconds = 600
$processReserveSeconds = 60
$utf8 = New-Object Text.UTF8Encoding($false)
$publicDirectory=Join-Path $Repository 'docs\evidence\readme-performance-20261109'
if (Test-Path -LiteralPath $publicDirectory) { throw 'Public evidence destination already exists; never mix trial receipts.' }
if (Test-Path -LiteralPath $OutputDirectory) { throw 'A fresh output directory is required; do not retry a closed trial.' }
[void][IO.Directory]::CreateDirectory($OutputDirectory)
$receiptPath = Join-Path $OutputDirectory 'session.json'
$session = [ordered]@{
    schemaVersion=1; kind='owner-authorized-readme-token-timing'; revision=$Revision
    startedUtc=$trialStartedUtc; startedTick=$trialStartedTick
    maximumAggregateSeconds=$maximumSeconds; processAndRetentionReserveSeconds=$processReserveSeconds
    authorization='Owner explicitly requested: actually, you run the benchmark. This direct local capture is authorized for this workload only; no CI identity is asserted.'
    fixedWork='One offline Release native_model build, new focused synthetic timing controls, one PQ2 then one PTQ1 eight-token request in fresh required-sandbox owners; four workers, adapter 1.3, seed 42, thinking off. No retry, downloads, CI, old-runtime or worker matrix.'
    estimatedSeconds=485
    estimateBreakdownSeconds=@{build=150; setupAndIntake=45; twoStartups=100; requests=100; cleanup=60; retention=30}
    allowanceReason='The conservative 485-second fixed-work estimate exceeds five minutes; ten minutes is the smallest justified allowance. Stop early on completion or any failed stage.'
    status='preparing'; phase='power-provider'; powerBefore=$null; powerAfter=$null
    powerSamples=@(); build=$null; controls=$null; capture=$null; failure=$null
    retries=0; ciActions=0; downloads=0; performanceQualified=$false
    testExecutable=$null; caseBudgetSeconds=$null; finishedUtc=$null; withinAggregateAllowance=$null
    elapsedSeconds=$null; continuousPowerStabilityProven=$false
}
function Save-Session {
    $session.elapsedSeconds = $trialClock.Elapsed.TotalSeconds
    [IO.File]::WriteAllText($receiptPath, ($session | ConvertTo-Json -Depth 64), $utf8)
}
function Read-Power {
    $observation = [ordered]@{utc=[DateTime]::UtcNow.ToString('o'); elapsedSeconds=$trialClock.Elapsed.TotalSeconds
        status='unknown'; source='GetSystemPowerStatus'; ac_online=$null; battery_saver=$null; battery_percent=$null}
    try {
        $value = New-Object ReadmeTimingPower.Status
        if ([ReadmeTimingPower.Native]::GetSystemPowerStatus([ref]$value)) {
            $observation.status='observed'
            if ($value.ACLineStatus -ne 255) { $observation.ac_online=($value.ACLineStatus -eq 1) }
            $observation.battery_saver=($value.SystemStatusFlag -eq 1)
            if ($value.BatteryLifePercent -le 100) { $observation.battery_percent=[int]$value.BatteryLifePercent }
        }
    } catch { }
    return $observation
}
function Run-OwnedStage([string]$Name,[string]$Executable,[string[]]$Arguments,[int]$LatestFinishSeconds) {
    if ($trialClock.Elapsed.TotalSeconds -ge $LatestFinishSeconds) { throw 'Original aggregate allowance cannot fit another stage.' }
    foreach ($argument in $Arguments) {
        if ($argument -match '["\r\n]' -or $argument.EndsWith('\')) { throw 'Unsupported native argument quoting.' }
    }
    $info = New-Object Diagnostics.ProcessStartInfo
    $info.FileName=$Executable
    $info.Arguments=(($Arguments | ForEach-Object { '"' + $_ + '"' }) -join ' ')
    $info.WorkingDirectory=$Repository
    $info.UseShellExecute=$false
    $info.CreateNoWindow=$true
    $info.RedirectStandardOutput=$true
    $info.RedirectStandardError=$true
    foreach ($name in @('ACTIONS_RUNTIME_TOKEN','ACTIONS_ID_TOKEN_REQUEST_TOKEN','GITHUB_TOKEN','GH_TOKEN','ACTIONS_RUNNER_INPUT_TOKEN')) {
        $info.EnvironmentVariables.Remove($name)
    }
    $process=New-Object Diagnostics.Process
    $process.StartInfo=$info
    $stageStart=$trialClock.Elapsed.TotalSeconds
    $expired=$false
    $launched=$false
    try {
        if (-not $process.Start()) { throw 'Owned stage did not start.' }
        $launched=$true
        $stdout=$process.StandardOutput.ReadToEndAsync()
        $stderr=$process.StandardError.ReadToEndAsync()
        while (-not $process.WaitForExit(1000)) {
            $session.powerSamples += ,(Read-Power)
            if ($trialClock.Elapsed.TotalSeconds -ge $LatestFinishSeconds) {
                $expired=$true
                & "$env:SystemRoot\System32\taskkill.exe" /PID $process.Id /T /F | Out-Null
                [void]$process.WaitForExit(5000)
                break
            }
        }
        if (-not $process.HasExited) { throw 'Owned stage exit remains unconfirmed.' }
        $drainMilliseconds=[int][Math]::Max(0, [Math]::Min(5000, ($LatestFinishSeconds-$trialClock.Elapsed.TotalSeconds)*1000))
        if (-not [Threading.Tasks.Task]::WaitAll([Threading.Tasks.Task[]]@($stdout,$stderr),$drainMilliseconds)) {
            throw 'Owned stage output did not close within the original allowance.'
        }
        [IO.File]::WriteAllText((Join-Path $OutputDirectory "$Name.stdout.log"), $stdout.GetAwaiter().GetResult(), $utf8)
        [IO.File]::WriteAllText((Join-Path $OutputDirectory "$Name.stderr.log"), $stderr.GetAwaiter().GetResult(), $utf8)
        return [ordered]@{exitCode=$process.ExitCode; expired=$expired; startedAtAggregateSeconds=$stageStart
            elapsedSeconds=($trialClock.Elapsed.TotalSeconds-$stageStart)}
    } finally {
        if ($launched -and -not $process.HasExited) {
            & "$env:SystemRoot\System32\taskkill.exe" /PID $process.Id /T /F | Out-Null
            if (-not $process.WaitForExit(5000)) { $session.failure='Owned process-tree exit remains unconfirmed.' }
        }
        $process.Dispose()
    }
}

Save-Session
try {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
namespace ReadmeTimingPower {
    [StructLayout(LayoutKind.Sequential)]
    public struct Status {
        public byte ACLineStatus, BatteryFlag, BatteryLifePercent, SystemStatusFlag;
        public uint BatteryLifeTime, BatteryFullLifeTime;
    }
    public static class Native {
        [DllImport("kernel32.dll", SetLastError=true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        public static extern bool GetSystemPowerStatus(out Status status);
    }
}
'@
    $session.powerBefore=Read-Power
    Save-Session
    $env:PATH = 'C:\Users\symbi\.cargo\bin;' + $env:PATH
    $cargo='C:\Users\symbi\.cargo\bin\cargo.exe'
    $session.phase='offline-release-build'
    Save-Session
    $session.build=Run-OwnedStage 'build' $cargo @('+nightly-2026-11-09','test','--offline','--locked','--release','--manifest-path',(Join-Path $Repository 'Cargo.toml'),'--package','bonsai-broker','--test','native_model','--no-run','--message-format=json') 270
    Save-Session
    if ($session.build.exitCode -ne 0 -or $session.build.expired) { throw 'The single offline build failed or exceeded its admitted setup allowance; no retry.' }
    $artifacts=@(Get-Content -LiteralPath (Join-Path $OutputDirectory 'build.stdout.log') -Encoding UTF8 | ForEach-Object { $_ | ConvertFrom-Json } | Where-Object { $_.reason -eq 'compiler-artifact' -and $_.target.name -eq 'native_model' -and $_.executable })
    if ($artifacts.Count -ne 1) { throw 'Exactly one source-built test executable is required.' }
    $test=$artifacts[0].executable
    $session.testExecutable=@{bytes=(Get-Item -LiteralPath $test).Length; sha256=(Get-FileHash -LiteralPath $test -Algorithm SHA256).Hash.ToLowerInvariant()}
    $session.phase='focused-synthetic-controls'
    Save-Session
    $session.controls=Run-OwnedStage 'controls' $test @($ControlFilter,'--nocapture') 290
    Save-Session
    if ($session.controls.exitCode -ne 0 -or $session.controls.expired) { throw 'Focused timing controls failed; no model trial or retry.' }
    $controlOutput=Get-Content -LiteralPath (Join-Path $OutputDirectory 'controls.stdout.log') -Raw -Encoding UTF8
    foreach ($requiredControl in @('token_timing::native_token_timing_requires_source_supported_boundaries_and_positive_interval','progress::native_progress_token_timing_retains_fragmented_deltas_and_cancelled_prefix')) {
        if (-not $controlOutput.Contains("test $requiredControl ... ok")) { throw 'Required named timing control did not pass; no model trial.' }
    }
    $caseSeconds=[int][Math]::Floor($maximumSeconds-$trialClock.Elapsed.TotalSeconds-$processReserveSeconds)
    if ($caseSeconds -lt 260) { throw 'The original allowance cannot conservatively fit the declared 260 seconds for both startups, requests and cleanup.' }
    $modelEvidence=Join-Path $OutputDirectory 'models'
    [void][IO.Directory]::CreateDirectory($modelEvidence)
    $env:BONSAI_NATIVE_OWNER_TIMING='true'
    $env:BONSAI_NATIVE_PROFILE='pq2-ptq1-token-timing-row4'
    $env:BONSAI_NATIVE_OWNER_REVISION=$Revision
    $env:BONSAI_NATIVE_OWNER_RUN_ID='readme-performance-20261109'
    $env:BONSAI_NATIVE_CASE_BUDGET_SECONDS=[string]$caseSeconds
    $env:BONSAI_NATIVE_WHOLE_JOB_SECONDS=[string]$maximumSeconds
    $env:BONSAI_NATIVE_DATA='C:\Users\symbi\AppData\Local\SwiftBonsai2'
    $env:BONSAI_NATIVE_EVIDENCE=$modelEvidence
    $env:BONSAI_NATIVE_EXTENDED='false'
    $env:BONSAI_NATIVE_STALLED_CONSUMER='false'
    $session.caseBudgetSeconds=$caseSeconds
    $session.phase='fixed-two-owner-capture'
    Save-Session
    $session.capture=Run-OwnedStage 'capture' $test @($ModelCase,'--exact','--ignored','--nocapture') ($maximumSeconds-45)
    $session.status='capture-failed-or-partial'
    $acceptancePath=Join-Path $modelEvidence 'acceptance.json'
    if ($session.capture.exitCode -eq 0 -and -not $session.capture.expired -and (Test-Path -LiteralPath $acceptancePath -PathType Leaf)) {
        $acceptance=Get-Content -LiteralPath $acceptancePath -Raw -Encoding UTF8 | ConvertFrom-Json
        if ($acceptance.passed -eq $true -and $acceptance.samples.Count -eq 2 -and $acceptance.revision -eq $Revision -and $acceptance.runtimeLock.version -eq '0.1.22') {
            $session.status='capture-returned-two-measured-arms'
        }
    }
} catch {
    $session.status='stopped'
    $session.failure=$_.Exception.Message
} finally {
    $session.phase='retention'
    $session.powerAfter=Read-Power
    Save-Session
    [void][IO.Directory]::CreateDirectory($publicDirectory)
    Copy-Item -LiteralPath (Join-Path $Repository 'target\tmp\run-readme-timing.ps1') -Destination (Join-Path $publicDirectory 'capture-driver.ps1')
    foreach ($relative in @('session.json','build.stdout.log','build.stderr.log','controls.stdout.log','controls.stderr.log','capture.stdout.log','capture.stderr.log','models\acceptance.json')) {
        $path=Join-Path $OutputDirectory $relative
        if (Test-Path -LiteralPath $path -PathType Leaf) {
            $leaf=if ($relative -eq 'models\acceptance.json') { 'acceptance.json' } else { $relative }
            Copy-Item -LiteralPath $path -Destination (Join-Path $publicDirectory $leaf)
        }
    }
    $session.phase='closed'
    $session.finishedUtc=[DateTime]::UtcNow.ToString('o')
    $session.withinAggregateAllowance=($trialClock.Elapsed.TotalSeconds -lt $maximumSeconds)
    Save-Session
    Copy-Item -LiteralPath $receiptPath -Destination (Join-Path $publicDirectory 'session.json')
    $manifest=@(Get-ChildItem -LiteralPath $publicDirectory -File | Where-Object { $_.Name -ne 'retention.json' } | ForEach-Object { [ordered]@{path=$_.Name; bytes=$_.Length; sha256=(Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()} })
    [IO.File]::WriteAllText((Join-Path $publicDirectory 'retention.json'), ($manifest | ConvertTo-Json -Depth 8), $utf8)
    $finalClock=[ordered]@{kind='final-retention-clock'; elapsedSecondsThroughEvidenceAndManifestRetention=$trialClock.Elapsed.TotalSeconds
        scope='Clock and power follow evidence/manifest retention; exclude writing this final small clock receipt itself.'
        maximumAggregateSeconds=$maximumSeconds; withinAggregateAllowance=($trialClock.Elapsed.TotalSeconds -lt $maximumSeconds)
        powerAfterFinalRetention=(Read-Power); retries=0; ciActions=0; closed=$true}
    [IO.File]::WriteAllText((Join-Path $publicDirectory 'final-clock.json'), ($finalClock | ConvertTo-Json -Depth 8), $utf8)
    Write-Output ($session | Select-Object status,phase,failure,elapsedSeconds,withinAggregateAllowance,build,controls,capture | ConvertTo-Json -Depth 8)
    Write-Output ($finalClock | ConvertTo-Json -Depth 8)
}
