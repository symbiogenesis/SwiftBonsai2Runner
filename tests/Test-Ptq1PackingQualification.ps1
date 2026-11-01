#requires -Version 7.6
<# .SYNOPSIS
Checks hosted-only packing admission, complete receipt binding and failure retention.
#>
[CmdletBinding()]
[OutputType([string])]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.TestSupport.psm1')
Import-Module (Join-Path $PSScriptRoot '../scripts/Bonsai.Ptq1Packing.psm1')
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$expectedPath = Join-Path $repo 'config/swift.ptq1-export.json'
$expected = Get-Content -LiteralPath $expectedPath -Raw | ConvertFrom-Json -AsHashtable -DateKind String
$digest = (Get-FileHash -LiteralPath $expectedPath -Algorithm SHA256).Hash.ToLowerInvariant()
# Match the complete CLI receipt shape, rather than passing the curated pin as
# an exporter result. At pinned Prism 749a5f5, tools/gguf/src/export.rs emits
# schema/codec/extents/inventory and tools/gguf/src/lib.rs adds verified
# SourceSha256, inspected TokenizerMetadataSha256 and OutputPath. Runner's
# Export-SwiftPtq1.ps1 then adds SourceRevision. The curated pin's filename,
# repository, saving and model-execution annotations are not exporter fields.
$export = [ordered]@{
    SchemaVersion = 1; CodecCommit = $expected.CodecCommit
    SourceBytes = $expected.SourceBytes; OutputBytes = $expected.OutputBytes; OutputSha256 = $expected.OutputSha256
    ConvertedTensorCount = $expected.ConvertedTensorCount; ConvertedBlocks = $expected.ConvertedBlocks
    ExactPq2ReconstructionVerified = $true; NonPq2TensorBytesCopiedVerbatim = $true
    MetadataChanges = $expected.MetadataChanges; WorkingBufferBytes = $expected.WorkingBufferBytes
    Qualification = $expected.Qualification; SourceSha256 = $expected.SourceSha256
    TokenizerMetadataSha256 = $expected.TokenizerMetadataSha256
    OutputPath = Join-Path ([IO.Path]::GetTempPath()) $expected.OutputFilename
    SourceRevision = $expected.SourceRevision
}
$packing = [ordered]@{
    schemaVersion = 1; kind = 'independent-current-weight-ptq1-packing'; status = 'matched'
    sourceBytes = $expected.SourceBytes; sourceSha256 = $expected.SourceSha256
    outputBytes = $expected.OutputBytes; outputSha256 = $expected.OutputSha256
    convertedTensors = $expected.ConvertedTensorCount; reconstructedBlocks = $expected.ConvertedBlocks
    exportReceiptSha256 = $digest; headerBytesPreservedExceptDeclaredEdits = $true
    unchangedOtherTensorBytes = 57767936; workingPayloadBufferBytes = 131072
}
Assert-Ptq1PackingResult -Expected $expected -Export $export -Packing $packing -ExportReceiptSha256 $digest
foreach ($name in @($packing.Keys)) {
    $changed = $packing | ConvertTo-Json | ConvertFrom-Json -AsHashtable -DateKind String
    $changed[$name] = 'unbound'
    Assert-Failure { Assert-Ptq1PackingResult -Expected $expected -Export $export -Packing $changed -ExportReceiptSha256 $digest } "Changed independent packing field was accepted: $name."
    [void]$changed.Remove($name)
    Assert-Failure { Assert-Ptq1PackingResult -Expected $expected -Export $export -Packing $changed -ExportReceiptSha256 $digest } "Missing independent packing field was accepted: $name."
}
foreach ($name in @('SchemaVersion', 'CodecCommit', 'SourceBytes', 'OutputBytes', 'SourceSha256', 'OutputSha256',
    'SourceRevision', 'ConvertedTensorCount', 'ConvertedBlocks', 'ExactPq2ReconstructionVerified',
    'NonPq2TensorBytesCopiedVerbatim', 'TokenizerMetadataSha256')) {
    $changed = $export | ConvertTo-Json | ConvertFrom-Json -AsHashtable -DateKind String
    $changed[$name] = 'unbound'
    Assert-Failure { Assert-Ptq1PackingResult -Expected $expected -Export $changed -Packing $packing -ExportReceiptSha256 $digest } "Changed export field was accepted: $name."
    [void]$changed.Remove($name)
    Assert-Failure { Assert-Ptq1PackingResult -Expected $expected -Export $changed -Packing $packing -ExportReceiptSha256 $digest } "Missing export field was accepted: $name."
}
$falseBoolean = $packing | ConvertTo-Json | ConvertFrom-Json -AsHashtable -DateKind String
$falseBoolean.headerBytesPreservedExceptDeclaredEdits = 'true'
Assert-Failure { Assert-Ptq1PackingResult -Expected $expected -Export $export -Packing $falseBoolean -ExportReceiptSha256 $digest } 'String-valued success bypassed independent packing checks.'
Assert-Failure { Assert-Ptq1PackingResult -Expected $expected -Export $export -Packing $packing -ExportReceiptSha256 'missing' }

# The published exporter verifies source/output outside its internal codec
# deadline. Exercise the actual process timeout through the GGUF wrapper, then
# verify Export-SwiftPtq1 forwards its selected bound to that wrapper.
Import-Module (Join-Path $repo 'scripts/Bonsai.Gguf.psm1')
$failure = Assert-Failure -PassThru {
    Invoke-GgufTool -GgufToolPath ([Environment]::ProcessPath) -TimeoutSeconds 1 -Arguments @(
        '-NoLogo', '-NoProfile', '-NonInteractive', '-Command', 'Start-Sleep -Seconds 5; [Console]::Write("{}")')
}
Assert ($failure -like '*Native process exceeded 1 seconds:*') 'The whole GGUF process failed for a reason other than its selected timeout.'
$unbounded = Invoke-GgufTool -GgufToolPath ([Environment]::ProcessPath) -Arguments @(
    '-NoLogo', '-NoProfile', '-NonInteractive', '-Command', '[Console]::Write("{}")')
Assert ($unbounded.Count -eq 0) 'Omitting the optional GGUF process timeout changed ordinary JSON tool behavior.'
$parseTokens = $null
$parseErrors = $null
$exportAst = [Management.Automation.Language.Parser]::ParseFile((Join-Path $repo 'scripts/Export-SwiftPtq1.ps1'), [ref]$parseTokens, [ref]$parseErrors)
$invoke = @($exportAst.FindAll({
    param([Management.Automation.Language.Ast]$Node)
    $Node -is [Management.Automation.Language.CommandAst] -and $Node.GetCommandName() -ceq 'Invoke-GgufTool'
}, $true))
Assert ($parseErrors.Count -eq 0 -and $invoke.Count -eq 1) 'The exporter timeout source control did not identify its one tool invocation.'
$forwarded = $false
for ($index = 0; $index -lt $invoke[0].CommandElements.Count - 1; $index++) {
    $element = $invoke[0].CommandElements[$index]
    if ($element -is [Management.Automation.Language.CommandParameterAst] -and $element.ParameterName -ceq 'TimeoutSeconds') {
        $forwarded = $invoke[0].CommandElements[$index + 1].Extent.Text -ceq '$TimeoutSeconds'
    }
}
Assert $forwarded 'The export caller omitted its whole-process bound, leaving source/output verification unbounded.'

$baseline = [ordered]@{
    GITHUB_ACTIONS = 'true'; RUNNER_ENVIRONMENT = 'github-hosted'; GITHUB_REPOSITORY = 'symbiogenesis/SwiftBonsai2Runner'
    GITHUB_REF = 'refs/heads/main'; RUNNER_ARCH = 'ARM64'; GITHUB_SHA = 'a' * 40
    GITHUB_RUN_ID = '123'; GITHUB_RUN_ATTEMPT = '1'; BONSAI_TOOLS_SOURCE_ID = 'b' * 64
}
$saved = @{}
foreach ($name in $baseline.Keys) { $saved[$name] = [Environment]::GetEnvironmentVariable($name) }
$tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
$fixture = [IO.Path]::GetFullPath((Join-Path $tempRoot ('SwiftBonsai-Ptq1PackingControls-' + [guid]::NewGuid().ToString('N'))))
try {
    foreach ($name in $baseline.Keys) { [Environment]::SetEnvironmentVariable($name, $baseline[$name]) }
    Assert-Ptq1PackingHostedContext
    foreach ($name in $baseline.Keys) {
        [Environment]::SetEnvironmentVariable($name, 'unbound')
        $failure = Assert-Failure -PassThru { Assert-Ptq1PackingHostedContext }
        Assert ($failure -like '*GitHub-hosted*owner-device execution is disabled*') "Invalid hosted context was accepted: $name."
        [Environment]::SetEnvironmentVariable($name, $baseline[$name])
    }
    $evidence = Join-Path $fixture 'evidence'
    Assert-Failure {
        & (Join-Path $repo 'scripts/Invoke-Ptq1PackingQualification.ps1') -DataDirectory (Join-Path $fixture 'data') -EvidenceDirectory $evidence -ToolPath (Join-Path $fixture 'absent.exe')
    } 'Missing tool unexpectedly reached download or packing.'
    $failed = Get-Content -LiteralPath (Join-Path $evidence 'qualification.json') -Raw | ConvertFrom-Json -DateKind String
    Assert ($failed.status -ceq 'failed' -and $failed.phase -ceq 'intake' -and
        -not $failed.modelProcessStarted -and -not $failed.ownerDeviceExecuted -and -not $failed.defaultMayChange -and
        $failed.execution.revision -ceq $baseline.GITHUB_SHA -and $failed.execution.runId -ceq $baseline.GITHUB_RUN_ID) 'Pre-download failure lost its exact execution or qualification boundary.'
    $manifest = Get-Content -LiteralPath (Join-Path $evidence 'manifest.json') -Raw | ConvertFrom-Json -DateKind String
    foreach ($file in $manifest.files) {
        $path = Join-Path $evidence $file.file
        Assert ($file.bytes -eq (Get-Item -LiteralPath $path).Length -and
            $file.sha256 -ceq (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()) 'Failed packing attempt retained an unbound evidence file.'
    }
    $reportPath = Join-Path $evidence 'qualification.json'
    $retained = [IO.File]::ReadAllText($reportPath)
    Assert-Failure { Save-Ptq1PackingQualification -Report @{ oversized = 'x' * 1048577 } -Path $reportPath }
    Assert ([IO.File]::ReadAllText($reportPath) -ceq $retained) 'An oversized update replaced the last complete packing receipt.'
    Assert-Failure { Save-Ptq1PackingQualification -Report @{ status = 'failed' } -Path $evidence }
    Assert (@(Get-ChildItem -LiteralPath $fixture -Filter '*.tmp').Count -eq 0) 'Failed receipt replacement left a temporary file.'
} finally {
    foreach ($name in $baseline.Keys) { [Environment]::SetEnvironmentVariable($name, $saved[$name]) }
    if (-not $fixture.StartsWith($tempRoot, [StringComparison]::OrdinalIgnoreCase)) { throw 'Packing-control cleanup escaped its temporary root.' }
    if ([IO.Directory]::Exists($fixture)) { Remove-Item -LiteralPath $fixture -Recurse -Force }
}
'Hosted PTQ1 packing context, complete receipts and failure retention checks passed.'
