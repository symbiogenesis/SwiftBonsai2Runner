#Requires -Version 7.6
param([string]$Test)
. "$PSScriptRoot/sdk_functional_review.ps1"
. "$PSScriptRoot/sdk_inventory.ps1" -Archive unused -Pin unused -Output unused -PublicInputDirectory unused
. "$PSScriptRoot/Hexagon.Tests.ps1"
$limits=@{functionalTokens=10000;functionalDeclarations=64;declarationTokens=256;preprocessorDirectives=64}
function Tokens([string]$Text) {return ,(Get-FunctionalTokens $Text 10000).tokens}
$tests=[ordered]@{}
$tests.test_comments_do_not_consume_literal_or_include_contents={
    $tokens=Tokens "#include <api // header.h>`nconst char *remote_url = `"https://x/*literal*/`"; /* tail */`n"
    Assert-Hexagon ($tokens -ccontains '<api // header.h>' -and $tokens -ccontains '"https://x/*literal*/"' -and $tokens -cnotcontains 'tail') 'literal/comment distinction lost'
    Assert-Hexagon (Test-HexagonEqual (Tokens 'int remote_x /* comment */ ;') @('int','remote_x',';')) 'comment token leaked'
}
$tests.test_macro_function_adjacency_and_directive_ends_remain_distinct={
    $function=Tokens "#define FN(x) x`n";$object=Tokens "#define FN (x) x`n";$comment=Tokens "#define FN/**/(x) x`n"
    Assert-Hexagon ($function -ccontains '<PP-MACRO-FUNCTIONLIKE>' -and $object -ccontains '<PP-MACRO-OBJECTLIKE-PAREN>') 'macro adjacency lost'
    Assert-Hexagon ((Test-HexagonEqual $object $comment) -and -not (Test-HexagonEqual $function $object)) 'macro comment gap changed'
    Assert-Hexagon (-not (Test-HexagonEqual (Tokens "#define X 1`nY") (Tokens "#define X 1 Y`n"))) 'directive boundary lost'
}
$tests.test_line_splices_precede_comment_recognition={Assert-Hexagon (Test-HexagonEqual (Tokens "#define FN(x) ((x) \`n+ 1)`n") (Tokens "#define FN(x) ((x) + 1)`n")) 'line splice changed';Assert-Hexagon ((Tokens "// comment\`nint remote_x;`n").Count -eq 0) 'splice comment ended early'}
$tests.test_literal_prefix_adjacency_is_not_whitespace={Assert-Hexagon (Test-HexagonEqual (Tokens 'L"x"') @('L"x"')) 'literal prefix split';Assert-Hexagon (-not (Test-HexagonEqual (Tokens 'L"x"') (Tokens 'L "x"'))) 'literal whitespace normalized'}
$tests.test_typedef_callback_enum_and_method_candidates_preserve_tokens={
    $meta=Get-FunctionalMetadata ([Text.Encoding]::UTF8.GetBytes("typedef void (*dspqueue_callback)(int fd, void *ctx);`nenum dspqueue_flags { DSPQUEUE_BUFFER_REF=4, DSPQUEUE_BUFFER_DEREF=8 };`nint dspqueue_read(int fd, unsigned timeout);`n")) $limits
    $declarations=$meta.declarationCandidates.declarations
    Assert-Hexagon ($declarations.Count -eq 3 -and (Test-HexagonEqual $declarations[0].callbackTypedefNameCandidates @('dspqueue_callback')) -and $declarations[1].enumPresent -and $declarations[1].tokens -ccontains 'DSPQUEUE_BUFFER_REF') 'declaration metadata lost'
    $comparison=Compare-Functional ([Text.Encoding]::UTF8.GetBytes('int remote_call(int arg);')) ([Text.Encoding]::UTF8.GetBytes('short remote_call(int arg);')) $limits
    Assert-Hexagon (-not $comparison.normalizedTokenStreamsEqual -and -not $comparison.matchingNamesOrTokensProveABIEquivalence -and -not $comparison.callbackLifetimeQualified) 'lexical comparison granted qualification'
}
$tests.test_unknown_source_syntax_preserves_pending_state={
    $meta=Get-FunctionalMetadata ([Text.Encoding]::UTF8.GetBytes('int @remote_value;')) $limits
    Assert-Hexagon ($meta.status -ceq 'pending-functional-token-review' -and -not $meta.ContainsKey('normalizedTokenSha256')) 'unsupported syntax qualified'
    $comparison=Compare-Functional ([Text.Encoding]::UTF8.GetBytes('int @remote_value;')) ([Text.Encoding]::UTF8.GetBytes('int remote_value;')) $limits
    Assert-Hexagon ($null -eq $comparison.normalizedTokenStreamsEqual -and -not $comparison.deviceInvocationAuthorized) 'pending comparison qualified'
}
$tests.test_known_boms_and_uncertain_windows_text_fallback={
    foreach ($encoding in @([Text.UnicodeEncoding]::new($false,$true),[Text.UTF32Encoding]::new($false,$true))) {$decoded=Get-DecodedText ([byte[]]($encoding.GetPreamble()+$encoding.GetBytes('QAIC terms')));Assert-Hexagon ($decoded.text -ceq 'QAIC terms' -and $decoded.observation.basis -ceq 'explicit-BOM' -and -not $decoded.observation.uncertain) 'explicit BOM lost'}
    $review=Get-LegalReview ([byte[]](@(147)+[Text.Encoding]::ASCII.GetBytes('QAIC')+@(148)+[Text.Encoding]::ASCII.GetBytes(' product license agreement'))) 25
    Assert-Hexagon ($review.encodingObservation.encoding -ceq 'cp1252' -and $review.encodingObservation.uncertain -and $review.termOccurrences['qaic-name'].count -eq 1 -and -not $review.keywordMatchesEstablishGrant -and -not $review.sdkToolTermsQualified -and ([regex]::Split($review.necessaryReviewExcerpt.text,'\s+')).Count -le 25) 'uncertain legal terms qualified'
}
$tests.test_cmake_recipe_keeps_balanced_tokens_and_pending_authority={
    $text=@'
# ignored (comment)
set(HEXAGON_SDK_ROOT "C:/sdk with spaces")
function(build_idl project)
  helper("${HEXAGON_SDK_ROOT}/qaic" [=[literal #(content)]=])
  message("Nonfunctional prose")
endfunction()
function(helper tool)
  add_custom_command(COMMAND "${tool}" --standard-types COMMENT "omit comment prose")
endfunction()
'@
    $statements=Get-CmakeStatements $text
    Assert-Hexagon ($statements.Count -eq 8 -and $statements[0].tokens[1] -ceq '"C:/sdk with spaces"' -and $statements[2].tokens[1] -ceq '[=[literal #(content)]=]') 'CMake quoted/bracket argument changed'
    $pin=@{limits=@{recipeFunctions=16;recipeStatements=256;recipeStatementTokens=128;recipeTokenCharacters=512}}
    $review=Get-RecipeMetadata @{'sdk/build/cmake/hexagon_fun.cmake'=[Text.Encoding]::UTF8.GetBytes($text)} $pin
    Assert-Hexagon ($review.functions.Count -eq 2 -and -not $review.variableExpansionExecuted -and -not $review.generatorCommandAuthorized -and $review.functions[0].statements[2].nonfunctionalMessageTextOmitted) 'recipe source authority changed'
    Assert-Rejected {Get-CmakeStatements 'function(build_idl "unterminated)'}
}
$tests.test_sdk_pe_and_notice_metadata_never_grant_rights={
    $data=[byte[]]::new(96);$data[0]=77;$data[1]=90;[Array]::Copy([BitConverter]::GetBytes([uint]64),0,$data,60,4);[Array]::Copy([byte[]]@(80,69,0,0,100,170),0,$data,64,6)
    $pe=Get-PeMetadata $data;Assert-Hexagon ($pe.validPeHeader -and $pe.machineHex -ceq 'aa64') 'native PE header metadata changed'
    $notice=Get-NoticeMetadata $data;Assert-Hexagon ($notice.encodingObservation.basis -ceq 'validated-PE-header; text-decoding-not-attempted' -and -not $notice.licenseRightsInterpretationReviewed) 'PE was treated as a license grant'
    [Array]::Copy([BitConverter]::GetBytes([uint]4294967295),0,$data,60,4);Assert-Hexagon (-not (Get-PeMetadata $data).validPeHeader) 'out-of-bounds PE accepted'
}
Invoke-HexagonTests $tests $Test
