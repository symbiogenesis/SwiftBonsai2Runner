#Requires -Version 7.0
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$RepositoryRoot,
    [Parameter(Mandatory)][string]$InputDirectory,
    [Parameter(Mandatory)][string]$OutputDirectory,
    [Parameter(Mandatory)][string]$EvidenceDirectory,
    [Parameter(Mandatory)][string]$WixPath,
    [Parameter(Mandatory)][string]$Tag,
    [Parameter(Mandatory)][string]$SourceRevision
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
. (Join-Path $PSScriptRoot '../../scripts/release/Release-Common.ps1')
if (-not $IsWindows -or [Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne 'Arm64' -or
    $env:RUNNER_ENVIRONMENT -cne 'github-hosted') { throw 'Release packaging and prerequisite installation require a disposable hosted Windows ARM64 runner.' }
if ($Tag -cnotmatch '^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$' -or $SourceRevision -cnotmatch '^[0-9a-f]{40}$') { throw 'Invalid release identity.' }
$version = $Tag.Substring(1)
$RepositoryRoot = Resolve-ReleasePath $RepositoryRoot
$InputDirectory = Resolve-ReleasePath $InputDirectory
$WixPath = Resolve-ReleasePath $WixPath -Kind File
$OutputDirectory = Resolve-ReleasePath $OutputDirectory -AllowMissing
$EvidenceDirectory = Resolve-ReleasePath $EvidenceDirectory -AllowMissing
foreach ($directory in @($OutputDirectory, $EvidenceDirectory)) {
    if (Test-Path -LiteralPath $directory) { throw "Release output must be new: $directory" }
    [void][IO.Directory]::CreateDirectory($directory)
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
$package = Join-Path $EvidenceDirectory 'portable-package'
$request = Join-Path $EvidenceDirectory 'create-package.json'
@{
    operation = 'create'; outputDirectory = $package; version = $version; sourceRevision = $SourceRevision
    chatPath = $executables.ui; brokerPath = $executables.core; asrPath = $executables.asr
    ttsPath = $executables.tts; toolPath = $executables.maintenance; thirdPartyNoticePath = $notices
} | ConvertTo-Json | Set-Content -LiteralPath $request -Encoding utf8NoBOM
$creation = Invoke-ReleaseCommand $executables.maintenance @('chat-package', '--request', $request) | ConvertFrom-Json
$creation | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $EvidenceDirectory 'package-receipt.json') -Encoding utf8NoBOM
$manifestDigest = (Get-FileHash -LiteralPath (Join-Path $package 'package.json') -Algorithm SHA256).Hash.ToLowerInvariant()
if ($creation.ManifestSha256 -cne $manifestDigest) { throw 'Package creation receipt disagrees with its manifest.' }
& (Join-Path $RepositoryRoot 'scripts/release/Build-WindowsInstaller.ps1') -PackageDirectory $package -ManifestSha256 $manifestDigest -OutputDirectory $OutputDirectory -WixPath $WixPath
$installerReceipt = Join-Path $OutputDirectory 'installer-receipt.json'
if (Test-Path -LiteralPath $installerReceipt -PathType Leaf) {
    Move-Item -LiteralPath $installerReceipt -Destination (Join-Path $EvidenceDirectory 'installer-receipt.json')
}
$msi = Join-Path $OutputDirectory "SwiftBonsai2Runner-$version-windows-arm64.msi"
if (-not (Test-Path -LiteralPath $msi -PathType Leaf)) { throw 'Expected ARM64 MSI was not created.' }

# The separately installed Microsoft runtime is a prerequisite, not a bundled
# component. Download only when needed, verify Microsoft's signature, and retain
# its actual identity. The MSI never performs a network install custom action.
$runtimeKey = 'HKLM:\SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\arm64'
$runtime = Get-ItemProperty -LiteralPath $runtimeKey -ErrorAction SilentlyContinue
if ($null -eq $runtime -or $runtime.Installed -ne 1) {
    $redist = Join-Path $EvidenceDirectory 'vc_redist.arm64.exe'
    Invoke-WebRequest -Uri 'https://aka.ms/vc14/vc_redist.arm64.exe' -OutFile $redist -TimeoutSec 60 -MaximumRedirection 3
    $signature = Get-AuthenticodeSignature -LiteralPath $redist
    if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch '(?:^|, )O=Microsoft Corporation(?:,|$)') { throw 'The ARM64 Microsoft runtime installer signature is invalid.' }
    $process = Start-Process -FilePath $redist -ArgumentList @('/install', '/quiet', '/norestart') -WindowStyle Hidden -PassThru
    try {
        if (-not $process.WaitForExit(120000)) {
            $process.Kill($true)
            [void]$process.WaitForExit(5000)
            throw 'Microsoft runtime installation exceeded its two-minute deadline.'
        }
        $runtimeExitCode = $process.ExitCode
        if ($runtimeExitCode -notin @(0, 1638, 3010)) { throw "Microsoft runtime installation failed: $runtimeExitCode" }
    } finally { $process.Dispose() }
    @{
        uri = 'https://aka.ms/vc14/vc_redist.arm64.exe'
        sha256 = (Get-FileHash -LiteralPath $redist -Algorithm SHA256).Hash.ToLowerInvariant()
        signer = $signature.SignerCertificate.Subject; exitCode = $runtimeExitCode
    } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $EvidenceDirectory 'vc-runtime.json') -Encoding utf8NoBOM
    # Keep evidence small; do not redistribute Microsoft's prerequisite installer.
    Remove-Item -LiteralPath $redist
}
& (Join-Path $RepositoryRoot 'scripts/release/Test-WindowsInstaller.ps1') -InstallerPath $msi -ExpectedVersion $version -EvidenceDirectory (Join-Path $EvidenceDirectory 'msi-smoke')

$archive = Join-Path $OutputDirectory "SwiftBonsai2Runner-$version-windows-arm64.zip"
Compress-Archive -LiteralPath $package -DestinationPath $archive -CompressionLevel Optimal
Copy-Item -LiteralPath $notices -Destination (Join-Path $OutputDirectory 'THIRD-PARTY-NOTICES.txt')
$assets = @($msi, $archive, (Join-Path $OutputDirectory 'THIRD-PARTY-NOTICES.txt')) | ForEach-Object {
    @{ name = [IO.Path]::GetFileName($_); bytes = (Get-Item -LiteralPath $_).Length; sha256 = (Get-FileHash -LiteralPath $_ -Algorithm SHA256).Hash.ToLowerInvariant() }
}
@{
    schemaVersion = 1; version = $version; tag = $Tag; sourceRevision = $SourceRevision
    target = 'aarch64-pc-windows-msvc'; toolchain = 'nightly-2026-11-09-aarch64-pc-windows-msvc'
    manifestSha256 = $manifestDigest; signed = $false
    runUrl = "https://github.com/$env:GITHUB_REPOSITORY/actions/runs/$env:GITHUB_RUN_ID"
    installerSmoke = 'passed-install-inspect-uninstall-without-model'; builds = $builds; assets = $assets
} | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $OutputDirectory 'release-manifest.json') -Encoding utf8NoBOM
$checksums = @(Get-ChildItem -LiteralPath $OutputDirectory -File | Sort-Object Name | ForEach-Object {
    '{0}  {1}' -f (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant(), $_.Name
})
[IO.File]::WriteAllLines((Join-Path $OutputDirectory 'SHA256SUMS.txt'), $checksums, [Text.UTF8Encoding]::new($false))
