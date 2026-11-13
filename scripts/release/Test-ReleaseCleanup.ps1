#Requires -Version 7.6
# Offline Windows file-lock controls; no application, network or installation work.
[CmdletBinding()]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'Release-Common.ps1')
if (-not $IsWindows) { throw 'Release cleanup controls require Windows file-sharing semantics.' }
if (-not ('SwiftBonsai.ReleaseCleanupLock' -as [type])) {
    Add-Type -TypeDefinition @'
using System.IO;
using System.Threading;
using System.Threading.Tasks;
namespace SwiftBonsai {
    public static class ReleaseCleanupLock {
        public static Task ReleaseAfter(FileStream stream, int milliseconds) {
            return Task.Run(() => {
                Thread.Sleep(milliseconds);
                stream.Dispose();
            });
        }
    }
}
'@
}
$parent = Resolve-ReleasePath ([IO.Path]::GetTempPath())
$leaf = 'SwiftBonsai-Cleanup-Controls-' + [guid]::NewGuid().ToString('N')
$fixture = Join-Path $parent $leaf
[void][IO.Directory]::CreateDirectory($fixture)
$extracted = Join-Path $fixture 'extracted'
$redirect = Join-Path $fixture 'redirect'
$nestedRedirect = Join-Path $extracted 'redirect'
$lock = $null
$release = $null
function New-LockedCleanupFixture {
    [void][IO.Directory]::CreateDirectory($extracted)
    $path = Join-Path $extracted 'bonsai-tools.exe'
    [IO.File]::WriteAllText($path, 'Inert locked file; never execute.')
    [IO.File]::Open($path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
}
try {
    foreach ($code in @(-2147024864, -2147024863)) {
        $failure = [IO.IOException]::new('Synthetic Windows sharing/lock violation.', $code)
        $wrapped = [Exception]::new('Provider wrapper.', $failure)
        if (-not (Test-ReleaseSharingViolation $wrapped)) { throw 'A Windows sharing/lock violation was not retryable.' }
    }
    foreach ($failure in @(
        [IO.IOException]::new('Access denied.', -2147024891)
        [IO.IOException]::new('Missing file.', -2147024894)
        [IO.IOException]::new('Unrelated HRESULT with the same low bits.', -2147483616)
        [UnauthorizedAccessException]::new('Permission failure.')
    )) {
        if (Test-ReleaseSharingViolation $failure) { throw 'Cleanup would retry an unrelated or permission failure.' }
    }
    # The real handle excludes delete sharing until a .NET worker releases it.
    $lock = New-LockedCleanupFixture
    $release = [SwiftBonsai.ReleaseCleanupLock]::ReleaseAfter($lock, 300)
    Remove-ReleaseDirectory -Path $extracted -ParentDirectory $fixture -LeafName 'extracted' -TimeoutMilliseconds 3000
    $release.GetAwaiter().GetResult()
    $lock.Dispose()
    $lock = $null
    if (Test-Path -LiteralPath $extracted) { throw 'Delayed sharing release did not complete cleanup.' }

    $lock = New-LockedCleanupFixture
    $clock = [Diagnostics.Stopwatch]::StartNew()
    $failed = $false
    $receipt = Join-Path $fixture 'unexpected-success.json'
    try {
        Remove-ReleaseDirectory -Path $extracted -ParentDirectory $fixture -LeafName 'extracted' -TimeoutMilliseconds 200
        [IO.File]::WriteAllText($receipt, 'Cleanup success must precede any receipt.')
    } catch { $failed = $true }
    if (-not $failed -or $clock.ElapsedMilliseconds -gt 2000 -or
        -not (Test-Path -LiteralPath (Join-Path $extracted 'bonsai-tools.exe')) -or
        (Test-Path -LiteralPath $receipt)) {
        throw 'Persistent sharing failure was unbounded, ignored or recorded as success.'
    }

    $primary = $null
    try { throw 'Original executable smoke failure.' } catch { $primary = $_ }
    $errorSink = [IO.StringWriter]::new()
    $previousError = [Console]::Error
    try {
        [Console]::SetError($errorSink)
        $caught = $null
        try {
            try { throw $primary }
            finally {
                Remove-ReleaseDirectory -Path $extracted -ParentDirectory $fixture -LeafName 'extracted' -TimeoutMilliseconds 100 -PrimaryFailure $primary
            }
        } catch { $caught = $_ }
        if ($null -eq $caught -or $caught.Exception.Message -cne 'Original executable smoke failure.' -or
            -not $caught.Exception.Data.Contains('ReleaseCleanupFailure') -or
            -not $errorSink.ToString().Contains('Release cleanup also failed:')) {
            throw 'Cleanup masked the primary failure or omitted its own diagnostic.'
        }
    } finally {
        [Console]::SetError($previousError)
        $errorSink.Dispose()
    }
    $lock.Dispose()
    $lock = $null
    Remove-ReleaseDirectory -Path $extracted -ParentDirectory $fixture -LeafName 'extracted'

    [void][IO.Directory]::CreateDirectory($extracted)
    $preserved = Join-Path $extracted 'keep.txt'
    [IO.File]::WriteAllText($preserved, 'Unsafe cleanup must leave this file unchanged.')
    & {
        # Override only deletion in this child scope; real path validation still
        # runs. An unrelated provider failure must receive exactly one attempt.
        $observed = @{ Attempts = 0 }
        function Remove-Item {
            [CmdletBinding()]
            param([string]$LiteralPath, [switch]$Recurse, [switch]$Force)
            if ($LiteralPath -cne $extracted -or -not $Recurse -or -not $Force) {
                throw 'Unexpected mocked cleanup request.'
            }
            $observed.Attempts++
            throw [IO.IOException]::new('Synthetic deletion access denied.', -2147024891)
        }
        $caught = $null
        try { Remove-ReleaseDirectory -Path $extracted -ParentDirectory $fixture -LeafName 'extracted' }
        catch { $caught = $_ }
        if ($null -eq $caught -or $observed.Attempts -ne 1 -or
            $caught.Exception.Message -cne 'Synthetic deletion access denied.') {
            throw 'Cleanup retried or changed an unrelated deletion failure.'
        }
    }
    $rejected = 0
    foreach ($invalid in @(
        @{Path = $fixture; ParentDirectory = $fixture; LeafName = 'extracted'}
        @{Path = $extracted; ParentDirectory = $fixture; LeafName = 'other'}
        @{Path = $extracted; ParentDirectory = $parent; LeafName = 'extracted'}
        @{Path = 'relative-cleanup'; ParentDirectory = $fixture; LeafName = 'relative-cleanup'}
        @{Path = $extracted; ParentDirectory = $fixture; LeafName = '..'}
        @{Path = $preserved; ParentDirectory = $extracted; LeafName = 'keep.txt'}
    )) {
        try { Remove-ReleaseDirectory @invalid }
        catch { $rejected++; continue }
        throw 'An unsafe cleanup target was accepted.'
    }
    $null = New-Item -ItemType Junction -Path $redirect -Value $extracted
    try {
        try { Remove-ReleaseDirectory -Path $redirect -ParentDirectory $fixture -LeafName 'redirect' }
        catch { $rejected++ }
        if (-not (Test-Path -LiteralPath $redirect)) { throw 'A redirected cleanup root was removed.' }
    } finally { [IO.Directory]::Delete($redirect) }
    $sibling = Join-Path $fixture 'sibling'
    [void][IO.Directory]::CreateDirectory($sibling)
    $null = New-Item -ItemType Junction -Path $nestedRedirect -Value $sibling
    try {
        try { Remove-ReleaseDirectory -Path $extracted -ParentDirectory $fixture -LeafName 'extracted' }
        catch { $rejected++ }
        if (-not (Test-Path -LiteralPath $nestedRedirect)) { throw 'A redirected cleanup descendant was removed.' }
    } finally { [IO.Directory]::Delete($nestedRedirect) }
    if ($rejected -ne 8 -or -not (Test-Path -LiteralPath $preserved)) {
        throw 'Cleanup target validation did not preserve the private fixture.'
    }
    Remove-ReleaseDirectory -Path $extracted -ParentDirectory $fixture -LeafName 'extracted'
    # Already-removed targets remain an explicitly checked harmless no-op.
    Remove-ReleaseDirectory -Path $extracted -ParentDirectory $fixture -LeafName 'extracted'
    Write-Host 'Release cleanup controls passed: exact sharing/lock error classification, real delayed and persistent sharing locks, bounded failure, no success receipt, primary failure preservation and eight unsafe-target rejections.'
} finally {
    if ($null -ne $release) { $release.GetAwaiter().GetResult() }
    if ($null -ne $lock) { $lock.Dispose() }
    foreach ($junction in @($nestedRedirect, $redirect)) {
        if ((Test-Path -LiteralPath $junction) -and
            ((Get-Item -LiteralPath $junction -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)) {
            [IO.Directory]::Delete($junction)
        }
    }
    Remove-ReleaseDirectory -Path $fixture -ParentDirectory $parent -LeafName $leaf
}
