Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$root = Join-Path $env:RUNNER_TEMP ('Native-Runtime-Acceptance-' + [guid]::NewGuid().ToString('N'))
foreach ($name in @('work', 'evidence')) {
  [void][IO.Directory]::CreateDirectory((Join-Path $root $name))
}
"BONSAI_NATIVE_WORK=$(Join-Path $root 'work')" >> $env:GITHUB_ENV
"BONSAI_NATIVE_EVIDENCE=$(Join-Path $root 'evidence')" >> $env:GITHUB_ENV
"BONSAI_NATIVE_DATA=$(Join-Path $env:LOCALAPPDATA 'SwiftBonsai2')" >> $env:GITHUB_ENV
$package = Join-Path $env:RUNNER_TEMP 'Native-Acceptance-Package'
$receiptPath = Join-Path $package 'receipt.json'
$receipt = Get-Content -LiteralPath $receiptPath -Raw | ConvertFrom-Json
if ($receipt.SchemaVersion -ne 1 -or $receipt.Revision -cne $env:GITHUB_SHA -or
    $receipt.RunId -cne $env:GITHUB_RUN_ID -or $receipt.RunAttempt -cne $env:GITHUB_RUN_ATTEMPT -or
    $receipt.Toolchain -cne 'nightly-2026-11-02-aarch64-pc-windows-msvc' -or
    $receipt.ModelExecuted -ne $false -or $receipt.ToolsSourceId -cnotmatch '^[a-f0-9]{64}$' -or
    [string]::IsNullOrWhiteSpace($receipt.CompilerIdentity) -or @($receipt.Files).Count -ne 3) { throw 'Hosted acceptance source/run identity mismatch.' }
foreach ($name in @('native-model-acceptance.exe', 'bonsai-tools.exe', 'strict-release-analyzers.json')) {
  $matching = @($receipt.Files | Where-Object Name -CEQ $name)
  $path = Join-Path $package $name
  if ($matching.Count -ne 1 -or (Get-Item -LiteralPath $path).Length -ne $matching[0].Bytes -or
      (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() -cne $matching[0].Sha256) { throw 'Hosted acceptance binary digest mismatch.' }
}
$analyzerReceiptPath = Join-Path $package 'strict-release-analyzers.json'
$analyzers = Get-Content -LiteralPath $analyzerReceiptPath -Raw | ConvertFrom-Json
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
foreach ($name in @('native-model-acceptance.exe', 'bonsai-tools.exe')) {
  & (Join-Path $package 'bonsai-tools.exe') artifact --json (@{ operation = 'arm64'; path = (Join-Path $package $name) } | ConvertTo-Json -Compress) | Out-Null
}
Copy-Item -LiteralPath $receiptPath -Destination (Join-Path $root 'evidence/hosted-build.json')
Copy-Item -LiteralPath $analyzerReceiptPath -Destination (Join-Path $root 'evidence/strict-release-analyzers.json')
"BONSAI_NATIVE_TEST=$(Join-Path $package 'native-model-acceptance.exe')" >> $env:GITHUB_ENV
"BONSAI_TOOLS_PATH=$(Join-Path $package 'bonsai-tools.exe')" >> $env:GITHUB_ENV
