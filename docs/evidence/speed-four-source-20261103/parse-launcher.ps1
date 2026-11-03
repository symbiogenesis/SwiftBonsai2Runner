param([Parameter(Mandatory)][string]$LauncherPath)
$parseTokens = $null
$parseErrors = $null
$null = [System.Management.Automation.Language.Parser]::ParseFile($LauncherPath, [ref]$parseTokens, [ref]$parseErrors)
if ($parseErrors.Count -ne 0) {
    $parseErrors | ForEach-Object { Write-Output $_.Message }
    exit 1
}
Write-Output 'Launcher PowerShell parsing passed.'
