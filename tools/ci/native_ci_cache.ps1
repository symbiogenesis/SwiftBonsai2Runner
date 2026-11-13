#Requires -Version 7.6
<# Verify exact hosted inputs and native executable bytes before selecting a cached CI tool. #>
[CmdletBinding()]
param([ValidateSet('create','verify')][string]$Operation='verify', [string]$PackageDirectory, [string]$SourceId, [string]$HostId, [string]$Compiler)
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
function Get-CiExecutableDigest([string]$Path) {
    $stream=[IO.File]::OpenRead($Path)
    try {
        $reader=[IO.BinaryReader]::new($stream,[Text.Encoding]::UTF8,$true)
        if ($stream.Length -lt 90 -or $reader.ReadUInt16() -ne 0x5a4d) { throw 'The CI bootstrap is not a PE executable.' }
        $stream.Position=60; $offset=$reader.ReadUInt32()
        if ($offset -lt 64 -or $offset -gt $stream.Length-26) { throw 'The CI bootstrap has an invalid PE header offset.' }
        $stream.Position=$offset
        if ($reader.ReadUInt32() -ne 0x4550 -or $reader.ReadUInt16() -ne 0xaa64) { throw 'The CI bootstrap must be a native Windows ARM64 PE32+ image.' }
        $stream.Position=$offset+22; $characteristics=$reader.ReadUInt16(); $magic=$reader.ReadUInt16()
        if ($magic -ne 0x20b) { throw 'The CI bootstrap must be a native Windows ARM64 PE32+ image.' }
        if (-not ($characteristics -band 2) -or ($characteristics -band 0x2000)) { throw 'The CI bootstrap must be an executable, not a DLL.' }
        $stream.Position=0; return [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($stream)).ToLowerInvariant()
    } finally { $stream.Dispose() }
}
function Read-CiCacheReceipt([string]$Path) {
    $raw=[IO.File]::ReadAllText($Path,[Text.UTF8Encoding]::new($false,$true))
    $document=[Text.Json.JsonDocument]::Parse($raw); $fields=[Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    try {
        if ($document.RootElement.ValueKind -ne [Text.Json.JsonValueKind]::Object) { throw 'The CI bootstrap receipt has unexpected fields.' }
        foreach ($property in $document.RootElement.EnumerateObject()) { if (-not $fields.Add($property.Name)) { throw "Duplicate receipt field: $($property.Name)" } }
        if ($fields.Count -ne 6 -or @($fields | Where-Object { $_ -cnotin @('schemaVersion','sourceId','hostId','toolchain','compiler','sha256') }).Count) { throw 'The CI bootstrap receipt has unexpected fields.' }
        if ($document.RootElement.GetProperty('schemaVersion').ValueKind -ne [Text.Json.JsonValueKind]::Number -or $document.RootElement.GetProperty('schemaVersion').GetRawText() -cne '1') { throw 'The CI bootstrap receipt has different build inputs.' }
    } finally { $document.Dispose() }
    return $raw | ConvertFrom-Json -AsHashtable
}
function Test-CiCache([string]$Package, [string]$Source, [string]$HostIdentity) {
    if ($Source -cnotmatch '^[a-f0-9]{64}$') { throw 'An exact SHA-256 source identity is required.' }
    if (-not $HostIdentity -or $HostIdentity.Trim() -cne $HostIdentity -or $HostIdentity.IndexOfAny([char[]]"`r`n`0") -ge 0) { throw 'An exact hosted image/OS identity is required.' }
    $receipt=Read-CiCacheReceipt (Join-Path $Package 'receipt.json')
    if ($receipt.sourceId -cne $Source -or $receipt.hostId -cne $HostIdentity -or $receipt.toolchain -cne 'nightly-2026-11-12-aarch64-pc-windows-msvc') { throw 'The CI bootstrap receipt has different build inputs.' }
    if ($receipt.compiler -isnot [string] -or 'host: aarch64-pc-windows-msvc' -cnotin ($receipt.compiler -split '\r?\n')) { throw 'The CI bootstrap compiler identity has a different native host.' }
    if ($receipt.sha256 -isnot [string] -or $receipt.sha256 -cnotmatch '^[a-f0-9]{64}$') { throw 'The CI bootstrap receipt has an invalid executable digest.' }
    $executable=Join-Path $Package 'bonsai-ci.exe'
    if ((Get-CiExecutableDigest $executable) -cne $receipt.sha256) { throw 'The CI bootstrap executable digest differs from its receipt.' }
    return $executable
}
function New-CiCacheReceipt([string]$Package, [string]$Source, [string]$HostIdentity, [string]$CompilerIdentity) {
    if ($Source -cnotmatch '^[a-f0-9]{64}$' -or -not $HostIdentity -or $HostIdentity.Trim() -cne $HostIdentity -or $HostIdentity.IndexOfAny([char[]]"`r`n`0") -ge 0) { throw 'Exact source and hosted image identities are required.' }
    $receipt=@{schemaVersion=1; sourceId=$Source; hostId=$HostIdentity; toolchain='nightly-2026-11-12-aarch64-pc-windows-msvc'; compiler=$CompilerIdentity; sha256=(Get-CiExecutableDigest (Join-Path $Package 'bonsai-ci.exe'))}
    [IO.File]::WriteAllText((Join-Path $Package 'receipt.json'),($receipt | ConvertTo-Json)+"`n")
    return Test-CiCache $Package $Source $HostIdentity
}
if ($MyInvocation.InvocationName -ne '.') {
    try {
        if ($Operation -ceq 'create') { if (-not $Compiler) { throw 'Receipt creation requires the native compiler identity.' }; New-CiCacheReceipt $PackageDirectory $SourceId $HostId ([IO.File]::ReadAllText($Compiler)) }
        else { Test-CiCache $PackageDirectory $SourceId $HostId }
    } catch { [Console]::Error.WriteLine("CI bootstrap verification failed: $_"); exit 1 }
}
