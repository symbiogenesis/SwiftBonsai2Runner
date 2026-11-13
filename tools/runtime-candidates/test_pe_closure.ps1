#requires -Version 7.6
[CmdletBinding()]
param([ValidateSet('candidate_gui','system_gui','optional_delay','api_alias','malformed','headers','raw_extent','namespace')][string[]]$Case = @('candidate_gui','system_gui','optional_delay','api_alias','malformed','headers','raw_extent','namespace'))
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'pe_closure.ps1')
function Assert($Condition, [string]$Message) { if (-not $Condition) { throw $Message } }
function Reject([scriptblock]$Action, [string]$Pattern = '.') {
    $caught = $false
    try { & $Action | Out-Null } catch { $caught = $true; Assert ($_.Exception.Message -match $Pattern) "Unexpected rejection: $_" }
    Assert $caught 'Expected rejection'
}
function Put16([byte[]]$Data, [int]$Offset, [uint16]$Value) { [BitConverter]::GetBytes($Value).CopyTo($Data, $Offset) }
function Put32([byte[]]$Data, [int]$Offset, [uint32]$Value) { [BitConverter]::GetBytes($Value).CopyTo($Data, $Offset) }
function Image([string]$Path, [string[]]$Normal = @(), [string[]]$Delayed = @()) {
    $data = [byte[]]::new(0x1400); $data[0] = 77; $data[1] = 90
    Put32 $data 60 0x80; Put32 $data 0x80 0x4550; Put16 $data 0x84 0xAA64; Put16 $data 0x86 1
    Put16 $data 0x94 240; Put16 $data 0x98 0x20B; Put32 $data (0x98 + 108) 16
    [Text.Encoding]::ASCII.GetBytes('.rdata').CopyTo($data, 0x188)
    Put32 $data 0x190 0x1000; Put32 $data 0x194 0x1000; Put32 $data 0x198 0x1000; Put32 $data 0x19c 0x400
    foreach ($spec in @(@{ Names = $Normal; Directory = 1; Start = 0x1000; Width = 20; Strings = 0x1100 },
        @{ Names = $Delayed; Directory = 13; Start = 0x1300; Width = 32; Strings = 0x1500 })) {
        if ($spec.Names.Count) { Put32 $data (0x98 + 112 + $spec.Directory * 8) $spec.Start; Put32 $data (0x98 + 116 + $spec.Directory * 8) (($spec.Names.Count + 1) * $spec.Width) }
        for ($i = 0; $i -lt $spec.Names.Count; $i++) {
            $record = $spec.Start - 0x1000 + 0x400 + $i * $spec.Width; $rva = $spec.Strings + $i * 80
            Put32 $data ($record + $(if ($spec.Directory -eq 13) { 4 } else { 12 })) $rva
            if ($spec.Directory -eq 13) { Put32 $data $record 1 }
            [Text.Encoding]::ASCII.GetBytes($spec.Names[$i]).CopyTo($data, ($rva - 0x1000 + 0x400))
        }
    }
    [IO.File]::WriteAllBytes($Path, $data)
}
function NamespaceImage([string]$Path) {
    Image $Path
    $data = [IO.File]::ReadAllBytes($Path)
    [Array]::Clear($data, 0x188, 8); [Text.Encoding]::ASCII.GetBytes('.apiset').CopyTo($data, 0x188)
    $space = [byte[]]::new(200); $name = [Text.Encoding]::Unicode.GetBytes('api-ms-win-core-test-l1-1-2'); $hostName = [Text.Encoding]::Unicode.GetBytes('kernel32.dll')
    Put32 $space 0 6; Put32 $space 4 200; Put32 $space 12 1; Put32 $space 16 28; Put32 $space 24 31
    Put32 $space 32 52; Put32 $space 36 $name.Length; Put32 $space 40 ($name.Length - 4); Put32 $space 44 120; Put32 $space 48 1
    $name.CopyTo($space, 52); Put32 $space 132 140; Put32 $space 136 $hostName.Length; $hostName.CopyTo($space, 140)
    $space.CopyTo($data, 0x400); [IO.File]::WriteAllBytes($Path, $data)
}
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('pe-closure-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($scratch)
try {
    foreach ($selected in $Case) {
        $root = Join-Path $scratch $selected; $bundlePath = Join-Path $root 'bundle'; $systemPath = Join-Path $root 'system'
        [void][IO.Directory]::CreateDirectory($bundlePath); [void][IO.Directory]::CreateDirectory($systemPath)
        $common = Join-Path $bundlePath 'common.dll'; $kernel = Join-Path $systemPath 'kernel32.dll'
        Image (Join-Path $bundlePath 'candidate.exe') @('common.dll'); Image $common @('kernel32.dll'); Image $kernel
        switch ($selected) {
            candidate_gui { Image $common @('shell32.dll'); Reject { Invoke-PeClosure $bundlePath $systemPath @{} } 'GUI dependency'; Image $common @() @('user32.dll'); Reject { Invoke-PeClosure $bundlePath $systemPath @{} } 'GUI dependency' }
            system_gui { Image $kernel @('gdi32.dll'); Reject { Invoke-PeClosure $bundlePath $systemPath @{} } 'GUI dependency' }
            optional_delay { Image $kernel @() @('user32.dll'); $r = Invoke-PeClosure $bundlePath $systemPath @{}; $k = @($r.modules | Where-Object { $_.name -eq 'kernel32.dll' })[0]; Assert ($k.delayImports[0] -eq 'user32.dll' -and -not $r.modelExecuted) 'Lost optional delay scope' }
            api_alias {
                $name = 'api-ms-win-core-test-l1-1-0.dll'; Image $common @($name)
                foreach ($alias in @($name, 'api-ms-win-core-test-l1-1-*.dll')) {
                    $r = Invoke-PeClosure $bundlePath $systemPath @{ $alias = ,@('', 'kernel32.dll') }; Assert (@($r.requiredEdges | Where-Object { $_.import -eq $name -and $_.resolved -eq 'kernel32.dll' }).Count -eq 1) 'Missing alias resolution'
                }
                Reject { Invoke-PeClosure $bundlePath $systemPath @{ $name = @() } } 'Unresolved API-set'
                Reject { Invoke-PeClosure $bundlePath $systemPath @{ $name = ,@('', 'user32.dll') } } 'GUI dependency'
            }
            malformed { [IO.File]::Delete($kernel); Reject { Invoke-PeClosure $bundlePath $systemPath @{} }; [IO.File]::WriteAllBytes($kernel, @(77,90)); Reject { Read-PeImage $kernel }; Image $kernel; $data = [IO.File]::ReadAllBytes($common); Put32 $data (0x98 + 124) 20; [IO.File]::WriteAllBytes($common, $data); Reject { Invoke-PeClosure $bundlePath $systemPath @{} } 'Unterminated import' }
            headers { foreach ($spec in @(@(0x94,111),@(0x94,224),@((0x98 + 108),1),@((0x98 + 108),17))) { Image $common @('kernel32.dll'); $data = [IO.File]::ReadAllBytes($common); if ($spec[0] -eq 0x94) { Put16 $data $spec[0] $spec[1] } else { Put32 $data $spec[0] $spec[1] }; [IO.File]::WriteAllBytes($common, $data); Reject { Read-PeImage $common } 'optional header|directory count' } }
            raw_extent { foreach ($directory in @(1,13)) { Image $common; $data = [IO.File]::ReadAllBytes($common); [Array]::Resize([ref]$data, ($data.Length + 4096)); Put32 $data (0x98 + 112 + $directory * 8) 0x1ff0; Put32 $data (0x98 + 116 + $directory * 8) $(if ($directory -eq 1) { 40 } else { 64 }); [IO.File]::WriteAllBytes($common, $data); Reject { Get-PeImports (Read-PeImage $common) -Delayed:($directory -eq 13) } 'extent outside raw section' } }
            namespace {
                $schema = Join-Path $systemPath 'apisetschema.dll'; NamespaceImage $schema; $mapping = Get-PeApiSets $systemPath; Assert ($mapping['api-ms-win-core-test-l1-1-2.dll'][0][1] -eq 'kernel32.dll') 'Missing namespace host'
                foreach ($spec in @(@(4,27),@(4,40),@(16,190),@(32,180),@(44,190),@(132,184))) { NamespaceImage $schema; $data = [IO.File]::ReadAllBytes($schema); Put32 $data (0x400 + $spec[0]) $spec[1]; [IO.File]::WriteAllBytes($schema, $data); Reject { Get-PeApiSets $systemPath } }
            }
        }
        Write-Output "PASS $selected"
    }
} finally { [IO.Directory]::Delete($scratch, $true) }
