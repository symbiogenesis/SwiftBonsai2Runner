Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$evidence = Join-Path $env:RUNNER_TEMP ('Native-Acceptance-Analyzers-' + [guid]::NewGuid().ToString('N'))
"BONSAI_NATIVE_ANALYZER_EVIDENCE=$evidence" >> $env:GITHUB_ENV
# This hosted prerequisite closes no model/device gate. In particular,
# it cannot extend the closed local UI-analyzer validation session.
& $env:BONSAI_CI_PATH rust-check --root $env:GITHUB_WORKSPACE --package bonsai-chat --package bonsai-broker --package bonsai-tools --phase clippy --phase build --toolchain nightly-2026-11-02-aarch64-pc-windows-msvc --target (Join-Path $env:RUNNER_TEMP 'Native-Acceptance-Build') --evidence $evidence --timeout-seconds 1200
if ($LASTEXITCODE -ne 0) { throw 'Hosted strict Release analyzers or native executable linking failed; no owner-device acceptance package is admitted.' }
