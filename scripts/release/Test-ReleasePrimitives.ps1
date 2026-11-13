#Requires -Version 7.6
# Model-free controls; never install software or contact a remote service.
[CmdletBinding()]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'Release-Common.ps1')
$repository = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$sources = @(
    (Join-Path $repository 'scripts/Start-Chat.ps1')
    (Join-Path $repository 'scripts/Build-WindowsExecutable.ps1')
    (Get-ChildItem -LiteralPath $PSScriptRoot -Filter '*.ps1' -File).FullName
    (Get-ChildItem -LiteralPath (Join-Path $repository '.github/release') -Filter '*.ps1' -File).FullName
)
foreach ($source in $sources) {
    $tokens = $null
    $parseErrors = $null
    [void][Management.Automation.Language.Parser]::ParseFile($source, [ref]$tokens, [ref]$parseErrors)
    if ($parseErrors.Count -ne 0) { throw "Release PowerShell source does not parse: $source`n$parseErrors" }
}
& (Join-Path $PSScriptRoot 'Test-ReleaseNotices.ps1')
& (Join-Path $PSScriptRoot 'Test-ReleasePolicy.ps1')
& (Join-Path $PSScriptRoot 'Test-PortableReleaseAssets.ps1')
& (Join-Path $PSScriptRoot 'Test-ReleasePublication.ps1')
& (Join-Path $PSScriptRoot 'Test-ReleaseCommandFailures.ps1')
& (Join-Path $PSScriptRoot 'Test-ReleaseCleanup.ps1')
& (Join-Path $PSScriptRoot 'Test-ExecutableBuild.ps1')
$fixture = Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-Release-Controls-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($fixture)
$echoScript = Join-Path $fixture 'arguments.ps1'
$failureScript = Join-Path $fixture 'failure.ps1'
try {
    '[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false); [Console]::Out.Write((ConvertTo-Json -InputObject @($args) -Compress))' | Set-Content -LiteralPath $echoScript -Encoding utf8NoBOM
    'exit 7' | Set-Content -LiteralPath $failureScript -Encoding utf8NoBOM
    $engine = (Get-Process -Id $PID).Path
    $expected = @('path with spaces', '{"quoted":"value"}', 'trailing\', 'semicolon;literal', 'Unicode-β')
    $actual = Invoke-ReleaseCommand $engine (@('-NoLogo', '-NoProfile', '-NonInteractive', '-File', $echoScript) + $expected) | ConvertFrom-Json
    if (@($actual).Count -ne $expected.Count) { throw 'Native argument count changed.' }
    for ($index = 0; $index -lt $expected.Count; $index++) {
        if ($actual[$index] -cne $expected[$index]) { throw "Native argument changed at index $index." }
    }
    $before = $env:BONSAI_RELEASE_ENV_PROBE
    $probe = Invoke-ReleaseCommand $engine @('-NoLogo', '-NoProfile', '-NonInteractive', '-Command', '[Console]::Out.Write($env:BONSAI_RELEASE_ENV_PROBE)') -Environment @{ BONSAI_RELEASE_ENV_PROBE = 'child only' }
    if ($probe -cne 'child only' -or $env:BONSAI_RELEASE_ENV_PROBE -cne $before) { throw 'Child environment isolation failed.' }
    $rejectedExit = $false
    try { Invoke-ReleaseCommand $engine @('-NoLogo', '-NoProfile', '-NonInteractive', '-File', $failureScript) }
    catch {
        if ($_.Exception.Message -notlike '*exit code 7:*') { throw }
        $rejectedExit = $true
    }
    if (-not $rejectedExit) { throw 'A failed native command was accepted.' }
    $rejectedPaths = 0
    foreach ($invalid in @(
        @{Path = 'relative-release-path'}
        @{Path = $echoScript; Kind = 'Directory'}
        @{Path = (Join-Path $fixture 'missing')}
    )) {
        try { $null = Resolve-ReleasePath @invalid }
        catch { $rejectedPaths++; continue }
        throw 'An invalid release path was accepted.'
    }
    $null = Resolve-ReleasePath (Join-Path $fixture 'new-output') -AllowMissing
    [ordered]@{
        schemaVersion = 1; powerShellVersion = $PSVersionTable.PSVersion.ToString(); minimumPowerShellVersion = '7.6'
        parsedSources = $sources.Count; argumentRoundTrip = $true
        rejectedNonzeroExit = $rejectedExit; rejectedPaths = $rejectedPaths
        networkUsed = $false; softwareInstalled = $false
    } | ConvertTo-Json
} finally {
    # Remove only known fixture files, then the empty directory; never recurse.
    Remove-Item -LiteralPath $echoScript, $failureScript -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $fixture -Force
}
