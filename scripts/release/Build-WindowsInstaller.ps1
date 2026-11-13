#Requires -Version 7.6
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$PackageDirectory,
    [Parameter(Mandatory)][ValidatePattern('^[a-f0-9]{64}$')][string]$ManifestSha256,
    [Parameter(Mandatory)][string]$OutputDirectory,
    [Parameter(Mandatory)][string]$WixPath,
    [switch]$AcceptWixEula
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
. (Join-Path $PSScriptRoot 'Release-Common.ps1')
. (Join-Path $PSScriptRoot 'Release-Policy.ps1')
if (-not $IsWindows -or [Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne 'Arm64') {
    throw 'Installer packaging requires Windows ARM64.'
}
Assert-WixEulaAcceptance -AcceptWixEula:$AcceptWixEula
$repositoryRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$PackageDirectory = Resolve-ReleasePath $PackageDirectory
$OutputDirectory = Resolve-ReleasePath $OutputDirectory -AllowMissing
$WixPath = Resolve-ReleasePath $WixPath -Kind File
$toolchain = Get-Content -LiteralPath (Join-Path $repositoryRoot 'installer/windows/toolchain.json') -Raw | ConvertFrom-Json
$null = Invoke-ReleaseCommand $WixPath @('eula', 'accept', 'wix7')
$wixVersion = (Invoke-ReleaseCommand $WixPath @('--version')).Trim()
if ($wixVersion -notmatch ('^' + [regex]::Escape($toolchain.wixVersion) + '(\+|$)')) {
    throw "Installer builds require pinned WiX $($toolchain.wixVersion); found '$wixVersion'."
}
$tool = Join-Path $PackageDirectory 'bin/bonsai-tools.exe'
$request = @{operation = 'inspect'; packageDirectory = $PackageDirectory; manifestSha256 = $ManifestSha256}
$package = Invoke-ReleaseCommand $tool @('chat-package', '--json', ($request | ConvertTo-Json -Compress)) | ConvertFrom-Json
if ($package.Manifest.schemaVersion -ne 2 -or $package.Manifest.target -cne $toolchain.target) {
    throw 'A verified schema-2 Windows ARM64 package is required.'
}
$version = [string]$package.Manifest.version
if ($version -cnotmatch '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$') {
    throw 'MSI versions require three canonical unsigned decimal components.'
}
$parts = $version.Split('.') | ForEach-Object { [uint32]$_ }
if ($parts[0] -gt 255 -or $parts[1] -gt 255 -or $parts[2] -gt 65535) { throw 'Version exceeds Windows Installer limits.' }
[void][IO.Directory]::CreateDirectory($OutputDirectory)
$installerPath = Join-Path $OutputDirectory "SwiftBonsai2Runner-$version-windows-arm64.msi"
if (Test-Path -LiteralPath $installerPath) { throw 'Installer output already exists; releases are immutable.' }
$stage = Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-Msi-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($stage)
try {
    @{
        schemaVersion = 1
        active = @{id = $package.Id; manifestSha256 = $ManifestSha256}
        previous = $null
    } | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $stage 'active.json') -Encoding utf8NoBOM
    Invoke-ReleaseCommand $WixPath @('build', (Join-Path $repositoryRoot 'installer/windows/Package.wxs'),
        '-arch', 'arm64', '-d', "ProductVersion=$version", '-d', "PackageId=$($package.Id)",
        '-d', "PackageDirectory=$PackageDirectory", '-d', "StageDirectory=$stage", '-d', "RepositoryRoot=$repositoryRoot",
        '-intermediateFolder', (Join-Path $stage 'obj'), '-pdbtype', 'none', '-out', $installerPath) -TimeoutSeconds 600
    if (-not (Test-Path -LiteralPath $installerPath -PathType Leaf)) { throw 'WiX MSI build produced no installer.' }
    [ordered]@{
        schemaVersion = 1; version = $version; target = $toolchain.target
        sourceRevision = $package.Manifest.sourceRevision; packageId = $package.Id
        manifestSha256 = $ManifestSha256; wixVersion = $toolchain.wixVersion
        installer = [IO.Path]::GetFileName($installerPath)
        sha256 = (Get-FileHash -LiteralPath $installerPath -Algorithm SHA256).Hash.ToLowerInvariant()
    } | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $OutputDirectory 'installer-receipt.json') -Encoding utf8NoBOM
    $installerPath
} finally {
    $resolved = Resolve-ReleasePath $stage
    $tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\') + '\'
    if (-not $resolved.StartsWith($tempRoot, [StringComparison]::OrdinalIgnoreCase) -or
        [IO.Path]::GetFileName($resolved) -notlike 'SwiftBonsai-Msi-*') { throw 'Refusing cleanup outside the private MSI stage.' }
    Remove-Item -LiteralPath $resolved -Recurse -Force
}
