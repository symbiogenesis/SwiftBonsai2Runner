#Requires -Version 7.6
param([string]$Test)
. "$PSScriptRoot/Hexagon.Native.ps1"
Invoke-HexagonAudit 'regression' @{ name = $Test }
