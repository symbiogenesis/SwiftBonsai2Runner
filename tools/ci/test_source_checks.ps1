#Requires -Version 7.6
param([string]$Case)
. (Join-Path $PSScriptRoot 'test_support.ps1')
$repository=Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$selector=Join-Path $PSScriptRoot 'select_source_checks.ps1'

function Invoke-SourceSelection([string]$Event,[string]$Ref,[string[]]$Paths=@(),[string]$Range='valid') {
    $scratch=New-CiScratch 'bonsai-source-selection-'
    $names=@('EVENT_NAME','EVENT_REF','BASE_SHA','HEAD_SHA','GITHUB_OUTPUT','GITHUB_STEP_SUMMARY')
    $original=@{}; foreach ($name in $names) { $original[$name]=[Environment]::GetEnvironmentVariable($name) }
    Push-Location $scratch
    try {
        & git init --quiet
        if ($LASTEXITCODE -ne 0) { throw 'Cannot initialize selector fixture.' }
        Write-CiFixture $scratch 'initial.txt' 'baseline'
        & git add --all
        & git -c user.name=Fixture -c user.email=fixture@example.invalid -c core.hooksPath= -c commit.gpgsign=false commit --quiet -m baseline
        if ($LASTEXITCODE -ne 0) { throw 'Cannot commit selector fixture baseline.' }
        $base=(& git rev-parse HEAD).Trim()
        foreach ($path in $Paths) { Write-CiFixture $scratch $path 'changed' }
        & git add --all
        & git -c user.name=Fixture -c user.email=fixture@example.invalid -c core.hooksPath= -c commit.gpgsign=false commit --quiet --allow-empty -m change
        if ($LASTEXITCODE -ne 0) { throw 'Cannot commit selector fixture change.' }
        $head=(& git rev-parse HEAD).Trim()
        $env:EVENT_NAME=$Event; $env:EVENT_REF=$Ref; $env:BASE_SHA=$base; $env:HEAD_SHA=$head
        if ($Range -ceq 'invalid') { $env:BASE_SHA='0'*40 }
        elseif ($Range -ceq 'missing') { $env:BASE_SHA='f'*40 }
        elseif ($Range -ceq 'diverged') {
            # A base-only source edit must not contaminate a docs-only PR diff.
            & git checkout --quiet --detach $base
            Write-CiFixture $scratch 'src/base-only.rs' 'base branch change'
            & git add --all
            & git -c user.name=Fixture -c user.email=fixture@example.invalid -c core.hooksPath= -c commit.gpgsign=false commit --quiet -m base-change
            if ($LASTEXITCODE -ne 0) { throw 'Cannot commit diverged selector baseline.' }
            $env:BASE_SHA=(& git rev-parse HEAD).Trim()
        }
        $env:GITHUB_OUTPUT=Join-Path $scratch 'output'; $env:GITHUB_STEP_SUMMARY=Join-Path $scratch 'summary'
        & $selector
        $summary=[IO.File]::ReadAllText($env:GITHUB_STEP_SUMMARY)
        if ($Range -cin @('valid','diverged') -and $Event -cin @('push','pull_request')) { Assert-Ci ($summary.StartsWith('Selected from the complete changed-path range.')) 'A valid source range must use the actual Git diff.' }
        $result=[ordered]@{}
        foreach ($line in [IO.File]::ReadAllLines($env:GITHUB_OUTPUT)) { $pair=$line -split '=',2; $result[$pair[0]]=($pair[1] -ceq 'true') }
        Assert-CiEqual @($result.Keys) @('portable','markdown','audit')
        return ,$result
    } finally {
        Pop-Location
        foreach ($name in $names) { [Environment]::SetEnvironmentVariable($name,$original[$name]) }
        Remove-CiScratch $scratch
    }
}
function Assert-SourceSelection($Actual,[bool]$Portable,[bool]$Markdown,[bool]$Audit) {
    Assert-CiEqual $Actual ([ordered]@{portable=$Portable;markdown=$Markdown;audit=$Audit})
}
$cases=[ordered]@{
 changed_path_coverage={
    Assert-SourceSelection (Invoke-SourceSelection pull_request refs/pull/1/merge @('docs/guide.md')) $false $true $false
    Assert-SourceSelection (Invoke-SourceSelection pull_request refs/pull/1/merge @('src/broker/request.rs')) $true $false $false
    Assert-SourceSelection (Invoke-SourceSelection pull_request refs/pull/1/merge @('.github/workflows/source-checks.yml')) $true $true $false
    Assert-SourceSelection (Invoke-SourceSelection push refs/heads/main @('Cargo.lock')) $false $false $true
    Assert-SourceSelection (Invoke-SourceSelection push refs/heads/main @('src/broker/Cargo.toml','docs/guide.md')) $false $true $true
    Assert-SourceSelection (Invoke-SourceSelection push refs/heads/main @('.github/actions/rust-toolchain/action.yml')) $false $false $true
 }
 event_eligibility={
    Assert-SourceSelection (Invoke-SourceSelection schedule refs/heads/main) $false $false $true
    Assert-SourceSelection (Invoke-SourceSelection workflow_dispatch refs/heads/main) $true $true $true
    Assert-SourceSelection (Invoke-SourceSelection workflow_dispatch refs/heads/topic) $true $true $false
    Assert-SourceSelection (Invoke-SourceSelection pull_request refs/pull/1/merge) $false $false $false
 }
 nested_lockfile_selects_independent_audit={
    Assert-SourceSelection (Invoke-SourceSelection push refs/heads/main @('tools/hexagon/sdk-reader/Cargo.lock')) $false $false $true
    Assert-SourceSelection (Invoke-SourceSelection push refs/heads/main @('tools/future-reader/Cargo.lock')) $false $false $true
    Assert-SourceSelection (Invoke-SourceSelection push refs/heads/main @('docs/Cargo.lock.md')) $false $true $false
 }
 every_workspace_lock_is_audited_after_root_failure={
    $workflow=[IO.File]::ReadAllText((Join-Path $repository '.github/workflows/source-checks.yml'))
    Assert-Ci ($workflow.Contains("      - '**/Cargo.lock'")) 'Nested lockfile changes must trigger Source checks.'
    $audit=(Get-CiJobBlocks $workflow)['audit']
    $commands=[regex]::Matches($audit,'audit --file "\$GITHUB_WORKSPACE/([^\"]+)" --deny yanked')
    Assert-CiSet @($commands | ForEach-Object { $_.Groups[1].Value }) @('Cargo.lock','tools/hexagon/sdk-reader/Cargo.lock')
    Assert-Ci ($audit.Contains('        id: workspace-audit')) 'The first audit must expose its outcome.'
    Assert-Ci ($audit.Contains("        if: `${{ !cancelled() && (steps.workspace-audit.outcome == 'success' || steps.workspace-audit.outcome == 'failure') }}")) 'The independent audit must run after either workspace-audit result.'
    Assert-Ci (-not $audit.Contains('continue-on-error: true')) 'Audit failures must still fail the job.'
 }
 unknown_ranges_fail_closed={
    foreach ($range in @('invalid','missing')) {
        Assert-SourceSelection (Invoke-SourceSelection push refs/heads/main @('docs/guide.md') $range) $false $true $true
        Assert-SourceSelection (Invoke-SourceSelection pull_request refs/pull/1/merge @('docs/guide.md') $range) $true $true $false
    }
 }
 pull_request_uses_merge_base={
    Assert-SourceSelection (Invoke-SourceSelection pull_request refs/pull/1/merge @('docs/guide.md') diverged) $false $true $false
 }
 handoff_preserves_incremental_push_coverage={
    $jobs=Get-CiJobBlocks ([IO.File]::ReadAllText((Join-Path $repository '.github/workflows/source-checks.yml')))
    Assert-CiSet $jobs.Keys @('actions-lock','portable','markdown','audit')
    $preflight=$jobs['actions-lock']
    Assert-Ci ($preflight.Contains("github.event_name == 'pull_request' && 'latest' || github.run_id")) 'Incremental push preflights must keep independent handoffs.'
    Assert-Ci ($preflight.Contains("cancel-in-progress: `${{ github.event_name == 'pull_request' }}")) 'Only cumulative PR preflights may supersede each other.'
    Assert-Ci ($preflight.Contains('fetch-depth: 0'))
    foreach ($name in @('portable','markdown','audit')) {
        Assert-CiEqual @(Get-CiDependencies $jobs[$name]) @('actions-lock')
        Assert-Ci ($jobs[$name].Contains("needs.actions-lock.result != 'success'")) "Failed preflight cannot silently skip $name."
        Assert-Ci ($jobs[$name].Contains('          exit 1'))
        Assert-Ci ($jobs[$name].Contains("github.event_name == 'pull_request' && 'latest' || github.run_id")) "An older push preflight must not supersede a newer $name check."
        Assert-Ci ($jobs[$name].Contains("cancel-in-progress: `${{ github.event_name == 'pull_request' }}")) "Only cumulative PR checks may supersede $name."
    }
 }
}
Invoke-CiCases $cases $Case
