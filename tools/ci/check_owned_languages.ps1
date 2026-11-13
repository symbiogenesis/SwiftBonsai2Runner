#Requires -Version 7.6
<# Repository tooling uses Rust or PowerShell 7.6+. Historical receipts, external artifact identities and vendored sources are data. #>
[CmdletBinding()]
param([string]$Root=(Split-Path (Split-Path $PSScriptRoot -Parent) -Parent))
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
function Test-OwnedLanguages([string]$RepositoryRoot) {
    $errors=[Collections.Generic.List[string]]::new()
    $pending=[Collections.Generic.Stack[string]]::new(); $pending.Push([IO.Path]::GetFullPath($RepositoryRoot))
    while ($pending.Count) {
        $directory=$pending.Pop()
        foreach ($entry in [IO.Directory]::EnumerateFileSystemEntries($directory)) {
            $relative=[IO.Path]::GetRelativePath($RepositoryRoot,$entry).Replace('\','/')
            $attributes=[IO.File]::GetAttributes($entry)
            if (($attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { continue }
            if (($attributes -band [IO.FileAttributes]::Directory) -ne 0) {
                if ($relative -cin @('.git','.worktrees','target','third_party') -or [IO.Path]::GetFileName($entry) -ceq 'target') { continue }; $pending.Push($entry); continue
            }
            $extension=[IO.Path]::GetExtension($entry)
            if ($extension -in @('.py','.pyc','.pyo','.pyw')) { $errors.Add("${relative}: owned Python code/runtime bytecode is prohibited"); continue }
            if ($extension -cnotin @('.ps1','.psm1','.rs','.yml','.yaml','.sh') -and $relative -cne '.githooks/pre-commit') { continue }
            $lineNumber=0
            foreach ($line in [IO.File]::ReadAllLines($entry)) {
                $lineNumber++
                if ($line -cmatch '^\s*(?:-\s*)?(?:run:\s*)?(?:&\s*)?(?:python(?:3(?:\.[0-9]+)?)?(?:\.exe)?|py\s+-3)(?:\s|$)' -or $line -cmatch '^\s*shell:\s*[''"]?python(?:3)?(?:\s|[''"]|$)' -or $line -cmatch '^\s*(?:-\s*)?uses:\s*[''"]?actions/setup-python@' -or $line -cmatch '(?:Command::new|startup_process::resolve|::resolve)\([^;]*["'']python(?:3)?(?:\.exe)?["'']') { $errors.Add("${relative}:${lineNumber}: Python execution/setup is prohibited") }
            }
            if ($extension -ceq '.rs' -and [regex]::IsMatch([IO.File]::ReadAllText($entry),'(?s)(?:Command::new|(?:startup_process::)?resolve)\(\s*(?:OsStr::new\(\s*)?"python(?:3)?(?:\.exe)?"')) { $errors.Add("${relative}: Python runtime process resolution is prohibited") }
        }
    }
    return ,$errors.ToArray()
}
if ($MyInvocation.InvocationName -ne '.') {
    $failures=Test-OwnedLanguages $Root; foreach ($failure in $failures) { [Console]::Error.WriteLine($failure) }; if ($failures.Count) { exit 1 }; 'Owned source-language policy passed: Rust and PowerShell tooling without Python runtime setup or execution.'
}
