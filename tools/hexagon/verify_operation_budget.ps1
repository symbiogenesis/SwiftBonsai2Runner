#Requires -Version 7.6
param([switch]$Write)
$operationBudgetWrite=$Write
. "$PSScriptRoot/verify_operation_traffic.ps1"
$Write=$operationBudgetWrite
$script:BudgetLedger='docs/evidence/hexagon-session-operation-budget.json'
function Get-OperationBudget {
    $native='tools/hexagon/native_rpc/projection.rs';$sources=@('tools/hexagon/verify_operation_budget.ps1',$native,$script:OperationLedger)
    $declarations=[regex]::Matches([Text.Encoding]::UTF8.GetString((Read-HexagonSource $native)),'pub const Q2_PROJECTION_MAX_OPERATIONS: u32 = ([0-9_]+);')
    Assert-Hexagon ($declarations.Count -eq 1) 'one enforced native operation cap is required';$cap=[long]::Parse($declarations[0].Groups[1].Value.Replace('_',''))
    Assert-Hexagon ($cap -gt 0 -and $cap -le [uint]::MaxValue) 'native cap exceeds the operation counter'
    $ledger=ConvertFrom-HexagonJson (Read-HexagonSource $script:OperationLedger);$counts=@(foreach ($family in $ledger.projectionFamilies) {
        $layers=Get-HexagonInteger $family layers;$columns=Get-HexagonInteger $family columns;$rows=Get-HexagonInteger $family weightRows
        Assert-Hexagon ($layers -gt 0 -and $columns -gt 0 -and $rows -gt 0 -and $columns % 256 -eq 0 -and $rows % 32 -eq 0) 'original Q8_K geometry required'
        $count=$layers*[bigint]::Divide($columns,128)*[bigint]::Divide($rows+17407,17408);Assert-Hexagon ($count -eq $family.operationsPerToken) 'historical family geometry no longer agrees';[long]$count
    })
    Assert-Hexagon ($ledger.projectionFamilies[-1].tensor -ceq 'output.weight') 'final logits family must be explicit'
    $evaluate=($counts | Measure-Object -Sum).Sum;$hidden=$evaluate-$counts[-1];$first=$counts[0]/$ledger.projectionFamilies[0].layers
    return @{schemaVersion=1;kind='current-original-pq2-terminal-operation-budget';sourceIdentityEncoding='UTF8-with-LF-newlines';sources=@($sources | ForEach-Object {@{path=$_;sha256=(Get-HexagonHash (Read-HexagonSource $_))}});historicalLedgerUnchanged=$true;historicalTerminalMaximumOperations=$ledger.totals.terminalSessionMaximumOperations;terminalMaximumOperations=$cap;hiddenOperationsPerToken=$hidden;evaluateOperationsPerToken=$evaluate;oneAllEligibleEvaluationFits=($evaluate -le $cap);remainingAfterOneAllEligibleEvaluation=[Math]::Max(0,$cap-$evaluate);oneHiddenPlusOneEvaluationOperations=$hidden+$evaluate;oneHiddenPlusOneEvaluationFits=($hidden+$evaluate -le $cap);firstProjectionOperationsPerToken=$first;maximumFirstProjectionEvaluationsByOperationsOnly=[long][Math]::Floor($cap/$first);sessionMaximumSeconds=120;cleanupReserveSeconds=3;completeRequestAdmissionRequired=$true;resetReplenishesCapacity=$false;deviceOpened=$false;modelLoaded=$false;profileEligible=$false;measuredInferenceSpeedup=$null;limitations=@('Static original-PQ2 counts; PTQ1, smaller row tiles and partial placement need independent accounting.','One evaluation fitting does not admit a prompt and generated reply or establish a persistent serving lifetime.','The complete unreused prompt, output reserve and prior session attempts must fit before inference.','An operation count establishes no device arithmetic, available resources, duration or performance benefit.')}
}
if ($MyInvocation.InvocationName -ne '.') {
    $expected=Get-OperationBudget
    if ($Write) {Write-HexagonJson (Join-Path $script:HexagonRoot $script:BudgetLedger) $expected} else {Assert-Hexagon (Test-HexagonEqual (Read-HexagonJson (Join-Path $script:HexagonRoot $script:BudgetLedger)) $expected) 'current terminal operation budget differs from source';Write-Output 'Current operation budget verified; no model or device execution.'}
}
