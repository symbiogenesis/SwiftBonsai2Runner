#Requires -Version 7.6
# Native admission and publication; importing performs no external work.
Set-StrictMode -Version Latest
function Publish-PortableRelease {
    [CmdletBinding()]
    param([string]$RepositoryRoot,[string]$AssetDirectory,[string]$Tag,[string]$SourceRevision,
        [bool]$Prerelease,[string]$Repository)
    Invoke-NativeReleasePolicy @{operation='publish';repositoryRoot=$RepositoryRoot;assetDirectory=$AssetDirectory;tag=$Tag;sourceRevision=$SourceRevision;prerelease=$Prerelease;repository=$Repository}
}
