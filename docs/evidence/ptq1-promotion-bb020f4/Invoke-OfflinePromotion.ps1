param(
    [Parameter(Mandatory)][ValidateSet('plan', 'file-score')][string]$Operation
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$tool = 'C:/Users/symbi/AppData/Local/SwiftBonsai2/qualification/ptq1-rollback-41573164864/package/bonsai-tools.exe'
$expected = 'd1552f19c5a84f6c9325b0fdc2baac10df844495a0f342f376a7c22d1d569afb'
$revision = 'bb020f45169b0698678ca5f789aef7af8cdc12b2'
$requestPath = Join-Path $PSScriptRoot ($Operation + '-request.json')
$outputPath = Join-Path $PSScriptRoot ($Operation + '-stdout.json')
$executionPath = Join-Path $PSScriptRoot ($Operation + '-execution.json')
if ((Test-Path -LiteralPath $outputPath) -or (Test-Path -LiteralPath $executionPath)) { throw 'Offline operation outputs must be fresh.' }
$requestData = Get-Content -LiteralPath $requestPath -Raw | ConvertFrom-Json
if ($requestData.operation -cne $Operation -or $requestData.runnerRevision -cne $revision -or $requestData.nativeRowWorkers -ne 4 -or $requestData.weightRepresentation -cne 'current-repacked-PTQ1_0') { throw 'Only the frozen reviewed PTQ1 offline operation is authorized.' }
$held = [IO.File]::Open($tool, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
$hash = [Security.Cryptography.SHA256]::Create()
$child = $null
$started = [DateTime]::UtcNow.ToString('o')
try {
    $actual = [Convert]::ToHexString($hash.ComputeHash($held)).ToLowerInvariant()
    if ($actual -cne $expected) { throw 'The frozen offline evaluator identity differs.' }
    $receipt = Get-Content -LiteralPath (Join-Path (Split-Path -Parent $tool) 'receipt.json') -Raw | ConvertFrom-Json
    if ($receipt.SchemaVersion -ne 1 -or $receipt.Revision -cne $revision -or $receipt.RunId -cne '41573164864' -or $receipt.RunAttempt -cne '1' -or @($receipt.Files).Count -ne 2) { throw 'The checked acceptance-package receipt differs.' }
    $toolRecord = @($receipt.Files | Where-Object Name -CEQ 'bonsai-tools.exe')
    if ($toolRecord.Count -ne 1 -or $toolRecord[0].Sha256 -cne $expected -or $toolRecord[0].Bytes -ne $held.Length) { throw 'The source-bound tool receipt differs.' }
    $start = [Diagnostics.ProcessStartInfo]::new($tool)
    $start.WorkingDirectory = $PSScriptRoot
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $start.StandardOutputEncoding = [Text.UTF8Encoding]::new($false, $true)
    $start.StandardErrorEncoding = [Text.UTF8Encoding]::new($false, $true)
    foreach ($argument in @('runtime-promotion', '--request', $requestPath)) { $start.ArgumentList.Add($argument) }
    $child = [Diagnostics.Process]::Start($start)
    $stdout = $child.StandardOutput.ReadToEndAsync()
    $stderr = $child.StandardError.ReadToEndAsync()
    if (-not $child.WaitForExit(20000)) { throw 'Offline scoring exceeded its short document-operation bound.' }
    $out = $stdout.GetAwaiter().GetResult()
    $err = $stderr.GetAwaiter().GetResult()
    if ($child.ExitCode -ne 0) { throw ('Offline evaluator rejected the documents: ' + $err.Substring(0, [Math]::Min(4096, $err.Length))) }
    if (-not [String]::IsNullOrWhiteSpace($err)) { throw 'Unexpected evaluator stderr.' }
    [void]($out | ConvertFrom-Json)
    [IO.File]::WriteAllText($outputPath, $out, [Text.UTF8Encoding]::new($false))
    $execution = [ordered]@{
        schemaVersion=1; kind='document-only-compiled-evaluator-operation'; operation=('runtime-promotion ' + $Operation)
        sourceRevision=$revision; toolSha256=$actual; toolBytes=$held.Length
        sourcePackageRunId='41573164864'; writeDeleteDenyingToolHandleRetained=$true
        startedAtUtc=$started; finishedAtUtc=[DateTime]::UtcNow.ToString('o'); exitCode=$child.ExitCode
        hardwareCommands=0; modelCommands=0; diagnostics=0; ownerTrials=0
        separateFromClosedOwnerClock=$true; ownerClockRestarted=$false
        scope='Offline policy/document integrity only; no build, test, model, hardware or admission operation. Duration is not charged to or claimed within the closed device session.'
    }
    [IO.File]::WriteAllText($executionPath, ($execution | ConvertTo-Json -Depth 6), [Text.UTF8Encoding]::new($false))
    Write-Output ('Completed document-only ' + $Operation + ': ' + $outputPath)
} finally {
    if ($null -ne $child) {
        if (-not $child.HasExited) { $child.Kill($true); [void]$child.WaitForExit(5000) }
        $child.Dispose()
    }
    $hash.Dispose()
    $held.Dispose()
}
