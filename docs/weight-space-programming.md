# Weight-space programming: Swift applicability

The 2026-11-02 assessment finds a plausible small-patch mechanism, but no
defensible Swift weight upgrade yet. Useful answers, creative quality,
instruction following, speed and memory are all eligible benefits. No particular
failure is assumed, no candidate weights were invented, and no runtime feature
or default changed. This is a technical assessment; [PLAN.md](../PLAN.md) owns
outstanding work.

## What the source establishes

[Takehi et al., *Sparks of Weight-Space Programming*](https://rikiyat.github.io/blog/weight-space-programming/)
reports a targeted edit to Qwen3.5-0.8B with thinking enabled: removing a direction
from selected FFN output columns reduced reasoning loops and improved held-out
math and instruction-following scores. Its largest gain used 50 practice-set forward-pass calls,
each potentially covering many sequences, with a 32,768-token evaluation limit.
The negative control matters: edits to Gemma 3 270M, whose failures were mostly
ordinary reasoning mistakes rather than loops, produced no established math gain.
This supports investigating concentrated defects, not a general intelligence
upgrade or a predictable 80/20 return.

Swift has different weights, dimensions and 64 layers. The source's layer numbers,
channel selection and correction direction cannot be transferred directly.
Runner's [defaults](inference-defaults.md) also differ: thinking starts disabled,
and the completion/context limits are 4,096/8,192 tokens. Existing retained
application evidence does not diagnose an equivalent Swift loop defect.

## A shared patch without rewriting the packed models

For a correction sharing one residual direction, write `delta_W = -d * c^T`,
where `d` is the output direction and `c` contains coefficients for selected FFN
input channels. Its application is `y = W*x - d*(c^T*x)`. This is a rank-one
adapter: F32 A contains `c`, and F32 B contains `-d`, with alpha one and patch
scale one. These are independent of the existing Orca scale, which remains 1.3.

Swift's FFN-down shape is `[17408,5120]` in GGUF order. One pair therefore has
`4 * (17408 + 5120) = 90,112` payload bytes before container overhead. This is
small distribution/storage cost, not proof of negligible inference overhead.
The same factor bytes can target original PQ2 and the exact
[current-weight PTQ1 repack](swift-memory.md#current-weight-ptq1-option), whose
[export receipt](../config/swift.ptq1-export.json) records exact reconstructed
weight parity. The older publisher PTQ1 checkpoint is outside that claim;
activation arithmetic and behavioral qualification remain representation-specific.

Swift's base projections include folded Hadamard transforms. Factors must use
the original activation basis on the existing independent adapter branch;
editing stored packed columns as if they were unfolded neurons is incorrect.
An FFN-only candidate avoids the separate grouped-SSM authoring ambiguity but
still needs a reproducible Swift-specific direction and channel mapping.

## Existing implementation and delivery limits

The clean sibling Prism checkout was inspected at
[`fca1e0bb04f418a4103e39c0eaf7171cd5cd46fc`](https://github.com/symbiogenesis/prism-inference/tree/fca1e0bb04f418a4103e39c0eaf7171cd5cd46fc):

- [`RankOneAdapter::from_model`](https://github.com/symbiogenesis/prism-inference/blob/fca1e0bb04f418a4103e39c0eaf7171cd5cd46fc/src/model/lora.rs#L125)
  accepts sparse supported FFN target pairs after metadata, shape and finite-value checks.
- [`Qwen35::with_rank_one_adapter`](https://github.com/symbiogenesis/prism-inference/blob/fca1e0bb04f418a4103e39c0eaf7171cd5cd46fc/src/graph.rs#L951)
  applies the correction and binds adapter identity and scale into graph state.
- [`SwiftSession` loading](https://github.com/symbiogenesis/prism-inference/blob/fca1e0bb04f418a4103e39c0eaf7171cd5cd46fc/src/swift_session.rs#L144)
  admits only the exact approved Orca artifact. The graph has one adapter slot;
  an independent correction plus Orca generally requires rank two at overlapping
  sites or explicit multiple-adapter support. General constructor support does
  not make an arbitrary candidate loadable through Runner's released service.

## Value and evidence boundary

The existing [LoRA capture and review path](swift-lora.md#standard-profile-qualification)
can supply useful controls without a new generic patch framework. Its fixed
greedy/512-token controls do not establish benefit at app sampling and output
settings. Exact template and effective thinking settings must be retained: the
[completion renderer](inference-defaults.md#applied-defaults) currently omits
template kwargs, whereas server requests apply the selected settings.

A smaller patch alone establishes neither better answers nor speed. Lowering the
4K/8K caps, removing Orca, or accepting early/incomplete answers cannot count as a
gain. A candidate must improve completed-answer usefulness or measured cost under
matched controls while preserving independently reviewed quality and rollback.
The owner's aggregate five-minute allowance includes setup and cleanup; full
discovery matrices and benchmarks remain CI-only. Current startup/lifecycle
[evidence](evidence/ptq1-delivery-0112-20261101.json) supplies no feasible local
weight-discovery matrix. A shipped patch would require new causal and held-out
evidence; this assessment provides neither a trained direction nor a proven win.
