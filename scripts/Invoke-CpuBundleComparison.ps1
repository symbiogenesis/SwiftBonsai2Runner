[CmdletBinding(DefaultParameterSetName = 'Capture')]
param(
    [Parameter(Mandatory)][string]$StartReceipt,
    [Parameter(Mandatory, ParameterSetName = 'Create')][switch]$CreateOwnerStartReceipt,
    [Parameter(Mandatory, ParameterSetName = 'Create')][string]$RunId,
    [Parameter(Mandatory, ParameterSetName = 'Create')][string]$RunnerCommit,
    [Parameter(Mandatory, ParameterSetName = 'Create')]
    [Parameter(Mandatory, ParameterSetName = 'Capture')][string]$CandidateCommit,
    [Parameter(Mandatory, ParameterSetName = 'Finalize')][switch]$FinalizeBudget,
    [Parameter(Mandatory, ParameterSetName = 'Capture')][string]$PacketDirectory,
    [Parameter(Mandatory, ParameterSetName = 'Capture')][string]$ExpectedPacketSha256,
    [Parameter(Mandatory, ParameterSetName = 'Capture')][string]$ToolingCommit,
    [Parameter(ParameterSetName = 'Capture')][string]$EvidenceDirectory,
    [Parameter(ParameterSetName = 'Capture')]
    [Parameter(ParameterSetName = 'Finalize')][string]$ToolsPath = $env:BONSAI_TOOLS_PATH
)

$entryTick = [Diagnostics.Stopwatch]::GetTimestamp()
$entryUnix = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$baselineCommit = '055c7430cb4771dfddb24ddaa30494af6ad182da'
$frequency = [Diagnostics.Stopwatch]::Frequency
$utf8 = [Text.UTF8Encoding]::new($false)

function Assert-Hash([string]$Value, [int]$Length) {
    if ($Value -cnotmatch "^[a-f0-9]{$Length}$") { throw 'Expected an exact lowercase source or SHA-256 identity.' }
}

function Read-Json([string]$Path) {
    $file = Get-Item -LiteralPath $Path
    if ($file.PSIsContainer -or $file.Length -gt 4194304) { throw 'Missing or oversized JSON input.' }
    return [IO.File]::ReadAllText($file.FullName) | ConvertFrom-Json -AsHashtable
}

function Write-Json([string]$Path, [object]$Value, [switch]$NewFile) {
    $text = ($Value | ConvertTo-Json -Depth 40) + "`n"
    if ($NewFile) {
        $stream = [IO.File]::Open($Path, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
        try { $bytes = $utf8.GetBytes($text); $stream.Write($bytes, 0, $bytes.Length); $stream.Flush($true) } finally { $stream.Dispose() }
    } else { [IO.File]::WriteAllText($Path, $text, $utf8) }
}

function File-Digest([string]$Path) {
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function External-Path([string]$Path) {
    if (-not [IO.Path]::IsPathFullyQualified($Path) -or $Path -match '[\r\n\x00]') {
        throw 'Expected an expanded absolute external path.'
    }
    $absolute = [IO.Path]::GetFullPath($Path)
    if ($env:GITHUB_WORKSPACE) {
        $checkout = [IO.Path]::GetFullPath($env:GITHUB_WORKSPACE).TrimEnd('\') + '\'
        if ($absolute.StartsWith($checkout, [StringComparison]::OrdinalIgnoreCase)) {
            throw 'Model capture inputs and outputs must remain outside the Git checkout.'
        }
    }
    $ancestor = [IO.Path]::GetDirectoryName($absolute)
    while ($ancestor) {
        if (Test-Path -LiteralPath (Join-Path $ancestor '.git')) { throw 'External capture paths cannot be inside a Git checkout.' }
        $ancestor = [IO.Path]::GetDirectoryName($ancestor)
    }
    return $absolute
}

function Packet-Member([string]$Root, [string]$Relative) {
    if (-not $Relative -or $Relative -match '[:\\]' -or [IO.Path]::IsPathRooted($Relative)) {
        throw 'Packet member must be a bounded relative path.'
    }
    $prefix = [IO.Path]::GetFullPath($Root).TrimEnd('\') + '\'
    $member = [IO.Path]::GetFullPath((Join-Path $Root $Relative))
    if (-not $member.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Packet member escaped its root.'
    }
    return $member
}

function Native-Power {
    if (-not ('CpuBundlePower' -as [type])) {
        Add-Type -TypeDefinition @'
using System.Runtime.InteropServices;
public static class CpuBundlePower {
    [StructLayout(LayoutKind.Sequential)]
    public struct Value {
        public byte ACLineStatus, BatteryFlag, BatteryLifePercent, SystemStatusFlag;
        public uint BatteryLifeTime, BatteryFullLifeTime;
    }
    [DllImport("kernel32.dll", SetLastError=true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool GetSystemPowerStatus(out Value value);
}
'@
    }
    $value = [CpuBundlePower+Value]::new()
    if (-not [CpuBundlePower]::GetSystemPowerStatus([ref]$value)) {
        return @{ status = 'unknown'; source = 'GetSystemPowerStatus'; ac_online = $null; battery_saver = $null; battery_percent = $null }
    }
    return @{
        status = 'observed'; source = 'GetSystemPowerStatus'
        ac_online = $(if ($value.ACLineStatus -eq 1) { $true } elseif ($value.ACLineStatus -eq 0) { $false } else { $null })
        battery_saver = $(if ($value.SystemStatusFlag -eq 1) { $true } elseif ($value.SystemStatusFlag -eq 0) { $false } else { $null })
        battery_percent = $(if ($value.BatteryLifePercent -le 100) { [int]$value.BatteryLifePercent } else { $null })
    }
}

if (-not $IsWindows -or [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture -ne 'Arm64' -or
    $PSVersionTable.PSVersion -lt [version]'7.6') { throw 'Use native Windows ARM64 PowerShell 7.6 or later.' }
$StartReceipt = External-Path $StartReceipt

if ($CreateOwnerStartReceipt) {
    Assert-Hash $RunnerCommit 40
    Assert-Hash $CandidateCommit 40
    if ($RunId -cnotmatch '^[1-9][0-9]*$' -or (Test-Path -LiteralPath $StartReceipt)) {
        throw 'Use the actual dispatched run ID and a new external owner-start receipt.'
    }
    # Run this once before registering/starting the run-specific ephemeral runner.
    # Its clock includes bootstrap, intake, all four arms and final retention.
    $started = $entryTick
    $unix = $entryUnix
    Write-Json $StartReceipt @{
        schema_version = 1; kind = 'prism-cpu-bundle-owner-start'; run_id = $RunId
        runner_commit = $RunnerCommit; candidate_commit = $CandidateCommit
        clock_frequency = $frequency; started_tick = $started; deadline_tick = $started + 600L * $frequency
        started_unix_milliseconds = $unix; deadline_unix_milliseconds = $unix + 600000L
        power_before = (Native-Power); budget_seconds = 600; conservative_estimate_seconds = 600
        planning_estimate_seconds = @(300, 480)
        schedule = @('baseline', 'candidate', 'candidate', 'baseline')
        fixed_work = 'One bundle-only ABBA comparison with 32 output tokens reserved per arm: each collector has 60 seconds, each supervised helper at most 95 seconds, with 10 seconds condition checks and 5 seconds metadata. Reserve 60 seconds bootstrap/intake and 100 seconds cleanup/evaluation/upload; 600 seconds maximum aggregate. Planning estimate 5-8 minutes is not an empirical timing prediction. Stop once complete or infeasible; no retry or split matrix.'
    } -NewFile
    return
}

foreach ($entry in @{
    GITHUB_ACTIONS = 'true'; GITHUB_REF = 'refs/heads/main'
    GITHUB_REPOSITORY = 'symbiogenesis/SwiftBonsai2Runner'; RUNNER_ARCH = 'ARM64'
    RUNNER_ENVIRONMENT = 'self-hosted'
}.GetEnumerator()) {
    if ([Environment]::GetEnvironmentVariable($entry.Key) -cne $entry.Value) {
        throw 'CPU bundle capture requires the owning main-only Windows ARM64 CI lane.'
    }
}
if ($env:GITHUB_RUN_ID -cnotmatch '^[1-9][0-9]*$' -or -not $env:RUNNER_NAME) {
    throw 'Identified native owner CI execution is required.'
}
Assert-Hash $env:GITHUB_SHA 40
$original = Read-Json $StartReceipt
if ($original.schema_version -ne 1 -or $original.kind -cne 'prism-cpu-bundle-owner-start' -or
    $original.run_id -cne $env:GITHUB_RUN_ID -or $original.runner_commit -cne $env:GITHUB_SHA -or
    $original.clock_frequency -ne $frequency -or $original.budget_seconds -ne 600 -or
    $original.deadline_tick -ne $original.started_tick + 600L * $frequency -or
    $original.deadline_unix_milliseconds -ne $original.started_unix_milliseconds + 600000L -or
    $original.started_tick -ge [Diagnostics.Stopwatch]::GetTimestamp()) {
    throw 'The original pre-bootstrap owner allowance is missing, foreign or malformed.'
}
$deadline = [long]$original.deadline_tick
function Remaining-Seconds { return ($deadline - [Diagnostics.Stopwatch]::GetTimestamp()) / [double]$frequency }
if ([Math]::Abs(($original.deadline_unix_milliseconds - [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()) / 1000.0 - (Remaining-Seconds)) -gt 2) {
    throw 'The original monotonic and UTC deadline anchors no longer agree.'
}

if ($FinalizeBudget) {
    $powerAfter = Native-Power
    $completed = [Diagnostics.Stopwatch]::GetTimestamp()
    $powerMatched = $powerAfter.status -ceq 'observed' -and $powerAfter.ac_online -is [bool] -and
        $powerAfter.battery_saver -is [bool] -and $original.power_before.status -ceq 'observed' -and
        $original.power_before.ac_online -is [bool] -and $original.power_before.battery_saver -is [bool] -and
        $powerAfter.ac_online -ceq $original.power_before.ac_online -and
        $powerAfter.battery_saver -ceq $original.power_before.battery_saver
    $final = @{
        kind = 'prism-cpu-bundle-post-upload-budget'; run_id = $env:GITHUB_RUN_ID
        original_deadline_tick = $deadline; completed_tick = $completed
        elapsed_seconds = ($completed - $original.started_tick) / [double]$frequency
        power_after = $powerAfter; power_endpoints_match = $powerMatched; full_promotion_qualified = $false
    }
    $final | ConvertTo-Json -Depth 5 -Compress | Add-Content -LiteralPath $env:GITHUB_STEP_SUMMARY
    if ((Remaining-Seconds) -le 0) { throw 'The aggregate owner allowance expired through upload/final recording.' }
    if (-not $powerMatched) { throw 'Final aggregate power is unknown or mismatched; comparison qualification remains pending.' }
    return
}

Assert-Hash $CandidateCommit 40
Assert-Hash $ToolingCommit 40
Assert-Hash $ExpectedPacketSha256 64
if ($original.candidate_commit -cne $CandidateCommit) { throw 'Owner-start candidate identity differs.' }
$PacketDirectory = External-Path $PacketDirectory
$ToolsPath = External-Path $ToolsPath
$toolsReceipt = Read-Json (Join-Path ([IO.Path]::GetDirectoryName($ToolsPath)) 'receipt.json')
if ($toolsReceipt.SchemaVersion -ne 2 -or $toolsReceipt.Revision -cne $env:GITHUB_SHA -or
    $toolsReceipt.Sha256 -cne (File-Digest $ToolsPath)) { throw 'Checked supervisor source or executable identity differs.' }
$toolDigest = $toolsReceipt.Sha256
$preparationPath = Join-Path $PacketDirectory 'preparation.json'
if ((File-Digest $preparationPath) -cne $ExpectedPacketSha256) { throw 'Same-run hosted preparation digest differs.' }
$packet = Read-Json $preparationPath
if ($packet.schema_version -ne 1 -or $packet.kind -cne 'prism-cpu-bundle-preparation' -or
    $packet.arms.baseline.source.revision -cne $baselineCommit -or
    $packet.arms.candidate.source.revision -cne $CandidateCommit -or
    $packet.target -cne 'aarch64-pc-windows-msvc' -or $packet.output_tokens -ne 32 -or $packet.per_arm_timeout_seconds -ne 60 -or
    $packet.profile.row_workers -ne 4 -or $packet.profile.kv_cache -cne 'f16' -or
    $packet.profile.adapter_scale_bits -cne '3fa66666' -or
    $packet.profile.backend_requested -cne 'ExperimentalNeon' -or
    $packet.profile.execution_device -cne 'cpu' -or $packet.profile.prompt_reuse -cne 'disabled' -or
    $packet.profile.seed -ne 42 -or $packet.profile.thinking_enabled -ne $false -or
    $packet.profile.execution_basis -cne 'direct-session-without-projection-executor' -or
    ($packet.schedule -join ',') -cne 'baseline,candidate,candidate,baseline') {
    throw 'Prepared bundle sources, CPU construction or fixed comparison profile differ.'
}
if ($packet.orchestrator.repository -cne $env:GITHUB_REPOSITORY -or
    $packet.orchestrator.revision -cne $env:GITHUB_SHA -or
    $packet.orchestrator.run_id -cne $env:GITHUB_RUN_ID -or
    [string]$packet.orchestrator.run_attempt -cne $env:GITHUB_RUN_ATTEMPT -or
    $packet.arms.candidate.source.kind -cne 'registry' -or
    $packet.arms.candidate.source.version -cne '0.1.25' -or
    $packet.arms.candidate.source.archive_sha256 -cne '0a117db7589a821b27c224e41aaababa5ddd2ac9852344c0a8121a0849046363' -or
    ($packet.retained_files -join ',') -cne 'preparation.json,messages.json,evaluator.py') {
    throw 'Preparation is foreign to this exact run or reviewed registry delivery.'
}
Assert-Hash $packet.tooling_source.revision 40
if ($packet.tooling_source.revision -cne $ToolingCommit -or $packet.tooling_source.clean -ne $true -or
    $packet.collector_sources.Count -lt 2) { throw 'Missing the selected clean, source-bound collector inputs.' }
foreach ($source in $packet.collector_sources.GetEnumerator()) {
    if ($source.Key -cnotmatch '^examples/cpu_bundle_check(?:\.rs|/[^:]+\.rs)$') { throw 'Unreviewed collector source member.' }
    Assert-Hash $source.Value 64
}
if ((File-Digest (Join-Path $PacketDirectory 'evaluator.py')) -cne $packet.evaluator_sha256) { throw 'Prepared evaluator bytes differ.' }
$messages = Packet-Member $PacketDirectory $packet.messages.path
if ((File-Digest $messages) -cne $packet.messages.sha256) { throw 'Fixed public messages differ.' }
foreach ($role in @('baseline', 'candidate')) {
    Assert-Hash $packet.arms[$role].driver_manifest_sha256 64
    Assert-Hash $packet.arms[$role].driver_lock_sha256 64
    Assert-Hash $packet.arms[$role].source.manifest_sha256 64
    Assert-Hash $packet.arms[$role].source.lock_sha256 64
    if (($packet.arms[$role].checks.phase -join ',') -cne 'source_analysis,portable_library,state_fixture,collector_analysis,collector_controls,collector_link' -or
        @($packet.arms[$role].checks | Where-Object { $_.exit_code -ne 0 }).Count -ne 0) {
        throw 'Required same-source hosted preparation checks are incomplete.'
    }
    $binary = Packet-Member $PacketDirectory $packet.arms[$role].executable.path
    if ((Get-Item -LiteralPath $binary).Length -ne $packet.arms[$role].executable.bytes -or
        (File-Digest $binary) -cne $packet.arms[$role].executable.sha256) { throw 'Prepared native collector executable differs.' }
}
if (-not $EvidenceDirectory) {
    $EvidenceDirectory = Join-Path $env:RUNNER_TEMP ('CpuBundle-Evidence-' + $env:GITHUB_RUN_ID + '-' + $env:GITHUB_RUN_ATTEMPT)
}
$EvidenceDirectory = External-Path $EvidenceDirectory
if (Test-Path -LiteralPath $EvidenceDirectory) { throw 'Use a new evidence root; earlier workloads cannot be resumed.' }
$working = Join-Path $EvidenceDirectory 'working'
$retained = Join-Path $EvidenceDirectory 'retained'
[void][IO.Directory]::CreateDirectory($working)
[void][IO.Directory]::CreateDirectory($retained)
"BONSAI_CPU_BUNDLE_EVIDENCE=$retained" >> $env:GITHUB_ENV
"BONSAI_CPU_BUNDLE_UPLOAD_ALLOWED=false" >> $env:GITHUB_ENV
$parent = [Diagnostics.Process]::GetCurrentProcess()
try { $parentCreation = $parent.StartTime.ToUniversalTime().ToFileTimeUtc() } finally { $parent.Dispose() }

function Invoke-Sanitized([string]$Executable, [string[]]$Arguments, [int]$MaximumSeconds, [int]$ReserveSeconds = 100) {
    $limit = [Math]::Min($MaximumSeconds, [Math]::Floor((Remaining-Seconds) - $ReserveSeconds))
    if ($limit -lt 1) { throw 'Original aggregate budget cannot fit another helper operation.' }
    $start = [Diagnostics.ProcessStartInfo]::new($Executable)
    $start.WorkingDirectory = $working; $start.UseShellExecute = $false; $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true; $start.RedirectStandardError = $true
    foreach ($argument in $Arguments) { $start.ArgumentList.Add($argument) }
    foreach ($name in @($start.Environment.Keys)) {
        if ($name -match '(?i)(TOKEN|SECRET|PASSWORD|CREDENTIAL)' -or
            $name -in @('ACTIONS_RUNTIME_URL', 'ACTIONS_RESULTS_URL', 'ACTIONS_CACHE_URL', 'ACTIONS_ID_TOKEN_REQUEST_URL')) {
            [void]$start.Environment.Remove($name)
        }
    }
    $child = [Diagnostics.Process]::Start($start)
    try {
        $stdout = $child.StandardOutput.ReadToEndAsync(); $stderr = $child.StandardError.ReadToEndAsync()
        if (-not $child.WaitForExit([int]($limit * 1000))) { throw 'Supervised helper exceeded the original remaining allowance.' }
        $text = $stdout.GetAwaiter().GetResult(); $diagnostic = $stderr.GetAwaiter().GetResult()
        if ($text.Length -gt 4194304 -or $diagnostic.Length -gt 4194304) { throw 'Supervisor diagnostic output exceeded its bound.' }
        if ($child.ExitCode -ne 0) { throw ('Checked helper failed: ' + $diagnostic.Substring(0, [Math]::Min(4096, $diagnostic.Length))) }
        return $text
    } finally {
        try {
            if (-not $child.HasExited) { $child.Kill($true); if (-not $child.WaitForExit(10000)) { throw 'Helper teardown exceeded its cleanup reserve.' } }
        } finally { $child.Dispose() }
    }
}

function Tool-Json([string]$Command, [object]$Request, [int]$MaximumSeconds = 5, [int]$ReserveSeconds = 100) {
    if ((File-Digest $ToolsPath) -cne $toolDigest) { throw 'Checked supervisor changed.' }
    $response = Invoke-Sanitized $ToolsPath @($Command, '--json', ($Request | ConvertTo-Json -Depth 20 -Compress)) $MaximumSeconds $ReserveSeconds
    return $response | ConvertFrom-Json -AsHashtable
}

function Proof([string]$Root, [string]$Relative) {
    return @{ path = $Relative; sha256 = (File-Digest (Join-Path $Root $Relative)) }
}

function Publish-UploadWindow {
    $allowed = (Remaining-Seconds) -ge 70
    "BONSAI_CPU_BUNDLE_UPLOAD_ALLOWED=$($allowed.ToString().ToLowerInvariant())" >> $env:GITHUB_ENV
    return $allowed
}

$owner = @{
    schema_version = 1; kind = 'prism-cpu-bundle-owner'; runner_commit = $env:GITHUB_SHA
    run_id = $env:GITHUB_RUN_ID; run_attempt = $env:GITHUB_RUN_ATTEMPT
    preparation_sha256 = $ExpectedPacketSha256; workload_predeclared = $true; completed = $false
    budget_seconds = 600; clock_frequency = $frequency; started_tick = $original.started_tick
    deadline_tick = $deadline; original_deadline_unix_milliseconds = $original.deadline_unix_milliseconds
    power_before = $original.power_before; arms = @(); failure = $null
    retention_scope = 'Selected local evidence stage; owning job must separately pass its post-upload original-deadline check.'
    full_promotion_qualified = $false
}
$captureFailure = $null
$failedArm = $null
try {
    if ((Remaining-Seconds) -lt 540) { throw 'Bootstrap/intake left insufficient room for the complete fixed four-arm workload.' }
    $before = Tool-Json 'host-cpu' @{}
    if ($before.architecture -cne 'aarch64' -or $before.host_identity.status -cne 'observed' -or
        $before.host_identity.processor_name -notmatch 'Snapdragon.*X2.*Elite.*Extreme') { throw 'Selected X2 native CPU identity was not observed.' }
    $owner.host_identity = $before.host_identity
    $owner.host_cpu_before = $before
    $initialConditions = Tool-Json 'cpu-evaluation' @{ operation = 'compare-conditions'; before = $before; after = $before }
    if ($initialConditions.Comparable -ne $true) { throw 'Initial native CPU, topology or power-policy conditions are unavailable.' }
    if ($original.power_before.status -cne 'observed' -or $original.power_before.source -cne 'GetSystemPowerStatus' -or
        $original.power_before.ac_online -isnot [bool] -or $original.power_before.battery_saver -isnot [bool] -or
        $before.power_snapshot.status -cne 'observed' -or
        $before.power_snapshot.ac_online -cne $original.power_before.ac_online -or
        $before.power_snapshot.battery_saver -cne $original.power_before.battery_saver) {
        throw 'Initial setup power is unknown or changed; no matched performance trial may start.'
    }
    foreach ($role in @('baseline', 'candidate')) {
        [void](Tool-Json 'artifact' @{ operation = 'arm64'; path = (Packet-Member $PacketDirectory $packet.arms[$role].executable.path) } 2)
    }
    $data = Join-Path $env:LOCALAPPDATA 'SwiftBonsai2'
    $plan = Tool-Json 'cpu-check-plan' @{ operation = 'plan'; dataDirectory = $data; profile = 'swift-pq2-assessment'; useAbliterateLora = $true; useRepackedPtq1 = $false }
    if ($plan.Policy.ServiceCeilingBytes -ne 12348030976L -or $plan.Policy.RequiredAvailableBytes -ne 16642998272L -or
        $plan.Policy.AvailableMemoryFloorBytes -ne 4294967296L) { throw 'Compiled model admission policy changed.' }
    $model = External-Path $plan.ModelPath; $adapter = External-Path $plan.AdapterPath
    if ($packet.model_sha256 -cne $plan.Artifact.Sha256 -or $packet.model_bytes -ne $plan.Artifact.Bytes -or
        $packet.adapter_sha256 -cne $plan.AdapterLock.adapter.sha256 -or $packet.adapter_bytes -ne $plan.AdapterLock.adapter.bytes) {
        throw 'Prepared and compiled model/adapter identities differ.'
    }
    $owner.model_sha256 = $plan.Artifact.Sha256; $owner.adapter_sha256 = $plan.AdapterLock.adapter.sha256
    Write-Json (Join-Path $retained 'cpu-plan.json') $plan
    Write-Json (Join-Path $retained 'owner-start.json') $original
    foreach ($index in 0..3) {
        $failedArm = $index + 1
        $remainingArms = 4 - $index
        if ((Remaining-Seconds) -lt ($remainingArms * 110 + 100)) { throw 'The remaining complete comparison cannot fit its original allowance.' }
        if ((File-Digest $preparationPath) -cne $ExpectedPacketSha256) { throw 'Preparation changed between arms.' }
        $role = $packet.schedule[$index]
        $relative = 'arms/arm-' + ($index + 1)
        $lane = Join-Path $working $relative
        [void][IO.Directory]::CreateDirectory($lane)
        $collector = Join-Path $lane 'collector'
        [void][IO.Directory]::CreateDirectory($collector)
        $supervisorLane = Join-Path $lane 'supervision'
        [void][IO.Directory]::CreateDirectory($supervisorLane)
        $begin = [Diagnostics.Stopwatch]::GetTimestamp()
        $executionDeadline = $begin + 60L * $frequency
        $collectorDeadline = [long]$original.deadline_unix_milliseconds - [long][Math]::Floor(($deadline - $executionDeadline) * 1000.0 / $frequency)
        $executable = Packet-Member $PacketDirectory $packet.arms[$role].executable.path
        $request = @{
            schemaVersion = 1; executable = $executable
            arguments = @($model, $adapter, $collector, $messages, '32', [string]$collectorDeadline, $packet.arms[$role].source.revision)
            workingDirectory = $lane; evidenceDirectory = $supervisorLane
            artifacts = @(
                @{ path = $model; bytes = $plan.Artifact.Bytes; sha256 = $plan.Artifact.Sha256 },
                @{ path = $adapter; bytes = $plan.AdapterLock.adapter.bytes; sha256 = $plan.AdapterLock.adapter.sha256 },
                @{ path = $executable; bytes = $packet.arms[$role].executable.bytes; sha256 = $packet.arms[$role].executable.sha256 },
                @{ path = $messages; bytes = (Get-Item -LiteralPath $messages).Length; sha256 = $packet.messages.sha256 }
            )
            memoryLimitBytes = 12348030976L; requiredAvailableBytes = 16642998272L; physicalReserveBytes = 4294967296L
            timeoutSeconds = 60; totalTimeoutSeconds = 90; stdoutLimitBytes = 1048576; stderrLimitBytes = 1048576
            requireNonemptyStdout = $false; ciOnly = $false; observeCpuConditions = $true
            parentProcessId = $PID; parentCreationTimeFileTime = $parentCreation
        }
        # ciOnly's publisher timing parser is inapplicable. The CI guard above,
        # digest-bound CPU collector and original owner deadline remain required.
        Write-Json (Join-Path $retained ('completion-request-' + ($index + 1) + '.json')) $request
        $supervision = Tool-Json 'completion' $request 95
        if ($supervision.Status -cne 'completed' -or $supervision.ExitCode -ne 0 -or
            $supervision.ModelProcessStarted -ne $true -or $supervision.ContainsKey('CancellationFailure')) {
            throw 'Owned Job did not complete and tear down successfully; no later arm may start.'
        }
        $captured = Read-Json (Join-Path $collector 'cpu-bundle.json')
        if ($captured.capture_status -cne 'complete' -or $captured.arm_succeeded -ne $true -or
            $captured.session_succeeded -ne $true -or $captured.terminal_delivered -ne $true -or
            $captured.terminal_accounting_matches -ne $true -or $captured.kv_cache -cne 'f16' -or
            $captured.source_commit_declaration -cne $packet.arms[$role].source.revision -or
            $captured.executable_sha256 -cne $packet.arms[$role].executable.sha256 -or
            $captured.original_deadline_unix_milliseconds -ne $collectorDeadline -or
            [string]::IsNullOrWhiteSpace($captured.output_text)) { throw 'The completed native collector receipt is incomplete or foreign.' }
        $conditions = Tool-Json 'cpu-evaluation' @{ operation = 'compare-conditions'; before = $supervision.CpuConditions.Before; after = $supervision.CpuConditions.After }
        $boundary = Tool-Json 'cpu-evaluation' @{ operation = 'compare-conditions'; before = $before; after = $supervision.CpuConditions.Before }
        if ($conditions.Comparable -ne $true -or $boundary.Comparable -ne $true) { throw 'CPU/power conditions changed or became unknown; stop the fixed comparison.' }
        $stdout = [IO.File]::ReadAllText((Join-Path $supervisorLane 'stdout.txt'))
        $stderr = [IO.File]::ReadAllText((Join-Path $supervisorLane 'stderr.txt'))
        $owner.arms += @{
            id = 'arm-' + ($index + 1); role = $role; started_tick = $begin
            execution_deadline_tick = $executionDeadline; collector_deadline_unix_milliseconds = $collectorDeadline
            cleanup_completed_tick = [Diagnostics.Stopwatch]::GetTimestamp()
            host_identity = $supervision.CpuConditions.Before.host_identity
            power_before = $supervision.CpuConditions.Before.power_snapshot
            power_after = $supervision.CpuConditions.After.power_snapshot
            owned_job_empty = $true; cleanup_succeeded = $true
            stdout_bytes = (Get-Item -LiteralPath (Join-Path $supervisorLane 'stdout.txt')).Length
            stderr_bytes = (Get-Item -LiteralPath (Join-Path $supervisorLane 'stderr.txt')).Length
            stdout_nonempty = -not [string]::IsNullOrWhiteSpace($stdout)
            stderr_nonempty = -not [string]::IsNullOrWhiteSpace($stderr)
            receipt = (Proof $working ($relative + '/collector/cpu-bundle.json'))
            started_receipt = (Proof $working ($relative + '/collector/cpu-bundle-started.json'))
            supervision = (Proof $working ($relative + '/supervision/supervision.json'))
        }
    }
    $failedArm = $null
    $owner.completed = $true
} catch {
    $captureFailure = $_.Exception.Message
    $owner.failure = $captureFailure
    $owner.failed_arm = $failedArm
} finally {
    try {
        $after = Tool-Json 'host-cpu' @{} 5 85
        $owner.host_cpu_after = $after; $owner.power_after = $after.power_snapshot
    } catch {
        $owner.power_after = @{ status = 'unknown'; source = 'GetSystemPowerStatus'; ac_online = $null; battery_saver = $null; battery_percent = $null }
        $owner.completed = $false
        if (-not $captureFailure) { $captureFailure = $_.Exception.Message; $owner.failure = $captureFailure }
    }
    # Keep the explicit replay selection; never upload whole work/build trees.
    foreach ($index in 1..4) {
        $relative = 'arms/arm-' + $index
        $lane = Join-Path $working $relative
        if (-not (Test-Path -LiteralPath $lane)) { continue }
        $selected = Join-Path $retained $relative
        [void][IO.Directory]::CreateDirectory($selected)
        foreach ($name in @('collector/cpu-bundle-started.json', 'collector/cpu-bundle.json', 'supervision/supervision.json')) {
            $path = Join-Path $lane $name
            if (Test-Path -LiteralPath $path) {
                $destination = Join-Path $selected $name
                [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($destination))
                Copy-Item -LiteralPath $path -Destination $destination
            }
        }
        if ($captureFailure -and $index -eq $failedArm) {
            foreach ($name in @('stdout.txt', 'stderr.txt')) {
                $path = Join-Path (Join-Path $lane 'supervision') $name
                if ((Test-Path -LiteralPath $path) -and -not [string]::IsNullOrWhiteSpace([IO.File]::ReadAllText($path))) {
                    $diagnosticDestination = Join-Path $selected ('supervision/' + $name)
                    [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($diagnosticDestination))
                    Copy-Item -LiteralPath $path -Destination $diagnosticDestination
                }
            }
        }
    }
    $packetStage = Join-Path $retained 'packet'
    [void][IO.Directory]::CreateDirectory($packetStage)
    foreach ($name in @('preparation.json', 'messages.json', 'evaluator.py')) {
        Copy-Item -LiteralPath (Join-Path $PacketDirectory $name) -Destination (Join-Path $packetStage $name)
    }
    $owner.retention_completed_tick = [Diagnostics.Stopwatch]::GetTimestamp()
    Write-Json (Join-Path $retained 'owner.json') $owner
    [void](Publish-UploadWindow)
}
if ($captureFailure) { throw $captureFailure }
if ((Remaining-Seconds) -lt 85) { throw 'Insufficient original budget for evaluation and bounded final upload.' }
$evaluator = Join-Path $PacketDirectory 'evaluator.py'
$python = (Get-Command python -CommandType Application).Source
try {
    if ((Remaining-Seconds) -lt 85) { throw 'Insufficient original budget for evaluation and bounded final upload.' }
    $comparison = Invoke-Sanitized $python @('-B', $evaluator, 'evaluate', '--preparation', (Join-Path $retained 'packet/preparation.json'), '--owner', (Join-Path $retained 'owner.json')) 15 70
    [IO.File]::WriteAllText((Join-Path $retained 'comparison.json'), $comparison + "`n", $utf8)
    $comparison | Add-Content -LiteralPath $env:GITHUB_STEP_SUMMARY
    if ((Remaining-Seconds) -lt 70) { throw 'Final evidence upload and power-recording reserve is exhausted.' }
} catch {
    Write-Json (Join-Path $retained 'comparison-rejection.json') @{
        status = 'rejected'; reason = $_.Exception.Message
        completed_tick = [Diagnostics.Stopwatch]::GetTimestamp(); full_promotion_qualified = $false
    }
    throw
} finally {
    [void](Publish-UploadWindow)
}
