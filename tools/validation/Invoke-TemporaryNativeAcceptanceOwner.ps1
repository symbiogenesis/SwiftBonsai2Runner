#requires -Version 7.6
<#
.SYNOPSIS
Owns one explicitly approved, prepared PQ2 scale/history acceptance attempt within 240 seconds.
.DESCRIPTION
Do not invoke until the exact helper, settled main revision, 0.1.10 release and run
are reviewed, publication and the one owner attempt are authorized, and the owning
Prism and Runner main gates pass. This file does not grant that authorization.
Uses the existing official runner without building tools or loading models itself.
The archive and extracted package must already be retained and independently reviewed.
Setup, registration, the single CI job and cleanup share one monotonic allowance:
210 seconds of work and 30 seconds reserved for cleanup, leaving 60 seconds of the
project's shared five-minute limit for work already charged outside this process.
The independent PowerShell watchdog stops only recorded process identities and removes
only this wrapper's registration. Registration tokens are environment-only and no
native stdout/stderr or token-bearing API responses are retained in these reports.
#>
[CmdletBinding(DefaultParameterSetName = 'Owner')]
[OutputType([pscustomobject])]
param(
    [Parameter(Mandatory)][ValidatePattern('^[1-9][0-9]{0,19}$')][string]$RunId,
    [Parameter(Mandatory)][ValidatePattern('^[a-f0-9]{40}$')][string]$Revision,
    [ValidateRange(1, 1000)][int]$RunAttempt = 1,
    [Parameter(Mandatory)][ValidateSet('pq2-adapter-lifecycle-row4')][string]$Profile,
    [Parameter(Mandatory)][string]$RunnerDirectory,
    [string]$EvidenceDirectory,
    [Parameter(Mandatory, ParameterSetName = 'Owner')][ValidatePattern('^[1-9][0-9]{0,19}$')][string]$RunnerFoundationRunId,
    [Parameter(Mandatory, ParameterSetName = 'Owner')][ValidatePattern('^[a-f0-9]{64}$')][string]$ExpectedRunnerListenerSha256,
    [Parameter(Mandatory, ParameterSetName = 'Owner')][ValidatePattern('^[1-9][0-9]{0,19}$')][string]$PreparationArtifactId,
    [Parameter(Mandatory, ParameterSetName = 'Owner')][ValidatePattern('^[a-f0-9]{64}$')][string]$PreparationArtifactSha256,
    [Parameter(Mandatory, ParameterSetName = 'Owner')][ValidateRange(1, 67108864)][long]$PreparationArtifactBytes,
    [Parameter(Mandatory, ParameterSetName = 'Owner')][string]$PreparationArchivePath,
    [Parameter(Mandatory, ParameterSetName = 'Owner')][ValidatePattern('^[a-f0-9]{64}$')][string]$PreparationReceiptSha256,
    [Parameter(Mandatory, ParameterSetName = 'Owner')][ValidateRange(1, 65536)][int]$PreparationReceiptBytes,
    [Parameter(Mandatory, ParameterSetName = 'Owner')][ValidatePattern('^[a-f0-9]{64}$')][string]$ExpectedToolsSourceId,
    [Parameter(Mandatory, ParameterSetName = 'Owner')][string]$PreparedPackageDirectory,
    [Parameter(Mandatory, ParameterSetName = 'Watchdog')][switch]$Watchdog
)
$startedTick = [Diagnostics.Stopwatch]::GetTimestamp()
$startedAt = [DateTime]::UtcNow
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$workDeadlineSeconds = 210
$totalDeadlineSeconds = 240
$repository = 'symbiogenesis/SwiftBonsai2Runner'
$repositoryUrl = 'https://github.com/' + $repository
if (-not [IO.Path]::IsPathFullyQualified($RunnerDirectory)) { throw 'The existing official runner path must be absolute.' }
$runnerDirectory = [IO.Path]::TrimEndingDirectorySeparator([IO.Path]::GetFullPath($RunnerDirectory))
$runnerExecutable = Join-Path $runnerDirectory 'bin/Runner.Listener.exe'
$credentialNames = @('.runner', '.credentials', '.credentials_rsaparams')
$label = 'native-acceptance-' + $RunId
$gh = @(Get-Command gh -CommandType Application -ErrorAction Stop)[0].Source
$powershell = [Environment]::ProcessPath
$control = $null

function Assert-OwnerPath {
    <# .SYNOPSIS Rejects nonabsolute, Git-contained and reparse-backed control or package paths. #>
    [CmdletBinding()]
    [OutputType([void])]
    param([Parameter(Mandatory)][string]$Path)
    if (-not [IO.Path]::IsPathFullyQualified($Path) -or $Path.StartsWith('\\') -or
        $Path.Contains('%') -or $Path.Contains([char]0)) { throw 'Owner paths must be explicit local absolute paths.' }
    $current = [IO.Path]::GetFullPath($Path)
    while ($current) {
        try {
            $attributes = [IO.File]::GetAttributes($current)
            if (($attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw 'Owner paths cannot traverse reparse points.' }
        } catch [IO.FileNotFoundException] { }
        catch [IO.DirectoryNotFoundException] { }
        if ([IO.File]::Exists((Join-Path $current '.git')) -or [IO.Directory]::Exists((Join-Path $current '.git'))) {
            throw 'Owner evidence, packages and runner storage must remain outside Git.'
        }
        $parent = [IO.Path]::GetDirectoryName($current)
        if ($parent -ceq $current) { break }
        $current = $parent
    }
}

function Get-OwnerElapsed {
    <# .SYNOPSIS Returns elapsed monotonic owner seconds, including registration. #>
    [CmdletBinding()]
    [OutputType([double])]
    param()
    return ([Diagnostics.Stopwatch]::GetTimestamp() - $script:startedTick) / [Diagnostics.Stopwatch]::Frequency
}

function Write-OwnerJson {
    <# .SYNOPSIS Atomically writes small public control metadata or fixed receipts. #>
    [CmdletBinding()]
    [OutputType([void])]
    param([string]$Name, [object]$Value)
    $bytes = [Text.UTF8Encoding]::new($false).GetBytes(($Value | ConvertTo-Json -Depth 12) + "`n")
    if ($bytes.Length -gt 65536) { throw 'Owner metadata exceeds its byte bound.' }
    $path = Join-Path $EvidenceDirectory $Name
    $temporary = $path + '.' + [guid]::NewGuid().ToString('N') + '.tmp'
    [IO.File]::WriteAllBytes($temporary, $bytes)
    [IO.File]::Move($temporary, $path, $true)
}

function Read-OwnerJson {
    <# .SYNOPSIS Reads one bounded owner metadata file. #>
    [CmdletBinding()]
    [OutputType([object])]
    param([string]$Name)
    $path = Join-Path $EvidenceDirectory $Name
    $stream = [IO.FileStream]::new($path, [IO.FileMode]::Open, [IO.FileAccess]::Read, ([IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete))
    try {
        if ($stream.Length -gt 65536) { throw 'Owner metadata exceeds its pre-read bound.' }
        $bytes = [byte[]]::new([int]$stream.Length)
        $stream.ReadExactly($bytes, 0, $bytes.Length)
        return [Text.UTF8Encoding]::new($false, $true).GetString($bytes) | ConvertFrom-Json -AsHashtable -DateKind String
    } finally { $stream.Dispose() }
}

function Invoke-OwnerApi {
    <# .SYNOPSIS Calls only the fixed repository API, suppressing all raw command output. #>
    [CmdletBinding()]
    [OutputType([object])]
    param([string]$Path, [ValidateSet('GET', 'POST', 'DELETE')][string]$Method = 'GET', [switch]$Cleanup)
    $maximum = if ($Cleanup) { $totalDeadlineSeconds } else { $workDeadlineSeconds }
    $left = [Math]::Min(3, $maximum - (Get-OwnerElapsed) - 2)
    if ($left -le 0) { throw 'The owner API deadline expired.' }
    $start = [Diagnostics.ProcessStartInfo]::new($gh)
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    foreach ($argument in @('api', '--method', $Method, ('repos/' + $repository + '/' + $Path))) { $start.ArgumentList.Add($argument) }
    foreach ($name in @('GH_DEBUG', 'DEBUG')) { [void]$start.Environment.Remove($name) }
    $process = [Diagnostics.Process]::Start($start)
    try {
        if ($null -ne $control -and -not $Watchdog) {
            $control.Processes += @{ Id=$process.Id; StartTicks=$process.StartTime.ToUniversalTime().Ticks }
            Write-OwnerJson 'owner-control.json' $control
        }
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit([int][Math]::Floor($left * 1000))) { throw 'A bounded owner API call timed out.' }
        if (-not $stdout.Wait(1000) -or -not $stderr.Wait(1000) -or $process.ExitCode -ne 0) { throw 'A bounded owner API call failed.' }
        $text = $stdout.GetAwaiter().GetResult()
        if ($text.Length -gt 1048576) { throw 'An owner API response exceeded its byte bound.' }
        if ([string]::IsNullOrWhiteSpace($text)) { return $null }
        return $text | ConvertFrom-Json -AsHashtable -DateKind String
    } finally {
        if (-not $process.HasExited) {
            $process.Kill($true)
            [void]$process.WaitForExit(1000)
        }
        $process.Dispose()
    }
}

function Get-IdleOwnerProcess {
    <# .SYNOPSIS Lists blockers without stopping any preexisting user processes. #>
    [CmdletBinding()]
    [OutputType([object[]])]
    param()
    return @(Get-Process | Where-Object ProcessName -Match '^(bonsai-|prism-inference|llama-|swift[_-]model|canary|qwen.*tts|whisper|Runner\.(Listener|Worker)$)' |
        ForEach-Object ProcessName | Sort-Object -Unique)
}

function Confirm-PreparedArtifact {
    <# .SYNOPSIS Verifies an explicitly reviewed immutable producer artifact without executing it. #>
    [CmdletBinding()]
    [OutputType([Collections.Specialized.OrderedDictionary])]
    param([object]$Run, [object]$Prepare)
    if (-not $PreparationArtifactId -or -not $PreparationArtifactSha256 -or
        -not $PreparationReceiptSha256 -or -not $PreparedPackageDirectory -or
        -not $PreparationArchivePath -or -not $ExpectedToolsSourceId) {
        throw 'All explicitly reviewed preparation artifact inputs are required.'
    }
    $artifact = Invoke-OwnerApi -Path ('actions/artifacts/' + $PreparationArtifactId)
    if ([string]$artifact.id -cne $PreparationArtifactId -or $artifact.expired -ne $false -or
        $artifact.name -cne "native-acceptance-bin-$RunId-$RunAttempt" -or
        $artifact.digest -cne ('sha256:' + $PreparationArtifactSha256) -or
        $artifact.size_in_bytes -ne $PreparationArtifactBytes -or
        [string]$artifact.workflow_run.id -cne $RunId -or $artifact.workflow_run.head_sha -cne $Revision -or
        $artifact.workflow_run.head_branch -cne 'main' -or
        $artifact.workflow_run.repository_id -ne $Run.repository.id -or
        $artifact.workflow_run.head_repository_id -ne $Run.repository.id) {
        throw 'The approved immutable artifact API identity differs.'
    }
    Assert-OwnerPath $PreparedPackageDirectory
    Assert-OwnerPath $PreparationArchivePath
    if ((Get-Item -LiteralPath $PreparationArchivePath).Length -ne $PreparationArtifactBytes -or
        (Get-FileHash -LiteralPath $PreparationArchivePath -Algorithm SHA256).Hash.ToLowerInvariant() -cne $PreparationArtifactSha256) {
        throw 'The retained approved artifact archive differs.'
    }
    $receiptPath = Join-Path $PreparedPackageDirectory 'receipt.json'
    Assert-OwnerPath $receiptPath
    if ((Get-Item -LiteralPath $receiptPath).Length -ne $PreparationReceiptBytes -or
        (Get-FileHash -LiteralPath $receiptPath -Algorithm SHA256).Hash.ToLowerInvariant() -cne $PreparationReceiptSha256) {
        throw 'The explicitly reviewed preparation receipt length or digest differs.'
    }
    $receipt = [IO.File]::ReadAllText($receiptPath) | ConvertFrom-Json
    $compilerIdentity = "rustc 1.101.0-nightly (1a2c4797e 2026-10-21)`nbinary: rustc`ncommit-hash: 1a2c4797ef2570bd76ca5dee34449169bc7dc457`ncommit-date: 2026-10-21`nhost: aarch64-pc-windows-msvc`nrelease: 1.101.0-nightly`nLLVM version: 23.1.4"
    if ($receipt.SchemaVersion -ne 1 -or $receipt.Revision -cne $Revision -or
        $receipt.RunId -cne $RunId -or [string]$receipt.RunAttempt -cne [string]$RunAttempt -or
        $receipt.Toolchain -cne 'nightly-2026-10-22-aarch64-pc-windows-msvc' -or
        $receipt.CompilerIdentity -cne $compilerIdentity -or $receipt.ModelExecuted -ne $false -or
        $receipt.ToolsSourceId -cne $ExpectedToolsSourceId -or
        @($receipt.Files).Count -ne 2) { throw 'Prepared source, run, compiler or model-free scope differs.' }
    foreach ($name in @('native-model-acceptance.exe', 'bonsai-tools.exe')) {
        $matching = @($receipt.Files | Where-Object Name -CEQ $name)
        if ($matching.Count -ne 1 -or $matching[0].Bytes -lt 1 -or $matching[0].Bytes -gt 67108864 -or
            $matching[0].Sha256 -cnotmatch '^[a-f0-9]{64}$') { throw 'Prepared binary inventory differs.' }
        $path = Join-Path $PreparedPackageDirectory $name
        Assert-OwnerPath $path
        if ((Get-Item -LiteralPath $path).Length -ne $matching[0].Bytes -or
            (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() -cne $matching[0].Sha256) {
            throw 'Prepared binary file identity differs.'
        }
        $reader = [IO.BinaryReader]::new([IO.File]::OpenRead($path))
        try {
            if ($reader.BaseStream.Length -lt 64 -or $reader.ReadUInt16() -ne 0x5a4d) { throw 'Prepared DOS signature differs.' }
            $reader.BaseStream.Position = 60
            $offset = $reader.ReadUInt32()
            if ($offset -lt 64 -or $offset -gt $reader.BaseStream.Length - 24) { throw 'Prepared PE offset differs.' }
            $reader.BaseStream.Position = $offset
            if ($reader.ReadUInt32() -ne 0x4550 -or $reader.ReadUInt16() -ne 0xaa64) { throw 'Prepared native ARM64 PE differs.' }
        } finally { $reader.Dispose() }
    }
    $archive = [IO.Compression.ZipFile]::OpenRead($PreparationArchivePath)
    try {
        if ($archive.Entries.Count -ne 3) { throw 'The retained package must contain exactly its receipt and two binaries.' }
        foreach ($name in @('receipt.json', 'native-model-acceptance.exe', 'bonsai-tools.exe')) {
            $entries = @($archive.Entries | Where-Object FullName -CEQ $name)
            if ($entries.Count -ne 1) { throw 'The retained package inventory differs.' }
            $expectedBytes = $PreparationReceiptBytes
            $expectedSha256 = $PreparationReceiptSha256
            if ($name -cne 'receipt.json') {
                $file = @($receipt.Files | Where-Object Name -CEQ $name)[0]
                $expectedBytes = $file.Bytes
                $expectedSha256 = $file.Sha256
            }
            if ($entries[0].Length -ne $expectedBytes) { throw 'A retained package member length differs.' }
            $stream = $entries[0].Open()
            try {
                $digest = [Security.Cryptography.SHA256]::HashData($stream)
                if ([Convert]::ToHexStringLower($digest) -cne $expectedSha256) { throw 'A retained package member digest differs.' }
            } finally { $stream.Dispose() }
        }
    } finally { $archive.Dispose() }
    return [ordered]@{
        Basis='passed-hosted-job-and-explicit-immutable-artifact'; Scope='one-bounded-diagnostic-only'
        ArtifactId=$PreparationArtifactId; ArtifactName=$artifact.name; ArtifactSha256=$PreparationArtifactSha256
        ArtifactCreatedAt=$artifact.created_at; ArtifactBytes=$PreparationArtifactBytes
        ReceiptSha256=$PreparationReceiptSha256; ReceiptBytes=$PreparationReceiptBytes
        Revision=$Revision; RunId=$RunId; RunAttempt=$RunAttempt; Files=$receipt.Files
        CompilerIdentity=$receipt.CompilerIdentity; ToolsSourceId=$receipt.ToolsSourceId; ModelExecuted=$false
        HostedJobApiStatus=$Prepare.status; HostedJobApiConclusion=$Prepare.conclusion
        HostedJobConclusionKnown=($Prepare.conclusion -ceq 'success'); RunnerFoundationRunId=$RunnerFoundationRunId
        OverallRuntimePromotionClaimed=$false
    }
}

function Test-OwnerIdentity {
    <# .SYNOPSIS Checks a PID and creation time before touching that owned process. #>
    [CmdletBinding()]
    [OutputType([bool])]
    param([object]$Identity)
    try {
        $process = [Diagnostics.Process]::GetProcessById([int]$Identity.Id)
        try { return $process.StartTime.ToUniversalTime().Ticks -eq [long]$Identity.StartTicks }
        finally { $process.Dispose() }
    } catch [ArgumentException] { return $false }
}

function Invoke-OwnedRunnerCleanup {
    <# .SYNOPSIS Stops only this wrapper's gated processes and their exact recorded children. #>
    [CmdletBinding()]
    [OutputType([object[]])]
    param()
    $identities = @($control.Processes)
    foreach ($identity in @($control.Processes)) {
        $childPath = Join-Path $EvidenceDirectory ('child-' + $identity.Id + '.json')
        if ([IO.File]::Exists($childPath)) {
            try {
                $child = Read-OwnerJson ('child-' + $identity.Id + '.json')
                if ($child.ParentId -eq $identity.Id -and $child.ParentStartTicks -eq $identity.StartTicks) { $identities += $child }
            } catch { continue }
        }
    }
    $failed = @()
    foreach ($identity in $identities) {
        try {
            if (Test-OwnerIdentity $identity) {
                $process = [Diagnostics.Process]::GetProcessById([int]$identity.Id)
                try {
                    if ($process.StartTime.ToUniversalTime().Ticks -eq [long]$identity.StartTicks) {
                        $process.Kill($true)
                        [void]$process.WaitForExit(1000)
                    }
                } finally { $process.Dispose() }
            }
        } catch { $failed += [int]$identity.Id }
    }
    return @($failed) + @($identities | Where-Object { Test-OwnerIdentity $_ } | ForEach-Object { [int]$_.Id })
}

function Complete-OwnerCleanup {
    <# .SYNOPSIS Removes only owned processes, this unique registration and three known credentials. #>
    [CmdletBinding()]
    [OutputType([Collections.Specialized.OrderedDictionary])]
    param()
    try { $remaining = @(Invoke-OwnedRunnerCleanup) } catch { $remaining = @(-1) }
    $credentialOwnerMatches = $true
    $settingsPath = Join-Path $runnerDirectory '.runner'
    if ([IO.File]::Exists($settingsPath)) {
        try {
            if ((Get-Item -Force -LiteralPath $settingsPath).Length -gt 65536) { throw 'Runner settings exceed their bound.' }
            $settings = [IO.File]::ReadAllText($settingsPath) | ConvertFrom-Json
            $credentialOwnerMatches = $settings.agentName -ceq $control.RunnerName
        } catch { $credentialOwnerMatches = $false }
    }
    if ($credentialOwnerMatches) {
        foreach ($name in $credentialNames) {
            $path = [IO.Path]::GetFullPath((Join-Path $runnerDirectory $name))
            if ([IO.Path]::GetDirectoryName($path) -cne $runnerDirectory) { throw 'Runner cleanup escaped the exact directory.' }
            try { [IO.File]::Delete($path) } catch { $credentialOwnerMatches = $false }
        }
    }
    $remoteClean = $false
    $remoteCount = $null
    try {
        $runners = Invoke-OwnerApi -Path 'actions/runners?per_page=100' -Cleanup
        if ($runners.total_count -gt 100) { throw 'Runner inventory exceeds the cleanup bound.' }
        foreach ($runner in @($runners.runners | Where-Object name -CEQ $control.RunnerName)) {
            if ($runner.id -isnot [long] -and $runner.id -isnot [int]) { throw 'Invalid owned runner identity.' }
            $null = Invoke-OwnerApi -Path ('actions/runners/' + $runner.id) -Method DELETE -Cleanup
        }
        $after = Invoke-OwnerApi -Path 'actions/runners?per_page=100' -Cleanup
        $remoteCount = $after.total_count
        $remoteClean = @($after.runners | Where-Object name -CEQ $control.RunnerName).Count -eq 0
    } catch { $remoteClean = $false }
    $files = @($credentialNames | ForEach-Object { @{ Name = $_; Exists = Test-Path -LiteralPath (Join-Path $runnerDirectory $_) } })
    return [ordered]@{
        RunId = $RunId; Revision = $Revision; RecordedAtUtc = [DateTime]::UtcNow.ToString('o')
        ElapsedSeconds = [Math]::Round((Get-OwnerElapsed), 3); OwnedRegistrationAbsent = $remoteClean
        RegisteredRunnerCount = $remoteCount; CredentialOwnerMatched = $credentialOwnerMatches
        RunnerCredentialFiles = $files; RemainingOwnedProcessIds = $remaining
        CleanupComplete = $remoteClean -and $credentialOwnerMatches -and $remaining.Count -eq 0 -and @($files | Where-Object Exists).Count -eq 0
    }
}

function Invoke-OwnerGate {
    <# .SYNOPSIS Starts a token-safe waiting trampoline before allowing its one runner child. #>
    [CmdletBinding()]
    [OutputType([Diagnostics.Process])]
    param([string[]]$Arguments, [string]$RegistrationValue)
    $gate = @'
$ErrorActionPreference = 'Stop'
[Console]::InputEncoding = [Text.UTF8Encoding]::new($false)
$line = [Console]::In.ReadLine()
if ($null -eq $line) { exit 125 }
$request = $line | ConvertFrom-Json -AsHashtable -DateKind String
$start = [Diagnostics.ProcessStartInfo]::new($request.Executable)
$start.WorkingDirectory = $request.WorkingDirectory
$start.UseShellExecute = $false
$start.CreateNoWindow = $true
$start.RedirectStandardInput = $true
foreach ($argument in $request.Arguments) { $start.ArgumentList.Add($argument) }
$child = [Diagnostics.Process]::Start($start)
try {
    $child.StandardInput.Close()
    $record = @{ Id=$child.Id; StartTicks=$child.StartTime.ToUniversalTime().Ticks; ParentId=$PID; ParentStartTicks=[Diagnostics.Process]::GetCurrentProcess().StartTime.ToUniversalTime().Ticks }
    [IO.File]::WriteAllText($request.ChildRecord, ($record | ConvertTo-Json -Compress), [Text.UTF8Encoding]::new($false))
    $child.WaitForExit()
    exit $child.ExitCode
} finally {
    if (-not $child.HasExited) { $child.Kill($true); [void]$child.WaitForExit(1000) }
    $child.Dispose()
}
'@
    $start = [Diagnostics.ProcessStartInfo]::new($powershell)
    $start.WorkingDirectory = $runnerDirectory
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardInput = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $start.StandardInputEncoding = [Text.UTF8Encoding]::new($false)
    foreach ($argument in @('-NoLogo', '-NoProfile', '-NonInteractive', '-EncodedCommand', [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($gate)))) { $start.ArgumentList.Add($argument) }
    foreach ($name in @('ACTIONS_RUNTIME_TOKEN', 'ACTIONS_ID_TOKEN_REQUEST_TOKEN', 'GITHUB_TOKEN', 'GH_TOKEN', 'ACTIONS_RUNNER_INPUT_TOKEN')) { [void]$start.Environment.Remove($name) }
    # The reviewed workflow must clamp its Rust/process allowance to this
    # monotonic outer deadline before launching any model work.
    $start.Environment['BONSAI_NATIVE_OWNER_RUN_ID'] = $RunId
    $start.Environment['BONSAI_NATIVE_OWNER_REVISION'] = $Revision
    $start.Environment['BONSAI_NATIVE_OWNER_STARTED_TICK'] = [string]$startedTick
    $start.Environment['BONSAI_NATIVE_OWNER_STARTED_UTC'] = $startedAt.ToString('o')
    $start.Environment['BONSAI_NATIVE_OWNER_WORK_SECONDS'] = [string]$workDeadlineSeconds
    if ($RegistrationValue) { $start.Environment['ACTIONS_RUNNER_INPUT_TOKEN'] = $RegistrationValue }
    $process = [Diagnostics.Process]::Start($start)
    $identity = @{ Id = $process.Id; StartTicks = $process.StartTime.ToUniversalTime().Ticks }
    $control.Processes += $identity
    Write-OwnerJson 'owner-control.json' $control
    $request = @{ Executable = $runnerExecutable; WorkingDirectory = $runnerDirectory; Arguments = $Arguments
        ChildRecord = Join-Path $EvidenceDirectory ('child-' + $process.Id + '.json') }
    $process.StandardInput.WriteLine(($request | ConvertTo-Json -Depth 4 -Compress))
    $process.StandardInput.Close()
    return $process
}

if (-not $IsWindows -or [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture -ne 'Arm64' -or
    [Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne 'Arm64') { throw 'Native Windows ARM64 is required.' }
if ($Watchdog) {
    $control = Read-OwnerJson 'owner-control.json'
    $parentId = (Get-CimInstance Win32_Process -Filter ('ProcessId=' + $PID) -OperationTimeoutSec 3).ParentProcessId
    if ($control.RunId -cne $RunId -or $control.Revision -cne $Revision -or $control.RunAttempt -ne $RunAttempt -or
        $control.Profile -cne $Profile -or $control.RunnerDirectory -cne $runnerDirectory -or
        $control.WorkDeadlineSeconds -ne $workDeadlineSeconds -or $control.TotalDeadlineSeconds -ne $totalDeadlineSeconds -or
        $control.Owner.Id -ne $parentId -or
        -not (Test-OwnerIdentity $control.Owner)) { throw 'Watchdog owner identity mismatch.' }
    $script:startedTick = [long]$control.StartedTick
    if ((Get-OwnerElapsed) -lt 0 -or (Get-OwnerElapsed) -ge $workDeadlineSeconds) { throw 'Watchdog monotonic start is invalid or expired.' }
    Write-OwnerJson 'watchdog-ready.json' @{ Ready=$true; RunId=$RunId; Revision=$Revision; RunAttempt=$RunAttempt; Profile=$Profile }
    while ((Get-OwnerElapsed) -lt $workDeadlineSeconds -and (Test-OwnerIdentity $control.Owner)) {
        if ([IO.File]::Exists((Join-Path $EvidenceDirectory 'owner-finished.json'))) { exit 0 }
        Start-Sleep -Milliseconds 200
        $control = Read-OwnerJson 'owner-control.json'
    }
    try { Write-OwnerJson 'watchdog-cleanup.json' (Complete-OwnerCleanup) }
    catch { Write-OwnerJson 'watchdog-cleanup.json' @{ CleanupComplete=$false; Failure='watchdog-cleanup-failed'; RunId=$RunId; Revision=$Revision } }
    if (Test-OwnerIdentity $control.Owner) {
        $owner = [Diagnostics.Process]::GetProcessById([int]$control.Owner.Id)
        # The independent watchdog is a child of this dedicated owner. Its own
        # cleanup already stopped recorded runner/API trees; do not kill self
        # midway through a recursive parent-tree termination.
        try {
            if ($owner.StartTime.ToUniversalTime().Ticks -eq [long]$control.Owner.StartTicks) { $owner.Kill() }
        } finally { $owner.Dispose() }
    }
    exit 1
}

if ($env:GITHUB_ACTIONS -ceq 'true') { throw 'This owner wrapper is separate from CI and must not run inside another job.' }
$hostArguments = [Environment]::GetCommandLineArgs()
$fileOption = -1
for ($index = 1; $index -lt $hostArguments.Length; $index++) {
    if ($hostArguments[$index] -ieq '-File') { $fileOption = $index; break }
}
if ($fileOption -lt 0 -or $fileOption + 1 -ge $hostArguments.Length -or
    [IO.Path]::GetFullPath($hostArguments[$fileOption + 1]) -ine [IO.Path]::GetFullPath($PSCommandPath) -or
    $hostArguments -inotcontains '-NoProfile' -or $hostArguments -inotcontains '-NonInteractive') {
    throw 'Run this wrapper only in its own pwsh -NoProfile -NonInteractive -File process.'
}
if (-not [IO.File]::Exists($runnerExecutable)) { throw 'The existing official ARM64 runner is required.' }
Assert-OwnerPath $runnerDirectory
Assert-OwnerPath $runnerExecutable
if ((Get-FileHash -LiteralPath $runnerExecutable -Algorithm SHA256).Hash.ToLowerInvariant() -cne $ExpectedRunnerListenerSha256) {
    throw 'The explicitly reviewed existing official runner executable differs.'
}
# This lease prevents two helpers from both observing an initially idle runner.
# It contains no credentials. A dedicated owner-process exit releases the handle;
# a surviving file is harmless and never authorizes deleting another registration.
$leasePath = Join-Path $runnerDirectory '.bonsai-native-owner.lock'
Assert-OwnerPath $leasePath
$lease = [IO.File]::Open($leasePath, [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::Read)
$cpu = Get-CimInstance Win32_Processor -OperationTimeoutSec 3 | Select-Object -First 1
if ($cpu.Name -notmatch 'Snapdragon.*X.*Elite') { throw 'The approved Snapdragon X/X2 Elite owner is required.' }
if (@(Get-IdleOwnerProcess).Count) { throw 'Close the existing Bonsai, model, speech or runner processes before this wrapper.' }
foreach ($name in $credentialNames) {
    if (Test-Path -LiteralPath (Join-Path $runnerDirectory $name)) { throw 'A runner credential already exists; no registration was attempted.' }
}
$runners = Invoke-OwnerApi -Path 'actions/runners?per_page=100'
if ($runners.total_count -ne 0) { throw 'The repository must have zero registered runners before this wrapper.' }
$run = Invoke-OwnerApi -Path ('actions/runs/' + $RunId)
if ($run.head_sha -cne $Revision -or $run.head_branch -cne 'main' -or $run.event -cne 'workflow_dispatch' -or
    $run.run_attempt -ne $RunAttempt -or $run.path -cne '.github/workflows/native-runtime-acceptance.yml' -or
    $run.display_title -cne ('Native Rust actual-model application acceptance (' + $Profile + ')')) {
    throw 'The approved run, revision, main ref, attempt, workflow and explicit profile must match.'
}
$head = Invoke-OwnerApi -Path 'commits/main'
if ($head.sha -cne $Revision) { throw 'The approved revision must still be the frozen main head.' }
$foundation = Invoke-OwnerApi -Path ('actions/runs/' + $RunnerFoundationRunId)
if ($foundation.head_sha -cne $Revision -or $foundation.head_branch -cne 'main' -or
    $foundation.path -cne '.github/workflows/foundation.yml' -or
    $foundation.status -cne 'completed' -or $foundation.conclusion -cne 'success' -or
    $foundation.repository.id -ne $run.repository.id) {
    throw 'The explicitly reviewed owning Runner Foundation must pass on this exact main revision.'
}
$jobs = Invoke-OwnerApi -Path ('actions/runs/' + $RunId + '/attempts/' + $RunAttempt + '/jobs?per_page=100')
$prepare = @($jobs.jobs | Where-Object name -CEQ 'prepare')
$acceptance = @($jobs.jobs | Where-Object name -CEQ 'acceptance')
if ($prepare.Count -ne 1 -or $acceptance.Count -ne 1 -or
    $acceptance[0].status -cne 'queued' -or @($acceptance[0].labels).Count -ne 1 -or $acceptance[0].labels[0] -cne $label) {
    throw 'One hosted prepare and one queued owner job with only the exact run label are required.'
}
if ($prepare[0].status -cne 'completed' -or $prepare[0].conclusion -cne 'success') {
    throw 'Completed successful hosted preparation is required; producer-point bypass is not supported.'
}
$preparationProof = Confirm-PreparedArtifact -Run $run -Prepare $prepare[0]
if (-not $EvidenceDirectory) { $EvidenceDirectory = Join-Path $env:TEMP ('Bonsai-NativeOwner-' + $RunId + '-' + [guid]::NewGuid().ToString('N')) }
if (-not [IO.Path]::IsPathFullyQualified($EvidenceDirectory)) { throw 'Owner evidence requires an explicit absolute directory.' }
$EvidenceDirectory = [IO.Path]::GetFullPath($EvidenceDirectory)
Assert-OwnerPath $EvidenceDirectory
if (Test-Path -LiteralPath $EvidenceDirectory) { throw 'Use a fresh owner evidence directory.' }
[void][IO.Directory]::CreateDirectory($EvidenceDirectory)
$self = [Diagnostics.Process]::GetCurrentProcess()
try { $ownerIdentity = @{ Id = $PID; StartTicks = $self.StartTime.ToUniversalTime().Ticks } } finally { $self.Dispose() }
$control = @{ RunId=$RunId; Revision=$Revision; RunAttempt=$RunAttempt; Profile=$Profile
    StartedTick=$startedTick; StartedAtUtc=$startedAt.ToString('o')
    WorkDeadlineSeconds=$workDeadlineSeconds; TotalDeadlineSeconds=$totalDeadlineSeconds; RunnerDirectory=$runnerDirectory
    Owner=$ownerIdentity; RunnerName=('codex-native-lifecycle-' + $RunId + '-' + [guid]::NewGuid().ToString('N').Substring(0, 8)); Processes=@()
    PreparationProof=$preparationProof }
Write-OwnerJson 'preparation-proof.json' $preparationProof
Write-OwnerJson 'owner-control.json' $control
$watchStart = [Diagnostics.ProcessStartInfo]::new($powershell)
$watchStart.UseShellExecute = $false
$watchStart.CreateNoWindow = $true
foreach ($argument in @('-NoLogo', '-NoProfile', '-NonInteractive', '-File', $PSCommandPath,
    '-RunId', $RunId, '-Revision', $Revision, '-RunAttempt', [string]$RunAttempt,
    '-Profile', $Profile, '-RunnerDirectory', $runnerDirectory, '-EvidenceDirectory', $EvidenceDirectory, '-Watchdog')) {
    $watchStart.ArgumentList.Add($argument)
}
$watch = [Diagnostics.Process]::Start($watchStart)
$stage = 'registration'
$passed = $false
$listenerExit = $null
$registrationValue = $null
try {
    $readyDeadline = [Diagnostics.Stopwatch]::StartNew()
    while (-not [IO.File]::Exists((Join-Path $EvidenceDirectory 'watchdog-ready.json'))) {
        if ($watch.HasExited -or $readyDeadline.Elapsed.TotalSeconds -ge 5 -or (Get-OwnerElapsed) -ge $workDeadlineSeconds) { throw 'The independent watchdog did not become ready.' }
        Start-Sleep -Milliseconds 100
    }
    $ready = Read-OwnerJson 'watchdog-ready.json'
    if ($ready.Ready -ne $true -or $ready.RunId -cne $RunId -or $ready.Revision -cne $Revision -or
        $ready.RunAttempt -ne $RunAttempt -or $ready.Profile -cne $Profile) {
        throw 'The independent watchdog readiness identity differs.'
    }
    if ($workDeadlineSeconds - (Get-OwnerElapsed) -lt 180) {
        throw 'Preparation left less than the bounded registration/job allowance; no registration was attempted.'
    }
    $registration = Invoke-OwnerApi -Path 'actions/runners/registration-token' -Method POST
    $registrationValue = $registration.token
    if ([string]::IsNullOrWhiteSpace($registrationValue)) { throw 'Registration did not return a token.' }
    $configuration = Invoke-OwnerGate -RegistrationValue $registrationValue -Arguments @('configure', '--unattended', '--ephemeral', '--disableupdate', '--url', $repositoryUrl,
        '--name', $control.RunnerName, '--work', ('_work/native-acceptance-' + $RunId), '--no-default-labels', '--labels', $label)
    $registrationValue = $null
    $registration = $null
    try {
        $stdout = $configuration.StandardOutput.ReadToEndAsync()
        $stderr = $configuration.StandardError.ReadToEndAsync()
        $left = [Math]::Min(30, $workDeadlineSeconds - (Get-OwnerElapsed))
        if ($left -le 0 -or -not $configuration.WaitForExit([int]($left * 1000)) -or $configuration.ExitCode -ne 0) { throw 'Bounded ephemeral runner configuration failed.' }
        if (-not $stdout.Wait(1000) -or -not $stderr.Wait(1000)) { throw 'Configuration output did not close.' }
    } finally { $configuration.Dispose() }
    if ((Get-Item -Force -LiteralPath (Join-Path $runnerDirectory '.runner')).Length -gt 65536) { throw 'Runner settings exceed their pre-read bound.' }
    $settings = Get-Content -LiteralPath (Join-Path $runnerDirectory '.runner') -Raw | ConvertFrom-Json
    if ($settings.agentName -cne $control.RunnerName -or $settings.gitHubUrl.TrimEnd('/') -cne $repositoryUrl -or
        $settings.ephemeral -ne $true -or $settings.disableUpdate -ne $true) { throw 'Runner configuration identity mismatch.' }
    $stage = 'listener'
    # The runner receives only this run's unique label. No other queued workload
    # can use this ephemeral owner. The independent watchdog retains the outer
    # deadline when the workflow's own five-minute allowance is still running.
    $head = Invoke-OwnerApi -Path 'commits/main'
    if ($head.sha -cne $Revision) { throw 'The settled main revision changed before listener admission.' }
    $listener = Invoke-OwnerGate -Arguments @('run', '--once')
    try {
        $stdout = $listener.StandardOutput.ReadToEndAsync()
        $stderr = $listener.StandardError.ReadToEndAsync()
        while (-not $listener.WaitForExit(200)) {
            if ((Get-OwnerElapsed) -ge $workDeadlineSeconds -or $watch.HasExited) { throw 'The absolute owner work deadline or watchdog failed.' }
        }
        $listenerExit = $listener.ExitCode
        if (-not $stdout.Wait(1000) -or -not $stderr.Wait(1000) -or $listenerExit -ne 0) { throw 'The single-job runner failed.' }
    } finally { $listener.Dispose() }
    $stage = 'result'
    $jobs = Invoke-OwnerApi -Path ('actions/runs/' + $RunId + '/attempts/' + $RunAttempt + '/jobs?per_page=100')
    $accepted = @($jobs.jobs | Where-Object name -CEQ 'acceptance')
    if ($accepted.Count -ne 1 -or $accepted[0].conclusion -cne 'success' -or $accepted[0].runner_name -cne $control.RunnerName) { throw 'The exact approved acceptance job did not pass on this runner.' }
    $passed = $true
} catch { $passed = $false }
finally {
    $registrationValue = $null
    try { $cleanup = Complete-OwnerCleanup }
    catch { $cleanup = @{ CleanupComplete=$false; Failure='owner-cleanup-failed'; RunId=$RunId; Revision=$Revision } }
    Write-OwnerJson 'cleanup.json' $cleanup
    Write-OwnerJson 'owner-lifetime.json' @{ RunId=$RunId; SourceRevision=$Revision; Profile=$Profile; RunAttempt=$RunAttempt
        StartedAtUtc=$startedAt.ToString('o'); FinishedAtUtc=[DateTime]::UtcNow.ToString('o'); TotalOwnerSeconds=[Math]::Round((Get-OwnerElapsed), 3)
        WorkDeadlineSeconds=$workDeadlineSeconds; WholeOwnerMaximumSeconds=$totalDeadlineSeconds
        CleanupReserveSeconds=($totalDeadlineSeconds - $workDeadlineSeconds); RunnerName=$control.RunnerName; ListenerExitCode=$listenerExit
        WholeOwnerLimitSatisfied=((Get-OwnerElapsed) -le $totalDeadlineSeconds)
        LastStage=$stage; AcceptancePassed=$passed; CleanupComplete=$cleanup.CleanupComplete; PreparationProof=$control.PreparationProof }
    if ($cleanup.CleanupComplete) {
        Write-OwnerJson 'owner-finished.json' @{ Complete=$true; RunId=$RunId; Revision=$Revision }
        if (-not $watch.WaitForExit(1000)) { $watch.Kill($true); [void]$watch.WaitForExit(1000) }
    }
    $watch.Dispose()
    $lease.Dispose()
}
if (-not $passed -or -not $cleanup.CleanupComplete -or (Get-OwnerElapsed) -gt $totalDeadlineSeconds) {
    throw 'The bounded owner run, absolute allowance or cleanup failed; inspect the fixed owner receipts.'
}
[pscustomobject]@{ RunId=$RunId; Revision=$Revision; Status='passed-and-cleaned'; EvidenceDirectory=$EvidenceDirectory; TotalOwnerSeconds=[Math]::Round((Get-OwnerElapsed), 3) }
