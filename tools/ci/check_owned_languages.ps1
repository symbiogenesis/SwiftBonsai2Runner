#Requires -Version 7.6
<# Repository tooling uses Rust or PowerShell 7.6+. Historical receipts, external artifact identities and vendored sources are data. #>
[CmdletBinding()]
param([string]$Root=(Split-Path (Split-Path $PSScriptRoot -Parent) -Parent))
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'

function Test-PythonInterpreter([string]$Name) {
    [IO.Path]::GetFileName($Name.Replace('\','/')) -imatch '^(?:python(?:[23](?:\.[0-9]+)*)?w?|py)(?:\.exe)?$'
}

function Get-PowerShellPythonCalls([string]$Path) {
    $tokens=$null; $parseErrors=$null
    $syntax=[Management.Automation.Language.Parser]::ParseFile($Path,[ref]$tokens,[ref]$parseErrors)
    $commands=$syntax.FindAll({ param($node) $node -is [Management.Automation.Language.CommandAst] },$true)
    foreach ($command in $commands) {
        $name=$command.GetCommandName()
        if (-not $name) { continue }
        $interpreter=Test-PythonInterpreter $name
        if ([IO.Path]::GetFileName($name.Replace('\','/')) -iin @('Start-Process','saps','start')) {
            # Only literal executable arguments are commands; quoted fixture/provenance text is data.
            $elements=$command.CommandElements
            for ($index=1; $index -lt $elements.Count; $index++) {
                $element=$elements[$index]
                if ($element -is [Management.Automation.Language.CommandParameterAst] -and $element.ParameterName -ieq 'FilePath') {
                    $element=if ($null -ne $element.Argument) { $element.Argument } elseif ($index+1 -lt $elements.Count) { $elements[$index+1] } else { $null }
                } elseif ($index -ne 1) { continue }
                if ($element -is [Management.Automation.Language.StringConstantExpressionAst] -and (Test-PythonInterpreter $element.Value)) { $interpreter=$true }
            }
        }
        if ($interpreter) { $command.Extent.StartLineNumber }
    }
}

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
            if ($extension -notin @('.ps1','.psm1','.rs','.yml','.yaml','.sh') -and $relative -cne '.githooks/pre-commit') { continue }
            if ($extension -in @('.ps1','.psm1')) {
                foreach ($lineNumber in (Get-PowerShellPythonCalls $entry)) { $errors.Add("${relative}:${lineNumber}: Python execution is prohibited") }
                continue
            }
            $lineNumber=0
            foreach ($line in [IO.File]::ReadAllLines($entry)) {
                $lineNumber++
                if ($line -imatch '^\s*(?:-\s*)?(?:run:\s*)?(?:&\s*)?[''"]?(?:python(?:[23](?:\.[0-9]+)*)?w?|py)(?:\.exe)?(?:\s|[''"]|$)' -or $line -imatch '^\s*shell:\s*[''"]?python(?:[23](?:\.[0-9]+)*)?(?:\s|[''"]|$)' -or $line -imatch '^\s*(?:-\s*)?uses:\s*[''"]?actions/setup-python@' -or $line -imatch '(?:Command::new|startup_process::resolve|::resolve)\([^;]*["''](?:python(?:[23](?:\.[0-9]+)*)?w?|py)(?:\.exe)?["'']') { $errors.Add("${relative}:${lineNumber}: Python execution/setup is prohibited") }
            }
            if ($extension -ieq '.rs' -and [regex]::IsMatch([IO.File]::ReadAllText($entry),'(?is)(?:Command::new|(?:startup_process::)?resolve)\(\s*(?:OsStr::new\(\s*)?"(?:python(?:[23](?:\.[0-9]+)*)?w?|py)(?:\.exe)?"')) { $errors.Add("${relative}: Python runtime process resolution is prohibited") }
        }
    }
    return ,$errors.ToArray()
}
if ($MyInvocation.InvocationName -ne '.') {
    $failures=Test-OwnedLanguages $Root; foreach ($failure in $failures) { [Console]::Error.WriteLine($failure) }; if ($failures.Count) { exit 1 }; 'Owned source-language policy passed: Rust and PowerShell tooling without Python runtime setup or execution.'
}
