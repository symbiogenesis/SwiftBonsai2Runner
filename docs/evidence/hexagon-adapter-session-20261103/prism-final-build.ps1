$ErrorActionPreference = 'Stop'
. 'C:\Users\symbi\AppData\Local\Temp\SwiftBonsai-Npu-Adapter-20261103\build-env.ps1'
cargo build --offline --locked --release --target aarch64-pc-windows-msvc --target-dir 'C:\Users\symbi\AppData\Local\Temp\SwiftBonsai-Hmx-Session-20261103\prism-target' -p bonsai-hexagon-runtime --features experimental-neon,parallel-rows --bin prism-hexagon-model-probe
exit $LASTEXITCODE
