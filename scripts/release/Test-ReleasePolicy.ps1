#Requires -Version 7.6
# Model-free release identity fixtures; no network or packaging tools are used.
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
    foreach ($tag in @('', 'v0.1.0', '0.1.0')) {
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
$bare = Resolve-ReleaseIdentity -ManifestText $manifest.Replace('0.1.0', '0.2.0') -Tag '0.2.0'
if ($bare.Tag -cne 'v0.2.0') { throw 'A bare owner-selected version was not normalized to the canonical tag.' }
$accepted++
$maximum = Resolve-ReleaseIdentity -ManifestText $manifest.Replace('0.1.0', '255.255.65535')
if ($maximum.Tag -cne 'v255.255.65535') { throw 'Valid release version boundary was rejected.' }
$accepted++
$rejected = 0
foreach ($case in @(
    @{ManifestText = $manifest; Tag = 'v0.2.0'}
    @{ManifestText = $manifest; Tag = 'V0.1.0'}
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
"Release policy controls passed: $accepted identities and $rejected rejections; no network or packaging tools used."
