#Requires -Version 7.6
param([string]$Case)
. (Join-Path $PSScriptRoot 'test_support.ps1')
$repository=Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$steps=Get-CiActionSteps ([IO.File]::ReadAllText((Join-Path $repository '.github/actions/rust-toolchain/action.yml')))
function New-ToolchainPhase([string]$Name,[hashtable]$Environment=@{}) { return @{Name=$Name; Environment=$Environment} }
function Invoke-ToolchainFixture([object[]]$Phases) {
    $scratch=New-CiScratch 'bonsai-toolchain-control-'; $original=@{}; foreach ($entry in [Environment]::GetEnvironmentVariables().GetEnumerator()) { $original[$entry.Key]=$entry.Value }
    try {
        $initial=@{ PINNED_TOOLCHAIN='nightly-2026-11-12-aarch64-pc-windows-msvc'; REQUIRED_COMPONENTS='rustfmt,clippy'; EXPECTED_HOST=''; RUNNER_ENVIRONMENT='github-hosted'; ImageOS='fixture-os'; ImageVersion='fixture-image'; GITHUB_OUTPUT=(Join-Path $scratch 'output'); GITHUB_ENV=(Join-Path $scratch 'environment'); FIXTURE_HOME=(Join-Path $scratch 'rustup'); FIXTURE_HOST='aarch64-pc-windows-msvc'; FIXTURE_COMPONENTS='clippy,rustfmt'; FIXTURE_SYSROOT='' }
        foreach ($key in $initial.Keys) { [Environment]::SetEnvironmentVariable($key,$initial[$key]) }; Remove-Item Env:BONSAI_RUST_TOOLCHAIN_VERIFIED_KEY -ErrorAction SilentlyContinue
        function rustup {
            if ($args[0] -ceq 'show' -and $args[1] -ceq 'home') { return $env:FIXTURE_HOME }
            if ($args[0] -ceq 'run' -and $args[-1] -ceq '-vV') { return @('rustc fixture',"host: $env:FIXTURE_HOST") }
            if ($args[0] -ceq 'run' -and $args[-1] -ceq 'sysroot') { if ($env:FIXTURE_SYSROOT) { return $env:FIXTURE_SYSROOT }; return Join-Path (Join-Path $env:FIXTURE_HOME 'toolchains') $env:PINNED_TOOLCHAIN }
            if ($args[0] -ceq 'component') { return @($env:FIXTURE_COMPONENTS -split ',' | Where-Object { $_ }) }
            throw "Unexpected rustup fixture invocation: $args"
        }
        function Read-ToolchainFields([string]$Path) { $fields=@{}; foreach ($line in [IO.File]::ReadAllLines($Path)) { if ($line) { $pair=$line -split '=',2; $fields[$pair[0]]=$pair[1] } }; return $fields }
        $results=@(); $selected=@{}
        foreach ($phase in $Phases) {
            foreach ($key in $phase.Environment.Keys) { [Environment]::SetEnvironmentVariable($key,$phase.Environment[$key]) }
            [IO.File]::WriteAllText($env:GITHUB_OUTPUT,''); [IO.File]::WriteAllText($env:GITHUB_ENV,''); $failure=$null
            try {
                if ($phase.Name -ceq 'paths') { & ([scriptblock]::Create((Get-CiActionBody $steps['Identify the exact toolchain directory and runner image']))) | Out-Null }
                else { $env:REQUIRED_COMPONENTS=$selected.components; $env:EXPECTED_HOST=$selected.host; $env:TOOLCHAIN_PATH=$selected['toolchain-path']; $env:TOOLCHAIN_REUSE_KEY=$selected['reuse-key']; & ([scriptblock]::Create((Get-CiActionBody $steps['Verify the compiler and all required components']))) | Out-Null }
            } catch { $failure=$_.ToString() }
            $outputs=Read-ToolchainFields $env:GITHUB_OUTPUT; $published=Read-ToolchainFields $env:GITHUB_ENV
            foreach ($key in $published.Keys) { [Environment]::SetEnvironmentVariable($key,$published[$key]) }; if ($phase.Name -ceq 'paths' -and -not $failure) { $selected=$outputs }
            $results+=@{Outputs=$outputs; Published=$published; Failure=$failure}
        }
        return ,$results
    } finally { foreach ($name in @([Environment]::GetEnvironmentVariables().Keys)) { if (-not $original.ContainsKey($name)) { [Environment]::SetEnvironmentVariable($name,$null) } }; foreach ($name in $original.Keys) { [Environment]::SetEnvironmentVariable($name,$original[$name]) }; Remove-CiScratch $scratch }
}
$cases=[ordered]@{
 repeat_reuses_only_after_verification={
    $results=Invoke-ToolchainFixture @((New-ToolchainPhase paths),(New-ToolchainPhase paths),(New-ToolchainPhase verify),(New-ToolchainPhase paths @{REQUIRED_COMPONENTS='clippy rustfmt rustfmt';EXPECTED_HOST=''}),(New-ToolchainPhase verify))
    Assert-CiEqual @($results | ForEach-Object { $_.Outputs['reuse-verified'] }) @('false','false',$null,'true',$null); Assert-Ci (@($results | Where-Object Failure).Count -eq 0); Assert-CiSet $results[2].Published.Keys $results[4].Published.Keys; Assert-CiEqual $results[2].Published['BONSAI_RUST_TOOLCHAIN_VERIFIED_KEY'] $results[4].Published['BONSAI_RUST_TOOLCHAIN_VERIFIED_KEY']; Assert-Ci ($results[4].Outputs.ContainsKey('compiler-identity'))
 }
 minimal_then_full_cannot_skip_installation={ $results=Invoke-ToolchainFixture @((New-ToolchainPhase paths @{REQUIRED_COMPONENTS=''}),(New-ToolchainPhase verify),(New-ToolchainPhase paths @{REQUIRED_COMPONENTS='rustfmt,clippy';EXPECTED_HOST=''})); Assert-CiEqual $results[-1].Outputs['reuse-verified'] 'false'; Assert-Ci ($results[0].Outputs['reuse-key'] -cne $results[-1].Outputs['reuse-key']) }
 changed_image_or_toolchain_cannot_reuse={ foreach ($changes in @(@{ImageVersion='new-image'},@{PINNED_TOOLCHAIN='nightly-2026-11-11-aarch64-pc-windows-msvc'})) { $changes.REQUIRED_COMPONENTS='clippy,rustfmt'; $changes.EXPECTED_HOST=''; $results=Invoke-ToolchainFixture @((New-ToolchainPhase paths),(New-ToolchainPhase verify),(New-ToolchainPhase paths $changes)); Assert-CiEqual $results[-1].Outputs['reuse-verified'] 'false' } }
 reuse_rejects_host_sysroot_missing_component={ foreach ($changes in @(@{FIXTURE_HOST='x86_64-pc-windows-msvc'},@{FIXTURE_SYSROOT=$repository},@{FIXTURE_COMPONENTS='rustfmt'})) { $results=Invoke-ToolchainFixture @((New-ToolchainPhase paths),(New-ToolchainPhase verify),(New-ToolchainPhase paths @{REQUIRED_COMPONENTS='clippy,rustfmt';EXPECTED_HOST=''}),(New-ToolchainPhase verify $changes)); Assert-CiEqual $results[2].Outputs['reuse-verified'] 'true'; Assert-Ci (-not [string]::IsNullOrEmpty($results[-1].Failure)); Assert-CiEqual $results[-1].Published.Count 0 } }
 only_transfer_and_installation_gated={ foreach ($name in @('Reuse only the exact toolchain directory','Install the pinned compiler on a cache miss')) { Assert-Ci ($steps[$name].Contains("steps.paths.outputs.reuse-verified != 'true'")) }; Assert-Ci (-not $steps['Verify the compiler and all required components'].Contains('      if:')); Assert-Ci (-not $steps['Reuse only the exact toolchain directory'].Contains('BONSAI_RUST_TOOLCHAIN_VERIFIED_KEY')) }
}
Invoke-CiCases $cases $Case
