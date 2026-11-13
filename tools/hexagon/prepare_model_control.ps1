#Requires -Version 7.6
param(
    [Parameter(Mandatory)][string]$RunnerRepository,[Parameter(Mandatory)][string]$PrismRepository,
    [Parameter(Mandatory)][string]$Probe,[Parameter(Mandatory)][string]$Supervisor,[Parameter(Mandatory)][string]$Compiler,
    [Parameter(Mandatory)][string]$SuccessfulBuildReceipt,[Parameter(Mandatory)][string]$OutputDirectory,[Parameter(Mandatory)][string]$Git,
    [ValidateSet('none','loaded-zero','standard-scale-1.3')][string]$AdapterCase='standard-scale-1.3',
    [long[]]$TokenIds=@(42,43,128),[string[]]$Projections=@('blk.0.ffn_up.weight'),
    [switch]$ResidentHostWeights,[switch]$BatchFourOperations,[switch]$BatchTenOperations)
. "$PSScriptRoot/Hexagon.Common.ps1"
function Get-ControlPin([string]$Path) {$identity=Get-HexagonIdentity $Path;return @{bytes=$identity.bytes;sha256=$identity.sha256}}
function Get-ControlRevision([string]$Source) {
    $dirty=& $Git -c "safe.directory=$($Source.Replace('\','/'))" -C $Source status --porcelain
    Assert-Hexagon ($LASTEXITCODE -eq 0 -and -not $dirty) ('The built source must be clean: '+$Source)
    $revision=& $Git -c "safe.directory=$($Source.Replace('\','/'))" -C $Source rev-parse HEAD
    Assert-Hexagon ($LASTEXITCODE -eq 0 -and $revision -cmatch '^[0-9a-f]{40}$') 'source identity unavailable';return $revision
}
Assert-Hexagon (-not ($BatchFourOperations -and $BatchTenOperations)) 'Select at most one explicit HMX batch mode.'
Assert-Hexagon ($TokenIds.Count -ge 1 -and $TokenIds.Count -le 3 -and @($TokenIds | Where-Object {$_ -lt 0}).Count -eq 0) 'The current snapshot budget permits one to three nonnegative token IDs.'
$maximum=if ($ResidentHostWeights) {2} else {4}
Assert-Hexagon ($Projections.Count -ge 1 -and $Projections.Count -le $maximum -and $Projections[0] -ceq 'blk.0.ffn_up.weight' -and @($Projections | Sort-Object -Unique -CaseSensitive).Count -eq $Projections.Count) 'Select unique FFN projections beginning with blk.0.ffn_up.weight within the arena bound.'
$built=Read-HexagonJson $SuccessfulBuildReceipt
Assert-Hexagon ($built.ContainsKey('exitCode') -and $built.exitCode -eq 0 -and (-not $built.ContainsKey('timedOut') -or -not $built.timedOut)) 'The supplied build receipt does not record a successful build.'
$RunnerRepository=[IO.Path]::GetFullPath($RunnerRepository);$PrismRepository=[IO.Path]::GetFullPath($PrismRepository)
$prismRevision=Get-ControlRevision $PrismRepository;$runnerRevision=Get-ControlRevision $RunnerRepository
$OutputDirectory=[IO.Path]::GetFullPath($OutputDirectory);Assert-Hexagon (-not (Test-Path -LiteralPath $OutputDirectory)) 'new output directory required'
$null=New-Item -ItemType Directory -Path (Join-Path $OutputDirectory 'model-bundle')
$probeCopy=Join-Path $OutputDirectory 'model-bundle/prism-hexagon-model-probe.exe';$supervisorCopy=Join-Path $OutputDirectory 'model-bundle/bonsai-tools.exe'
Copy-Item -LiteralPath $Probe -Destination $probeCopy;Copy-Item -LiteralPath $Supervisor -Destination $supervisorCopy;Copy-Item -LiteralPath $SuccessfulBuildReceipt -Destination (Join-Path $OutputDirectory 'observed-probe-build.json')
$names=[Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal);$null=$names.Add('Cargo.toml');$null=$names.Add('Cargo.lock')
foreach ($name in @('build.rs','rust-toolchain.toml','.cargo/config.toml')) {if (Test-Path -LiteralPath (Join-Path $PrismRepository $name) -PathType Leaf) {$null=$names.Add($name)}}
function Add-ControlSources([string]$Directory) {
    foreach ($item in Get-ChildItem -LiteralPath $Directory -Force) {
        Assert-Hexagon (-not ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) 'source identity cannot traverse redirects'
        if ($item.PSIsContainer) {if ($item.Name -notin @('target','.git','.worktrees')) {Add-ControlSources $item.FullName}} else {$null=$names.Add([IO.Path]::GetRelativePath($PrismRepository,$item.FullName).Replace('\','/'))}
    }
}
foreach ($base in @('src','tools')) {Add-ControlSources (Join-Path $PrismRepository $base)}
$sources=@($names | Sort-Object -CaseSensitive | ForEach-Object {$pin=Get-ControlPin (Join-Path $PrismRepository $_);@{path=$_;bytes=$pin.bytes;sha256=$pin.sha256}})
$compilerPin=Get-ControlPin $Compiler;$receipt=@{schemaVersion=1;kind='local-prism-hexagon-model-probe-build';hostedBuild=$false;buildSucceeded=$true;cargoLocked=$true;target='aarch64-pc-windows-msvc';prismRevision=$prismRevision;compiler=@{path=[IO.Path]::GetFullPath($Compiler);bytes=$compilerPin.bytes;sha256=$compilerPin.sha256};executable=(Get-ControlPin $probeCopy);sources=$sources}
$receiptPath=Join-Path $OutputDirectory 'local-model-build.json';Write-HexagonJson $receiptPath $receipt
$envelope=Read-HexagonJson (Join-Path $RunnerRepository 'docs/evidence/hexagon-adapter-session-20261103/model-device-request.json');$request=$envelope.request
$updates=@{repositoryDirectory=$RunnerRepository;prismRepositoryDirectory=$PrismRepository;expectedPrismRevision=$prismRevision;executablePath=$probeCopy;executableSha256=$receipt.executable.sha256;buildReceiptPath=$receiptPath;outputDirectory=(Join-Path $OutputDirectory 'device');tokenId=$TokenIds[0];tokenIds=$TokenIds;projections=$Projections;executionBackend='hmx-bitplane';residentHostWeights=[bool]$ResidentHostWeights;batchFourOperations=[bool]$BatchFourOperations;batchTenOperations=[bool]$BatchTenOperations;adapterCase=$AdapterCase;cpuBackend='neon';rowWorkers=4;maximumSeconds=300}
foreach ($key in $updates.Keys) {$request[$key]=$updates[$key]};if ($AdapterCase -ceq 'none') {$request.Remove('adapterPath')}
foreach ($name in @('modelPath','packageDirectory','signtoolPath')) {Assert-Hexagon (Test-Path -LiteralPath $request[$name]) ('The retained control input is absent: '+$request[$name])}
if ($AdapterCase -cne 'none') {Assert-Hexagon (Test-HexagonEqual (Get-ControlPin $request.adapterPath) @{bytes=9682464;sha256='f1669534803d340a496015f5c45125f3437b4d13ec764f40e34488ce83967f42'}) 'The approved standard adapter identity differs.'}
$requestPath=Join-Path $OutputDirectory 'model-device-request.json';Write-HexagonJson $requestPath $envelope
$intake=@{runnerRevision=$runnerRevision;prismRevision=$prismRevision;supervisor=(Get-ControlPin $supervisorCopy);probe=(Get-ControlPin $probeCopy);sourceFiles=$sources.Count;observedBuildReceipt=(Get-ControlPin (Join-Path $OutputDirectory 'observed-probe-build.json'));deviceCommand=@($supervisorCopy,'hexagon-tools','--request',$requestPath);deviceAttempted=$false;performanceQualified=$false}
Write-HexagonJson (Join-Path $OutputDirectory 'intake.json') $intake;$intake | ConvertTo-Json -Depth 64
