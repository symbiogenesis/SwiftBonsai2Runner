#Requires -Version 7.6
# Hosted integrity replay of one historical packet. Historical source is data only.
[CmdletBinding()]
param([string]$Comparator='', [string]$Output='', [string]$ToolPath='')
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

function Assert-RetainedProducer($Run,$Artifact,$Jobs) {
    Assert-Reference ($Run.id -eq $script:RetainedRun -and $Run.head_sha -ceq $script:RetainedRevision -and $Run.head_branch -ceq 'main' -and $Run.event -ceq 'workflow_dispatch' -and $Run.run_attempt -eq 1 -and $Run.path -ceq $script:RetainedWorkflow -and $Run.status -ceq 'completed' -and $Run.conclusion -ceq 'success' -and $Run.repository.full_name -ceq $script:RetainedRepository) 'inexact original producer'
    Assert-Reference ($Artifact.id -eq $script:RetainedArtifact -and $Artifact.name -ceq $script:RetainedArtifactName -and $Artifact.size_in_bytes -eq $script:RetainedArchiveBytes -and $Artifact.expired -is [bool] -and -not $Artifact.expired -and $Artifact.digest -ceq ('sha256:'+$script:RetainedArchiveSha) -and $Artifact.workflow_run.id -eq $script:RetainedRun -and $Artifact.workflow_run.head_sha -ceq $script:RetainedRevision -and $Artifact.workflow_run.head_branch -ceq 'main' -and $Artifact.workflow_run.repository_id -eq $Run.repository.id) 'inexact original artifact'
    Assert-Reference ($Jobs.total_count -eq 2 -and $Jobs.jobs.Count -eq 2 -and (Test-ReferenceEqual @($Jobs.jobs.name|Sort-Object) @('numerics','prepare'))) 'original job inventory differs'
    foreach($job in $Jobs.jobs){Assert-Reference ($job.status -ceq 'completed' -and $job.conclusion -ceq 'success' -and $job.run_id -eq $script:RetainedRun -and $job.head_sha -ceq $script:RetainedRevision) 'original jobs did not complete successfully';if($job.name -ceq 'numerics'){Assert-Reference (Test-ReferenceEqual $job.labels @("ptq1-numerics-$script:RetainedRun")) 'inexact original owner label'}}
}
function Assert-RetainedRedirect([uri]$Target){Assert-Reference ($Target.Scheme -ceq 'https' -and -not $Target.UserInfo -and ($Target.Host.EndsWith('.blob.core.windows.net',[StringComparison]::OrdinalIgnoreCase) -or $Target.Host.EndsWith('.githubusercontent.com',[StringComparison]::OrdinalIgnoreCase))) 'unapproved artifact redirect'}
function Receive-RetainedEndpoint([string]$Endpoint,[string]$Token,[string]$Destination='') {
    $handler=[Net.Http.HttpClientHandler]::new();$handler.AllowAutoRedirect=$false;$client=[Net.Http.HttpClient]::new($handler);$client.Timeout=[TimeSpan]::FromSeconds(120)
    $request=[Net.Http.HttpRequestMessage]::new([Net.Http.HttpMethod]::Get,"https://api.github.com/repos/$script:RetainedRepository/$Endpoint")
    $request.Headers.Add('Authorization','Bearer '+$Token);$request.Headers.Add('Accept','application/vnd.github+json');$request.Headers.Add('X-GitHub-Api-Version','2022-11-28');$request.Headers.Add('User-Agent','bonsai-retained-reference')
    $response=$null;$input=$null;$output=$null
    try {
        $response=$client.Send($request,[Net.Http.HttpCompletionOption]::ResponseHeadersRead)
        if([int]$response.StatusCode -in 301,302,303,307,308){
            Assert-Reference ([bool]$Destination) 'unexpected metadata redirect'
            $target=$response.Headers.Location;Assert-RetainedRedirect $target
            $response.Dispose();$request.Dispose();$request=[Net.Http.HttpRequestMessage]::new([Net.Http.HttpMethod]::Get,$target)
            # New request contains no token. No automatic redirect is enabled.
            $response=$client.Send($request,[Net.Http.HttpCompletionOption]::ResponseHeadersRead)
        }
        Assert-Reference ([int]$response.StatusCode -eq 200) 'retained endpoint response differs'
        $input=$response.Content.ReadAsStream();$output=if($Destination){[IO.File]::Open($Destination,[IO.FileMode]::CreateNew)}else{[IO.MemoryStream]::new()}
        $maximum=if($Destination){$script:RetainedArchiveBytes}else{4194304L};$total=0L;$buffer=[byte[]]::new(1048576)
        $cancel=[Threading.CancellationTokenSource]::new([TimeSpan]::FromSeconds(120))
        try{while(($count=$input.ReadAsync($buffer,0,$buffer.Length,$cancel.Token).GetAwaiter().GetResult()) -gt 0){$total+=$count;Assert-Reference ($total -le $maximum) 'retained response exceeds byte bound';$output.Write($buffer,0,$count)}}finally{$cancel.Dispose()}
        if($Destination){$output.Flush();Assert-Reference ($total -eq $maximum) 'archive byte length differs';$output.Dispose();$output=$null;Assert-Reference ((Get-ReferenceFileDigest $Destination) -ceq $script:RetainedArchiveSha) 'archive raw hash differs'}else{return ConvertFrom-ReferenceJson $output.ToArray()}
    }finally{if($output){$output.Dispose()};if($input){$input.Dispose()};if($response){$response.Dispose()};$request.Dispose();$client.Dispose();$handler.Dispose()}
}
function Get-RetainedInventory([string]$Path) {
    $inventory=Read-ReferenceJson $Path
    Assert-Reference ($inventory.sourceRunId -ceq [string]$script:RetainedRun -and $inventory.sourceRevision -ceq $script:RetainedRevision -and $inventory.files.Count -eq 155 -and ($inventory.files|Measure-Object bytes -Sum).Sum -eq 316267936) 'expected inventory producer/extent differs'
    $expected=[Collections.Generic.Dictionary[string,object]]::new([StringComparer]::Ordinal)
    foreach($file in $inventory.files){Assert-Reference (-not $expected.ContainsKey($file.path)) 'duplicate expected inventory member';$expected.Add($file.path,$file)}
    return ,$expected
}
function Get-RetainedZipMetadata([string]$Archive) {
    # Inspect central records to enforce encryption and method policy before .NET
    # decompresses any member. This packet is below ZIP64's size/count boundary.
    $stream=[IO.File]::OpenRead($Archive);$reader=[IO.BinaryReader]::new($stream)
    try {
        $length=[int][Math]::Min($stream.Length,65557);$stream.Position=$stream.Length-$length;$tail=$reader.ReadBytes($length);$end=-1
        for($i=$tail.Length-22;$i -ge 0;$i--){if([BitConverter]::ToUInt32($tail,$i) -eq 0x06054b50 -and $i+22+[BitConverter]::ToUInt16($tail,$i+20) -eq $tail.Length){$end=$i;break}}
        Assert-Reference ($end -ge 0 -and [BitConverter]::ToUInt16($tail,$end+4) -eq 0 -and [BitConverter]::ToUInt16($tail,$end+6) -eq 0) 'unsupported ZIP directory'
        $count=[BitConverter]::ToUInt16($tail,$end+10);Assert-Reference ($count -eq [BitConverter]::ToUInt16($tail,$end+8) -and $count -lt 65535) 'unsupported ZIP member count'
        $directoryBytes=[BitConverter]::ToUInt32($tail,$end+12);$directoryOffset=[BitConverter]::ToUInt32($tail,$end+16)
        Assert-Reference ($directoryOffset+$directoryBytes -eq $stream.Length-$length+$end) 'ZIP directory extent differs'
        $stream.Position=$directoryOffset;$records=[Collections.Generic.List[object]]::new()
        for($i=0;$i -lt $count;$i++){
            $header=$reader.ReadBytes(46);Assert-Reference ($header.Length -eq 46 -and [BitConverter]::ToUInt32($header,0) -eq 0x02014b50) 'ZIP central record differs'
            $flags=[BitConverter]::ToUInt16($header,8);$method=[BitConverter]::ToUInt16($header,10);$size=[BitConverter]::ToUInt32($header,24)
            Assert-Reference (($flags -band 1) -eq 0 -and $method -in 0,8 -and $size -le 536870912) 'invalid ZIP encryption/method/extent'
            $nameBytes=$reader.ReadBytes([BitConverter]::ToUInt16($header,28));$name=[Text.UTF8Encoding]::new($false,$true).GetString($nameBytes)
            $skip=[BitConverter]::ToUInt16($header,30)+[BitConverter]::ToUInt16($header,32);$stream.Position+=$skip
            $records.Add(@{name=$name;size=$size})
        }
        Assert-Reference ($stream.Position -eq $directoryOffset+$directoryBytes) 'ZIP central directory trailing bytes'
        return ,$records.ToArray()
    }finally{$reader.Dispose();$stream.Dispose()}
}
function Expand-RetainedChecked([string]$Archive,[string]$Destination,$Expected) {
    $metadata=Get-RetainedZipMetadata $Archive
    Assert-Reference ($metadata.Count -eq $Expected.Count) 'archive member count differs'
    $zip=[IO.Compression.ZipFile]::OpenRead($Archive);$seen=[Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase);$manifest=[Collections.Generic.List[object]]::new()
    try {
        Assert-Reference ($zip.Entries.Count -eq $Expected.Count) 'archive member count differs'
        for($i=0;$i -lt $zip.Entries.Count;$i++) {
            $member=$zip.Entries[$i];$name=$member.FullName
            Assert-Reference ($name -ceq $metadata[$i].name -and $member.Length -eq $metadata[$i].size -and $name.Length -le 200 -and $name -cmatch '^[^/:\\]+(?:/[^/:\\]+)*$' -and -not @($name.Split('/')|Where-Object{$_ -cin '.','..'}).Count -and $Expected.ContainsKey($name) -and $seen.Add($name)) 'unsafe unexpected or duplicate archive member'
            $mode=($member.ExternalAttributes -shr 16) -band 0xf000
            Assert-Reference ($mode -in 0,0x8000 -and ($member.ExternalAttributes -band 0x10) -eq 0 -and $member.Length -eq $Expected[$name].bytes -and $member.Length -le 536870912) 'invalid archive member type or extent'
            $target=Join-Path $Destination $name;[void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($target))
            $input=$member.Open();$output=[IO.File]::Open($target,[IO.FileMode]::CreateNew);$hash=[Security.Cryptography.IncrementalHash]::CreateHash([Security.Cryptography.HashAlgorithmName]::SHA256)
            try{$buffer=[byte[]]::new(1048576);$total=0L;while(($count=$input.Read($buffer,0,$buffer.Length)) -gt 0){$total+=$count;Assert-Reference ($total -le $member.Length) 'expanded member exceeds declared extent';$hash.AppendData($buffer,0,$count);$output.Write($buffer,0,$count)};$sha=[Convert]::ToHexStringLower($hash.GetHashAndReset());Assert-Reference ($total -eq $member.Length -and $sha -ceq $Expected[$name].sha256) 'retained member hash differs';$manifest.Add(@{path=$name;bytes=$total;sha256=$sha})}finally{$input.Dispose();$output.Dispose();$hash.Dispose()}
        }
        Assert-Reference ($seen.Count -eq $Expected.Count) 'missing retained member'
        return ,@($manifest|Sort-Object path)
    }finally{$zip.Dispose()}
}
function Invoke-RetainedReplay([string]$Root,[string]$Comparator,[string]$ToolPath='') {
    # Preserve the original source's digest as immutable evidence, never execute it.
    Assert-Reference ((Get-ReferenceFileDigest $Comparator) -ceq $script:RetainedComparatorSha) 'original comparator source differs'
    $original=Read-ReferenceJson (Join-Path $Root 'comparison.json');$owner=Read-ReferenceJson (Join-Path $Root 'numerics.json')
    Assert-Reference ($owner.Revision -ceq $script:RetainedRevision -and [string]$owner.RunAttempt -ceq '1' -and $owner.RunUrl.EndsWith("/actions/runs/$script:RetainedRun",[StringComparison]::Ordinal) -and $owner.Status -ceq 'completed-one-token-smoke' -and $owner.Phase -ceq 'completed' -and (Test-ReferenceEqual $owner.Comparison $original) -and $owner.DefaultMayChange -is [bool] -and -not $owner.DefaultMayChange -and $owner.Plan.RustSource.commit -ceq $script:RetainedRuntime -and $owner.Plan.RustBackend -ceq 'neon' -and $owner.Plan.RustRowWorkers -eq 4 -and $owner.Plan.AdapterCondition -ceq 'standard' -and $owner.Plan.AdapterScale -eq 1.3) 'retained owner/comparison binding differs'
    Assert-Reference ($original.status -ceq 'passed-bounded-one-token-smoke' -and $original.files -eq 129 -and $original.elements -eq 39504384 -and $original.byteExactFiles -eq 129 -and $original.failedElements -eq 0 -and $original.defaultMayChange -is [bool] -and -not $original.defaultMayChange -and $original.backendRequested -ceq 'neon' -and $original.rustRowWorkers -eq 4 -and $original.adapterCondition -ceq 'standard' -and $original.adapterScale -eq 1.3) 'retained selected comparison differs'
    $recomputed=Invoke-NativeReferenceComparison (Join-Path $Root 'native') (Join-Path $Root 'neon') 'neon' 4 'standard' $ToolPath
    Assert-Reference (Test-ReferenceEqual $recomputed $original) 'replayed comparison differs from original report'
    return $recomputed
}
function Invoke-RetainedAudit([string]$Comparator,[string]$Output,[string]$ToolPath='') {
    Assert-Reference (-not [IO.Directory]::Exists($Output)) 'retained audit output must be fresh';[void][IO.Directory]::CreateDirectory($Output)
    $audit=[ordered]@{schemaVersion=2;status='failed';stage='hosted-guard';sourceRunId=[string]$script:RetainedRun;sourceRevision=$script:RetainedRevision;artifactId=[string]$script:RetainedArtifact;artifactSha256=$script:RetainedArchiveSha;comparatorSourceSha256=$script:RetainedComparatorSha;replayImplementationSha256=(Get-ReferenceFileDigest (Join-Path $PSScriptRoot 'compare.ps1'));originalComparatorExecuted=$false;auditRevision=$env:GITHUB_SHA;auditRunId=$env:GITHUB_RUN_ID;ownerDeviceExecutedByAudit=$false;modelExecutedByAudit=$false;defaultMayChange=$false}
    $scratch=$null
    try {
        Assert-Reference ($env:GITHUB_ACTIONS -ceq 'true' -and $env:GITHUB_REPOSITORY -ceq $script:RetainedRepository -and $env:GITHUB_REF -ceq 'refs/heads/main' -and $env:RUNNER_ENVIRONMENT -ceq 'github-hosted') 'hosted main only'
        Assert-Reference ([bool]$env:GH_TOKEN) 'artifact credential unavailable'
        $audit.stage='producer-metadata';$run=Receive-RetainedEndpoint "actions/runs/$script:RetainedRun" $env:GH_TOKEN;$artifact=Receive-RetainedEndpoint "actions/artifacts/$script:RetainedArtifact" $env:GH_TOKEN;$jobs=Receive-RetainedEndpoint "actions/runs/$script:RetainedRun/jobs?filter=all&per_page=100" $env:GH_TOKEN
        Assert-RetainedProducer $run $artifact $jobs;Write-ReferenceJson (Join-Path $Output 'producer.json') @{run=$run;artifact=$artifact;jobs=$jobs}
        $scratch=Join-Path $env:RUNNER_TEMP ('ptq1-retained-'+[guid]::NewGuid().ToString('N'));[void][IO.Directory]::CreateDirectory($scratch)
        $archive=Join-Path $scratch 'evidence.zip';$extracted=Join-Path $scratch 'extracted';$audit.stage='archive-download';Receive-RetainedEndpoint "actions/artifacts/$script:RetainedArtifact/zip" $env:GH_TOKEN $archive
        $audit.stage='retained-member-integrity';$manifest=Expand-RetainedChecked $archive $extracted (Get-RetainedInventory (Join-Path $PSScriptRoot "fixtures/retained-$script:RetainedRun.inventory.json"))
        Write-ReferenceJson (Join-Path $Output 'manifest.json') @{sourceRunId=[string]$script:RetainedRun;sourceRevision=$script:RetainedRevision;files=$manifest}
        $audit.stage='replacement-comparator-replay';$comparison=Invoke-RetainedReplay $extracted $Comparator $ToolPath
        [IO.File]::Copy((Join-Path $extracted 'comparison.json'),(Join-Path $Output 'original-comparison.json'));Write-ReferenceJson (Join-Path $Output 'replayed-comparison.json') $comparison
        $audit.retainedFiles=$manifest.Count;$audit.retainedBytes=($manifest|Measure-Object bytes -Sum).Sum
        foreach($pair in @(@('comparedFiles','files'),@('comparedValues','elements'),@('byteExactFiles','byteExactFiles'),@('modelSha256','modelSha256'),@('adapterSha256','adapterSha256'),@('nativeCaptureSha256','nativeCaptureSha256'),@('nativeStateSha256','nativeStateSha256'),@('rustStdoutSha256','rustStdoutSha256'))){$audit[$pair[0]]=$comparison[$pair[1]]}
        $audit.runtimeRevision=$script:RetainedRuntime;$audit.originalExecutionUnchanged=$true;$audit.fullContextQualified=$false;$audit.scope='Retained byte integrity and replacement-comparator replay only; no new model/device execution, promotion, provenance, quality or full-context acceptance.'
        $audit.status='passed-retained-capture-integrity-and-replay';$audit.stage='complete'
    }catch{$audit.status='failed';$audit.failureClass=$_.Exception.GetType().Name;if($_.Exception -is [IO.InvalidDataException]){$audit.failureReason=$_.Exception.Message}}
    finally{if($scratch -and [IO.Directory]::Exists($scratch)){[IO.Directory]::Delete($scratch,$true)};Write-ReferenceJson (Join-Path $Output 'audit.json') $audit}
    return $audit
}
if($MyInvocation.InvocationName -ne '.'){$audit=Invoke-RetainedAudit $Comparator $Output $ToolPath;Write-Output "Retained evidence audit: $($audit.status); stage: $($audit.stage)";if(-not $audit.status.StartsWith('passed-')){exit 1}}
