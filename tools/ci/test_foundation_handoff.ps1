#Requires -Version 7.6
param([string]$Case)
. (Join-Path $PSScriptRoot 'test_support.ps1')
$repository=Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$jobs=Get-CiJobBlocks ([IO.File]::ReadAllText((Join-Path $repository '.github/workflows/foundation.yml')))
$config=[IO.File]::ReadAllText((Join-Path $repository '.github/ci/impact.json')) | ConvertFrom-Json -AsHashtable
$cases=[ordered]@{
 consumers_wait_for_checked_cli={
    foreach ($name in @('speech','foundation-tests','release-integration')) { Assert-CiSet (Get-CiDependencies $jobs[$name]) @('changes','checked-tools') }
    foreach ($name in @('checked-tools','maintenance','speech-tts')) { Assert-CiSet (Get-CiDependencies $jobs[$name]) @('changes') }
    $expected=@($config.workflows.foundation.Keys)+@('changes'); Assert-CiSet (Get-CiDependencies $jobs.foundation) $expected; Assert-CiSet $jobs.Keys ($expected+@('foundation')); Assert-Ci (Test-CiAcyclic $jobs)
 }
 missing_producer_and_cycles_fail={ $changed=$jobs.Clone(); $changed.Remove('checked-tools'); Assert-Ci (-not (Test-CiAcyclic $changed)); $changed=$jobs.Clone(); $changed['checked-tools']="`n    needs: speech`n"; Assert-Ci (-not (Test-CiAcyclic $changed)) }
 every_package_has_one_owning_lane={
    $registry=[IO.File]::ReadAllText((Join-Path $repository 'config/rust-packages.json')) | ConvertFrom-Json -AsHashtable; $owned=@()
    foreach ($name in @('maintenance','checked-tools')) {
        Assert-Ci ($jobs[$name] -cmatch '(?m)^          packages: ''(\[.*\])''$'); $packages=@(ConvertFrom-Json $Matches[1]); $group=$config.groups[$(if ($name -ceq 'checked-tools') { 'tools' } else { 'maintenance' })]
        $expected=if ($group.packages.Count -eq 1 -and $group.packages[0] -ceq '*') { @($registry.Packages.Name | Where-Object { $_ -cnotin $group.exclude_packages }) } else { @($group.packages) }
        Assert-CiSet $packages $expected; $owned+=$packages
    }
    Assert-CiEqual $owned.Count @($owned | Sort-Object -Unique).Count; Assert-CiSet $owned @('bonsai-artifacts','bonsai-audio','bonsai-cpu-policy','bonsai-windows-process','bonsai-windows-credentials','bonsai-ci','bonsai-tools')
 }
 cli_checked_before_publication={
    $producer=$jobs['checked-tools']; Assert-Ci ($producer.IndexOf('uses: $/.github/actions/check-rust') -lt $producer.IndexOf('Stage the checked maintenance CLI'))
    foreach ($fragment in @('lane: tools','SchemaVersion = 3','Revision = $env:GITHUB_SHA','NumericSha256 = (Get-FileHash','bonsai-reference-numerics.exe','ProbeSha256 = (Get-FileHash','name: bonsai-tools-windows-arm64-${{ github.sha }}')) { Assert-Ci ($producer.Contains($fragment)) $fragment }
    $consumer=[IO.File]::ReadAllText((Join-Path $repository '.github/actions/use-bonsai-tools/action.yml')); Assert-Ci ($consumer.Contains('name: bonsai-tools-windows-arm64-${{ github.sha }}')); Assert-Ci (-not $consumer.Contains('run-id:')); Assert-Ci ($consumer.Contains('$receipt.Revision -cne $env:GITHUB_SHA'))
 }
 router_and_gate_select_producer={ Assert-CiEqual $config.workflows.foundation['checked-tools'] 'tools'; Assert-CiEqual $config.workflows.foundation.maintenance 'maintenance'; Assert-Ci ('tools' -cnotin $config.groups.maintenance.groups); Assert-Ci ('bonsai-tools' -cin $config.groups.maintenance.exclude_packages); Assert-Ci ('tools' -cin $config.groups.rust_lanes.groups) }
 speech_lanes_keep_independent_results={
    foreach ($pair in @(@('speech','asr'),@('speech-tts','tts'))) { $block=$jobs[$pair[0]]; $lane=$pair[1]; foreach ($fragment in @("contains(fromJSON(needs.changes.outputs.speech_lanes), '$lane')","lane: $lane",("packages: '[`"bonsai-speech-$lane`"]'"))) { Assert-Ci ($block.Contains($fragment)) $fragment }; Assert-CiEqual $config.workflows.foundation[$pair[0]] "speech_$lane" }
    Assert-Ci ($jobs.speech.Contains('uses: $/.github/actions/use-bonsai-tools')); Assert-Ci (-not $jobs['speech-tts'].Contains('bonsai-tools')); Assert-Ci (-not $jobs.speech.Contains('strategy:'))
 }
 only_asr_needs_native_compiler={
    $action=[IO.File]::ReadAllText((Join-Path $repository '.github/actions/check-rust/action.yml')); foreach ($fragment in @("if (`$env:RUST_LANE -ceq 'asr')","if: steps.packages.outputs.has-packages == 'true' && inputs.lane == 'asr'",'--full-tests --build')) { Assert-Ci ($action.Contains($fragment)) $fragment }
    $release=[IO.File]::ReadAllText((Join-Path $repository '.github/workflows/release.yml')); Assert-Ci ($release -cmatch 'uses: \$/\.github/actions/bonsai-tools\r?\n\s+if: matrix.lane == ''asr''\r?\n'); Assert-Ci ([IO.File]::ReadAllText((Join-Path $repository 'tools/ci/src/checks/mod.rs')).Contains('--all-targets'))
 }
 planner_reuses_build_but_runs_regressions={
    $planner=$jobs.changes; foreach ($fragment in @('restore-keys:','ci-tool-v2-${{ runner.os }}-${{ runner.arch }}-${{ env.ImageOS }}-${{ env.ImageVersion }}','~/.cargo/registry/cache',"'rust-toolchain.toml', '.cargo/**'",'cargo +nightly-2026-11-12 test --locked','test_native_ci_cache.ps1','test_tools_cache_inputs.ps1','test_rust_toolchain.ps1')) { Assert-Ci ($planner.Contains($fragment)) $fragment }; Assert-Ci (-not $planner.Contains('cache-hit'))
 }
}
Invoke-CiCases $cases $Case
