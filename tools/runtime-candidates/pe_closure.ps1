#requires -Version 7.6
<# Bounded static ARM64 PE import audit. Never loads a DLL or executes a target. #>
[CmdletBinding()]
param([string]$Bundle, [string]$System, [string]$Output)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Assert-PeExtent([byte[]]$Data, [long]$Offset, [long]$Size) {
    if ($Offset -lt 0 -or $Size -lt 0 -or $Offset -gt $Data.LongLength -or $Size -gt $Data.LongLength - $Offset) {
        throw 'PE extent outside file'
    }
}
function Read-PeU16([byte[]]$Data, [long]$Offset) { Assert-PeExtent $Data $Offset 2; [BitConverter]::ToUInt16($Data, $Offset) }
function Read-PeU32([byte[]]$Data, [long]$Offset) { Assert-PeExtent $Data $Offset 4; [BitConverter]::ToUInt32($Data, $Offset) }
function Read-PeU64([byte[]]$Data, [long]$Offset) { Assert-PeExtent $Data $Offset 8; [BitConverter]::ToUInt64($Data, $Offset) }
function Read-PeImage([string]$Path) {
    $item = Get-Item -LiteralPath $Path -Force
    if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Dependency must be a regular file' }
    $stream = [IO.File]::Open($Path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
    try {
        if ($stream.Length -gt 33554432) { throw 'PE exceeds static read bound' }
        $data = [byte[]]::new($stream.Length)
        $stream.ReadExactly($data)
        if ($stream.ReadByte() -ne -1) { throw 'PE exceeds static read bound' }
    } finally { $stream.Dispose() }
    Assert-PeExtent $data 0 64
    if ($data[0] -ne 77 -or $data[1] -ne 90) { throw 'Missing DOS signature' }
    $pe = [long](Read-PeU32 $data 60)
    if ((Read-PeU32 $data $pe) -ne 0x4550 -or (Read-PeU16 $data ($pe + 4)) -ne 0xAA64) { throw 'Expected native ARM64 PE' }
    $optional = $pe + 24
    $size = Read-PeU16 $data ($pe + 20)
    if ($size -lt 112) { throw 'Truncated PE32+ optional header' }
    Assert-PeExtent $data $optional $size
    if ((Read-PeU16 $data $optional) -ne 0x20b) { throw 'Expected PE32+' }
    $count = Read-PeU32 $data ($optional + 108)
    if ($count -lt 14 -or $count -gt [Math]::Floor(($size - 112) / 8)) { throw 'PE directory count exceeds optional header or omits import tables' }
    $sections = [Collections.Generic.List[object]]::new()
    $section = $optional + $size
    $count = Read-PeU16 $data ($pe + 6)
    if ($count -gt 96) { throw 'Too many PE sections' }
    Assert-PeExtent $data $section ($count * 40)
    for ($index = 0; $index -lt $count; $index++) {
        $header = $section + $index * 40
        $entry = @{ Name = [Text.Encoding]::ASCII.GetString($data, $header, 8).TrimEnd([char]0)
            VirtualSize = [long](Read-PeU32 $data ($header + 8)); Virtual = [long](Read-PeU32 $data ($header + 12))
            RawSize = [long](Read-PeU32 $data ($header + 16)); Raw = [long](Read-PeU32 $data ($header + 20)) }
        Assert-PeExtent $data $entry.Raw $entry.RawSize
        $sections.Add($entry)
    }
    @{ Path = $Path; Data = $data; Optional = $optional; Base = (Read-PeU64 $data ($optional + 24)); Sections = $sections }
}
function Get-PeExtent($Image, [long]$Rva, [long]$Size) {
    foreach ($section in $Image.Sections) {
        if ($Rva -ge $section.Virtual -and $Rva -lt $section.Virtual + [Math]::Max($section.VirtualSize, $section.RawSize)) {
            $delta = $Rva - $section.Virtual
            if ($Size -lt 0 -or $delta + $Size -gt $section.RawSize) { throw 'PE RVA extent outside raw section' }
            Assert-PeExtent $Image.Data ($section.Raw + $delta) $Size
            return @{ Offset = $section.Raw + $delta; Available = $section.RawSize - $delta }
        }
    }
    throw 'Unmapped PE RVA'
}
function Get-PeModule($Image, [long]$Rva) {
    $extent = Get-PeExtent $Image $Rva 1
    $size = [Math]::Min(260, $extent.Available)
    $end = [Array]::IndexOf[byte]($Image.Data, 0, [int]$extent.Offset, [int]$size) - $extent.Offset
    if ($end -lt 1) { throw 'Unterminated import name' }
    $name = [Text.Encoding]::ASCII.GetString($Image.Data, $extent.Offset, $end).ToLowerInvariant()
    if ($name -cnotmatch '^[a-z0-9_.-]+\.dll$') { throw 'Unsafe import module name' }
    $name
}
function Get-PeImports($Image, [switch]$Delayed) {
    $directory = $Image.Optional + 112 + $(if ($Delayed) { 13 } else { 1 }) * 8
    $rva = Read-PeU32 $Image.Data $directory
    $size = Read-PeU32 $Image.Data ($directory + 4)
    if ($rva -eq 0 -and $size -eq 0) { return ,@() }
    $width = if ($Delayed) { 32 } else { 20 }
    if ($rva -eq 0 -or $size -lt $width -or $size -gt 65536) { throw 'Invalid import directory extent' }
    $start = (Get-PeExtent $Image $rva $size).Offset
    $result = [Collections.Generic.List[string]]::new()
    for ($position = 0; $position -le $size - $width; $position += $width) {
        $values = @(for ($i = 0; $i -lt $width; $i += 4) { Read-PeU32 $Image.Data ($start + $position + $i) })
        if (@($values | Where-Object { $_ -ne 0 }).Count -eq 0) { return ,$result.ToArray() }
        $name = [long]$values[$(if ($Delayed) { 1 } else { 3 })]
        if ($Delayed -and -not ($values[0] -band 1)) { $name -= $Image.Base }
        $result.Add((Get-PeModule $Image $name))
    }
    throw 'Unterminated import directory'
}
function Read-ApiText([byte[]]$Data, [long]$Offset, [long]$Length) {
    if ($Length % 2) { throw 'Invalid API-set name extent' }
    Assert-PeExtent $Data $Offset $Length
    [Text.UnicodeEncoding]::new($false, $false, $true).GetString($Data, $Offset, $Length).ToLowerInvariant()
}
function Get-PeApiSets([string]$SystemDirectory) {
    $image = Read-PeImage (Join-Path $SystemDirectory 'apisetschema.dll')
    $sections = @($image.Sections | Where-Object Name -CEQ '.apiset')
    if ($sections.Count -ne 1) { throw 'Expected one API-set namespace' }
    $section = $sections[0]
    $data = [byte[]]::new($section.RawSize)
    [Array]::Copy($image.Data, $section.Raw, $data, 0, $data.Length)
    $size = Read-PeU32 $data 4
    if ($data.Length -lt 28 -or (Read-PeU32 $data 0) -ne 6 -or $size -lt 28 -or $size -gt $data.Length) { throw 'Unsupported API-set namespace' }
    [Array]::Resize([ref]$data, [int]$size)
    $count = Read-PeU32 $data 12; $offset = Read-PeU32 $data 16
    if ($count -gt 4096 -or $offset -lt 28) { throw 'API-set entry table outside declared namespace' }
    Assert-PeExtent $data $offset ($count * 24)
    $result = @{}
    for ($i = 0; $i -lt $count; $i++) {
        $entry = $offset + $i * 24
        $length = Read-PeU32 $data ($entry + 8); $hashedLength = Read-PeU32 $data ($entry + 12)
        if ($hashedLength -eq 0 -or $hashedLength -gt $length) { throw 'Invalid API-set hashed name extent' }
        $name = (Read-ApiText $data (Read-PeU32 $data ($entry + 4)) $length) + '.dll'
        $hashed = (Read-ApiText $data (Read-PeU32 $data ($entry + 4)) $hashedLength) + '-*.dll'
        $hosts = [Collections.Generic.List[object]]::new()
        $values = Read-PeU32 $data ($entry + 16); $hostCount = Read-PeU32 $data ($entry + 20)
        if ($hostCount -gt 64) { throw 'API-set host table outside declared namespace' }
        Assert-PeExtent $data $values ($hostCount * 20)
        for ($j = 0; $j -lt $hostCount; $j++) {
            $value = $values + $j * 20
            $hosts.Add(@((Read-ApiText $data (Read-PeU32 $data ($value + 4)) (Read-PeU32 $data ($value + 8))),
                (Read-ApiText $data (Read-PeU32 $data ($value + 12)) (Read-PeU32 $data ($value + 16)))))
        }
        $result[$name] = $hosts.ToArray()
        if ($result.ContainsKey($hashed) -and (ConvertTo-Json -InputObject $result[$hashed] -Compress) -cne (ConvertTo-Json -InputObject $hosts.ToArray() -Compress)) { throw 'Ambiguous API-set hashed contract' }
        $result[$hashed] = $hosts.ToArray()
    }
    $result
}
function Invoke-PeClosure([string]$BundleDirectory, [string]$SystemDirectory, [hashtable]$Aliases) {
    foreach ($path in @($BundleDirectory, $SystemDirectory)) {
        $item = Get-Item -LiteralPath $path -Force
        if (-not $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Redirected audit directory' }
    }
    $candidates = @{}
    foreach ($item in [IO.Directory]::EnumerateFiles($BundleDirectory)) {
        if ([IO.Path]::GetExtension($item) -in '.dll', '.exe') { $candidates[[IO.Path]::GetFileName($item).ToLowerInvariant()] = $item }
        if ($candidates.Count -gt 64) { throw 'Candidate inventory outside bound' }
    }
    if ($candidates.Count -eq 0) { throw 'Candidate inventory outside bound' }
    if ($null -eq $Aliases) { $Aliases = Get-PeApiSets $SystemDirectory }
    $pending = [Collections.Generic.Stack[object]]::new()
    foreach ($name in $candidates.Keys) { $pending.Push(@($name, 'candidate')) }
    $modules = @{}; $edges = [Collections.Generic.List[object]]::new()
    $gui = @('shell32.dll','user32.dll','gdi32.dll','gdi32full.dll','win32u.dll','comdlg32.dll','comctl32.dll','dwmapi.dll','uxtheme.dll','imm32.dll')
    while ($pending.Count) {
        $next = $pending.Pop(); $name = $next[0]; $location = $next[1]; $key = "$location/$name"
        if ($modules.ContainsKey($key)) { continue }
        if ($name -in $gui) { throw "GUI dependency: $name" }
        if ($modules.Count -ge 128) { throw 'Dependency closure exceeds module bound' }
        $path = if ($location -eq 'candidate') { $candidates[$name] } else { Join-Path $SystemDirectory $name }
        $image = Read-PeImage $path
        $metadata = @{ name = [IO.Path]::GetFileName($path); bytes = $image.Data.Length
            sha256 = [Convert]::ToHexStringLower([Security.Cryptography.SHA256]::HashData($image.Data)); machine = '0xAA64'
            normalImports = (Get-PeImports $image); delayImports = (Get-PeImports $image -Delayed); location = $location }
        $modules[$key] = $metadata
        $required = @($metadata.normalImports) + $(if ($location -eq 'candidate') { @($metadata.delayImports) } else { @() })
        foreach ($dependency in $required) {
            $resolved = $dependency; $hashed = $dependency -replace '-[^-]+$', '-*.dll'
            $hosts = $null
            if ($Aliases.ContainsKey($dependency)) { $hosts = $Aliases[$dependency] }
            elseif ($Aliases.ContainsKey($hashed)) { $hosts = $Aliases[$hashed] }
            if ($null -ne $hosts) {
                $matches = @($hosts | Where-Object { $_[0] -ceq $name -or $_[0] -ceq ($name -replace '\.dll$', '') } | ForEach-Object { $_[1] })
                if (-not $matches.Count) { $matches = @($hosts | Where-Object { $_[0] -ceq '' } | ForEach-Object { $_[1] }) }
                $unique = @($matches | Select-Object -Unique)
                if ($unique.Count -ne 1 -or -not $unique[0]) { throw "Unresolved API-set host: $dependency" }
                $resolved = $unique[0]
            } elseif ($Aliases.ContainsKey($dependency) -or $Aliases.ContainsKey($hashed)) { throw "Unresolved API-set host: $dependency" }
            if ($resolved -cnotmatch '^[a-z0-9_.-]+\.dll$') { throw 'Unsafe resolved module name' }
            $destination = if ($candidates.ContainsKey($resolved)) { 'candidate' } else { 'system' }
            $edges.Add(@{ from = $name; import = $dependency; resolved = $resolved; location = $destination })
            $pending.Push(@($resolved, $destination))
        }
    }
    @{ schemaVersion = 1; status = 'candidate-and-mandatory-named-import-closure-gui-free'
        scope = 'All candidate normal/delay imports and recursive system normal imports; optional OS delays are recorded, not followed or claimed absent. No DLL was loaded.'
        forwardedExportsResolved = $false; modules = @($modules.Values | Sort-Object { $_.location }, { $_.name })
        requiredEdges = $edges.ToArray(); modelExecuted = $false; workerPolicyChanged = $false }
}
if ($MyInvocation.InvocationName -ne '.') {
    if (-not $Bundle -or -not $System -or -not $Output) { throw 'Supply -Bundle, -System and -Output.' }
    $code = 0
    try { $report = Invoke-PeClosure $Bundle $System $null }
    catch { $report = @{ schemaVersion = 1; status = 'rejected'; error = $_.Exception.Message; modelExecuted = $false }; $code = 1 }
    [IO.File]::WriteAllText($Output, (ConvertTo-Json -InputObject $report -Depth 30) + "`n", [Text.UTF8Encoding]::new($false))
    exit $code
}
