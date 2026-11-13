#Requires -Version 7.6
# Model-free release-policy fixtures; no WiX command or license acceptance occurs.
[CmdletBinding()]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'Release-Policy.ps1')
$manifest = @'
[workspace]
members = []
[workspace.package]
version = "0.1.0"
edition = "2024"
[package]
version = "9.9.9"
'@
$manifest = $manifest.Replace("`r`n", "`n")
$accepted = 0
foreach ($text in @($manifest, $manifest.Replace("`n", "`r`n"))) {
    foreach ($tag in @('', 'v0.1.0')) {
        $identity = Resolve-ReleaseIdentity -ManifestText $text -Tag $tag
        if ($identity.Version -cne '0.1.0' -or $identity.Tag -cne 'v0.1.0') { throw 'Release identity changed.' }
        $accepted++
    }
}
$next = Resolve-ReleaseIdentity -ManifestText $manifest.Replace('0.1.0', '0.1.1')
if ($next.Tag -cne 'v0.1.1') { throw 'An omitted tag did not follow the new workspace version.' }
$accepted++
$requested = Resolve-ReleaseIdentity -ManifestText $manifest.Replace('0.1.0', '0.2.0') -Tag 'v0.2.0'
if ($requested.Version -cne '0.2.0' -or $requested.Tag -cne 'v0.2.0') { throw 'The owner-selected v0.2.0 release was rejected.' }
$accepted++
$maximum = Resolve-ReleaseIdentity -ManifestText $manifest.Replace('0.1.0', '255.255.65535')
if ($maximum.Tag -cne 'v255.255.65535') { throw 'Valid MSI version boundary was rejected.' }
$accepted++
$rejected = 0
foreach ($case in @(
    @{ManifestText = $manifest; Tag = 'v0.2.0'}
    @{ManifestText = $manifest; Tag = '0.1.0'}
    @{ManifestText = $manifest; Tag = 'v0.1.0-rc.1'}
    @{ManifestText = $manifest; Tag = 'v0.1.0 '}
    @{ManifestText = $manifest.Replace('[workspace.package]', '[other]')}
    @{ManifestText = $manifest.Replace('version = "0.1.0"', "version = `"0.1.0`"`nversion = `"0.1.0`"")}
    @{ManifestText = $manifest.Replace('0.1.0', '0.01.0')}
    @{ManifestText = $manifest.Replace('0.1.0', '256.0.0')}
    @{ManifestText = $manifest.Replace('0.1.0', '1.256.0')}
    @{ManifestText = $manifest.Replace('0.1.0', '1.0.65536')}
)) {
    try { $null = Resolve-ReleaseIdentity @case }
    catch {
        if ($case.ContainsKey('Tag') -and $case.Tag -ceq 'v0.2.0' -and $_.Exception.Message -notlike "*'v0.2.0'*'0.1.0'*'v0.1.0'*") { throw 'The mismatch omitted the requested or expected identity.' }
        $rejected++
        continue
    }
    throw 'An invalid release identity was accepted.'
}
$consentRejected = $false
try { Assert-WixEulaAcceptance }
catch {
    if ($_.Exception.Message -notlike '*https://docs.firegiant.com/wix/osmf/*accept_wix_eula=true*') { throw }
    $consentRejected = $true
}
if (-not $consentRejected) { throw 'Missing owner consent was accepted.' }
# This pure policy function runs no process and does not accept the actual EULA.
Assert-WixEulaAcceptance -AcceptWixEula
"Release policy controls passed: $accepted identities, $rejected rejections, explicit WiX consent gate; no WiX command executed."
