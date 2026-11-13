#Requires -Version 7.6
# Offline controls for command diagnostics; no tool installation or network use.
[CmdletBinding()]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'Release-Common.ps1')
$fixture = Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-Command-Controls-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($fixture)
$probe = Join-Path $fixture 'output.ps1'
$engine = (Get-Process -Id $PID).Path
try {
    @'
param([string]$Mode)
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
$unicode = [char]0x03b2
switch ($Mode) {
    success {
        [Console]::Out.Write("exact-$unicode`nsecond line`n")
        [Console]::Error.Write("success diagnostic-$unicode`n")
        exit 0
    }
    failure {
        [Console]::Out.Write("WiX diagnostic-$unicode`n")
        [Console]::Error.Write("stderr diagnostic-$unicode`n")
        exit 17
    }
    empty { exit 19 }
    large {
        [Console]::Out.Write('stdout-head' + ('x' * 12000) + 'stdout-tail')
        [Console]::Error.Write('stderr-head' + ('y' * 12000) + 'stderr-tail')
        exit 23
    }
}
'@ | Set-Content -LiteralPath $probe -Encoding utf8NoBOM
    function Get-RejectedCommandMessage([string]$Mode, [int]$Code) {
        try {
            $null = Invoke-ReleaseCommand $engine @('-NoLogo', '-NoProfile', '-NonInteractive', '-File', $probe, $Mode)
        } catch {
            if ($_.Exception.Message -notlike "*exit code ${Code}:*") { throw }
            return $_.Exception.Message
        }
        throw 'A failed command was accepted.'
    }
    $unicode = [char]0x03b2
    $diagnostic = Get-RejectedCommandMessage 'failure' 17
    if (-not $diagnostic.Contains("stdout: WiX diagnostic-$unicode") -or
        -not $diagnostic.Contains("stderr: stderr diagnostic-$unicode")) {
        throw 'Native stdout/stderr failure diagnostics were lost or changed.'
    }
    $empty = Get-RejectedCommandMessage 'empty' 19
    if (-not $empty.Contains('stdout: <empty>') -or -not $empty.Contains('stderr: <empty>')) {
        throw 'Empty failure output was not explicit.'
    }
    $large = Get-RejectedCommandMessage 'large' 23
    foreach ($marker in @('stdout-head', 'stdout-tail', 'stderr-head', 'stderr-tail', '[... output truncated ...]')) {
        if (-not $large.Contains($marker)) { throw 'Bounded failure output lost its diagnostic context.' }
    }
    if ($large.Length -gt 9000) { throw 'Failure diagnostics exceed their bounded excerpt.' }
    $previousError = [Console]::Error
    $errorSink = [IO.StringWriter]::new()
    try {
        [Console]::SetError($errorSink)
        $actual = Invoke-ReleaseCommand $engine @('-NoLogo', '-NoProfile', '-NonInteractive', '-File', $probe, 'success')
        if ($actual -cne "exact-$unicode`nsecond line`n" -or
            $errorSink.ToString() -cne "success diagnostic-$unicode`n") {
            throw 'Successful command output changed.'
        }
    } finally {
        [Console]::SetError($previousError)
        $errorSink.Dispose()
    }
    Write-Host 'Release command failure controls passed: UTF-8 stdout/stderr, explicit empty output, bounded context and exact success output.'
} finally {
    Remove-Item -LiteralPath $probe -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $fixture -Force
}
