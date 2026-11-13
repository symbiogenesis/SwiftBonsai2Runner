#Requires -Version 7.6
<# Invoke the native static ARM64 PE audit. No DLL is loaded or executed. #>
[CmdletBinding()]
param([string]$Bundle, [string]$System, [string]$Output, [string]$ToolPath = $env:BONSAI_TOOLS_PATH)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false

function Invoke-PeClosure([string]$BundleDirectory, [string]$SystemDirectory, [hashtable]$Aliases, [string]$NativeTool = $ToolPath) {
    if (-not $NativeTool) { $NativeTool = (Get-Command bonsai-tools -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source }
    $request = @{ bundleDirectory = $BundleDirectory; systemDirectory = $SystemDirectory; aliases = $Aliases }
    $result = $request | ConvertTo-Json -Depth 30 -Compress | & $NativeTool pe-closure --request - 2>&1
    if ($LASTEXITCODE -ne 0) { throw ($result | Out-String).Trim() }
    ($result -join "`n") | ConvertFrom-Json -AsHashtable -Depth 30
}

if ($MyInvocation.InvocationName -ne '.') {
    if (-not $Bundle -or -not $System -or -not $Output) { throw 'Supply -Bundle, -System and -Output.' }
    $code = 0
    try { $report = Invoke-PeClosure $Bundle $System $null }
    catch { $report = @{ schemaVersion = 1; status = 'rejected'; error = $_.Exception.Message; modelExecuted = $false }; $code = 1 }
    [IO.File]::WriteAllText($Output, (ConvertTo-Json -InputObject $report -Depth 30) + "`n", [Text.UTF8Encoding]::new($false))
    exit $code
}
