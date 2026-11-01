#requires -Version 7.6
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true

function Assert-Ptq1PackingHostedContext {
    <# .SYNOPSIS
    Rejects owner-device and unbound CI execution before any artifact work.
    #>
    [CmdletBinding()]
    [OutputType([void])]
    param()
    if (-not $IsWindows -or [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture -ne 'Arm64' -or
        $env:GITHUB_ACTIONS -cne 'true' -or $env:RUNNER_ENVIRONMENT -cne 'github-hosted' -or
        $env:GITHUB_REPOSITORY -cne 'symbiogenesis/SwiftBonsai2Runner' -or $env:GITHUB_REF -cne 'refs/heads/main' -or
        $env:RUNNER_ARCH -cne 'ARM64' -or $env:GITHUB_SHA -cnotmatch '^[a-f0-9]{40}$' -or
        $env:GITHUB_RUN_ID -cnotmatch '^[1-9][0-9]*$' -or $env:GITHUB_RUN_ATTEMPT -cnotmatch '^[1-9][0-9]*$' -or
        $env:BONSAI_TOOLS_SOURCE_ID -cnotmatch '^[a-f0-9]{64}$') {
        throw 'PTQ1 packing qualification requires exact-main GitHub-hosted native Windows ARM64 CI; owner-device execution is disabled.'
    }
}

function Assert-Ptq1PackingResult {
    <# .SYNOPSIS
    Binds independently decoded packing and reproduced export receipts to the complete pin.
    #>
    [CmdletBinding()]
    [OutputType([void])]
    param([Parameter(Mandatory)][Collections.IDictionary]$Expected,
          [Parameter(Mandatory)][Collections.IDictionary]$Export,
          [Parameter(Mandatory)][Collections.IDictionary]$Packing,
          [Parameter(Mandatory)][string]$ExportReceiptSha256)
    foreach ($name in @('SchemaVersion', 'CodecCommit', 'SourceBytes', 'OutputBytes', 'SourceSha256', 'OutputSha256',
        'SourceRevision', 'ConvertedTensorCount', 'ConvertedBlocks', 'ExactPq2ReconstructionVerified',
        'NonPq2TensorBytesCopiedVerbatim', 'TokenizerMetadataSha256')) {
        if (-not $Export.Contains($name) -or
            ($Export[$name] | ConvertTo-Json -Compress) -cne ($Expected[$name] | ConvertTo-Json -Compress)) {
            throw "The reproduced PTQ1 export differs from its committed receipt: $name."
        }
    }
    $required = [ordered]@{
        schemaVersion = 1; kind = 'independent-current-weight-ptq1-packing'; status = 'matched'
        sourceBytes = $Expected.SourceBytes; sourceSha256 = $Expected.SourceSha256
        outputBytes = $Expected.OutputBytes; outputSha256 = $Expected.OutputSha256
        convertedTensors = $Expected.ConvertedTensorCount; reconstructedBlocks = $Expected.ConvertedBlocks
        exportReceiptSha256 = $ExportReceiptSha256; headerBytesPreservedExceptDeclaredEdits = $true
        unchangedOtherTensorBytes = 57767936; workingPayloadBufferBytes = 131072
    }
    if ($ExportReceiptSha256 -cnotmatch '^[a-f0-9]{64}$') { throw 'A complete export receipt digest is required.' }
    foreach ($name in $required.Keys) {
        if (-not $Packing.Contains($name) -or
            ($Packing[$name] | ConvertTo-Json -Compress) -cne ($required[$name] | ConvertTo-Json -Compress)) {
            throw "Independent PTQ1 packing differs from its complete committed inventory: $name."
        }
    }
}

function Save-Ptq1PackingQualification {
    <# .SYNOPSIS
    Retains the latest bounded credential-free packing phase before external work.
    #>
    [CmdletBinding()]
    [OutputType([void])]
    param([Parameter(Mandatory)][Collections.IDictionary]$Report,
          [Parameter(Mandatory)][string]$Path)
    $bytes = [Text.UTF8Encoding]::new($false).GetBytes((ConvertTo-Json -InputObject $Report -Depth 12))
    if ($bytes.Length -gt 1048576) { throw 'Packing qualification receipt exceeds one MiB.' }
    $temporary = $Path + '.' + [guid]::NewGuid().ToString('N') + '.tmp'
    try {
        $stream = [IO.FileStream]::new($temporary, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
        try {
            $stream.Write($bytes, 0, $bytes.Length)
            $stream.Flush($true)
        } finally { $stream.Dispose() }
        [IO.File]::Move($temporary, $Path, $true)
    } finally {
        if ([IO.File]::Exists($temporary)) { [IO.File]::Delete($temporary) }
    }
}

Export-ModuleMember -Function Assert-Ptq1PackingHostedContext, Assert-Ptq1PackingResult, Save-Ptq1PackingQualification
