#Requires -Version 7.6
param([string]$Case)
. (Join-Path $PSScriptRoot 'test_support.ps1')
. (Join-Path $PSScriptRoot 'check_workflow_startup.ps1')
$checkout='actions/checkout@'+('1'*40); $cache='actions/cache@'+('2'*40)
function Write-StartupFixtureLock([string]$Root,[string]$CheckoutKey=$checkout,[bool]$IncludeCache=$true) {
    $keys=@($CheckoutKey); if ($IncludeCache) { $keys+=$cache }
    $lines=@("version: 'v0.0.3'",'workflows:',"    '.github/workflows/main.yml':")
    $lines+=@($keys | ForEach-Object { "        - '$_'" }); $lines+='dependencies:'
    foreach ($key in $keys) { $sha=if ($key -ceq $CheckoutKey) { '1'*40 } else { '2'*40 }; $lines+=@("    '${key}':","        ref: 'v1.0.0'","        commit: 'sha1-$sha'",'        owner_id: 1','        repo_id: 2') }
    Write-CiFixture $Root '.github/workflows/actions.lock' (($lines -join "`n")+"`n")
}
function Invoke-StartupFixture([scriptblock]$Work) {
    $fixture=New-CiScratch 'bonsai-workflow-startup-'
    try {
        Write-CiFixture $fixture '.github/workflows/main.yml' "jobs:`n  check:`n    steps:`n      - uses: $checkout`n      - uses: `$/.github/actions/build`n"
        Write-CiFixture $fixture '.github/actions/build/action.yml' "runs:`n  using: composite`n  steps:`n    - uses: `$/.github/actions/cache`n"
        Write-CiFixture $fixture '.github/actions/cache/action.yaml' "runs:`n  using: composite`n  steps:`n    - uses: $cache`n"
        Write-StartupFixtureLock $fixture; & $Work $fixture
    } finally { Remove-CiScratch $fixture }
}
function Assert-StartupFailure([string]$Root,[string]$Phrase) { $failures=Test-WorkflowStartup $Root; Assert-Ci (@($failures | Where-Object { $_.Contains($Phrase) }).Count -gt 0) ($failures -join "`n") }
$cases=[ordered]@{
 exact_sha_keys_and_nested_composite_pass={ Invoke-StartupFixture { param($r) Assert-CiEqual (Test-WorkflowStartup $r) @() } }
 tag_key_does_not_cover_sha_uses={ Invoke-StartupFixture { param($r) Write-StartupFixtureLock $r 'actions/checkout@v1.0.0'; Assert-StartupFailure $r "lockfile missing exact uses key $checkout" } }
 nested_dependency_requires_coverage={ Invoke-StartupFixture { param($r) Write-StartupFixtureLock $r $checkout $false; Assert-StartupFailure $r "lockfile missing exact uses key $cache" } }
 mutable_inline_reference_fails={ Invoke-StartupFixture { param($r) Write-CiFixture $r '.github/actions/cache/action.yaml' "uses: actions/cache@v1`n"; Assert-StartupFailure $r 'full 40-character commit SHA' } }
 lock_commit_must_agree={ Invoke-StartupFixture { param($r) $path=Join-Path $r '.github/workflows/actions.lock'; [IO.File]::WriteAllText($path,[IO.File]::ReadAllText($path).Replace('sha1-'+('1'*40),'sha1-'+('9'*40))); Assert-StartupFailure $r 'commit metadata disagrees' } }
 missing_and_stale_workflows_fail={ Invoke-StartupFixture { param($r) Write-CiFixture $r '.github/workflows/additional.yaml' "uses: $checkout`n"; Assert-StartupFailure $r 'missing workflow .github/workflows/additional.yaml'; [IO.File]::Delete((Join-Path $r '.github/workflows/main.yml')); Assert-StartupFailure $r 'stale workflow .github/workflows/main.yml' } }
 missing_local_action_and_cycle_fail={ Invoke-StartupFixture { param($r) [IO.File]::Delete((Join-Path $r '.github/actions/cache/action.yaml')); Assert-StartupFailure $r 'same-repository reference must identify exactly one'; Write-CiFixture $r '.github/actions/cache/action.yaml' "uses: `$/.github/actions/build`n"; Assert-StartupFailure $r 'dependency cycle' } }
 block_scripts_are_not_references={ Invoke-StartupFixture { param($r) [IO.File]::AppendAllText((Join-Path $r '.github/workflows/main.yml'),"      - run: |`n          uses: actions/not-an-action@main`n      - run: >-`n          uses: actions/also-not-an-action@main`n"); Assert-CiEqual (Test-WorkflowStartup $r) @() } }
 quoted_uses_and_comments_checked={ Invoke-StartupFixture { param($r) Write-CiFixture $r '.github/actions/cache/action.yaml' "'uses': '$cache' # pinned`n"; Assert-CiEqual (Test-WorkflowStartup $r) @(); Write-CiFixture $r '.github/actions/cache/action.yaml' '"uses": "actions/cache@main"'; Assert-StartupFailure $r 'full 40-character commit SHA' } }
 flow_and_multiline_uses_fail_closed={ Invoke-StartupFixture { param($r) Write-CiFixture $r '.github/actions/cache/action.yaml' "{uses: $cache}`n"; Assert-StartupFailure $r 'write uses as a block mapping'; Write-CiFixture $r '.github/actions/cache/action.yaml' "uses: >-`n  actions/cache@main`n"; Assert-StartupFailure $r 'unsupported uses syntax' } }
 malformed_duplicate_lock_fail_closed={ Invoke-StartupFixture { param($r) $path=Join-Path $r '.github/workflows/actions.lock'; $original=[IO.File]::ReadAllText($path); [IO.File]::WriteAllText($path,$original.Replace("version: 'v0.0.3'","version: 'future'")); Assert-StartupFailure $r 'expected complete generated lockfile schema'; [IO.File]::WriteAllText($path,$original.Replace('        repo_id: 2',"        repo_id: 2`n        repo_id: 3")); Assert-StartupFailure $r 'duplicate dependency field repo_id' } }
 missing_lockfile_fails={ Invoke-StartupFixture { param($r) [IO.File]::Delete((Join-Path $r '.github/workflows/actions.lock')); Assert-StartupFailure $r 'actions.lock' } }
}
Invoke-CiCases $cases $Case
