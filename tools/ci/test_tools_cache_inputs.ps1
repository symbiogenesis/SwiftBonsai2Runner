#Requires -Version 7.6
param([string]$Case)
. (Join-Path $PSScriptRoot 'test_support.ps1')
$repository=Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$actionPath=Join-Path $repository '.github/actions/bonsai-tools/action.yml'
# Cargo's checked-in manifests use one-line path/workspace declarations. Fail
# closed for unfamiliar local declaration syntax rather than guessing a closure.
function Get-CargoDependencyDeclarations([string]$Manifest,[bool]$WorkspaceOnly=$false) {
    $table=''; $result=@{}
    foreach ($line in [IO.File]::ReadAllLines($Manifest)) {
        if ($line -cmatch '^\[([^\[].*)\]\s*$') { $table=$Matches[1]; continue }
        if ($line -cmatch '^\[\[') { $table=''; continue }
        $selected=if ($WorkspaceOnly) { $table -ceq 'workspace.dependencies' } else { $table -cmatch '^(?:(?:target\..+)\.)?(?:build-|dev-)?dependencies$' }
        if (-not $selected -or $line -cmatch '^\s*(?:#|$)') { continue }
        if ($line -cnotmatch '^\s*([A-Za-z0-9_-]+)(\.workspace)?\s*=\s*(.+?)\s*$') { throw "Extend cache closure controls for dependency syntax in $Manifest" }
        $name=$Matches[1]; $workspace=$Matches[2]; $value=$Matches[3]
        if ($workspace) { if ($value -cne 'true') { throw 'Unexpected workspace dependency declaration' }; $result[$name]=@{Workspace=$true}; continue }
        if ($value -cmatch '\bpath\s*=\s*"([^"\\]+)"') { $result[$name]=@{Path=$Matches[1]}; continue }
        if ($value -cmatch '\bworkspace\s*=\s*true\b') { $result[$name]=@{Workspace=$true}; continue }
        if ($value -cmatch '\b(?:path|workspace)\b' -or ($value.StartsWith('{') -and -not $value.EndsWith('}'))) { throw "Extend cache closure controls for local dependency syntax in $Manifest" }
        $result[$name]=@{}
    }
    return $result
}
function Get-CargoPackageClosure([string]$Root,[string]$Initial) {
    $Root=[IO.Path]::GetFullPath($Root); $workspace=Get-CargoDependencyDeclarations (Join-Path $Root 'Cargo.toml') $true
    $pending=[Collections.Generic.Stack[string]]::new(); $pending.Push((Join-Path $Root $Initial)); $result=[Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    while ($pending.Count) {
        $package=[IO.Path]::GetFullPath($pending.Pop()); if (-not $package.StartsWith($Root+[IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)) { throw 'Path dependency escaped repository' }; if (-not $result.Add($package)) { continue }
        $declarations=Get-CargoDependencyDeclarations (Join-Path $package 'Cargo.toml')
        foreach ($name in $declarations.Keys) {
            $declaration=$declarations[$name]; $base=$package
            if ($declaration.ContainsKey('Workspace')) { if (-not $workspace.ContainsKey($name)) { throw "Unknown workspace dependency $name" }; $declaration=$workspace[$name]; $base=$Root }
            if ($declaration.ContainsKey('Path')) { $pending.Push((Join-Path $base $declaration.Path)) }
        }
    }
    return ,@($result)
}
function Get-CargoEmbeddedFiles([string]$Package,[string]$Source) {
    $text=([IO.File]::ReadAllText($Source) -split '#\[cfg\(test\)\]',2)[0]
    $ordinary=[regex]::Matches($text,'include_(?:str|bytes)!\(\s*"([^"\\]+)"\s*\)')
    $manifestRelative=[regex]::Matches($text,'include_(?:str|bytes)!\(\s*concat!\(\s*env!\("CARGO_MANIFEST_DIR"\),\s*"([^"\\]+)"\s*\)\s*\)')
    if ([regex]::Matches($text,'\binclude_(?:str|bytes)!\s*\(').Count -ne $ordinary.Count+$manifestRelative.Count) { throw "Extend cache closure controls for an unfamiliar include expression in $Source" }
    foreach ($match in $ordinary) { [IO.Path]::GetFullPath((Join-Path ([IO.Path]::GetDirectoryName($Source)) $match.Groups[1].Value)) }
    foreach ($match in $manifestRelative) { [IO.Path]::GetFullPath((Join-Path $Package $match.Groups[1].Value.TrimStart('/'))) }
    if ([IO.Path]::GetFileName($Source) -ceq 'build.rs') { foreach ($match in [regex]::Matches($text,'\.set_icon\("([^"\\]+)"\)')) { [IO.Path]::GetFullPath((Join-Path $Package $match.Groups[1].Value)) } }
}
function Get-RequiredToolsFiles([string]$Root) {
    $files=[Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    foreach ($name in @('Cargo.toml','Cargo.lock','rust-toolchain.toml')) { [void]$files.Add((Join-Path $Root $name)) }
    foreach ($file in Get-ChildItem -LiteralPath (Join-Path $Root '.cargo') -File -Recurse) { [void]$files.Add($file.FullName) }
    foreach ($package in (Get-CargoPackageClosure $Root 'tools/bonsai')) {
        [void]$files.Add((Join-Path $package 'Cargo.toml')); $sources=[Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
        foreach ($file in Get-ChildItem -LiteralPath $package -File -Filter '*.rs' -Recurse) { [void]$sources.Add($file.FullName) }
        # Binaries may explicitly point outside the package directory.
        foreach ($match in [regex]::Matches([IO.File]::ReadAllText((Join-Path $package 'Cargo.toml')),'(?m)^path\s*=\s*"([^"\\]+\.rs)"\s*$')) { [void]$sources.Add([IO.Path]::GetFullPath((Join-Path $package $match.Groups[1].Value))) }
        foreach ($source in $sources) {
            [void]$files.Add($source); $relative=[IO.Path]::GetRelativePath($package,$source).Replace('\','/')
            if ('tests' -cin ($relative -split '/') -or $source -cmatch '(?:_tests|tests)\.rs$') { continue }
            foreach ($embedded in @(Get-CargoEmbeddedFiles $package $source)) { [void]$files.Add($embedded) }
        }
    }
    foreach ($file in $files) { $relative=[IO.Path]::GetRelativePath($Root,[IO.Path]::GetFullPath($file)).Replace('\','/'); if ($relative.StartsWith('../',[StringComparison]::Ordinal)) { throw 'Embedded build input escaped repository' }; $relative }
}
function Get-ToolsHashPatterns {
    $text=[IO.File]::ReadAllText($actionPath)
    if ($text -cnotmatch 'TOOLS_SOURCE_ID: \$\{\{ hashFiles\((.*)\) \}\}') { throw 'Missing exact packaged-tool source hash.' }
    return @([regex]::Matches($Matches[1],'''([^'']+)''') | ForEach-Object { $_.Groups[1].Value })
}
function Get-UncoveredToolsFiles([string[]]$Files,[string[]]$Patterns) {
    foreach ($file in $Files) { $covered=$false; foreach ($pattern in $Patterns) { if ([Management.Automation.WildcardPattern]::new($pattern,[Management.Automation.WildcardOptions]::CultureInvariant).IsMatch($file)) { $covered=$true; break } }; if (-not $covered) { $file } }
}
function Invoke-ToolsEnvironmentFixture([object[]]$Scenarios) {
    $scratch=New-CiScratch 'bonsai-tools-identity-'; $original=@{}; foreach ($entry in [Environment]::GetEnvironmentVariables().GetEnumerator()) { $original[$entry.Key]=$entry.Value }
    try {
        foreach ($name in @([Environment]::GetEnvironmentVariables().Keys)) { if ($name.ToUpperInvariant().StartsWith('RUST',[StringComparison]::Ordinal) -or $name.ToUpperInvariant().StartsWith('CARGO_',[StringComparison]::Ordinal)) { [Environment]::SetEnvironmentVariable($name,$null) } }
        $cargoDirectory=Join-Path $scratch 'cargo'; [void][IO.Directory]::CreateDirectory($cargoDirectory)
        $environment=@{TOOLS_SOURCE_ID=('a'*64);RUNNER_ENVIRONMENT='github-hosted';ImageOS='fixture-os';ImageVersion='fixture-image';RUNNER_TEMP=$scratch;CARGO_HOME=$cargoDirectory;GITHUB_ENV=(Join-Path $scratch 'environment')}; foreach ($key in $environment.Keys) { [Environment]::SetEnvironmentVariable($key,$environment[$key]) }
        $action=[IO.File]::ReadAllText($actionPath); $begin=$action.IndexOf('        $buildInputs = '); $end=$action.IndexOf('        "BONSAI_TOOLS_SOURCE_ID=',$begin); $script=(($action.Substring($begin,$end-$begin) -split '\r?\n' | ForEach-Object { if ($_.Length -ge 8) { $_.Substring(8) } else { '' } }) -join "`n")
        $result=@()
        foreach ($scenario in $Scenarios) {
            foreach ($key in $scenario.environment.Keys) { if ($null -eq $scenario.environment[$key]) { Remove-Item -LiteralPath "Env:$key" -ErrorAction SilentlyContinue } }
            foreach ($key in $scenario.environment.Keys) { if ($null -ne $scenario.environment[$key]) { [Environment]::SetEnvironmentVariable($key,$scenario.environment[$key]) } }
            foreach ($name in @('config','config.toml')) { $path=Join-Path $cargoDirectory $name; if ($scenario.ContainsKey($name)) { [IO.File]::WriteAllText($path,$scenario[$name]) } elseif ([IO.File]::Exists($path)) { [IO.File]::Delete($path) } }
            [IO.File]::WriteAllText($env:GITHUB_ENV,''); & ([scriptblock]::Create($script)) | Out-Null
            $result+=@([IO.File]::ReadAllLines($env:GITHUB_ENV) | Where-Object { $_.StartsWith('BONSAI_TOOLS_CACHE_HOST=',[StringComparison]::Ordinal) })
        }
        return ,$result
    } finally { foreach ($name in @([Environment]::GetEnvironmentVariables().Keys)) { if (-not $original.ContainsKey($name)) { [Environment]::SetEnvironmentVariable($name,$null) } }; foreach ($name in $original.Keys) { [Environment]::SetEnvironmentVariable($name,$original[$name]) }; Remove-CiScratch $scratch }
}
$cases=[ordered]@{
 real_dependency_embedded_closure_hashed={ $files=@(Get-RequiredToolsFiles $repository); Assert-CiEqual @(Get-UncoveredToolsFiles $files (Get-ToolsHashPatterns)) @(); foreach ($file in $files) { Assert-Ci (Test-Path -LiteralPath (Join-Path $repository $file) -PathType Leaf) $file } }
 missing_audio_and_embedded_input_detected={ $files=@(Get-RequiredToolsFiles $repository); $patterns=Get-ToolsHashPatterns; foreach ($path in @('src/audio/src/lib.rs','tools/canary-asr/model.lock.json','tools/canary-asr/tokenizer.lock.json','tools/qwen-tts/model.lock.json','tools/hexagon/diagnostic.manifest','src/chat-ui/assets/bonsai.ico','LICENSE','NOTICE')) { Assert-Ci ($path -cin $files) $path; $reduced=@($patterns | Where-Object { -not [Management.Automation.WildcardPattern]::new($_).IsMatch($path) }); Assert-Ci ($path -cin @(Get-UncoveredToolsFiles $files $reduced)) $path } }
 traversal_and_literals_use_manifest_owner={
    $scratch=New-CiScratch 'bonsai-cache-closure-'
    try {
        Write-CiFixture $scratch 'Cargo.toml' "[workspace.dependencies]`nshared = { path = `"shared`" }`n"
        Write-CiFixture $scratch 'tools/Cargo.toml' "[dependencies]`nshared.workspace = true`n[target.'cfg(windows)'.build-dependencies]`nleaf = { path = `"../leaf`" }`n"
        Write-CiFixture $scratch 'shared/Cargo.toml' ''; Write-CiFixture $scratch 'leaf/Cargo.toml' ''
        Assert-CiSet (Get-CargoPackageClosure $scratch 'tools') @((Join-Path $scratch 'tools'),(Join-Path $scratch 'shared'),(Join-Path $scratch 'leaf'))
        $source=Join-Path $scratch 'tools/build.rs'; [IO.File]::WriteAllText($source,'const A: &str = include_str!("../shared/data");'+"`n"+'const B: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../leaf/data"));'+"`n"+'r.set_icon("../leaf/icon.ico");')
        Assert-CiSet @(Get-CargoEmbeddedFiles (Split-Path $source -Parent) $source) @((Join-Path $scratch 'shared/data'),(Join-Path $scratch 'leaf/data'),(Join-Path $scratch 'leaf/icon.ico'))
        [IO.File]::WriteAllText($source,'const A: &str = include_str!(unreviewed!());'); Assert-CiReject { Get-CargoEmbeddedFiles (Split-Path $source -Parent) $source }
    } finally { Remove-CiScratch $scratch }
 }
 native_caches_share_environment_filter={ $filters=@(); foreach ($name in @('bonsai-tools','ci-tool')) { $text=[IO.File]::ReadAllText((Join-Path $repository ".github/actions/$name/action.yml")); $filters+=,@($text -split '\r?\n' | Where-Object { $_.Contains('$buildInputs = @(') } | ForEach-Object Trim) }; Assert-CiEqual $filters[0].Count 1; Assert-CiEqual $filters[0] $filters[1] }
 both_cache_layers_bind_same_inputs={ $action=[IO.File]::ReadAllText($actionPath); $exact=(($action -split '    - name: Reuse only the exact packaged CLI',2)[1] -split '    - name:',2)[0]; $incremental=(($action -split '    - name: Reuse Cargo downloads',2)[1] -split '    - uses:',2)[0]; foreach ($block in @($exact,$incremental)) { Assert-Ci ($block.Contains('${{ env.BONSAI_TOOLS_SOURCE_ID }}')); Assert-Ci ($block.Contains('${{ env.BONSAI_TOOLS_CACHE_HOST }}')) }; Assert-Ci (-not $exact.Contains('restore-keys:')); Assert-Ci ($action.Contains('HostId = $env:BONSAI_TOOLS_CACHE_HOST')); Assert-Ci ($action.Contains('$receipt.HostId -cne $env:BONSAI_TOOLS_CACHE_HOST')) }
 compiler_profile_and_cargo_config_changes_invalidate={
    foreach ($variable in @('RUSTFLAGS','CARGO_ENCODED_RUSTFLAGS','RUSTC','RUSTC_WRAPPER','RUSTC_WORKSPACE_WRAPPER','RUSTC_BOOTSTRAP','CARGO_INCREMENTAL','CARGO_BUILD_RUSTFLAGS','CARGO_PROFILE_RELEASE_LTO','CARGO_TARGET_AARCH64_PC_WINDOWS_MSVC_LINKER')) { $values=Invoke-ToolsEnvironmentFixture @(@{environment=@{}},@{environment=@{$variable='changed'}}); Assert-Ci ($values[0] -cne $values[1]) $variable }
    foreach ($filename in @('config','config.toml')) { $values=Invoke-ToolsEnvironmentFixture @(@{environment=@{}},@{environment=@{};$filename="[build]`njobs = 2`n"},@{environment=@{};$filename="[build]`njobs = 4`n"}); Assert-CiEqual @($values | Sort-Object -Unique).Count 3 }
 }
 mixed_case_names_have_same_build_identity={ foreach ($pair in @(@('RUSTFLAGS','RustFlags'),@('CARGO_PROFILE_RELEASE_LTO','Cargo_Profile_Release_Lto'))) { $values=Invoke-ToolsEnvironmentFixture @(@{environment=@{}},@{environment=@{$pair[0]='changed'}},@{environment=@{$pair[0]=$null;$pair[1]='changed'}},@{environment=@{$pair[1]=$null}}); Assert-CiEqual $values[0] $values[3]; Assert-CiEqual $values[1] $values[2]; Assert-Ci ($values[0] -cne $values[1]) } }
 metadata_and_override_paths_do_not_partition={ $values=Invoke-ToolsEnvironmentFixture @(@{environment=@{}},@{environment=@{GITHUB_RUN_ID='new-run';CARGO_TARGET_DIR='different-target';CARGO_BUILD_FINGERPRINT='mtime'}}); Assert-CiEqual $values[0] $values[1] }
}
Invoke-CiCases $cases $Case
