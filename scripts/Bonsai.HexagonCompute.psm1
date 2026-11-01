#requires -Version 7.6
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.HexagonSession.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.HexagonDispatch.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.HexagonIdentity.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Native.psm1')

function Get-BonsaiHexagonComputeResult {
    <# .SYNOPSIS
    Checks fixed native queue computation without granting model or teardown qualification.
    #>
    [CmdletBinding()]
    [OutputType([Collections.IDictionary])]
    param([Parameter(Mandatory)][string]$Json, [Parameter(Mandatory)][int]$ExitCode,
        [ValidateSet('F32','GuardedQ2')][string]$Case = 'F32')
    $record = ConvertFrom-BonsaiHexagonDispatchJson -Json $Json
    $kind = if ($Case -ceq 'F32') { 'native-fastrpc-queue-compute-probe' } else { 'native-fastrpc-queue-q2-probe' }
    if ($ExitCode -ne 0 -or ($record.schemaVersion -isnot [int] -and $record.schemaVersion -isnot [long]) -or $record.schemaVersion -ne 1 -or
        $record.kind -cne $kind -or
        $record.stage -isnot [string] -or $record.stage -cnotmatch '^[a-z][a-z0-9-]{0,95}$' -or
        $record.driverDll -isnot [string] -or -not [IO.Path]::IsPathFullyQualified($record.driverDll)) {
        throw 'Invalid fixed native computation receipt.'
    }
    if ($Case -ceq 'GuardedQ2') {
        foreach ($pair in @(@('columns',256),@('rows',32),@('batch',2),@('outputValuesChecked',64))) {
            $value = $record[$pair[0]]
            if (($value -isnot [int] -and $value -isnot [long]) -or $value -ne $pair[1]) { throw 'Guarded Q2 probe changed its fixed geometry.' }
        }
    }
    foreach ($name in @('passed','uriVerified','unsignedProcessRequested','handleObserved',
        'responseValidated','numericValidated','freeCompleted','ownerPinnedUntilExit','poisoned',
        'computeSubmitted','computeExecuted','profileEligible','sessionTeardownVerified')) {
        if ($record[$name] -isnot [bool]) { throw 'Native computation flags require exact booleans.' }
    }
    if ($record.profileEligible -or $record.sessionTeardownVerified -or
        ($record.computeExecuted -and (-not $record.computeSubmitted -or -not $record.numericValidated -or -not $record.responseValidated))) {
        throw 'Native computation claimed an unsupported independent gate.'
    }
    if ($record.passed) {
        foreach ($name in @('uriVerified','unsignedProcessRequested','handleObserved','responseValidated',
            'numericValidated','freeCompleted','ownerPinnedUntilExit','computeSubmitted','computeExecuted')) {
            if (-not $record[$name]) { throw 'Incomplete native computation claimed success.' }
        }
        foreach ($name in @('domainResult','uriResult','processInitializationResult','moduleOpenResult',
            'queueCreateResult','queueExportResult','queueStartResult','queueWriteResult','queueReadResult',
            'queueStopResult','queueCloseResult','mappingResult','unmappingResult','moduleCloseResult')) {
            if (($record[$name] -isnot [int] -and $record[$name] -isnot [long]) -or $record[$name] -ne 0) {
                throw 'Native computation omitted a completed lifecycle step.'
            }
        }
        if ($record.architecture -ne 81 -or $record.effectiveDomain -ne 3 -or $record.poisoned) {
            throw 'Native computation changed its fixed X2 domain or retained uncertain work.'
        }
    }
    return $record
}

function Invoke-BonsaiHexagonComputeWorker {
    <# .SYNOPSIS
    Verifies the source-bound fixture, new signed package and driver inside an owned Job.
    #>
    [CmdletBinding()]
    [OutputType([Collections.Specialized.OrderedDictionary])]
    param([Parameter(Mandatory)][Collections.IDictionary]$Request)
    if ($Request.ComputeCase -isnot [string] -or $Request.ComputeCase -cnotin @('F32','GuardedQ2')) { throw 'Select exactly one fixed compute case.' }
    $holds = [Collections.Generic.List[IO.FileStream]]::new()
    $report = [ordered]@{
        SchemaVersion = 1; Kind = 'runner-hexagon-compute-worker'; SourceRevision = $Request.ExpectedRunnerRevision; ComputeCase = $Request.ComputeCase
        Outcome = 'pending'; Stage = 'source-identity'; LastNativeStage = 'not-started'; Failure = $null
        SignedPackageVerified = $false; RuntimeFilesVerified = 0; DriverIdentity = $null; NativeResult = $null
        BuildReceiptSha256 = $null; CandidateLockSha256 = $null; ComputeExecuted = $false
        ModelLoaded = $false; SwiftQualified = $false; ProfileEligible = $false; TrustChanged = $false
    }
    try {
        Save-BonsaiHexagonDispatchWorkerReceipt -Report $report -Path $Request.WorkerReceiptPath
        $environment = Get-BonsaiHexagonDispatchEnvironment -RuntimeDirectory $Request.PackageDirectory -SystemDirectory ([Environment]::SystemDirectory)
        foreach ($variable in @(Get-ChildItem Env:)) {
            if ($environment.RemoveNames -contains $variable.Name -or @($environment.RemovePrefixes | Where-Object { $variable.Name.StartsWith($_, [StringComparison]::OrdinalIgnoreCase) }).Count) {
                [Environment]::SetEnvironmentVariable($variable.Name, $null, 'Process')
            }
        }
        [Environment]::SetEnvironmentVariable('PATH', [Environment]::SystemDirectory, 'Process')
        $entries = @([IO.Directory]::EnumerateFileSystemEntries($Request.FixtureDirectory))
        if ($entries.Count -ne 2) { throw 'Native compute bundle requires exactly two entries.' }
        foreach ($entry in $entries) {
            if ([IO.Path]::GetFileName($entry) -cnotin @('native-build.json','native-hexagon-query.exe') -or
                ([IO.File]::GetAttributes($entry) -band ([IO.FileAttributes]::Directory -bor [IO.FileAttributes]::ReparsePoint))) {
                throw 'Unreviewed native compute bundle entry.'
            }
        }
        $receiptStream = [IO.FileStream]::new((Join-Path $Request.FixtureDirectory 'native-build.json'), 'Open', 'Read', 'Read'); $holds.Add($receiptStream)
        $receipt = Read-BonsaiHexagonDispatchJson -Stream $receiptStream
        $receiptStream.Position = 0
        $report.BuildReceiptSha256 = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($receiptStream)).ToLowerInvariant()
        $sourceDigests = @{}
        foreach ($name in @('lib.rs','windows.rs','mapping.rs','session.rs','session_native.rs','session_mapping.rs','session_mapping_native.rs','compute.rs','compute_native.rs','query.rs','sources.json')) {
            $stream = [IO.FileStream]::new((Join-Path $Request.NativeSourceDirectory $name), 'Open', 'Read', 'Read'); $holds.Add($stream)
            if ($stream.Length -le 0 -or $stream.Length -gt 1MB) { throw 'Native source exceeds its byte bound.' }
            $sourceDigests[$name] = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($stream)).ToLowerInvariant()
        }
        $manifestStream = [IO.FileStream]::new($Request.ManifestPath, 'Open', 'Read', 'Read'); $holds.Add($manifestStream)
        $manifestHash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($manifestStream)).ToLowerInvariant()
        $lockStream = [IO.FileStream]::new($Request.LockPath, 'Open', 'Read', 'Read'); $holds.Add($lockStream)
        $lock = Read-BonsaiHexagonDispatchJson -Stream $lockStream
        $lockStream.Position = 0
        $lockHash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($lockStream)).ToLowerInvariant()
        Assert-BonsaiHexagonSessionBuild -Receipt $receipt -ExpectedRunnerRevision $Request.ExpectedRunnerRevision -SourceDigests $sourceDigests -ManifestSha256 $manifestHash -LockSha256 $lockHash
        $exe = Join-Path $Request.FixtureDirectory 'native-hexagon-query.exe'
        $holds.Add((Get-BonsaiHexagonDispatchFile -Path $exe -Pin @{ bytes = $receipt.Executable.Bytes; sha256 = $receipt.Executable.Sha256 }))
        Assert-Arm64PE -Path $exe
        $candidateStream = [IO.FileStream]::new($Request.CandidateLockPath, 'Open', 'Read', 'Read'); $holds.Add($candidateStream)
        # This source audit is larger than a runtime receipt. It is never child output.
        if ($candidateStream.Length -le 0 -or $candidateStream.Length -gt 1MB) { throw 'Candidate lock exceeds its byte bound.' }
        $candidateReader = [IO.StreamReader]::new($candidateStream, [Text.UTF8Encoding]::new($false, $true), $true, 4096, $true)
        try { $candidate = ConvertFrom-BonsaiHexagonDispatchJson -Json $candidateReader.ReadToEnd() } finally { $candidateReader.Dispose() }
        $candidateStream.Position = 0
        $report.CandidateLockSha256 = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($candidateStream)).ToLowerInvariant()
        if ($candidate.candidate.tag -cne 'v0.10.0' -or $candidate.candidate.gitlink.commit -cne '19cce6919992fdffd92e0ac152caaf1e2b8f254e' -or
            $candidate.verifiedRuntimeFiles -isnot [array] -or $candidate.verifiedRuntimeFiles.Count -ne 17 -or
            $candidate.verifiedCatalogMembers -isnot [array] -or $candidate.verifiedCatalogMembers.Count -ne 5) {
            throw 'The compute probe requires the exact newer signed package.'
        }
        $report.Stage = 'runtime-identity'
        Save-BonsaiHexagonDispatchWorkerReceipt -Report $report -Path $Request.WorkerReceiptPath
        $names = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
        [long]$total = 0
        foreach ($pin in $candidate.verifiedRuntimeFiles) {
            if ($pin.Name -isnot [string] -or $pin.Name -cnotmatch '^[A-Za-z0-9][A-Za-z0-9_.-]{0,127}$' -or -not $names.Add($pin.Name)) { throw 'Invalid native runtime file pin.' }
            $holds.Add((Get-BonsaiHexagonDispatchFile -Path (Join-Path $Request.PackageDirectory $pin.Name) -Pin @{ bytes = $pin.Bytes; sha256 = $pin.Sha256 }))
            $total += $pin.Bytes
            if ($total -gt 256MB) { throw 'Native runtime exceeds its byte bound.' }
            $report.RuntimeFilesVerified++
        }
        $entries = @([IO.Directory]::EnumerateFileSystemEntries($Request.PackageDirectory))
        if ($entries.Count -ne 17) { throw 'Native package contains unpinned files.' }
        foreach ($entry in $entries) {
            if (-not $names.Contains([IO.Path]::GetFileName($entry)) -or
                ([IO.File]::GetAttributes($entry) -band ([IO.FileAttributes]::Directory -bor [IO.FileAttributes]::ReparsePoint))) { throw 'Unpinned runtime entry or link.' }
        }
        $members = @('libggml-htp-v73.so','libggml-htp-v75.so','libggml-htp-v79.so','libggml-htp-v81.so','libggml-htp-v85.so')
        for ($index = 0; $index -lt $members.Count; $index++) {
            if ($candidate.verifiedCatalogMembers[$index] -cne $members[$index]) { throw 'Signed member set changed.' }
        }
        $report.Stage = 'catalog-verification'
        Save-BonsaiHexagonDispatchWorkerReceipt -Report $report -Path $Request.WorkerReceiptPath
        $holds.Add((Get-BonsaiHexagonDispatchFile -Path $Request.SigntoolPath -Pin $lock.signtool))
        $signatureArguments = @('verify','/pa','/v','/c',(Join-Path $Request.PackageDirectory 'libggml-htp.cat')) + @(foreach ($member in $members) { Join-Path $Request.PackageDirectory $member })
        $signature = Invoke-BonsaiNativeProcess -Executable $Request.SigntoolPath -ArgumentList $signatureArguments -WorkingDirectory $Request.PackageDirectory -TimeoutSeconds 10
        if ($signature.ExitCode -ne 0) { throw 'Native DSP catalog verification failed.' }
        $report.SignedPackageVerified = $true
        $report.Stage = 'driver-inspection'
        Save-BonsaiHexagonDispatchWorkerReceipt -Report $report -Path $Request.WorkerReceiptPath
        $inspection = Invoke-BonsaiHexagonBoundedCapture -Executable $exe -ArgumentList @('--inspect') -WorkingDirectory $Request.PackageDirectory
        $driver = ConvertFrom-BonsaiHexagonDispatchJson -Json $inspection.StandardOutput
        if ($inspection.ExitCode -ne 0 -or $driver.kind -cne 'native-fastrpc-inspection' -or $driver.driverLoaded -cne $false -or $driver.dspCodeLoaded -cne $false -or $driver.profileEligible -cne $false) {
            throw 'Native driver inspection failed.'
        }
        $guard = Get-BonsaiHexagonDriverGuard -Path $driver.driverDll
        $holds.Add($guard.Stream); $report.DriverIdentity = $guard.Identity
        $report.Stage = 'queue-compute'
        Save-BonsaiHexagonDispatchWorkerReceipt -Report $report -Path $Request.WorkerReceiptPath
        [Environment]::SetEnvironmentVariable('ADSP_LIBRARY_PATH', $Request.PackageDirectory, 'Process')
        $mode = if ($Request.ComputeCase -ceq 'F32') { '--reference-queue-compute-probe' } else { '--reference-queue-q2-probe' }
        $probe = Invoke-BonsaiHexagonBoundedCapture -Executable $exe -ArgumentList @($mode, $Request.PackageDirectory) -WorkingDirectory $Request.PackageDirectory -ProgressReceipt $report -ProgressPath $Request.WorkerReceiptPath -ProgressKind Compute
        $native = Get-BonsaiHexagonComputeResult -Json $probe.StandardOutput -ExitCode $probe.ExitCode -Case $Request.ComputeCase
        if ($native.driverDll -cne $guard.Identity.Path) { throw 'Native computation loaded a different inspected driver.' }
        $report.NativeResult = $native; $report.ComputeExecuted = $native.computeExecuted
        if ($native.passed) { $report.Outcome = 'passed'; $report.Stage = 'queue-compute-complete' }
        else { $report.Failure = 'native-queue-compute-incomplete' }
    } catch {
        $report.Failure = $report.Stage + '-rejected'
    } finally {
        foreach ($stream in $holds) { $stream.Dispose() }
        Save-BonsaiHexagonDispatchWorkerReceipt -Report $report -Path $Request.WorkerReceiptPath
    }
    return $report
}

Export-ModuleMember -Function Get-BonsaiHexagonComputeResult, Invoke-BonsaiHexagonComputeWorker
