#requires -Version 7.6
<# .SYNOPSIS
Checks concurrent bounded Hexagon pipe capture and entered-stage retention.
#>
[CmdletBinding()]
[OutputType([string])]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.TestSupport.psm1')
Import-Module (Join-Path $PSScriptRoot '../scripts/Bonsai.HexagonCompute.psm1')
$computeModule = Get-Module -Name Bonsai.HexagonCompute
$helpers = @(& $computeModule {
    foreach ($name in @('Assert-Arm64PE','Assert-BonsaiHexagonSessionBuild','Get-BonsaiHexagonDispatchFile','Get-BonsaiHexagonDriverGuard','Invoke-BonsaiHexagonBoundedCapture')) {
        (Get-Command -Name $name -CommandType Function -ErrorAction Stop).Name
    }
})
Assert ($helpers.Count -eq 5) 'The owned worker cannot resolve a required verification helper.'
# The test process itself enters the existing Job gate before its synthetic
# stdout/stderr children run. No fixture, package, verifier or driver loads.
Import-Module (Join-Path $PSScriptRoot '../scripts/Bonsai.Native.psm1')
$modulePath = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../scripts/Bonsai.HexagonDispatch.psm1'))
$command = @'
$ErrorActionPreference = 'Stop'
Import-Module '__MODULE__'
& (Get-Module Bonsai.HexagonDispatch) {
$normal = '[Console]::Out.Write("ok"); [Console]::Error.Write("error")'
$arguments = @('-NoLogo','-NoProfile','-NonInteractive','-EncodedCommand',[Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($normal)))
$result = Invoke-BonsaiHexagonBoundedCapture -Executable (Join-Path $PSHOME 'pwsh.exe') -ArgumentList $arguments -WorkingDirectory ([IO.Path]::GetTempPath())
if ($result.ExitCode -ne 0 -or $result.StandardOutput -cne 'ok' -or $result.StandardError -cne 'error') { throw 'Concurrent stream capture failed.' }
$progressPath = Join-Path ([IO.Path]::GetTempPath()) ('Bonsai-Hexagon-Progress-' + [guid]::NewGuid().ToString('N') + '.json')
try {
    $progress = [ordered]@{ LastFixtureStage = 'not-started' }
    # An invalid vendor label must never enter retained evidence; a fixed
    # marker split across pipe reads must be recovered once its LF arrives.
    $markers = '[Console]::Error.Write("hexagon-fixture:stage=private-path`nhexagon-fixture:stage=con"); Start-Sleep -Milliseconds 30; [Console]::Error.Write("text`n"); exit 2'
    $arguments = @('-NoLogo','-NoProfile','-NonInteractive','-EncodedCommand',[Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($markers)))
    $result = Invoke-BonsaiHexagonBoundedCapture -Executable (Join-Path $PSHOME 'pwsh.exe') -ArgumentList $arguments -WorkingDirectory ([IO.Path]::GetTempPath()) -ProgressReceipt $progress -ProgressPath $progressPath
    $saved = [IO.File]::ReadAllText($progressPath) | ConvertFrom-Json -AsHashtable
    if ($result.ExitCode -ne 2 -or $progress.LastFixtureStage -cne 'context' -or $saved.LastFixtureStage -cne 'context') { throw 'Bounded native progress was not retained.' }
    $progress = [ordered]@{ LastNativeStage = 'not-started'; UnsignedProcessRequested = $null }
    $markers = '[Console]::Error.Write("hexagon-session:stage=private-label`nhexagon-session:stage=reference-process-init-"); Start-Sleep -Milliseconds 30; [Console]::Error.Write("entered`n"); exit 2'
    $arguments = @('-NoLogo','-NoProfile','-NonInteractive','-EncodedCommand',[Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($markers)))
    $result = Invoke-BonsaiHexagonBoundedCapture -Executable (Join-Path $PSHOME 'pwsh.exe') -ArgumentList $arguments -WorkingDirectory ([IO.Path]::GetTempPath()) -ProgressReceipt $progress -ProgressPath $progressPath -ProgressKind Session
    $saved = [IO.File]::ReadAllText($progressPath) | ConvertFrom-Json -AsHashtable
    if ($result.ExitCode -ne 2 -or $saved.LastNativeStage -cne 'reference-process-init-entered' -or $null -ne $saved.UnsignedProcessRequested) { throw 'An entered reference-process call lost its distinct unknown request outcome.' }
} finally { if ([IO.File]::Exists($progressPath)) { [IO.File]::Delete($progressPath) } }
$overflow = '[Console]::Out.Write("x" * 131073)'
$arguments = @('-NoLogo','-NoProfile','-NonInteractive','-EncodedCommand',[Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($overflow)))
$rejected = $false
try { $null = Invoke-BonsaiHexagonBoundedCapture -Executable (Join-Path $PSHOME 'pwsh.exe') -ArgumentList $arguments -WorkingDirectory ([IO.Path]::GetTempPath()) }
catch { $rejected = $_.Exception.Message -like '*fixed byte cap*' }
if (-not $rejected) { throw 'Fixture output overflow was accepted.' }
}
'@
$command = $command.Replace('__MODULE__', $modulePath.Replace("'", "''"))
$result = Invoke-BonsaiNativeProcess -Executable (Join-Path $PSHOME 'pwsh.exe') -ArgumentList @('-NoLogo', '-NoProfile', '-NonInteractive', '-EncodedCommand',
    [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($command))) -WorkingDirectory ([IO.Path]::GetTempPath()) -TimeoutSeconds 5 -OwnDescendants
Assert ($result.ExitCode -eq 0) 'Bounded synthetic capture worker failed.'
'Bounded Hexagon capture checks passed without device access.'
