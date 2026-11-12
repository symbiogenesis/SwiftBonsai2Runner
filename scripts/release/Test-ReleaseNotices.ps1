#Requires -Version 7.6
# Offline regression controls for omitted crate-archive licenses.
[CmdletBinding()]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'Release-Common.ps1')
. (Join-Path $PSScriptRoot '../../.github/release/Notice-Common.ps1')
$fixture = Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-Notice-Controls-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($fixture)
$provenance = Join-Path $fixture '.cargo_vcs_info.json'
$revision = 'a' * 40
$blobId = 'b' * 40
$notice = "Copyright Example contributors`nPermission is hereby granted. Unicode-β"
$bytes = [Text.UTF8Encoding]::new($false).GetBytes($notice)
$script:calls = @()
$script:badBlob = $false
function Invoke-ReleaseCommand([string]$FilePath, [string[]]$ArgumentList) {
    if ($FilePath -cne 'gh' -or $ArgumentList[0] -cne 'api') { throw 'Unexpected command.' }
    $script:calls += $ArgumentList[1]
    switch -Exact ($ArgumentList[1]) {
        "repos/example/library/git/trees/$revision" {
            return (@{tree = @(@{path = 'LICENSE-MIT'; type = 'blob'; mode = '100644'; sha = $blobId; size = $bytes.Length})} | ConvertTo-Json -Depth 5)
        }
        "repos/example/library/git/blobs/$blobId" {
            return (@{sha = $(if ($script:badBlob) { 'c' * 40 } else { $blobId }); encoding = 'base64'; size = $bytes.Length; content = [Convert]::ToBase64String($bytes)} | ConvertTo-Json)
        }
        'repos/emilk/egui/git/trees/841a328ba2dd8f6996beb7189ebb7fc00e668a2a' {
            return (@{tree = @(@{path = 'LICENSE-MIT'; type = 'blob'; mode = '100644'; sha = $blobId; size = $bytes.Length})} | ConvertTo-Json -Depth 5)
        }
        "repos/emilk/egui/git/blobs/$blobId" {
            return (@{sha = $blobId; encoding = 'base64'; size = $bytes.Length; content = [Convert]::ToBase64String($bytes)} | ConvertTo-Json)
        }
        default { throw 'The fallback requested unpinned source.' }
    }
}
try {
    @{git = @{sha1 = $revision}} | ConvertTo-Json | Set-Content -LiteralPath $provenance -Encoding utf8NoBOM
    $actual = Get-UpstreamNoticeText -Directory $fixture -Repository 'https://github.com/example/library'
    $digest = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($bytes)).ToLowerInvariant()
    if (-not $actual.Contains($notice) -or -not $actual.Contains("/blob/$revision/LICENSE-MIT sha256=$digest")) { throw 'Source-bound license text changed.' }
    $again = Get-UpstreamNoticeText -Directory $fixture -Repository 'https://github.com/example/library.git'
    if ($again -cne $actual -or $script:calls.Count -ne 2) { throw 'Notice caching changed source identity.' }
    $script:upstreamNoticeCache.Clear()
    $script:badBlob = $true
    $rejected = $false
    try { $null = Get-UpstreamNoticeText -Directory $fixture -Repository 'https://github.com/example/library' }
    catch { if ($_.Exception.Message -cne 'Upstream notice identity changed.') { throw }; $rejected = $true }
    if (-not $rejected) { throw 'Changed license identity was accepted.' }
    $rejected = $false
    try { $null = Get-UpstreamNoticeText -Directory $fixture -Repository 'https://untrusted.example/library' }
    catch { if ($_.Exception.Message -cne 'Missing notices require reviewed GitHub repository provenance.') { throw }; $rejected = $true }
    if (-not $rejected) { throw 'Unreviewed notice host was accepted.' }
    @{git = @{sha1 = '7c0b40402271e8d40cda69f87cf3c2617c2e4382'}} | ConvertTo-Json | Set-Content -LiteralPath $provenance -Encoding utf8NoBOM
    $actual = Get-UpstreamNoticeText -Directory $fixture -Repository 'https://github.com/emilk/egui/tree/main/crates/egui-wgpu' -PackageName 'egui-wgpu' -PackageVersion '0.37.0'
    if (-not $actual.Contains('Archive source commit: 7c0b40402271e8d40cda69f87cf3c2617c2e4382.') -or
        -not $actual.Contains('/blob/841a328ba2dd8f6996beb7189ebb7fc00e668a2a/LICENSE-MIT')) { throw 'Reviewed egui license exception lost its identities.' }
    Write-Output 'Release notice controls passed: exact source, UTF-8, digest, cache, changed identity, host rejection and scoped egui exception.'
} finally {
    Remove-Item -LiteralPath $provenance -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $fixture -Force
}
