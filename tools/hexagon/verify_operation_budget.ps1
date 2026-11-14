#Requires -Version 7.6
param([switch]$Write)
. "$PSScriptRoot/Hexagon.Native.ps1"
Invoke-HexagonAudit 'operation-budget' @{ write = [bool]$Write }
