$ErrorActionPreference = 'Stop'
$trialStart = [Diagnostics.Stopwatch]::GetTimestamp()
$trialUtc = [DateTime]::UtcNow.ToString('o')
$trialFrequency = [Diagnostics.Stopwatch]::Frequency
$trialRoot = $PSScriptRoot
$trialPlan = Get-Content -LiteralPath (Join-Path $trialRoot 'plan.json') -Raw | ConvertFrom-Json
$trialReceiptPath = Join-Path $trialRoot 'receipt.json'
if (Test-Path -LiteralPath $trialReceiptPath) { throw 'This fixed verification already has a receipt; do not restart its clock.' }
$trialRepo = (Resolve-Path -LiteralPath (Join-Path $trialRoot '../../..')).Path
$trialPowerReady = $false
try { Add-Type -AssemblyName System.Windows.Forms; $trialPowerReady = $true } catch { }
function Get-TrialElapsed { return ([Diagnostics.Stopwatch]::GetTimestamp() - $trialStart) / [double]$trialFrequency }
function Get-TrialPower {
    $sample = [ordered]@{ observedAtUtc=[DateTime]::UtcNow.ToString('o'); acConnected=$null; chargePercent=$null; batterySaver='unknown'; source='Windows.Forms.PowerStatus' }
    if ($trialPowerReady) {
        try {
            $power = [Windows.Forms.SystemInformation]::PowerStatus
            if ($power.PowerLineStatus.ToString() -eq 'Online') { $sample.acConnected = $true }
            if ($power.PowerLineStatus.ToString() -eq 'Offline') { $sample.acConnected = $false }
            if ($power.BatteryLifePercent -ge 0 -and $power.BatteryLifePercent -le 1) { $sample.chargePercent = 100 * $power.BatteryLifePercent }
        } catch { }
    }
    return $sample
}
$trialReceipt = [ordered]@{
    schemaVersion=1; kind='new-owner-packet-controls-source-verification'; startedAtUtc=$trialUtc
    startedTick=$trialStart; tickFrequency=$trialFrequency; sourceRevision=$null; repository=$trialRepo
    maximumTotalSeconds=$trialPlan.maximumTotalSeconds; workCutoffSeconds=$trialPlan.workCutoffSeconds
    powerBefore=(Get-TrialPower); checks=@(); allRequestedChecksPassed=$false
    modelExecutions=0; benchmarks=0; fullSuites=0; ciActions=0; pushes=0
    performanceComparisonQualified=$false; defaultPromotionQualified=$false
}
function Save-TrialReceipt {
    $trialReceipt.elapsedSeconds = Get-TrialElapsed
    [IO.File]::WriteAllText($trialReceiptPath, ($trialReceipt | ConvertTo-Json -Depth 12), (New-Object Text.UTF8Encoding($false)))
}
Save-TrialReceipt
try {
    $trialCompiler = 'C:\Users\symbi\.rustup\toolchains\nightly-2026-11-02-aarch64-pc-windows-msvc\bin'
    $env:PATH = $trialCompiler + [IO.Path]::PathSeparator + $env:PATH
    $env:RUSTC = Join-Path $trialCompiler 'rustc.exe'
    $env:RUSTDOC = Join-Path $trialCompiler 'rustdoc.exe'
    $env:CARGO_UNSTABLE_CHECKSUM_FRESHNESS = 'true'
    $env:CARGO_BUILD_FINGERPRINT = 'content'
    $env:CARGO_TARGET_DIR = Join-Path $env:TEMP 'ptq1-local-validation-20261103/target'
    $trialGit = 'C:\Program Files\Microsoft Visual Studio\18\Enterprise\Common7\IDE\CommonExtensions\Microsoft\TeamFoundation\Team Explorer\Git\cmd\git.exe'
    $trialReceipt.sourceRevision = (& $trialGit -C $trialRepo rev-parse HEAD)
    if ($LASTEXITCODE -ne 0) { throw 'Exact source lookup failed.' }
    $trialCommon = @('--locked','--offline','--release','--manifest-path',(Join-Path $trialRepo 'Cargo.toml'),'-p','bonsai-broker','--test','native_model')
    $trialCargo = Join-Path $trialCompiler 'cargo.exe'
    $trialJobs = @(
        @{name='canonical-source-format'; executable=(Join-Path $trialCompiler 'rustfmt.exe'); arguments=@('--edition','2024','src/broker/tests/native_model/service_integration.rs','src/broker/tests/native_model/service_rollback.rs')},
        @{name='listener-refusal-control'; executable=$trialCargo; arguments=@('test')+$trialCommon+@('service_integration::native_service_integration_listener_release_requires_connection_refusal','--','--exact')},
        @{name='cleanup-observations-control'; executable=$trialCargo; arguments=@('test')+$trialCommon+@('service_integration::native_service_integration_cleanup_preserves_failures_and_listener_observations','--','--exact')},
        @{name='stuck-owner-release-control'; executable=$trialCargo; arguments=@('test')+$trialCommon+@('service_integration::native_service_integration_stuck_owner_is_dropped_inside_existing_stop_allowance','--','--exact')},
        @{name='rollback-tail-control'; executable=$trialCargo; arguments=@('test')+$trialCommon+@('service_rollback::native_service_rollback_reserves_tail_before_ptq1_owner','--','--exact')},
        @{name='acceptance-target-release-clippy'; executable=$trialCargo; arguments=@('clippy')+$trialCommon+@('--','-D','warnings')}
    )
    $parseCheck = [ordered]@{name='workflow-powershell-parse'; startedSeconds=(Get-TrialElapsed); powerBefore=(Get-TrialPower); blocks=0; passed=$false}
    $trialReceipt.checks += @($parseCheck)
    try {
        $workflow = [IO.File]::ReadAllText((Join-Path $trialRepo '.github/workflows/native-runtime-acceptance.yml'))
        $blocks = [regex]::Matches($workflow, '(?m)^        run: \|\r?\n((?:          [^\r\n]*(?:\r?\n|$)|\r?\n)+)')
        if ($blocks.Count -eq 0) { throw 'Expected literal workflow PowerShell blocks.' }
        foreach ($block in $blocks) {
            $text = [regex]::Replace($block.Groups[1].Value, '(?m)^          ', '')
            $text = [regex]::Replace($text, '\$\{\{.*?\}\}', 'hosted-expression')
            $tokens=$null; $errors=$null
            [void][Management.Automation.Language.Parser]::ParseInput($text, [ref]$tokens, [ref]$errors)
            if (@($errors).Count -ne 0) { throw ('Workflow PowerShell parse failed: ' + $errors[0].Message) }
            $parseCheck.blocks++
        }
        $parseCheck.passed=$true
    } finally {
        $parseCheck.powerAfter=Get-TrialPower; $parseCheck.finishedSeconds=Get-TrialElapsed
        Save-TrialReceipt
    }
    foreach ($job in $trialJobs) {
        $remaining = $trialPlan.workCutoffSeconds - (Get-TrialElapsed)
        if ($remaining -lt 5) { throw 'Original work cutoff reached; remaining checks stay pending.' }
        $check = [ordered]@{name=$job.name; arguments=$job.arguments; startedSeconds=(Get-TrialElapsed); powerBefore=(Get-TrialPower); passed=$false}
        $trialReceipt.checks += @($check)
        Save-TrialReceipt
        $stdoutPath=Join-Path $trialRoot ($job.name+'.stdout.log')
        $stderrPath=Join-Path $trialRoot ($job.name+'.stderr.log')
        $child=$null
        try {
            Write-Output ('Running '+$job.name)
            $child=Start-Process -FilePath $job.executable -ArgumentList $job.arguments -WorkingDirectory $trialRepo -WindowStyle Hidden -PassThru -RedirectStandardOutput $stdoutPath -RedirectStandardError $stderrPath
            if (-not $child.WaitForExit([int]($remaining*1000))) { throw 'The original work cutoff expired.' }
            $child.Refresh()
            $check.exitCode=$child.ExitCode; $check.passed=$child.ExitCode -eq 0
            if ($check.passed -and $job.arguments[0] -eq 'test') {
                $match=[regex]::Match([IO.File]::ReadAllText($stdoutPath), 'test result: ok\. ([1-9][0-9]*) passed;')
                $check.passed=$match.Success
                if ($match.Success) { $check.testsPassed=[int]$match.Groups[1].Value }
            }
        } finally {
            if ($child -and -not $child.HasExited) {
                & 'C:\Windows\System32\taskkill.exe' /PID $child.Id /T /F | Out-Null
                [void]$child.WaitForExit(10000)
            }
            $check.powerAfter=Get-TrialPower; $check.finishedSeconds=Get-TrialElapsed
            Save-TrialReceipt
        }
        if (-not $check.passed) { throw ('Focused check failed: '+$job.name) }
        Write-Output ($job.name+' passed')
    }
    $trialReceipt.allRequestedChecksPassed=$true
} catch {
    $trialReceipt.failure=$_.Exception.Message
    Write-Output $trialReceipt.failure
} finally {
    $trialReceipt.sourceFiles = @('src/broker/tests/native_model/service_integration.rs','src/broker/tests/native_model/service_rollback.rs','.github/workflows/native-runtime-acceptance.yml') | ForEach-Object {
        $path=Join-Path $trialRepo $_
        @{file=$_; bytes=(Get-Item -LiteralPath $path).Length; sha256=(Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()}
    }
    $trialReceipt.powerAfter=Get-TrialPower
    $trialReceipt.uninterruptedPowerStabilityEstablished=$false
    $trialReceipt.clockClosed=$true
    Save-TrialReceipt
    $manifest = Get-ChildItem -LiteralPath $trialRoot -File | Where-Object { $_.Name -ne 'file-manifest.json' } | ForEach-Object {
        @{file=$_.Name; bytes=$_.Length; sha256=(Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()}
    }
    $retention = @{files=@($manifest); elapsedSecondsThroughRetention=(Get-TrialElapsed); maximumTotalSeconds=$trialPlan.maximumTotalSeconds; retainedWithinDeclaredTotal=((Get-TrialElapsed) -le $trialPlan.maximumTotalSeconds)}
    [IO.File]::WriteAllText((Join-Path $trialRoot 'file-manifest.json'), ($retention | ConvertTo-Json -Depth 6), (New-Object Text.UTF8Encoding($false)))
    Write-Output ('Charged seconds through retention: '+(Get-TrialElapsed))
    Write-Output ('All requested checks passed: '+$trialReceipt.allRequestedChecksPassed)
}
