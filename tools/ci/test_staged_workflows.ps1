#Requires -Version 7.6
param([string]$Case)
. (Join-Path $PSScriptRoot 'test_support.ps1')
. (Join-Path $PSScriptRoot 'check_staged_workflows.ps1')
$checkout='actions/checkout@'+('1'*40); $cache='actions/cache@'+('2'*40)
$workflow='.github/workflows/main.yml'; $action='.github/actions/build/action.yml'; $lockPath='.github/workflows/actions.lock'
function Invoke-FixtureGit([string]$Root,[string[]]$Arguments,[string]$InputText='') { [Text.Encoding]::UTF8.GetString((Invoke-StagedGit $Root $Arguments ([Diagnostics.Stopwatch]::StartNew()) $InputText)) }
function Write-StagedFixtureLock([string]$Root,[string]$Workflow=$workflow,[string]$CheckoutKey=$checkout) {
    $lines=@("version: 'v0.0.3'",'workflows:',"    '${Workflow}':","        - '$CheckoutKey'","        - '$cache'",'dependencies:')
    foreach ($pair in @(@($CheckoutKey,('1'*40)),@($cache,('2'*40)))) { $lines+=@("    '$($pair[0])':","        ref: 'v1.0.0'","        commit: 'sha1-$($pair[1])'",'        owner_id: 1','        repo_id: 2') }
    Write-CiFixture $Root $lockPath (($lines -join "`n")+"`n")
}
function Invoke-StagedFixture([scriptblock]$Work) {
    $scratch=New-CiScratch 'bonsai staged workflows '
    try {
        Invoke-FixtureGit $scratch @('init','--quiet') | Out-Null
        Write-CiFixture $scratch $workflow "jobs:`n  check:`n    steps:`n      - uses: $checkout`n      - uses: `$/.github/actions/build`n"
        Write-CiFixture $scratch $action "runs:`n  using: composite`n  steps:`n    - uses: $cache`n"; Write-StagedFixtureLock $scratch
        Invoke-FixtureGit $scratch @('add','--','.github') | Out-Null
        Invoke-FixtureGit $scratch @('-c','user.name=Workflow Fixture','-c','user.email=fixture@example.invalid','-c',('core.hooksPath='+(Join-Path $scratch 'no-hooks')),'-c','commit.gpgsign=false','commit','--quiet','-m','fixture baseline') | Out-Null
        & $Work $scratch
    } finally { Remove-CiScratch $scratch }
}
function Add-StagedFixtureChange([string]$Root) { [IO.File]::AppendAllText((Join-Path $Root $workflow),"# staged edit`n"); Invoke-FixtureGit $Root @('add','--',$workflow) | Out-Null }
function Assert-StagedFailure([string]$Root,[string]$Phrase) { $result=Test-StagedWorkflows $Root; Assert-Ci $result.Selected; Assert-Ci (@($result.Errors | Where-Object { $_.Contains($Phrase) }).Count -gt 0) ($result.Errors -join "`n") }
function Assert-StagedPass([string]$Root,[bool]$Selected=$true) { $result=Test-StagedWorkflows $Root; Assert-CiEqual $result.Selected $Selected; Assert-CiEqual $result.Errors @() }
$cases=[ordered]@{
 unrelated_changes_skip_unstaged_broken_workflow={ Invoke-StagedFixture { param($r) Write-CiFixture $r 'readme.txt' 'ordinary edit'; Invoke-FixtureGit $r @('add','--','readme.txt') | Out-Null; Write-CiFixture $r $workflow 'uses: actions/checkout@main'; Assert-StagedPass $r $false } }
 valid_snapshot_ignores_unstaged_bad_lock={ Invoke-StagedFixture { param($r) Add-StagedFixtureChange $r; Write-CiFixture $r $lockPath 'invalid working-tree lock'; Assert-StagedPass $r } }
 unstaged_repair_cannot_hide_bad_staged_lock={ Invoke-StagedFixture { param($r) Write-StagedFixtureLock $r $workflow 'actions/checkout@v1.0.0'; Invoke-FixtureGit $r @('add','--',$lockPath) | Out-Null; Write-StagedFixtureLock $r; Assert-StagedFailure $r "lockfile missing exact uses key $checkout" } }
 staged_composite_requires_matching_closure={ Invoke-StagedFixture { param($r) Write-CiFixture $r $action ('uses: actions/upload-artifact@'+('3'*40)); Invoke-FixtureGit $r @('add','--',$action) | Out-Null; Assert-StagedFailure $r 'lockfile missing exact uses key actions/upload-artifact@' } }
 deleted_lock_fails_despite_worktree_copy={ Invoke-StagedFixture { param($r) Invoke-FixtureGit $r @('rm','--cached','--',$lockPath) | Out-Null; Assert-StagedFailure $r 'actions.lock' } }
 deleted_workflow_requires_pruning_lock={ Invoke-StagedFixture { param($r) Invoke-FixtureGit $r @('rm','--',$workflow) | Out-Null; Assert-StagedFailure $r 'no workflows found' } }
 renamed_workflow_requires_matching_names={ Invoke-StagedFixture { param($r) $renamed='.github/workflows/workflow with spaces.yml'; Invoke-FixtureGit $r @('mv','--',$workflow,$renamed) | Out-Null; Assert-StagedFailure $r "missing workflow $renamed"; Write-StagedFixtureLock $r $renamed; Invoke-FixtureGit $r @('add','--',$lockPath) | Out-Null; Assert-StagedPass $r } }
 local_composite_outside_github_checked={ Invoke-StagedFixture { param($r) $outside='ci/build/action.yaml'; Write-CiFixture $r $outside ([IO.File]::ReadAllText((Join-Path $r $action))); Invoke-FixtureGit $r @('rm','--',$action) | Out-Null; Write-CiFixture $r $workflow "uses: $checkout`n  - uses: `$/ci/build`n"; Invoke-FixtureGit $r @('add','--',$outside,$workflow) | Out-Null; Assert-StagedPass $r } }
 guard_changes_select_index_policy={ Invoke-StagedFixture { param($r) Write-CiFixture $r '.githooks/pre-commit' "#!/bin/sh`nexit 0`n"; Invoke-FixtureGit $r @('add','--','.githooks/pre-commit') | Out-Null; Assert-StagedPass $r } }
 deleted_composite_cannot_use_worktree={ Invoke-StagedFixture { param($r) Invoke-FixtureGit $r @('rm','--cached','--',$action) | Out-Null; Assert-StagedFailure $r 'same-repository reference must identify exactly one' } }
 unmerged_manifest_fails_closed={ Invoke-StagedFixture { param($r) $oid=(Invoke-FixtureGit $r @('rev-parse',(':'+$action))).Trim(); Invoke-FixtureGit $r @('rm','--cached','--',$action) | Out-Null; $conflict=(@(1,2,3) | ForEach-Object { "100644 $oid $_`t$action`0" }) -join ''; Invoke-FixtureGit $r @('update-index','-z','--index-info') $conflict | Out-Null; Assert-StagedFailure $r 'unmerged staged workflow input' } }
 symlink_manifest_fails_closed={ Invoke-StagedFixture { param($r) $oid=(Invoke-FixtureGit $r @('hash-object','-w','--stdin') "elsewhere.yml`n").Trim(); Invoke-FixtureGit $r @('update-index','--add','--cacheinfo','120000',$oid,$action) | Out-Null; Assert-StagedFailure $r 'must be a regular file' } }
 snapshot_preserves_index_and_worktree={ Invoke-StagedFixture { param($r) Add-StagedFixtureChange $r; Write-CiFixture $r $lockPath 'unstaged content retained'; $before=Invoke-FixtureGit $r @('ls-files','--stage','-z'); Assert-StagedPass $r; Assert-CiEqual (Invoke-FixtureGit $r @('ls-files','--stage','-z')) $before; Assert-CiEqual ([IO.File]::ReadAllText((Join-Path $r $lockPath))) 'unstaged content retained' } }
 git_failure_blocks_instead_of_skipping={ $scratch=New-CiScratch; try { $saved=$env:PATH; $env:PATH=''; $result=Test-StagedWorkflows $scratch; Assert-Ci $result.Selected; Assert-Ci ($result.Errors.Count -gt 0) } finally { $env:PATH=$saved; Remove-CiScratch $scratch } }
 non_repository_fails_closed={ $scratch=New-CiScratch; try { Assert-StagedFailure $scratch 'git diff failed' } finally { Remove-CiScratch $scratch } }
 initial_repository_is_checked={ $scratch=New-CiScratch; try { Invoke-FixtureGit $scratch @('init','--quiet') | Out-Null; Write-CiFixture $scratch $workflow "uses: $checkout`n"; Invoke-FixtureGit $scratch @('add','--',$workflow) | Out-Null; Assert-StagedFailure $scratch 'actions.lock' } finally { Remove-CiScratch $scratch } }
}
Invoke-CiCases $cases $Case
