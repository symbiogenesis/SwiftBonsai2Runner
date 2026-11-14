#Requires -Version 7.6
# Compiler bootstrap and parameter forwarding. Native Rust owns the build after bootstrap.
[CmdletBinding(DefaultParameterSetName='Build')]
param([Parameter(ParameterSetName='Build')][string]$RepositoryRoot=(Split-Path $PSScriptRoot -Parent),
    [Parameter(ParameterSetName='Build')][string]$TargetDirectory=$env:CARGO_TARGET_DIR,
    [string]$DataDirectory=(Join-Path $env:LOCALAPPDATA 'SwiftBonsai2'),[string]$OutputPath,
    [Parameter(Mandatory,ParameterSetName='Package')][string]$PackageDirectory,
    [Parameter(Mandatory,ParameterSetName='Package')][string]$ManifestSha256)
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
$PSNativeCommandUseErrorActionPreference=$false
if ($PSCmdlet.ParameterSetName -eq 'Package') {
    $tool=Join-Path $PackageDirectory 'bin/bonsai-tools.exe'
    $selectedOutput=if($OutputPath){$OutputPath}else{Join-Path $DataDirectory 'build/distribution/SwiftBonsai2Runner.exe'}
    $request=@{operation='create';packageDirectory=$PackageDirectory;manifestSha256=$ManifestSha256;outputPath=$selectedOutput}
    & $tool chat-bundle --json (ConvertTo-Json -InputObject $request -Compress)
    exit $LASTEXITCODE
}
$request=@{operation='source-build';repositoryRoot=$RepositoryRoot;dataDirectory=$DataDirectory
    targetDirectory=$(if($TargetDirectory){$TargetDirectory}else{$null});outputPath=$(if($OutputPath){$OutputPath}else{$null})}
$json=ConvertTo-Json -InputObject $request -Compress
if ($env:BONSAI_TOOLS_PATH) { & $env:BONSAI_TOOLS_PATH release-package --json $json }
else {
    # The compiler pin must be selected before a Rust helper exists.
    $toolchain=Get-Content -LiteralPath (Join-Path $RepositoryRoot 'rust-toolchain.toml') -Raw
    $channel=[regex]::Match($toolchain,'(?m)^channel\s*=\s*"(nightly-[0-9]{4}-[0-9]{2}-[0-9]{2})"\s*$').Groups[1].Value
    if (-not $channel) { throw 'The bootstrap requires the repository-pinned dated nightly.' }
    $target=if($TargetDirectory){$TargetDirectory}else{Join-Path $DataDirectory 'build/maintenance'}
    & cargo "+$channel-aarch64-pc-windows-msvc" run --quiet --locked --release --manifest-path (Join-Path $RepositoryRoot 'Cargo.toml') -p bonsai-tools --bin bonsai-tools --target aarch64-pc-windows-msvc --target-dir $target -- release-package --json $json
}
exit $LASTEXITCODE
