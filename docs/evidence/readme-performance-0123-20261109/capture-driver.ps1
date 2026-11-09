param(
    [Parameter(Mandatory=$true)][string]$Repository,
    [Parameter(Mandatory=$true)][string]$Revision,
    [Parameter(Mandatory=$true)][string]$OutputDirectory,
    [Parameter(Mandatory=$true)][string]$EvidenceDirectory,
    [Parameter(Mandatory=$true)][string]$SourceDriverPath,
    [Parameter(Mandatory=$true)][string]$VerifiedBundleJsonPath,
    [Parameter(Mandatory=$true)][string]$PublishedReleaseJsonPath,
    [Parameter(Mandatory=$true)][string]$CratePublicationReceiptPath
)

$trialStartedTick = [Diagnostics.Stopwatch]::GetTimestamp()
$trialStartedUtc = [DateTime]::UtcNow.ToString('o')
$trialClock = [Diagnostics.Stopwatch]::StartNew()
$ErrorActionPreference = 'Stop'
$maximumSeconds = 420
$processReserveSeconds = 60
$utf8 = New-Object Text.UTF8Encoding($false)
$expectedRuntimeRevision='055c7430cb4771dfddb24ddaa30494af6ad182da'
$expectedRuntimeVersion='0.1.23'
$ModelCase='token_timing::pinned_native_row4_captures_separated_token_timings'
$ControlFilter='token_timing'
if (-not [IO.Path]::IsPathRooted($SourceDriverPath) -or -not (Test-Path -LiteralPath $SourceDriverPath -PathType Leaf)) {
    throw 'Supply the mandatory exact absolute SourceDriverPath explicitly.'
}
$expectedDriver=[IO.Path]::GetFullPath((Join-Path $Repository 'docs/evidence/readme-performance-0123-20261109/capture-driver.ps1'))
if (-not [string]::Equals([IO.Path]::GetFullPath($SourceDriverPath), $expectedDriver, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'SourceDriverPath must identify this exact committed driver in the primary source checkout.'
}
$publicDirectory=$EvidenceDirectory
if (-not [IO.Path]::IsPathRooted($OutputDirectory) -or -not [IO.Path]::IsPathRooted($EvidenceDirectory)) {
    throw 'Working and retained evidence directories must be explicit absolute paths.'
}
if (Test-Path -LiteralPath $publicDirectory) { throw 'Public evidence destination already exists; never mix trial receipts.' }
if (Test-Path -LiteralPath $OutputDirectory) { throw 'A fresh output directory is required; do not retry a closed trial.' }
[void][IO.Directory]::CreateDirectory($OutputDirectory)
$receiptPath = Join-Path $OutputDirectory 'session.json'
$session = [ordered]@{
    schemaVersion=1; kind='post-repin-owner-authorized-short-token-timing'; revision=$Revision
    startedUtc=$trialStartedUtc; startedTick=$trialStartedTick
    maximumAggregateSeconds=$maximumSeconds; processAndRetentionReserveSeconds=$processReserveSeconds
    authorization='Owner explicitly requested promoting the new release and then checking performance after repinning. This permits one fresh0.1.23/a7 capture only; all historical clocks remain closed and no retry or CI action is authorized.'
    fixedWork='Verify actual published/pinned0.1.23/a7 identities, cache-only runtime provisioning with existing helper, one offline Release broker library/native_model build, four focused controls, then one PQ2 and one PTQ1 eight-token request in fresh required-sandbox owners; four workers, adapter1.3, seed42, Thinkingoff.'
    estimatedSeconds=380
    estimateBreakdownSeconds=@{sourceAndCachedProvision=45; build=35; fourControls=10; twoStartups=100; requests=100; cleanup=60; retention=30}
    allowanceReason='The conservative380-second fixed-work estimate exceeds five minutes; seven minutes is the smallest whole-minute allowance above it. Stop on completion, failure or insufficient original time.'
    status='preparing'; phase='power-provider'; powerBefore=$null; powerAfter=$null
    sourceIntake=$null; sourceAfter=$null; publicationProof=$null; provisioning=$null
    powerSamples=@(); build=$null; controls=@(); capture=$null; failure=$null
    retries=0; ciActions=0; downloads=0; performanceQualified=$false
    testExecutables=$null; caseBudgetSeconds=$null; finishedUtc=$null; withinAggregateAllowance=$null
    elapsedSeconds=$null; continuousPowerStabilityProven=$false
}
function Save-Session {
    $session.elapsedSeconds = $trialClock.Elapsed.TotalSeconds
    [IO.File]::WriteAllText($receiptPath, ($session | ConvertTo-Json -Depth 64), $utf8)
}
$proofIdentities = New-Object 'Collections.Generic.List[object]'
function Read-Proof([string]$Path,[string]$Leaf) {
    if (-not [IO.Path]::IsPathRooted($Path) -or -not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw 'Each publication/bundle proof must be an explicit absolute file.'
    }
    $item=Get-Item -LiteralPath $Path
    if ($item.Length -gt 8MB -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw 'Proof is oversized or redirects elsewhere.'
    }
    $bytes=[IO.File]::ReadAllBytes($Path)
    [IO.File]::WriteAllBytes((Join-Path $OutputDirectory $Leaf),$bytes)
    $proofIdentities.Add([ordered]@{path=$Path; retained=$Leaf; bytes=$bytes.Length
        sha256=(Get-FileHash -LiteralPath (Join-Path $OutputDirectory $Leaf) -Algorithm SHA256).Hash.ToLowerInvariant()})
    return ([Text.Encoding]::UTF8.GetString($bytes).TrimStart([char]0xfeff) | ConvertFrom-Json)
}
function Require-ExactControl([string]$Stage,[string]$Name) {
    $output=Get-Content -LiteralPath (Join-Path $OutputDirectory "$Stage.stdout.log") -Raw -Encoding UTF8
    if ([regex]::Matches($output,('(?m)^test '+[regex]::Escape($Name)+' \.\.\. ok\r?$')).Count -ne 1 -or
        $output -notmatch '(?m)^running 1 test\r?$' -or
        $output -notmatch '(?m)^test result: ok\. 1 passed; 0 failed; 0 ignored; 0 measured;') {
        throw 'A required exact focused control was absent, ignored or unsuccessful.'
    }
}
function Read-Power {
    $observation = [ordered]@{utc=[DateTime]::UtcNow.ToString('o'); elapsedSeconds=$trialClock.Elapsed.TotalSeconds
        status='unknown'; source='GetSystemPowerStatus'; ac_online=$null; battery_saver=$null; battery_percent=$null}
    try {
        $value = New-Object Readme0123TimingPower.Status
        if ([Readme0123TimingPower.Native]::GetSystemPowerStatus([ref]$value)) {
            $observation.status='observed'
            if ($value.ACLineStatus -ne 255) { $observation.ac_online=($value.ACLineStatus -eq 1) }
            $observation.battery_saver=($value.SystemStatusFlag -eq 1)
            if ($value.BatteryLifePercent -le 100) { $observation.battery_percent=[int]$value.BatteryLifePercent }
        }
    } catch { }
    return $observation
}
function Run-OwnedStage([string]$StageName,[string]$Executable,[string[]]$Arguments,[int]$LatestFinishSeconds) {
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
    foreach ($credentialEnvironmentName in @('ACTIONS_RUNTIME_TOKEN','ACTIONS_ID_TOKEN_REQUEST_TOKEN','GITHUB_TOKEN','GH_TOKEN','ACTIONS_RUNNER_INPUT_TOKEN')) {
        $info.EnvironmentVariables.Remove($credentialEnvironmentName)
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
        [IO.File]::WriteAllText((Join-Path $OutputDirectory "$StageName.stdout.log"), $stdout.GetAwaiter().GetResult(), $utf8)
        [IO.File]::WriteAllText((Join-Path $OutputDirectory "$StageName.stderr.log"), $stderr.GetAwaiter().GetResult(), $utf8)
        return [ordered]@{name=$StageName; exitCode=$process.ExitCode; expired=$expired; startedAtAggregateSeconds=$stageStart
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
namespace Readme0123TimingPower {
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
    $session.phase='source-intake'
    $git='C:\Users\symbi\AppData\Local\Microsoft\WinGet\Packages\Git.MinGit_Microsoft.Winget.Source_8wekyb3d8bbwe\cmd\git.exe'
    $observedRevision=(& $git -C $Repository rev-parse HEAD | Out-String).Trim()
    if ($LASTEXITCODE -ne 0 -or $observedRevision -ne $Revision) { throw 'Committed source revision mismatch.' }
    $observedBranch=(& $git -C $Repository branch --show-current | Out-String).Trim()
    if ($LASTEXITCODE -ne 0 -or $observedBranch -ne 'main') { throw 'The primary source checkout must remain on local main.' }
    $sourceStatus=(& $git -C $Repository status --porcelain | Out-String).Trim()
    if ($LASTEXITCODE -ne 0 -or $sourceStatus.Length -ne 0) { throw 'Clean committed source is required before building.' }
    $sourcePaths=@(& $git -C $Repository ls-files -- Cargo.toml Cargo.lock rust-toolchain.toml rustfmt.toml config src)
    if ($LASTEXITCODE -ne 0 -or $sourcePaths.Count -eq 0) { throw 'Tracked source input selection failed.' }
    $sourceInputs=@($sourcePaths | ForEach-Object {
        $sourcePath=Join-Path $Repository $_
        [ordered]@{path=$_; bytes=(Get-Item -LiteralPath $sourcePath).Length
            sha256=(Get-FileHash -LiteralPath $sourcePath -Algorithm SHA256).Hash.ToLowerInvariant()}
    })
    $planPath=Join-Path (Split-Path -Parent $SourceDriverPath) 'workload-plan.json'
    $sourceIntake=[ordered]@{revision=$observedRevision; branch=$observedBranch; clean=$true
        selectedSourceCount=$sourceInputs.Count; sourceInputs=$sourceInputs
        driverSha256=(Get-FileHash -LiteralPath $SourceDriverPath -Algorithm SHA256).Hash.ToLowerInvariant()
        workloadPlanSha256=(Get-FileHash -LiteralPath $planPath -Algorithm SHA256).Hash.ToLowerInvariant()}
    [IO.File]::WriteAllText((Join-Path $OutputDirectory 'source-intake.json'), ($sourceIntake | ConvertTo-Json -Depth 8), $utf8)
    $session.sourceIntake=@{revision=$observedRevision; clean=$true; selectedSourceCount=$sourceInputs.Count}
    Save-Session
    $session.phase='published-runtime-proof-and-cached-provision'
    $pinPath=Join-Path $Repository 'config/prism-inference.lock.json'
    $pin=Read-Proof $pinPath 'runtime-lock.json'
    $bundleProof=Read-Proof $VerifiedBundleJsonPath 'verified-bundle.json'
    $releaseProof=Read-Proof $PublishedReleaseJsonPath 'published-release.json'
    $crateProof=Read-Proof $CratePublicationReceiptPath 'crate-publication-receipt.json'
    $assetName='prism-inference-0.1.23-aarch64-pc-windows-msvc.zip'
    $assetUrl='https://github.com/symbiogenesis/prism-inference/releases/download/v0.1.23/'+$assetName
    if ($pin.schemaVersion -ne 1 -or $pin.package -cne 'prism-inference' -or
        $pin.version -cne $expectedRuntimeVersion -or $pin.commit -cne $expectedRuntimeRevision -or
        $pin.repository -cne 'https://github.com/symbiogenesis/prism-inference' -or
        $pin.target -cne 'aarch64-pc-windows-msvc' -or $pin.release -cne 'v0.1.23' -or
        $pin.asset.filename -cne $assetName -or $pin.asset.url -cne $assetUrl -or
        $pin.asset.bytes -le 0 -or $pin.asset.sha256 -cnotmatch '^[a-f0-9]{64}$') {
        throw 'Primary committed runtime pin is not the required actual0.1.23/a7 release.'
    }
    $requiredNames=@('prism-inference.exe','bonsai-token-worker.exe','bonsai-swift-worker.exe',
        'prism-inference-server.exe','bonsai-gguf.exe','LICENSE','NOTICE','PRISM-LICENSE.txt','CAMELID-LICENSE.txt')
    $names=@($pin.files | ForEach-Object { $_.name })
    if ($names.Count -ne 9 -or @($names | Select-Object -Unique).Count -ne 9 -or
        @(Compare-Object -ReferenceObject $requiredNames -DifferenceObject $names -CaseSensitive).Count -ne 0) {
        throw 'The pinned runtime requires exactly five executable and four license identities.'
    }
    foreach ($file in $pin.files) {
        if ($file.bytes -le 0 -or $file.sha256 -cnotmatch '^[a-f0-9]{64}$') { throw 'Invalid pinned runtime file identity.' }
    }
    if ($bundleProof.passed -ne $true -or $bundleProof.sourceRevision -cne $expectedRuntimeRevision -or
        $bundleProof.version -cne $expectedRuntimeVersion -or $bundleProof.workflowFile -cne 'foundation-bundle.yml' -or
        $bundleProof.runConclusion -cne 'success' -or $bundleProof.finalSourceChecksPassed -ne $true -or
        $bundleProof.productionLinkingPassed -ne $true -or $bundleProof.fullFoundationQualified -ne $false -or
        $bundleProof.bundleArchiveSha256 -cne $pin.asset.sha256 -or $bundleProof.sourceArchiveVerified -ne $true -or
        $bundleProof.sourceArchiveSha256 -cne $pin.Source.Sha256 -or
        $pin.Source.Sha256 -cnotmatch '^[a-f0-9]{64}$' -or $pin.Source.Bytes -le 0 -or
        $bundleProof.sourceArchiveBytes -ne $pin.Source.Bytes -or
        $pin.Source.Repository -cne 'https://github.com/symbiogenesis/prism-inference' -or
        $pin.Source.Commit -cne $expectedRuntimeRevision -or
        $pin.Source.Url -cne ('https://github.com/symbiogenesis/prism-inference/archive/'+$expectedRuntimeRevision+'.zip')) {
        throw 'Original verified hosted bundle/source proof does not match the committed pin.'
    }
    if ($crateProof.status -cne 'passed' -or $crateProof.publicationOutcome -cne 'verified-version-and-checksum' -or
        $crateProof.sourceRevision -cne $expectedRuntimeRevision -or $crateProof.version -cne $expectedRuntimeVersion -or
        $crateProof.operatorInputSha256.'verified-bundle.json' -cne $proofIdentities[1].sha256) {
        throw 'Actual final-source registry publication is not proven for this hosted bundle.'
    }
    $publishedAssets=@($releaseProof.assets | Where-Object { $_.name -ceq $assetName })
    if ($releaseProof.tag_name -cne 'v0.1.23' -or $releaseProof.draft -ne $false -or
        $null -eq $releaseProof.prerelease -or [string]::IsNullOrWhiteSpace($releaseProof.published_at) -or
        $publishedAssets.Count -ne 1 -or $publishedAssets[0].state -cne 'uploaded' -or
        $publishedAssets[0].browser_download_url -cne $assetUrl -or $publishedAssets[0].size -ne $pin.asset.bytes -or
        $publishedAssets[0].digest -cne ('sha256:'+$pin.asset.sha256)) {
        throw 'Official published release asset is absent or differs from the committed immutable pin.'
    }
    $archivePath=$bundleProof.bundleArchivePath
    if (-not [IO.Path]::IsPathRooted($archivePath) -or -not (Test-Path -LiteralPath $archivePath -PathType Leaf) -or
        (Get-Item -LiteralPath $archivePath).Length -ne $pin.asset.bytes -or
        (Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash.ToLowerInvariant() -cne $pin.asset.sha256) {
        throw 'Already verified Foundation ZIP is absent or changed; do not download during timing setup.'
    }
    $dataDirectory='C:\Users\symbi\AppData\Local\SwiftBonsai2'
    $cacheDirectory=Join-Path $dataDirectory 'downloads'
    [void][IO.Directory]::CreateDirectory($cacheDirectory)
    foreach ($directory in @($dataDirectory,$cacheDirectory)) {
        if ((Get-Item -LiteralPath $directory).Attributes -band [IO.FileAttributes]::ReparsePoint) {
            throw 'The selected installed-data/cache directory redirects elsewhere.'
        }
    }
    $cachedArchive=Join-Path $cacheDirectory $assetName
    $cachePreviouslyExisted=Test-Path -LiteralPath $cachedArchive
    if (-not $cachePreviouslyExisted) { [IO.File]::Copy($archivePath,$cachedArchive,$false) }
    if ((Get-Item -LiteralPath $cachedArchive).Length -ne $pin.asset.bytes -or
        (Get-FileHash -LiteralPath $cachedArchive -Algorithm SHA256).Hash.ToLowerInvariant() -cne $pin.asset.sha256) {
        throw 'Cached ZIP collision preserved; contents differ from the published pin.'
    }
    $helper=Join-Path $Repository 'target/release/bonsai-tools.exe'
    if (-not (Test-Path -LiteralPath $helper -PathType Leaf)) { throw 'Trusted existing provision helper is absent; no tools rebuild is admitted.' }
    $helperIdentity=[ordered]@{path=$helper; bytes=(Get-Item -LiteralPath $helper).Length
        sha256=(Get-FileHash -LiteralPath $helper -Algorithm SHA256).Hash.ToLowerInvariant()}
    $provisionRequest=Join-Path $OutputDirectory 'provision-request.json'
    [IO.File]::WriteAllText($provisionRequest,([ordered]@{operation='inference'; dataDirectory=$dataDirectory
        lockPath=(Join-Path $OutputDirectory 'runtime-lock.json'); workingDirectory=$OutputDirectory
        includeCapabilities=$false; includeHelp=$false; verifyOnly=$false} | ConvertTo-Json),$utf8)
    # Hold the present ZIP against writes/deletion while the helper selects and
    # reads it. Its verified-cache route therefore cannot fall into a download.
    $cacheGuard=[IO.File]::Open($cachedArchive,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read)
    try {
        $session.provisioning=Run-OwnedStage 'provision' $helper @('provision','--request',$provisionRequest) 50
    } finally { $cacheGuard.Dispose() }
    if ($session.provisioning.exitCode -ne 0 -or $session.provisioning.expired) { throw 'Cache-only runtime provisioning failed; no retry or model.' }
    $installed=Get-Content -LiteralPath (Join-Path $OutputDirectory 'provision.stdout.log') -Raw -Encoding UTF8 | ConvertFrom-Json
    $installedDirectory=Join-Path (Join-Path (Join-Path $dataDirectory 'inference') $expectedRuntimeRevision) 'aarch64-pc-windows-msvc'
    if ($installed.Verified -ne $true -or $installed.Version -cne $expectedRuntimeVersion -or
        $installed.Commit -cne $expectedRuntimeRevision -or $installed.Directory -cne $installedDirectory -or
        $null -ne $installed.Capabilities -or $null -ne $installed.CapabilityVerification) {
        throw 'Explicit custom-lock provision result does not match the selected release.'
    }
    foreach ($file in $pin.files) {
        $installedPath=Join-Path $installedDirectory $file.name
        if ((Get-Item -LiteralPath $installedPath).Length -ne $file.bytes -or
            (Get-FileHash -LiteralPath $installedPath -Algorithm SHA256).Hash.ToLowerInvariant() -cne $file.sha256) {
            throw 'Installed runtime file differs from the committed pin.'
        }
    }
    if ((Get-FileHash -LiteralPath $helper -Algorithm SHA256).Hash.ToLowerInvariant() -cne $helperIdentity.sha256) {
        throw 'Provision helper changed during setup.'
    }
    $session.publicationProof=[ordered]@{version=$expectedRuntimeVersion; sourceRevision=$expectedRuntimeRevision
        pinSha256=$proofIdentities[0].sha256; proofInputs=@($proofIdentities); archivePath=$archivePath
        archiveSha256=$pin.asset.sha256; cachedArchive=$cachedArchive; cachePreviouslyExisted=$cachePreviouslyExisted
        helper=$helperIdentity; installedDirectory=$installedDirectory; verifiedInstalledFiles=9
        publishedAssetId=$publishedAssets[0].id; releasePrerelease=$releaseProof.prerelease}
    [IO.File]::WriteAllText((Join-Path $OutputDirectory 'runtime-setup.json'),($session.publicationProof | ConvertTo-Json -Depth 12),$utf8)
    Save-Session
    $env:PATH = 'C:\Users\symbi\.cargo\bin;' + $env:PATH
    $cargo='C:\Users\symbi\.cargo\bin\cargo.exe'
    $session.phase='offline-release-build'
    Save-Session
    $session.build=Run-OwnedStage 'build' $cargo @('+nightly-2026-11-09','test','--offline','--locked','--release','--manifest-path',(Join-Path $Repository 'Cargo.toml'),'--package','bonsai-broker','--lib','--test','native_model','--no-run','--message-format=json') 90
    Save-Session
    if ($session.build.exitCode -ne 0 -or $session.build.expired) { throw 'The single offline build failed or exceeded its admitted setup allowance; no retry.' }
    $artifacts=@(Get-Content -LiteralPath (Join-Path $OutputDirectory 'build.stdout.log') -Encoding UTF8 | ForEach-Object { $_ | ConvertFrom-Json } | Where-Object { $_.reason -eq 'compiler-artifact' -and $_.profile.test -eq $true -and $_.executable })
    $nativeArtifacts=@($artifacts | Where-Object { $_.target.name -eq 'native_model' -and $_.target.kind -contains 'test' })
    $libraryArtifacts=@($artifacts | Where-Object { $_.target.name -eq 'bonsai_broker' -and $_.target.kind -contains 'lib' })
    if ($nativeArtifacts.Count -ne 1 -or $libraryArtifacts.Count -ne 1) { throw 'Exactly one native_model and one broker-library test executable are required.' }
    $test=$nativeArtifacts[0].executable
    $libraryTest=$libraryArtifacts[0].executable
    $session.testExecutables=@($test,$libraryTest | ForEach-Object { [ordered]@{path=$_; bytes=(Get-Item -LiteralPath $_).Length
        sha256=(Get-FileHash -LiteralPath $_ -Algorithm SHA256).Hash.ToLowerInvariant()} })
    $session.phase='focused-synthetic-controls'
    Save-Session
    $libraryControl='inference::tests::four_row_workers_require_the_reviewed_parallel_release'
    $workerControl='native_worker_receipts_require_exact_release_and_verified_readiness'
    $session.controls+=,(Run-OwnedStage 'library-control' $libraryTest @($libraryControl,'--exact','--nocapture') 100)
    if ($session.controls[-1].exitCode -ne 0 -or $session.controls[-1].expired) { throw 'Four-worker source control failed.' }
    Require-ExactControl 'library-control' $libraryControl
    $session.controls+=,(Run-OwnedStage 'worker-control' $test @($workerControl,'--exact','--nocapture') 100)
    if ($session.controls[-1].exitCode -ne 0 -or $session.controls[-1].expired) { throw 'Strict worker-readiness source control failed.' }
    Require-ExactControl 'worker-control' $workerControl
    $session.controls+=,(Run-OwnedStage 'timing-controls' $test @($ControlFilter,'--nocapture') 100)
    Save-Session
    if ($session.controls[-1].exitCode -ne 0 -or $session.controls[-1].expired) { throw 'Focused timing controls failed; no model trial or retry.' }
    $controlOutput=Get-Content -LiteralPath (Join-Path $OutputDirectory 'timing-controls.stdout.log') -Raw -Encoding UTF8
    foreach ($requiredControl in @('token_timing::native_token_timing_requires_source_supported_boundaries_and_positive_interval','progress::native_progress_token_timing_retains_fragmented_deltas_and_cancelled_prefix')) {
        if ([regex]::Matches($controlOutput,('(?m)^test '+[regex]::Escape($requiredControl)+' \.\.\. ok\r?$')).Count -ne 1) { throw 'Required named timing control did not pass; no model trial.' }
    }
    if ($controlOutput -notmatch '(?m)^test result: ok\. 2 passed; 0 failed; 1 ignored; 0 measured;') {
        throw 'Require exactly two timing-control passes and the ignored model case before capture.'
    }
    $caseSeconds=[int][Math]::Floor($maximumSeconds-$trialClock.Elapsed.TotalSeconds-$processReserveSeconds)
    if ($caseSeconds -lt 260) { throw 'The original allowance cannot conservatively fit the declared 260 seconds for both startups, requests and cleanup.' }
    $modelEvidence=Join-Path $OutputDirectory 'models'
    [void][IO.Directory]::CreateDirectory($modelEvidence)
    $env:BONSAI_NATIVE_OWNER_TIMING='true'
    $env:BONSAI_NATIVE_PROFILE='pq2-ptq1-token-timing-row4'
    $env:BONSAI_NATIVE_OWNER_REVISION=$Revision
    $env:BONSAI_NATIVE_OWNER_RUN_ID=Split-Path -Path $OutputDirectory -Leaf
    $env:BONSAI_NATIVE_CASE_BUDGET_SECONDS=[string]$caseSeconds
    $env:BONSAI_NATIVE_WHOLE_JOB_SECONDS=[string]$maximumSeconds
    $env:BONSAI_NATIVE_DATA=$dataDirectory
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
        if ($acceptance.passed -eq $true -and $acceptance.samples.Count -eq 2 -and $acceptance.revision -eq $Revision -and
            $acceptance.runtimeLock.version -ceq $expectedRuntimeVersion -and $acceptance.runtimeLock.commit -ceq $expectedRuntimeRevision) {
            $session.status='capture-returned-two-measured-arms'
        }
    }
    $finalRevision=(& $git -C $Repository rev-parse HEAD | Out-String).Trim()
    if ($LASTEXITCODE -ne 0) { throw 'Final source revision query failed.' }
    $finalStatus=(& $git -C $Repository status --porcelain | Out-String).Trim()
    if ($LASTEXITCODE -ne 0) { throw 'Final source status query failed.' }
    $session.sourceAfter=@{revision=$finalRevision; clean=($finalStatus.Length -eq 0)}
    if ($finalRevision -ne $Revision -or $finalStatus.Length -ne 0) { throw 'Source changed during the capture.' }
    foreach ($inputFile in @($sourceInputs)+@($proofIdentities)) {
        $inputPath=if ([IO.Path]::IsPathRooted($inputFile.path)) { $inputFile.path } else { Join-Path $Repository $inputFile.path }
        if ((Get-FileHash -LiteralPath $inputPath -Algorithm SHA256).Hash.ToLowerInvariant() -cne $inputFile.sha256) {
            throw 'Bound source or publication input changed during the capture.'
        }
    }
    if ((Get-FileHash -LiteralPath $SourceDriverPath -Algorithm SHA256).Hash.ToLowerInvariant() -cne $sourceIntake.driverSha256 -or
        (Get-FileHash -LiteralPath $planPath -Algorithm SHA256).Hash.ToLowerInvariant() -cne $sourceIntake.workloadPlanSha256) {
        throw 'Committed driver or declaration bytes changed during capture.'
    }
} catch {
    $session.status='stopped'
    $session.failure=$_.Exception.Message
} finally {
    $session.phase='retention'
    $session.powerAfter=Read-Power
    Save-Session
    [void][IO.Directory]::CreateDirectory($publicDirectory)
    Copy-Item -LiteralPath $SourceDriverPath -Destination (Join-Path $publicDirectory 'capture-driver.ps1')
    $retainedPlan=Join-Path (Split-Path -Parent $SourceDriverPath) 'workload-plan.json'
    Copy-Item -LiteralPath $retainedPlan -Destination (Join-Path $publicDirectory 'workload-plan.json')
    foreach ($relative in @('session.json','source-intake.json','runtime-lock.json','verified-bundle.json','published-release.json','crate-publication-receipt.json','runtime-setup.json','provision-request.json','provision.stdout.log','provision.stderr.log','build.stdout.log','build.stderr.log','library-control.stdout.log','library-control.stderr.log','worker-control.stdout.log','worker-control.stderr.log','timing-controls.stdout.log','timing-controls.stderr.log','capture.stdout.log','capture.stderr.log','models\acceptance.json')) {
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
    $brief=[ordered]@{}
    foreach ($briefField in @('status','phase','failure','elapsedSeconds','withinAggregateAllowance','build','controls','capture')) {
        $brief[$briefField]=$session[$briefField]
    }
    Write-Output ($brief | ConvertTo-Json -Depth 8)
    Write-Output ($finalClock | ConvertTo-Json -Depth 8)
}
if ($session.status -cne 'capture-returned-two-measured-arms' -or
    $finalClock.withinAggregateAllowance -ne $true -or $null -ne $session.failure) { exit 1 }
