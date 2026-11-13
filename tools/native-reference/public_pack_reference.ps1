#Requires -Version 7.6
[CmdletBinding()]
param([Alias('data-directory')][string]$DataDirectory='',
    [Alias('evidence-directory')][string]$EvidenceDirectory='', [string]$ToolPath='')
. (Join-Path $PSScriptRoot 'public_pack_formats.ps1')
$script:PublicStarted=[Diagnostics.Stopwatch]::StartNew()

function Assert-PublicHosted {
    $expected=@{GITHUB_ACTIONS='true';RUNNER_ENVIRONMENT='github-hosted';RUNNER_OS='Windows';RUNNER_ARCH='ARM64';GITHUB_REPOSITORY='symbiogenesis/SwiftBonsai2Runner';GITHUB_REF='refs/heads/main';GITHUB_EVENT_NAME='workflow_dispatch'}
    foreach($name in $expected.Keys){Assert-Reference ([Environment]::GetEnvironmentVariable($name) -ceq $expected[$name]) 'public reconstruction requires main-only GitHub-hosted Windows ARM64'}
    Assert-Reference ($env:GITHUB_SHA -cmatch '^[0-9a-f]{40}$') 'source revision absent'
}
function Get-PublicRemaining {
    $remaining=2400.0-$script:PublicStarted.Elapsed.TotalSeconds
    Assert-Reference ($remaining -gt 0) 'hosted reference absolute allowance expired'
    return $remaining
}
function Get-PublicStreamDigest([IO.Stream]$Stream) {
    $hash=[Security.Cryptography.IncrementalHash]::CreateHash([Security.Cryptography.HashAlgorithmName]::SHA256)
    try{$buffer=[byte[]]::new(1048576);while(($count=$Stream.Read($buffer,0,$buffer.Length)) -gt 0){[void](Get-PublicRemaining);$hash.AppendData($buffer,0,$count)};return [Convert]::ToHexStringLower($hash.GetHashAndReset())}finally{$hash.Dispose()}
}
function Copy-PublicTransfer([IO.Stream]$Source,[IO.Stream]$Output,$Pin) {
    $hash=[Security.Cryptography.IncrementalHash]::CreateHash([Security.Cryptography.HashAlgorithmName]::SHA256)
    try {
        $buffer=[byte[]]::new(1048576);$count=0L
        while($true) {
            $remaining=Get-PublicRemaining;$cancel=[Threading.CancellationTokenSource]::new([TimeSpan]::FromSeconds([Math]::Min(30,$remaining)))
            try{$received=$Source.ReadAsync($buffer,0,[int][Math]::Min($buffer.Length,$Pin.bytes+1-$count),$cancel.Token).GetAwaiter().GetResult()}finally{$cancel.Dispose()}
            if($received -eq 0){break}
            $count+=$received;Assert-Reference ($count -le $Pin.bytes) 'input exceeds pinned byte length'
            $hash.AppendData($buffer,0,$received);$Output.Write($buffer,0,$received)
        }
        Assert-Reference ($count -eq $Pin.bytes -and [Convert]::ToHexStringLower($hash.GetHashAndReset()) -ceq $Pin.sha256) 'complete input raw-byte identity differs'
    }finally{$hash.Dispose()}
}
function Receive-PublicInput([string]$Directory,$Pin) {
    [void](Get-PublicRemaining)
    Assert-ReferenceKeys $Pin @('filename','bytes','sha256','url')
    Assert-Reference ((Test-ReferenceInteger $Pin.bytes 9000000000) -and $Pin.bytes -gt 0 -and $Pin.filename -cmatch '^[A-Za-z0-9_.-]+$' -and $Pin.filename -cnotin '.','..' -and $Pin.sha256 -cmatch '^[0-9a-f]{64}$' -and ([uri]$Pin.url).Scheme -ceq 'https') 'input identity differs'
    $target=Join-Path $Directory $Pin.filename;$partial=$target+'.partial'
    $handler=[Net.Http.HttpClientHandler]::new();$handler.AutomaticDecompression=[Net.DecompressionMethods]::None
    $client=[Net.Http.HttpClient]::new($handler);$client.Timeout=[TimeSpan]::FromSeconds([Math]::Min(30,(Get-PublicRemaining)))
    $request=[Net.Http.HttpRequestMessage]::new([Net.Http.HttpMethod]::Get,$Pin.url);$request.Headers.Add('Accept-Encoding','identity')
    $response=$null;$source=$null;$output=$null
    try {
        $response=$client.Send($request,[Net.Http.HttpCompletionOption]::ResponseHeadersRead)
        Assert-Reference ([int]$response.StatusCode -eq 200 -and $response.RequestMessage.RequestUri.Scheme -ceq 'https' -and $response.Content.Headers.ContentEncoding.Count -eq 0) 'unsupported transfer response'
        $declared=$response.Content.Headers.ContentLength
        Assert-Reference ($null -eq $declared -or $declared -eq $Pin.bytes) 'declared input length differs'
        $source=$response.Content.ReadAsStream();$output=[IO.File]::Open($partial,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None)
        Copy-PublicTransfer $source $output $Pin
        $output.Flush($true);$output.Dispose();$output=$null
        [IO.File]::Move($partial,$target,$false)
        return $target
    }finally{if($output){$output.Dispose()};if($source){$source.Dispose()};if($response){$response.Dispose()};$request.Dispose();$client.Dispose();$handler.Dispose()}
}
function Get-PublicConfigSites([byte[]]$Raw) {
    $config=ConvertFrom-ReferenceJson $Raw
    Assert-Reference ($config.schema_version -eq 2 -and $config.model_type -ceq 'prism_hadamard_qwen35' -and $config.base_model_type -ceq 'qwen3_5' -and $config.tensor_namespace -ceq 'mlx-vlm-qwen3_5' -and $config.gdn_activation_layout -ceq 'grouped' -and (Test-ReferenceEqual $config.quantization @{bits=2;group_size=128;mode='affine'})) 'public model layout/schema differs'
    Assert-Reference ($config.modules -is [array] -and $config.modules.Count -le 1024) 'packed module inventory differs'
    $seen=[Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal);$selected=[Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    foreach($record in $config.modules) {
        Assert-Reference ($record -is [Collections.IDictionary] -and $record.path -is [string] -and $seen.Add($record.path)) 'duplicate/invalid packed module'
        if($record.path.EndsWith('.linear_attn.out_proj',[StringComparison]::Ordinal)) {
            Assert-Reference ($record.block -eq 1024 -and $record.dtype -ceq 'float16' -and $record.embedding -is [bool] -and -not $record.embedding) 'exact SSM module geometry differs'
            [void]$selected.Add($record.path)
        }
    }
    $wanted=[string[]]@($script:ReferenceBlocks|ForEach-Object{"model.layers.$_.linear_attn.out_proj"})
    Assert-Reference ($selected.Count -eq 48 -and $selected.SetEquals($wanted)) 'exact SSM module set differs'
    return ,@($script:ReferenceBlocks|ForEach-Object{"language_model.model.layers.$_.linear_attn.out_proj"})
}
function Save-PublicReplaySource([string]$Repository,[string]$Evidence,[string]$Revision) {
    $bundle=Join-Path $Evidence 'replay-source'
    Assert-Reference (-not [IO.Directory]::Exists($bundle)) 'retain earlier source selections'
    [void][IO.Directory]::CreateDirectory($bundle);$files=[Collections.Generic.List[object]]::new()
    foreach($relative in $script:ReplaySourceFiles) {
        $raw=Read-ReferenceBounded (Join-Path $Repository $relative) 1048576
        Assert-Reference ($raw.Length -gt 0) 'replay source is empty'
        $target=Join-Path $bundle $relative;[void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($target));[IO.File]::WriteAllBytes($target,$raw)
        $files.Add(@{path=$relative;bytes=$raw.Length;sha256=(Get-ReferenceDigest $raw)})
    }
    $manifest=Join-Path $bundle 'source-manifest.json'
    Write-ReferenceJson $manifest @{schemaVersion=1;kind='public-reference-replay-source';sourceRevision=$Revision;files=$files.ToArray()}
    return @{replaySourceManifestFile='replay-source/source-manifest.json';replaySourceManifestSha256=(Get-ReferenceFileDigest $manifest)}
}
function Get-PublicVectorMetrics([string]$Tool,[string]$Expected,[string]$Actual,[int]$Count,[double]$Absolute=0.00001,[double]$Relative=0.00001) {
    $result=Invoke-ReferenceNumeric $Tool @('compare',$Expected,$Actual,'f32','0',[string]$Count,'4',$Absolute.ToString('R',[Globalization.CultureInfo]::InvariantCulture),$Relative.ToString('R',[Globalization.CultureInfo]::InvariantCulture),'fma')
    return @{comparedElements=$result.elements;mismatchedElements=$result.failedElements;maximumAbsoluteDifference=$result.maximumAbsoluteError}
}
function Invoke-PublicReference([string]$Data,[string]$Evidence,[string]$ToolPath='') {
    Assert-PublicHosted
    $tool=Get-ReferenceNumericTool $ToolPath
    $temporary=[IO.Path]::GetFullPath($env:RUNNER_TEMP).TrimEnd('\','/')+[IO.Path]::DirectorySeparatorChar
    foreach($path in @($Data,$Evidence)){Assert-Reference ([IO.Path]::IsPathFullyQualified($path) -and [IO.Path]::GetFullPath($path).StartsWith($temporary,[StringComparison]::OrdinalIgnoreCase)) 'reference data/output must be external runner-temp directories'}
    Assert-Reference ([IO.Path]::GetFullPath($Data) -cne [IO.Path]::GetFullPath($Evidence) -and -not [IO.Directory]::Exists($Data)) 'reference data/output must be fresh separate directories'
    [void][IO.Directory]::CreateDirectory($Data);[void][IO.Directory]::CreateDirectory($Evidence)
    Assert-Reference (-not [IO.File]::Exists((Join-Path $Evidence 'attempt.json'))) 'retain earlier reference attempts'
    $repository=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
    $pinsRaw=Read-ReferenceBounded (Join-Path $PSScriptRoot 'public-authoring-pins.json') 65536;$pins=ConvertFrom-ReferenceJson $pinsRaw
    Assert-Reference ($pins.schemaVersion -eq 2 -and $pins.arithmeticId -ceq $script:ReferenceArithmetic -and $pins.absoluteTolerance -eq 0.00001 -and $pins.relativeTolerance -eq 0.00001 -and $pins.normalization -ceq 'scalar-f64-sum-sqrt-rounded-f32' -and $pins.reduction -ceq 'scalar-sequential-f32-no-fma') 'reference recipe or fixed bounds differ'
    foreach($pair in @(@('swift-lora.lock.json','adapter'),@('swift-lora.direction.json','direction'))) {
        $selected=(Read-ReferenceJson (Join-Path $repository ('config/'+$pair[0])))[$pair[1]]
        foreach($key in 'filename','bytes','sha256','url'){Assert-Reference ($pins[$pair[1]][$key] -ceq $selected[$key]) 'approved adapter/direction lock differs'}
    }
    $report=[ordered]@{schemaVersion=2;kind='independent-public-pack-A-reference';status='running';stage='inputs';arithmeticId=$script:ReferenceArithmetic;referencePinsSha256=(Get-ReferenceDigest $pinsRaw);inputPins=$pins;repository=$env:GITHUB_REPOSITORY;workflowPath='.github/workflows/ptq1-public-authoring-reference.yml';sourceRevision=$env:GITHUB_SHA;runId=$env:GITHUB_RUN_ID;runAttempt=$env:GITHUB_RUN_ATTEMPT;sites=@();runnerEnvironment=$env:RUNNER_ENVIRONMENT;authorOriginalProvenanceQualified=$false;nativeWriterBasisQualified=$false;logitsOrStateQualified=$false;defaultMayChange=$false;ownerDeviceTrials=0;modelInferenceCommands=0;fullCheckpointWritten=$false;all48PublicAComparisonPassed=$false}
    $attempt=Join-Path $Evidence 'attempt.json';$work=Join-Path $Data 'numeric-work';[void][IO.Directory]::CreateDirectory($work)
    try {
        Write-ReferenceJson $attempt $report
        $binding=Save-PublicReplaySource $repository $Evidence $report.sourceRevision;foreach($key in $binding.Keys){$report[$key]=$binding[$key]}
        $rustIdentity=(& rustc --version --verbose)-join "`n";Assert-Reference ($LASTEXITCODE -eq 0) 'reference compiler identity unavailable'
        Write-ReferenceJson (Join-Path $Evidence 'toolchain.json') @{powershell=$PSVersionTable.PSVersion.ToString();rust=$rustIdentity;referenceScriptSha256=(Get-ReferenceFileDigest $PSCommandPath);formatScriptSha256=(Get-ReferenceFileDigest (Join-Path $PSScriptRoot 'public_pack_formats.ps1'));numericSourceSha256=(Get-ReferenceFileDigest (Join-Path $PSScriptRoot 'reference_numerics.rs'));numericExecutableSha256=(Get-ReferenceFileDigest $tool);reduction=$pins.reduction;normalization=$pins.normalization;threads=1}
        $paths=@{}
        foreach($name in 'config','adapter','direction'){$report.stage=$name+'-intake';Write-ReferenceJson $attempt $report;$paths[$name]=Receive-PublicInput $Data $pins[$name]}
        $configRaw=Read-ReferenceBounded $paths.config $pins.config.bytes;$sites=Get-PublicConfigSites $configRaw;[IO.File]::WriteAllBytes((Join-Path $Evidence 'config.json'),$configRaw)
        $normalized=Join-Path $work 'normalized.f32le';$normal=Invoke-ReferenceNumeric $tool @('normalize',$paths.direction,$normalized)
        $direction=Read-ReferenceBounded $normalized 20480 20480;$negative=[byte[]]::new($direction.Length)
        for($i=0;$i -lt 5120;$i++){[Array]::Copy($direction,$i*4,$negative,$i*4,4);$negative[$i*4+3]=$negative[$i*4+3] -bxor 0x80}
        $negativePath=Join-Path $work 'negative.f32le';[IO.File]::WriteAllBytes($negativePath,$negative)
        $factors=Read-ReferenceAdapter (Read-ReferenceBounded $paths.adapter $pins.adapter.bytes $pins.adapter.bytes $pins.adapter.sha256)
        $bResults=[Collections.Generic.List[object]]::new();$factorPath=Join-Path $work 'factor.f32le'
        foreach($name in $factors.Keys){if($name.EndsWith('.lora_b',[StringComparison]::Ordinal)){
            [IO.File]::WriteAllBytes($factorPath,$factors[$name]);$metric=Get-PublicVectorMetrics $tool $negativePath $factorPath 5120 0.000001 0.000001
            $metric.tensor=$name;$metric.sha256=Get-ReferenceDigest $factors[$name];$bResults.Add($metric)
        }}
        Write-ReferenceJson (Join-Path $Evidence 'direction-comparison.json') @{sites=$bResults.ToArray()}
        Assert-Reference ($bResults.Count -eq 129 -and @($bResults|Where-Object{$_.mismatchedElements -ne 0}).Count -eq 0) 'normalized direction differs from approved B factors'
        $report.directionF32NormBeforeNormalization=$normal.norm;$report.normalizedDirectionSha256=Get-ReferenceDigest $direction;$report.all129BComparisonsPassed=$true
        $drive=[IO.DriveInfo]::new([IO.Path]::GetPathRoot($Data));Assert-Reference ($drive.AvailableFreeSpace -ge $pins.model.bytes+1073741824L) 'hosted storage cannot retain model plus reserve'
        $report.stage='model-intake';Write-ReferenceJson $attempt $report;$paths.model=Receive-PublicInput $Data $pins.model
        $report.stage='model-full-rehash-and-header';Write-ReferenceJson $attempt $report
        $model=[IO.File]::Open($paths.model,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read)
        try {
            $before=(Get-Item -LiteralPath $paths.model);$beforeLength=$before.Length;$beforeTime=$before.LastWriteTimeUtc.Ticks
            Assert-Reference ($beforeLength -eq $pins.model.bytes -and (Get-PublicStreamDigest $model) -ceq $pins.model.sha256) 'pre-read full input hash differs'
            $header=Read-ReferenceSafetensorsHeader $model;[IO.File]::WriteAllBytes((Join-Path $Evidence 'model-header.json'),$header.raw)
            $report.modelHeaderSha256=Get-ReferenceDigest $header.raw;$report.modelHeaderBytes=$header.raw.Length;$report.modelHeaderFile='model-header.json';$report.modelTensorCount=$header.descriptors.Count
            $siteResults=[Collections.Generic.List[object]]::new()
            for($index=0;$index -lt 48;$index++) {
                [void](Get-PublicRemaining);$block=$script:ReferenceBlocks[$index];$site=$sites[$index];$report.stage="reconstruct-ssm-$block";Write-ReferenceJson $attempt $report
                $wanted=[ordered]@{};$hashes=[ordered]@{}
                foreach($suffix in 'weight','scales','biases','signs'){
                    Assert-Reference ($header.descriptors.Contains($site+'.'+$suffix)) 'missing SSM tensor'
                    $descriptor=$header.descriptors[$site+'.'+$suffix];[object[]]$shape=switch($suffix){weight{@(5120,384)}scales{@(5120,48)}biases{@(5120,48)}signs{@(6144)}}
                    $dtypes=switch($suffix){weight{@('U32')}signs{@('F16','F32')}default{@('F16')}}
                    Assert-Reference ((Test-ReferenceEqual $descriptor.shape $shape) -and $descriptor.dtype -cin $dtypes) 'reference tensor unsupported shape/dtype'
                    $wanted[$suffix]=$descriptor;$length=[int]($descriptor.absoluteByteEnd-$descriptor.absoluteByteBegin);$raw=[byte[]]::new($length);$model.Position=$descriptor.absoluteByteBegin;$model.ReadExactly($raw);$hashes[$suffix]=Get-ReferenceDigest $raw
                }
                $name='ssm-{0:D2}.f32le' -f $block;$vector=Join-Path $Evidence $name;$matrix=Join-Path $work 'candidate.f16le'
                [void](Invoke-ReferenceNumeric $tool @('reconstruct',$paths.model,[string]$wanted.weight.absoluteByteBegin,[string]$wanted.scales.absoluteByteBegin,[string]$wanted.biases.absoluteByteBegin,[string]$wanted.signs.absoluteByteBegin,'5120','6144',$normalized,$vector,$matrix,$wanted.signs.dtype))
                $matrixDigest=Get-ReferenceFileDigest $matrix;[IO.File]::Delete($matrix)
                $shipped=$factors["blk.$block.ssm_out.weight.lora_a"];[IO.File]::WriteAllBytes($factorPath,$shipped)
                # The independently reconstructed vector is the reference for tolerance.
                $metric=Get-PublicVectorMetrics $tool $vector $factorPath 6144
                $metric.block=$block;$metric.publicTensorPrefix=$site;$metric.candidateF16MatrixSha256=$matrixDigest;$metric.authoringAFile=$name;$metric.authoringASha256=Get-ReferenceFileDigest $vector;$metric.authoringABytes=24576;$metric.shippedASha256=Get-ReferenceDigest $shipped;$metric.inputTensorDescriptors=$wanted;$metric.inputTensorSha256=$hashes
                $siteResults.Add($metric);$report.sites=$siteResults.ToArray();Write-ReferenceJson $attempt $report
            }
            $after=Get-Item -LiteralPath $paths.model
            Assert-Reference ($after.Length -eq $beforeLength -and $after.LastWriteTimeUtc.Ticks -eq $beforeTime) 'input endpoint identity changed'
        }finally{$model.Dispose()}
        [void](Get-PublicRemaining)
        $passed=$report.sites.Count -eq 48 -and @($report.sites|Where-Object{$_.mismatchedElements -ne 0}).Count -eq 0
        $report.all48PublicAComparisonPassed=$passed;$report.status=if($passed){'complete-public-reference-match'}else{'rejected-public-A-comparison'};$report.stage='complete'
        Write-ReferenceJson (Join-Path $Evidence 'report.json') $report
        return $passed
    }catch{$report.status='failed';$report.failureClass=$_.Exception.GetType().Name;if($_.Exception -is [IO.InvalidDataException]){$report.validationFailure=$_.Exception.Message};throw}
    finally{Write-ReferenceJson $attempt $report}
}
if($MyInvocation.InvocationName -ne '.') {
    try{if(-not (Invoke-PublicReference $DataDirectory $EvidenceDirectory $ToolPath)){exit 1}}
    catch{[Console]::Error.WriteLine('Public reference rejected: '+$_.Exception.GetType().Name);exit 1}
}
