#Requires -Version 7.6
param([Parameter(Mandatory)][string]$ActionlintPath, [string]$Case)
. (Join-Path $PSScriptRoot 'test_support.ps1')
$checker = Join-Path $PSScriptRoot 'check_workflow_syntax.ps1'
function Invoke-SyntaxFixture([string]$Source, [bool]$Reject = $false) {
    $scratch = New-CiScratch 'bonsai-workflow-syntax-'
    try {
        [void][IO.Directory]::CreateDirectory((Join-Path $scratch '.git'))
        Write-CiFixture $scratch '.github/workflows/fixture.yml' $Source
        Write-CiFixture $scratch '.github/actions/fixture/action.yml' @'
name: Fixture
description: Inert input contract
inputs:
  known:
    required: false
    description: Known input
runs:
  using: composite
  steps:
    - shell: pwsh
      run: Write-Output fixture
'@
        if ($Reject) { Assert-CiReject { & $checker -Root $scratch -ActionlintPath $ActionlintPath *> $null } }
        else { & $checker -Root $scratch -ActionlintPath $ActionlintPath }
    } finally { Remove-CiScratch $scratch }
}
$valid = @'
name: Fixture
on: workflow_dispatch
jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: $/.github/actions/fixture
        with:
          known: value
      - shell: pwsh
        run: |
          # The linter must leave script text alone.
          Write-Output 'uses: $/literal-text'
'@
$cases = [ordered]@{
    same_repository_action_and_script_text_pass = { Invoke-SyntaxFixture $valid }
    quoted_same_repository_action_keys_pass = {
        Invoke-SyntaxFixture ($valid.Replace('uses: $/', "'uses': `$/"))
        Invoke-SyntaxFixture ($valid.Replace('uses: $/', '"uses": $/'))
    }
    syntax_rejects_unknown_job_dependency = { Invoke-SyntaxFixture ($valid.Replace('    runs-on:', "    needs: missing`n    runs-on:")) $true }
    syntax_rejects_invalid_expression = { Invoke-SyntaxFixture ($valid.Replace('known: value', 'known: ${{ github. }}')) $true }
    syntax_checks_normalized_local_action_inputs = {
        Invoke-SyntaxFixture $valid
        Invoke-SyntaxFixture ($valid.Replace('known: value', 'unknown: value')) $true
    }
}
Invoke-CiCases $cases $Case
