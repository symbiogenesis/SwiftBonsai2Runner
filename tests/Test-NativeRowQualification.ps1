#requires -Version 7.6
<# .SYNOPSIS
Checks hosted-only row capture preparation without models or compilers.
#>
[CmdletBinding()]
[OutputType([string])]
param([ValidateSet('HostedGuard')][string]$Case = 'HostedGuard')
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.TestSupport.psm1')
$builder = Join-Path $PSScriptRoot '../scripts/Build-NativeRowCaptureTools.ps1'
$output = Join-Path ([IO.Path]::GetTempPath()) ('Bonsai-RowGuard-' + [guid]::NewGuid().ToString('N'))
$savedActions = $env:GITHUB_ACTIONS
$savedEnvironment = $env:RUNNER_ENVIRONMENT
try {
    foreach ($environment in @('local', 'self-hosted')) {
        $env:GITHUB_ACTIONS = if ($environment -ceq 'local') { 'false' } else { 'true' }
        $env:RUNNER_ENVIRONMENT = if ($environment -ceq 'local') { 'github-hosted' } else { 'self-hosted' }
        $failure = Assert-Failure -PassThru { & $builder -OutputDirectory $output }
        Assert ($failure -like '*hosted native Windows ARM64 CI*') 'Row builder accepted a non-hosted environment.'
        Assert (-not (Test-Path -LiteralPath $output)) 'Rejected row builder created output storage.'
    }
} finally {
    [Environment]::SetEnvironmentVariable('GITHUB_ACTIONS', $savedActions)
    [Environment]::SetEnvironmentVariable('RUNNER_ENVIRONMENT', $savedEnvironment)
}
"Focused $Case row qualification checks passed."
