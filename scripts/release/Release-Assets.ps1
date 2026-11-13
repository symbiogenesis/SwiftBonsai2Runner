#Requires -Version 7.6
# Exact portable release inventory shared by packaging and publication.
Set-StrictMode -Version Latest

function Get-PortableReleaseAssetNames([string]$Version) {
    @("SwiftBonsai2Runner-$Version-windows-arm64.exe", 'release-manifest.json', 'SHA256SUMS.txt')
}

function Assert-PortableReleaseAssets {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$AssetDirectory,
        [Parameter(Mandatory)][string]$Tag,
        [Parameter(Mandatory)][string]$SourceRevision
    )
    if ($Tag -cnotmatch '^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$' -or $SourceRevision -cnotmatch '^[0-9a-f]{40}$') { throw 'Invalid release identity.' }
    $AssetDirectory = Resolve-ReleasePath $AssetDirectory
    $version = $Tag.Substring(1)
    $names = @(Get-PortableReleaseAssetNames $version)
    $children = @(Get-ChildItem -LiteralPath $AssetDirectory -Force)
    if ($children.Count -ne $names.Count) { throw 'Release asset inventory differs from the portable inventory.' }
    foreach ($file in $children) {
        if ($file.PSIsContainer -or $file.Name -cnotin $names -or ($file.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Unexpected or redirected release asset.' }
    }
    $checksums = @{}
    foreach ($line in Get-Content -LiteralPath (Join-Path $AssetDirectory 'SHA256SUMS.txt')) {
        if ($line -cnotmatch '^([a-f0-9]{64})  ([A-Za-z0-9._-]+)$') { throw 'Invalid release checksum record.' }
        $digest = $Matches[1]; $name = $Matches[2]
        if ($name -ceq 'SHA256SUMS.txt' -or $name -cnotin $names -or $checksums.ContainsKey($name)) { throw 'Unexpected or duplicate checksum record.' }
        $actual = (Get-FileHash -LiteralPath (Join-Path $AssetDirectory $name) -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($actual -cne $digest) { throw "Release checksum differs: $name" }
        $checksums[$name] = $digest
    }
    if ($checksums.Count -ne ($names.Count - 1)) { throw 'Release checksums are incomplete.' }
    $manifest = Get-Content -LiteralPath (Join-Path $AssetDirectory 'release-manifest.json') -Raw | ConvertFrom-Json
    if ($manifest.schemaVersion -ne 1 -or $manifest.distribution -cne 'portable-executable' -or
        $manifest.tag -cne $Tag -or $manifest.version -cne $version -or
        $manifest.sourceRevision -cne $SourceRevision -or $manifest.target -cne 'aarch64-pc-windows-msvc' -or
        $manifest.executableSmoke -cne 'passed-inspect-extract-verify-without-model' -or $manifest.signed -ne $false) { throw 'Portable release manifest source, architecture or checks differ.' }
    if (@($manifest.assets).Count -ne 1) { throw 'Portable release must contain exactly one application executable.' }
    $asset = $manifest.assets[0]
    $path = Join-Path $AssetDirectory $names[0]
    if ($asset.name -cne $names[0] -or $asset.bytes -le 0 -or
        $asset.bytes -ne (Get-Item -LiteralPath $path).Length -or $asset.sha256 -cne $checksums[$names[0]]) { throw 'Portable executable identity disagrees with its checksum record.' }
    $buildFiles = @{
        maintenance = 'bonsai-tools.exe'; core = 'bonsai-broker.exe'; ui = 'bonsai-chat.exe'
        asr = 'bonsai-speech-asr.exe'; tts = 'bonsai-speech-tts.exe'
    }
    $seenBuilds = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    if (@($manifest.builds).Count -ne $buildFiles.Count -or $manifest.manifestSha256 -cnotmatch '^[a-f0-9]{64}$') { throw 'Release build inventory is incomplete.' }
    foreach ($build in $manifest.builds) {
        if ($build.lane -cnotin @($buildFiles.Keys) -or -not $seenBuilds.Add($build.lane) -or
            $build.schemaVersion -ne 1 -or $build.sourceRevision -cne $SourceRevision -or $build.target -cne 'aarch64-pc-windows-msvc' -or
            $build.file -cne $buildFiles[$build.lane] -or $build.bytes -le 0 -or $build.sha256 -cnotmatch '^[a-f0-9]{64}$') { throw 'Unexpected build lane, source or executable identity.' }
    }
    $manifest
}
