$ErrorActionPreference = 'Stop'
. 'C:\Users\symbi\source\repos\SwiftBonsai2Runner\docs\evidence\hexagon-adapter-session-20261103\build-env.ps1'
$env:CARGO_TARGET_DIR = 'C:\Users\symbi\AppData\Local\Temp\SwiftBonsai-Hmx-Session-20261103\runner-target'
cargo fmt --check
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
cargo test --offline --locked --release -p bonsai-tools --lib hexagon_tools::model_device::tests
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
cargo clippy --offline --locked --release -p bonsai-tools --lib -- -D warnings
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
cargo build --offline --locked --release -p bonsai-tools --bin bonsai-tools
exit $LASTEXITCODE
