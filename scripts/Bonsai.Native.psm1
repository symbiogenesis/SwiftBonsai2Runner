#requires -Version 7.6
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true

function Initialize-BonsaiBootstrapJob {
    <# .SYNOPSIS
    Loads the small Windows Job boundary used before the Rust launcher exists.
    #>
    [CmdletBinding()]
    [OutputType([void])]
    param()
    if ('Bonsai.BootstrapJob' -as [type]) { return }
    Add-Type -CompilerOptions @('/nullable:enable', '/checked+', '/unsafe-', '/warn:9999', '/warnaserror+', '/langversion:14.0') -TypeDefinition @'
using System;
using System.Buffers.Binary;
using System.ComponentModel;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Runtime.Versioning;
using System.Text;
using System.Threading;
using Microsoft.Win32.SafeHandles;

namespace Bonsai
{
    /// <summary>Owns the cold compiler bootstrap and all of its descendants.</summary>
    [SupportedOSPlatform("windows")]
    public sealed class BootstrapJob : IDisposable
    {
        private readonly SafeFileHandle handle;
        private int assignedRootProcessId;

        /// <summary>Bounded process-ID and basename evidence captured before terminating survivors.</summary>
        public string RemainingProcessEvidence { get; private set; } = "not-captured";

        /// <summary>Creates a noninheritable, unnamed kill-on-close Job.</summary>
        public BootstrapJob()
        {
            if (IntPtr.Size != 8) { throw new PlatformNotSupportedException("Bootstrap ownership requires 64-bit Windows."); }
            handle = NativeMethods.CreateJobObjectW(IntPtr.Zero, IntPtr.Zero);
            if (handle.IsInvalid) { throw new Win32Exception(Marshal.GetLastPInvokeError()); }
            // JOBOBJECT_EXTENDED_LIMIT_INFORMATION is 144 bytes on 64-bit Windows;
            // BasicLimitInformation.LimitFlags is the DWORD at byte offset 16.
            byte[] limits = new byte[144];
            BinaryPrimitives.WriteUInt32LittleEndian(limits.AsSpan(16), 0x2000);
            if (!NativeMethods.SetInformationJobObject(handle, 9, limits, checked((uint)limits.Length)))
            {
                int error = Marshal.GetLastPInvokeError();
                handle.Dispose();
                throw new Win32Exception(error);
            }
        }

        /// <summary>Assigns the trusted waiting trampoline before it can start a tool.</summary>
        /// <param name="process">The newly started process still waiting for its input gate.</param>
        public void Assign(Process process)
        {
            ArgumentNullException.ThrowIfNull(process);
            if (!NativeMethods.AssignProcessToJobObject(handle, process.SafeHandle))
            {
                throw new Win32Exception(Marshal.GetLastPInvokeError());
            }
            if (!NativeMethods.IsProcessInJob(process.SafeHandle, handle, out bool assigned))
            {
                throw new Win32Exception(Marshal.GetLastPInvokeError());
            }
            if (!assigned) { throw new InvalidOperationException("Bootstrap trampoline did not enter its Job."); }
            assignedRootProcessId = process.Id;
        }

        /// <summary>Terminates remaining members and waits for empty Job accounting.</summary>
        /// <param name="milliseconds">A positive bounded cleanup interval.</param>
        /// <returns>Whether members remained when cleanup started.</returns>
        public bool Stop(int milliseconds)
        {
            return StopCore(milliseconds, 0);
        }

        /// <summary>Allows just-exited members' Job accounting to settle before classifying survivors.</summary>
        /// <param name="milliseconds">The total positive cleanup interval, including natural exit grace.</param>
        /// <returns>Whether members remained after at most 250 milliseconds of natural exit grace.</returns>
        public bool StopAfterExit(int milliseconds)
        {
            return StopCore(milliseconds, 250);
        }

        [System.Diagnostics.CodeAnalysis.SuppressMessage("Design", "CA1031:Do not catch general exception types",
            Justification = "Best-effort survivor diagnostics must never prevent termination of the owned Job; no exception is accepted as cleanup success.")]
        private bool StopCore(int milliseconds, int naturalExitMilliseconds)
        {
            ArgumentOutOfRangeException.ThrowIfNegativeOrZero(milliseconds);
            Stopwatch timer = Stopwatch.StartNew();
            uint active = ActiveProcesses();
            int grace = Math.Min(naturalExitMilliseconds, milliseconds);
            while (active != 0 && timer.ElapsedMilliseconds < grace)
            {
                Thread.Sleep(checked((int)Math.Max(0, Math.Min(10, grace - timer.ElapsedMilliseconds))));
                active = ActiveProcesses();
            }
            bool remaining = active != 0;
            if (remaining)
            {
                // Diagnostics must never interfere with termination. This fixed
                // snapshot contains no executable paths, arguments or environment.
                try { RemainingProcessEvidence = DescribeRemainingProcesses(active); }
                catch (Exception error) { RemainingProcessEvidence = "snapshot-error=" + error.GetType().Name; }
            }
            // Retain explicit termination even for an apparently empty Job. The
            // grace never removes kill-on-close ownership or the empty-Job gate.
            if (!NativeMethods.TerminateJobObject(handle, 1)) { throw new Win32Exception(Marshal.GetLastPInvokeError()); }
            while (ActiveProcesses() != 0)
            {
                if (timer.ElapsedMilliseconds >= milliseconds) { throw new TimeoutException("Bootstrap Job did not become empty."); }
                Thread.Sleep(10);
            }
            return remaining;
        }

        private uint ActiveProcesses()
        {
            // JOBOBJECT_BASIC_ACCOUNTING_INFORMATION is 48 bytes. ActiveProcesses
            // is the DWORD at byte offset 40; no raw pointers escape the call.
            uint[] accounting = new uint[12];
            if (!NativeMethods.QueryInformationJobObject(handle, 1, accounting, 48, IntPtr.Zero))
            {
                throw new Win32Exception(Marshal.GetLastPInvokeError());
            }
            return accounting[10];
        }

        private string DescribeRemainingProcesses(uint active)
        {
            // JOBOBJECT_BASIC_PROCESS_ID_LIST has two DWORD counts followed by
            // ULONG_PTR IDs at offset8 on64-bit Windows. One bounded query only;
            // truncated/failed snapshots never become permission to accept a Job.
            const int capacity = 32;
            byte[] information = new byte[8 + capacity * 8];
            StringBuilder evidence = new StringBuilder(4096);
            evidence.Append("accounted=").Append(active).Append(";root=").Append(assignedRootProcessId);
            if (!NativeMethods.QueryInformationJobObjectBytes(handle, 3, information,
                    checked((uint)information.Length), out uint returned))
            {
                return evidence.Append(";pid-query-error=").Append(Marshal.GetLastPInvokeError())
                    .Append(";capacity=").Append(capacity).ToString();
            }
            uint assigned = BinaryPrimitives.ReadUInt32LittleEndian(information);
            uint listed = BinaryPrimitives.ReadUInt32LittleEndian(information.AsSpan(4));
            if (returned < 8 || returned > information.Length || listed > (returned - 8) / 8)
            {
                return evidence.Append(";pid-query-invalid-extent=").Append(returned)
                    .Append(";listed=").Append(listed).ToString();
            }
            evidence.Append(";assigned=").Append(assigned).Append(";listed=").Append(listed)
                .Append(";returned=").Append(returned).Append(";truncated=")
                .Append(assigned > listed || listed > capacity ? "true" : "false").Append(";members=[");
            int count = checked((int)Math.Min(listed, capacity));
            for (int index = 0; index < count; index++)
            {
                if (index != 0) { evidence.Append('|'); }
                ulong id = BinaryPrimitives.ReadUInt64LittleEndian(information.AsSpan(8 + index * 8));
                evidence.Append("pid=").Append(id).Append(";role=")
                    .Append(id == checked((ulong)assignedRootProcessId) ? "root" : "descendant");
                if (id == 0 || id > int.MaxValue)
                {
                    evidence.Append(";state=invalid;name=unknown");
                    continue;
                }
                try
                {
                    using Process process = Process.GetProcessById(checked((int)id));
                    if (process.HasExited) { evidence.Append(";state=exited;name=unknown"); }
                    else if (!NativeMethods.IsProcessInJob(process.SafeHandle, handle, out bool member))
                    {
                        evidence.Append(";state=membership-error;code=").Append(Marshal.GetLastPInvokeError())
                            .Append(";name=unknown");
                    }
                    else if (!member) { evidence.Append(";state=outside-job;name=unknown"); }
                    else { evidence.Append(";state=live;name=").Append(SafeProcessName(process.ProcessName)); }
                }
                catch (ArgumentException) { evidence.Append(";state=gone;name=unknown"); }
                catch (InvalidOperationException) { evidence.Append(";state=exited-or-unavailable;name=unknown"); }
                catch (Win32Exception error) { evidence.Append(";state=access-error;code=").Append(error.NativeErrorCode).Append(";name=unknown"); }
            }
            return evidence.Append(']').ToString();
        }

        private static string SafeProcessName(string name)
        {
            ArgumentNullException.ThrowIfNull(name);
            if (name.Length == 0) { return "unknown"; }
            int length = Math.Min(name.Length, 64);
            StringBuilder result = new StringBuilder(length);
            for (int index = 0; index < length; index++)
            {
                char value = name[index];
                bool permitted = value is >= 'a' and <= 'z' or >= 'A' and <= 'Z' or >= '0' and <= '9' or '.' or '-' or '_';
                result.Append(permitted ? value : '_');
            }
            return result.ToString();
        }

        /// <summary>Closes the sole Job handle; abrupt owner exit also closes it.</summary>
        public void Dispose() { handle.Dispose(); }

        private static class NativeMethods
        {
            [DllImport("kernel32.dll", ExactSpelling = true, SetLastError = true)]
            [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
            internal static extern SafeFileHandle CreateJobObjectW(IntPtr attributes, IntPtr name);
            [DllImport("kernel32.dll", ExactSpelling = true, SetLastError = true)]
            [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
            [return: MarshalAs(UnmanagedType.Bool)]
            internal static extern bool SetInformationJobObject(SafeFileHandle job, int informationClass, [In] byte[] information, uint length);
            [DllImport("kernel32.dll", ExactSpelling = true, SetLastError = true)]
            [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
            [return: MarshalAs(UnmanagedType.Bool)]
            internal static extern bool QueryInformationJobObject(SafeFileHandle job, int informationClass, [Out] uint[] information, uint length, IntPtr returnedLength);
            [DllImport("kernel32.dll", EntryPoint = "QueryInformationJobObject", ExactSpelling = true, SetLastError = true)]
            [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
            [return: MarshalAs(UnmanagedType.Bool)]
            internal static extern bool QueryInformationJobObjectBytes(SafeFileHandle job, int informationClass, [Out] byte[] information, uint length, out uint returnedLength);
            [DllImport("kernel32.dll", ExactSpelling = true, SetLastError = true)]
            [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
            [return: MarshalAs(UnmanagedType.Bool)]
            internal static extern bool AssignProcessToJobObject(SafeFileHandle job, SafeProcessHandle process);
            [DllImport("kernel32.dll", ExactSpelling = true, SetLastError = true)]
            [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
            [return: MarshalAs(UnmanagedType.Bool)]
            internal static extern bool IsProcessInJob(SafeProcessHandle process, SafeFileHandle job, [MarshalAs(UnmanagedType.Bool)] out bool assigned);
            [DllImport("kernel32.dll", ExactSpelling = true, SetLastError = true)]
            [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
            [return: MarshalAs(UnmanagedType.Bool)]
            internal static extern bool TerminateJobObject(SafeFileHandle job, uint exitCode);
        }
    }
}
'@
}

function Invoke-BonsaiNativeProcess {
    <#
    .SYNOPSIS
    Captures an ordinary tool's separate output streams with owned process cleanup.
    .DESCRIPTION
    Inherits the host environment and passes arguments without a shell. A zero
    timeout leaves execution unbounded; output drainage is bounded after exit.
    Nonzero exit codes are returned for the caller to interpret. Optional logs
    contain stdout followed by stderr, including when execution times out.
    OwnDescendants gates a trusted trampoline into a Windows kill-on-close Job
    before it starts the tool. This is reserved for cold compiler bootstrap;
    resource-limited model processes use the separate Rust Windows Job launcher.
    #>
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param(
        [Parameter(Mandatory)][string]$Executable,
        [AllowEmptyCollection()][AllowEmptyString()][string[]]$ArgumentList = @(),
        [Parameter(Mandatory)][string]$WorkingDirectory,
        [ValidateRange(0,86400)][int]$TimeoutSeconds = 0,
        [Collections.IDictionary]$Environment,
        [string]$LogPath,
        [switch]$OwnDescendants
    )
    if (-not [IO.Path]::IsPathFullyQualified($WorkingDirectory) -or $WorkingDirectory -match '%[^%]+%' -or
        -not [IO.Directory]::Exists($WorkingDirectory)) {
        throw 'Native working directory must be an existing expanded absolute directory.'
    }
    if ($LogPath -and (-not [IO.Path]::IsPathFullyQualified($LogPath) -or $LogPath -match '%[^%]+%')) {
        throw 'Native log path must be an expanded absolute path.'
    }
    $start = [Diagnostics.ProcessStartInfo]::new($Executable)
    $start.WorkingDirectory = $WorkingDirectory
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    foreach ($argument in $ArgumentList) { $start.ArgumentList.Add($argument) }
    if ($Environment) {
        foreach ($key in $Environment.Keys) { $start.Environment[$key] = [string]$Environment[$key] }
    }
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $start
    $started = $false
    $job = $null
    try {
        if ($OwnDescendants) {
            if (-not $IsWindows) { throw 'Compiler bootstrap ownership requires Windows.' }
            Initialize-BonsaiBootstrapJob
            $job = [Bonsai.BootstrapJob]::new()
            # No target tool can run before assignment: the no-profile trampoline
            # waits for one input line. Owner death before assignment closes stdin,
            # producing EOF and exit without launching a compiler. After assignment,
            # Job inheritance and kill-on-close cover the complete descendant tree.
            $trampoline = @'
$ErrorActionPreference = 'Stop'
[Console]::InputEncoding = [Text.UTF8Encoding]::new($false)
$line = [Console]::In.ReadLine()
if ($null -eq $line) { exit 125 }
$request = ConvertFrom-Json -InputObject $line -AsHashtable -NoEnumerate -DateKind String
$start = [Diagnostics.ProcessStartInfo]::new($request.executable)
$start.WorkingDirectory = $request.workingDirectory
$start.UseShellExecute = $false
$start.CreateNoWindow = $true
# Redirecting only stdin makes .NET preserve the inherited stdout/stderr handles;
# close the new input pipe immediately so an interactive tool cannot read the gate.
$start.RedirectStandardInput = $true
foreach ($argument in $request.arguments) { $start.ArgumentList.Add($argument) }
$child = [Diagnostics.Process]::Start($start)
try { $child.StandardInput.Close(); $child.WaitForExit(); exit $child.ExitCode } finally { $child.Dispose() }
'@
            $start.FileName = [Environment]::ProcessPath
            $start.ArgumentList.Clear()
            foreach ($argument in @('-NoLogo', '-NoProfile', '-NonInteractive', '-EncodedCommand', [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($trampoline)))) { $start.ArgumentList.Add($argument) }
            $start.RedirectStandardInput = $true
            $start.StandardInputEncoding = [Text.UTF8Encoding]::new($false)
        }
        if (-not $process.Start()) { throw "Native process failed to start: $Executable" }
        $started = $true
        # Drain both pipes concurrently so compiler output cannot block the child.
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        if ($OwnDescendants) {
            $job.Assign($process)
            $request = ConvertTo-Json -Compress -InputObject @{ executable = $Executable; arguments = @($ArgumentList); workingDirectory = $WorkingDirectory }
            $process.StandardInput.WriteLine($request)
            $process.StandardInput.Close()
        }
        $timedOut = $false
        $leftDescendants = $false
        if ($TimeoutSeconds -eq 0) { $process.WaitForExit() }
        elseif (-not $process.WaitForExit($TimeoutSeconds * 1000)) {
            $timedOut = $true
            if ($null -ne $job) { $null = $job.Stop(10000) } else { $process.Kill($true) }
            if (-not $process.WaitForExit(10000)) { throw "Native process did not terminate: $Executable" }
        }
        if ($null -ne $job) {
            # Process signaling can precede removal from Job active accounting.
            # Only normal root exit permits a short natural-empty grace; timeout
            # and finally cleanup still terminate the complete Job immediately.
            $leftDescendants = if ($timedOut) { $job.Stop(10000) } else { $job.StopAfterExit(10000) }
        }
        # A descendant may keep a redirected pipe open after the root exits.
        if (-not $stdout.Wait(10000) -or -not $stderr.Wait(10000)) { throw "Native process output drain timed out: $Executable" }
        $result = [pscustomobject]@{
            ExitCode = $process.ExitCode
            StandardOutput = $stdout.GetAwaiter().GetResult()
            StandardError = $stderr.GetAwaiter().GetResult()
        }
        if ($LogPath) {
            $survivors = if ($leftDescendants) { "`nBootstrap job survivors: $($job.RemainingProcessEvidence)`n" } else { '' }
            [IO.File]::WriteAllText($LogPath, $result.StandardOutput + $result.StandardError + $survivors)
        }
        if ($timedOut) {
            $evidence = if ($LogPath) { "; see $LogPath" } else { '' }
            throw "Native process exceeded $TimeoutSeconds seconds: $Executable$evidence"
        }
        if ($leftDescendants) { throw "Native process left running descendants: $Executable; job=$($job.RemainingProcessEvidence); root-exited=$($process.HasExited)" }
        return $result
    } finally {
        try {
            if ($null -ne $job) { $null = $job.Stop(10000) }
            if ($started -and -not $process.HasExited) {
                $process.Kill($true)
                if (-not $process.WaitForExit(10000)) { throw "Native process did not terminate during cleanup: $Executable" }
            }
        } finally {
            if ($null -ne $job) { $job.Dispose() }
            $process.Dispose()
        }
    }
}

Export-ModuleMember -Function Invoke-BonsaiNativeProcess
