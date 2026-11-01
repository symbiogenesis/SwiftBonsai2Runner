#requires -Version 7.6
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Get-Ptq1CapturePins {
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

Export-ModuleMember -Function Get-Ptq1CapturePins
