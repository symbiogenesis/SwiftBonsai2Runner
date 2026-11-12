#Requires -Version 7.6
# Imported after Release-Common.ps1; no network work occurs on import.
$script:upstreamNoticeCache = @{}
function Get-UpstreamNoticeText {
    [CmdletBinding()]
    [OutputType([string])]
    param(
        [Parameter(Mandatory)][string]$Directory,
        [Parameter(Mandatory)][string]$Repository,
        [string]$PackageName = '',
        [string]$PackageVersion = ''
    )
    # Some workspace crates omit the repository-root licenses from their crate
    # archive. Bind the fallback to the exact commit recorded inside that locked
    # archive, never to the repository's current default branch or an SPDX label.
    if ($Repository -cnotmatch '^https://github\.com/(?<owner>[A-Za-z0-9_.-]+)/(?<repo>[A-Za-z0-9_.-]+?)(?:\.git)?(?:/tree/[^\r\n?#]+)?/?$') {
        throw 'Missing notices require reviewed GitHub repository provenance.'
    }
    $upstream = "$($Matches.owner)/$($Matches.repo)"
    $provenancePath = Resolve-ReleasePath (Join-Path $Directory '.cargo_vcs_info.json') -Kind File
    $provenance = Get-Content -LiteralPath $provenancePath -Raw | ConvertFrom-Json
    $revision = [string]$provenance.git.sha1
    if ($revision -cnotmatch '^[0-9a-f]{40}$') { throw 'Missing notices require an exact source commit.' }
    $recordedRevision = $revision
    # These published 0.37.0 archives record a commit unavailable upstream.
    # Their shared MIT/Apache notices are reviewed at the immutable 0.37.0
    # release commit instead. Scope this exception to these exact crate inputs.
    if ($upstream -ceq 'emilk/egui' -and $revision -ceq '7c0b40402271e8d40cda69f87cf3c2617c2e4382' -and
        $PackageVersion -ceq '0.37.0' -and $PackageName -cin @('ecolor', 'egui', 'egui-wgpu', 'egui-winit', 'emath', 'epaint')) {
        $revision = '841a328ba2dd8f6996beb7189ebb7fc00e668a2a'
    }
    $key = "$upstream@$revision"
    if ($script:upstreamNoticeCache.ContainsKey($key)) { return $script:upstreamNoticeCache[$key] }
    $tree = Invoke-ReleaseCommand 'gh' @('api', "repos/$upstream/git/trees/$revision") | ConvertFrom-Json
    $files = @($tree.tree | Where-Object {
        $_.type -ceq 'blob' -and $_.mode -ceq '100644' -and
        $_.path -match '^(LICENSE|LICENCE|COPYING|COPYRIGHT|NOTICE|AUTHORS)([._-].*)?$'
    } | Sort-Object path)
    if ($files.Count -eq 0) { throw "No repository-root license material at $key; review this dependency before release." }
    $result = [Text.StringBuilder]::new()
    if ($recordedRevision -cne $revision) {
        [void]$result.AppendLine("Archive source commit: $recordedRevision. Shared license material: reviewed 0.37.0 release commit $revision.")
    }
    foreach ($file in $files) {
        if ($file.sha -cnotmatch '^[0-9a-f]{40}$' -or $file.size -le 0 -or $file.size -gt 4MB) { throw 'Invalid upstream notice identity or size.' }
        $blob = Invoke-ReleaseCommand 'gh' @('api', "repos/$upstream/git/blobs/$($file.sha)") | ConvertFrom-Json
        if ($blob.sha -cne $file.sha -or $blob.encoding -cne 'base64' -or $blob.size -ne $file.size) { throw 'Upstream notice identity changed.' }
        $bytes = [Convert]::FromBase64String($blob.content)
        if ($bytes.Length -ne $file.size) { throw 'Upstream notice size changed.' }
        $digest = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($bytes)).ToLowerInvariant()
        [void]$result.AppendLine("`n--- https://github.com/$upstream/blob/$revision/$($file.path) sha256=$digest ---")
        [void]$result.AppendLine([Text.UTF8Encoding]::new($false, $true).GetString($bytes))
    }
    $script:upstreamNoticeCache[$key] = $result.ToString()
    $result.ToString()
}
