#Requires -Version 7.6
param([string]$Metadata,[string]$Census,[string]$Payload,[long]$Columns,[long]$Rows=1)
. "$PSScriptRoot/Hexagon.Native.ps1"
$parameters = @{ columns = $Columns; rows = $Rows }
foreach ($name in @('Metadata','Census','Payload')) {
    $value = Get-Variable -Name $name -ValueOnly
    if ($value) { $parameters[$name.ToLowerInvariant()] = $value }
}
Invoke-HexagonAudit 'pq2-bridge' $parameters
