#requires -Version 7.6
[CmdletBinding()]
param([string]$Test)
. (Join-Path $PSScriptRoot 'cpu_bundle.ps1')
function SaveJson([string]$Path,$Value) { [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($Path)); [IO.File]::WriteAllText($Path,(ConvertTo-Json -InputObject $Value -Depth 70),[Text.UTF8Encoding]::new($false)) }
function Clone($Value) { ConvertFrom-Json -InputObject (ConvertTo-Json -InputObject $Value -Depth 70) -AsHashtable -NoEnumerate }
function Reject([scriptblock]$Action) { $caught=$false; try { & $Action | Out-Null } catch { $caught=$true }; Require $caught 'Expected rejection.' }
function NewFixture([string]$Root) {
    $packetPath=Join-Path $Root 'packet'; [void][IO.Directory]::CreateDirectory($packetPath)
    $power=@{status='observed';source='GetSystemPowerStatus';ac_online=$true;battery_saver=$false;battery_percent=90}
    $hostValue=@{status='observed';source='HKLM/HARDWARE/DESCRIPTION/System/CentralProcessor/0;RtlGetVersion;IsProcessorFeaturePresent';processor_name='Snapdragon(R) X2 Elite Extreme - X2E94100 - Qualcomm Oryon(TM) CPU';processor_identifier='ARMv8 (64-bit)';vendor_identifier='Qualcomm Technologies Inc';windows_version=@(10,0,26300);processor_features=@(19,43,66)}
    $native=@{schema_version=1;kind='runner-native-cpu-observations';status='observed';architecture='aarch64';scope='current-process';source='GetSystemCpuSetInformation';host_identity=$hostValue;topology=@{logical_processors=18;cpu_sets=@(@{id=1})};power_snapshot=$power;power_policy_snapshot=@{status='observed';notification_version=2;source='PowerGetActiveScheme/PowerRegisterForEffectivePowerModeNotifications';active_scheme='381b4222-f694-41f0-9685-ff5bb260df2e';effective_mode='balanced'}}
    [IO.File]::WriteAllText((Join-Path $packetPath 'messages.json'),(ConvertTo-Json -InputObject $script:Messages -Compress))
    [IO.File]::WriteAllText((Join-Path $packetPath 'evaluator.ps1'),"# synthetic retained evaluator fixture`n")
    $packet=@{schema_version=2;kind='prism-cpu-bundle-preparation';target=$script:Target;features=$script:Features;schedule=$script:Schedule;profile=$script:Profile;model_executed=$false;promotion_qualified=$false
        retained_files=@('preparation.json','messages.json','evaluator.ps1');evaluator_language='PowerShell 7.6';evaluator_sha256=(FileDigest (Join-Path $packetPath 'evaluator.ps1'))
        collector_sources=@{'examples/cpu_bundle_check.rs'=('1'*64);'examples/cpu_bundle_check/tests.rs'=('2'*64)};messages=@{path='messages.json';sha256=(FileDigest (Join-Path $packetPath 'messages.json'))}
        model_sha256=$script:Model;adapter_sha256=$script:Adapter;per_arm_timeout_seconds=60;output_tokens=32;minimum_relative_request_improvement=0.10;maximum_owned_arm_regression=0.10;arms=@{};registry_packages=@{}}
    foreach ($name in $script:RegistryPackages.Keys) { $pin=$script:RegistryPackages[$name];$packet.registry_packages[$name]=@{name=$name;version=$pin[0];sha256=$pin[1];revision=$script:RegistryRevision} }
    foreach ($role in @('baseline','candidate')) {
        $checks=@(foreach ($phase in $script:Phases) { $check=@{phase=$phase;exit_code=0;command=@('synthetic-fixture')};if ($script:Inventories.ContainsKey($phase)) { $names=@($script:Inventories[$phase] | Sort-Object);$check.test_summary=@{passed=$names.Count;failed=0;ignored=0;measured=0;names=$names} };if ($phase -ceq 'source_analysis') {$check.helper_checks=@(foreach ($package in @('bonsai-neon-dot','bonsai-native-hash')) { @{package=$package;exit_code=0;command=@('synthetic-helper-fixture');manifest_sha256=('1'*64);lock_sha256=('2'*64)} })};$check })
        $source=if ($role -ceq 'baseline') { @{revision=$script:Baseline;clean=$true} } else { @{revision=$script:RegistryRevision;clean=$true;kind='registry';version='0.1.25';archive_sha256=$script:RegistryPackages['prism-inference'][1]} }
        $packet.arms[$role]=@{source=$source;legacy_f16_build=($role -ceq 'baseline');executable=@{sha256=$(if ($role -ceq 'baseline') {'b'*64} else {'c'*64})};checks=$checks}
    }
    $proof=@{schema_version=1;kind='prism-cpu-bundle-owner';workload_predeclared=$true;completed=$true;budget_seconds=600;clock_frequency=1000;started_tick=100000;deadline_tick=700000;retention_completed_tick=300000;original_deadline_unix_milliseconds=2000000000000L
        model_sha256=$script:Model;adapter_sha256=$script:Adapter;host_identity=$hostValue;power_before=(Clone $power);power_after=(Clone $power);arms=@()}
    $receipts=@();$supervisions=@();$starts=@()
    for ($i=0;$i -lt 4;$i++) {
        $role=$script:Schedule[$i];$candidate=$role -ceq 'candidate';$begin=110000+$i*20000;$end=$begin+$(if ($candidate){1000}else{1500});$execution=$begin+60000;$unix=$proof.original_deadline_unix_milliseconds-(700000-$execution);$text='1, 2, 3, 4, 5, 6, 7, 8, 9, 10'
        $receipt=$script:Profile.Clone();$extra=@{schema_version=1;kind='swift-cpu-bundle-arm';source_commit_declaration=$packet.arms[$role].source.revision;executable_sha256=$packet.arms[$role].executable.sha256;input_sha256=$packet.messages.sha256;legacy_f16_build=($role -ceq 'baseline');output_tokens_reserved=32;original_deadline_unix_milliseconds=$unix
            capture_status='complete';arm_succeeded=$true;session_succeeded=$true;terminal_delivered=$true;stage='complete';terminal_accounting_matches=$true;reused_tokens=0;model_sha256=$script:Model;adapter_sha256=$script:Adapter;model_bytes=7206168928L;weight_representation='PQ2_0';effective_row_workers=4;observed_adapter_sha256=$script:Adapter;model_loaded=$true;output_committed=$true;receipt_reserve_milliseconds=2000;sampler='production SwiftSampler and model-declared stops';prompt_tokens=20;generated_tokens=29;finish='stop';output_text=$text;output_bytes=[Text.Encoding]::UTF8.GetByteCount($text);output_sha256=(Digest ([Text.Encoding]::UTF8.GetBytes($text)))
            load_milliseconds=$(if ($candidate){500}else{800});preparation_milliseconds=10;request_milliseconds=$(if ($candidate){300}else{500});elapsed_milliseconds=$(if ($candidate){850}else{1350})}
        foreach ($key in $extra.Keys) {$receipt[$key]=$extra[$key]};$receipts+=,$receipt;$starts+=,@{source_commit_declaration=$receipt.source_commit_declaration;original_deadline_unix_milliseconds=$unix}
        $supervisions+=,@{Status='completed';ExitCode=0;ModelProcessStarted=$true;WallMilliseconds=$(if ($candidate){900}else{1400});MinimumAvailablePhysicalBytes=8589934592L;PeakJobMemoryBytes=2147483648L;CpuConditions=@{Before=(Clone $native);After=(Clone $native)}}
        $proof.arms+=,@{id="arm-$($i+1)";role=$role;started_tick=$begin;cleanup_completed_tick=$end;execution_deadline_tick=$execution;collector_deadline_unix_milliseconds=$unix;host_identity=(Clone $hostValue);power_before=(Clone $power);power_after=(Clone $power);owned_job_empty=$true;cleanup_succeeded=$true}
    }
    @{Root=$Root;Packet=$packet;Proof=$proof;Receipts=$receipts;Supervisions=$supervisions;Starts=$starts;PreparationPath=(Join-Path $packetPath 'preparation.json');OwnerPath=(Join-Path $Root 'owner.json')}
}
function SaveFixture($Fixture) {
    SaveJson $Fixture.PreparationPath $Fixture.Packet;$Fixture.Proof.preparation_sha256=FileDigest $Fixture.PreparationPath
    for ($i=0;$i -lt 4;$i++) {
        foreach ($spec in @(@('receipt',$Fixture.Receipts[$i]),@('started_receipt',$Fixture.Starts[$i]),@('supervision',$Fixture.Supervisions[$i]))) {
            $relative="arm-$($i+1)/$($spec[0]).json";$path=Join-Path $Fixture.Root $relative;SaveJson $path $spec[1];$Fixture.Proof.arms[$i][$spec[0]]=@{path=$relative;sha256=(FileDigest $path)}
        }
    }
    SaveJson $Fixture.OwnerPath $Fixture.Proof
}
function EvaluateFixture($Fixture) { SaveFixture $Fixture;Evaluate $Fixture.PreparationPath $Fixture.OwnerPath }
$tests=[ordered]@{}
$tests.admitted={param($f) $r=EvaluateFixture $f;Require ($r.status -ceq 'admitted' -and $r.short_request_benefit_observed -and -not $r.promotion_qualified -and -not $r.benefit_gate_qualified -and -not $r.DefaultMayChange -and $r.pairs -eq 2) 'Admitted scope changed.'}
$tests.losing={param($f) foreach ($i in @(1,2)) {$f.Supervisions[$i].WallMilliseconds=1500;$f.Proof.arms[$i].cleanup_completed_tick=$f.Proof.arms[$i].started_tick+1600};$r=EvaluateFixture $f;Require (-not $r.short_request_benefit_observed -and -not $r.promotion_qualified) 'Losing outcome promoted.'}
$tests.telemetry={param($f) foreach ($r in $f.Receipts) {$r.first_text_delta_milliseconds=100;$r.generation_phases=@(@{phase='decode';milliseconds=100;attempted_calls=1;completed_calls=1;attempted_tokens=1;completed_tokens_lower_bound=1;partial_progress_unknown=$false})};$r=EvaluateFixture $f;Require ($r.observations[1].first_text_delta_milliseconds -eq 100 -and $r.observations[1].source_request_tps_scope.Contains('not post-first-token decode TPS')) 'Telemetry scope changed.'}
$tests.callback={param($f) $f.Receipts[1].first_text_delta_milliseconds=301;Reject {EvaluateFixture $f}}
$tests.preparation={param($f) foreach ($field in @('phase','exit_code')) {$old=$f.Packet.arms.candidate.checks[0][$field];$f.Packet.arms.candidate.checks[0][$field]=$(if ($field -eq 'phase'){'unknown'}else{1});Reject {EvaluateFixture $f};$f.Packet.arms.candidate.checks[0][$field]=$old}}
$tests.summary={param($f) $name='tests::example';$text="test $name ... ok`ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 3 filtered out; finished in 0.01s";[void](TestSummary $text @($name));foreach ($bad in @($text.Replace('1 passed','0 passed'),$text.Replace($name,'tests::wrong'),($text+"`n"+$text))) {Reject {TestSummary $bad @($name)}}}
$tests.zero_controls={param($f) $f.Packet.arms.candidate.checks[2].test_summary.passed=0;Reject {EvaluateFixture $f}}
$tests.identity={param($f) foreach ($change in @(@('model_sha256',('0'*64)),@('backend_requested','Auto'),@('kv_cache','rsq4'),@('row_workers',1),@('row_workers',4.0))) {$r=$f.Receipts[1];$old=$r[$change[0]];$r[$change[0]]=$change[1];Reject {EvaluateFixture $f};$r[$change[0]]=$old}}
$tests.terminal={param($f) foreach ($change in @(@('output_text',''),@('generated_tokens',0),@('terminal_delivered',$false),@('arm_succeeded',$false))) {$r=$f.Receipts[1];$old=$r[$change[0]];$r[$change[0]]=$change[1];Reject {EvaluateFixture $f};$r[$change[0]]=$old}}
$tests.output_mismatch={param($f) $f.Receipts[1].generated_tokens=28;Reject {EvaluateFixture $f}}
$tests.length={param($f) foreach ($r in $f.Receipts){$r.finish='length'};$r=EvaluateFixture $f;Require (-not $r.complete_answer_observed) 'Length claimed complete.'}
$tests.power={param($f) foreach ($value in @($null,$false)) {$f.Proof.arms[1].power_after.ac_online=$value;Reject {EvaluateFixture $f}}}
$tests.scheme={param($f) $f.Supervisions[1].CpuConditions.After.power_policy_snapshot.effective_mode='high-performance';Reject {EvaluateFixture $f}}
$tests.host={param($f) $f.Supervisions[1].CpuConditions.After.host_identity.windows_version[2]=26301;Reject {EvaluateFixture $f}}
$tests.budget={param($f) $old=$f.Proof.retention_completed_tick;$f.Proof.retention_completed_tick=700001;Reject {EvaluateFixture $f};$f.Proof.retention_completed_tick=$old;$f.Proof.started_tick++;Reject {EvaluateFixture $f}}
$tests.wall={param($f) $f.Supervisions[1].WallMilliseconds=60002;Reject {EvaluateFixture $f}}
$tests.cleanup={param($f) $f.Proof.arms[1].owned_job_empty=$false;Reject {EvaluateFixture $f};$f.Proof.arms[1].owned_job_empty=$true;$f.Proof.arms[1].started_tick=$f.Proof.arms[0].started_tick;Reject {EvaluateFixture $f}}
$tests.resources={param($f) foreach ($change in @(@('MinimumAvailablePhysicalBytes',$null),@('PeakJobMemoryBytes',12348030977L),@('ExitCode',1),@('ExitCode',$false))) {$r=$f.Supervisions[1];$old=$r[$change[0]];$r[$change[0]]=$change[1];Reject {EvaluateFixture $f};$r[$change[0]]=$old}}
$tests.tamper={param($f) SaveFixture $f;[IO.File]::AppendAllText((Join-Path $f.Root 'arm-2/receipt.json'),' ');Reject {Evaluate $f.PreparationPath $f.OwnerPath};Reject {Member $f.Root '../escape'};Reject {Member $f.Root 'arm\receipt.json'}}
$tests.json={param($f) $path=Join-Path $f.Root 'duplicate.json';foreach ($raw in @('{"a":1,"a":2}','{"a":NaN}','{"a":{"x":1,"x":2}}')) {[IO.File]::WriteAllText($path,$raw);Reject {ReadJson $path}}}
$tests.local={param($f) $old=$env:GITHUB_ACTIONS;try {$env:GITHUB_ACTIONS='false';Reject {Prepare '' '' '0.1.25' '' ''}}finally{$env:GITHUB_ACTIONS=$old}}
$tests.lock={param($f) $source=Join-Path $f.Root 'source.lock';$driver=Join-Path $f.Root 'driver.lock';[IO.File]::WriteAllText($source,"[[package]]`nname = `"example`"`nversion = `"1.0.0`"`n");[IO.File]::WriteAllText($driver,"[[package]]`nname = `"example`"`nversion = `"1.0.1`"`n");Reject {LockSubset $driver $source $false}}
$tests.registry={param($f) $f.Packet.registry_packages['bonsai-neon-dot'].sha256='0'*64;Reject {EvaluateFixture $f}}
$tests.archive={param($f)
    $cache=Join-Path $f.Root 'registry/cache/index';$source=Join-Path $f.Root 'registry/src/index/example-1.0.0';[void][IO.Directory]::CreateDirectory($cache);[void][IO.Directory]::CreateDirectory($source)
    $archive=Join-Path $cache 'example-1.0.0.crate';$expected=[Text.Encoding]::UTF8.GetBytes('reviewed source');[IO.File]::WriteAllBytes((Join-Path $source 'lib.rs'),$expected)
    $file=[IO.File]::Open($archive,[IO.FileMode]::CreateNew);$gzip=[IO.Compression.GZipStream]::new($file,[IO.Compression.CompressionMode]::Compress);$writer=[System.Formats.Tar.TarWriter]::new($gzip)
    try {$entry=[System.Formats.Tar.PaxTarEntry]::new([System.Formats.Tar.TarEntryType]::RegularFile,'example-1.0.0/lib.rs');$entry.DataStream=[IO.MemoryStream]::new($expected);$writer.WriteEntry($entry);$entry.DataStream.Dispose()}finally{$writer.Dispose();$gzip.Dispose();$file.Dispose()}
    $digest=FileDigest $archive;VerifyRegistryTree $source $digest
    [IO.File]::WriteAllText((Join-Path $source 'extra.rs'),'unreviewed source');Reject {VerifyRegistryTree $source $digest};[IO.File]::Delete((Join-Path $source 'extra.rs'))
    [IO.File]::WriteAllText((Join-Path $source 'lib.rs'),'changed source');Reject {VerifyRegistryTree $source $digest}
}
$tests.archive_traversal={param($f)
    $cache=Join-Path $f.Root 'registry/cache/index';$source=Join-Path $f.Root 'registry/src/index/example-1.0.0';[void][IO.Directory]::CreateDirectory($cache);[void][IO.Directory]::CreateDirectory($source)
    $archive=Join-Path $cache 'example-1.0.0.crate';$file=[IO.File]::Open($archive,[IO.FileMode]::CreateNew);$gzip=[IO.Compression.GZipStream]::new($file,[IO.Compression.CompressionMode]::Compress);$writer=[System.Formats.Tar.TarWriter]::new($gzip)
    try {$entry=[System.Formats.Tar.PaxTarEntry]::new([System.Formats.Tar.TarEntryType]::RegularFile,'example-1.0.0/../escape');$entry.DataStream=[IO.MemoryStream]::new([byte[]]@(1));$writer.WriteEntry($entry);$entry.DataStream.Dispose()}finally{$writer.Dispose();$gzip.Dispose();$file.Dispose()}
    Reject {VerifyRegistryTree $source (FileDigest $archive)};Require (-not (Test-Path -LiteralPath (Join-Path $f.Root 'registry/src/index/escape'))) 'Archive extraction occurred.'
}
$scratch=Join-Path ([IO.Path]::GetTempPath()) ('cpu-policy-'+[guid]::NewGuid().ToString('N'));[void][IO.Directory]::CreateDirectory($scratch)
try {
    $selected=if ($Test) {Require ($tests.Contains($Test)) "Unknown test $Test";@($Test)}else{@($tests.Keys)}
    foreach ($name in $selected) {$fixture=NewFixture (Join-Path $scratch $name);& $tests[$name] $fixture;Write-Output "PASS $name"}
} finally {[IO.Directory]::Delete($scratch,$true)}
