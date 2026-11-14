#Requires -Version 7.6
[CmdletBinding()]
param([string]$Comparator='',[string]$Output='',[string]$ToolPath='')
. (Join-Path $PSScriptRoot 'compare.ps1') -ToolPath $ToolPath -Output $Output
$script:RetainedRepository='symbiogenesis/SwiftBonsai2Runner'
$script:RetainedRevision='469f57b233898fd10b97d5ebc10aaef78b940e30'
$script:RetainedRuntime='cae16fa6998dac58a947eba9ae4398ed6a9fb863'
$script:RetainedRun=41568281287L;$script:RetainedArtifact=13200218351L
$script:RetainedArchiveBytes=294092332L
$script:RetainedArchiveSha='c31de3e837f4d4d43df5d144892c0b5c4b36d747ab763799abe0556b419af8d8'
$script:RetainedComparatorSha='b8372072b3d77a918c5c6fa7038da95f51a50701f6927fb4b4931a591b056c08'
$script:RetainedWorkflow='.github/workflows/native-ptq1-numerics.yml'
$script:RetainedArtifactName="native-ptq1-numerics-$script:RetainedRun-1"
function Assert-RetainedProducer($Run,$Artifact,$Jobs) { [void](Invoke-ReferencePolicy @{operation='retained-producer';run=$Run;artifact=$Artifact;jobs=$Jobs}) }
function Assert-RetainedRedirect([uri]$Target) { [void](Invoke-ReferencePolicy @{operation='retained-redirect';url=$Target.OriginalString}) }
function Expand-RetainedChecked([string]$Archive,[string]$Destination,$Expected) { return ,(Invoke-ReferencePolicy @{operation='retained-extract';archive=$Archive;destination=$Destination;expected=$Expected}) }
function Invoke-RetainedReplay([string]$Root,[string]$Comparator,[string]$ToolPath='') { Invoke-ReferencePolicy @{operation='retained-replay';root=$Root;comparator=$Comparator} }
function Invoke-RetainedAudit([string]$Comparator,[string]$Output,[string]$ToolPath='') { Invoke-ReferencePolicy @{operation='retained-audit';comparator=$Comparator;outputDirectory=$Output;repository=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))} }
if ($MyInvocation.InvocationName -ne '.') { $audit=Invoke-RetainedAudit $Comparator $Output $ToolPath;$audit|ConvertTo-Json -Depth 64;if(-not $audit.status.StartsWith('passed-')){exit 1} }
