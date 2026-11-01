#requires -Version 7.6
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.HexagonPolicy.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.HexagonDispatch.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.HexagonIdentity.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Native.psm1')

function Get-BonsaiHexagonSessionPlan {
    <# .SYNOPSIS
    Describes one signed default-session FD mapping probe without opening devices.
    #>
    [CmdletBinding()]
    [OutputType([Collections.IDictionary])]
    param([ValidateRange(1,240)][int]$MaximumSeconds = 120, [switch]$ReferenceInitialization)
    return Invoke-BonsaiHexagonPolicy -Request @{ operation = 'session-plan'; maximumSeconds = $MaximumSeconds; referenceInitialization = [bool]$ReferenceInitialization }
}

function Assert-BonsaiHexagonSessionBuild {
    <# .SYNOPSIS
    Checks the exact hosted native query executable and every source input.
    #>
    [CmdletBinding()]
    [OutputType([void])]
    param([Parameter(Mandatory)][Collections.IDictionary]$Receipt,
        [Parameter(Mandatory)][string]$ExpectedRunnerRevision,
        [Parameter(Mandatory)][Collections.IDictionary]$SourceDigests,
        [Parameter(Mandatory)][string]$ManifestSha256,
        [Parameter(Mandatory)][string]$LockSha256)
    $null = Invoke-BonsaiHexagonPolicy -Request @{ operation = 'session-build'; json = (ConvertTo-Json -InputObject $Receipt -Depth 32 -Compress); expectedRunnerRevision = $ExpectedRunnerRevision; sourceDigests = $SourceDigests; manifestSha256 = $ManifestSha256; lockSha256 = $LockSha256 }
}

function Get-BonsaiHexagonSessionProbeResult {
    <# .SYNOPSIS
    Checks every completed native step without converting metadata or partial mapping into success.
    #>
    [CmdletBinding()]
    [OutputType([Collections.IDictionary])]
    param([Parameter(Mandatory)][AllowEmptyString()][string]$Json, [Parameter(Mandatory)][int]$ExitCode, [switch]$ReferenceInitialization)
    return Invoke-BonsaiHexagonPolicy -Request @{ operation = 'session-result'; json = $Json; exitCode = $ExitCode; referenceInitialization = [bool]$ReferenceInitialization }
}

function Get-BonsaiHexagonSessionWorkerResult {
    <# .SYNOPSIS
    Validates retained native session evidence without granting admission or teardown.
    #>
    [CmdletBinding()]
    [OutputType([Collections.IDictionary])]
    param([Parameter(Mandatory)][string]$Json, [Parameter(Mandatory)][string]$ExpectedRunnerRevision, [switch]$ReferenceInitialization)
    return Invoke-BonsaiHexagonPolicy -Request @{ operation = 'session-worker'; json = $Json; expectedRunnerRevision = $ExpectedRunnerRevision; referenceInitialization = [bool]$ReferenceInitialization }
}

function Invoke-BonsaiHexagonSessionWorker {
    <# .SYNOPSIS
    Verifies all signed package/source bytes before one owned native FD mapping probe.
    #>
    [CmdletBinding()]
    [OutputType([Collections.Specialized.OrderedDictionary])]
    param([Parameter(Mandatory)][Collections.IDictionary]$Request)
    if ($Request.ReferenceProcessInitializationSelected -isnot [bool]) { throw 'The reference-process initialization mode requires an explicit boolean selection.' }
    $holds = [Collections.Generic.List[IO.FileStream]]::new()
    $report = [ordered]@{
        SchemaVersion = 1; Kind = 'runner-hexagon-session-worker'; SourceRevision = $Request.ExpectedRunnerRevision
        Outcome = 'pending'; Stage = 'source-identity'; Failure = $null
        LastNativeStage = 'not-started'
        ReferenceProcessInitializationSelected = $Request.ReferenceProcessInitializationSelected
        UnsignedProcessRequested = $false; ProcessInitializationResult = $null
        SignedPackageVerified = $false; RuntimeFilesVerified = 0; DriverIdentityVerified = $false; DriverIdentity = $null
        MappingCycleVerified = $false; NativeResult = $null
        ExecutableSha256 = $null; BuildReceiptSha256 = $null; ManifestSha256 = $null; LockSha256 = $null
        ModelLoaded = $false; SwiftQualified = $false; ProfileEligible = $false
        DriverInstalled = $false; TrustChanged = $false; ComputeExecuted = $false
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
        $receiptPath = Join-Path $Request.FixtureDirectory 'native-build.json'
        $entries = @([IO.Directory]::EnumerateFileSystemEntries($Request.FixtureDirectory))
        if ($entries.Count -ne 2) { throw 'Native session bundle must contain exactly two files.' }
        foreach ($entry in $entries) {
            if ([IO.Path]::GetFileName($entry) -cnotin @('native-build.json','native-hexagon-query.exe') -or
                ([IO.File]::GetAttributes($entry) -band ([IO.FileAttributes]::Directory -bor [IO.FileAttributes]::ReparsePoint))) { throw 'Unreviewed native session bundle entry.' }
        }
        $receiptStream = [IO.FileStream]::new($receiptPath, 'Open', 'Read', 'Read'); $holds.Add($receiptStream)
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
        if ($manifestStream.Length -le 0 -or $manifestStream.Length -gt 65536) { throw 'Native manifest exceeds its byte bound.' }
        $report.ManifestSha256 = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($manifestStream)).ToLowerInvariant()
        $lockStream = [IO.FileStream]::new($Request.LockPath, 'Open', 'Read', 'Read'); $holds.Add($lockStream)
        $lock = Read-BonsaiHexagonDispatchJson -Stream $lockStream
        $lockStream.Position = 0
        $report.LockSha256 = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($lockStream)).ToLowerInvariant()
        Assert-BonsaiHexagonSessionBuild -Receipt $receipt -ExpectedRunnerRevision $Request.ExpectedRunnerRevision -SourceDigests $sourceDigests -ManifestSha256 $report.ManifestSha256 -LockSha256 $report.LockSha256
        $fixturePath = Join-Path $Request.FixtureDirectory 'native-hexagon-query.exe'
        $holds.Add((Get-BonsaiHexagonDispatchFile -Path $fixturePath -Pin @{ bytes = $receipt.Executable.Bytes; sha256 = $receipt.Executable.Sha256 }))
        Assert-Arm64PE -Path $fixturePath
        $report.ExecutableSha256 = $receipt.Executable.Sha256
        $report.Stage = 'runtime-identity'
        Save-BonsaiHexagonDispatchWorkerReceipt -Report $report -Path $Request.WorkerReceiptPath
        $members = @('libggml-htp-v73.so','libggml-htp-v75.so','libggml-htp-v79.so','libggml-htp-v81.so')
        if (($lock.schemaVersion -isnot [int] -and $lock.schemaVersion -isnot [long]) -or $lock.schemaVersion -ne 1 -or
            $lock.ggmlCommit -cne '94256114c229674ef96e76eb2dea596e65b43818' -or
            $lock.files -isnot [array] -or $lock.files.Count -ne 16 -or $lock.catalog.name -cne 'libggml-htp.cat' -or
            $lock.catalog.members -isnot [array] -or $lock.catalog.members.Count -ne 4) { throw 'Invalid native signed-package lock.' }
        for ($index = 0; $index -lt 4; $index++) {
            if ($lock.catalog.members[$index] -isnot [string] -or $lock.catalog.members[$index] -cne $members[$index]) { throw 'Native catalog member set changed.' }
        }
        $names = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
        [long]$total = 0
        foreach ($pin in $lock.files) {
            if ($pin -isnot [Collections.IDictionary] -or $pin.name -isnot [string] -or
                $pin.name -cnotmatch '^[A-Za-z0-9][A-Za-z0-9_.-]{0,127}$' -or -not $names.Add($pin.name)) { throw 'Invalid native runtime file pin.' }
            $holds.Add((Get-BonsaiHexagonDispatchFile -Path (Join-Path $Request.PackageDirectory $pin.name) -Pin $pin))
            $total += $pin.bytes
            if ($total -gt 256MB) { throw 'Native runtime package exceeds its byte bound.' }
            $report.RuntimeFilesVerified++
        }
        $entries = @([IO.Directory]::EnumerateFileSystemEntries($Request.PackageDirectory))
        if ($entries.Count -ne 16) { throw 'Native package directory contains unpinned files.' }
        foreach ($entry in $entries) {
            if (-not $names.Contains([IO.Path]::GetFileName($entry)) -or
                ([IO.File]::GetAttributes($entry) -band ([IO.FileAttributes]::Directory -bor [IO.FileAttributes]::ReparsePoint))) { throw 'Native runtime contains an unpinned entry or link.' }
        }
        foreach ($member in @('libggml-htp.cat','libggml-htp.inf') + $members) {
            if (-not $names.Contains($member)) { throw 'Native runtime omits a signed module deployment input.' }
        }
        $report.Stage = 'catalog-verification'
        Save-BonsaiHexagonDispatchWorkerReceipt -Report $report -Path $Request.WorkerReceiptPath
        $holds.Add((Get-BonsaiHexagonDispatchFile -Path $Request.SigntoolPath -Pin $lock.signtool))
        $arguments = @('verify','/pa','/v','/c',(Join-Path $Request.PackageDirectory 'libggml-htp.cat')) + @(foreach ($member in $members) { Join-Path $Request.PackageDirectory $member })
        $signature = Invoke-BonsaiNativeProcess -Executable $Request.SigntoolPath -ArgumentList $arguments -WorkingDirectory $Request.PackageDirectory -TimeoutSeconds 10
        if ($signature.ExitCode -ne 0) { throw 'Native module catalog membership verification failed.' }
        $report.SignedPackageVerified = $true
        $report.Stage = 'driver-inspection'
        Save-BonsaiHexagonDispatchWorkerReceipt -Report $report -Path $Request.WorkerReceiptPath
        $inspection = Invoke-BonsaiHexagonBoundedCapture -Executable $fixturePath -ArgumentList @('--inspect') -WorkingDirectory $Request.PackageDirectory
        $driver = ConvertFrom-BonsaiHexagonDispatchJson -Json $inspection.StandardOutput
        if ($inspection.ExitCode -ne 0 -or ($driver.schemaVersion -isnot [int] -and $driver.schemaVersion -isnot [long]) -or
            $driver.schemaVersion -ne 1 -or $driver.kind -isnot [string] -or $driver.kind -cne 'native-fastrpc-inspection' -or $driver.driverDll -isnot [string]) { throw 'Native driver inspection failed.' }
        foreach ($field in @('driverLoaded','dspCodeLoaded','profileEligible')) {
            if ($driver[$field] -isnot [bool] -or $driver[$field]) { throw 'Native inspection claimed execution.' }
        }
        $driverGuard = Get-BonsaiHexagonDriverGuard -Path $driver.driverDll
        $holds.Add($driverGuard.Stream)
        $report.DriverIdentity = $driverGuard.Identity
        $report.DriverIdentityVerified = $true
        $report.Stage = 'session-mapping'
        Save-BonsaiHexagonDispatchWorkerReceipt -Report $report -Path $Request.WorkerReceiptPath
        # This is the owned child worker's environment only. The query admits
        # exactly the guarded route and rechecks its own module file handles.
        [Environment]::SetEnvironmentVariable('ADSP_LIBRARY_PATH', $Request.PackageDirectory, 'Process')
        $mode = if ($Request.ReferenceProcessInitializationSelected) { '--reference-session-mapping-probe' } else { '--session-mapping-probe' }
        if ($Request.ReferenceProcessInitializationSelected) {
            # An entered marker alone cannot prove the control call executed.
            $report.UnsignedProcessRequested = $null
            Save-BonsaiHexagonDispatchWorkerReceipt -Report $report -Path $Request.WorkerReceiptPath
        }
        $probe = Invoke-BonsaiHexagonBoundedCapture -Executable $fixturePath -ArgumentList @($mode, $Request.PackageDirectory) -WorkingDirectory $Request.PackageDirectory -ProgressReceipt $report -ProgressPath $Request.WorkerReceiptPath -ProgressKind Session
        $native = Get-BonsaiHexagonSessionProbeResult -Json $probe.StandardOutput -ExitCode $probe.ExitCode -ReferenceInitialization:$Request.ReferenceProcessInitializationSelected
        if ($native.driverDll -cne $driverGuard.Identity.Path) { throw 'Native session loaded a different inspected driver.' }
        $report.NativeResult = $native
        $report.UnsignedProcessRequested = $native.unsignedProcessRequested
        $report.ProcessInitializationResult = $native.processInitializationResult
        if ($native.passed) {
            $report.MappingCycleVerified = $true
            $report.Outcome = 'passed'
            $report.Stage = 'session-mapping-complete'
        } else { $report.Failure = 'native-session-mapping-incomplete' }
    } catch {
        $report.Failure = $report.Stage + '-rejected'
        $report.Outcome = 'pending'
    } finally {
        # BoundedCapture cannot return until the child has exited. If it cannot
        # confirm exit, the outer kill-on-close Job owns these retained handles.
        foreach ($stream in $holds) { $stream.Dispose() }
        Save-BonsaiHexagonDispatchWorkerReceipt -Report $report -Path $Request.WorkerReceiptPath
    }
    return $report
}

Export-ModuleMember -Function Get-BonsaiHexagonSessionPlan, Assert-BonsaiHexagonSessionBuild, Get-BonsaiHexagonSessionProbeResult, Get-BonsaiHexagonSessionWorkerResult, Invoke-BonsaiHexagonSessionWorker
