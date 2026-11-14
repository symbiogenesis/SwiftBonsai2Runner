#Requires -Version 7.6
# Historical fixture selector; semantic assertions live beside the native implementation.
param([ValidateSet('test_comments_do_not_consume_literal_or_include_contents',
    'test_macro_function_adjacency_and_directive_ends_remain_distinct',
    'test_line_splices_precede_comment_recognition','test_literal_prefix_adjacency_is_not_whitespace',
    'test_typedef_callback_enum_and_method_candidates_preserve_tokens',
    'test_unknown_source_syntax_preserves_pending_state','test_known_boms_and_uncertain_windows_text_fallback',
    'test_cmake_recipe_keeps_balanced_tokens_and_pending_authority',
    'test_sdk_pe_and_notice_metadata_never_grant_rights','test_metadata_bounds_and_duplicate_json_are_fail_closed')][string]$Test)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$target = if ($env:RUNNER_TEMP) { Join-Path $env:RUNNER_TEMP 'Bonsai-Sdk-Reader-Checks' } else { Join-Path ([IO.Path]::GetTempPath()) 'Bonsai-Sdk-Reader-Checks' }
$arguments = @('test','--locked','--release','--manifest-path',(Join-Path $PSScriptRoot 'sdk-reader/Cargo.toml'),'--target-dir',$target)
if ($Test) { $arguments += @("functional::tests::$Test",'--','--exact') } else { $arguments += 'functional::tests::' }
& cargo @arguments
exit $LASTEXITCODE
