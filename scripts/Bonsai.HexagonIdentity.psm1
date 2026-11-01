#requires -Version 7.6
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.HexagonPolicy.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')

function Get-BonsaiHexagonDriverGuard {
    <# .SYNOPSIS
    Independently hashes and verifies an observed DriverStore DLL while retaining its bytes.
    #>
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param([Parameter(Mandatory)][string]$Path)
    $full = Resolve-ExternalPath -Path $Path
    $root = [IO.Path]::GetFullPath((Join-Path ([Environment]::SystemDirectory) 'DriverStore/FileRepository')) + [IO.Path]::DirectorySeparatorChar
    if (-not $full.StartsWith($root, [StringComparison]::OrdinalIgnoreCase) -or
        [IO.Path]::GetFileName($full) -ine 'libcdsprpc.dll') {
        throw 'The observed FastRPC driver is outside the Windows DriverStore.'
    }
    $stream = [IO.FileStream]::new($full, 'Open', 'Read', 'Read')
    try {
        if ($stream.Length -le 0 -or $stream.Length -gt 32MB) { throw 'The observed driver exceeds its byte bound.' }
        $digest = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($stream)).ToLowerInvariant()
        $signature = Get-AuthenticodeSignature -LiteralPath $full
        if ($signature.Status -ne [System.Management.Automation.SignatureStatus]::Valid -or
            $null -eq $signature.SignerCertificate -or
            $signature.SignerCertificate.Subject -notmatch '(^|,\s*)CN=Microsoft Windows Hardware Compatibility Publisher(,|$)' -or
            $signature.SignerCertificate.Thumbprint -cnotmatch '^[A-Fa-f0-9]{40,64}$') {
            throw 'The observed driver lacks the reviewed Microsoft publisher signature.'
        }
        return [pscustomobject]@{
            Stream = $stream
            Identity = [ordered]@{
                Path = $full; Bytes = $stream.Length; Sha256 = $digest
                SignatureStatus = 'Valid'; Publisher = 'Microsoft Windows Hardware Compatibility Publisher'
                SignerThumbprint = $signature.SignerCertificate.Thumbprint.ToLowerInvariant()
                FileHeldAgainstMutation = $true
            }
        }
    } catch { $stream.Dispose(); throw }
}

function Assert-BonsaiHexagonDriverIdentity {
    <# .SYNOPSIS
    Checks the schema of independently retained driver evidence without asserting provenance.
    #>
    [CmdletBinding()]
    [OutputType([void])]
    param([Parameter(Mandatory)][Collections.IDictionary]$Identity)
    $null = Invoke-BonsaiHexagonPolicy -Request @{ operation = 'driver-identity'; json = (ConvertTo-Json -InputObject $Identity -Depth 32 -Compress) }
}

Export-ModuleMember -Function Get-BonsaiHexagonDriverGuard, Assert-BonsaiHexagonDriverIdentity
