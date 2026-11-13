#Requires -Version 7.6
# Bounded data readers shared by the producer. Replay uses independent metrics.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$script:ReferenceArithmetic = 'public-mlx-f16-inverse1024-f16-rust-scalar-f32-dot-v2'
$script:ReferenceBlocks = @(0..63 | Where-Object { ($_ + 1) % 4 -ne 0 })
$script:ReplaySourceFiles = @('config/swift-lora.lock.json', 'config/swift-lora.direction.json',
    'tools/native-reference/public-authoring-pins.json', 'tools/native-reference/public_pack_reference.ps1',
    'tools/native-reference/public_pack_formats.ps1', 'tools/native-reference/audit_public_reference.ps1',
    'tools/native-reference/reference_numerics.rs')

function Assert-Reference([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw [IO.InvalidDataException]::new($Message) }
}
function Get-ReferenceDigest([byte[]]$Bytes) { [Convert]::ToHexStringLower([Security.Cryptography.SHA256]::HashData($Bytes)) }
function Get-ReferenceFileDigest([string]$Path) {
    $stream = [IO.File]::OpenRead($Path)
    try { [Convert]::ToHexStringLower([Security.Cryptography.SHA256]::HashData($stream)) } finally { $stream.Dispose() }
}
function Read-ReferenceBounded([string]$Path, [long]$Maximum, [long]$Exact = -1, [string]$Sha256 = '') {
    $stream = [IO.File]::OpenRead($Path)
    try {
        Assert-Reference ($stream.Length -le $Maximum -and ($Exact -lt 0 -or $stream.Length -eq $Exact)) 'retained file byte extent differs'
        Assert-Reference ($stream.Length -le [int]::MaxValue) 'bounded reader extent exceeds its allocation contract'
        $raw = [byte[]]::new([int]$stream.Length)
        $stream.ReadExactly($raw)
        Assert-Reference ($stream.ReadByte() -eq -1) 'retained input grew beyond byte extent'
        if ($Sha256) { Assert-Reference ((Get-ReferenceDigest $raw) -ceq $Sha256) 'retained file digest differs' }
        return ,$raw
    } finally { $stream.Dispose() }
}
function Assert-JsonElement([System.Text.Json.JsonElement]$Element, [int]$Depth = 0) {
    Assert-Reference ($Depth -le 64) 'JSON depth exceeds bound'
    switch ($Element.ValueKind.ToString()) {
        Object {
            $keys = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
            foreach ($property in $Element.EnumerateObject()) {
                Assert-Reference ($keys.Add($property.Name)) 'duplicate JSON key'
                Assert-JsonElement $property.Value ($Depth + 1)
            }
        }
        Array { foreach ($item in $Element.EnumerateArray()) { Assert-JsonElement $item ($Depth + 1) } }
        Number { Assert-Reference ([double]::IsFinite($Element.GetDouble())) 'nonfinite JSON number' }
    }
}
function ConvertFrom-ReferenceJson([byte[]]$Raw) {
    $text = [Text.UTF8Encoding]::new($false, $true).GetString($Raw)
    $document = [Text.Json.JsonDocument]::Parse($text)
    try { Assert-JsonElement $document.RootElement } finally { $document.Dispose() }
    return ConvertFrom-Json -InputObject $text -AsHashtable -Depth 64 -NoEnumerate
}
function Read-ReferenceJson([string]$Path, [long]$Maximum = 1048576) {
    ConvertFrom-ReferenceJson (Read-ReferenceBounded $Path $Maximum)
}
function Write-ReferenceJson([string]$Path, $Value) {
    $temporary = $Path + '.' + [guid]::NewGuid().ToString('N') + '.partial'
    $raw = [Text.UTF8Encoding]::new($false).GetBytes((ConvertTo-Json -InputObject $Value -Depth 64) + "`n")
    $stream = [IO.File]::Open($temporary, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
    try { $stream.Write($raw); $stream.Flush($true) } finally { $stream.Dispose() }
    [IO.File]::Move($temporary, $Path, $true)
}
function Test-ReferenceEqual($Left, $Right) {
    if ($null -eq $Left -or $null -eq $Right) { return $null -eq $Left -and $null -eq $Right }
    if ($Left -is [Collections.IDictionary] -and $Right -is [Collections.IDictionary]) {
        if ($Left.Count -ne $Right.Count) { return $false }
        foreach ($key in $Left.Keys) { if (-not $Right.Contains($key) -or -not (Test-ReferenceEqual $Left[$key] $Right[$key])) { return $false } }
        return $true
    }
    if ($Left -is [array] -and $Right -is [array]) {
        if ($Left.Count -ne $Right.Count) { return $false }
        for ($i=0; $i -lt $Left.Count; $i++) { if (-not (Test-ReferenceEqual $Left[$i] $Right[$i])) { return $false } }
        return $true
    }
    return $Left -ceq $Right
}
function Assert-ReferenceKeys($Value, [string[]]$Keys) {
    Assert-Reference ($Value -is [Collections.IDictionary] -and $Value.Count -eq $Keys.Count) 'reference object schema differs'
    foreach ($key in $Keys) { Assert-Reference ($Value.Contains($key)) 'reference object field missing' }
}
function Test-ReferenceInteger($Value, [long]$Maximum) {
    ($Value -is [long] -or $Value -is [int] -or $Value -is [System.Numerics.BigInteger]) -and $Value -ge 0 -and $Value -le $Maximum
}
function Get-ReferenceNumericTool([string]$ToolPath = '') {
    if (-not $ToolPath) { $ToolPath = $env:BONSAI_REFERENCE_NUMERIC_TOOL }
    if (-not $ToolPath) {
        $command = Get-Command bonsai-reference-numerics -CommandType Application -ErrorAction SilentlyContinue
        if ($command) { $ToolPath = $command.Source }
    }
    Assert-Reference ($ToolPath -and [IO.Path]::IsPathFullyQualified($ToolPath) -and [IO.File]::Exists($ToolPath)) 'prebuilt reference numerical helper is required'
    return $ToolPath
}
function Invoke-ReferenceNumeric([string]$ToolPath, [string[]]$Arguments) {
    $output = & $ToolPath @Arguments
    Assert-Reference ($LASTEXITCODE -eq 0) 'bounded numerical helper rejected inputs'
    ConvertFrom-ReferenceJson ([Text.Encoding]::UTF8.GetBytes(($output -join "`n")))
}
function Read-ReferenceTensorHeader([byte[]]$Raw, [long]$FileBytes) {
    Assert-Reference ($Raw.Length -ge 2 -and $Raw.Length -le 4194304 -and $Raw.Length + 8 -le $FileBytes) 'safetensors header exceeds bound'
    $value = ConvertFrom-ReferenceJson $Raw
    Assert-Reference ($value -is [Collections.IDictionary] -and $value.Count -le 8193) 'invalid safetensors root/inventory'
    if ($value.Contains('__metadata__')) {
        $metadata = $value['__metadata__']
        Assert-Reference ($metadata -is [Collections.IDictionary] -and $metadata.Count -le 64) 'invalid safetensors metadata'
        foreach ($key in $metadata.Keys) { Assert-Reference ($key.Length -le 512 -and $metadata[$key] -is [string] -and $metadata[$key].Length -le 4096) 'invalid safetensors metadata' }
        $value.Remove('__metadata__')
    }
    $widths = @{BOOL=1;U8=1;I8=1;U16=2;I16=2;F16=2;BF16=2;U32=4;I32=4;F32=4;U64=8;I64=8;F64=8}
    $descriptors = [ordered]@{}
    $extents = [Collections.Generic.List[object]]::new()
    foreach ($name in $value.Keys) {
        Assert-Reference ($name.Length -gt 0 -and $name.Length -le 512 -and -not $name.Contains([char]0)) 'invalid safetensors tensor name'
        $record = $value[$name]
        Assert-ReferenceKeys $record @('dtype','shape','data_offsets')
        Assert-Reference ($record.dtype -is [string] -and $widths.Contains($record.dtype) -and $record.shape -is [array] -and $record.shape.Count -le 8) 'unsupported safetensors dtype/rank'
        $elements = [System.Numerics.BigInteger]::One
        foreach ($dimension in $record.shape) { Assert-Reference (Test-ReferenceInteger $dimension 1000000000) 'invalid tensor dimension'; $elements *= $dimension }
        Assert-Reference ($record.data_offsets -is [array] -and $record.data_offsets.Count -eq 2) 'invalid tensor offsets'
        $begin,$end = $record.data_offsets
        Assert-Reference ((Test-ReferenceInteger $begin $FileBytes) -and (Test-ReferenceInteger $end $FileBytes) -and $begin -le $end -and $end-$begin -eq $elements*$widths[$record.dtype] -and $Raw.Length+8+$end -le $FileBytes) 'tensor shape/storage extent differs'
        $descriptors[$name] = @{dtype=$record.dtype;shape=$record.shape;absoluteByteBegin=[long]($Raw.Length+8+$begin);absoluteByteEnd=[long]($Raw.Length+8+$end)}
        if ($begin -ne $end) { $extents.Add(@{begin=$begin;end=$end}) }
    }
    $cursor = 0L
    foreach ($extent in @($extents | Sort-Object begin)) { Assert-Reference ($extent.begin -eq $cursor) 'safetensors payload hole or overlap'; $cursor=$extent.end }
    Assert-Reference ($descriptors.Count -gt 0 -and $Raw.Length+8+$cursor -eq $FileBytes) 'unindexed safetensors payload'
    return $descriptors
}
function Read-ReferenceSafetensorsHeader([IO.Stream]$Stream) {
    $Stream.Position=0
    $reader=[IO.BinaryReader]::new($Stream,[Text.Encoding]::UTF8,$true)
    $length=$reader.ReadUInt64()
    Assert-Reference ($length -ge 2 -and $length -le 4194304 -and $length+8 -le $Stream.Length) 'safetensors header exceeds bound'
    $raw=$reader.ReadBytes([int]$length)
    Assert-Reference ($raw.Length -eq $length) 'truncated safetensors header'
    return @{descriptors=(Read-ReferenceTensorHeader $raw $Stream.Length);raw=$raw}
}
function Read-GgufText([IO.BinaryReader]$Reader) {
    $length=$Reader.ReadUInt64()
    Assert-Reference ($length -le 4096) 'oversized GGUF text'
    $raw=$Reader.ReadBytes([int]$length)
    Assert-Reference ($raw.Length -eq $length) 'truncated GGUF text'
    [Text.UTF8Encoding]::new($false,$true).GetString($raw)
}
function Read-GgufValue([IO.BinaryReader]$Reader, [uint32]$Kind, [int]$Depth=0) {
    Assert-Reference ($Depth -le 2) 'GGUF metadata nesting exceeds bound'
    switch ($Kind) {
        0 { return $Reader.ReadByte() } 1 { return $Reader.ReadSByte() }
        2 { return $Reader.ReadUInt16() } 3 { return $Reader.ReadInt16() }
        4 { return $Reader.ReadUInt32() } 5 { return $Reader.ReadInt32() }
        6 { return $Reader.ReadSingle() } 10 { return $Reader.ReadUInt64() }
        11 { return $Reader.ReadInt64() } 12 { return $Reader.ReadDouble() }
        7 { $value=$Reader.ReadByte(); Assert-Reference ($value -in 0,1) 'invalid GGUF bool'; return [bool]$value }
        8 { return Read-GgufText $Reader }
        9 {
            $element=$Reader.ReadUInt32(); $count=$Reader.ReadUInt64()
            Assert-Reference ($count -le 65536) 'GGUF array count exceeds bound'
            $values=[Collections.Generic.List[object]]::new()
            for ($i=0L;$i -lt $count;$i++) { $values.Add((Read-GgufValue $Reader $element ($Depth+1))) }
            return ,$values.ToArray()
        }
        default { throw [IO.InvalidDataException]::new('unsupported GGUF metadata type') }
    }
}
function Read-ReferenceAdapter([byte[]]$Raw) {
    $stream=[IO.MemoryStream]::new($Raw,$false); $reader=[IO.BinaryReader]::new($stream)
    try {
        Assert-Reference ([Text.Encoding]::ASCII.GetString($reader.ReadBytes(4)) -ceq 'GGUF' -and $reader.ReadUInt32() -eq 3) 'adapter is not GGUF v3'
        $count=$reader.ReadUInt64(); $metadataCount=$reader.ReadUInt64()
        Assert-Reference ($count -eq 258 -and $metadataCount -le 128) 'adapter inventory differs'
        $metadata=[ordered]@{}
        for($i=0;$i -lt $metadataCount;$i++) {
            $key=Read-GgufText $reader
            Assert-Reference (-not $metadata.Contains($key)) 'duplicate GGUF metadata'
            $metadata[$key]=Read-GgufValue $reader ($reader.ReadUInt32())
        }
        Assert-Reference ((-not $metadata.Contains('general.alignment') -or $metadata['general.alignment'] -eq 32) -and $metadata['general.architecture'] -ceq 'qwen35' -and $metadata['general.type'] -ceq 'adapter' -and $metadata['adapter.type'] -ceq 'lora' -and $metadata['adapter.lora_alpha'] -eq 1.0) 'adapter metadata differs'
        $descriptors=[ordered]@{}
        for($i=0;$i -lt $count;$i++) {
            $name=Read-GgufText $reader
            Assert-Reference ($name.Length -le 512 -and -not $descriptors.Contains($name) -and $reader.ReadUInt32() -eq 2) 'adapter name/rank differs'
            $shape=@($reader.ReadUInt64(),$reader.ReadUInt64())
            Assert-Reference ($shape[0] -gt 0 -and $shape[0] -le 248320 -and $shape[1] -gt 0 -and $shape[1] -le 248320 -and $reader.ReadUInt32() -eq 0) 'adapter tensor shape/type differs'
            $offset=$reader.ReadUInt64()
            Assert-Reference ($offset % 32 -eq 0 -and $offset -le $Raw.Length) 'misaligned GGUF factor'
            $descriptors[$name]=@{shape=$shape;offset=$offset}
        }
        $expected=[ordered]@{'token_embd.weight'=@(1,248320)}
        foreach($block in 0..63) {
            $expected["blk.$block.ffn_down.weight"]=@(17408,1)
            $family=if(($block+1)%4 -eq 0){'attn_output'}else{'ssm_out'}
            $expected["blk.$block.$family.weight"]=@(6144,1)
        }
        $base=([long][Math]::Ceiling($stream.Position/32.0))*32
        $factors=[ordered]@{}; $extents=[Collections.Generic.List[object]]::new()
        foreach($site in $expected.Keys) {
            foreach($suffix in '.lora_a','.lora_b') {
                $name=$site+$suffix
                Assert-Reference ($descriptors.Contains($name)) 'adapter writer inventory differs'
                $descriptor=$descriptors[$name]; $shape=if($suffix -ceq '.lora_a'){$expected[$site]}else{@(1,5120)}
                Assert-Reference (Test-ReferenceEqual $descriptor.shape $shape) 'adapter writer shape differs'
                $begin=$base+$descriptor.offset; $end=$begin+$shape[0]*$shape[1]*4L
                Assert-Reference ($end -le $Raw.Length) 'GGUF factor extends beyond file'
                $factor=[byte[]]::new([int]($end-$begin)); [Array]::Copy($Raw,[long]$begin,$factor,0L,[long]$factor.Length)
                $factors[$name]=$factor; $extents.Add(@{begin=$begin;end=$end})
            }
        }
        Assert-Reference ($descriptors.Count -eq $factors.Count) 'adapter writer inventory differs'
        $cursor=$base
        foreach($extent in @($extents|Sort-Object begin)) { Assert-Reference ($extent.begin -ge $cursor) 'GGUF factors overlap';$cursor=$extent.end }
        return $factors
    } finally { $reader.Dispose();$stream.Dispose() }
}
