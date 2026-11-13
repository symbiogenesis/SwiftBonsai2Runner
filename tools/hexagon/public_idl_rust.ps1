#Requires -Version 7.6
. "$PSScriptRoot/public_qaic_wire.ps1"
function Compare-PublicRustWire($Pin,$Report,$Control,[string]$Output,[string]$Review,[string]$Rustc) {
    $selection=$Pin.rustWireControl;Assert-Hexagon ($selection.deviceInvocationAuthorized -is [bool] -and -not $selection.deviceInvocationAuthorized) 'Rust wire encoding does not authorize device invocation'
    $capture=$Report.wireCapture;Assert-Hexagon ($capture.passed -is [bool] -and $capture.passed) 'independent generated-C wire capture must pass first'
    $compiler=(Resolve-Path $Rustc).Path;$compilerIdentity=Get-HexagonIdentity $compiler
    $wire=@{kind='independent-Rust-public-wire-encoding-control';passed=$false;controller=(Get-HexagonIdentity $PSCommandPath);compiler=$compilerIdentity;testExecutionAttempted=$false;testExecutionCompleted=$false;encodingExecutionAttempted=$false;encodingExecutionCompleted=$false;driverLoaded=$false;deviceExecuted=$false;sdkCompatibilityQualified=$false;profileEligible=$false};$Report.rustWireControl=$wire;$Report.stage='independent-Rust-wire-compiler'
    $version=Invoke-HexagonControl $Control wire-rustc-version @($compiler,'-vV') $Output
    Assert-Hexagon (($version -split '\r?\n') -ccontains ('host: '+$selection.target)) 'Rust wire control requires fixed native Linux target'
    $sysroot=(Invoke-HexagonControl $Control wire-rustc-sysroot @($compiler,'--print','sysroot') $Output).Trim()
    Assert-Hexagon ([IO.Path]::GetFileName($sysroot) -ceq $selection.toolchain -and [IO.Path]::GetFullPath((Join-Path (Split-Path $compiler) '..')) -ceq $sysroot) 'Rust compiler resolved outside selected dated toolchain'
    $wire.toolchain=$selection.toolchain;$wire.target=$selection.target;$wire.sysroot=$sysroot;$wire.identityScope='selected rustc/driver files and verbose identity; not-complete-OS-closure'
    $drivers=@(Get-ChildItem -LiteralPath (Join-Path $sysroot 'lib') -Filter 'librustc_driver-*.so');Assert-Hexagon ($drivers.Count -le 8) 'Rust compiler driver identity count exceeds bound'
    $wire.selectedCompilerDrivers=@($drivers | ForEach-Object {Assert-Hexagon ($_.Length -le 536870912) 'Rust compiler driver exceeds byte bound';$item=Get-HexagonIdentity $_.FullName;$item.path=$_.FullName;$item})
    $source=Join-Path $PSScriptRoot $selection.source.name;$sourceIdentity=Get-HexagonIdentity $source;Assert-Hexagon (Test-HexagonEqual $sourceIdentity $selection.source) 'independent Rust encoder differs from source pin'
    $retained=Join-Path $Review $selection.source.name;Copy-Item -LiteralPath $source -Destination $retained;$wire.source=Get-HexagonIdentity $retained;$linker=$capture.compiler.path;$null=Assert-HexagonIdentity $linker $capture.compiler
    $flags=@('--edition=2024','--target',$selection.target,'-D','warnings','-C','opt-level=2','-C','overflow-checks=yes','-C',('linker='+$linker));$wire.compilerFlags=$flags
    $executable=Join-Path $Output 'public-idl-rust-control';$tests=Join-Path $Output 'public-idl-rust-contract';$Report.stage='independent-Rust-wire-build'
    $null=Invoke-HexagonControl $Control wire-rust-build (@($compiler)+$flags+@($source,'-o',$executable)) $Output 30
    $null=Invoke-HexagonControl $Control wire-rust-test-build (@($compiler)+$flags+@('--test',$source,'-o',$tests)) $Output 30
    Assert-Hexagon ((Test-HexagonEqual (Get-HexagonIdentity $compiler) $compilerIdentity) -and (Test-HexagonEqual (Get-HexagonIdentity $source) $sourceIdentity)) 'Rust compiler or encoder changed during bounded build'
    $wire.executable=Get-HexagonIdentity $executable;$wire.contractExecutable=Get-HexagonIdentity $tests;$Report.stage='independent-Rust-wire-test';$wire.testExecutionAttempted=$true
    $null=Invoke-HexagonControl $Control wire-rust-contract @($tests) $Output 5;$wire.testExecutionCompleted=$true;$Report.stage='independent-Rust-wire-encoding';$wire.encodingExecutionAttempted=$true
    $text=Invoke-HexagonControl $Control wire-rust-encoding @($executable) $Output 5;$wire.encodingExecutionCompleted=$true
    Assert-Hexagon ((Test-HexagonEqual (Get-HexagonIdentity $executable) $wire.executable) -and (Test-HexagonEqual (Get-HexagonIdentity $tests) $wire.contractExecutable)) 'Rust control bytes changed during execution'
    Assert-Hexagon ([Text.Encoding]::UTF8.GetByteCount($text) -le 16384) 'Rust encoding receipt exceeds byte bound';$actual=ConvertFrom-HexagonJson ([Text.Encoding]::UTF8.GetBytes($text))
    $shared=@('schemaVersion','testVectorId','handleHex','start','stop');$falseFlags=@('driverLoaded','deviceExecuted','sdkCompatibilityQualified','profileEligible')
    Assert-Hexagon ((Test-HexagonEqual @($actual.Keys | Sort-Object) @($shared+$falseFlags+@('kind') | Sort-Object)) -and $actual.kind -ceq 'public-idl-rust-encoding-control') 'Rust encoding emitted unsupported receipt shape'
    foreach ($name in $falseFlags) {Assert-Hexagon ($actual[$name] -is [bool] -and -not $actual[$name]) 'Rust encoding cannot establish device or profile authority'}
    foreach ($name in $shared) {Assert-Hexagon (Test-HexagonEqual $actual[$name] $capture.observed[$name]) 'Rust encoding differs from independently captured generated-C wire'}
    $wire.observed=$actual;$wire.captureReceipt=Get-HexagonIdentity (Join-Path $Review 'wire-rust-encoding.log');$wire.generatedCWireMatched=$true;$wire.passed=$true;$Report.stage='independent-public-C-Rust-wire-matched';$Report.outcome='public-generated-C-and-Rust-wire-cross-checked-only'
}
