#Requires -Version 7.6
function Assert-Rejected([scriptblock]$Action,[string]$Pattern='.') {
    $rejected=$false
    try {& $Action | Out-Null} catch {$rejected=$true;Assert-Hexagon ($_.Exception.Message -match $Pattern) ('wrong rejection: '+$_.Exception.Message)}
    Assert-Hexagon $rejected 'invalid input was accepted'
}
function Invoke-HexagonTests($Tests,[string]$Selection) {
    $names=@(if ($Selection) {Assert-Hexagon ($Tests.Contains($Selection)) ('unknown regression selection: '+$Selection);$Selection} else {$Tests.Keys | Sort-Object})
    foreach ($name in $names) {& $Tests[$name];Write-Output ('PASS '+$name)}
    Write-Output ("{0} focused controls passed." -f $names.Count)
}
