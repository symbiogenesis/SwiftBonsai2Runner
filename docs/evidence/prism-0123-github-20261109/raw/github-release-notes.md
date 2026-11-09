Prism Inference 0.1.23 delivers the CPU inference efficiency work after 0.1.22: direct floating projections, compact rollback checkpoints, validated-logit sampling, masked-attention traversal, recurrent staging and bounded suffix matching. It also includes further native-Q2 resident queue work. Source reductions alone do not establish measured speed or device execution.

The Windows ARM64 bundle contains the five executables and four license files built from `055c7430cb4771dfddb24ddaa30494af6ad182da` using `nightly-2026-11-09-aarch64-pc-windows-msvc`.

Validation: [Foundation bundle run 42721996680, attempt 1](https://github.com/symbiogenesis/prism-inference/actions/runs/42721996680) passed nine receipt/version controls, formatting, strict Release all-target analysis, portable library compilation, production linking and help smoke checks. The artifact and all nine bundled file identities were independently verified against its source-bound producer receipt. Full Foundation, actual-model and performance qualification remain pending.

Archive: `prism-inference-0.1.23-aarch64-pc-windows-msvc.zip`

SHA-256: `bc792fdd6044ff4392a8aedbe241f634bd4492ce086d62dcca392f8d993725ee`

The private server and accelerator packages remain unpublished on crates.io. The five unchanged registry helpers retain their existing versions. Applications must verify the new immutable source, archive and executable identities before selecting this bundle. Explicit representations, adapters and experimental paths retain their admission and qualification requirements.
