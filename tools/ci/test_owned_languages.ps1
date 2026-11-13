#Requires -Version 7.6
param([string]$Case)
. (Join-Path $PSScriptRoot 'test_support.ps1')
. (Join-Path $PSScriptRoot 'check_owned_languages.ps1')
function Invoke-LanguageFixture([scriptblock]$Work) { $scratch=New-CiScratch 'bonsai-language-policy-'; try { & $Work $scratch } finally { Remove-CiScratch $scratch } }
$cases=[ordered]@{
 owned_source_and_bytecode_rejected={ Invoke-LanguageFixture { param($r) foreach ($name in @('tools/a.py','tools/b.PY','scripts/c.pyc','tools/d.pyo','tools/e.pyw')) { Write-CiFixture $r $name 'fixture' }; Assert-CiEqual (Test-OwnedLanguages $r).Count 5 } }
 runtime_setup_and_commands_rejected={ Invoke-LanguageFixture { param($r) Write-CiFixture $r '.github/workflows/control.yml' "steps:`n  - uses: actions/setup-python@fixture`n    shell: python`n    run: python3 -B tools/control.py`n    run: PYTHON3.14 -B tools/control.py`n"; Assert-CiEqual (Test-OwnedLanguages $r).Count 4 } }
 powershell_literal_interpreters_rejected={ Invoke-LanguageFixture { param($r)
    $commands=@('python3.14 -B control.py', '& "C:\Tools\Python.EXE" control.py', "& '/opt/bin/python3' control.py", 'py control.py',
        'Start-Process -FilePath "python.exe" -Wait', 'Start-Process PYTHON3 -Wait', 'saps -FilePath:pythonw.exe', '$result = & python3 control.py; Write-Output $result')
    Write-CiFixture $r 'tools/control.PS1' ($commands -join "`n")
    Assert-CiEqual (Test-OwnedLanguages $r).Count $commands.Count
 } }
 powershell_provenance_strings_preserved={ Invoke-LanguageFixture { param($r)
    $source=@('# python control.py', '$historical = ''python control.py''', 'Write-Output "Start-Process python.exe"',
        'Start-Process bonsai-tools.exe -ArgumentList "python is historical metadata"', '& pwsh -NoProfile -File control.ps1')
    Write-CiFixture $r 'tools/control.ps1' ($source -join "`n")
    Assert-CiEqual (Test-OwnedLanguages $r) @()
 } }
 historical_metadata_and_vendor_sources_preserved={ Invoke-LanguageFixture { param($r) Write-CiFixture $r 'docs/history.md' 'Historical python tools/a.py identity'; Write-CiFixture $r 'tools/schema.rs' 'const HISTORICAL_PATH: &str = "original/export.py";'; Write-CiFixture $r 'third_party/vendor/a.py' 'external source'; Write-CiFixture $r '.worktrees/worker/a.py' 'isolated parallel source'; Assert-CiEqual (Test-OwnedLanguages $r) @() } }
 rust_multiline_runtime_resolution_rejected={ Invoke-LanguageFixture { param($r) Write-CiFixture $r 'tools/runtime.rs' "let executable = startup_process::resolve(`n OsStr::new(`"python.exe`"));`n"; Assert-Ci ((Test-OwnedLanguages $r).Count -gt 0) } }
 allowed_rust_and_powershell_commands_pass={ Invoke-LanguageFixture { param($r) Write-CiFixture $r 'tools/control.ps1' "#Requires -Version 7.6`n& pwsh -NoProfile -File tools/check.ps1`n"; Write-CiFixture $r '.github/workflows/check.yml' "shell: pwsh`nrun: cargo check --locked`n"; Write-CiFixture $r 'src/main.rs' 'fn main() {}'; Assert-CiEqual (Test-OwnedLanguages $r) @() } }
}
Invoke-CiCases $cases $Case
