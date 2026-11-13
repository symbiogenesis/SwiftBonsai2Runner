#Requires -Version 7.6
# Remote publication policy. Importing this file performs no external work.
Set-StrictMode -Version Latest

function ConvertFrom-ReleaseResponse([string]$Text) {
    if ([string]::IsNullOrWhiteSpace($Text) -or $Text.Length -gt 1MB) { throw 'Release API response is empty or exceeds its allowed size.' }
    $value = ConvertFrom-Json -InputObject $Text -NoEnumerate
    if ($value -isnot [pscustomobject]) { throw 'Release API response must be one object.' }
    $value
}

function Assert-ReleaseObjectId($Value) {
    if (($Value -isnot [long] -and $Value -isnot [int]) -or $Value -le 0) { throw 'Release API object ID must be a positive integer.' }
}

function Find-PortableRemoteRelease([string]$Repository, [string]$Tag) {
    # JSON strings preserve each record on one line, including unexpected tags.
    $catalog = Invoke-ReleaseCommand 'gh' @('api', "repos/$Repository/releases?per_page=100", '--paginate', '--jq', '.[] | {id, tag_name} | @json')
    if ($catalog.Length -gt 1MB) { throw 'Release catalog exceeds its allowed size.' }
    $lines = @($catalog -split '\r?\n' | Where-Object { $_.Length -gt 0 })
    if ($lines.Count -gt 1024) { throw 'Release catalog exceeds its allowed inventory.' }
    $matches = @()
    foreach ($line in $lines) {
        $item = ConvertFrom-ReleaseResponse $line
        Assert-ReleaseObjectId $item.id
        if ($item.tag_name -isnot [string]) { throw 'Release catalog tag must be a string.' }
        if ($item.tag_name -ceq $Tag) { $matches += $item }
    }
    if ($matches.Count -gt 1) { throw 'Duplicate releases exist for this tag; publication requires review.' }
    if ($matches.Count -eq 1) {
        $release = ConvertFrom-ReleaseResponse (Invoke-ReleaseCommand 'gh' @('api', "repos/$Repository/releases/$($matches[0].id)"))
        Assert-ReleaseObjectId $release.id
        if ($release.id -ne $matches[0].id) { throw 'Release identity differs from its catalog entry.' }
        $release
    }
}

function Confirm-PortableRemoteTag([string]$Repository, [string]$Tag, [string]$SourceRevision, [switch]$RequireExisting) {
    $text = Invoke-ReleaseCommand 'gh' @('api', "repos/$Repository/git/matching-refs/tags/$Tag")
    if ($text.Length -gt 1MB) { throw 'Release tag response exceeds its allowed size.' }
    $references = ConvertFrom-Json -InputObject $text -NoEnumerate
    if ($references -isnot [array] -or $references.Count -gt 1024) { throw 'Release tag inventory is invalid.' }
    $exact = @($references | Where-Object { $_.ref -ceq "refs/tags/$Tag" })
    if ($exact.Count -gt 1) { throw 'Unexpected duplicate tag reference.' }
    if ($exact.Count -eq 0) {
        if ($RequireExisting) { throw 'The published release tag is missing.' }
        return
    }
    $object = $exact[0].object
    $depth = 0
    while ($object.type -ceq 'tag') {
        if (++$depth -gt 8 -or $object.sha -cnotmatch '^[0-9a-f]{40}$') { throw 'Release tag nesting or identity is invalid.' }
        $annotation = ConvertFrom-ReleaseResponse (Invoke-ReleaseCommand 'gh' @('api', "repos/$Repository/git/tags/$($object.sha)"))
        $object = $annotation.object
    }
    if ($object.type -cne 'commit' -or $object.sha -cne $SourceRevision) { throw 'Remote release tag points to different source.' }
}

function Assert-PortableRemoteRelease {
    param($Release, [string]$Repository, [string]$Tag, [string]$SourceRevision, [string]$Notes, [bool]$Prerelease,
        [Collections.IDictionary]$Assets, [bool]$ExpectedDraft, [long]$ExpectedId = 0, [switch]$Complete)
    Assert-ReleaseObjectId $Release.id
    if (($ExpectedId -gt 0 -and $Release.id -ne $ExpectedId) -or
        $Release.tag_name -isnot [string] -or $Release.tag_name -cne $Tag -or
        $Release.target_commitish -isnot [string] -or $Release.target_commitish -cne $SourceRevision -or
        $Release.name -isnot [string] -or $Release.name -cne "Swift Bonsai 2 Runner $Tag" -or
        $Release.html_url -isnot [string] -or $Release.html_url -cne "https://github.com/$Repository/releases/tag/$Tag" -or
        $Release.body -isnot [string] -or $Release.body.Replace("`r`n", "`n") -cne $Notes.Replace("`r`n", "`n") -or
        $Release.draft -isnot [bool] -or $Release.draft -ne $ExpectedDraft -or
        $Release.prerelease -isnot [bool] -or $Release.prerelease -ne $Prerelease) {
        throw 'Remote release source, notes or publication metadata differs; existing releases are never replaced.'
    }
    if ($Release.assets -isnot [array] -or $Release.assets.Count -gt $Assets.Count -or
        ($Complete -and $Release.assets.Count -ne $Assets.Count)) { throw 'Remote release asset inventory is invalid or incomplete.' }
    $seen = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    $seenIds = [Collections.Generic.HashSet[long]]::new()
    foreach ($asset in $Release.assets) {
        Assert-ReleaseObjectId $asset.id
        if (-not $seenIds.Add($asset.id) -or $asset.name -isnot [string] -or -not $Assets.Contains($asset.name) -or
            -not $seen.Add($asset.name) -or $asset.state -cne 'uploaded' -or
            ($asset.size -isnot [long] -and $asset.size -isnot [int]) -or
            $asset.size -ne $Assets[$asset.name].Bytes -or
            $asset.digest -isnot [string] -or $asset.digest -cne "sha256:$($Assets[$asset.name].Sha256)") {
            throw 'Remote release has an unknown, duplicate, incomplete or changed asset; no asset is overwritten.'
        }
    }
}

function Publish-PortableRelease {
    [CmdletBinding()]
    param([string]$RepositoryRoot, [string]$AssetDirectory, [string]$Tag, [string]$SourceRevision,
        [bool]$Prerelease, [string]$Repository)
    $null = Assert-PortableReleaseAssets -AssetDirectory $AssetDirectory -Tag $Tag -SourceRevision $SourceRevision
    $assets = [ordered]@{}
    foreach ($name in Get-PortableReleaseAssetNames $Tag.Substring(1)) {
        $path = Join-Path $AssetDirectory $name
        $assets[$name] = @{ Path = $path; Bytes = (Get-Item -LiteralPath $path).Length
            Sha256 = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() }
    }
    $notes = [IO.File]::ReadAllText((Join-Path $RepositoryRoot "docs/releases/$Tag.md"))
    $policy = @{ Repository = $Repository; Tag = $Tag; SourceRevision = $SourceRevision; Notes = $notes; Prerelease = $Prerelease; Assets = $assets }
    $release = Find-PortableRemoteRelease $Repository $Tag
    if ($null -ne $release -and $release.draft -is [bool] -and -not $release.draft) {
        # A previous PATCH may have completed before its final readback failed.
        # Only verify that completed publication; this path never mutates remote
        # state and does not require main to remain at an already released SHA.
        Assert-PortableRemoteRelease -Release $release @policy -ExpectedDraft $false -Complete
        Confirm-PortableRemoteTag $Repository $Tag $SourceRevision -RequireExisting
        $verified = ConvertFrom-ReleaseResponse (Invoke-ReleaseCommand 'gh' @('api', "repos/$Repository/releases/$($release.id)"))
        Assert-PortableRemoteRelease -Release $verified @policy -ExpectedDraft $false -ExpectedId $release.id -Complete
        Write-Host "Release $Tag is already published; exact metadata, assets and source tag verified without mutation."
        return $verified.html_url
    }
    $main = Invoke-ReleaseCommand 'gh' @('api', "repos/$Repository/commits/main", '--jq', '.sha')
    if ($main.Trim() -cne $SourceRevision) { throw 'Remote main advanced during the build; dispatch the settled source again.' }
    Confirm-PortableRemoteTag $Repository $Tag $SourceRevision
    $tags = (Invoke-ReleaseCommand 'git' @('-C', $RepositoryRoot, 'tag', '--list', $Tag)).Trim()
    if ($tags) {
        $target = (Invoke-ReleaseCommand 'git' @('-C', $RepositoryRoot, 'rev-parse', "$Tag^{commit}")).Trim()
        if ($target -cne $SourceRevision) { throw 'Release tag points to different source.' }
    }
    if ($null -ne $release) {
        Assert-PortableRemoteRelease -Release $release @policy -ExpectedDraft $true
        Write-Host "Resuming verified same-source draft $Tag; matching uploaded assets will be reused."
    }
    $releaseWork = Join-Path (Resolve-ReleasePath $env:RUNNER_TEMP) ('SwiftBonsai-Publish-' + [guid]::NewGuid().ToString('N'))
    [void][IO.Directory]::CreateDirectory($releaseWork)
    $create = Join-Path $releaseWork 'release-create.json'
    $edit = Join-Path $releaseWork 'release-publish.json'
    $publicationFailure = $null
    try {
        if ($null -eq $release) {
            @{ tag_name = $Tag; target_commitish = $SourceRevision; name = "Swift Bonsai 2 Runner $Tag"
                body = $notes; draft = $true; prerelease = $Prerelease } |
                ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $create -Encoding utf8NoBOM
            $createFailure = $null
            try { $null = Invoke-ReleaseCommand 'gh' @('api', '--method', 'POST', "repos/$Repository/releases", '--input', $create) }
            catch { $createFailure = $_.Exception.Message }
            # A lost response can still mean a completed mutation. Read state;
            # never repeat POST or infer success from missing local output.
            $release = Find-PortableRemoteRelease $Repository $Tag
            if ($null -eq $release) { throw "Draft creation was not confirmed; rerun the failed publication job. $createFailure" }
            Assert-PortableRemoteRelease -Release $release @policy -ExpectedDraft $true
            if ($createFailure) { Write-Host 'Draft creation response failed; authoritative readback verified completion.' }
        }
        $id = [long]$release.id
        foreach ($name in $assets.Keys) {
            $release = ConvertFrom-ReleaseResponse (Invoke-ReleaseCommand 'gh' @('api', "repos/$Repository/releases/$id"))
            Assert-PortableRemoteRelease -Release $release @policy -ExpectedDraft $true -ExpectedId $id
            if (@($release.assets | Where-Object { $_.name -ceq $name }).Count -eq 1) { continue }
            $uploadFailure = $null
            try {
                $null = Invoke-ReleaseCommand 'gh' @('api', '--method', 'POST', "https://uploads.github.com/repos/$Repository/releases/$id/assets?name=$name",
                    '--header', 'Content-Type: application/octet-stream', '--input', $assets[$name].Path) -TimeoutSeconds 600
            } catch { $uploadFailure = $_.Exception.Message }
            $release = ConvertFrom-ReleaseResponse (Invoke-ReleaseCommand 'gh' @('api', "repos/$Repository/releases/$id"))
            Assert-PortableRemoteRelease -Release $release @policy -ExpectedDraft $true -ExpectedId $id
            if (@($release.assets | Where-Object { $_.name -ceq $name }).Count -ne 1) {
                throw "Upload was not confirmed for $name; the release remains a draft. Rerun the failed publication job. $uploadFailure"
            }
            if ($uploadFailure) { Write-Host "Upload response failed for $name; authoritative readback verified its exact bytes." }
        }
        $release = ConvertFrom-ReleaseResponse (Invoke-ReleaseCommand 'gh' @('api', "repos/$Repository/releases/$id"))
        Assert-PortableRemoteRelease -Release $release @policy -ExpectedDraft $true -ExpectedId $id -Complete
        Confirm-PortableRemoteTag $Repository $Tag $SourceRevision
        $main = Invoke-ReleaseCommand 'gh' @('api', "repos/$Repository/commits/main", '--jq', '.sha')
        if ($main.Trim() -cne $SourceRevision) { throw 'Remote main advanced during upload; the release remains a draft.' }
        @{ draft = $false; prerelease = $Prerelease; make_latest = $(if ($Prerelease) { 'false' } else { 'true' }) } |
            ConvertTo-Json | Set-Content -LiteralPath $edit -Encoding utf8NoBOM
        $publishFailure = $null
        try { $null = Invoke-ReleaseCommand 'gh' @('api', '--method', 'PATCH', "repos/$Repository/releases/$id", '--input', $edit) }
        catch { $publishFailure = $_.Exception.Message }
        $published = ConvertFrom-ReleaseResponse (Invoke-ReleaseCommand 'gh' @('api', "repos/$Repository/releases/$id"))
        Assert-PortableRemoteRelease -Release $published @policy -ExpectedDraft $false -ExpectedId $id -Complete
        Confirm-PortableRemoteTag $Repository $Tag $SourceRevision -RequireExisting
        if ($publishFailure) { Write-Host 'Publication response failed; authoritative readback verified completion.' }
        $expectedUrl = "https://github.com/$Repository/releases/tag/$Tag"
        if ($published.html_url -cne $expectedUrl) { throw 'Published release URL differs from the expected repository/tag.' }
        $expectedUrl
    } catch {
        $publicationFailure = $_
        throw
    } finally {
        Remove-ReleaseDirectory -Path $releaseWork -ParentDirectory $env:RUNNER_TEMP -LeafName ([IO.Path]::GetFileName($releaseWork)) -PrimaryFailure $publicationFailure
    }
}
