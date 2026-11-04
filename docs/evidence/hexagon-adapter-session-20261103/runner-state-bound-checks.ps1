$ErrorActionPreference = 'Stop'
. 'C:\Users\symbi\AppData\Local\Temp\SwiftBonsai-Npu-Adapter-20261103\build-env.ps1'
$env:CARGO_TARGET_DIR = 'C:\Users\symbi\AppData\Local\Temp\SwiftBonsai-Hmx-Session-20261103\runner-target'
rustfmt --edition 2024 tools/bonsai/src/hexagon_tools/model_device.rs
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
cargo test --locked --offline --release -p bonsai-tools --lib hexagon_tools::model_device::tests
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
cargo clippy --locked --offline --release -p bonsai-tools --lib -- -D warnings
exit $LASTEXITCODE
