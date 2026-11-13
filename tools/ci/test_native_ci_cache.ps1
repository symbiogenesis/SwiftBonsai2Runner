#Requires -Version 7.6
param([string]$Case)
. (Join-Path $PSScriptRoot 'test_support.ps1')
. (Join-Path $PSScriptRoot 'native_ci_cache.ps1')
$source='a'*64; $image='win11-arm64-fixture-image'; $compilerIdentity="rustc fixture`nhost: aarch64-pc-windows-msvc`n"
function New-FixturePe([int]$Machine=0xaa64,[int]$Magic=0x20b,[int]$Characteristics=2) {
    $raw=[byte[]]::new(128); [BitConverter]::GetBytes([uint16]0x5a4d).CopyTo($raw,0); [BitConverter]::GetBytes([uint32]64).CopyTo($raw,60); [BitConverter]::GetBytes([uint32]0x4550).CopyTo($raw,64)
    [BitConverter]::GetBytes([uint16]$Machine).CopyTo($raw,68); [BitConverter]::GetBytes([uint16]$Characteristics).CopyTo($raw,86); [BitConverter]::GetBytes([uint16]$Magic).CopyTo($raw,88); return ,$raw
}
function Invoke-CacheFixture([scriptblock]$Work) {
    $scratch=New-CiScratch 'bonsai-ci-cache-'
    try { [IO.File]::WriteAllBytes((Join-Path $scratch 'bonsai-ci.exe'),(New-FixturePe)); New-CiCacheReceipt $scratch $source $image $compilerIdentity | Out-Null; & $Work $scratch }
    finally { Remove-CiScratch $scratch }
}
function Update-CacheFixture([string]$Root,[string]$Field,$Value) { $path=Join-Path $Root 'receipt.json'; $receipt=[IO.File]::ReadAllText($path) | ConvertFrom-Json -AsHashtable; $receipt[$Field]=$Value; [IO.File]::WriteAllText($path,($receipt | ConvertTo-Json)) }
$cases=[ordered]@{
 exact_receipt_and_native_bytes_pass={ Invoke-CacheFixture { param($r) Assert-CiEqual (Test-CiCache $r $source $image) (Join-Path $r 'bonsai-ci.exe') } }
 every_build_identity_mismatch_fails={ Invoke-CacheFixture { param($r) foreach ($pair in @(@('sourceId',('b'*64)),@('hostId','another-image'),@('toolchain','nightly-other'),@('compiler','host: x86_64-pc-windows-msvc'),@('schemaVersion',$true),@('schemaVersion',2))) { New-CiCacheReceipt $r $source $image $compilerIdentity | Out-Null; Update-CacheFixture $r $pair[0] $pair[1]; Assert-CiReject { Test-CiCache $r $source $image } } } }
 changed_and_wrong_native_bytes_fail={ Invoke-CacheFixture { param($r) foreach ($raw in @((New-FixturePe 0x8664),(New-FixturePe 0xaa64 0x10b),(New-FixturePe 0xaa64 0x20b 0x2002),(New-FixturePe 0xaa64 0x20b 0),([Text.Encoding]::UTF8.GetBytes('not a PE')))) { [IO.File]::WriteAllBytes((Join-Path $r 'bonsai-ci.exe'),$raw); Assert-CiReject { Test-CiCache $r $source $image } }; $raw=New-FixturePe; $raw[127]=1; [IO.File]::WriteAllBytes((Join-Path $r 'bonsai-ci.exe'),$raw); Assert-CiReject { Test-CiCache $r $source $image } } }
 malformed_duplicate_missing_extra_digest_fail={ Invoke-CacheFixture { param($r) foreach ($raw in @('{}','[]','null','not-json','{"schemaVersion":1,"schemaVersion":1}')) { [IO.File]::WriteAllText((Join-Path $r 'receipt.json'),$raw); Assert-CiReject { Test-CiCache $r $source $image } }; foreach ($pair in @(@('sha256','not-a-digest'),@('unexpected','field'))) { New-CiCacheReceipt $r $source $image $compilerIdentity | Out-Null; Update-CacheFixture $r $pair[0] $pair[1]; Assert-CiReject { Test-CiCache $r $source $image } } } }
 missing_inputs_and_members_fail={ Invoke-CacheFixture { param($r) foreach ($pair in @(@('',$image),@(('a'*63),$image),@($source,''),@($source,"image`nother"))) { Assert-CiReject { Test-CiCache $r $pair[0] $pair[1] } }; [IO.File]::Delete((Join-Path $r 'bonsai-ci.exe')); Assert-CiReject { Test-CiCache $r $source $image } } }
 cli_failure_selects_no_bytes={ Invoke-CacheFixture { param($r) [IO.File]::WriteAllText((Join-Path $r 'bonsai-ci.exe'),'invalid'); $start=[Diagnostics.ProcessStartInfo]::new((Get-Process -Id $PID).Path); $start.UseShellExecute=$false; $start.RedirectStandardOutput=$true; $start.RedirectStandardError=$true; foreach ($arg in @('-NoProfile','-File',(Join-Path $PSScriptRoot 'native_ci_cache.ps1'),'verify','-PackageDirectory',$r,'-SourceId',$source,'-HostId',$image)) { $start.ArgumentList.Add($arg) }; $process=[Diagnostics.Process]::Start($start); try { $stdout=$process.StandardOutput.ReadToEnd(); $stderr=$process.StandardError.ReadToEnd(); Assert-Ci ($process.WaitForExit(15000)) 'Cache CLI fixture timed out'; Assert-CiEqual $process.ExitCode 1; Assert-CiEqual $stdout ''; Assert-Ci ($stderr.Contains('CI bootstrap verification failed:')) } finally { $process.Dispose() } } }
}
Invoke-CiCases $cases $Case
