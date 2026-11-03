Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
& $env:BONSAI_TOOLS_PATH provision --json (@{ operation = 'inference'; dataDirectory = $env:BONSAI_NATIVE_DATA; verifyOnly = $true } | ConvertTo-Json -Compress)
