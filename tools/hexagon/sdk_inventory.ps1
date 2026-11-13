#Requires -Version 7.6
param([Parameter(Mandatory)][string]$Archive,[Parameter(Mandatory)][string]$Pin,[Parameter(Mandatory)][string]$Output,[Parameter(Mandatory)][string]$PublicInputDirectory,[string]$Reader)
. "$PSScriptRoot/sdk_functional_review.ps1"
function Get-PeMetadata([byte[]]$Data) {
    if ($Data.Length -lt 64 -or $Data[0] -ne 77 -or $Data[1] -ne 90) {return @{validPeHeader=$false}}
    $offset=[BitConverter]::ToUInt32($Data,60)
    if ($offset -lt 64 -or [long]$offset+24 -gt $Data.Length -or [Convert]::ToHexString($Data[$offset..($offset+3)]) -cne '50450000') {return @{validPeHeader=$false}}
    $machine=[BitConverter]::ToUInt16($Data,$offset+4);return @{validPeHeader=$true;machine=$machine;machineHex=$machine.ToString('x4')}
}
function Get-NoticeMetadata([byte[]]$Data) {
    $markers=@{copyright='copyright';'all-rights-reserved'='all rights reserved';'qualcomm-name'='qualcomm';'confidential-marker'='confidential';'proprietary-marker'='proprietary';'redistribution-word'='redistribution';'distribution-word'='distribution';'license-word'='license';'agreement-word'='agreement';'terms-word'='terms';'bsd-name'='bsd';'apache-name'='apache';'gpl-name'='general public license'}
    if ((Get-PeMetadata $Data).validPeHeader) {$text=$null;$encoding=@{encoding=$null;basis='validated-PE-header; text-decoding-not-attempted';uncertain=$false};$lower=[Text.Encoding]::Latin1.GetString($Data).ToLowerInvariant()} else {$decoded=Get-DecodedText $Data;$text=$decoded.text;$encoding=$decoded.observation;$lower=if ($null -ne $text) {$text.ToLowerInvariant()} else {[Text.Encoding]::Latin1.GetString($Data).ToLowerInvariant()}}
    $result=@{markersPresent=@($markers.Keys | Where-Object {$lower.Contains($markers[$_])});licenseRightsInterpretationReviewed=$false;encodingObservation=$encoding}
    if ($null -ne $text) {$result.spdxIdentifiersMentioned=@([regex]::Matches($text,'SPDX-License-Identifier:\s*([^\r\n*]{1,128})') | ForEach-Object {$_.Groups[1].Value.Trim()} | Sort-Object -Unique | Select-Object -First 16);$index=0;$result.copyrightMarkerLines=@($text -split '\r?\n' | ForEach-Object {$index++;if ($_.ToLowerInvariant().Contains('copyright')) {$index}} | Select-Object -First 32);$result.classificationScope='text-markers-only-not-a-license-grant'} else {$result.classificationScope='raw-byte-markers-only-not-a-license-grant; no-binary-classification-from-decode-failure'}
    return $result
}
function Get-CmakeStatements([string]$Text) {
    $statements=[Collections.Generic.List[object]]::new();$cursor=0
    while ($cursor -lt $Text.Length) {
        if ([char]::IsWhiteSpace($Text[$cursor])) {$cursor++;continue}
        if ($Text[$cursor] -eq '#') {
            $bracket=[regex]::Match($Text.Substring($cursor),'^#\[(=*)\[')
            if ($bracket.Success) {$closing=']'+$bracket.Groups[1].Value+']';$end=$Text.IndexOf($closing,$cursor+$bracket.Length);Assert-Hexagon ($end -ge 0) 'unterminated CMake bracket comment';$cursor=$end+$closing.Length} else {$end=$Text.IndexOf("`n",$cursor);$cursor=if ($end -lt 0) {$Text.Length} else {$end+1}};continue
        }
        $head=[regex]::Match($Text.Substring($cursor),'^([A-Za-z_][A-Za-z0-9_]*)\s*\(');Assert-Hexagon $head.Success 'unsupported CMake statement boundary';$start=$cursor;$cursor+=$head.Length;$depth=1;$tokens=[Collections.Generic.List[string]]::new();$token=[Text.StringBuilder]::new();$quoted=$false
        while ($cursor -lt $Text.Length -and $depth -gt 0) {
            $char=$Text[$cursor]
            if ($char -eq '\' -and $cursor+1 -lt $Text.Length) {$null=$token.Append($Text.Substring($cursor,2));$cursor+=2;continue}
            if ($char -eq '"') {$quoted=-not $quoted;$null=$token.Append($char);$cursor++;continue}
            if (-not $quoted -and $char -eq '#') {
                if ($token.Length) {$tokens.Add($token.ToString());$null=$token.Clear()}
                $bracket=[regex]::Match($Text.Substring($cursor),'^#\[(=*)\[')
                if ($bracket.Success) {$closing=']'+$bracket.Groups[1].Value+']';$end=$Text.IndexOf($closing,$cursor+$bracket.Length);Assert-Hexagon ($end -ge 0) 'unterminated CMake argument comment';$cursor=$end+$closing.Length} else {$end=$Text.IndexOf("`n",$cursor);$cursor=if ($end -lt 0) {$Text.Length} else {$end+1}};continue
            }
            if (-not $quoted -and $char -eq '[') {
                $bracket=[regex]::Match($Text.Substring($cursor),'^\[(=*)\[')
                if ($bracket.Success) {$closing=']'+$bracket.Groups[1].Value+']';$end=$Text.IndexOf($closing,$cursor+$bracket.Length);Assert-Hexagon ($end -ge 0) 'unterminated CMake bracket argument';$extent=$end+$closing.Length;$null=$token.Append($Text.Substring($cursor,$extent-$cursor));$cursor=$extent;continue}
            }
            if (-not $quoted -and $char -in @('(',')')) {if ($token.Length) {$tokens.Add($token.ToString());$null=$token.Clear()};if ($char -eq '(') {$depth++} else {$depth--};if ($depth) {$tokens.Add([string]$char)}} elseif (-not $quoted -and [char]::IsWhiteSpace($char)) {if ($token.Length) {$tokens.Add($token.ToString());$null=$token.Clear()}} else {$null=$token.Append($char)};$cursor++
        }
        Assert-Hexagon ($depth -eq 0 -and -not $quoted) 'unterminated CMake command'
        $statements.Add(@{command=$head.Groups[1].Value.ToLowerInvariant();tokens=@($tokens);startLine=[regex]::Matches($Text.Substring(0,$start),"`n").Count+1;endLine=[regex]::Matches($Text.Substring(0,$cursor),"`n").Count+1})
    }
    return ,@($statements)
}
function Get-RecipeMetadata($Recipes,$Pin) {
    $limits=$Pin.limits;$definitions=@{};$outside=[Collections.Generic.List[object]]::new();$failures=[Collections.Generic.List[string]]::new()
    foreach ($path in $Recipes.Keys) {
        try {$statements=Get-CmakeStatements ([Text.UTF8Encoding]::new($false,$true).GetString($Recipes[$path]).TrimStart([char]0xfeff))} catch {$failures.Add($path);continue}
        $cursor=0
        while ($cursor -lt $statements.Count) {
            $statement=$statements[$cursor]
            if ($statement.command -notin @('function','macro')) {$outside.Add(@{path=$path;statement=$statement});$cursor++;continue}
            $end=$cursor+1;$depth=1
            while ($end -lt $statements.Count -and $depth -gt 0) {$command=$statements[$end].command;if ($command -in @('function','macro')) {$depth++} elseif ($command -in @('endfunction','endmacro')) {$depth--};$end++}
            if ($depth -gt 0 -or $statement.tokens.Count -eq 0) {$failures.Add($path);break}
            $name=$statement.tokens[0].ToLowerInvariant();if (-not $definitions.ContainsKey($name)) {$definitions[$name]=[Collections.Generic.List[object]]::new()};$definitions[$name].Add(@{path=$path;statements=@($statements[$cursor..($end-1)])});$cursor=$end
        }
    }
    $pending=[Collections.Generic.Queue[string]]::new();$pending.Enqueue('build_idl');$chosen=[Collections.Generic.List[object]]::new();$seen=[Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal);$variables=[Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal);foreach ($name in @('proj-qaic_src','HEXAGON_SDK_ROOT','cmdLineOptions')) {$null=$variables.Add($name)}
    while ($pending.Count) {
        $name=$pending.Dequeue();if (-not $seen.Add($name)) {continue}
        Assert-Hexagon ($definitions.ContainsKey($name) -and $definitions[$name].Count -eq 1 -and $chosen.Count -lt $limits.recipeFunctions) 'required CMake function absent, ambiguous or exceeds bound'
        $match=$definitions[$name][0];$emitted=@(foreach ($statement in $match.statements) {
            $item=$statement.Clone();$item.tokens=@($item.tokens)
            if ($item.command -ceq 'message') {$item.tokens=@();$item.nonfunctionalMessageTextOmitted=$true} else {for ($i=0;$i+1 -lt $item.tokens.Count;$i++) {if ($item.tokens[$i].ToUpperInvariant() -ceq 'COMMENT') {$item.tokens[$i+1]='<nonfunctional-comment-text-omitted>'}}};$item
        });$chosen.Add(@{name=$name;sourcePath=$match.path;statements=$emitted})
        foreach ($statement in $match.statements) {if ($definitions.ContainsKey($statement.command) -and -not $seen.Contains($statement.command)) {$pending.Enqueue($statement.command)};foreach ($token in $statement.tokens) {foreach ($reference in [regex]::Matches($token,'\$(?:ENV)?\{([^}]+)\}')) {$null=$variables.Add($reference.Groups[1].Value)}}}
    }
    $setup=@(foreach ($entry in $outside) {$statement=$entry.statement;if ($statement.command -notin @('set','list','include','get_filename_component')) {continue};if (@($statement.tokens | Where-Object {$variables.Contains($_) -or $_ -match 'qaic|include|/incs|cmdLineOptions'}).Count) {$item=$statement.Clone();$item.sourcePath=$entry.path;$item}})
    $all=@($chosen | ForEach-Object {$_.statements})+$setup;Assert-Hexagon ($all.Count -le $limits.recipeStatements) 'selected functional CMake statement metadata exceeds bound'
    foreach ($statement in $all) {Assert-Hexagon ($statement.tokens.Count -le $limits.recipeStatementTokens -and @($statement.tokens | Where-Object {$_.Length -gt $limits.recipeTokenCharacters}).Count -eq 0) 'selected CMake argument token metadata exceeds bound'}
    return @{status='functional-token-metadata-collected-unreviewed';scope='functional-command-token-metadata-only-no-comments-or-CMake-evaluation';functions=@($chosen);relatedTopLevelSetup=$setup;otherRecipeFilesNotParsed=@($failures);variableExpansionExecuted=$false;generatorCommandAuthorized=$false}
}
function Get-SdkReview([byte[]]$Data,[string]$Kind,[int]$Words) {
    if ($Kind -ceq 'license-review') {return Get-LegalReview $Data $Words};if ($Kind -cne 'build-idl-recipe') {return $null}
    try {$text=[Text.UTF8Encoding]::new($false,$true).GetString($Data).TrimStart([char]0xfeff)} catch {return @{textEncoding='not-UTF8';excerpt=$null}}
    $lines=$text -split '\r?\n';$qaic=@(for ($i=0;$i -lt $lines.Count;$i++) {if ($lines[$i].ToLowerInvariant().Contains('qaic')) {@{line=$i+1;text=$lines[$i].Trim()}}})
    return @{textEncoding='UTF8';qaicReferenceLines=@($qaic | Select-Object -First 64 | ForEach-Object {$_.line});qaicReferenceExcerpt=([regex]::Split((($qaic.text -join ' ').Trim()),'\s+') | Select-Object -First $Words) -join ' ';flagSpellingsMentionedInRecipe=@([regex]::Matches($text,'(?<![A-Za-z0-9_])-[A-Za-z][A-Za-z0-9_-]*') | ForEach-Object {$_.Value} | Sort-Object -Unique | Select-Object -First 128);commandAndImportInterpretationReviewed=$false}
}
function Get-SdkInventory([string]$Archive,$Pin,[string]$PublicDirectory,[string]$Reader) {
    $limits=$Pin.limits;$selected=[Collections.Generic.List[object]]::new();$recipes=@{};$comparisonBytes=@{};$metrics=$null
    $info=[Diagnostics.ProcessStartInfo]::new();$info.FileName=$Reader;$info.ArgumentList.Add($Archive);$info.ArgumentList.Add($script:PinPath);$info.UseShellExecute=$false;$info.CreateNoWindow=$true;$info.RedirectStandardOutput=$true;$info.RedirectStandardError=$true
    $process=[Diagnostics.Process]::new();$process.StartInfo=$info;$clock=[Diagnostics.Stopwatch]::StartNew();$started=$false
    try {
        Assert-Hexagon ($process.Start()) 'SDK reader did not start';$started=$true;$stderr=$process.StandardError.ReadToEndAsync()
        while ($true) {
            $remaining=[int]((360-$clock.Elapsed.TotalSeconds)*1000);Assert-Hexagon ($remaining -gt 0) 'SDK inventory aggregate allowance expired'
            $read=$process.StandardOutput.ReadLineAsync();Assert-Hexagon ($read.Wait($remaining)) 'SDK inventory stream timed out';$line=$read.Result;if ($null -eq $line) {break}
            Assert-Hexagon ($line.Length -le $limits.selectedFileBytes*2+4096) 'SDK reader output exceeds fixed bound'
            $entry=ConvertFrom-HexagonJson ([Text.Encoding]::UTF8.GetBytes($line))
            if ($entry.ContainsKey('complete')) {Assert-Hexagon ($null -eq $metrics -and $entry.complete -is [bool] -and $entry.complete) 'SDK reader duplicated completion';$metrics=$entry;continue}
            Assert-Hexagon ($null -eq $metrics -and $selected.Count -lt $limits.selectedFiles) 'SDK reader emitted invalid sequence'
            $data=[Convert]::FromHexString($entry.hex);$entry.Remove('hex');Assert-Hexagon ($data.Length -eq $entry.bytes -and (Get-HexagonHash $data) -ceq $entry.sha256) 'SDK stream record integrity differs'
            $name=$entry.path;$kind=$entry.category
            if ($name -ceq $Pin.requiredRecipePath) {Assert-Hexagon ($entry.bytes -eq $Pin.requiredRecipeBytes -and $entry.sha256 -ceq $Pin.requiredRecipeSha256) 'observed CMake recipe differs from reviewed identity'}
            $entry.noticeClassification=Get-NoticeMetadata $data
            if ($kind -in @('sdk-idl-input-candidate','sdk-rpc-header-candidate')) {$entry.functionalMetadata=Get-FunctionalMetadata $data $limits}
            foreach ($comparison in $Pin.functionalComparisons) {if ($name -ceq $comparison.sdkPath) {Assert-Hexagon ($entry.bytes -eq $comparison.sdkBytes -and $entry.sha256 -ceq $comparison.sdkSha256) 'SDK functional input differs from observed identity';$comparisonBytes[$name]=$data}}
            if ($name -ceq $Pin.requiredNotice.path) {Assert-Hexagon ($entry.bytes -eq $Pin.requiredNotice.bytes -and $entry.sha256 -ceq $Pin.requiredNotice.sha256) 'SDK NOTICE differs from observed identity'}
            if ($kind -ceq 'qaic-generator') {$entry.peHeader=Get-PeMetadata $data}
            if ($kind -in @('build-idl-recipe','build-recipe-helper')) {$recipes[$name]=$data}
            $review=Get-SdkReview $data $kind $limits.reviewWordsPerFile;if ($null -ne $review) {$entry.review=$review};$selected.Add($entry)
        }
        Assert-Hexagon ($process.WaitForExit([Math]::Max(1,[int]((360-$clock.Elapsed.TotalSeconds)*1000)))) 'SDK reader exit timed out'
        Assert-Hexagon ($stderr.Wait(10000) -and $stderr.Result.Length -le 65536 -and $process.ExitCode -eq 0 -and $null -ne $metrics) ('SDK inventory stream failed: '+$stderr.Result)
    } finally {if ($started -and -not $process.HasExited) {$process.Kill($true);$null=$process.WaitForExit(10000)};$process.Dispose()}
    foreach ($name in @($Pin.intendedQaicPath,$Pin.requiredRecipePath,$Pin.requiredNotice.path)) {Assert-Hexagon (@($selected | Where-Object {$_.path -ceq $name}).Count -eq 1) 'required SDK input absent from checked archive'}
    try {$recipe=Get-RecipeMetadata $recipes $Pin} catch {$recipe=@{status='pending-functional-token-review';reason=$_.Exception.Message;variableExpansionExecuted=$false;generatorCommandAuthorized=$false}}
    $comparisons=@(foreach ($comparison in $Pin.functionalComparisons) {
        Assert-Hexagon ($comparisonBytes.ContainsKey($comparison.sdkPath)) 'SDK/public comparison omitted fixed input';$publicPath=Join-Path $PublicDirectory $comparison.publicName;$item=Get-Item -LiteralPath $publicPath;Assert-Hexagon (-not ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) 'public comparison input cannot be linked'
        $public=Read-HexagonBytes $publicPath $comparison.publicBytes;Assert-Hexagon ($public.Length -eq $comparison.publicBytes -and (Get-HexagonHash $public) -ceq $comparison.publicSha256) 'public declaration comparison input differs from source pin'
        $sdk=$comparisonBytes[$comparison.sdkPath];@{sdkPath=$comparison.sdkPath;publicName=$comparison.publicName;sdkRawSha256=(Get-HexagonHash $sdk);publicRawSha256=(Get-HexagonHash $public);comparison=(Compare-Functional $sdk $public $limits)}
    })
    return @{schemaVersion=1;kind='hexagon-SDK-data-only-inventory';archive=$Pin.asset;archiveEntriesRead=$metrics.archiveEntriesRead;declaredUncompressedBytes=$metrics.declaredUncompressedBytes;linkEntriesNotExtracted=$metrics.linkEntriesNotExtracted;selectedBytesRead=$metrics.selectedBytesRead;selectedFiles=@($selected | Sort-Object path);buildIdlRecipe=$recipe;sdkPublicFunctionalComparisons=$comparisons;functionalComparisonScope='unpreprocessed-lexical-token-and-declaration-candidates-only; no-ABI-layout-or-runtime-equivalence';WindowsExtensionDeclarationsQualified=$false;licenseFilenameSearch=@{scope='all-regular-archive-entry-leaf-names-without-depth-or-prefix-restrictions';substringIndicators=@('license','licence','eula','notice','copyright','terms','agreement','copying');matchingNamesEstablishLicenseAbsenceOrRights=$false};sdkContentsExtracted=$false;sdkCodeExecuted=$false;generatorExecuted=$false;generatedSourceUploaded=$false;licenseReview='pending; bounded necessary review excerpts and file identities only';generatorInvocationReviewed=$false;deviceCodeLoaded=$false;profileEligible=$false}
}
if ($MyInvocation.InvocationName -ne '.') {
    $script:PinPath=[IO.Path]::GetFullPath($Pin);$selection=Read-HexagonJson $script:PinPath
    Assert-Hexagon ($selection.schemaVersion -eq 1 -and $selection.generatorExecutionAllowed -is [bool] -and -not $selection.generatorExecutionAllowed -and $selection.deviceExecutionAllowed -is [bool] -and -not $selection.deviceExecutionAllowed) 'SDK inventory pin does not authorize operation'
    if (-not $Reader) {
        Assert-Hexagon ($env:RUNNER_ENVIRONMENT -ceq 'github-hosted' -and $env:RUNNER_TEMP) 'SDK reader build requires hosted scratch'
        $target=Join-Path $env:RUNNER_TEMP 'Bonsai-Sdk-Reader';$manifest=Join-Path $PSScriptRoot 'sdk-reader/Cargo.toml'
        & cargo build --locked --release --manifest-path $manifest --target-dir $target;if ($LASTEXITCODE -ne 0) {throw 'SDK reader compilation failed'}
        $Reader=Join-Path $target ('release/bonsai-sdk-reader'+$(if ($IsWindows) {'.exe'} else {''}))
    }
    $result=Get-SdkInventory ([IO.Path]::GetFullPath($Archive)) $selection $PublicInputDirectory $Reader
    $result.inventorySourceSha256=(Get-FileHash -LiteralPath $PSCommandPath -Algorithm SHA256).Hash.ToLowerInvariant();$result.inventoryPinSha256=(Get-FileHash -LiteralPath $Pin -Algorithm SHA256).Hash.ToLowerInvariant();$result.functionalReviewSourceSha256=(Get-FileHash -LiteralPath (Join-Path $PSScriptRoot 'sdk_functional_review.ps1') -Algorithm SHA256).Hash.ToLowerInvariant();$result.archiveReader=Get-HexagonIdentity $Reader
    $encoded=[Text.Encoding]::ASCII.GetBytes((ConvertTo-Json -InputObject $result -Depth 64 -EscapeHandling EscapeNonAscii)+"`n");Assert-Hexagon ($encoded.Length -le $selection.limits.receiptBytes) 'SDK inventory receipt exceeds byte bound'
    $stream=[IO.File]::Open($Output,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write);try {$stream.Write($encoded);$stream.Flush()} finally {$stream.Dispose()}
    @{inventoryWritten=$true;selectedFiles=$result.selectedFiles.Count;sdkCodeExecuted=$false} | ConvertTo-Json -Compress
}
