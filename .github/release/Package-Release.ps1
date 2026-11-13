#Requires -Version 7.6
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$RepositoryRoot,
    [Parameter(Mandatory)][string]$InputDirectory,
    [Parameter(Mandatory)][string]$OutputDirectory,
    [Parameter(Mandatory)][string]$EvidenceDirectory,
    [Parameter(Mandatory)][string]$Tag,
    [Parameter(Mandatory)][string]$SourceRevision
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
. (Join-Path $PSScriptRoot '../../scripts/release/Release-Common.ps1')
. (Join-Path $PSScriptRoot '../../scripts/release/Release-Assets.ps1')
if (-not $IsWindows -or [Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne 'Arm64' -or
    $env:RUNNER_ENVIRONMENT -cne 'github-hosted') { throw 'Release packaging requires a disposable hosted Windows ARM64 runner.' }
if ($Tag -cnotmatch '^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$' -or $SourceRevision -cnotmatch '^[0-9a-f]{40}$') { throw 'Invalid release identity.' }
$version = $Tag.Substring(1)
$RepositoryRoot = Resolve-ReleasePath $RepositoryRoot
$InputDirectory = Resolve-ReleasePath $InputDirectory
$OutputDirectory = Resolve-ReleasePath $OutputDirectory -AllowMissing
$EvidenceDirectory = Resolve-ReleasePath $EvidenceDirectory -AllowMissing
foreach ($directory in @($OutputDirectory, $EvidenceDirectory)) {
    if (Test-Path -LiteralPath $directory) { throw "Release output must be new: $directory" }
}
$inventory = [ordered]@{
    maintenance = 'bonsai-tools.exe'; core = 'bonsai-broker.exe'; ui = 'bonsai-chat.exe'
    asr = 'bonsai-speech-asr.exe'; tts = 'bonsai-speech-tts.exe'
}
$executables = @{}
$builds = @()
foreach ($lane in $inventory.Keys) {
    $directory = Resolve-ReleasePath (Join-Path $InputDirectory "release-build-$lane")
    $receiptPath = Resolve-ReleasePath (Join-Path $directory 'build-manifest.json') -Kind File
    $receipt = Get-Content -LiteralPath $receiptPath -Raw | ConvertFrom-Json
    $path = Resolve-ReleasePath (Join-Path $directory $inventory[$lane]) -Kind File
    if ($receipt.schemaVersion -ne 1 -or $receipt.sourceRevision -cne $SourceRevision -or
        $receipt.target -cne 'aarch64-pc-windows-msvc' -or $receipt.lane -cne $lane -or
        $receipt.file -cne $inventory[$lane] -or (Get-Item -LiteralPath $path).Length -ne $receipt.bytes -or
        (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() -cne $receipt.sha256) { throw "Release input identity differs: $lane" }
    $executables[$lane] = $path
    $builds += $receipt
}
$notices = Resolve-ReleasePath (Join-Path $InputDirectory 'release-build-tts/THIRD-PARTY-NOTICES.txt') -Kind File
if ((Get-Item -LiteralPath $notices).Length -lt 1024) { throw 'The complete third-party notice bundle is missing.' }
[void][IO.Directory]::CreateDirectory($OutputDirectory)
[void][IO.Directory]::CreateDirectory($EvidenceDirectory)
$stageParent = Resolve-ReleasePath ([IO.Path]::GetTempPath())
$stageLeaf = 'SwiftBonsai-ReleasePackage-' + [guid]::NewGuid().ToString('N')
$stage = Join-Path $stageParent $stageLeaf
[void][IO.Directory]::CreateDirectory($stage)
$packageFailure = $null
try {
    $package = Join-Path $stage 'package'
    $request = @{
        operation = 'create'; outputDirectory = $package; version = $version; sourceRevision = $SourceRevision
        chatPath = $executables.ui; brokerPath = $executables.core; asrPath = $executables.asr
        ttsPath = $executables.tts; toolPath = $executables.maintenance; thirdPartyNoticePath = $notices
    } | ConvertTo-Json -Compress
    $creation = Invoke-ReleaseCommand $executables.maintenance @('chat-package', '--json', $request) -BuildCommand | ConvertFrom-Json
    $manifestDigest = (Get-FileHash -LiteralPath (Join-Path $package 'package.json') -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($creation.ManifestSha256 -cne $manifestDigest) { throw 'Package creation receipt disagrees with its manifest.' }
    $bundle = Join-Path $OutputDirectory "SwiftBonsai2Runner-$version-windows-arm64.exe"
    $null = & (Join-Path $RepositoryRoot 'scripts/release/Build-WindowsExecutable.ps1') -PackageDirectory $package -ManifestSha256 $manifestDigest -OutputPath $bundle
    & (Join-Path $RepositoryRoot 'scripts/release/Test-WindowsExecutable.ps1') -ExecutablePath $bundle -ManifestSha256 $manifestDigest -EvidenceDirectory (Join-Path $EvidenceDirectory 'executable-smoke')
    $asset = @{
        name = [IO.Path]::GetFileName($bundle); bytes = (Get-Item -LiteralPath $bundle).Length
        sha256 = (Get-FileHash -LiteralPath $bundle -Algorithm SHA256).Hash.ToLowerInvariant()
    }
    @{
        schemaVersion = 1; distribution = 'portable-executable'
        version = $version; tag = $Tag; sourceRevision = $SourceRevision
        target = 'aarch64-pc-windows-msvc'; toolchain = 'nightly-2026-11-12-aarch64-pc-windows-msvc'
        manifestSha256 = $manifestDigest; signed = $false
        runUrl = "https://github.com/$env:GITHUB_REPOSITORY/actions/runs/$env:GITHUB_RUN_ID"
        executableSmoke = 'passed-inspect-extract-verify-without-model'; builds = $builds; assets = @($asset)
    } | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $OutputDirectory 'release-manifest.json') -Encoding utf8NoBOM
    $checksums = @(Get-ChildItem -LiteralPath $OutputDirectory -File | Sort-Object Name | ForEach-Object {
        '{0}  {1}' -f (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant(), $_.Name
    })
    [IO.File]::WriteAllLines((Join-Path $OutputDirectory 'SHA256SUMS.txt'), $checksums, [Text.UTF8Encoding]::new($false))
    $null = Assert-PortableReleaseAssets -AssetDirectory $OutputDirectory -Tag $Tag -SourceRevision $SourceRevision
} catch {
    $packageFailure = $_
    throw
} finally {
    Remove-ReleaseDirectory -Path $stage -ParentDirectory $stageParent -LeafName $stageLeaf -PrimaryFailure $packageFailure
}
