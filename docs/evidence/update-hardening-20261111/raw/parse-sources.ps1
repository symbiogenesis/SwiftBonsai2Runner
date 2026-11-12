param([string]$Root)
$ErrorActionPreference = 'Stop'
$files = @((Join-Path $Root 'scripts/Start-Chat.ps1')) +
    @((Get-ChildItem -LiteralPath (Join-Path $Root 'scripts/release') -Filter '*.ps1' -File).FullName) +
    @((Get-ChildItem -LiteralPath (Join-Path $Root '.github/release') -Filter '*.ps1' -File).FullName)
foreach ($file in $files) {
    $tokens = $null
    $issues = $null
    [void][Management.Automation.Language.Parser]::ParseFile($file, [ref]$tokens, [ref]$issues)
    if ($issues.Count -gt 0) { throw "PowerShell parse failed: $file`n$issues" }
}
Write-Output "Parsed $($files.Count) sources with Windows PowerShell AST; PS7 runtime behavior remains pending."
