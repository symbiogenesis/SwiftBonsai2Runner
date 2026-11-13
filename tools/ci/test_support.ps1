#Requires -Version 7.6
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
function Assert-Ci([bool]$Condition,[string]$Message='Assertion failed') { if (-not $Condition) { throw $Message } }
function Assert-CiEqual($Actual,$Expected) {
    $left=ConvertTo-Json -InputObject $Actual -Depth 30 -Compress; $right=ConvertTo-Json -InputObject $Expected -Depth 30 -Compress
    Assert-Ci ($left -ceq $right) "Expected $right; received $left"
}
function Assert-CiSet($Actual,$Expected) { Assert-CiEqual @($Actual | Sort-Object -Unique) @($Expected | Sort-Object -Unique) }
function Assert-CiReject([scriptblock]$Work,[string]$Phrase='') {
    $failed=$false
    try { & $Work | Out-Null } catch { if ($Phrase -and -not $_.ToString().Contains($Phrase)) { throw }; $failed=$true }
    Assert-Ci $failed "Expected rejection containing '$Phrase'"
}
function Write-CiFixture([string]$Root,[string]$Path,[string]$Value) {
    $target=Join-Path $Root $Path; [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($target)); [IO.File]::WriteAllText($target,$Value)
}
function New-CiScratch([string]$Prefix='bonsai-ci-test-') { $path=Join-Path ([IO.Path]::GetTempPath()) ($Prefix+[guid]::NewGuid().ToString('N')); [void][IO.Directory]::CreateDirectory($path); return $path }
function Remove-CiScratch([string]$Path) {
    $absolute=[IO.Path]::GetFullPath($Path); $temporary=[IO.Path]::TrimEndingDirectorySeparator([IO.Path]::GetFullPath([IO.Path]::GetTempPath()))+[IO.Path]::DirectorySeparatorChar
    if (-not $absolute.StartsWith($temporary,[StringComparison]::OrdinalIgnoreCase) -or -not [IO.Path]::GetFileName($absolute).StartsWith('bonsai',[StringComparison]::Ordinal)) { throw 'Fixture cleanup escaped its named temporary directory.' }
    Remove-Item -LiteralPath $absolute -Recurse -Force
}
function Invoke-CiCases([Collections.IDictionary]$Cases,[string]$SelectedCase) {
    if ($SelectedCase -and -not $Cases.Contains($SelectedCase)) { throw "Unknown named case: $SelectedCase" }
    $selected=@(if ($SelectedCase) { $SelectedCase } else { $Cases.Keys })
    foreach ($name in $selected) { & $Cases[$name]; "PASS $name" }
    "$($selected.Count) focused fixture(s) passed."
}
function Get-CiJobBlocks([string]$Text) {
    $body=([regex]::Split($Text,'(?m)^jobs:\s*$'))[1]; $starts=[regex]::Matches($body,'(?m)^  ([a-z][a-z-]*):\s*$'); $result=@{}
    for ($i=0;$i -lt $starts.Count;$i++) { $end=if ($i+1 -lt $starts.Count) { $starts[$i+1].Index } else { $body.Length }; $start=$starts[$i].Index+$starts[$i].Length; $result[$starts[$i].Groups[1].Value]=$body.Substring($start,$end-$start) }
    return $result
}
function Get-CiDependencies([string]$Block) { if ($Block -cmatch '(?m)^    needs: (.+)$') { return @($Matches[1].Trim('[]').Replace(' ','') -split ',') }; return @() }
function Test-CiAcyclic([hashtable]$Jobs) {
    $pending=@{}; foreach ($name in $Jobs.Keys) { $pending[$name]=@(Get-CiDependencies $Jobs[$name]) }; $done=@()
    while ($pending.Count) { $ready=@($pending.Keys | Where-Object { @($pending[$_] | Where-Object { $_ -cnotin $done }).Count -eq 0 }); if (-not $ready.Count) { return $false }; $done+=$ready; foreach ($name in $ready) { $pending.Remove($name) } }
    return $true
}
function Get-CiActionSteps([string]$Text) {
    $starts=[regex]::Matches($Text,'(?m)^    - name: (.+)$'); $result=@{}
    for ($i=0;$i -lt $starts.Count;$i++) { $end=if ($i+1 -lt $starts.Count) { $starts[$i+1].Index } else { $Text.Length }; $start=$starts[$i].Index+$starts[$i].Length; $result[$starts[$i].Groups[1].Value]=$Text.Substring($start,$end-$start) }
    return $result
}
function Get-CiActionBody([string]$Block) { $body=($Block -split '      run: \|\r?\n',2)[1]; return (($body -split '\r?\n' | ForEach-Object { if ($_.Length -ge 8) { $_.Substring(8) } else { '' } }) -join "`n") }
