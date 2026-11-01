#requires -Version 7.6
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Tools.psm1')

function Invoke-BonsaiHexagonPolicy {
    <# .SYNOPSIS
    Calls offline Rust evidence policy using the source-bound hosted tool only.
    #>
    [CmdletBinding()]
    [OutputType([Collections.IDictionary])]
    param([Parameter(Mandatory)][Collections.IDictionary]$Request)
    if (-not $env:BONSAI_TOOLS_PATH) { throw 'Hexagon policy requires the source-bound hosted maintenance tool in BONSAI_TOOLS_PATH; no owner-device compilation is permitted.' }
    return Invoke-BonsaiTool -Command hexagon-policy -Request $Request -ToolPath $env:BONSAI_TOOLS_PATH -TimeoutSeconds 10
}

Export-ModuleMember -Function Invoke-BonsaiHexagonPolicy
