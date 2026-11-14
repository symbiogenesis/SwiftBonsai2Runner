#Requires -Version 7.6
# Packages already built native inputs; all intake and source policy lives in Rust.
param(
    [Parameter(Mandatory)][string]$RunnerRepository,[Parameter(Mandatory)][string]$PrismRepository,
    [Parameter(Mandatory)][string]$Probe,[Parameter(Mandatory)][string]$Supervisor,[Parameter(Mandatory)][string]$Compiler,
    [Parameter(Mandatory)][string]$SuccessfulBuildReceipt,[Parameter(Mandatory)][string]$OutputDirectory,[Parameter(Mandatory)][string]$Git,
    [string]$AdapterCase='standard-scale-1.3',[long[]]$TokenIds=@(42,43,128),
    [string[]]$Projections=@('blk.0.ffn_up.weight'),[switch]$ResidentHostWeights,
    [switch]$BatchFourOperations,[switch]$BatchTenOperations)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$tool = if ($env:BONSAI_TOOLS_PATH) { $env:BONSAI_TOOLS_PATH } else { $Supervisor }
@{ operation = 'prepare-model-control'; request = @{
    runnerRepository = [IO.Path]::GetFullPath($RunnerRepository); prismRepository = [IO.Path]::GetFullPath($PrismRepository)
    probe = [IO.Path]::GetFullPath($Probe); supervisor = [IO.Path]::GetFullPath($Supervisor); compiler = [IO.Path]::GetFullPath($Compiler)
    successfulBuildReceipt = [IO.Path]::GetFullPath($SuccessfulBuildReceipt); outputDirectory = [IO.Path]::GetFullPath($OutputDirectory)
    git = $Git; adapterCase = $AdapterCase; tokenIds = $TokenIds; projections = $Projections
    residentHostWeights = [bool]$ResidentHostWeights; batchFourOperations = [bool]$BatchFourOperations; batchTenOperations = [bool]$BatchTenOperations
} } | ConvertTo-Json -Depth 8 -Compress | & $tool hexagon-tools --request -
exit $LASTEXITCODE
