#Requires -Version 7.6
param([Parameter(Mandatory)][string]$Source,[Parameter(Mandatory)][string]$Pin,[Parameter(Mandatory)][string]$Output,[Parameter(Mandatory)][string]$Rustc)
. "$PSScriptRoot/public_idl_rust.ps1"
function Assert-OrdinaryTree([string]$Path) {
    Assert-Hexagon ([IO.Path]::IsPathFullyQualified($Path)) 'expanded absolute source/output paths required'
    $cursor=[IO.Path]::GetFullPath($Path)
    while ($cursor) {if (Test-Path -LiteralPath $cursor) {Assert-Hexagon (-not ((Get-Item -LiteralPath $cursor -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)) 'linked source/output paths unsupported'};$cursor=Split-Path $cursor -Parent}
}
function Save-PublicOracle([string]$Path,$Value) {
    $bytes=[Text.Encoding]::ASCII.GetBytes((ConvertTo-Json -InputObject $Value -Depth 64 -EscapeHandling EscapeNonAscii)+"`n");Assert-Hexagon ($bytes.Length -le 2097152) 'public QAIC receipt exceeds byte bound'
    $temporary=$Path+'.partial';[IO.File]::WriteAllBytes($temporary,$bytes);[IO.File]::Move($temporary,$Path,$true)
}
function Save-CabalProjects([string]$Directory,[string]$Name,[string]$Review) {
    return @(foreach ($filename in @('cabal.project','cabal.project.freeze','cabal.project.local','dist-newstyle/cache/plan.json')) {
        $path=Join-Path $Directory $filename
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {Assert-Hexagon ($filename -ceq 'cabal.project.local') 'source build omitted exact dependency project/freeze/plan';continue}
        Assert-Hexagon ((Get-Item -LiteralPath $path).Length -le 2097152) 'dependency project/freeze/plan exceeds bound'
        $target=Join-Path $Review ($Name+'-'+$filename.Replace('/','-'));Copy-Item -LiteralPath $path -Destination $target;Get-HexagonIdentity $target
    })
}
Assert-Hexagon ($IsLinux -and [Runtime.InteropServices.RuntimeInformation]::OSArchitecture -eq [Runtime.InteropServices.Architecture]::X64 -and $env:GITHUB_ACTIONS -ceq 'true' -and $env:RUNNER_ENVIRONMENT -ceq 'github-hosted') 'this control requires hosted Linux x86_64; owner fallback unsupported'
foreach ($entry in @(@('GITHUB_SHA','^[a-f0-9]{40}$'),@('GITHUB_RUN_ID','^[0-9]{1,32}$'),@('GITHUB_RUN_ATTEMPT','^[0-9]{1,8}$'))) {Assert-Hexagon ([Environment]::GetEnvironmentVariable($entry[0]) -cmatch $entry[1]) 'exact hosted source/run identity required'}
Assert-OrdinaryTree $Source;Assert-OrdinaryTree $Output
Assert-Hexagon (-not (Test-Path -LiteralPath $Output) -and (Test-Path -LiteralPath (Split-Path $Output) -PathType Container)) 'new external output directory required'
$ancestor=Split-Path $Output;while ($ancestor) {Assert-Hexagon (-not (Test-Path -LiteralPath (Join-Path $ancestor '.git'))) 'build/evidence output must be outside Git storage';$ancestor=Split-Path $ancestor}
$selection=Read-HexagonJson $Pin
Assert-Hexagon ($selection.schemaVersion -eq 1 -and $selection.deviceInvocationAuthorized -is [bool] -and -not $selection.deviceInvocationAuthorized -and $selection.profileEligible -is [bool] -and -not $selection.profileEligible) 'unsupported public source-control authority'
$null=New-Item -ItemType Directory -Path $Output;$review=Join-Path $Output 'review';$inputs=Join-Path $review 'inputs-and-notices';$generated=Join-Path $Output 'generated';$null=New-Item -ItemType Directory -Path $inputs,$generated
$environment=Get-HexagonEnvironment
foreach ($name in @('GITHUB_TOKEN','GH_TOKEN','ACTIONS_RUNTIME_TOKEN','ACTIONS_ID_TOKEN_REQUEST_TOKEN','ACTIONS_ID_TOKEN_REQUEST_URL','GHC_PACKAGE_PATH','GHC_ENVIRONMENT','GHC_OPTIONS','CABAL_CONFIG','CABAL_DIR','CABAL_BUILDDIR','LD_PRELOAD','LD_LIBRARY_PATH','GCC_EXEC_PREFIX','COMPILER_PATH','LIBRARY_PATH','CPATH','C_INCLUDE_PATH','CPLUS_INCLUDE_PATH','CFLAGS','CPPFLAGS','LDFLAGS')) {$environment.Remove($name)}
$cabalDirectory=Join-Path $Output 'cabal';$null=New-Item -ItemType Directory -Path $cabalDirectory;$config=Join-Path $cabalDirectory 'config'
[IO.File]::WriteAllText($config,"repository hackage.haskell.org`n  url: https://hackage.haskell.org/`n  secure: True`nremote-repo-cache: $(Join-Path $cabalDirectory 'packages')`nstore-dir: $(Join-Path $cabalDirectory 'store')`n",[Text.UTF8Encoding]::new($false))
$environment.CABAL_DIR=$cabalDirectory;$environment.CABAL_CONFIG=$config;$environment.GHC_ENVIRONMENT='-';$control=New-HexagonControl $review $environment
$report=@{schemaVersion=1;kind='public-QAIC-independent-source-oracle';runnerRevision=$env:GITHUB_SHA;runId=$env:GITHUB_RUN_ID;runAttempt=$env:GITHUB_RUN_ATTEMPT;qaicCommit=$selection.qaicCommit;sourceSha256=(Get-FileHash -LiteralPath $PSCommandPath -Algorithm SHA256).Hash.ToLowerInvariant();pinSha256=(Get-FileHash -LiteralPath $Pin -Algorithm SHA256).Hash.ToLowerInvariant();outcome='pending';stage='source-identity';generatorExecuted=$false;generatorAttempted=$false;generatorCompleted=$false;versionQueryAttempted=$false;versionQueryCompleted=$false;idlGenerationAttempted=$false;idlGenerationCompleted=$false;hackageIndexState=$selection.hackageIndexState;sourceFiles=[Collections.Generic.List[object]]::new();publicInputs=[Collections.Generic.List[object]]::new();toolchain=@{};dependencyResolution=@();generatedFiles=[Collections.Generic.List[object]]::new();commands=@();wireCapture=$null;sdkExecutableUsed=$false;sdkImportsUsed=$false;sdkQaicEquivalenceQualified=$false;sdkImportEquivalenceQualified=$false;deviceCodeLoaded=$false;deviceInvocationAuthorized=$false;profileEligible=$false}
try {
    $root=(Invoke-HexagonControl $control source-root @('git','-C',$Source,'rev-parse','--show-toplevel') $Output).Trim()
    $revision=(Invoke-HexagonControl $control source-revision @('git','-C',$Source,'rev-parse','HEAD') $Output).Trim()
    $status=(Invoke-HexagonControl $control source-clean @('git','-C',$Source,'status','--porcelain') $Output).Trim()
    Assert-Hexagon ([IO.Path]::GetFullPath($root) -ceq [IO.Path]::GetFullPath($Source) -and $revision -ceq $selection.qaicCommit -and -not $status) 'QAIC checkout differs from exact clean reviewed source'
    foreach ($entry in $selection.sourceFiles) {$actual=Assert-HexagonIdentity (Join-Path $Source $entry.path) $entry;$actual.name=$entry.path;$report.sourceFiles.Add($actual)}
    $tracked=@((Invoke-HexagonControl $control source-inventory @('git','-C',$Source,'ls-files') $Output) -split '\r?\n' | Where-Object {$_})
    Assert-Hexagon ($tracked.Count -le 4096) 'QAIC tracked source inventory exceeds bound';$sourceBytes=0L
    $fullSource=@(foreach ($name in $tracked) {Assert-Hexagon (-not [IO.Path]::IsPathRooted($name) -and $name -notmatch '(^|/)\.\.(/|$)') 'unsafe tracked source path';$item=Get-HexagonIdentity (Join-Path $Source $name);$item.name=$name;$sourceBytes+=$item.bytes;Assert-Hexagon ($sourceBytes -le 67108864) 'QAIC tracked source bytes exceed bound';$item})
    Save-PublicOracle (Join-Path $review 'qaic-source-files.json') $fullSource
    Copy-Item -LiteralPath (Join-Path $Source 'LICENSE.txt') -Destination (Join-Path $inputs 'LICENSE-QAIC.txt')
    foreach ($name in @('CStub.c','Stub.c','Env.c','Allocator.c','Slim.c')) {Copy-Item -LiteralPath (Join-Path $Source ('src/Language/SO/'+$name)) -Destination (Join-Path $inputs ('QAIC-template-'+$name))}
    foreach ($entry in $selection.publicInputs) {$report.publicInputs.Add((Receive-HexagonPublic $entry $inputs))}
    $report.stage='toolchain-identity';$ghc=(Resolve-HexagonExecutable (Get-Command ghc -CommandType Application).Source);$cabal=(Resolve-HexagonExecutable (Get-Command cabal -CommandType Application).Source)
    $ghcVersion=(Invoke-HexagonControl $control ghc-version @($ghc,'--numeric-version') $Output).Trim();$cabalVersion=(Invoke-HexagonControl $control cabal-version @($cabal,'--numeric-version') $Output).Trim()
    Assert-Hexagon ($ghcVersion -ceq $selection.ghcVersion -and $cabalVersion -ceq $selection.cabalVersion) 'GHC/Cabal identity differs from pinned versions'
    $report.toolchain=@{ghcVersion=$ghcVersion;cabalVersion=$cabalVersion;ghcExecutable=(Get-HexagonIdentity $ghc);cabalExecutable=(Get-HexagonIdentity $cabal);setupAction=$selection.setupAction}
    $null=Invoke-HexagonControl $control ghc-info @($ghc,'--info') $Output;$null=Invoke-HexagonControl $control ghc-package-db @('ghc-pkg','dump') $Output;$report.toolchain.selectedGhcRuntime=Get-GhcRuntime $ghc $control $Output
    $sourceDirectory=Join-Path $Source 'src';$helper=Join-Path $sourceDirectory 'file2hs';$project="packages: .`nindex-state: $($selection.hackageIndexState)`nactive-repositories: hackage.haskell.org`n"
    foreach ($directory in @($sourceDirectory,$helper)) {foreach ($name in @('cabal.project','cabal.project.freeze','cabal.project.local')) {Assert-Hexagon (-not (Test-Path -LiteralPath (Join-Path $directory $name))) 'upstream source unexpectedly contains project override'};[IO.File]::WriteAllText((Join-Path $directory 'cabal.project'),$project,[Text.UTF8Encoding]::new($false))}
    $report.stage='pinned-dependency-resolution';$null=Invoke-HexagonControl $control hackage-update @($cabal,'update') $sourceDirectory 150
    foreach ($entry in @(@('file2hs',$helper),@('qaic',$sourceDirectory))) {$name=$entry[0];$directory=$entry[1];$null=Invoke-HexagonControl $control ($name+'-freeze') @($cabal,'freeze') $directory 60;$freeze=Join-Path $directory 'cabal.project.freeze';Assert-Hexagon ((Get-Item -LiteralPath $freeze).Length -le 65536) 'Cabal omitted bounded dependency freeze';$null=Assert-HexagonIdentity $freeze $selection.dependencyFreezePins[$name];Copy-Item -LiteralPath $freeze -Destination (Join-Path $review ($name+'-before-build.freeze'))}
    $report.stage='source-generator-build';$null=Invoke-HexagonControl $control upstream-make $selection.buildArguments $Source 420
    $report.dependencyResolution=@(Save-CabalProjects $helper file2hs $review)+@(Save-CabalProjects $sourceDirectory qaic $review)
    $plan=Read-HexagonJson (Join-Path $sourceDirectory 'dist-newstyle/cache/plan.json') 2097152;$external=@($plan['install-plan'] | Where-Object {$_.ContainsKey('pkg-src') -and $_['pkg-src'].type -ceq 'repo-tar'})
    Assert-Hexagon ($external.Count -eq 1) 'external source dependency differs from reviewed control';foreach ($name in $selection.externalPackagePin.Keys) {Assert-Hexagon (Test-HexagonEqual $external[0][$name] $selection.externalPackagePin[$name]) 'external source dependency differs from reviewed control'};$report.externalPackage=$selection.externalPackagePin
    foreach ($entry in @(@('file2hs',$helper),@('qaic',$sourceDirectory))) {Assert-Hexagon ((Get-FileHash -LiteralPath (Join-Path $entry[1] 'cabal.project.freeze')).Hash -ceq (Get-FileHash -LiteralPath (Join-Path $review ($entry[0]+'-before-build.freeze'))).Hash) 'upstream build changed preselected dependency freeze'}
    $executable=Join-Path $sourceDirectory 'Linux_ReleaseG/ship/qaic';$report.generatorExecutable=Get-HexagonIdentity $executable;$report.generatorAttempted=$true;$report.generatorExecuted=$null;$report.versionQueryAttempted=$true
    $null=Invoke-HexagonControl $control source-generator-version @($executable,'--version') $Output;$report.generatorExecuted=$true;$report.versionQueryCompleted=$true;$report.stage='independent-IDL-generation'
    $arguments=@($executable,'--map-dll','--standard-types',('--include-path='+$inputs),('--output-path='+$generated),(Join-Path $inputs 'htp_iface.idl'));$report.idlGenerationAttempted=$true
    $null=Invoke-HexagonControl $control independent-generation $arguments $Output 15;$report.generatorExecuted=$true;$report.generatorCompleted=$true;$report.idlGenerationCompleted=$true
    $files=@(Get-ChildItem -LiteralPath $generated);Assert-Hexagon (Test-HexagonEqual @($files.Name | Sort-Object) @($selection.expectedGeneratedFiles | Sort-Object)) 'public generator output set differs from fixed IDL contract'
    $retained=Join-Path $review 'generated-public-source';$null=New-Item -ItemType Directory -Path $retained
    foreach ($file in $files) {Assert-Hexagon (-not ($file.Attributes -band [IO.FileAttributes]::ReparsePoint) -and -not $file.PSIsContainer -and $file.Length -le 1048576) 'public generated output exceeds per-file bound';$data=Read-HexagonBytes $file.FullName;$text=[Text.Encoding]::UTF8.GetString($data);$item=Get-HexagonIdentity $file.FullName;$item.copyrightMarkerPresent=$text.ToLowerInvariant().Contains('copyright');$item.bsdClearIdentifierPresent=$text.Contains('BSD-3-Clause-Clear');$report.generatedFiles.Add($item);Copy-Item -LiteralPath $file.FullName -Destination (Join-Path $retained $file.Name)}
    [IO.File]::WriteAllText((Join-Path $review 'NOTICES.txt'),"Independent public-source QAIC output only.`nQAIC compiler/templates: BSD-3-Clause-Clear; preserve LICENSE-QAIC.txt and template notices.`nPublic FastRPC IDL imports/runtime headers: BSD-3-Clause; preserve LICENSE-FastRPC.txt and original notices.`nTarget htp_iface.idl: llama.cpp MIT; preserve LICENSE-llama.cpp.txt and original input.`nSDK6.7 generator/import equivalence and device invocation remain unqualified.`n",[Text.UTF8Encoding]::new($false))
    $report.stage='independent-public-generation-complete';$report.outcome='generated-independent-oracle-only'
    Assert-Hexagon ($selection.generationReviewOnly -is [bool]) 'public generation review mode must be explicit boolean'
    if ($selection.generationReviewOnly) {$report.stage='public-standard-type-generation-review';$report.outcome='public-standard-generated-source-awaiting-review';$report.wireCapture=@{passed=$false;executionSelected=$false;reason='new-standard-type-output-requires-independent-review'}} else {Invoke-PublicWireCapture $selection $report $control $Output $generated $review;Compare-PublicRustWire $selection $report $control $Output $review $Rustc}
} catch {$report.outcome='failed';$report.failureClass=$_.Exception.GetType().Name;$report.failure=$_.Exception.Message.Substring(0,[Math]::Min(512,$_.Exception.Message.Length));throw}
finally {$report.commands=@($control.commands);$report.sourceFiles=@($report.sourceFiles);$report.publicInputs=@($report.publicInputs);$report.generatedFiles=@($report.generatedFiles);Save-PublicOracle (Join-Path $review 'host.json') $report}
