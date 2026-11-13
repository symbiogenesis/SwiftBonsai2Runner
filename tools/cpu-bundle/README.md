# Native CPU bundle policy

[`cpu_bundle.ps1`](cpu_bundle.ps1) prepares the fixed original Prism 0.1.23 and
registry 0.1.25 collectors on owning hosted Windows ARM64 CI. It reads the exact
published Prism Rust collector source, verifies Cargo registry archives and
locked dependency subsets, runs the declared positive controls, and creates a
schema-2 packet with a digest-bound PowerShell evaluator. No Python runtime is
installed or executed.

`-Operation prepare` accepts `-BaselineRoot`, `-ToolingRoot`,
`-CandidateRegistryVersion 0.1.25`, `-RunnerRevision` and a new absolute `-Output`.
It refuses local preparation before invoking Cargo or loading a model.
`-Operation evaluate -Preparation ABSOLUTE_JSON -Owner ABSOLUTE_JSON` admits the
complete fixed ABBA receipt set offline. It validates source/artifact identity,
complete output, the original deadline, native host/power, resources and cleanup.
Passing admission never qualifies promotion or changes defaults. New packets
retain `preparation.json`, `messages.json` and `evaluator.ps1`; schema-1 packets
remain historical.

Use one named model-free check locally, for example
`pwsh -NoLogo -NoProfile -File tools/cpu-bundle/test_cpu_bundle.ps1 -Test archive`.
The full fixture inventory and complete hosted preparation belong to the owning
[workflow](../../.github/workflows/cpu-bundle-comparison.yml). The owner retains
dispatch, device execution and CI verdicts under the
[comparison contract](../../docs/cpu-bundle-comparison.md).
