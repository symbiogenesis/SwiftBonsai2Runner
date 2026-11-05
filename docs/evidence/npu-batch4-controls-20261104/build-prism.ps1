$ErrorActionPreference = 'Stop'
. 'C:\Users\symbi\source\repos\SwiftBonsai2Runner\docs\evidence\hexagon-adapter-session-20261103\build-env.ps1'
$env:CARGO_TARGET_DIR = 'C:\Users\symbi\AppData\Local\Temp\SwiftBonsai-Hmx-Session-20261103\prism-target'
cargo fmt --check
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
cargo test --offline --locked --release --target aarch64-pc-windows-msvc -p bonsai-hexagon-native --lib
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
cargo test --offline --locked --release --target aarch64-pc-windows-msvc -p bonsai-hexagon-runtime --features experimental-neon,parallel-rows --lib
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
cargo test --offline --locked --release --target aarch64-pc-windows-msvc -p bonsai-hexagon-runtime --features experimental-neon,parallel-rows --bin prism-hexagon-model-probe
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
cargo clippy --offline --locked --release --target aarch64-pc-windows-msvc -p bonsai-hexagon-native -p bonsai-hexagon-runtime --features experimental-neon,parallel-rows --lib --bin prism-hexagon-model-probe -- -D warnings
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
cargo build --offline --locked --release --target aarch64-pc-windows-msvc -p bonsai-hexagon-runtime --features experimental-neon,parallel-rows --bin prism-hexagon-model-probe
exit $LASTEXITCODE
