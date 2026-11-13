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
# Require the GUI subsystem before launching the image: hiding an already-created
# console would still flash a terminal during ordinary Explorer startup.
$reader = [IO.BinaryReader]::new([IO.File]::OpenRead($ExecutablePath))
try {
    if ($reader.ReadUInt16() -ne 0x5a4d) { throw 'Invalid executable DOS header.' }
    $reader.BaseStream.Position = 0x3c
    $pe = $reader.ReadUInt32()
    if ($pe -lt 64 -or [int64]$pe + 94 -gt $reader.BaseStream.Length) { throw 'Invalid executable PE offset.' }
    $reader.BaseStream.Position = $pe
    if ($reader.ReadUInt32() -ne 0x4550 -or $reader.ReadUInt16() -ne 0xaa64) { throw 'Expected a native ARM64 PE image.' }
    $reader.BaseStream.Position = $pe + 24
    if ($reader.ReadUInt16() -ne 0x20b) { throw 'Expected a PE32+ application.' }
    $reader.BaseStream.Position = $pe + 24 + 68
    if ($reader.ReadUInt16() -ne 2) { throw 'The bundled application must use the Windows GUI subsystem.' }
} finally { $reader.Dispose() }
$EvidenceDirectory = Resolve-ReleasePath $EvidenceDirectory -AllowMissing
if (Test-Path -LiteralPath $EvidenceDirectory) { throw 'Executable smoke evidence must use a new directory.' }
[void][IO.Directory]::CreateDirectory($EvidenceDirectory)
$installation = Join-Path $EvidenceDirectory 'extracted'
$checked = $false
$smokeFailure = $null
try {
    $request = @{
        operation = 'extract'; executablePath = $ExecutablePath; installationDirectory = $installation
    } | ConvertTo-Json -Compress
    $bundle = Invoke-ReleaseCommand $ExecutablePath @('chat-bundle', '--json', $request) -TimeoutSeconds 60 | ConvertFrom-Json
    if ($bundle.ManifestSha256 -cne $ManifestSha256 -or $bundle.Manifest.target -cne 'aarch64-pc-windows-msvc' -or
        $bundle.Manifest.schemaVersion -ne 2 -or @($bundle.Manifest.files).Count -ne 8) {
        throw 'The single executable has an unexpected package identity or inventory.'
    }
    $tool = Resolve-ReleasePath (Join-Path $bundle.Directory 'bin/bonsai-tools.exe') -Kind File
    $request = @{ operation = 'installed'; installationDirectory = $installation } | ConvertTo-Json -Compress
    $package = Invoke-ReleaseCommand $tool @('chat-package', '--json', $request) -TimeoutSeconds 60 | ConvertFrom-Json
    if ($package.ManifestSha256 -cne $ManifestSha256 -or $package.Id -cne $bundle.Id) {
        throw 'Extracted application inventory differs from the bundled package.'
    }
    $checked = $true
} catch {
    $smokeFailure = $_
    throw
} finally {
    Remove-ReleaseDirectory -Path $installation -ParentDirectory $EvidenceDirectory -LeafName 'extracted' -PrimaryFailure $smokeFailure
}
if (-not $checked) { throw 'The single executable smoke check did not complete.' }
[ordered]@{
    schemaVersion = 1; version = $bundle.Manifest.version; sourceRevision = $bundle.Manifest.sourceRevision
    manifestSha256 = $ManifestSha256; executableSha256 = (Get-FileHash -LiteralPath $ExecutablePath -Algorithm SHA256).Hash.ToLowerInvariant()
    guiSubsystemVerified = $true; inventoryVerified = $true; extractionVerified = $true; maintenanceExecutableStarted = $true
    extractedFilesRemoved = $true; modelStarted = $false; softwareInstalled = $false
    actualModelAcceptance = 'pending'; cleanMachineAcceptance = 'pending'
} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $EvidenceDirectory 'smoke-receipt.json') -Encoding utf8NoBOM
