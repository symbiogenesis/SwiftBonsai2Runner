#requires -Version 7.6
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true

function Get-Ptq1CapturePin {
    <# .SYNOPSIS
    Requires numerical captures to test the exact currently pinned Rust source.
    #>
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param([Parameter(Mandatory)][string]$RepositoryDirectory)
    $pins = Get-Content -LiteralPath (Join-Path $RepositoryDirectory 'tools/native-reference/pins.json') -Raw | ConvertFrom-Json -DateKind String
    $runtime = Get-Content -LiteralPath (Join-Path $RepositoryDirectory 'config/prism-inference.lock.json') -Raw | ConvertFrom-Json -DateKind String
    if ($pins.rustSource.commit -cne $runtime.commit -or
        $pins.rustSource.commit -cne $runtime.Source.Commit -or
        $pins.rustSource.url -cne $runtime.Source.Url -or
        $pins.rustSource.bytes -ne $runtime.Source.Bytes -or
        $pins.rustSource.sha256 -cne $runtime.Source.Sha256) {
        throw 'PTQ1 numerical source differs from the pinned Rust runtime; update and review the capture pins first.'
    }
    return $pins
}

function Save-Ptq1NumericsReport {
    <# .SYNOPSIS
    Flushes bounded partial observations before replacing the prior numerical receipt.
    #>
    [CmdletBinding()]
    [OutputType([void])]
    param([Parameter(Mandatory)][Collections.IDictionary]$Report,
          [Parameter(Mandatory)][string]$Path)
    $bytes = [Text.UTF8Encoding]::new($false).GetBytes(($Report | ConvertTo-Json -Depth 40) + "`n")
    if ($bytes.Length -gt 1048576) { throw 'PTQ1 numerical receipt exceeds its fixed byte bound.' }
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

Export-ModuleMember -Function Get-Ptq1CapturePin, Save-Ptq1NumericsReport
