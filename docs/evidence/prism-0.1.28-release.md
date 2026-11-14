# Prism 0.1.28 release delivery

Runner's [runtime lock](../../config/prism-inference.lock.json) selects the
[0.1.28 prerelease](https://github.com/symbiogenesis/prism-inference/releases/tag/v0.1.28).
This record establishes source-bound binary delivery and integrity, not full
Foundation, actual-model, quality, resource, isolation or performance acceptance.

| Identity | Value |
| --- | --- |
| Source/tag commit | `99671be7ab2be01e406a11b0c3d6caede398d96f` |
| Target/compiler | `aarch64-pc-windows-msvc`, `nightly-2026-11-09-aarch64-pc-windows-msvc` |
| Build | [Foundation bundle 43575647298, attempt 1](https://github.com/symbiogenesis/prism-inference/actions/runs/43575647298) |
| Operation | `daily-deadline-f61b02bd3203` |
| Runtime ZIP | 6,311,275 bytes; SHA-256 `2e789c53fb6ce82c34558a96e7cda5defba87b6bae4307772dbeb282ecb4864c` |
| Exact source ZIP | 5,140,796 bytes; SHA-256 `12eb6a3ecc9bc7ae28ed0be93bf3be86807a7435168fd88b1f4871ce2719cbcd` |
| Original bundle receipt | 1,979 bytes; SHA-256 `5b3510f0bd3c404b50a2622f0f24748914e9289cd7ef15bead28421dc9eff650` |
| Local integrity verifier | Cached `bonsai-tools.exe`, SHA-256 `391048ad7cab1f4cc884fb6e7482d390181cd71ee9c6884c6e553ee9261225c2` |

The dedicated hosted lane completed successfully: formatting, strict Release
analyzers, scalar compilation, nine receipt/version controls, five PE-import
controls, two unwind/drain controls, six scoped-context controls, production
linking and five executable help probes. It reported no external VC-runtime
imports. It explicitly left full Foundation, model acceptance and performance
unqualified. The run used GitHub's hosted execution cap; no local operator or
compiler timeout was imposed and no build was cancelled for elapsed wall time.

The native `inference-lock` verifier checked the selected run/revision/attempt,
receipt, source manifests/compiler, archive identity, all nine executable/license
entries, lengths, full SHA-256 and ARM64 headers. The tag was independently checked
against the selected commit. Both uploaded assets were downloaded from the draft
and compared byte-for-byte by SHA-256 before publication. After publication,
the ZIP and receipt were downloaded from the immutable release URLs and passed
the native verifier again before replacing Runner's lock. No runtime executable
or model was run during this local artifact verification.

The runtime and original receipt remain release assets; the exact source URL and
every installed-file identity are in the lock. Those selected bytes plus the
receipt are the replay inputs for the
[lock-update procedure](../development-guide.md#prepare-a-runtime-lock-update).
Successful build chatter and duplicate ZIP/source copies are not repository
evidence. Preserve any required independent replay inputs under
[retention policy](../validation.md#evidence-retention).

The release removes the ordinary private HTTP generation cutoff while retaining
explicit diagnostic deadlines, cancellation and bounded intake/writes. Readiness
now waits for model-thread initialization and reports `startup_projection`.
Runner binds the new capability tuples and requires that observation. K4/V3 and
fitted-value precision remain outside Runner admission.

Final-source Runner compiler/analyzer and named regression acceptance, actual
long-prefill/Stop/Retry, full-state/logits/adapters, signed-device placement,
resources, independent creative/speech review and clean-machine acceptance remain
pending in [PLAN.md](../../PLAN.md). Prior 0.1.27 receipts retain their original
identities and verdicts. For a source rollback, recover its complete lock from
Runner commit `250c3e9b`, restore its matching capability guards and rebuild all
three lock-embedding consumers; changing a version string alone is insufficient.
