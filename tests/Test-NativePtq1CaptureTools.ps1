#requires -Version 7.6
<# .SYNOPSIS
Checks capture preparation and the Rust collector's source/ABI admission boundary.
#>
[CmdletBinding()]
[OutputType([string])]
param([Parameter(Mandatory)][ValidateSet('HostedGuard', 'BuildEvidence')][string]$Case)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.TestSupport.psm1')
if ($Case -ceq 'BuildEvidence') {
    $tokens = $null; $parseErrors = $null
    $ast = [Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/Build-NativePtq1CaptureTools.ps1'), [ref]$tokens, [ref]$parseErrors)
    Assert ($parseErrors.Count -eq 0) 'Capture builder did not parse.'
    $function = $ast.Find({ param($node)
        $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -ceq 'Save-Ptq1BuildEvidence'
    }, $true)
    Assert ($null -ne $function) 'The bounded evidence writer was not found.'
    . ([scriptblock]::Create($function.Extent.Text))
    $tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
    $scratch = [IO.Path]::GetFullPath((Join-Path $tempRoot ('Bonsai-Ptq1BuildEvidence-' + [guid]::NewGuid().ToString('N'))))
    $evidence = Join-Path $scratch 'evidence'
    [void][IO.Directory]::CreateDirectory($evidence)
    try {
        $buildReceipt = [ordered]@{ RunnerRevision = ('a' * 40); Status = 'failed'; Phase = 'collector-build-and-tests'; ModelProcessStarted = $false }
        [IO.File]::WriteAllText((Join-Path $scratch 'collector-build.txt'), ('x' * 70000) + 'tail')
        [IO.File]::WriteAllText((Join-Path $scratch 'collector-tests.txt'), 'seven model-free controls passed')
        [IO.File]::WriteAllText((Join-Path $scratch 'unselected.txt'), 'must not be uploaded')
        Save-Ptq1BuildEvidence
        $tail = [IO.File]::ReadAllText((Join-Path $evidence 'collector-build.txt'))
        Assert ($tail.Length -eq 65536 -and $tail.EndsWith('tail')) 'Compiler evidence did not preserve its bounded tail.'
        Assert ([IO.File]::ReadAllText((Join-Path $evidence 'collector-tests.txt')) -ceq 'seven model-free controls passed') 'Small collector-test evidence changed.'
        Assert (-not [IO.File]::Exists((Join-Path $evidence 'unselected.txt'))) 'Unselected scratch data entered the retained evidence.'
        $actual = Get-Content -LiteralPath (Join-Path $evidence 'build-receipt.json') -Raw | ConvertFrom-Json -DateKind String
        Assert ($actual.RunnerRevision -ceq ('a' * 40) -and $actual.Status -ceq 'failed' -and
            $actual.Phase -ceq 'collector-build-and-tests' -and -not $actual.ModelProcessStarted) 'Failure receipt lost its identity or scope.'
        $buildReceipt.Failure = 'x' * 65537
        Assert-Failure { Save-Ptq1BuildEvidence }
    } finally {
        $prefix = $tempRoot.TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
        if (-not $scratch.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase) -or $scratch -ceq $tempRoot) {
            throw 'Refusing cleanup outside the generated temporary test directory.'
        }
        Remove-Item -LiteralPath $scratch -Recurse -Force
    }
    return "Focused $Case capture preparation checks passed."
}
$script = Join-Path $PSScriptRoot '../scripts/Build-NativePtq1CaptureTools.ps1'
$output = Join-Path ([IO.Path]::GetTempPath()) ('Bonsai-HostedCaptureGuard-' + [guid]::NewGuid().ToString('N'))
$savedActions = $env:GITHUB_ACTIONS
$savedEnvironment = $env:RUNNER_ENVIRONMENT
try {
    foreach ($environment in @('local', 'self-hosted')) {
        $env:GITHUB_ACTIONS = if ($environment -ceq 'local') { 'false' } else { 'true' }
        $env:RUNNER_ENVIRONMENT = if ($environment -ceq 'local') { 'github-hosted' } else { 'self-hosted' }
        $failure = Assert-Failure -PassThru { & $script -OutputDirectory $output }
        Assert ($failure -like '*hosted native Windows ARM64 CI*') 'Capture build did not reject the non-hosted environment.'
        Assert (-not (Test-Path -LiteralPath $output)) 'Rejected capture preparation created an output directory.'
    }
} finally {
    [Environment]::SetEnvironmentVariable('GITHUB_ACTIONS', $savedActions)
    [Environment]::SetEnvironmentVariable('RUNNER_ENVIRONMENT', $savedEnvironment)
}
"Focused $Case capture preparation checks passed."
