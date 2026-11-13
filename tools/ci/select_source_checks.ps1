#Requires -Version 7.6
<# Select eligible source checks conservatively without a compiler or additional runtime. #>
[CmdletBinding()]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
$eventName=$env:EVENT_NAME; $main=$env:EVENT_REF -ceq 'refs/heads/main'
$eligible=[ordered]@{portable=($eventName -cin @('pull_request','workflow_dispatch')); markdown=($eventName -cin @('push','pull_request','workflow_dispatch')); audit=($main -and $eventName -cin @('push','schedule','workflow_dispatch'))}
$selected=[ordered]@{}; foreach ($key in $eligible.Keys) { $selected[$key]=$eligible[$key] }
$reason='Manual checks or weekly fresh-advisory audit.'
if ($eventName -cin @('push','pull_request')) {
    try {
        $base=$env:BASE_SHA; $head=$env:HEAD_SHA
        foreach ($sha in @($base,$head)) { if ($sha -cnotmatch '^[0-9a-fA-F]{40}$' -or $sha -cmatch '^0+$') { throw 'Missing, zero or malformed source range.' } }
        $separator=if ($eventName -ceq 'pull_request') { '...' } else { '..' }
        $start=[Diagnostics.ProcessStartInfo]::new('git'); $start.UseShellExecute=$false; $start.RedirectStandardOutput=$true; $start.RedirectStandardError=$true
        foreach ($arg in @('diff','--name-only','--no-renames','-z',($base+$separator+$head),'--')) { $start.ArgumentList.Add($arg) }
        $process=[Diagnostics.Process]::Start($start)
        try {
            $raw=[IO.MemoryStream]::new(); $copy=$process.StandardOutput.BaseStream.CopyToAsync($raw); $errorOutput=$process.StandardError.ReadToEndAsync()
            if (-not $process.WaitForExit(20000)) { $process.Kill($true); throw 'Source range query exceeded 20 seconds.' }
            [void]$copy.GetAwaiter().GetResult(); [void]$errorOutput.GetAwaiter().GetResult(); if ($process.ExitCode) { throw 'Source range query failed.' }
            $paths=@([Text.UTF8Encoding]::new($false,$true).GetString($raw.ToArray()) -split "`0" | Where-Object { $_ })
        } finally { $process.Dispose() }
        $workflow='.github/workflows/source-checks.yml'
        $selected.portable=$eligible.portable -and @($paths | Where-Object { -not ($_.EndsWith('.md',[StringComparison]::Ordinal) -or $_.StartsWith('docs/',[StringComparison]::Ordinal) -or $_ -ceq '.rumdl.toml') }).Count -gt 0
        $selected.markdown=$eligible.markdown -and @($paths | Where-Object { $_.EndsWith('.md',[StringComparison]::Ordinal) -or $_ -cin @('.rumdl.toml',$workflow) }).Count -gt 0
        $selected.audit=$eligible.audit -and @($paths | Where-Object { $_ -cin @('Cargo.lock','Cargo.toml','rust-toolchain.toml',$workflow) -or $_.EndsWith('/Cargo.toml',[StringComparison]::Ordinal) -or $_.StartsWith('.github/actions/rust-toolchain/',[StringComparison]::Ordinal) }).Count -gt 0
        $reason='Selected from the complete changed-path range.'
    } catch { foreach ($key in $eligible.Keys) { $selected[$key]=$eligible[$key] }; $reason='Unavailable source range: conservatively select every eligible check.' }
}
foreach ($name in $selected.Keys) { $value=$selected[$name].ToString().ToLowerInvariant(); [IO.File]::AppendAllText($env:GITHUB_OUTPUT,"$name=$value`n") }
[IO.File]::AppendAllText($env:GITHUB_STEP_SUMMARY,$reason+"`n")
foreach ($name in $selected.Keys) { [IO.File]::AppendAllText($env:GITHUB_STEP_SUMMARY,"- ${name}: $($selected[$name].ToString().ToLowerInvariant())`n") }
