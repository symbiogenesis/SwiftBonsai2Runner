Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$env:CARGO_TARGET_DIR = Join-Path $env:RUNNER_TEMP 'Native-Acceptance-Build'
foreach ($case in @('chat_launch::tests::graphics_backend_selection_is_explicit_and_scoped_to_the_chat_child',
    'chat_launch::tests::graphics_backend_selection_rejects_unsupported_values_before_startup',
    'cli::tests::graphics_backend_option_preserves_explicit_selection_and_rejects_duplicates',
    'chat_launch::tests::development_default_and_explicit_rollback_survive_deferred_preparation')) {
  $output = @(& cargo +nightly-2026-11-02 test --locked --release --manifest-path (Join-Path $env:GITHUB_WORKSPACE 'Cargo.toml') --package bonsai-tools --lib $case -- --exact)
  $output | Write-Output
  if ($LASTEXITCODE -ne 0 -or -not ($output -cmatch '^test result: ok\. 1 passed; 0 failed; 0 ignored;')) { throw 'Exact renderer/default-selection control failed or was not executed.' }
}
$output = @(& cargo +nightly-2026-11-02 test --locked --release --manifest-path (Join-Path $env:GITHUB_WORKSPACE 'Cargo.toml') --package bonsai-broker --lib owner_shutdown_)
$output | Write-Output
if ($LASTEXITCODE -ne 0 -or -not ($output -cmatch '^test result: ok\. [1-9][0-9]* passed;')) { throw 'Private owner shutdown controls failed or none were executed.' }
$output = @(& cargo +nightly-2026-11-02 test --locked --release --manifest-path (Join-Path $env:GITHUB_WORKSPACE 'Cargo.toml') --package bonsai-chat --bin bonsai-chat owned_broker_)
$output | Write-Output
if ($LASTEXITCODE -ne 0 -or -not ($output -cmatch '^test result: ok\. [1-9][0-9]* passed;')) { throw 'Native UI owner shutdown controls failed or none were executed.' }
