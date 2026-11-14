#Requires -Version 7.6
# Exact portable release inventory is validated by the native release tool.
Set-StrictMode -Version Latest
function Get-PortableReleaseAssetNames([string]$Version) {
    @("SwiftBonsai2Runner-$Version-windows-arm64.exe", 'release-manifest.json', 'SHA256SUMS.txt')
}
function Assert-PortableReleaseAssets {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$AssetDirectory,[Parameter(Mandatory)][string]$Tag,
        [Parameter(Mandatory)][string]$SourceRevision)
    Invoke-NativeReleasePolicy @{operation='assets';assetDirectory=$AssetDirectory;tag=$Tag;sourceRevision=$SourceRevision}
}
