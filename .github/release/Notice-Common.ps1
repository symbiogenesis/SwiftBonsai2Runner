#Requires -Version 7.6
# Transport and rendering only; Rust selects and validates immutable notices.
$script:upstreamNoticeCache = @{}
function Get-UpstreamNoticeText {
    [CmdletBinding()]
    [OutputType([string])]
    param([Parameter(Mandatory)][string]$Directory,[Parameter(Mandatory)][string]$Repository,
        [string]$PackageName='',[string]$PackageVersion='')
    $identity = Invoke-NativeReleasePolicy @{operation='notice-provenance';directory=$Directory;repository=$Repository;packageName=$PackageName;packageVersion=$PackageVersion}
    $key = "$($identity.upstream)@$($identity.revision)"
    if ($script:upstreamNoticeCache.ContainsKey($key)) { return $script:upstreamNoticeCache[$key] }
    $tree = Invoke-ReleaseCommand 'gh' @('api',"repos/$($identity.upstream)/git/trees/$($identity.revision)") | ConvertFrom-Json
    $files = Invoke-NativeReleasePolicy @{operation='notice-tree';response=$tree}
    $result = [Text.StringBuilder]::new()
    if ($identity.recordedRevision -cne $identity.revision) {
        [void]$result.AppendLine("Archive source commit: $($identity.recordedRevision). Shared license material: reviewed 0.37.0 release commit $($identity.revision).")
    }
    foreach ($file in $files) {
        $blob = Invoke-ReleaseCommand 'gh' @('api',"repos/$($identity.upstream)/git/blobs/$($file.sha)") | ConvertFrom-Json
        $notice = Invoke-NativeReleasePolicy @{operation='notice-blob';response=$blob;file=$file}
        [void]$result.AppendLine("`n--- https://github.com/$($identity.upstream)/blob/$($identity.revision)/$($file.path) sha256=$($notice.sha256) ---")
        [void]$result.AppendLine($notice.text)
    }
    $script:upstreamNoticeCache[$key] = $result.ToString()
    $result.ToString()
}
