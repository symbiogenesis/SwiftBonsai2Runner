#Requires -Version 7.6
# Compatibility adapters; Rust owns reference parsing, validation and policy.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$script:ReferenceArithmetic = 'public-mlx-f16-inverse1024-f16-rust-scalar-f32-dot-v2'
$script:ReferenceBlocks = @(0..63 | Where-Object { ($_ + 1) % 4 -ne 0 })
# Schema-1 source inventories remain fixed for independent historical replay.
$script:ReplaySourceFiles = @('config/swift-lora.lock.json','config/swift-lora.direction.json',
    'tools/native-reference/public-authoring-pins.json','tools/native-reference/public_pack_reference.ps1',
    'tools/native-reference/public_pack_formats.ps1','tools/native-reference/audit_public_reference.ps1',
    'tools/native-reference/reference_numerics.rs')
function Get-ReferencePolicyTool {
    if ($env:BONSAI_TOOLS_PATH) { return $env:BONSAI_TOOLS_PATH }
    if ($env:BONSAI_REFERENCE_NUMERIC_TOOL) { return Join-Path ([IO.Path]::GetDirectoryName($env:BONSAI_REFERENCE_NUMERIC_TOOL)) 'bonsai-tools.exe' }
    $selected = Get-Variable ToolPath -ValueOnly -ErrorAction SilentlyContinue
    if ($selected) { return Join-Path ([IO.Path]::GetDirectoryName($selected)) 'bonsai-tools.exe' }
    return (Get-Command bonsai-tools -CommandType Application -ErrorAction Stop).Source
}
function Invoke-ReferencePolicy($Request) {
    $tool = Get-ReferencePolicyTool
    $PSNativeCommandUseErrorActionPreference = $false
    $encoded = ConvertTo-Json -InputObject $Request -Depth 64 -Compress
    $result = $encoded | & $tool native-reference --request - 2>&1
    if ($LASTEXITCODE -ne 0) { throw [IO.InvalidDataException]::new(($result -join "`n")) }
    return ConvertFrom-Json -InputObject ($result -join "`n") -AsHashtable -Depth 64 -NoEnumerate
}
function Assert-Reference([bool]$Condition,[string]$Message) { if (-not $Condition) { throw [IO.InvalidDataException]::new($Message) } }
function Get-ReferenceDigest([byte[]]$Bytes) { (Invoke-ReferencePolicy @{operation='hash-bytes';rawHex=[Convert]::ToHexStringLower($Bytes)}).sha256 }
function Get-ReferenceFileDigest([string]$Path) { (Invoke-ReferencePolicy @{operation='hash-file';path=$Path}).sha256 }
function Read-ReferenceBounded([string]$Path,[long]$Maximum,[long]$Exact=-1,[string]$Sha256='') {
    $request=@{operation='read-bytes';path=$Path;maximum=$Maximum}
    if ($Exact -ge 0) {$request.exact=$Exact}; if ($Sha256) {$request.sha256=$Sha256}
    return ,([Convert]::FromHexString((Invoke-ReferencePolicy $request).rawHex))
}
function ConvertFrom-ReferenceJson([byte[]]$Raw) { Invoke-ReferencePolicy @{operation='decode-json';rawHex=[Convert]::ToHexStringLower($Raw)} }
function Read-ReferenceJson([string]$Path,[long]$Maximum=1048576) { Invoke-ReferencePolicy @{operation='read-json';path=$Path;maximum=$Maximum} }
function Write-ReferenceJson([string]$Path,$Value) { [void](Invoke-ReferencePolicy @{operation='write-json';path=$Path;value=$Value}) }
function Test-ReferenceEqual($Left,$Right) { (Invoke-ReferencePolicy @{operation='equal';left=$Left;right=$Right}).equal }
function Assert-ReferenceKeys($Value,[string[]]$Keys) { [void](Invoke-ReferencePolicy @{operation='keys';value=$Value;keys=$Keys}) }
function Test-ReferenceInteger($Value,[long]$Maximum) { (Invoke-ReferencePolicy @{operation='integer';value=$Value;maximum=$Maximum}).valid }
function Get-ReferenceNumericTool([string]$ToolPath='') {
    if ($ToolPath) { return $ToolPath }; if ($env:BONSAI_REFERENCE_NUMERIC_TOOL) { return $env:BONSAI_REFERENCE_NUMERIC_TOOL }
    return (Get-Command bonsai-reference-numerics -CommandType Application -ErrorAction Stop).Source
}
function Invoke-ReferenceNumeric([string]$ToolPath,[string[]]$Arguments) {
    $result=& $ToolPath @Arguments
    if ($LASTEXITCODE -ne 0) { throw [IO.InvalidDataException]::new('Bounded numerical helper rejected inputs.') }
    ConvertFrom-ReferenceJson ([Text.Encoding]::UTF8.GetBytes(($result -join "`n")))
}
function Read-ReferenceTensorHeader([byte[]]$Raw,[long]$FileBytes) {
    Invoke-ReferencePolicy @{operation='tensor-header';header=[Text.UTF8Encoding]::new($false,$true).GetString($Raw);fileBytes=$FileBytes}
}
function Read-ReferenceSafetensorsHeader([IO.Stream]$Stream) {
    $temporary=$null
    try {
        if ($Stream -is [IO.FileStream]) { $path=$Stream.Name }
        else { $temporary=Join-Path ([IO.Path]::GetTempPath()) ([guid]::NewGuid().ToString('N')+'.safetensors'); $Stream.Position=0; $output=[IO.File]::Create($temporary);try{$Stream.CopyTo($output)}finally{$output.Dispose()};$path=$temporary }
        $result=Invoke-ReferencePolicy @{operation='safetensors-header';path=$path}
        return @{descriptors=$result.descriptors;raw=[Text.Encoding]::UTF8.GetBytes($result.raw)}
    }finally{if($temporary){[IO.File]::Delete($temporary)}}
}
function Invoke-ReferenceAdapter([byte[]]$Raw,[string]$Operation='adapter') {
    $temporary=Join-Path ([IO.Path]::GetTempPath()) ([guid]::NewGuid().ToString('N')+'.gguf')
    try { [IO.File]::WriteAllBytes($temporary,$Raw);$result=Invoke-ReferencePolicy @{operation=$Operation;path=$temporary};$factors=[ordered]@{};foreach($key in $result.Keys){$factors[$key]=[Convert]::FromHexString($result[$key])};return $factors }
    finally { [IO.File]::Delete($temporary) }
}
function Read-ReferenceAdapter([byte[]]$Raw) { Invoke-ReferenceAdapter $Raw }
