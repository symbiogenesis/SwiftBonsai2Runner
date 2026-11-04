$ErrorActionPreference = 'Stop'
$taskToolRoot = Join-Path ([IO.Path]::GetTempPath()) 'Bonsai-Codex-Tools'
$env:CARGO_TARGET_AARCH64_PC_WINDOWS_MSVC_LINKER = Join-Path $env:USERPROFILE '.rustup\toolchains\nightly-2026-10-22-aarch64-pc-windows-msvc\lib\rustlib\aarch64-pc-windows-msvc\bin\rust-lld.exe'
$env:LIB = (Join-Path $taskToolRoot 'arm64-crt-onecore\Contents\VC\Tools\MSVC\14.51.36231\lib\onecore\arm64') + ';' + (Join-Path $taskToolRoot 'arm64-crt\Contents\VC\Tools\MSVC\14.51.36231\lib\arm64') + ';' + (Join-Path $taskToolRoot 'sdk\lib\arm64')
$env:RC_PATH = Join-Path $taskToolRoot 'sdk-buildtools\bin\10.0.26100.0\arm64\rc.exe'
