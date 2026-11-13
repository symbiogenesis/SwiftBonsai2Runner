#Requires -Version 7.6
. "$PSScriptRoot/Hexagon.Common.ps1"
function Get-FunctionalTokenHash($Tokens) {
    $json = ConvertTo-Json -InputObject @($Tokens) -Compress -EscapeHandling EscapeNonAscii
    Get-HexagonHash ([Text.Encoding]::ASCII.GetBytes($json))
}
function Get-DecodedText([byte[]]$Data) {
    $attempts=[Collections.Generic.List[string]]::new()
    $encodings=@(
        @{bom=@(255,254,0,0);name='utf-32';encoding=[Text.UTF32Encoding]::new($false,$true,$true)},
        @{bom=@(0,0,254,255);name='utf-32';encoding=[Text.UTF32Encoding]::new($true,$true,$true)},
        @{bom=@(255,254);name='utf-16';encoding=[Text.UnicodeEncoding]::new($false,$true,$true)},
        @{bom=@(254,255);name='utf-16';encoding=[Text.UnicodeEncoding]::new($true,$true,$true)},
        @{bom=@(239,187,191);name='utf-8-sig';encoding=[Text.UTF8Encoding]::new($true,$true)})
    foreach ($candidate in $encodings) {
        if ($Data.Length -ge $candidate.bom.Count -and [Convert]::ToHexString($Data[0..($candidate.bom.Count-1)]) -ceq [Convert]::ToHexString([byte[]]$candidate.bom)) {
            try { return @{text=$candidate.encoding.GetString($Data,$candidate.bom.Count,$Data.Length-$candidate.bom.Count);observation=@{encoding=$candidate.name;basis='explicit-BOM';uncertain=$false}} }
            catch { return @{text=$null;observation=@{encoding=$candidate.name;basis='invalid-BOM-declared-text';uncertain=$true}} }
        }
    }
    try {
        $text=[Text.UTF8Encoding]::new($false,$true).GetString($Data)
        if (-not $text.Contains([char]0)) { return @{text=$text;observation=@{encoding='utf-8';basis='strict-decode-no-BOM';uncertain=$false}} }
        $attempts.Add('UTF8-decoded-with-NULs')
    } catch { $attempts.Add('strict-UTF8-failed') }
    if ($Data.Length -ge 4 -and $Data.Length % 2 -eq 0) {
        foreach ($parity in @(1,0)) {
            $zeros=0; for ($i=$parity;$i -lt $Data.Length;$i+=2) { if ($Data[$i] -eq 0) { $zeros++ } }
            if ($zeros / ($Data.Length/2) -gt 0.6) {
                $name=if ($parity -eq 1) {'utf-16-le'} else {'utf-16-be'}
                try { return @{text=[Text.UnicodeEncoding]::new(($parity -eq 0),$false,$true).GetString($Data);observation=@{encoding=$name;basis='NUL-pattern-Windows-text-fallback-no-BOM';uncertain=$true;priorAttempts=@($attempts)}} }
                catch { $attempts.Add($name+'-failed') }
            }
        }
    }
    [Text.Encoding]::RegisterProvider([Text.CodePagesEncodingProvider]::Instance)
    try { $text=[Text.Encoding]::GetEncoding(1252,[Text.EncoderFallback]::ExceptionFallback,[Text.DecoderFallback]::ExceptionFallback).GetString($Data); $name='cp1252'; $basis='Windows-1252-fallback-no-BOM' }
    catch { $attempts.Add('cp1252-failed'); $text=[Text.Encoding]::Latin1.GetString($Data); $name='latin-1'; $basis='byte-preserving-Latin1-fallback-no-BOM' }
    return @{text=$text;observation=@{encoding=$name;basis=$basis;uncertain=$true;priorAttempts=@($attempts)}}
}
function Get-LegalReview([byte[]]$Data,[int]$Words) {
    $decoded=Get-DecodedText $Data; $text=$decoded.text
    $result=@{encodingObservation=$decoded.observation;sdkToolTermsQualified=$false;generatedOutputRightsQualified=$false;keywordMatchesEstablishGrant=$false;absenceOfTermsEstablished=$false}
    if ($null -eq $text) { $result.status='pending-text-decoding';return $result }
    $controls=@($text.ToCharArray() | Where-Object { [int]$_ -lt 32 -and $_ -notin @("`r","`n","`t","`f") }).Count
    $result.nonTextControlCharacters=$controls
    if ($controls -gt [Math]::Max(2,[Math]::Floor($text.Length/100))) { $result.status='pending-text-plausibility; no-binary-classification-inferred';return $result }
    $terms=@{'qaic-name'='\bqaic\b';'IDL-compiler-phrase'='\b(?:another\s+)?idl\s+compiler\b';'SDK-phrase'='\b(?:hexagon\s+)?sdk\b';'license-agreement-phrase'='licen[sc]e\s+agreement';'product-license-phrase'='product\s+licen[sc]e';'redistribution-word'='\bredistribution\b';'patent-word'='\bpatent\b';'confidential-word'='\bconfidential\b';'proprietary-word'='\bproprietary\b';'GHC-Haskell-name'='\b(?:ghc|haskell)\b'}
    $occurrences=@{}
    foreach ($name in $terms.Keys) {
        $found=[regex]::Matches($text,$terms[$name],[Text.RegularExpressions.RegexOptions]::IgnoreCase)
        $occurrences[$name]=@{count=$found.Count;firstLines=@($found | Select-Object -First 8 | ForEach-Object { [regex]::Matches($text.Substring(0,$_.Index),"`n").Count+1 })}
    }
    $result.termOccurrences=$occurrences
    $focus=[regex]::Match($text,$terms['qaic-name'],'IgnoreCase')
    if (-not $focus.Success) { $focus=[regex]::Match($text,($terms['license-agreement-phrase']+'|'+$terms['product-license-phrase']),'IgnoreCase') }
    if ($focus.Success) {
        $start=[Math]::Max(0,$focus.Index-256)
        $before=@([regex]::Split($text.Substring($start,$focus.Index-$start).Trim(),'\s+') | Select-Object -Last 8)
        $after=@([regex]::Split($text.Substring($focus.Index,[Math]::Min($text.Length-$focus.Index,$focus.Length+512)).Trim(),'\s+'))
        $excerpt=(@($before+$after) | Select-Object -First $Words) -join ' '; $line=[regex]::Matches($text.Substring(0,$focus.Index),"`n").Count+1
    } else { $excerpt=([regex]::Split($text.Trim(),'\s+') | Select-Object -First $Words) -join ' '; $line=1 }
    $result.necessaryReviewExcerpt=@{line=$line;wordsMaximum=$Words;text=$excerpt}; $result.status='decoded-data-only-review; applicability-and-grant-unresolved'
    return $result
}
function Get-FunctionalTokens([string]$Text,[int]$Limit) {
    $Text=$Text.Replace("`r`n","`n").Replace("`r","`n"); $splices=[regex]::Matches($Text,"\\`n").Count; $Text=$Text.Replace("\`n",'')
    $tokens=[Collections.Generic.List[string]]::new();$lines=[Collections.Generic.List[int]]::new();$directives=[Collections.Generic.List[object]]::new()
    $state=@{position=0;line=1;lineStart=$true;preprocessor=$false;current=[Collections.Generic.List[string]]::new();directiveLine=$null;previousEnd=-1}
    $emit={ param([string]$Value)
        Assert-Hexagon ($tokens.Count -lt $Limit -and $Value.Length -le 4096) 'functional-token-count-or-length-bound'
        $tokens.Add($Value);$lines.Add($state.line)
        if ($state.preprocessor -and $Value -notin @('<PP-BEGIN>','<PP-END>')) { $state.current.Add($Value) }
    }
    $endDirective={
        if ($state.preprocessor) { & $emit '<PP-END>'; $directives.Add(@{line=$state.directiveLine;tokens=@($state.current)});$state.current=[Collections.Generic.List[string]]::new();$state.directiveLine=$null;$state.preprocessor=$false }
    }
    while ($state.position -lt $Text.Length) {
        $position=$state.position;$char=$Text[$position];$tail=$Text.Substring($position)
        if ([char]::IsWhiteSpace($char)) { if ($char -eq "`n") { & $endDirective;$state.line++;$state.lineStart=$true };$state.position++;continue }
        if ($tail.StartsWith('//')) { $end=$Text.IndexOf("`n",$position+2);$state.position=if ($end -lt 0) {$Text.Length} else {$end};continue }
        if ($tail.StartsWith('/*')) {
            $end=$Text.IndexOf('*/',$position+2);Assert-Hexagon ($end -ge 0) 'unterminated-block-comment';$comment=$Text.Substring($position,$end+2-$position)
            if ($comment.Contains("`n")) { & $endDirective;$state.line+=[regex]::Matches($comment,"`n").Count;$state.lineStart=$true };$state.position=$end+2;continue
        }
        if ($char -eq '#' -and $state.lineStart) { $state.preprocessor=$true;$state.directiveLine=$state.line;& $emit '<PP-BEGIN>' }
        if ($state.preprocessor -and $state.current.Count -eq 2 -and $state.current[0] -ceq '#' -and $state.current[1] -ceq 'include' -and $char -eq '<') {
            $end=$Text.IndexOf('>',$position+1);$newline=$Text.IndexOf("`n",$position+1)
            Assert-Hexagon ($end -ge 0 -and ($newline -lt 0 -or $newline -ge $end)) 'unterminated-include-header-name'
            & $emit ($Text.Substring($position,$end+1-$position));$state.position=$end+1;$state.previousEnd=$state.position;$state.lineStart=$false;continue
        }
        if ($state.preprocessor -and $state.current.Count -eq 3 -and $state.current[0] -ceq '#' -and $state.current[1] -ceq 'define' -and $char -eq '(') { & $emit $(if ($state.previousEnd -eq $position) {'<PP-MACRO-FUNCTIONLIKE>'} else {'<PP-MACRO-OBJECTLIKE-PAREN>'}) }
        $literal=[regex]::Match($tail,'^(?:u8|u|U|L)?["'']')
        if ($literal.Success) {
            $quote=$literal.Value[-1];$state.position+=$literal.Length;$closed=$false
            while ($state.position -lt $Text.Length) {
                $next=$Text[$state.position]
                if ($next -eq '\') { $state.position+=2 } elseif ($next -eq $quote) { $state.position++;$closed=$true;break } elseif ($next -eq "`n") { throw 'newline-in-unclosed-literal' } else { $state.position++ }
            }
            Assert-Hexagon $closed 'unterminated-quoted-literal'; & $emit ($Text.Substring($position,$state.position-$position))
        } else {
            $match=[regex]::Match($tail,'^(?:[A-Za-z_][A-Za-z0-9_]*|(?:\.?[0-9])(?:[A-Za-z0-9_.]|(?<=[eEpP])[+-])*|>>=|<<=|\.\.\.|->|\+\+|--|==|!=|<=|>=|&&|\|\||\+=|-=|\*=|/=|%=|&=|\|=|\^=|<<|>>|::|##|[{}\[\]();,:?~!+\-*/%&|^<>=.#])')
            Assert-Hexagon $match.Success 'unsupported-functional-source-character'; & $emit $match.Value;$state.position+=$match.Length
        }
        $state.previousEnd=$state.position;$state.lineStart=$false
    }
    & $endDirective
    return @{tokens=@($tokens);lines=@($lines);directives=@($directives);splices=$splices}
}
function Get-DeclarationCandidates($Tokens,$Lines,$Limits) {
    $code=[Collections.Generic.List[string]]::new();$codeLines=[Collections.Generic.List[int]]::new();$preprocessor=$false
    for ($i=0;$i -lt $Tokens.Count;$i++) { if ($Tokens[$i] -ceq '<PP-BEGIN>') {$preprocessor=$true} elseif ($Tokens[$i] -ceq '<PP-END>') {$preprocessor=$false} elseif (-not $preprocessor) {$code.Add($Tokens[$i]);$codeLines.Add($Lines[$i])} }
    $declarations=[Collections.Generic.List[object]]::new();$current=[Collections.Generic.List[string]]::new();$startLine=$null;$braces=0;$parentheses=0;$brackets=0
    for ($position=0;$position -lt $code.Count;$position++) {
        $token=$code[$position]
        if ($current.Count -eq 0 -and $position+2 -lt $code.Count -and $token -ceq 'extern' -and $code[$position+1] -ceq '"C"' -and $code[$position+2] -ceq '{') {$position+=2;continue}
        if ($current.Count -eq 0 -and $token -ceq '}') {continue}
        if ($null -eq $startLine) {$startLine=$codeLines[$position]};$current.Add($token)
        switch -CaseSensitive ($token) {'(' {$parentheses++} ')' {$parentheses--} '[' {$brackets++} ']' {$brackets--} '{' {$braces++} '}' {$braces--}}
        if ([Math]::Min($braces,[Math]::Min($parentheses,$brackets)) -lt 0) {return @{status='pending-declaration-review';reason='unbalanced-declaration-delimiter';declarations=@($declarations);unprocessedTailTokens=$code.Count-$position;typeCheckingExecuted=$false}}
        if ($token -ceq ';' -and $braces -eq 0 -and $parentheses -eq 0 -and $brackets -eq 0) {
            $names=@($current | Where-Object {$_ -cmatch '^[A-Za-z_][A-Za-z0-9_]*$' -and $_ -match 'dspqueue|fastrpc|remote|callback|rpcmem'} | Sort-Object -Unique -CaseSensitive)
            if ($names.Count -gt 0 -or $current -ccontains 'typedef' -or $current -ccontains 'enum') {
                if ($current.Count -gt $Limits.declarationTokens -or $declarations.Count -ge $Limits.functionalDeclarations) {return @{status='pending-declaration-review';reason='functional-declaration-output-bound';declarations=@($declarations);unprocessedTailTokens=$code.Count-$position;typeCheckingExecuted=$false}}
                $callbacks=@(for ($j=0;$j+3 -lt $current.Count;$j++) {if ($current[$j] -ceq '(' -and $current[$j+1] -ceq '*' -and $current[$j+3] -ceq ')') {$current[$j+2]}})
                $methods=@(for ($j=0;$j+1 -lt $current.Count;$j++) {if ($current[$j] -cmatch '^[A-Za-z_][A-Za-z0-9_]*$' -and $current[$j+1] -ceq '(' -and $current[$j] -cnotin @('if','while','for','switch','sizeof','defined','_Alignof','__attribute__','__declspec')) {$current[$j]}}) | Sort-Object -Unique -CaseSensitive
                $alias=$null
                if ($current -ccontains 'typedef') {if ($callbacks.Count) {$alias=$callbacks[-1]} else {for ($j=$current.Count-2;$j -ge 0;$j--) {if ($current[$j] -cmatch '^[A-Za-z_][A-Za-z0-9_]*$') {$alias=$current[$j];break}}}}
                $declarations.Add(@{startLine=$startLine;identifierCandidates=$names;typedefPresent=($current -ccontains 'typedef');enumPresent=($current -ccontains 'enum');typedefAliasNameCandidate=$alias;methodNameCandidates=@($methods);callbackTypedefNameCandidates=$(if ($current -ccontains 'typedef') {$callbacks} else {@()});normalizedTokenSha256=(Get-FunctionalTokenHash @($current));tokens=@($current);conditionalPreprocessorNotEvaluated=$true})
            }
            $current=[Collections.Generic.List[string]]::new();$startLine=$null
        }
    }
    return @{status='lexical-candidates-only-not-an-ABI-parser';declarations=@($declarations);unclosedOrNonDeclarationTailTokens=$current.Count;typeCheckingExecuted=$false}
}
function Get-FunctionalMetadata([byte[]]$Data,$Limits) {
    $decoded=Get-DecodedText $Data;$text=$decoded.text
    $result=@{encodingObservation=$decoded.observation;macrosEvaluated=$false;ABIEquivalenceQualified=$false;normalization='C-phase2-line-splices; comment-aware-C-IDL-lexing; canonical-JSON-ASCII-token-array; explicit-preprocessor-boundaries-and-macro-adjacency; literal-include-header-names'}
    if ($null -eq $text -or $text.Contains([char]0)) {$result.status='pending-functional-text-decoding';return $result}
    try {$lex=Get-FunctionalTokens $text $Limits.functionalTokens} catch {$result.status='pending-functional-token-review';$result.reason=$_.Exception.Message;return $result}
    $result.status='normalized-functional-tokens-observed-only';$result.normalizedTokenSha256=Get-FunctionalTokenHash $lex.tokens;$result.tokenCount=$lex.tokens.Count;$result.lineSplicesRemoved=$lex.splices
    if ($lex.directives.Count -gt $Limits.preprocessorDirectives) {$result.preprocessorStatus='pending-preprocessor-metadata-bound'} else {
        $result.preprocessorDirectives=$lex.directives;$result.includeDirectives=@($lex.directives | Where-Object {$_.tokens.Count -ge 2 -and $_.tokens[0] -ceq '#' -and $_.tokens[1] -ceq 'include'});$result.macroDefinitions=@($lex.directives | Where-Object {$_.tokens.Count -ge 2 -and $_.tokens[0] -ceq '#' -and $_.tokens[1] -ceq 'define'})
    }
    $result.declarationCandidates=Get-DeclarationCandidates $lex.tokens $lex.lines $Limits
    return $result
}
function Compare-Functional([byte[]]$Sdk,[byte[]]$Public,$Limits) {
    $left=Get-FunctionalMetadata $Sdk $Limits;$right=Get-FunctionalMetadata $Public $Limits;$complete=$left.ContainsKey('normalizedTokenSha256') -and $right.ContainsKey('normalizedTokenSha256')
    $indexes=@(@{},@{})
    for ($i=0;$i -lt 2;$i++) { $meta=@($left,$right)[$i];if (-not $meta.ContainsKey('declarationCandidates')) {continue}
        foreach ($declaration in $meta.declarationCandidates.declarations) {
            $names=@($declaration.identifierCandidates)+@($declaration.methodNameCandidates)+@($declaration.callbackTypedefNameCandidates)
            if ($declaration.typedefAliasNameCandidate) {$names+=@($declaration.typedefAliasNameCandidate)}
            foreach ($name in $names) {if (-not $indexes[$i].ContainsKey($name)) {$indexes[$i][$name]=[Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)};$null=$indexes[$i][$name].Add($declaration.normalizedTokenSha256)}
        }
    }
    $shared=@($indexes[0].Keys | Where-Object {$indexes[1].ContainsKey($_)} | Sort-Object -CaseSensitive)
    $named=@($shared | Select-Object -First $Limits.functionalDeclarations | ForEach-Object {@{identifier=$_;sdkCandidateTokenHashes=@($indexes[0][$_] | Sort-Object -CaseSensitive);publicCandidateTokenHashes=@($indexes[1][$_] | Sort-Object -CaseSensitive);fullLexicalCandidateSetsEqual=$indexes[0][$_].SetEquals($indexes[1][$_])}})
    return @{status=$(if ($complete) {'lexical-comparison-only'} else {'pending-functional-token-review'});sdk=$left;public=$right;rawBytesEqual=([Convert]::ToHexString($Sdk) -ceq [Convert]::ToHexString($Public));normalizedTokenStreamsEqual=$(if ($complete) {$left.normalizedTokenSha256 -ceq $right.normalizedTokenSha256} else {$null});sameIdentifierCandidateComparisons=$named;sameIdentifierCandidateComparisonsTruncated=($named.Count -ne $shared.Count);matchingNamesOrTokensProveABIEquivalence=$false;WindowsRuntimeCompatibilityQualified=$false;callbackLifetimeQualified=$false;cacheAndBufferOwnershipQualified=$false;installedDriverAbiQualified=$false;deviceInvocationAuthorized=$false}
}
