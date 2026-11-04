$ErrorActionPreference = 'Stop'
. 'C:\Users\symbi\source\repos\SwiftBonsai2Runner\docs\evidence\hexagon-adapter-session-20261103\build-env.ps1'
$env:CARGO_TARGET_DIR = 'C:\Users\symbi\AppData\Local\Temp\SwiftBonsai-Hmx-Session-20261103\prism-target'
cargo test --offline --locked --release --target aarch64-pc-windows-msvc -p bonsai-hexagon-runtime --features experimental-neon,parallel-rows --bin prism-hexagon-model-probe model_probe_timing_windows_keep_native_wait_and_oracle_separate
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
cargo test --offline --locked --release --target aarch64-pc-windows-msvc -p bonsai-hexagon-runtime --features experimental-neon,parallel-rows --bin prism-hexagon-model-probe model_probe_admits_complete_cpu_snapshot_retention_before_model_work
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
cargo build --offline --locked --release --target aarch64-pc-windows-msvc -p bonsai-hexagon-runtime --features experimental-neon,parallel-rows --bin prism-hexagon-model-probe
exit $LASTEXITCODE
