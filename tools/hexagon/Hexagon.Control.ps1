#Requires -Version 7.6
. "$PSScriptRoot/Hexagon.Common.ps1"
function New-HexagonControl([string]$Review,[hashtable]$Environment,[int]$Seconds=660) {
    return @{review=$Review;environment=$Environment;clock=[Diagnostics.Stopwatch]::StartNew();maximumSeconds=$Seconds;commands=[Collections.Generic.List[object]]::new()}
}
function Invoke-HexagonControl($Control,[string]$Name,[string[]]$Arguments,[string]$Directory,[int]$Seconds=30,[switch]$ExpectedFailure) {
    $allowance=[Math]::Min($Seconds,$Control.maximumSeconds-$Control.clock.Elapsed.TotalSeconds-15)
    Assert-Hexagon ($allowance -gt 0) 'aggregate hosted source-control budget exhausted'
    $start=[Diagnostics.Stopwatch]::StartNew();$record=@{name=$Name;arguments=$Arguments;completed=$false};$Control.commands.Add($record)
    $info=[Diagnostics.ProcessStartInfo]::new();$info.FileName=$Arguments[0];$info.WorkingDirectory=$Directory;$info.UseShellExecute=$false;$info.CreateNoWindow=$true;$info.RedirectStandardOutput=$true;$info.RedirectStandardError=$true;$info.RedirectStandardInput=$true
    for ($index=1;$index -lt $Arguments.Count;$index++) {$info.ArgumentList.Add($Arguments[$index])}
    $info.Environment.Clear();foreach ($key in $Control.environment.Keys) {$info.Environment[$key]=[string]$Control.environment[$key]}
    $stdoutPath=Join-Path $Control.review ($Name+'.log');$stderrPath=Join-Path $Control.review ($Name+'.stderr.log')
    $stdout=[IO.File]::Open($stdoutPath,[IO.FileMode]::CreateNew,[IO.FileAccess]::ReadWrite,[IO.FileShare]::ReadWrite)
    $stderr=[IO.File]::Open($stderrPath,[IO.FileMode]::CreateNew,[IO.FileAccess]::ReadWrite,[IO.FileShare]::ReadWrite)
    $process=[Diagnostics.Process]::new();$process.StartInfo=$info;$started=$false
    try {
        Assert-Hexagon ($process.Start()) ('command did not start: '+$Name);$started=$true;$process.StandardInput.Close()
        $outTask=$process.StandardOutput.BaseStream.CopyToAsync($stdout);$errTask=$process.StandardError.BaseStream.CopyToAsync($stderr)
        while (-not $process.WaitForExit(50)) {
            if ($start.Elapsed.TotalSeconds -ge $allowance -or $stdout.Length+$stderr.Length -gt 4194304) {throw ('bounded hosted source command exceeded its time/output allowance: '+$Name)}
        }
        Assert-Hexagon ([Threading.Tasks.Task]::WaitAll([Threading.Tasks.Task[]]@($outTask,$errTask),10000)) 'command output drain exceeded allowance'
        Assert-Hexagon ($stdout.Length+$stderr.Length -le 4194304) ('bounded command output exceeded: '+$Name)
        $record.completed=$true;$record.exitCode=$process.ExitCode
    } finally {
        if ($started -and -not $process.HasExited) {$process.Kill($true);$null=$process.WaitForExit(10000)}
        $stdout.Dispose();$stderr.Dispose();$process.Dispose();$record.elapsedMilliseconds=[long]$start.Elapsed.TotalMilliseconds
        foreach ($pair in @(@('log',$stdoutPath),@('stderr',$stderrPath))) {if ((Get-Item -LiteralPath $pair[1]).Length) {$record[$pair[0]]=Get-HexagonIdentity $pair[1]} else {Remove-Item -LiteralPath $pair[1];$record[$pair[0]+'Empty']=$true}}
    }
    Assert-Hexagon (($record.exitCode -eq 0) -ne [bool]$ExpectedFailure) ('command outcome differed: '+$Name)
    if (Test-Path -LiteralPath $stdoutPath) {return [Text.Encoding]::UTF8.GetString((Read-HexagonBytes $stdoutPath 4194304))};return ''
}
function Get-HexagonEnvironment {
    $result=@{};foreach ($entry in [Environment]::GetEnvironmentVariables().GetEnumerator()) {$result[[string]$entry.Key]=[string]$entry.Value};return $result
}
function Receive-HexagonPublic($Entry,[string]$Directory,[long]$MaximumBytes=65625) {
    Assert-Hexagon ($Entry.name -cmatch '^[A-Za-z0-9][A-Za-z0-9_.-]{0,127}$' -and $Entry.url.StartsWith('https://raw.githubusercontent.com/') -and $Entry.bytes -gt 0 -and $Entry.bytes -le $MaximumBytes) 'invalid bounded public source download'
    $client=[Net.Http.HttpClient]::new();$client.Timeout=[TimeSpan]::FromSeconds(30)
    try {
        $response=$client.GetAsync($Entry.url,[Net.Http.HttpCompletionOption]::ResponseHeadersRead).GetAwaiter().GetResult();$null=$response.EnsureSuccessStatusCode()
        Assert-Hexagon ($response.RequestMessage.RequestUri.AbsoluteUri.StartsWith('https://raw.githubusercontent.com/')) 'public source download left the reviewed HTTPS origin'
        $stream=$response.Content.ReadAsStream();$data=[byte[]]::new($Entry.bytes+1);$length=0
        try {while ($length -lt $data.Length) {$read=$stream.Read($data,$length,$data.Length-$length);if ($read -eq 0) {break};$length+=$read}} finally {$stream.Dispose()}
        Assert-Hexagon ($length -eq $Entry.bytes) 'public input differs from its byte pin'
        $data=[byte[]]$data[0..($length-1)];Assert-Hexagon ((Get-HexagonHash $data) -ceq $Entry.sha256) 'public source input differs from its exact pin'
        $path=Join-Path $Directory $Entry.name;$file=[IO.File]::Open($path,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write);try {$file.Write($data)} finally {$file.Dispose()}
        $identity=Get-HexagonIdentity $path;$identity.url=$Entry.url;return $identity
    } finally {$client.Dispose()}
}
function Assert-HexagonIdentity([string]$Path,$Expected) {
    $actual=Get-HexagonIdentity $Path;Assert-Hexagon ($actual.bytes -eq $Expected.bytes -and $actual.sha256 -ceq $Expected.sha256) ('input identity changed: '+$actual.name);return $actual
}
function Resolve-HexagonExecutable([string]$Path) {
    $item=Get-Item -LiteralPath $Path -Force
    if ($item.LinkTarget) {return $item.ResolveLinkTarget($true).FullName};return $item.FullName
}
