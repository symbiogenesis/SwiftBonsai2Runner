#Requires -Version 7.6
param([switch]$Emit,[switch]$Write)
. "$PSScriptRoot/Hexagon.Native.ps1"
Invoke-HexagonAudit 'operation-traffic' @{ emit = [bool]$Emit; write = [bool]$Write }
