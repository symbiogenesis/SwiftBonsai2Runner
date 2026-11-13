#Requires -Version 7.6
param([string]$Case)
. (Join-Path $PSScriptRoot 'test_support.ps1')
$repository=Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$workflow=[IO.File]::ReadAllText((Join-Path $repository '.github/workflows/hexagon-qualification.yml')); $jobs=Get-CiJobBlocks $workflow
function Get-HexagonCaches([string]$Job) { return ,@([regex]::Split($Job,'(?m)^      - ') | Where-Object { $_.Contains('uses: actions/cache@') }) }
$cases=[ordered]@{
 independent_controls_and_intake_prerequisites={
    foreach ($name in @('checked-policy','layout-controls','ranking-controls','public-native-abi')) { Assert-CiSet (Get-CiDependencies $jobs[$name]) @() }
    Assert-CiSet (Get-CiDependencies $jobs.controls) @('checked-policy','layout-controls','ranking-controls'); Assert-CiSet (Get-CiDependencies $jobs['artifact-binder']) @('checked-policy'); Assert-Ci (Test-CiAcyclic $jobs)
    foreach ($name in @('session-probe','inventory','dispatch')) { Assert-CiSet (Get-CiDependencies $jobs[$name]) @('controls') }
 }
 missing_policy_and_cycles_rejected={ $changed=$jobs.Clone(); $changed.Remove('checked-policy'); Assert-Ci (-not (Test-CiAcyclic $changed)); $changed=$jobs.Clone(); $changed['checked-policy']="`n    needs: controls`n"; Assert-Ci (-not (Test-CiAcyclic $changed)) }
 policy_checked_before_publication={
    $producer=$jobs['checked-policy']; Assert-CiEqual ([regex]::Matches($workflow,'uses: \$/\.github/actions/bonsai-tools\r?\n')).Count 1
    Assert-Ci ($producer.Contains('--package bonsai-tools --all-features --target-dir $env:BONSAI_TOOLS_TARGET --lib hexagon_')); Assert-Ci ($producer -cnotmatch 'if:.*cache-hit'); Assert-Ci ($producer.IndexOf('--lib hexagon_') -lt $producer.IndexOf('Stage the current source-bound policy executable'))
    foreach ($fragment in @('SchemaVersion = 3','Revision = $env:GITHUB_SHA','NumericSha256 = (Get-FileHash','bonsai-reference-numerics.exe','ProbeSha256 = (Get-FileHash','name: bonsai-tools-windows-arm64-${{ github.sha }}')) { Assert-Ci ($producer.Contains($fragment)) }
    foreach ($name in @('controls','artifact-binder','session-probe','inventory','dispatch')) { Assert-Ci ($jobs[$name].Contains('uses: $/.github/actions/use-bonsai-tools')) }
 }
 all_layout_and_ranking_checks_selected={
    foreach ($script in @('test_pq2_bridge','pq2_bridge','verify_packed_fixture','verify_operation_traffic','verify_operation_budget','verify_resident_traffic')) { Assert-Ci ($jobs['layout-controls'].Contains("tools/hexagon/$script.ps1")) $script }
    Assert-Ci ($jobs['layout-controls'].Contains('inputs.reference_initialization && !inputs.run_session_probe')); foreach ($fragment in @('hardware::ranking::tests','--all-targets -- -D warnings','-- --check')) { Assert-Ci ($jobs['ranking-controls'].Contains($fragment)) }; Assert-Ci ($jobs['ranking-controls'] -cnotmatch 'if:.*cache-hit')
 }
 sdk_reader_has_hosted_fixture_owner={
    $block=$jobs['layout-controls']; foreach ($fragment in @('runs-on: windows-11-arm','uses: $/.github/actions/rust-toolchain','cargo +nightly-2026-11-12-aarch64-pc-windows-msvc test --locked --release','tools/hexagon/sdk-reader/Cargo.toml','Bonsai-Sdk-Reader-Checks','tools/hexagon/test_sdk_functional_review.ps1')) { Assert-Ci ($block.Contains($fragment)) $fragment }; Assert-Ci (-not $block.Contains('self-hosted'))
 }
 caches_exclude_fresh_bundles_and_evidence={
    foreach ($fragment in @("CARGO_UNSTABLE_CHECKSUM_FRESHNESS: 'true'",'CARGO_BUILD_FINGERPRINT: content')) { Assert-Ci ($workflow.Contains($fragment)) }
    foreach ($name in @('checked-policy','ranking-controls','controls','artifact-binder')) { $caches=Get-HexagonCaches $jobs[$name]; Assert-CiEqual $caches.Count 1; $paths=($caches[0] -split 'path: \|',2)[1] -split '          key:',2; Assert-Ci ($paths[0] -notmatch '(?i)(?:bundle|prepared|evidence|receipt|checkedtools)'); Assert-Ci (-not $paths[0].Contains('GITHUB_WORKSPACE')) }
    Assert-Ci ((Get-HexagonCaches $jobs['checked-policy'])[0].Contains('${{ env.BONSAI_TOOLS_TARGET }}'))
    foreach ($fragment in @('BONSAI_TOOLS_SOURCE_ID','BONSAI_TOOLS_CACHE_HOST')) { Assert-Ci ($jobs['checked-policy'].Contains($fragment)) }
    foreach ($fragment in @('needs.checked-policy.outputs.target-cache-key','needs.checked-policy.outputs.image-identity == steps.rust.outputs.image-identity','needs.checked-policy.outputs.compiler-identity == steps.rust.outputs.compiler-identity')) { Assert-Ci ($jobs.controls.Contains($fragment)) }
    foreach ($name in @('ranking-controls','controls','artifact-binder')) { foreach ($fragment in @('steps.rust.outputs.image-identity','steps.rust.outputs.compiler-identity')) { Assert-Ci ($jobs[$name].Contains($fragment)) } }
 }
 native_builds_and_artifacts_fresh={
    foreach ($fragment in @("operation = 'build-dispatch'","BONSAI_TOOLS_TARGET=`$(Join-Path `$env:RUNNER_TEMP 'SwiftBonsai-PackagedTools')")) { Assert-Ci ($jobs.controls.Contains($fragment)) }
    foreach ($fragment in @("operation = 'build-packed-artifact'","buildTargetDirectory = (Join-Path `$env:RUNNER_TEMP 'SwiftBonsai-Hexagon-Artifact-Target')","outputDirectory = (Join-Path `$env:RUNNER_TEMP 'SwiftBonsai-Hexagon-Artifact')")) { Assert-Ci ($jobs['artifact-binder'].Contains($fragment)) }
    foreach ($artifact in @('hexagon-dispatch-fixture','hexagon-hosted-controls','hexagon-native-probe','hexagon-projection-probe','hexagon-packed-artifact','hexagon-packed-artifact-controls','hexagon-windows-abi')) { Assert-Ci ($workflow.Contains('name: '+$artifact+'-${{ github.run_id }}-${{ github.run_attempt }}')) }
    foreach ($name in @('controls','artifact-binder')) { Assert-Ci ($jobs[$name].Contains('if: always()')); Assert-Ci ($jobs[$name] -cnotmatch 'if:.*cache-hit') }
 }
 owner_budget_mutex_same_run_admission={
    foreach ($name in @('session-probe','inventory','dispatch')) { foreach ($fragment in @('timeout-minutes: 5','group: swift-lora-snapdragon','cancel-in-progress: false','BONSAI_HEXAGON_JOB_STARTED_UTC','WholeJobMaximumSeconds = 300')) { Assert-Ci ($jobs[$name].Contains($fragment)) "$name $fragment" } }
    foreach ($name in @('session-probe','dispatch')) { foreach ($fragment in @('$build.RunnerRevision -cne $env:GITHUB_SHA','$build.RunId -cne $env:GITHUB_RUN_ID','$build.RunAttempt -cne $env:GITHUB_RUN_ATTEMPT')) { Assert-Ci ($jobs[$name].Contains($fragment)) } }
 }
}
Invoke-CiCases $cases $Case
