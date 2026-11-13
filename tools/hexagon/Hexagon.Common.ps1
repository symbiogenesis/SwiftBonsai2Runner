#Requires -Version 7.6
# Data-only helpers shared by the independent Hexagon controls.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$script:HexagonRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
function Assert-Hexagon([bool]$Condition, [string]$Message) { if (-not $Condition) { throw $Message } }
function Read-HexagonBytes([string]$Path, [long]$Limit = 1048576) {
    $stream = [IO.File]::OpenRead($Path)
    try {
        $buffer = [byte[]]::new($Limit + 1)
        $length = 0
        while ($length -lt $buffer.Length) {
            $read = $stream.Read($buffer, $length, $buffer.Length - $length)
            if ($read -eq 0) { break }; $length += $read
        }
        Assert-Hexagon ($length -le $Limit) 'input exceeds bounded audit size'
        if ($length -eq 0) { return ,([byte[]]::new(0)) }; return ,$buffer[0..($length - 1)]
    } finally { $stream.Dispose() }
}
function Get-HexagonHash([byte[]]$Bytes) { [Convert]::ToHexStringLower([Security.Cryptography.SHA256]::HashData($Bytes)) }
function Get-HexagonIdentity([string]$Path) {
    $file = Get-Item -LiteralPath $Path
    Assert-Hexagon (-not $file.PSIsContainer -and -not ($file.Attributes -band [IO.FileAttributes]::ReparsePoint)) 'identity requires an ordinary file'
    return @{name=$file.Name; bytes=$file.Length; sha256=(Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()}
}
function ConvertFrom-HexagonJson([byte[]]$Bytes, [int]$Depth = 64) {
    $options = [Text.Json.JsonDocumentOptions]::new(); $options.MaxDepth = $Depth
    $document = [Text.Json.JsonDocument]::Parse([ReadOnlyMemory[byte]]::new($Bytes), $options)
    try {
        function Test-UniqueJson($Element) {
            if ($Element.ValueKind -eq [Text.Json.JsonValueKind]::Object) {
                $names = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
                foreach ($property in $Element.EnumerateObject()) {
                    Assert-Hexagon ($names.Add($property.Name)) ('duplicate JSON field: ' + $property.Name)
                    Test-UniqueJson $property.Value
                }
            } elseif ($Element.ValueKind -eq [Text.Json.JsonValueKind]::Array) {
                foreach ($child in $Element.EnumerateArray()) { Test-UniqueJson $child }
            }
        }
        Test-UniqueJson $document.RootElement
        return [Text.Encoding]::UTF8.GetString($Bytes) | ConvertFrom-Json -AsHashtable -Depth $Depth
    } finally { $document.Dispose() }
}
function Read-HexagonJson([string]$Path, [long]$Limit = 1048576) { ConvertFrom-HexagonJson (Read-HexagonBytes $Path $Limit) }
function Read-HexagonSource([string]$Relative) {
    $bytes = Read-HexagonBytes (Join-Path $script:HexagonRoot $Relative)
    return ,[Text.Encoding]::UTF8.GetBytes([Text.Encoding]::UTF8.GetString($bytes).Replace("`r`n", "`n"))
}
function Test-HexagonEqual($Left, $Right) {
    if ($null -eq $Left -or $null -eq $Right) { return $null -eq $Left -and $null -eq $Right }
    if ($Left -is [Collections.IDictionary] -and $Right -is [Collections.IDictionary]) {
        if ($Left.Count -ne $Right.Count) { return $false }
        foreach ($key in $Left.Keys) { if (-not $Right.Contains($key) -or -not (Test-HexagonEqual $Left[$key] $Right[$key])) { return $false } }
        return $true
    }
    if ($Left -is [Array] -and $Right -is [Array]) {
        if ($Left.Length -ne $Right.Length) { return $false }
        for ($i=0; $i -lt $Left.Length; $i++) { if (-not (Test-HexagonEqual $Left[$i] $Right[$i])) { return $false } }
        return $true
    }
    if (($Left -is [bool]) -ne ($Right -is [bool]) -or ($Left -is [string]) -ne ($Right -is [string])) { return $false }
    return $Left -ceq $Right
}
function Write-HexagonJson([string]$Path, $Value, [switch]$CreateNew) {
    $bytes = [Text.Encoding]::UTF8.GetBytes(($Value | ConvertTo-Json -Depth 64) + "`n")
    $mode = if ($CreateNew) { [IO.FileMode]::CreateNew } else { [IO.FileMode]::Create }
    $stream = [IO.File]::Open($Path, $mode, [IO.FileAccess]::Write)
    try { $stream.Write($bytes); $stream.Flush() } finally { $stream.Dispose() }
}
function Get-HexagonInteger($Record, [string]$Name) {
    $value = $Record[$Name]
    Assert-Hexagon ($null -ne $value -and $value -isnot [bool] -and $value -isnot [string] -and $value -isnot [double] -and $value -isnot [decimal] -and [bigint]$value -ge 0 -and [bigint]$value -le [ulong]::MaxValue) ('invalid integer field: ' + $Name)
    return [bigint]$value
}
function Get-HexagonHalf([ushort]$Bits) {
    Assert-Hexagon (($Bits -band 0x7c00) -ne 0x7c00) 'nonfinite FP16 scale'
    return [float][BitConverter]::UInt16BitsToHalf($Bits)
}
