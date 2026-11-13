#Requires -Version 7.6
# Stateful offline publication controls. Every Git/GitHub operation is mocked;
# inert fixture executable bytes are never launched.
[CmdletBinding()]
param([ValidateSet('all', 'resume', 'ambiguous', 'reject', 'smoke')][string]$Case = 'all')
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'Release-Common.ps1')
. (Join-Path $PSScriptRoot 'Release-Assets.ps1')
. (Join-Path $PSScriptRoot 'Release-Publication.ps1')
$fixture = Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-Publication-Controls-' + [guid]::NewGuid().ToString('N'))
$assetDirectory = Join-Path $fixture 'assets'
$notesDirectory = Join-Path $fixture 'docs/releases'
foreach ($directory in @($fixture, $assetDirectory, $notesDirectory)) { [void][IO.Directory]::CreateDirectory($directory) }
$tag = 'v0.3.0'; $source = 'a' * 40; $repository = 'symbiogenesis/SwiftBonsai2Runner'
$names = @(Get-PortableReleaseAssetNames '0.3.0')
$notesPath = Join-Path $notesDirectory "$tag.md"
$notes = "# Release fixture`n`nReviewed notes with Unicode 雪.`n"
$oldTemp = $env:RUNNER_TEMP
$env:RUNNER_TEMP = $fixture
$passed = 0

function New-MockRelease {
    @{ id = 123; tag_name = $tag; target_commitish = $source; name = "Swift Bonsai 2 Runner $tag"
        body = $notes; draft = $true; prerelease = $true; assets = @()
        html_url = "https://github.com/$repository/releases/tag/$tag" }
}
function New-MockAsset([string]$Name) {
    $path = Join-Path $assetDirectory $Name
    @{ id = 1000 + [array]::IndexOf($names, $Name); name = $Name; state = 'uploaded'
        size = (Get-Item -LiteralPath $path).Length
        digest = 'sha256:' + (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() }
}

# The production helper calls this transport by its existing name. No process,
# HTTP client or installed GitHub executable can run through this fixture.
function Invoke-ReleaseCommand {
    param([string]$FilePath, [string[]]$ArgumentList, [int]$TimeoutSeconds = 180,
        [switch]$BuildCommand, [hashtable]$Environment = @{})
    if ($FilePath -ceq 'git') { return '' }
    if ($FilePath -cne 'gh' -or $ArgumentList[0] -cne 'api') { throw 'Unexpected fixture transport.' }
    $method = 'GET'; $endpoint = $ArgumentList[1]
    if ($endpoint -ceq '--method') { $method = $ArgumentList[2]; $endpoint = $ArgumentList[3] }
    if ($method -ceq 'GET') {
        if ($endpoint -ceq "repos/$repository/commits/main") {
            $script:state.MainReads++
            if ($script:state.Mode -ceq 'MainAdvance' -and $script:state.MainReads -gt 1) { return ('b' * 40) }
            return $source
        }
        if ($endpoint -ceq "repos/$repository/git/matching-refs/tags/$tag") {
            if ($script:state.TagChanged) { return (@(@{ref = "refs/tags/$tag"; object = @{type = 'commit'; sha = ('b' * 40)}}) | ConvertTo-Json -Depth 5 -Compress -AsArray) }
            if ($null -ne $script:state.Release -and $script:state.Release.draft -is [bool] -and -not $script:state.Release.draft) {
                return (@(@{ref = "refs/tags/$tag"; object = @{type = 'commit'; sha = $source}}) | ConvertTo-Json -Depth 5 -Compress -AsArray)
            }
            return '[]'
        }
        if ($endpoint -ceq "repos/$repository/releases?per_page=100") {
            if ($null -eq $script:state.Release) { return '' }
            $record = @{id = 123; tag_name = $tag} | ConvertTo-Json -Compress
            if ($script:state.DuplicateCatalog) { return "$record`n$record" }
            return $record
        }
        if ($endpoint -ceq "repos/$repository/releases/123" -and $null -ne $script:state.Release) {
            if ($script:state.FailReadback) { throw 'Simulated unavailable mutation readback.' }
            return ($script:state.Release | ConvertTo-Json -Depth 10 -Compress)
        }
        throw "Unexpected fixture read: $endpoint"
    }
    if ($method -ceq 'POST' -and $endpoint -ceq "repos/$repository/releases") {
        $script:state.Mutations.Add('create')
        if ($null -ne $script:state.Release) { throw 'Fixture attempted duplicate creation.' }
        if ($script:state.Mode -ceq 'CreateNoEffect') { throw 'Simulated creation failure before mutation.' }
        $script:state.Release = New-MockRelease
        if ($script:state.Mode -ceq 'CreateAmbiguous') { throw 'Simulated lost draft creation response.' }
    } elseif ($method -ceq 'POST' -and $endpoint.StartsWith("https://uploads.github.com/repos/$repository/releases/123/assets?name=", [StringComparison]::Ordinal)) {
        $name = $endpoint.Substring($endpoint.IndexOf('?name=', [StringComparison]::Ordinal) + 6)
        $script:state.Mutations.Add("upload:$name")
        if ($name -cnotin $names -or @($script:state.Release.assets | Where-Object { $_.name -ceq $name }).Count) { throw 'Fixture attempted unknown or duplicate asset upload.' }
        if ($script:state.Mode -ceq 'UploadNoEffect') { throw 'Simulated upload failure before mutation.' }
        $script:state.Release.assets += (New-MockAsset $name)
        if ($script:state.Mode -ceq 'UploadReadbackUnavailable') { $script:state.FailReadback = $true; throw 'Simulated upload with unavailable readback.' }
        if ($script:state.Mode -ceq 'UploadAmbiguous') { throw 'Simulated lost asset upload response.' }
    } elseif ($method -ceq 'PATCH' -and $endpoint -ceq "repos/$repository/releases/123") {
        $script:state.Mutations.Add('publish')
        if ($script:state.Mode -ceq 'PublishNoEffect') { throw 'Simulated publish failure before mutation.' }
        $script:state.Release.draft = $false
        if ($script:state.Mode -ceq 'PublishAmbiguous') { throw 'Simulated lost publication response.' }
    } else { throw "Unexpected fixture mutation: $method $endpoint" }
    $script:state.Release | ConvertTo-Json -Depth 10 -Compress
}

function Invoke-PublicationScenario {
    param([string]$Name, [int]$InitialAssets = -1, [string]$Mode = '', [scriptblock]$Change,
        [switch]$Reject, [string[]]$ExpectedMutations = @())
    $script:state = @{ Release = $null; Mode = $Mode; MainReads = 0; TagChanged = $false; DuplicateCatalog = $false; FailReadback = $false
        Mutations = [Collections.Generic.List[string]]::new() }
    if ($InitialAssets -ge 0) {
        $script:state.Release = New-MockRelease
        for ($index = 0; $index -lt $InitialAssets; $index++) { $script:state.Release.assets += (New-MockAsset $names[$index]) }
    }
    if ($null -ne $Change) { & $Change $script:state }
    $failure = $null; $url = $null
    try {
        $url = Publish-PortableRelease -RepositoryRoot $fixture -AssetDirectory $assetDirectory -Tag $tag -SourceRevision $source -Prerelease $true -Repository $repository
    } catch { $failure = $_ }
    if ($Reject -and $null -eq $failure) { throw "$Name accepted invalid or incomplete publication." }
    if (-not $Reject -and $null -ne $failure) { throw "$Name failed: $($failure.Exception.Message)" }
    if (-not $Reject -and ($url -cne "https://github.com/$repository/releases/tag/$tag" -or $script:state.Release.draft -ne $false)) { throw "$Name did not verify publication." }
    if (($script:state.Mutations -join '|') -cne ($ExpectedMutations -join '|')) {
        throw "$Name mutated unexpectedly: $($script:state.Mutations -join ', ')"
    }
    if (@(Get-ChildItem -LiteralPath $fixture -Directory -Filter 'SwiftBonsai-Publish-*').Count) { throw "$Name left private publication payloads behind." }
    $script:passed++
}

try {
    [IO.File]::WriteAllText($notesPath, $notes, [Text.UTF8Encoding]::new($false))
    $executable = Join-Path $assetDirectory $names[0]
    [IO.File]::WriteAllText($executable, 'Inert portable fixture; never execute.', [Text.UTF8Encoding]::new($false))
    $files = [ordered]@{ maintenance = 'bonsai-tools.exe'; core = 'bonsai-broker.exe'; ui = 'bonsai-chat.exe'
        asr = 'bonsai-speech-asr.exe'; tts = 'bonsai-speech-tts.exe' }
    $manifest = @{ schemaVersion = 1; distribution = 'portable-executable'; version = '0.3.0'; tag = $tag
        sourceRevision = $source; target = 'aarch64-pc-windows-msvc'; executableSmoke = 'passed-inspect-extract-verify-without-model'
        signed = $false; manifestSha256 = ('b' * 64)
        builds = @($files.Keys | ForEach-Object { @{schemaVersion = 1; sourceRevision = $source; target = 'aarch64-pc-windows-msvc'
            lane = $_; file = $files[$_]; bytes = 1; sha256 = ('c' * 64)} })
        assets = @(@{name = $names[0]; bytes = (Get-Item -LiteralPath $executable).Length
            sha256 = (Get-FileHash -LiteralPath $executable -Algorithm SHA256).Hash.ToLowerInvariant()}) }
    $manifest | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $assetDirectory $names[1]) -Encoding utf8NoBOM
    $checksums = @(foreach ($name in $names[0..1]) {
        '{0}  {1}' -f (Get-FileHash -LiteralPath (Join-Path $assetDirectory $name) -Algorithm SHA256).Hash.ToLowerInvariant(), $name
    })
    [IO.File]::WriteAllLines((Join-Path $assetDirectory $names[2]), $checksums, [Text.UTF8Encoding]::new($false))
    $uploads = @($names | ForEach-Object { "upload:$_" })
    if ($Case -eq 'smoke') {
        Invoke-PublicationScenario -Name partial-draft -InitialAssets 1 -ExpectedMutations ($uploads[1..2] + @('publish'))
        Invoke-PublicationScenario -Name lost-upload-response -InitialAssets 1 -Mode UploadAmbiguous -ExpectedMutations ($uploads[1..2] + @('publish'))
        Invoke-PublicationScenario -Name foreign-source -InitialAssets 3 -Change { param($s) $s.Release.target_commitish = 'b' * 40 } -Reject
    }
    if ($Case -in @('all', 'resume')) {
        Invoke-PublicationScenario -Name fresh -ExpectedMutations (@('create') + $uploads + @('publish'))
        Invoke-PublicationScenario -Name empty-draft -InitialAssets 0 -ExpectedMutations ($uploads + @('publish'))
        Invoke-PublicationScenario -Name partial-draft -InitialAssets 1 -ExpectedMutations ($uploads[1..2] + @('publish'))
        Invoke-PublicationScenario -Name complete-draft -InitialAssets 3 -ExpectedMutations @('publish')
        Invoke-PublicationScenario -Name notes-line-endings -InitialAssets 3 -Change { param($s) $s.Release.body = $notes.Replace("`n", "`r`n") } -ExpectedMutations @('publish')
    }
    if ($Case -in @('all', 'ambiguous')) {
        Invoke-PublicationScenario -Name lost-create-response -Mode CreateAmbiguous -ExpectedMutations (@('create') + $uploads + @('publish'))
        Invoke-PublicationScenario -Name lost-upload-response -InitialAssets 1 -Mode UploadAmbiguous -ExpectedMutations ($uploads[1..2] + @('publish'))
        Invoke-PublicationScenario -Name lost-publish-response -InitialAssets 3 -Mode PublishAmbiguous -ExpectedMutations @('publish')
        Invoke-PublicationScenario -Name failed-create -Mode CreateNoEffect -Reject -ExpectedMutations @('create')
        Invoke-PublicationScenario -Name failed-upload -InitialAssets 1 -Mode UploadNoEffect -Reject -ExpectedMutations @($uploads[1])
        Invoke-PublicationScenario -Name failed-publish -InitialAssets 3 -Mode PublishNoEffect -Reject -ExpectedMutations @('publish')
        Invoke-PublicationScenario -Name unavailable-upload-readback -InitialAssets 1 -Mode UploadReadbackUnavailable -Reject -ExpectedMutations @($uploads[1])
        Invoke-PublicationScenario -Name main-advanced -InitialAssets 3 -Mode MainAdvance -Reject
    }
    if ($Case -in @('all', 'reject')) {
        $rejections = [ordered]@{
            published = { param($s) $s.Release.draft = $false }
            'foreign-source' = { param($s) $s.Release.target_commitish = 'b' * 40 }
            'changed-tag' = { param($s) $s.Release.tag_name = 'v0.3.1' }
            'changed-title' = { param($s) $s.Release.name += ' changed' }
            'foreign-url' = { param($s) $s.Release.html_url = 'https://github.com/other/repository/releases/tag/v0.3.0' }
            'changed-id' = { param($s) $s.Release.id = 124 }
            'changed-notes' = { param($s) $s.Release.body += ' changed' }
            'changed-prerelease' = { param($s) $s.Release.prerelease = $false }
            'string-draft' = { param($s) $s.Release.draft = 'true' }
            'string-prerelease' = { param($s) $s.Release.prerelease = 'true' }
            'null-draft' = { param($s) $s.Release.draft = $null }
            'unknown-asset' = { param($s) $s.Release.assets[0].name = 'unknown.exe' }
            'changed-digest' = { param($s) $s.Release.assets[0].digest = 'sha256:' + ('d' * 64) }
            'changed-size' = { param($s) $s.Release.assets[0].size++ }
            'string-size' = { param($s) $s.Release.assets[0].size = [string]$s.Release.assets[0].size }
            'incomplete-upload' = { param($s) $s.Release.assets[0].state = 'starter' }
            'duplicate-asset' = { param($s) $s.Release.assets[1] = $s.Release.assets[0] }
            'duplicate-asset-id' = { param($s) $s.Release.assets[1].id = $s.Release.assets[0].id }
            'duplicate-release' = { param($s) $s.DuplicateCatalog = $true }
            'foreign-tag' = { param($s) $s.TagChanged = $true }
        }
        foreach ($name in $rejections.Keys) { Invoke-PublicationScenario -Name $name -InitialAssets 3 -Change $rejections[$name] -Reject }
    }
    Write-Host "Release publication controls passed: $passed $Case cases; all transport mocked, no network or executable launches."
} finally {
    $env:RUNNER_TEMP = $oldTemp
    foreach ($name in $names) { $path = Join-Path $assetDirectory $name; if (Test-Path -LiteralPath $path) { Remove-Item -LiteralPath $path -Force } }
    if (Test-Path -LiteralPath $notesPath) { Remove-Item -LiteralPath $notesPath -Force }
    foreach ($directory in @($assetDirectory, $notesDirectory, (Join-Path $fixture 'docs'), $fixture)) { [IO.Directory]::Delete($directory) }
}
