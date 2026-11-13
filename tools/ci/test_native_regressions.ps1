#Requires -Version 7.6
param([string]$Case)
. (Join-Path $PSScriptRoot 'test_support.ps1')
$repository=Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$action=[IO.File]::ReadAllText((Join-Path $repository '.github/actions/native-regressions/action.yml'))
$actionBody=Get-CiActionBody $action
function Invoke-NativeRegressionFixture([object[]]$Selections,[int]$CargoExit=0,[int]$GuardExit=0,[hashtable]$Overrides=@{}) {
    $scratch=New-CiScratch 'bonsai-native-control-'; $original=@{}; foreach ($entry in [Environment]::GetEnvironmentVariables().GetEnumerator()) { $original[$entry.Key]=$entry.Value }
    try {
        $cargo=Join-Path $scratch 'cargo.ps1'; $guard=Join-Path $scratch 'guard.ps1'; $calls=Join-Path $scratch 'cargo.jsonl'; $receipts=Join-Path $scratch 'guard.jsonl'
        [IO.File]::WriteAllText($cargo,@'
ConvertTo-Json -InputObject @($args) -Compress | Add-Content -LiteralPath $env:FIXTURE_INVOCATIONS
'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s'
$global:LASTEXITCODE=[int]$env:FIXTURE_CARGO_EXIT
'@)
        [IO.File]::WriteAllText($guard,@'
$request=$args[2] | ConvertFrom-Json
$args[2] | Add-Content -LiteralPath $env:FIXTURE_RECEIPTS
$global:LASTEXITCODE=if ($request.exitCode -ne 0) { 1 } else { [int]$env:FIXTURE_GUARD_EXIT }
'@)
        $environment=@{REGRESSION_PACKAGE='bonsai-tools';REGRESSION_TARGET_KIND='lib';REGRESSION_TARGET_NAME='';REGRESSION_SELECTIONS=(ConvertTo-Json -InputObject @($Selections) -Compress);REGRESSION_TARGET_DIRECTORY=(Join-Path $scratch 'target');REGRESSION_TOOLCHAIN='nightly-2026-11-12-aarch64-pc-windows-msvc';GITHUB_WORKSPACE=(Join-Path $scratch 'source');BONSAI_CI_PATH=$guard;FIXTURE_CARGO=$cargo;FIXTURE_INVOCATIONS=$calls;FIXTURE_RECEIPTS=$receipts;FIXTURE_CARGO_EXIT=[string]$CargoExit;FIXTURE_GUARD_EXIT=[string]$GuardExit}
        foreach ($key in $Overrides.Keys) { $environment[$key]=$Overrides[$key] }; foreach ($key in $environment.Keys) { [Environment]::SetEnvironmentVariable($key,$environment[$key]) }
        function Get-Command { param($Name,$CommandType,$ErrorAction) [pscustomobject]@{Source=$env:FIXTURE_CARGO} }
        $failure=$null; try { & ([scriptblock]::Create($actionBody)) | Out-Null } catch { $failure=$_.ToString() }
        $invocations=if (Test-Path -LiteralPath $calls) { @([IO.File]::ReadAllLines($calls) | ForEach-Object { ,(ConvertFrom-Json $_ -NoEnumerate) }) } else { @() }
        $proofs=if (Test-Path -LiteralPath $receipts) { @([IO.File]::ReadAllLines($receipts) | ForEach-Object { ConvertFrom-Json $_ -AsHashtable }) } else { @() }
        return @{Failure=$failure; Calls=$invocations; Proofs=$proofs}
    } finally { foreach ($name in @([Environment]::GetEnvironmentVariables().Keys)) { if (-not $original.ContainsKey($name)) { [Environment]::SetEnvironmentVariable($name,$null) } }; foreach ($name in $original.Keys) { [Environment]::SetEnvironmentVariable($name,$original[$name]) }; Remove-CiScratch $scratch }
}
$cases=[ordered]@{
 exact_grouped_arguments_and_typed_counts={
    $result=Invoke-NativeRegressionFixture @(@{name='module::exact_case';exact=$true;'expected-passed'=1},@{name='owner_shutdown_';exact=$false;'expected-passed'=$null})
    Assert-Ci (-not $result.Failure) $result.Failure; Assert-CiEqual $result.Calls.Count 2; Assert-CiEqual @($result.Calls[0][0..3]) @('+nightly-2026-11-12-aarch64-pc-windows-msvc','test','--locked','--release'); Assert-CiEqual @($result.Calls[0][-4..-1]) @('--lib','module::exact_case','--','--exact'); Assert-CiEqual @($result.Calls[1][-2..-1]) @('--lib','owner_shutdown_'); Assert-CiEqual @($result.Proofs.expectedPassed) @(1,$null); Assert-Ci (@($result.Proofs | Where-Object operation -cne 'test-output').Count -eq 0)
 }
 binary_target_without_ignored_tests={ $result=Invoke-NativeRegressionFixture @(@{name='owned_broker_';exact=$false;'expected-passed'=$null}) 0 0 @{REGRESSION_PACKAGE='bonsai-chat';REGRESSION_TARGET_KIND='bin';REGRESSION_TARGET_NAME='bonsai-chat'}; Assert-Ci (-not $result.Failure) $result.Failure; Assert-CiEqual @($result.Calls[0][-3..-1]) @('--bin','bonsai-chat','owned_broker_'); Assert-Ci ('--ignored' -cnotin $result.Calls[0]) }
 complete_selection_validated_before_execution={ $valid=@{name='valid_case';exact=$true;'expected-passed'=1}; foreach ($invalid in @(@{name='--ignored';exact=$true;'expected-passed'=1},@{name='other';exact='true';'expected-passed'=1},@{name='other';exact=$true;'expected-passed'=$null},@{name='other';exact=$false;'expected-passed'=0},$valid)) { $result=Invoke-NativeRegressionFixture @($valid,$invalid); Assert-Ci (-not [string]::IsNullOrEmpty($result.Failure)); Assert-CiEqual $result.Calls.Count 0; Assert-CiEqual $result.Proofs.Count 0 } }
 cargo_failure_reaches_guard_and_stops={ $result=Invoke-NativeRegressionFixture @(@{name='first';exact=$true;'expected-passed'=1},@{name='second';exact=$true;'expected-passed'=1}) 9; Assert-Ci (-not [string]::IsNullOrEmpty($result.Failure)); Assert-CiEqual $result.Calls.Count 1; Assert-CiEqual $result.Proofs[0].exitCode 9 }
 guard_failure_stops_action={ $result=Invoke-NativeRegressionFixture @(@{name='one';exact=$true;'expected-passed'=1}) 0 7; Assert-Ci (-not [string]::IsNullOrEmpty($result.Failure)); Assert-CiEqual $result.Proofs.Count 1 }
}
if ($cases.Count -ne 5) { throw 'The five shared native-control fixtures must remain selected' }
Invoke-CiCases $cases $Case
