#Requires -Version 7.6
. "$PSScriptRoot/Hexagon.Control.ps1"
function Get-GhcRuntime([string]$Ghc,$Control,[string]$Output) {
    $libdir=(Invoke-HexagonControl $Control ghc-libdir @($Ghc,'--print-libdir') $Output).Trim();$root=[IO.Path]::GetFullPath((Join-Path (Split-Path $Ghc) '..'))
    $result=@{reportedLibdir=$libdir;selectedToolRoot=$root;scope='selected-installation-compiler/runtime-executable-candidates; not-complete-tool-or-OS-closure';scanTruncated=$false;entriesObserved=0;candidates=[Collections.Generic.List[object]]::new()}
    if (-not [IO.Path]::GetRelativePath($root,$libdir).StartsWith('..')) {
        $pending=[Collections.Generic.Queue[string]]::new();$pending.Enqueue($root);$seen=[Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
        while ($pending.Count) {
            $directory=$pending.Dequeue();foreach ($entry in Get-ChildItem -LiteralPath $directory -Force) {
                $result.entriesObserved++;if ($result.entriesObserved -gt 50000) {$result.scanTruncated=$true;break}
                if ($entry.Attributes -band [IO.FileAttributes]::ReparsePoint) {continue}
                if ($entry.PSIsContainer) {$pending.Enqueue($entry.FullName);continue}
                if ($entry.Name -cne 'ghc' -and $entry.Name -cne [IO.Path]::GetFileName($Ghc) -and -not $entry.Name.StartsWith('ghc-iserv')) {continue}
                if (-not $seen.Add($entry.FullName)) {continue}
                $identity=Get-HexagonIdentity $entry.FullName;$stream=[IO.File]::OpenRead($entry.FullName);try {$bytes=[byte[]]::new(4);$read=$stream.Read($bytes);$identity.elfExecutable=($read -eq 4 -and [Convert]::ToHexString($bytes) -ceq '7F454C46')} finally {$stream.Dispose()}
                $identity.path=$entry.FullName;$identity.discoveredAs=$entry.FullName;$result.candidates.Add($identity)
            };if ($result.scanTruncated) {break}
        }
    } else {$result.scope='reported-libdir-outside-selected-wrapper-installation; runtime-closure-pending'}
    $result.candidates=@($result.candidates);return $result
}
function Invoke-PublicWireCapture($Pin,$Report,$Control,[string]$Output,[string]$Generated,[string]$Review) {
    $selection=$Pin.wireCapture
    Assert-Hexagon ($selection.enabled -is [bool] -and $selection.enabled -and $selection.sdkLinked -is [bool] -and -not $selection.sdkLinked -and $selection.deviceInvocationAuthorized -is [bool] -and -not $selection.deviceInvocationAuthorized -and $selection.profileEligible -is [bool] -and -not $selection.profileEligible) 'the fixed mock wire control must be explicitly selected'
    $wire=@{kind='public-generated-C-mock-wire-control';passed=$false;controller=(Get-HexagonIdentity $PSCommandPath);headers=[Collections.Generic.List[object]]::new();compiler=@{};mockExecutionAttempted=$false;mockExecuted=$false;mockCompleted=$false;sdkLinked=$false;driverLoaded=$false;deviceCodeLoaded=$false;sdkEquivalenceQualified=$false;WindowsArm64AbiQualified=$false;scope='public-generated-stub-and-public-headers-on-hosted-Linux-LP64-only'}
    $Report.wireCapture=$wire;$Report.stage='mock-wire-source-intake';$inputs=Join-Path $Review 'wire-public-inputs';$null=New-Item -ItemType Directory -Path $inputs
    foreach ($entry in $selection.headers) {$wire.headers.Add((Receive-HexagonPublic $entry $inputs))}
    foreach ($entry in @($selection.harness,$selection.expected)) {$source=Join-Path $PSScriptRoot $entry.name;$null=Assert-HexagonIdentity $source $entry;Copy-Item -LiteralPath $source -Destination (Join-Path $inputs $entry.name)}
    $wire.headers=@($wire.headers);$wire.harness=Get-HexagonIdentity (Join-Path $inputs $selection.harness.name);$wire.independentExpected=Get-HexagonIdentity (Join-Path $inputs $selection.expected.name);$expected=Read-HexagonJson (Join-Path $inputs $selection.expected.name)
    foreach ($entry in $selection.reviewedGeneratedFiles) {Assert-Hexagon (Test-HexagonEqual (Get-HexagonIdentity (Join-Path $Generated $entry.name)) $entry) 'public generated C differs from the reviewed first-generation bytes'}
    $wire.generatedInputs=@('htp_iface.h','htp_iface_stub.c' | ForEach-Object {Get-HexagonIdentity (Join-Path $Generated $_)})
    $Report.stage='mock-wire-compiler-identity';$compiler=(Resolve-HexagonExecutable '/usr/bin/gcc');$original=Get-HexagonIdentity $compiler
    $version=(Invoke-HexagonControl $Control wire-gcc-version @($compiler,'-dumpfullversion','-dumpversion') $Output).Trim();$target=(Invoke-HexagonControl $Control wire-gcc-target @($compiler,'-dumpmachine') $Output).Trim()
    $null=Invoke-HexagonControl $Control wire-gcc-banner @($compiler,'--version') $Output;$null=Invoke-HexagonControl $Control wire-gcc-specs @($compiler,'-dumpspecs') $Output
    Assert-Hexagon ($version -cmatch '^[0-9]+\.[0-9]+(?:\.[0-9]+)?$' -and $target -ceq 'x86_64-linux-gnu') 'mock wire control requires the fixed native Linux x86_64 C target'
    $components=@(foreach ($name in @('cc1','as','ld')) {$candidate=(Invoke-HexagonControl $Control ('wire-gcc-'+$name) @($compiler,('-print-prog-name='+$name)) $Output).Trim();if (-not [IO.Path]::IsPathRooted($candidate)) {$candidate=(Get-Command $candidate -CommandType Application).Source};$path=(Resolve-HexagonExecutable $candidate);$item=Get-HexagonIdentity $path;$item.path=$path;$item.role=$name;$item})
    $wire.compiler=$original.Clone();$wire.compiler.path=$compiler;$wire.compiler.version=$version;$wire.compiler.target=$target;$wire.compiler.components=$components;$wire.compiler.identityPinScope='observed-and-rechecked-within-this-hosted-run';$wire.compiler.closure='driver/cc1/as/ld identities; not-complete-OS-closure'
    $flags=@('-std=gnu11','-O0','-fno-lto','-fno-strict-aliasing');Assert-Hexagon (Test-HexagonEqual $selection.compilerFlags @($flags+@('-include','{checked-public-AEEStdDef.h}'))) 'the fixed mock-only compiler flags changed'
    $flags+=@('-include',(Join-Path $inputs 'AEEStdDef.h'));$executable=Join-Path $Output 'public-qaic-wire-mock';$linkMap=Join-Path $Review 'wire-link.map'
    $arguments=@($compiler)+$flags+@(('-I'+$inputs),('-I'+$Generated),(Join-Path $Generated 'htp_iface_stub.c'),(Join-Path $inputs $selection.harness.name),('-Wl,-Map='+$linkMap),'-o',$executable)
    $wire.compiler.arguments=$arguments;$Report.stage='mock-wire-compile';$null=Invoke-HexagonControl $Control wire-compile $arguments $Output 30
    Assert-Hexagon (Test-HexagonEqual (Get-HexagonIdentity $compiler) $original) 'GCC driver changed during bounded compilation'
    foreach ($component in $components) {$null=Assert-HexagonIdentity $component.path $component}
    Assert-Hexagon ((Get-Item -LiteralPath $linkMap).Length -le 1048576) 'mock link map omitted or exceeded its bound'
    $wire.linkMap=Get-HexagonIdentity $linkMap;$wire.executable=Get-HexagonIdentity $executable;$Report.stage='mock-wire-execution';$wire.mockExecutionAttempted=$true;$wire.mockExecuted=$null
    try {$text=Invoke-HexagonControl $Control wire-capture @($executable) $Output 5} finally {if ($Control.commands[-1].name -ceq 'wire-capture' -and $Control.commands[-1].completed) {$wire.mockExecuted=$true;$wire.mockCompleted=$true}}
    Assert-Hexagon (Test-HexagonEqual (Get-HexagonIdentity $executable) $wire.executable) 'mock executable changed during bounded capture';Assert-Hexagon ([Text.Encoding]::UTF8.GetByteCount($text) -le 16384) 'mock capture receipt exceeded fixed output bound';$actual=ConvertFrom-HexagonJson ([Text.Encoding]::UTF8.GetBytes($text))
    Assert-Hexagon (Test-HexagonEqual @($actual.Keys | Sort-Object) @('schemaVersion','kind','testVectorId','handleHex','start','stop','callCount','unexpectedCalls','returnCodes','layout' | Sort-Object)) 'mock capture emitted unsupported receipt shape'
    foreach ($name in @('schemaVersion','testVectorId','handleHex','start','stop')) {Assert-Hexagon (Test-HexagonEqual $actual[$name] $expected[$name]) ('public generated wire differs from independent vector: '+$name)}
    $layout=@{charBits=8;pointerBytes=8;sizeTBytes=8;uint32Bytes=4;uint64Bytes=8;remoteHandle64Bytes=8;remoteBufferBytes=16;remoteBufferAlignment=8;remoteBufferPointerOffset=0;remoteBufferLengthOffset=8;remoteArgBytes=16;remoteArgAlignment=8;littleEndian=$true}
    $checks=@{kind='public-qaic-mock-wire-capture';callCount=2;unexpectedCalls=0;returnCodes=@{start=0;stop=0};layout=$layout};foreach ($name in $checks.Keys) {Assert-Hexagon (Test-HexagonEqual $actual[$name] $checks[$name]) 'mock capture failed fixed call/return/native-layout contract'}
    $wire.captureReceipt=Get-HexagonIdentity (Join-Path $Review 'wire-capture.log');$wire.observed=$actual;$wire.independentVectorMatched=$true;$wire.compilerIdentityRechecked=$true;$wire.passed=$true;$Report.stage='independent-public-mock-wire-complete';$Report.outcome='captured-independent-public-wire-only'
}
