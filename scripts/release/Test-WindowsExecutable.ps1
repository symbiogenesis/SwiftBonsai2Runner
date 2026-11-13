#Requires -Version 7.6
# Model-free executable extraction checks; never start the application or install software.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$ExecutablePath,
    [Parameter(Mandatory)][ValidatePattern('^[a-f0-9]{64}$')][string]$ManifestSha256,
    [Parameter(Mandatory)][string]$EvidenceDirectory
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
. (Join-Path $PSScriptRoot 'Release-Common.ps1')
$ExecutablePath = Resolve-ReleasePath $ExecutablePath -Kind File
$EvidenceDirectory = Resolve-ReleasePath $EvidenceDirectory -AllowMissing
if (Test-Path -LiteralPath $EvidenceDirectory) { throw 'Executable smoke evidence must use a new directory.' }
[void][IO.Directory]::CreateDirectory($EvidenceDirectory)
$installation = Join-Path $EvidenceDirectory 'extracted'
$checked = $false
try {
    $request = @{ operation = 'inspect'; executablePath = $ExecutablePath } | ConvertTo-Json -Compress
    $bundle = Invoke-ReleaseCommand $ExecutablePath @('chat-bundle', '--json', $request) -TimeoutSeconds 60 | ConvertFrom-Json
    if ($bundle.ManifestSha256 -cne $ManifestSha256 -or $bundle.Manifest.target -cne 'aarch64-pc-windows-msvc' -or
        $bundle.Manifest.schemaVersion -ne 2 -or @($bundle.Manifest.files).Count -ne 8) {
        throw 'The single executable has an unexpected package identity or inventory.'
    }
    $request = @{
        operation = 'extract'; executablePath = $ExecutablePath; installationDirectory = $installation
    } | ConvertTo-Json -Compress
    $extracted = Invoke-ReleaseCommand $ExecutablePath @('chat-bundle', '--json', $request) -TimeoutSeconds 60 | ConvertFrom-Json
    $tool = Resolve-ReleasePath (Join-Path $extracted.Directory 'bin/bonsai-tools.exe') -Kind File
    $capabilities = Invoke-ReleaseCommand $tool @('--version') -TimeoutSeconds 15 | ConvertFrom-Json
    if ('chat-bundle' -cnotin $capabilities.Commands -or 'installed-chat' -cnotin $capabilities.Commands) {
        throw 'The extracted native maintenance executable is unavailable.'
    }
    $request = @{ operation = 'installed'; installationDirectory = $installation } | ConvertTo-Json -Compress
    $package = Invoke-ReleaseCommand $tool @('chat-package', '--json', $request) -TimeoutSeconds 60 | ConvertFrom-Json
    if ($package.ManifestSha256 -cne $ManifestSha256 -or $package.Id -cne $bundle.Id -or $extracted.Id -cne $bundle.Id) {
        throw 'Extracted application inventory differs from the bundled package.'
    }
    $checked = $true
} finally {
    if (Test-Path -LiteralPath $installation) {
        $resolved = Resolve-ReleasePath $installation
        $parent = [IO.Path]::GetDirectoryName($resolved)
        if ($parent -ine $EvidenceDirectory -or [IO.Path]::GetFileName($resolved) -cne 'extracted') {
            throw 'Refusing cleanup outside the private extraction stage.'
        }
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
}
if (-not $checked) { throw 'The single executable smoke check did not complete.' }
[ordered]@{
    schemaVersion = 1; version = $bundle.Manifest.version; sourceRevision = $bundle.Manifest.sourceRevision
    manifestSha256 = $ManifestSha256; executableSha256 = (Get-FileHash -LiteralPath $ExecutablePath -Algorithm SHA256).Hash.ToLowerInvariant()
    inventoryVerified = $true; extractionVerified = $true; maintenanceExecutableStarted = $true
    extractedFilesRemoved = $true; modelStarted = $false; softwareInstalled = $false
    actualModelAcceptance = 'pending'; cleanMachineAcceptance = 'pending'
} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $EvidenceDirectory 'smoke-receipt.json') -Encoding utf8NoBOM
