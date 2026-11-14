#Requires -Version 7.6
# Entry adapter only. Native Rust owns admission, clocks, power, capture and retention.
[CmdletBinding(DefaultParameterSetName = 'Capture')]
param(
    [Parameter(Mandatory)][string]$StartReceipt,
    [Parameter(Mandatory, ParameterSetName = 'Create')][switch]$CreateOwnerStartReceipt,
    [Parameter(Mandatory, ParameterSetName = 'Create')][string]$RunId,
    [Parameter(Mandatory, ParameterSetName = 'Create')][string]$RunnerCommit,
    [Parameter(Mandatory, ParameterSetName = 'Create')]
    [Parameter(Mandatory, ParameterSetName = 'Capture')][string]$CandidateCommit,
    [Parameter(Mandatory, ParameterSetName = 'Finalize')][switch]$FinalizeBudget,
    [Parameter(Mandatory, ParameterSetName = 'Capture')][string]$PacketDirectory,
    [Parameter(Mandatory, ParameterSetName = 'Capture')][string]$ExpectedPacketSha256,
    [Parameter(Mandatory, ParameterSetName = 'Capture')][string]$ToolingCommit,
    [Parameter(ParameterSetName = 'Capture')][string]$EvidenceDirectory,
    [string]$ToolsPath = $env:BONSAI_TOOLS_PATH
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
if (-not $ToolsPath) { $ToolsPath = (Get-Command bonsai-tools -CommandType Application -ErrorAction Stop).Source }
$request = @{ startReceipt = $StartReceipt }
switch ($PSCmdlet.ParameterSetName) {
    Create { $request.operation = 'bundle-owner-start'; $request.runId = $RunId; $request.runnerCommit = $RunnerCommit; $request.candidateCommit = $CandidateCommit }
    Finalize { $request.operation = 'bundle-owner-finalize' }
    Capture {
        $request.operation = 'bundle-capture'; $request.candidateCommit = $CandidateCommit
        $request.packetDirectory = $PacketDirectory; $request.expectedPacketSha256 = $ExpectedPacketSha256
        $request.toolingCommit = $ToolingCommit; $request.evidenceDirectory = $EvidenceDirectory
    }
}
& $ToolsPath cpu-evaluation --json (ConvertTo-Json -InputObject $request -Depth 5 -Compress)
exit $LASTEXITCODE