#requires -Version 7.6
<# .SYNOPSIS
Reproduces and independently verifies every pinned PTQ1 block on hosted CI.
.DESCRIPTION
Downloads only the immutable original PQ2 artifact and pinned release. The
released GGUF exporter reproduces PTQ1; the source-bound independent scanner
checks every symbol, raw scale, metadata identity and other tensor byte. No
inference process starts. This hosted-only path consumes no owner-device budget.
Receipts retain execution, tool and file identities on both success and failure.
#>
[CmdletBinding()]
[OutputType([Collections.IDictionary])]
param([Parameter(Mandatory)][string]$DataDirectory,
      [Parameter(Mandatory)][string]$EvidenceDirectory,
      [Parameter(Mandatory)][string]$ToolPath)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Ptq1Packing.psm1')
Assert-Ptq1PackingHostedContext
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Tools.psm1')
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$data = Resolve-ExternalPath $DataDirectory
$evidence = Resolve-ExternalPath $EvidenceDirectory
$tool = Resolve-ExternalPath $ToolPath
if ([IO.Directory]::Exists($evidence)) { throw 'Use a new evidence directory for each hosted packing attempt.' }
[void][IO.Directory]::CreateDirectory($evidence)
$clock = [Diagnostics.Stopwatch]::StartNew()
$limits = Get-Ptq1PackingHostedLimit
$report = [ordered]@{
    schemaVersion = 1; kind = 'hosted-current-weight-ptq1-packing-qualification'; status = 'preparing'; phase = 'intake'
    limits = $limits; phaseStartedElapsedSeconds = 0.0; phaseDurations = [Collections.Generic.List[object]]::new()
    execution = [ordered]@{
        repository = $env:GITHUB_REPOSITORY; revision = $env:GITHUB_SHA; gitRef = $env:GITHUB_REF
        runId = $env:GITHUB_RUN_ID; runAttempt = [int]$env:GITHUB_RUN_ATTEMPT
        architecture = $env:RUNNER_ARCH; deviceModel = 'GitHub-hosted Windows ARM64'; selfHosted = $false
    }
    workflowPath = '.github/workflows/ptq1-packing-qualification.yml'
    runUrl = "https://github.com/$env:GITHUB_REPOSITORY/actions/runs/$env:GITHUB_RUN_ID/attempts/$env:GITHUB_RUN_ATTEMPT"
    modelProcessStarted = $false; ownerDeviceExecuted = $false; defaultMayChange = $false
    scope = 'Complete weight packing only. Activation, logits, live state, adapter basis, quality, resources and performance remain separate gates.'
}
$reportPath = Join-Path $evidence 'qualification.json'
Save-Ptq1PackingQualification -Report $report -Path $reportPath
try {
    $expectedPath = Join-Path $repo 'config/swift.ptq1-export.json'
    $expected = Get-Content -LiteralPath $expectedPath -Raw | ConvertFrom-Json -AsHashtable -NoEnumerate -DateKind String
    $expectedDigest = (Get-FileHash -LiteralPath $expectedPath -Algorithm SHA256).Hash.ToLowerInvariant()
    Copy-Item -LiteralPath $expectedPath -Destination (Join-Path $evidence 'expected-export.json')
    $assessment = Read-BonsaiLock (Join-Path $repo 'config/swift-assessment.lock.json')
    $runtimeLock = Get-Content -LiteralPath (Join-Path $repo 'config/prism-inference.lock.json') -Raw | ConvertFrom-Json -AsHashtable -DateKind String
    $report['expectedExportReceiptSha256'] = $expectedDigest
    Assert-Arm64PE $tool
    $report['maintenanceTool'] = @{
        sha256 = (Get-FileHash -LiteralPath $tool -Algorithm SHA256).Hash.ToLowerInvariant()
        bytes = (Get-Item -LiteralPath $tool).Length; sourceId = $env:BONSAI_TOOLS_SOURCE_ID
    }
    $report['exporterRuntime'] = @{
        revision = $runtimeLock.commit; archiveSha256 = $runtimeLock.asset.sha256
        gguf = @($runtimeLock.files | Where-Object name -CEQ 'bonsai-gguf.exe')
    }
    if ($assessment.model.sha256 -cne $expected.SourceSha256 -or $assessment.model.bytes -ne $expected.SourceBytes -or
        $assessment.model.revision -cne $expected.SourceRevision -or $assessment.model.repository -cne $expected.SourceRepository) {
        throw 'The complete export receipt differs from the current original PQ2 identity.'
    }
    [void][IO.Directory]::CreateDirectory($data)
    $requiredDisk = [long]$expected.SourceBytes + [long]$expected.OutputBytes + 536870912
    if ([IO.DriveInfo]::new([IO.Path]::GetPathRoot($data)).AvailableFreeSpace -lt $requiredDisk) {
        throw 'Hosted packing qualification needs room for both complete artifacts and 512 MiB reserve.'
    }
    Set-Ptq1PackingPhase -Report $report -Clock $clock -Phase 'original-pq2-download'
    Save-Ptq1PackingQualification -Report $report -Path $reportPath
    $source = Join-Path $data $expected.SourceFilename
    $sourceUri = "https://huggingface.co/$($assessment.model.repository)/resolve/$($assessment.model.revision)/$($assessment.model.filename)"
    Receive-LockedArtifact -Uri $sourceUri -Path $source -Bytes $expected.SourceBytes -Sha256 $expected.SourceSha256 -TimeoutSeconds $limits.originalDownloadSeconds | Out-Null
    Set-Ptq1PackingPhase -Report $report -Clock $clock -Phase 'pinned-exporter-provisioning'
    Save-Ptq1PackingQualification -Report $report -Path $reportPath
    $archive = Join-Path $data ('downloads/' + $runtimeLock.asset.filename)
    Receive-LockedArtifact -Uri $runtimeLock.asset.url -Path $archive -Bytes $runtimeLock.asset.bytes -Sha256 $runtimeLock.asset.sha256 -TimeoutSeconds $limits.runtimeDownloadSeconds | Out-Null
    $runtime = & (Join-Path $PSScriptRoot 'Get-Inference.ps1') -DataDirectory $data
    if (-not $runtime.Gguf) { throw 'The pinned release has no GGUF exporter.' }
    Set-Ptq1PackingPhase -Report $report -Clock $clock -Phase 'ptq1-reproduction'
    Save-Ptq1PackingQualification -Report $report -Path $reportPath
    $output = Join-Path $data $expected.OutputFilename
    $export = & (Join-Path $PSScriptRoot 'Export-SwiftPtq1.ps1') -ModelPath $source -OutputPath $output -GgufToolPath $runtime.Gguf -TimeoutSeconds $limits.exporterProcessSeconds
    $export | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $evidence 'reproduced-export.json') -Encoding utf8NoBOM
    Set-Ptq1PackingPhase -Report $report -Clock $clock -Phase 'independent-complete-packing'
    Save-Ptq1PackingQualification -Report $report -Path $reportPath
    $packing = Invoke-BonsaiTool -Command ptq1-packing -ToolPath $tool -TimeoutSeconds $limits.independentProcessSeconds -Request @{
        sourcePath = $source; outputPath = $output; timeoutSeconds = $limits.independentVerificationSeconds
    }
    $packing | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $evidence 'independent-packing.json') -Encoding utf8NoBOM
    Assert-Ptq1PackingResult -Expected $expected -Export $export -Packing $packing -ExportReceiptSha256 $expectedDigest
    $report.status = 'matched'
} catch {
    $report.status = 'failed'
    $report['failure'] = 'The selected hosted packing phase failed; inspect the phase and workflow log. No promotion is inferred.'
    try {
        if ($report.phase -ceq 'original-pq2-download') {
            $report['partialDownload'] = Get-Ptq1PackingPartialObservation -Path ($source + '.partial') -ExpectedBytes $expected.SourceBytes
        } elseif ($report.phase -ceq 'pinned-exporter-provisioning') {
            $report['partialDownload'] = Get-Ptq1PackingPartialObservation -Path ($archive + '.partial') -ExpectedBytes $runtimeLock.asset.bytes
        }
    } catch { $report['partialDownload'] = @{ status = 'unavailable' } }
    throw
} finally {
    $finalPhase = if ($report.status -ceq 'matched') { 'complete' } else { $report.phase }
    Set-Ptq1PackingPhase -Report $report -Clock $clock -Phase $finalPhase
    $report['elapsedSeconds'] = $clock.Elapsed.TotalSeconds
    Save-Ptq1PackingQualification -Report $report -Path $reportPath
    $files = @(Get-ChildItem -LiteralPath $evidence -File | Sort-Object Name | ForEach-Object {
        @{ file = $_.Name; bytes = $_.Length; sha256 = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant() }
    })
    @{ schemaVersion = 1; revision = $env:GITHUB_SHA; runId = $env:GITHUB_RUN_ID; files = $files } |
        ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $evidence 'manifest.json') -Encoding utf8NoBOM
}
return $report
