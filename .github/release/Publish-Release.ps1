#Requires -Version 7.0
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$RepositoryRoot,
    [Parameter(Mandatory)][string]$AssetDirectory,
    [Parameter(Mandatory)][string]$Tag,
    [Parameter(Mandatory)][string]$SourceRevision,
    [Parameter(Mandatory)][bool]$Prerelease
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
. (Join-Path $PSScriptRoot '../../scripts/release/Release-Common.ps1')
if ($Tag -cnotmatch '^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$' -or $SourceRevision -cnotmatch '^[0-9a-f]{40}$') { throw 'Invalid release identity.' }
if ($env:GITHUB_REPOSITORY -cne 'symbiogenesis/SwiftBonsai2Runner' -or $env:GITHUB_REF -cne 'refs/heads/main' -or $env:GITHUB_SHA -cne $SourceRevision) { throw 'Publishing requires this repository main at the dispatched revision.' }
$RepositoryRoot = Resolve-ReleasePath $RepositoryRoot
$AssetDirectory = Resolve-ReleasePath $AssetDirectory
$version = $Tag.Substring(1)
$names = @("SwiftBonsai2Runner-$version-windows-arm64.msi", "SwiftBonsai2Runner-$version-windows-arm64.zip", 'THIRD-PARTY-NOTICES.txt', 'release-manifest.json', 'SHA256SUMS.txt')
$children = @(Get-ChildItem -LiteralPath $AssetDirectory -Force)
if ($children.Count -ne $names.Count) { throw 'Release asset inventory differs from the reviewed inventory.' }
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
if ($manifest.schemaVersion -ne 1 -or $manifest.tag -cne $Tag -or $manifest.version -cne $version -or
    $manifest.sourceRevision -cne $SourceRevision -or $manifest.target -cne 'aarch64-pc-windows-msvc' -or
    $manifest.installerSmoke -cne 'passed-install-inspect-uninstall-without-model' -or $manifest.signed -ne $false) { throw 'Release manifest source, architecture or installer checks differ.' }
$assetNames = @("SwiftBonsai2Runner-$version-windows-arm64.msi", "SwiftBonsai2Runner-$version-windows-arm64.zip", 'THIRD-PARTY-NOTICES.txt')
$seenAssets = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
if (@($manifest.assets).Count -ne $assetNames.Count) { throw 'Release manifest asset inventory is incomplete.' }
foreach ($asset in $manifest.assets) {
    if ($asset.name -cnotin $assetNames -or -not $seenAssets.Add($asset.name)) { throw 'Unexpected or duplicate manifest asset.' }
    $path = Join-Path $AssetDirectory $asset.name
    if ($asset.bytes -ne (Get-Item -LiteralPath $path).Length -or $asset.sha256 -cne $checksums[$asset.name]) { throw 'Manifest asset identity disagrees with its checksum record.' }
}
$buildFiles = @{
    maintenance = 'bonsai-tools.exe'; core = 'bonsai-broker.exe'; ui = 'bonsai-chat.exe'
    asr = 'bonsai-speech-asr.exe'; tts = 'bonsai-speech-tts.exe'
}
$seenBuilds = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
if (@($manifest.builds).Count -ne $buildFiles.Count -or $manifest.manifestSha256 -cnotmatch '^[a-f0-9]{64}$') { throw 'Release build inventory is incomplete.' }
foreach ($build in $manifest.builds) {
    if (-not $buildFiles.ContainsKey($build.lane) -or -not $seenBuilds.Add($build.lane) -or
        $build.schemaVersion -ne 1 -or $build.sourceRevision -cne $SourceRevision -or $build.target -cne 'aarch64-pc-windows-msvc' -or
        $build.file -cne $buildFiles[$build.lane] -or $build.bytes -le 0 -or $build.sha256 -cnotmatch '^[a-f0-9]{64}$') { throw 'Unexpected build lane, source or executable identity.' }
}

$repository = $env:GITHUB_REPOSITORY
function Confirm-RemoteTag([switch]$RequireExisting) {
    $references = @(Invoke-ReleaseCommand 'gh' @('api', "repos/$repository/git/matching-refs/tags/$Tag") | ConvertFrom-Json)
    $exact = @($references | Where-Object { $_.ref -ceq "refs/tags/$Tag" })
    if ($exact.Count -gt 1) { throw 'Unexpected duplicate tag reference.' }
    if ($exact.Count -eq 0) {
        if ($RequireExisting) { throw 'The published release tag is missing.' }
        return
    }
    $object = $exact[0].object
    $depth = 0
    while ($object.type -ceq 'tag') {
        if (++$depth -gt 8) { throw 'Release tag nesting exceeds the allowed bound.' }
        $annotation = Invoke-ReleaseCommand 'gh' @('api', "repos/$repository/git/tags/$($object.sha)") | ConvertFrom-Json
        $object = $annotation.object
    }
    if ($object.type -cne 'commit' -or $object.sha -cne $SourceRevision) { throw 'Remote release tag points to different source.' }
}
# Refuse stale main rather than require broader workflow permissions to tag an
# earlier workflow-bearing commit. The owner can dispatch again on settled main.
$main = Invoke-ReleaseCommand 'gh' @('api', "repos/$repository/commits/main", '--jq', '.sha')
if ($main.Trim() -cne $SourceRevision) { throw 'Remote main advanced during the build; no release was created. Dispatch the settled source again.' }
$releases = (Invoke-ReleaseCommand 'gh' @('api', "repos/$repository/releases", '--paginate', '--jq', '.[] | .tag_name')) -split '\r?\n'
if ($releases -ccontains $Tag) { throw 'A release already exists for this tag; existing releases and assets are never overwritten.' }
Confirm-RemoteTag
# Resolve an existing tag (including annotated tags) before draft creation.
$tags = (Invoke-ReleaseCommand 'git' @('-C', $RepositoryRoot, 'tag', '--list', $Tag)).Trim()
if ($tags) {
    $target = (Invoke-ReleaseCommand 'git' @('-C', $RepositoryRoot, 'rev-parse', "$Tag^{commit}")).Trim()
    if ($target -cne $SourceRevision) { throw 'Release tag points to different source.' }
}
$notes = [IO.File]::ReadAllText((Join-Path $RepositoryRoot "docs/releases/$Tag.md"))
$releaseWork = Join-Path (Resolve-ReleasePath $env:RUNNER_TEMP) ('SwiftBonsai-Publish-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($releaseWork)
$create = Join-Path $releaseWork 'release-create.json'
@{
    tag_name = $Tag; target_commitish = $SourceRevision; name = "Swift Bonsai 2 Runner $Tag"
    body = $notes; draft = $true; prerelease = $Prerelease
} | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $create -Encoding utf8NoBOM
$release = Invoke-ReleaseCommand 'gh' @('api', '--method', 'POST', "repos/$repository/releases", '--input', $create) | ConvertFrom-Json
if (-not $release.draft -or $release.tag_name -cne $Tag -or $release.target_commitish -cne $SourceRevision) { throw 'Draft creation returned an unexpected release identity.' }
$id = [long]$release.id
foreach ($name in $names) {
    $path = Join-Path $AssetDirectory $name
    $uploaded = Invoke-ReleaseCommand 'gh' @('api', '--method', 'POST', "https://uploads.github.com/repos/$repository/releases/$id/assets?name=$name",
        '--header', 'Content-Type: application/octet-stream', '--input', $path) | ConvertFrom-Json
    $digest = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($uploaded.name -cne $name -or $uploaded.state -cne 'uploaded' -or $uploaded.size -ne (Get-Item -LiteralPath $path).Length -or $uploaded.digest -cne "sha256:$digest") { throw "Uploaded asset identity differs: $name. The release remains a draft." }
}
$verified = Invoke-ReleaseCommand 'gh' @('api', "repos/$repository/releases/$id") | ConvertFrom-Json
if (-not $verified.draft -or @($verified.assets).Count -ne $names.Count) { throw 'Uploaded release inventory is incomplete; the release remains a draft.' }
$seenRemote = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
foreach ($asset in $verified.assets) {
    if ($asset.name -cnotin $names -or -not $seenRemote.Add($asset.name) -or $asset.state -cne 'uploaded') { throw 'Unexpected or duplicate remote asset; the release remains a draft.' }
    $digest = (Get-FileHash -LiteralPath (Join-Path $AssetDirectory $asset.name) -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($asset.digest -cne "sha256:$digest" -or $asset.size -ne (Get-Item -LiteralPath (Join-Path $AssetDirectory $asset.name)).Length) { throw 'Remote asset identity changed; the release remains a draft.' }
}
Confirm-RemoteTag
$main = Invoke-ReleaseCommand 'gh' @('api', "repos/$repository/commits/main", '--jq', '.sha')
if ($main.Trim() -cne $SourceRevision) { throw 'Remote main advanced during upload; the release remains a draft.' }
$edit = Join-Path $releaseWork 'release-publish.json'
@{ draft = $false; prerelease = $Prerelease; make_latest = $(if ($Prerelease) { 'false' } else { 'true' }) } | ConvertTo-Json | Set-Content -LiteralPath $edit -Encoding utf8NoBOM
$published = Invoke-ReleaseCommand 'gh' @('api', '--method', 'PATCH', "repos/$repository/releases/$id", '--input', $edit) | ConvertFrom-Json
if ($published.draft -or $published.tag_name -cne $Tag -or $published.prerelease -ne $Prerelease) { throw 'Published release status differs; inspect GitHub before any subsequent operation.' }
Confirm-RemoteTag -RequireExisting
if (@($published.assets).Count -ne $names.Count) { throw 'Published release asset inventory changed.' }
$seenRemote.Clear()
foreach ($asset in $published.assets) {
    if ($asset.name -cnotin $names -or -not $seenRemote.Add($asset.name) -or $asset.state -cne 'uploaded') { throw 'Published release contains an unexpected or duplicate asset.' }
    $path = Join-Path $AssetDirectory $asset.name
    $digest = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($asset.digest -cne "sha256:$digest" -or $asset.size -ne (Get-Item -LiteralPath $path).Length) { throw 'Published release asset identity changed.' }
}
$published.html_url | Write-Output
