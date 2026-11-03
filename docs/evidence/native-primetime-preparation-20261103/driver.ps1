$ErrorActionPreference = 'Stop'
$trialStartedTick = [Diagnostics.Stopwatch]::GetTimestamp()
$trialStartedUtc = [DateTime]::UtcNow.ToString('o')
$trialFrequency = [Diagnostics.Stopwatch]::Frequency
$trialRoot = $PSScriptRoot
$trialPlan = Get-Content -LiteralPath (Join-Path $trialRoot 'plan.json') -Raw | ConvertFrom-Json
$trialReceiptPath = Join-Path $trialRoot 'receipt.json'
if (Test-Path -LiteralPath $trialReceiptPath) { throw 'This fixed workload already has a receipt; its clock must not restart.' }
$trialRepository = (Resolve-Path -LiteralPath (Join-Path $trialRoot '../../..')).Path
$trialPowerReady = $false
try { Add-Type -AssemblyName System.Windows.Forms; $trialPowerReady = $true } catch { }
function Get-TrialPower {
    if (-not $trialPowerReady) {
        return @{ observedAtUtc=[DateTime]::UtcNow.ToString('o'); acConnected=$null; chargePercent=$null; batterySaver='unknown' }
    }
    $observation = [Windows.Forms.SystemInformation]::PowerStatus
    $connected = $null
    if ($observation.PowerLineStatus.ToString() -eq 'Online') { $connected = $true }
    if ($observation.PowerLineStatus.ToString() -eq 'Offline') { $connected = $false }
    return @{ observedAtUtc=[DateTime]::UtcNow.ToString('o'); acConnected=$connected; chargePercent=100*$observation.BatteryLifePercent; batteryFlags=$observation.BatteryChargeStatus.ToString(); batterySaver='unknown' }
}
function Get-TrialElapsed { return ([Diagnostics.Stopwatch]::GetTimestamp()-$trialStartedTick)/[double]$trialFrequency }
$trialReceipt = [ordered]@{
    schemaVersion=1; kind='focused-native-primetime-preparation'; startedAtUtc=$trialStartedUtc
    startedTick=$trialStartedTick; tickFrequency=$trialFrequency; repository=$trialRepository
    maximumTotalSeconds=$trialPlan.maximumTotalSeconds; workCutoffSeconds=$trialPlan.workCutoffSeconds
    powerBefore=(Get-TrialPower); checks=@(); modelExecutions=0; benchmarks=0; fullSuites=0; ciActions=0
    performanceComparisonQualified=$false; defaultPromotionQualified=$false; allRequestedChecksPassed=$false
}
function Save-TrialReceipt {
    $trialReceipt.elapsedSeconds=Get-TrialElapsed
    [IO.File]::WriteAllText($trialReceiptPath, ($trialReceipt | ConvertTo-Json -Depth 12), (New-Object Text.UTF8Encoding($false)))
}
Save-TrialReceipt
try {
    $trialCompiler = 'C:\Users\symbi\.rustup\toolchains\nightly-2026-11-02-aarch64-pc-windows-msvc\bin'
    $trialCargo = Join-Path $trialCompiler 'cargo.exe'
    $env:PATH = $trialCompiler + [IO.Path]::PathSeparator + $env:PATH
    $env:RUSTC = Join-Path $trialCompiler 'rustc.exe'
    $env:RUSTDOC = Join-Path $trialCompiler 'rustdoc.exe'
    $env:CARGO_UNSTABLE_CHECKSUM_FRESHNESS = 'true'
    $env:CARGO_BUILD_FINGERPRINT = 'content'
    $env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA 'Temp/ptq1-local-validation-20261103/target'
    $trialCommon = @('--locked','--offline','--release','--manifest-path',(Join-Path $trialRepository 'Cargo.toml'))
    $trialGit = Join-Path $env:LOCALAPPDATA 'Microsoft/WinGet/Packages/Git.MinGit_Microsoft.Winget.Source_8wekyb3d8bbwe/cmd/git.exe'
    $trialReceipt.sourceRevision = (& $trialGit -C $trialRepository rev-parse HEAD)
    if ($LASTEXITCODE -ne 0) { throw 'Source revision lookup failed.' }
    $trialReceipt.sourceFiles = @('tools/bonsai/src/native_acceptance_budget.rs','src/broker/tests/native_model/service_integration.rs','src/broker/tests/native_model/service_rollback.rs','.github/workflows/native-runtime-acceptance.yml','.github/workflows/native-ptq1-numerics.yml','.github/workflows/native-row-qualification.yml') | ForEach-Object {
        $sourcePath = Join-Path $trialRepository $_
        @{ file=$_; bytes=(Get-Item -LiteralPath $sourcePath).Length; sha256=(Get-FileHash -LiteralPath $sourcePath -Algorithm SHA256).Hash.ToLowerInvariant() }
    }
    $parseCheck = [ordered]@{ name='workflow-powershell-parse'; powerBefore=(Get-TrialPower); startedSeconds=(Get-TrialElapsed); blocks=0; passed=$false }
    $trialReceipt.checks += @($parseCheck)
    foreach ($workflow in @('native-runtime-acceptance','native-ptq1-numerics','native-row-qualification')) {
        $workflowText = [IO.File]::ReadAllText((Join-Path $trialRepository ('.github/workflows/'+$workflow+'.yml')))
        $blocks = [regex]::Matches($workflowText, '(?m)^        run: \|\r?\n((?:          [^\r\n]*(?:\r?\n|$)|\r?\n)+)')
        if ($blocks.Count -eq 0) { throw 'Expected literal workflow run blocks.' }
        foreach ($block in $blocks) {
            $scriptText = [regex]::Replace($block.Groups[1].Value, '(?m)^          ', '')
            $scriptText = [regex]::Replace($scriptText, '\$\{\{.*?\}\}', 'hosted-expression')
            $parseTokens=$null; $parseErrors=$null
            [void][System.Management.Automation.Language.Parser]::ParseInput($scriptText, [ref]$parseTokens, [ref]$parseErrors)
            if (@($parseErrors).Count -ne 0) { throw ('Workflow PowerShell parse failed: '+$workflow+' '+$parseErrors[0].Message) }
            $parseCheck.blocks++
        }
    }
    $parseCheck.passed=$true; $parseCheck.powerAfter=Get-TrialPower; $parseCheck.finishedSeconds=Get-TrialElapsed
    Save-TrialReceipt
    $trialJobs = @(
        @{name='changed-rust-format'; executable=(Join-Path $trialCompiler 'rustfmt.exe'); arguments=@('--check','--edition','2024','tools/bonsai/src/native_acceptance_budget.rs','src/broker/tests/native_model/service_integration.rs','src/broker/tests/native_model/service_rollback.rs')},
        @{name='budget-controls'; executable=$trialCargo; arguments=@('test')+$trialCommon+@('-p','bonsai-tools','--lib','native_acceptance_budget::tests::')},
        @{name='service-cleanup-controls'; executable=$trialCargo; arguments=@('test')+$trialCommon+@('-p','bonsai-broker','--test','native_model','native_service_integration_')},
        @{name='rollback-controls'; executable=$trialCargo; arguments=@('test')+$trialCommon+@('-p','bonsai-broker','--test','native_model','native_service_rollback_')},
        @{name='tools-release-clippy'; executable=$trialCargo; arguments=@('clippy')+$trialCommon+@('-p','bonsai-tools','--lib','--','-D','warnings')},
        @{name='acceptance-release-clippy'; executable=$trialCargo; arguments=@('clippy')+$trialCommon+@('-p','bonsai-broker','--test','native_model','--','-D','warnings')}
    )
    foreach ($job in $trialJobs) {
        $remaining=$trialPlan.workCutoffSeconds-(Get-TrialElapsed)
        if ($remaining -lt 5) { throw 'The fixed source-verification work cutoff was reached.' }
        $check = [ordered]@{name=$job.name; arguments=$job.arguments; startedSeconds=(Get-TrialElapsed); powerBefore=(Get-TrialPower); passed=$false}
        $trialReceipt.checks += @($check)
        Save-TrialReceipt
        $stdoutPath=Join-Path $trialRoot ($job.name+'.stdout.log')
        $stderrPath=Join-Path $trialRoot ($job.name+'.stderr.log')
        $child=$null
        try {
            Write-Output ('Running '+$job.name)
            $child=Start-Process -FilePath $job.executable -ArgumentList $job.arguments -WorkingDirectory $trialRepository -WindowStyle Hidden -PassThru -RedirectStandardOutput $stdoutPath -RedirectStandardError $stderrPath
            if (-not $child.WaitForExit([int]($remaining*1000))) { throw 'The check exceeded the original work cutoff.' }
            $child.Refresh()
            $check.exitCode=$child.ExitCode; $check.passed=$child.ExitCode -eq 0
            if ($check.passed -and $job.arguments[0] -eq 'test') {
                $matched=[regex]::Match([IO.File]::ReadAllText($stdoutPath), 'test result: ok\. ([1-9][0-9]*) passed;')
                $check.passed=$matched.Success
                if ($matched.Success) { $check.testsPassed=[int]$matched.Groups[1].Value }
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
    $trialReceipt.powerAfter=Get-TrialPower
    $trialReceipt.uninterruptedPowerStabilityEstablished=$false
    Save-TrialReceipt
    $manifest = Get-ChildItem -LiteralPath $trialRoot -File | Where-Object { $_.Name -ne 'file-manifest.json' } | ForEach-Object {
        @{ file=$_.Name; bytes=$_.Length; sha256=(Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant() }
    }
    $retention = @{ files=@($manifest); elapsedSecondsThroughRetention=(Get-TrialElapsed); maximumTotalSeconds=$trialPlan.maximumTotalSeconds; retainedWithinDeclaredTotal=((Get-TrialElapsed) -le $trialPlan.maximumTotalSeconds) }
    [IO.File]::WriteAllText((Join-Path $trialRoot 'file-manifest.json'), ($retention | ConvertTo-Json -Depth 6), (New-Object Text.UTF8Encoding($false)))
    Write-Output ('Charged seconds through retention: '+(Get-TrialElapsed))
    Write-Output ('All requested checks passed: '+$trialReceipt.allRequestedChecksPassed)
}
