#Requires -Version 7.6
<# Inspect immutable index objects, never unstaged repairs, without changing Git metadata or contacting a network. #>
[CmdletBinding()]
param([string]$Root = (Split-Path (Split-Path $PSScriptRoot -Parent) -Parent))
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'check_workflow_startup.ps1')
function Invoke-StagedGit([string]$RepositoryRoot, [string[]]$Arguments, [Diagnostics.Stopwatch]$Clock, [string]$InputText = '') {
    $remaining = 30000 - [int]$Clock.ElapsedMilliseconds
    if ($remaining -le 0) { throw 'staged startup check exceeded its 30-second Git allowance' }
    $start = [Diagnostics.ProcessStartInfo]::new('git')
    $start.WorkingDirectory=$RepositoryRoot; $start.UseShellExecute=$false; $start.CreateNoWindow=$true
    $start.RedirectStandardOutput=$true; $start.RedirectStandardError=$true; $start.RedirectStandardInput=$true
    foreach ($argument in $Arguments) { $start.ArgumentList.Add($argument) }
    $process = [Diagnostics.Process]::Start($start)
    try {
        $output = [IO.MemoryStream]::new(); $copy = $process.StandardOutput.BaseStream.CopyToAsync($output); $errorOutput=$process.StandardError.ReadToEndAsync()
        if ($InputText) { $process.StandardInput.Write($InputText) }; $process.StandardInput.Close()
        if (-not $process.WaitForExit($remaining)) { $process.Kill($true); throw 'staged startup check exceeded its 30-second Git allowance' }
        [void]$copy.GetAwaiter().GetResult(); $detail=$errorOutput.GetAwaiter().GetResult()
        if ($process.ExitCode) { throw "git $($Arguments[0]) failed ($($process.ExitCode)): $($detail.Substring(0,[Math]::Min(2000,$detail.Length)))" }
        return ,$output.ToArray()
    } finally { $process.Dispose() }
}
function Test-StagedWorkflows([string]$RepositoryRoot) {
    $selected = $true; $scratch = $null; $clock=[Diagnostics.Stopwatch]::StartNew()
    try {
        $RepositoryRoot=[IO.Path]::GetFullPath($RepositoryRoot)
        $changed=[Text.UTF8Encoding]::new($false,$true).GetString((Invoke-StagedGit $RepositoryRoot @('diff','--cached','--name-only','--no-renames','-z','--') $clock)) -split "`0"
        $selected = @($changed | Where-Object { $_ -cmatch '\.ya?ml$' -or $_ -cin @('.github/workflows/actions.lock','.githooks/pre-commit','tools/ci/check_staged_workflows.ps1','tools/ci/check_workflow_startup.ps1') }).Count -gt 0
        if (-not $selected) { return @{ Selected=$false; Errors=@() } }
        $index=[Text.UTF8Encoding]::new($false,$true).GetString((Invoke-StagedGit $RepositoryRoot @('ls-files','--cached','--stage','--full-name','-z') $clock)) -split "`0"
        $entries=[Collections.Generic.Dictionary[string,string]]::new([StringComparer]::Ordinal)
        foreach ($record in $index) {
            if (-not $record) { continue }; $parts=$record -split "`t",2; $path=$parts[1]
            if ($path -cnotmatch '\.ya?ml$' -and $path -cne '.github/workflows/actions.lock') { continue }
            $fields=$parts[0] -split ' '
            if ($fields[2] -cne '0') { throw "unmerged staged workflow input: $path" }
            if ($fields[0] -cnotin @('100644','100755')) { throw "staged workflow input must be a regular file: $path" }
            if ($fields[1] -cnotmatch '^(?:[0-9a-f]{40}|[0-9a-f]{64})$') { throw "invalid staged object identity: $path" }
            $entries[$path]=$fields[1]
        }
        $identities=@($entries.Values | Sort-Object -Unique); $objects=@{}
        if ($identities.Count) {
            $raw=Invoke-StagedGit $RepositoryRoot @('cat-file','--batch') $clock (($identities -join "`n")+"`n"); $offset=0
            foreach ($oid in $identities) {
                $end=[Array]::IndexOf($raw,[byte]10,$offset); if ($end -lt 0) { throw 'incomplete staged object response' }
                $header=[Text.Encoding]::ASCII.GetString($raw,$offset,$end-$offset) -split ' '
                if ($header.Count -ne 3 -or $header[0] -cne $oid -or $header[1] -cne 'blob' -or $header[2] -cnotmatch '^[0-9]+$') { throw "invalid staged object response for $oid" }
                $start=$end+1; $size=[int]::Parse($header[2]); $offset=$start+$size
                if ($offset -ge $raw.Length -or $raw[$offset] -ne 10) { throw "incomplete staged blob for $oid" }
                $bytes=[byte[]]::new($size); [Array]::Copy($raw,$start,$bytes,0,$size); $objects[$oid]=$bytes; $offset++
            }
            if ($offset -ne $raw.Length) { throw 'unexpected trailing staged object response' }
        }
        $scratch=Join-Path ([IO.Path]::GetTempPath()) ('bonsai-staged-workflows-'+[guid]::NewGuid().ToString('N')); [void][IO.Directory]::CreateDirectory($scratch)
        foreach ($path in $entries.Keys) {
            $target=[IO.Path]::GetFullPath((Join-Path $scratch $path))
            if ([IO.Path]::IsPathRooted($path) -or '..' -cin ($path -split '/') -or $path.Contains('\') -or -not $target.StartsWith($scratch+[IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)) { throw "unsafe staged workflow path: $path" }
            [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($target)); [IO.File]::WriteAllBytes($target,$objects[$entries[$path]])
        }
        $failures=Test-WorkflowStartup $scratch
        return @{ Selected=$true; Errors=@($failures | ForEach-Object { $_.Replace($scratch,'staged index') }) }
    } catch { return @{ Selected=$selected; Errors=@($_.ToString()) } }
    finally { if ($scratch -and [IO.Directory]::Exists($scratch)) { [IO.Directory]::Delete($scratch,$true) } }
}
if ($MyInvocation.InvocationName -ne '.') {
    $result=Test-StagedWorkflows $Root
    foreach ($failure in $result.Errors) { [Console]::Error.WriteLine($failure) }
    if ($result.Errors.Count) { [Console]::Error.WriteLine('Commit blocked: repair and stage the workflow/action and matching actions.lock together.'); exit 1 }
    if ($result.Selected) { 'Staged workflow startup policy passed (offline; upstream provenance and hosted CI remain separate).' }
}
