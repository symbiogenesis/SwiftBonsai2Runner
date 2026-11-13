#Requires -Version 7.6
# Pure release identity policy; importing this file performs no external work.
Set-StrictMode -Version Latest

function Resolve-ReleaseIdentity {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$ManifestText,
        [AllowEmptyString()][string]$Tag = ''
    )
    $section = [regex]::Match($ManifestText, '(?ms)^\[workspace\.package\][ \t]*\r?\n(.*?)(?=^\[|\z)')
    $versions = [regex]::Matches($section.Groups[1].Value, '(?m)^version[ \t]*=[ \t]*"([^"\r\n]+)"[ \t]*(?:#[^\r\n]*)?\r?$')
    if (-not $section.Success -or $versions.Count -ne 1) { throw 'Cargo.toml must declare one workspace.package.version.' }
    $version = $versions[0].Groups[1].Value
    if ($version -cnotmatch '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$') { throw "Workspace version '$version' must use canonical MAJOR.MINOR.PATCH." }
    $parts = $version.Split('.') | ForEach-Object { [uint64]$_ }
    if ($parts[0] -gt 255 -or $parts[1] -gt 255 -or $parts[2] -gt 65535) { throw "Workspace version '$version' exceeds the supported application version bounds." }
    $expected = "v$version"
    if (-not $Tag) { $Tag = $expected }
    if ($Tag -cmatch '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$') { $Tag = "v$Tag" }
    if ($Tag -cnotmatch '^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$') { throw 'Release tag must use vMAJOR.MINOR.PATCH without a prerelease suffix.' }
    if ($Tag -cne $expected) { throw "Release tag '$Tag' does not match workspace version '$version'; expected '$expected'. Omit the tag to use the workspace version, or update the workspace version first." }
    [pscustomobject]@{ Version = $version; Tag = $Tag }
}
