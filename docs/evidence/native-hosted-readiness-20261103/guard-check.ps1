Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$env:GITHUB_SHA = '06cac12e4d9716fac9063f6357d7ac35c2b2f327'
$compilerIdentity = 'fixture-only compiler identity; no compiler executed'
$receipt = [pscustomobject]@{ Toolchain='nightly-2026-11-02-aarch64-pc-windows-msvc'; CompilerIdentity=$compilerIdentity }
function Test-Publication($analyzers) {
if ($analyzers.SchemaVersion -ne 2 -or $analyzers.Success -ne $true -or
              $analyzers.Revision -cne $env:GITHUB_SHA -or
              $analyzers.Toolchain -cne 'nightly-2026-11-02-aarch64-pc-windows-msvc' -or
              [string]::IsNullOrWhiteSpace($compilerIdentity) -or $analyzers.CompilerIdentity -cne $compilerIdentity -or
              (@($analyzers.Packages | Sort-Object) -join ',') -cne 'bonsai-broker,bonsai-chat,bonsai-tools' -or
              @($analyzers.Checks).Count -ne 2) { throw 'Hosted strict Release analyzer/link receipt is not bound to this source and compiler.' }
          foreach ($phase in @('clippy', 'build')) {
            $checks = @($analyzers.Checks | Where-Object Phase -CEQ $phase)
            if ($checks.Count -ne 1 -or $checks[0].Passed -ne $true -or $checks[0].ExitCode -ne 0 -or
                (@($checks[0].Packages | Sort-Object) -join ',') -cne 'bonsai-broker,bonsai-chat,bonsai-tools') { throw 'Hosted strict Release analyzer/link package coverage differs.' }
          }
}
function Test-Intake($analyzers) {
if ($analyzers.SchemaVersion -ne 2 -or $analyzers.Success -ne $true -or
              $analyzers.Revision -cne $env:GITHUB_SHA -or $analyzers.Toolchain -cne $receipt.Toolchain -or
              $analyzers.CompilerIdentity -cne $receipt.CompilerIdentity -or
              (@($analyzers.Packages | Sort-Object) -join ',') -cne 'bonsai-broker,bonsai-chat,bonsai-tools' -or
              @($analyzers.Checks).Count -ne 2) { throw 'Hosted strict Release analyzer/link coverage or source binding differs.' }
          foreach ($phase in @('clippy', 'build')) {
            $checks = @($analyzers.Checks | Where-Object Phase -CEQ $phase)
            if ($checks.Count -ne 1 -or $checks[0].Passed -ne $true -or $checks[0].ExitCode -ne 0 -or
                (@($checks[0].Packages | Sort-Object) -join ',') -cne 'bonsai-broker,bonsai-chat,bonsai-tools') { throw 'Hosted strict Release analyzer/link package coverage differs.' }
          }
}
function Test-ExactOutput($output, $exitCode) {
$LASTEXITCODE=$exitCode
if ($LASTEXITCODE -ne 0 -or -not ($output -cmatch '^test result: ok\. 1 passed; 0 failed; 0 ignored;')) { throw 'Exact renderer/default-selection control failed or was not executed.' }
}
$parseFiles = @('C:\Users\symbi\source\repos\SwiftBonsai2Runner\scripts\Start-Chat.ps1','C:\Users\symbi\AppData\Local\Temp\native-hosted-readiness-20261103-cdhjdgfi\workflow-block-0.ps1','C:\Users\symbi\AppData\Local\Temp\native-hosted-readiness-20261103-cdhjdgfi\workflow-block-1.ps1','C:\Users\symbi\AppData\Local\Temp\native-hosted-readiness-20261103-cdhjdgfi\workflow-block-2.ps1','C:\Users\symbi\AppData\Local\Temp\native-hosted-readiness-20261103-cdhjdgfi\workflow-block-3.ps1','C:\Users\symbi\AppData\Local\Temp\native-hosted-readiness-20261103-cdhjdgfi\workflow-block-4.ps1','C:\Users\symbi\AppData\Local\Temp\native-hosted-readiness-20261103-cdhjdgfi\workflow-block-5.ps1','C:\Users\symbi\AppData\Local\Temp\native-hosted-readiness-20261103-cdhjdgfi\workflow-block-6.ps1','C:\Users\symbi\AppData\Local\Temp\native-hosted-readiness-20261103-cdhjdgfi\workflow-block-7.ps1')
foreach ($path in $parseFiles) {
  $tokens=$null; $errors=$null
  [Management.Automation.Language.Parser]::ParseFile($path,[ref]$tokens,[ref]$errors) | Out-Null
  if ($errors.Count) { throw ('PowerShell parse failed: ' + $path) }
}
$fixtures = Get-Content -LiteralPath 'C:\Users\symbi\AppData\Local\Temp\native-hosted-readiness-20261103-cdhjdgfi\fixtures.json' -Raw | ConvertFrom-Json
$checks=0
foreach ($guard in @('Test-Publication', 'Test-Intake')) {
  foreach ($fixture in $fixtures.Receipts) {
    $rejected=$false
    try { & $guard $fixture.Receipt } catch { $rejected=$true }
    if ($rejected -eq $fixture.ShouldPass) { throw ('Unexpected receipt verdict: ' + $guard + '/' + $fixture.Name) }
    $checks++
  }
}
foreach ($fixture in $fixtures.Outputs) {
  $rejected=$false
  try { Test-ExactOutput $fixture.Output $fixture.ExitCode } catch { $rejected=$true }
  if ($rejected -eq $fixture.ShouldPass) { throw ('Unexpected exact-test verdict: ' + $fixture.Name) }
  $checks++
}
[ordered]@{ GuardChecksPassed=$checks; ParsedFiles=$parseFiles.Count } | ConvertTo-Json -Compress
