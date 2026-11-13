#Requires -Version 7.6
[CmdletBinding()]
param([Parameter(ValueFromRemainingArguments)][string[]]$Name=@())
. (Join-Path $PSScriptRoot 'audit_retained.ps1')
function Assert-Rejected([scriptblock]$Action){$rejected=$false;try{& $Action 2>$null|Out-Null}catch{$rejected=$true};Assert-Reference $rejected 'fixture expected rejection'}
function New-ProducerFixture{
    $run=@{id=$script:RetainedRun;head_sha=$script:RetainedRevision;head_branch='main';event='workflow_dispatch';run_attempt=1;path=$script:RetainedWorkflow;status='completed';conclusion='success';repository=@{full_name=$script:RetainedRepository;id=1}}
    $artifact=@{id=$script:RetainedArtifact;name=$script:RetainedArtifactName;size_in_bytes=$script:RetainedArchiveBytes;expired=$false;digest=('sha256:'+$script:RetainedArchiveSha);workflow_run=@{id=$script:RetainedRun;head_sha=$script:RetainedRevision;head_branch='main';repository_id=1}}
    $jobs=@{total_count=2;jobs=@('prepare','numerics'|ForEach-Object{@{name=$_;status='completed';conclusion='success';run_id=$script:RetainedRun;head_sha=$script:RetainedRevision;labels=@("ptq1-numerics-$script:RetainedRun")}})}
    return @($run,$artifact,$jobs)
}
function New-ExpectedRecord([string]$Name='safe.json',[byte[]]$Data=([Text.Encoding]::UTF8.GetBytes('{}'))){return @{path=$Name;bytes=$Data.Length;sha256=(Get-ReferenceDigest $Data)}}
function Invoke-ExtractionFixture($Entries,$Expected){
    $lane=Join-Path $scratch ([guid]::NewGuid().ToString('N'));[void][IO.Directory]::CreateDirectory($lane);$path=Join-Path $lane 'test.zip';$zip=[IO.Compression.ZipFile]::Open($path,[IO.Compression.ZipArchiveMode]::Create)
    try{foreach($item in $Entries){$entry=$zip.CreateEntry($item.name);$entry.ExternalAttributes=[int]($item.mode -shl 16);$output=$entry.Open();try{$output.Write([byte[]]$item.data)}finally{$output.Dispose()}}}finally{$zip.Dispose()}
    Expand-RetainedChecked $path (Join-Path $lane 'output') $Expected
}
function New-ExpectedDictionary($Names){$expected=[Collections.Generic.Dictionary[string,object]]::new([StringComparer]::Ordinal);foreach($name in $Names){$expected.Add($name,(New-ExpectedRecord $name))};return ,$expected}
$scratch=Join-Path ([IO.Path]::GetTempPath()) ('Bonsai-RetainedAudit-Tests-'+[guid]::NewGuid().ToString('N'));[void][IO.Directory]::CreateDirectory($scratch)
$tests=[ordered]@{
    test_inexact_producer_artifact_and_job_reject={
        $values=New-ProducerFixture;Assert-RetainedProducer @values
        foreach($case in @(@(0,'id',1),@(0,'head_sha',('0'*40)),@(0,'head_branch','branch'),@(0,'event','pull_request'),@(0,'run_attempt',2),@(0,'path','different.yml'),@(0,'conclusion','failure'),@(1,'id',1),@(1,'name','different'),@(1,'size_in_bytes',1),@(1,'expired',$true),@(1,'digest',('sha256:'+('0'*64))))){$values=New-ProducerFixture;$values[$case[0]][$case[1]]=$case[2];Assert-Rejected {Assert-RetainedProducer @values}}
        foreach($mode in 'repository','artifact-source','job','label'){$values=New-ProducerFixture;switch($mode){repository{$values[0].repository.full_name='different/repository'}artifact-source{$values[1].workflow_run.head_sha='0'*40}job{$values[2].jobs[1].conclusion='failure'}label{$values[2].jobs[1].labels=@('self-hosted')}};Assert-Rejected {Assert-RetainedProducer @values}}
    }
    test_regular_files_match_exact_manifest={
        $expected=New-ExpectedDictionary @('safe.json','native/state.native-seq');$entries=@($expected.Keys|ForEach-Object{@{name=$_;data=[Text.Encoding]::UTF8.GetBytes('{}');mode=0x81a4}});$observed=Invoke-ExtractionFixture $entries $expected;Assert-Reference ($observed.Count -eq 2 -and $observed[0].path -ceq 'native/state.native-seq') 'exact archive inventory differs'
    }
    test_traversal_symlink_and_duplicate_members_reject={
        foreach($name in '../escape.json','/escape.json','a/../escape.json','a//escape.json','C:/escape.json','a\escape.json'){Assert-Rejected {Invoke-ExtractionFixture @(@{name=$name;data=[Text.Encoding]::UTF8.GetBytes('{}');mode=0x8000}) (New-ExpectedDictionary @($name))}}
        Assert-Rejected {Invoke-ExtractionFixture @(@{name='safe.json';data=[Text.Encoding]::UTF8.GetBytes('{}');mode=0xa1ff}) (New-ExpectedDictionary @('safe.json'))}
        Assert-Rejected {Invoke-ExtractionFixture @(@{name='safe.json';data=[Text.Encoding]::UTF8.GetBytes('{}');mode=0x8000},@{name='safe.json';data=[Text.Encoding]::UTF8.GetBytes('{}');mode=0x8000}) (New-ExpectedDictionary @('safe.json','other.json'))}
    }
    test_missing_unexpected_extent_and_hash_mismatch_reject={
        Assert-Rejected {Invoke-ExtractionFixture @() (New-ExpectedDictionary @('safe.json'))}
        Assert-Rejected {Invoke-ExtractionFixture @(@{name='other.json';data=[Text.Encoding]::UTF8.GetBytes('{}');mode=0x8000}) (New-ExpectedDictionary @('safe.json'))}
        foreach($field in 'bytes','sha256'){$expected=New-ExpectedDictionary @('safe.json');$expected['safe.json'][$field]=if($field -ceq 'bytes'){3}else{'0'*64};Assert-Rejected {Invoke-ExtractionFixture @(@{name='safe.json';data=[Text.Encoding]::UTF8.GetBytes('{}');mode=0x8000}) $expected}}
    }
    test_original_comparator_identity_rejects_before_import={
        $path=Join-Path $scratch 'comparator.source';[IO.File]::WriteAllText($path,'This source must never execute');Assert-Rejected {Invoke-RetainedReplay (Join-Path $scratch 'absent-capture') $path}
    }
    test_redirect_strips_credentials_and_rejects_unapproved_hosts={
        Assert-RetainedRedirect ([uri]'https://test.blob.core.windows.net/evidence?signature=synthetic')
        foreach($target in 'http://test.blob.core.windows.net/evidence','https://example.com/evidence','https://user:password@test.blob.core.windows.net/evidence'){Assert-Rejected {Assert-RetainedRedirect ([uri]$target)}}
        $source=[IO.File]::ReadAllText((Join-Path $PSScriptRoot 'audit_retained.ps1'));Assert-Reference ($source.Contains('AllowAutoRedirect=$false') -and $source.Contains("New request contains no token")) 'explicit credential-free redirect boundary missing'
    }
}
try{$selected=if($Name.Count){@($Name|ForEach-Object{($_ -split '\.')[-1]})}elseif($env:GITHUB_ACTIONS -ceq 'true'){@($tests.Keys)}else{throw 'Select a named focused regression locally; complete tests are CI-only.'};foreach($case in $selected){Assert-Reference ($tests.Contains($case)) 'unknown focused regression';& $tests[$case];Write-Output "PASS $case"}}finally{[IO.Directory]::Delete($scratch,$true)}
