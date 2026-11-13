#Requires -Version 7.6
# Offline asset-contract fixtures. The inert .exe bytes are never executed.
[CmdletBinding()]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'Release-Common.ps1')
. (Join-Path $PSScriptRoot 'Release-Assets.ps1')
$fixture = Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-Asset-Controls-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($fixture)
$tag = 'v0.2.0'
$source = 'a' * 40
$name = 'SwiftBonsai2Runner-0.2.0-windows-arm64.exe'
$executable = Join-Path $fixture $name
$manifestPath = Join-Path $fixture 'release-manifest.json'
$checksumPath = Join-Path $fixture 'SHA256SUMS.txt'
$bytes = [Text.UTF8Encoding]::new($false).GetBytes('Inert portable asset fixture; never execute.')
function New-AssetManifest {
    $files = [ordered]@{
        maintenance = 'bonsai-tools.exe'; core = 'bonsai-broker.exe'; ui = 'bonsai-chat.exe'
        asr = 'bonsai-speech-asr.exe'; tts = 'bonsai-speech-tts.exe'
    }
    $builds = @($files.Keys | ForEach-Object {
        @{ schemaVersion = 1; sourceRevision = $source; target = 'aarch64-pc-windows-msvc'
            lane = $_; file = $files[$_]; bytes = 1; sha256 = ('c' * 64) }
    })
    @{
        schemaVersion = 1; distribution = 'portable-executable'
        version = '0.2.0'; tag = $tag; sourceRevision = $source; target = 'aarch64-pc-windows-msvc'
        executableSmoke = 'passed-inspect-extract-verify-without-model'; signed = $false
        manifestSha256 = ('b' * 64); builds = $builds
        assets = @(@{ name = $name; bytes = $bytes.Length
            sha256 = (Get-FileHash -LiteralPath $executable -Algorithm SHA256).Hash.ToLowerInvariant() })
    }
}
function Write-AssetFixture($Manifest) {
    $Manifest | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $manifestPath -Encoding utf8NoBOM
    $records = @($executable, $manifestPath) | ForEach-Object {
        '{0}  {1}' -f (Get-FileHash -LiteralPath $_ -Algorithm SHA256).Hash.ToLowerInvariant(), [IO.Path]::GetFileName($_)
    }
    [IO.File]::WriteAllLines($checksumPath, $records, [Text.UTF8Encoding]::new($false))
}
function Assert-RejectedAssets {
    try { $null = Assert-PortableReleaseAssets -AssetDirectory $fixture -Tag $tag -SourceRevision $source }
    catch { return }
    throw 'An invalid portable release inventory was accepted.'
}
try {
    [IO.File]::WriteAllBytes($executable, $bytes)
    Write-AssetFixture (New-AssetManifest)
    $accepted = Assert-PortableReleaseAssets -AssetDirectory $fixture -Tag $tag -SourceRevision $source
    if ($accepted.distribution -cne 'portable-executable' -or @($accepted.assets).Count -ne 1) { throw 'The valid portable inventory changed.' }
    $rejected = 0
    foreach ($extension in @('msi', 'zip')) {
        $extra = Join-Path $fixture "SwiftBonsai2Runner-0.2.0-windows-arm64.$extension"
        [IO.File]::WriteAllBytes($extra, $bytes)
        try { Assert-RejectedAssets; $rejected++ }
        finally { Remove-Item -LiteralPath $extra -Force }
    }
    foreach ($change in @(
        { param($m) $m.distribution = 'msi' }
        { param($m) $m.sourceRevision = ('d' * 40) }
        { param($m) $m.assets[0].name = 'SwiftBonsai2Runner-0.2.0-windows-arm64.msi' }
        { param($m) $m.assets[0].bytes++ }
        { param($m) $m.builds[0].sourceRevision = ('e' * 40) }
        { param($m) $m.builds[0].lane = 'Maintenance' }
        { param($m) $m.builds[1].lane = $m.builds[0].lane }
    )) {
        $manifest = New-AssetManifest
        & $change $manifest
        Write-AssetFixture $manifest
        Assert-RejectedAssets
        $rejected++
    }
    Write-AssetFixture (New-AssetManifest)
    [IO.File]::WriteAllBytes($executable, [byte[]]@(1, 2, 3))
    Assert-RejectedAssets
    $rejected++
    [IO.File]::WriteAllBytes($executable, $bytes)
    Write-AssetFixture (New-AssetManifest)
    Add-Content -LiteralPath $checksumPath -Value (Get-Content -LiteralPath $checksumPath -First 1) -Encoding utf8NoBOM
    Assert-RejectedAssets
    $rejected++
    Write-Host "Portable release asset controls passed: exact three-file inventory and $rejected source, checksum and retired-asset rejections."
} finally {
    Remove-Item -LiteralPath $executable, $manifestPath, $checksumPath -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $fixture -Force
}
