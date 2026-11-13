#Requires -Version 7.6
param([Parameter(Mandatory)][string]$Output,[Parameter(Mandatory)][string]$Rustc)
. "$PSScriptRoot/Hexagon.Control.ps1"
$script:RetainedQaic=Join-Path $script:HexagonRoot 'docs/evidence/hexagon-public-qaic-41171859264'
function Assert-Arm64Pe([string]$Path) {
    $stream=[IO.File]::OpenRead($Path)
    try {$header=[byte[]]::new(64);Assert-Hexagon ($stream.Read($header) -eq 64 -and $header[0] -eq 77 -and $header[1] -eq 90) ('expected native ARM64 PE: '+[IO.Path]::GetFileName($Path));$offset=[BitConverter]::ToUInt32($header,60);Assert-Hexagon ([long]$offset+6 -le $stream.Length) 'PE header outside executable';$stream.Position=$offset;$signature=[byte[]]::new(6);Assert-Hexagon ($stream.Read($signature) -eq 6 -and [Convert]::ToHexString($signature) -ceq '5045000064AA') ('expected native ARM64 PE: '+[IO.Path]::GetFileName($Path))} finally {$stream.Dispose()}
}
function Get-NativeCompiler($Control,[string]$Output,$Report) {
    $installer=Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
    $installation=(Invoke-HexagonControl $Control vswhere @($installer,'-latest','-products','*','-requires','Microsoft.VisualStudio.Component.VC.Tools.ARM64','-property','installationPath') $Output).Trim()
    Assert-Hexagon (Test-Path -LiteralPath $installation -PathType Container) 'native compiler installation absent'
    $version=[Text.Encoding]::UTF8.GetString((Read-HexagonBytes (Join-Path $installation 'VC/Auxiliary/Build/Microsoft.VCToolsVersion.default.txt') 1024)).Trim();Assert-Hexagon ($version -cmatch '^\d+(?:\.\d+){2,3}$') 'invalid MSVC tools version'
    $tools=Join-Path $installation ('VC/Tools/MSVC/'+$version);$binary=Join-Path $tools 'bin/Hostarm64/arm64';$compiler=Join-Path $binary 'cl.exe';$linker=Join-Path $binary 'link.exe';Assert-Arm64Pe $compiler;Assert-Arm64Pe $linker
    $sdk=Join-Path ${env:ProgramFiles(x86)} 'Windows Kits/10'
    $versions=@(Get-ChildItem -LiteralPath (Join-Path $sdk 'Include') -Directory | Where-Object {$_.Name -cmatch '^\d+(?:\.\d+){3}$' -and (Test-Path -LiteralPath (Join-Path $sdk ('Lib/'+$_.Name+'/ucrt/arm64/ucrt.lib'))) -and (Test-Path -LiteralPath (Join-Path $sdk ('Lib/'+$_.Name+'/um/arm64/kernel32.lib')))} | Sort-Object {[version]$_.Name} -Descending)
    Assert-Hexagon ($versions.Count -gt 0) 'native ARM64 Windows SDK libraries required';$sdkVersion=$versions[0].Name
    $Control.environment.INCLUDE=@((Join-Path $tools 'include'),(Join-Path $sdk ('Include/'+$sdkVersion+'/ucrt')),(Join-Path $sdk ('Include/'+$sdkVersion+'/shared')),(Join-Path $sdk ('Include/'+$sdkVersion+'/um'))) -join ';'
    $Control.environment.LIB=@((Join-Path $tools 'lib/arm64'),(Join-Path $sdk ('Lib/'+$sdkVersion+'/ucrt/arm64')),(Join-Path $sdk ('Lib/'+$sdkVersion+'/um/arm64'))) -join ';';$Control.environment.PATH=$binary+';'+$Control.environment.PATH
    $selected=@($compiler,$linker,(Join-Path $binary 'c1.dll'),(Join-Path $binary 'c2.dll') | ForEach-Object {$item=Get-HexagonIdentity $_;$item.path=$_;$item})
    $Report.compiler=@{selectedFiles=$selected;toolsVersion=$version;windowsSdkVersion=$sdkVersion;target='aarch64-pc-windows-msvc';scope='selected native compiler/linker components; not-complete-OS-closure'}
    return @{path=$compiler;selected=$selected}
}
function Receive-WindowsAbiInputs($Control,[string]$Output,$Pin,$Report) {
    $inputs=Join-Path $Output 'inputs';$retainedInputs=Join-Path $Control.review 'inputs';$null=New-Item -ItemType Directory -Path $inputs,$retainedInputs;$entries=[Collections.Generic.List[object]]::new()
    foreach ($entry in $Pin.publicInputs) {$entries.Add((Receive-HexagonPublic $entry $inputs 1048576));Copy-Item -LiteralPath (Join-Path $inputs $entry.name) -Destination (Join-Path $retainedInputs ($entry.name+'.txt'))}
    $public=Read-HexagonJson (Join-Path $PSScriptRoot 'public-qaic.lock.json')
    foreach ($entry in $public.wireCapture.headers) {$source=Join-Path $script:RetainedQaic ('wire-public-inputs/'+$entry.name+'.txt');$item=Assert-HexagonIdentity $source $entry;Copy-Item -LiteralPath $source -Destination (Join-Path $inputs $entry.name);Copy-Item -LiteralPath $source -Destination (Join-Path $retainedInputs ($entry.name+'.txt'));$item.origin=[IO.Path]::GetRelativePath($script:HexagonRoot,$source).Replace('\','/');$item.url=$entry.url;$entries.Add($item)}
    foreach ($entry in $public.wireCapture.reviewedGeneratedFiles) {if ($entry.name -ceq 'htp_iface_skel.c') {continue};$source=Join-Path $script:RetainedQaic ('generated-public-source/'+$entry.name+'.txt');$item=Assert-HexagonIdentity $source $entry;Copy-Item -LiteralPath $source -Destination (Join-Path $inputs $entry.name);Copy-Item -LiteralPath $source -Destination (Join-Path $retainedInputs ($entry.name+'.txt'));$item.origin=[IO.Path]::GetRelativePath($script:HexagonRoot,$source).Replace('\','/');$item.generatedBy=$public.qaicCommit;$entries.Add($item)}
    $notices=Join-Path $Control.review 'notices';$null=New-Item -ItemType Directory -Path $notices
    foreach ($file in Get-ChildItem -LiteralPath (Join-Path $script:RetainedQaic 'inputs-and-notices') -File) {Assert-Hexagon (-not ($file.Attributes -band [IO.FileAttributes]::ReparsePoint)) 'retained notice cannot be linked';Copy-Item -LiteralPath $file.FullName -Destination (Join-Path $notices $file.Name)}
    Copy-Item -LiteralPath (Join-Path $script:RetainedQaic 'NOTICES.txt') -Destination (Join-Path $notices 'generation-NOTICES.txt')
    foreach ($name in @('windows_queue_abi.c','windows_queue_abi.rs','public_qaic_wire.c','public_idl_control.rs','public-qaic-wire.expected.json')) {Copy-Item -LiteralPath (Join-Path $PSScriptRoot $name) -Destination (Join-Path $retainedInputs ($name+'.txt'))}
    $Report.inputs=@($entries);$Report.localSources=@(Get-ChildItem -LiteralPath $retainedInputs -File | ForEach-Object {Get-HexagonIdentity $_.FullName});return $inputs
}
function Build-WindowsAbiC($Control,[string]$Output,[string]$Compiler,[string]$Inputs,[string]$Name,[string[]]$Sources,[switch]$Branch) {
    $executable=Join-Path $Output ($Name+'.exe');$arguments=@($Compiler,'/nologo','/std:c11','/TC','/Od','/GS','/guard:cf',('/I'+$Inputs),('/Fo'+$Output+'\'),('/Fe'+$executable))
    if ($Branch) {$arguments+=@('/W1','/wd4068','/U_WIN32','/D_CRT_SECURE_NO_WARNINGS','/D_ATTRIBUTE_UNUSED=','/D_ATTRIBUTE_VISIBILITY=',('/FI'+(Join-Path $Inputs 'AEEStdDef.h')))} else {$arguments+=@('/W4','/WX')}
    $null=Invoke-HexagonControl $Control ($Name+'-compile') ($arguments+$Sources+@('/link','/machine:arm64',('/MAP:'+(Join-Path $Control.review ($Name+'.map'))))) $Output
    Assert-Arm64Pe $executable;return $executable
}
function Compare-WindowsQueueLayout($Control,[string]$Output,[string]$Compiler,[string]$Inputs,[string]$Rustc,$Report) {
    $executable=Build-WindowsAbiC $Control $Output $Compiler $Inputs queue-abi @((Join-Path $PSScriptRoot 'windows_queue_abi.c'))
    $observed=ConvertFrom-HexagonJson ([Text.Encoding]::UTF8.GetBytes((Invoke-HexagonControl $Control queue-abi-capture @($executable) $Output 5)))
    $expected=@{schemaVersion=1;kind='windows-arm64-public-queue-abi-control';queueBuffer=@{bytes=24;alignment=8;fdOffset=0;sizeOffset=4;offsetOffset=8;flagsOffset=12;addressOffset=16};htp=@{tensorBytes=56;bufferBytes=24;operationBytes=232;requestBytes=24;responseBytes=88;profileBytes=48};queueWrites=@{timed=@{arguments=7;timeoutMicroseconds=100000;calls=1};nonblocking=@{arguments=6;calls=1}};infiniteTimeout=4294967295;mockCalls=6;unexpectedCalls=0;returnCode=0;bufferHex='98badcfe403020100403020194000000efcdab8967452381';driverLoaded=$false;deviceExecuted=$false;profileEligible=$false}
    Assert-Hexagon (Test-HexagonEqual $observed $expected) 'public native queue/packet ABI differs from independent expectation'
    $rustExecutable=Join-Path $Output 'queue-abi-rust.exe';$null=Invoke-HexagonControl $Control queue-abi-rust-build @($Rustc,'--edition=2024','--target','aarch64-pc-windows-msvc','-D','warnings','-C',('linker='+(Join-Path (Split-Path $Compiler) 'link.exe')),(Join-Path $PSScriptRoot 'windows_queue_abi.rs'),'-o',$rustExecutable) $Output
    Assert-Arm64Pe $rustExecutable;$rust=ConvertFrom-HexagonJson ([Text.Encoding]::UTF8.GetBytes((Invoke-HexagonControl $Control queue-abi-rust-capture @($rustExecutable) $Output 5)))
    $keys=@('schemaVersion','queueBuffer','htp','infiniteTimeout','bufferHex','driverLoaded','deviceExecuted','profileEligible');Assert-Hexagon ((Test-HexagonEqual @($rust.Keys | Sort-Object) @($keys+@('kind') | Sort-Object)) -and $rust.kind -ceq 'windows-arm64-Rust-queue-abi-control') 'native Rust queue receipt shape differs'
    foreach ($key in $keys) {Assert-Hexagon (Test-HexagonEqual $rust[$key] $observed[$key]) 'native Rust queue layout differs from public C headers'}
    $Report.queueAbi=@{passed=$true;nativeWindowsConditionals=$true;observed=$observed;rustObserved=$rust;executable=(Get-HexagonIdentity $executable);rustExecutable=(Get-HexagonIdentity $rustExecutable)}
    $negatives=[ordered]@{
        'narrow-queue-pointer'="#include `"dspqueue.h`"`ntypedef int (*bad_close)(uint32_t);`nbad_close narrowed = dspqueue_close;`n"
        'timed-write-without-timeout'="#include `"dspqueue.h`"`ntypedef AEEResult (*bad_write)(dspqueue_t, uint32_t, uint32_t, struct dspqueue_buffer *, uint32_t, const uint8_t *);`nbad_write missing_timeout = dspqueue_write;`n"
        'noblock-write-with-timeout'="#include `"dspqueue.h`"`ntypedef AEEResult (*bad_noblock)(dspqueue_t, uint32_t, uint32_t, struct dspqueue_buffer *, uint32_t, const uint8_t *, uint32_t);`nbad_noblock extra_timeout = dspqueue_write_noblock;`n"
        'wrong-buffer-layout'="#include `"dspqueue.h`"`n_Static_assert(sizeof(struct dspqueue_buffer) == 20, `"deliberate wrong buffer ABI`");`n"
        'wrong-operation-layout'="#include <stdint.h>`n#include `"htp-ops.h`"`n_Static_assert(sizeof(struct htp_op_desc) == 224, `"deliberate wrong operation ABI`");`n"
    }
    foreach ($name in $negatives.Keys) {
        $path=Join-Path $Output ($name+'.c');[IO.File]::WriteAllText($path,$negatives[$name],[Text.UTF8Encoding]::new($false));Copy-Item -LiteralPath $path -Destination (Join-Path $Control.review ($name+'.c.txt'))
        $null=Invoke-HexagonControl $Control $name @($Compiler,'/nologo','/std:c11','/TC','/W4','/WX','/c',('/I'+$Inputs),('/Fo'+(Join-Path $Output ($name+'.obj'))),$path) $Output -ExpectedFailure
        $diagnostics='';foreach ($suffix in @('.log','.stderr.log')) {$log=Join-Path $Control.review ($name+$suffix);if (Test-Path -LiteralPath $log) {$diagnostics+=[IO.File]::ReadAllText($log)}}
        if ($name -in @('narrow-queue-pointer','timed-write-without-timeout','noblock-write-with-timeout')) {Assert-Hexagon ($diagnostics -match '\bC(?:4113|4133|4047)\b') ('incompatible queue signature failed for unrelated reason: '+$name)} else {$kind=if ($name -ceq 'wrong-buffer-layout') {'buffer'} else {'operation'};Assert-Hexagon ($diagnostics.Contains('deliberate wrong '+$kind+' ABI')) 'wrong layout control failed for unrelated reason'}
    }
    $Report.queueAbi.negativeControlsRejected=@($negatives.Keys)
}
function Compare-WindowsStartStopWire($Control,[string]$Output,[string]$Compiler,[string]$Inputs,[string]$Rustc,$Report) {
    $executable=Build-WindowsAbiC $Control $Output $Compiler $Inputs start-stop-wire @((Join-Path $Inputs 'htp_iface_stub.c'),(Join-Path $PSScriptRoot 'public_qaic_wire.c')) -Branch
    $observed=ConvertFrom-HexagonJson ([Text.Encoding]::UTF8.GetBytes((Invoke-HexagonControl $Control start-stop-wire-capture @($executable) $Output 5)));$expected=Read-HexagonJson (Join-Path $PSScriptRoot 'public-qaic-wire.expected.json');$shared=@('schemaVersion','testVectorId','handleHex','start','stop')
    foreach ($name in $shared) {Assert-Hexagon (Test-HexagonEqual $observed[$name] $expected[$name]) 'native generated C start/stop differs from independent golden frame'}
    $layout=@{charBits=8;pointerBytes=8;sizeTBytes=8;uint32Bytes=4;uint64Bytes=8;remoteHandle64Bytes=8;remoteBufferBytes=16;remoteBufferAlignment=8;remoteBufferPointerOffset=0;remoteBufferLengthOffset=8;remoteArgBytes=16;remoteArgAlignment=8;littleEndian=$true}
    Assert-Hexagon ((Test-HexagonEqual $observed.layout $layout) -and $observed.callCount -eq 2 -and $observed.unexpectedCalls -eq 0 -and (Test-HexagonEqual $observed.returnCodes @{start=0;stop=0})) 'native generated C wire failed call/layout checks'
    $rustExecutable=Join-Path $Output 'start-stop-rust.exe';$null=Invoke-HexagonControl $Control start-stop-rust-build @($Rustc,'--edition=2024','--target','aarch64-pc-windows-msvc','-D','warnings','-C',('linker='+(Join-Path (Split-Path $Compiler) 'link.exe')),(Join-Path $PSScriptRoot 'public_idl_control.rs'),'-o',$rustExecutable) $Output
    Assert-Arm64Pe $rustExecutable;$rust=ConvertFrom-HexagonJson ([Text.Encoding]::UTF8.GetBytes((Invoke-HexagonControl $Control start-stop-rust-capture @($rustExecutable) $Output 5)))
    foreach ($name in $shared) {Assert-Hexagon (Test-HexagonEqual $rust[$name] $observed[$name]) 'native independent Rust encoder differs from unchanged generated C'}
    $Report.startStopWire=@{passed=$true;generatedFilesUnchanged=$true;nativeCompilerTarget='aarch64-pc-windows-msvc';publicHeaderConditionals='non-Windows public branch: /U_WIN32; _WIN64 and _M_ARM64 preserved';generatedSourceAllocatorBranch='standard malloc/free; no qtest_stdlib replacement';generatedAttributeConfiguration='documented #ifndef overrides _ATTRIBUTE_UNUSED/_ATTRIBUTE_VISIBILITY empty for MSVC';wholeWindowsHeaderClosureQualified=$false;sdkEquivalenceQualified=$false;observed=$observed;rustObserved=$rust;executable=(Get-HexagonIdentity $executable);rustExecutable=(Get-HexagonIdentity $rustExecutable)}
}
Assert-Hexagon ($IsWindows -and [Runtime.InteropServices.RuntimeInformation]::OSArchitecture -eq [Runtime.InteropServices.Architecture]::Arm64 -and $env:RUNNER_ENVIRONMENT -ceq 'github-hosted') 'this control runs only on hosted native Windows ARM64; no owner-device workload'
$Output=[IO.Path]::GetFullPath($Output);$temp=[IO.Path]::GetFullPath($env:RUNNER_TEMP)
Assert-Hexagon (-not (Test-Path -LiteralPath $Output) -and -not [IO.Path]::GetRelativePath($temp,$Output).StartsWith('..')) 'select new empty hosted RUNNER_TEMP directory'
$null=New-Item -ItemType Directory -Path $Output;$review=Join-Path $Output 'review';$null=New-Item -ItemType Directory -Path $review;$control=New-HexagonControl $review (Get-HexagonEnvironment) 600
$report=@{schemaVersion=1;kind='hosted-windows-arm64-public-queue-wire-control';passed=$false;sourceCommit=$env:GITHUB_SHA;runId=$env:GITHUB_RUN_ID;runAttempt=$env:GITHUB_RUN_ATTEMPT;controller=(Get-HexagonIdentity $PSCommandPath);driverLoaded=$false;sdkUsed=$false;deviceExecuted=$false;profileEligible=$false;scope='public queue/packet ABI and explicitly selected non-Windows public start/stop wire branch';wholeWindowsHeaderClosureQualified=$false;sdkEquivalenceQualified=$false}
try {
    $pinPath=Join-Path $PSScriptRoot 'windows-queue-abi.lock.json';$pin=Read-HexagonJson $pinPath;$report.pin=Get-HexagonIdentity $pinPath;$inputs=Receive-WindowsAbiInputs $control $Output $pin $report;$compiler=Get-NativeCompiler $control $Output $report
    $Rustc=(Resolve-Path $Rustc).Path;Assert-Arm64Pe $Rustc;$report.rustCompiler=Get-HexagonIdentity $Rustc;$banner=Invoke-HexagonControl $control rustc-identity @($Rustc,'-vV') $Output
    Assert-Hexagon (($banner -split '\r?\n') -ccontains 'host: aarch64-pc-windows-msvc') 'independent Rust compiler must be native Windows ARM64'
    Compare-WindowsQueueLayout $control $Output $compiler.path $inputs $Rustc $report;Compare-WindowsStartStopWire $control $Output $compiler.path $inputs $Rustc $report
    foreach ($component in $compiler.selected) {$null=Assert-HexagonIdentity $component.path $component};$null=Assert-HexagonIdentity $Rustc $report.rustCompiler;$report.compilerIdentitiesRechecked=$true;$report.passed=$true
} finally {$report.commands=@($control.commands);Write-HexagonJson (Join-Path $review 'provenance.json') $report}
@{passed=$report.passed;receipt=(Join-Path $review 'provenance.json')} | ConvertTo-Json -Compress
