#Requires -Version 7.6
# Shared release primitives; importing this file performs no external work.
Set-StrictMode -Version Latest

function Resolve-ReleasePath {
    [CmdletBinding()]
    [OutputType([string])]
    param(
        [Parameter(Mandatory)][string]$Path,
        [ValidateSet('File', 'Directory')][string]$Kind = 'Directory',
        [switch]$AllowMissing
    )
    if (-not [IO.Path]::IsPathFullyQualified($Path)) { throw 'Release paths must be expanded and absolute.' }
    $resolved = [IO.Path]::GetFullPath($Path)
    $cursor = $resolved
    $leaf = $true
    while ($cursor) {
        if (Test-Path -LiteralPath $cursor) {
            $item = Get-Item -LiteralPath $cursor -Force
            if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "Redirected release path: $cursor" }
            if ($leaf -and (($Kind -ceq 'Directory') -ne $item.PSIsContainer)) { throw "Expected release $Kind path: $resolved" }
        } elseif ($leaf -and -not $AllowMissing) {
            throw "Missing release path: $resolved"
        }
        $leaf = $false
        $cursor = [IO.Path]::GetDirectoryName($cursor)
    }
    $resolved
}

function Test-ReleaseSharingViolation([Exception]$Exception) {
    while ($null -ne $Exception) {
        # Exact HRESULT_FROM_WIN32(ERROR_SHARING_VIOLATION/ERROR_LOCK_VIOLATION).
        if ($Exception -is [IO.IOException] -and $Exception.HResult -in @(-2147024864, -2147024863)) {
            return $true
        }
        $Exception = $Exception.InnerException
    }
    $false
}

function Remove-ReleaseDirectory {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$Path,
        [Parameter(Mandatory)][string]$ParentDirectory,
        [Parameter(Mandatory)][string]$LeafName,
        [ValidateRange(0, 10000)][int]$TimeoutMilliseconds = 5000,
        [Management.Automation.ErrorRecord]$PrimaryFailure
    )
    try {
        if ($LeafName -in @('.', '..') -or
            $LeafName.IndexOfAny([IO.Path]::GetInvalidFileNameChars()) -ge 0 -or
            [IO.Path]::GetFileName($LeafName) -cne $LeafName) {
            throw 'Release cleanup requires one exact directory leaf name.'
        }
        $parent = [IO.Path]::TrimEndingDirectorySeparator((Resolve-ReleasePath $ParentDirectory))
        $resolved = Resolve-ReleasePath $Path -AllowMissing
        if ([IO.Path]::GetDirectoryName($resolved) -ine $parent -or
            [IO.Path]::GetFileName($resolved) -cne $LeafName) {
            throw 'Refusing cleanup outside the exact private release directory.'
        }
        $clock = [Diagnostics.Stopwatch]::StartNew()
        while (Test-Path -LiteralPath $resolved) {
            # Revalidate the exact target and every ancestor on each attempt.
            # Never follow a redirected entry introduced into the private stage.
            $null = Resolve-ReleasePath $resolved
            foreach ($entry in Get-ChildItem -LiteralPath $resolved -Force -Recurse -ErrorAction Stop) {
                if ($entry.Attributes -band [IO.FileAttributes]::ReparsePoint) {
                    throw "Redirected release cleanup entry: $($entry.FullName)"
                }
            }
            try {
                Remove-Item -LiteralPath $resolved -Recurse -Force -ErrorAction Stop
                if (Test-Path -LiteralPath $resolved) { throw 'Release cleanup left its directory behind.' }
                return
            } catch {
                $remaining = $TimeoutMilliseconds - $clock.ElapsedMilliseconds
                if (-not (Test-ReleaseSharingViolation $_.Exception) -or $remaining -le 0) { throw }
                Start-Sleep -Milliseconds ([int][Math]::Min(100, $remaining))
                if ($clock.ElapsedMilliseconds -ge $TimeoutMilliseconds) { throw }
            }
        }
    } catch {
        if ($null -ne $PrimaryFailure) {
            $PrimaryFailure.Exception.Data['ReleaseCleanupFailure'] = $_.Exception.Message
            [Console]::Error.WriteLine("Release cleanup also failed: $($_.Exception.Message)")
            throw $PrimaryFailure
        }
        throw
    }
}

function Format-ReleaseFailureOutput([string]$Text) {
    if ([string]::IsNullOrWhiteSpace($Text)) { return '<empty>' }
    # Retain the first error and final context without flooding CI exceptions.
    if ($Text.Length -le 4096) { return $Text }
    $head = 2048
    $tail = $Text.Length - 2048
    if ([char]::IsHighSurrogate($Text[$head - 1])) { $head-- }
    if ([char]::IsLowSurrogate($Text[$tail])) { $tail++ }
    $Text.Substring(0, $head) + "`n[... output truncated ...]`n" + $Text.Substring($tail)
}

function Invoke-ReleaseCommand {
    [CmdletBinding()]
    [OutputType([string])]
    param(
        [Parameter(Mandatory)][string]$FilePath,
        [string[]]$ArgumentList = @(),
        [ValidateRange(1, 600)][int]$TimeoutSeconds = 180
    )
    # ArgumentList preserves JSON, spaces and quotes on every supported PS 7
    # version; explicit exit checks do not depend on a preference added later.
    $executable = if ([IO.Path]::IsPathFullyQualified($FilePath)) {
        Resolve-ReleasePath $FilePath -Kind File
    } else {
        (Get-Command -Name $FilePath -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source
    }
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = $executable
    $commandDirectory = Join-Path (Resolve-ReleasePath ([IO.Path]::GetTempPath())) ('SwiftBonsai-Release-Command-' + [guid]::NewGuid().ToString('N'))
    [void][IO.Directory]::CreateDirectory($commandDirectory)
    $start.WorkingDirectory = $commandDirectory
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    # Release tools emit UTF-8. ProcessStartInfo otherwise chooses the host's
    # default decoding, which can silently change Unicode paths and JSON.
    $start.StandardOutputEncoding = [Text.UTF8Encoding]::new($false)
    $start.StandardErrorEncoding = [Text.UTF8Encoding]::new($false)
    foreach ($argument in $ArgumentList) { [void]$start.ArgumentList.Add($argument) }
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $start
    try {
        if (-not $process.Start()) { throw "Could not start release command: $FilePath" }
        $output = $process.StandardOutput.ReadToEndAsync()
        $errorOutput = $process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit($TimeoutSeconds * 1000)) {
            $process.Kill($true)
            [void]$process.WaitForExit(5000)
            throw "Release command exceeded its deadline: $FilePath"
        }
        $drained = [Threading.Tasks.Task]::WhenAll([Threading.Tasks.Task[]]@($output, $errorOutput))
        if (-not $drained.Wait(5000)) { throw "Release command output did not close: $FilePath" }
        $stdout = $output.GetAwaiter().GetResult()
        $stderr = $errorOutput.GetAwaiter().GetResult()
        if ($process.ExitCode -ne 0) {
            throw "Release command failed with exit code $($process.ExitCode): $FilePath`nstdout: $(Format-ReleaseFailureOutput $stdout)`nstderr: $(Format-ReleaseFailureOutput $stderr)"
        }
        if ($stderr) { [Console]::Error.Write($stderr) }
        $stdout
    } finally {
        $process.Dispose()
        # Remove only an empty directory; unexpected diagnostic files survive.
        if (@(Get-ChildItem -LiteralPath $commandDirectory -Force).Count -eq 0) {
            [IO.Directory]::Delete($commandDirectory)
        }
    }
}
