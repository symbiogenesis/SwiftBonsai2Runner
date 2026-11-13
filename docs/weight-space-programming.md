# Weight-space programming: Swift applicability

The 2026-11-11 assessment recommends complete-answer diagnosis and ordinary
controls first, with small runtime corrections contingent on a reproducible
Swift-specific fault. Swift/Bonsai 27B remains the priority on the owner's
Snapdragon X2 Elite Extreme laptop with 48 GB RAM. No beneficial Swift correction
or equivalent reasoning-loop defect is established, and no runtime feature,
artifact, default or admission limit changed. This is a technical assessment;
[PLAN.md](../PLAN.md) owns outstanding work.

## What the three sources establish

[Takehi et al., *Sparks of Weight-Space Programming*](https://rikiyat.github.io/blog/weight-space-programming/)
reports a targeted Qwen3.5-0.8B edit with thinking enabled: subtracting a direction
from selected FFN output columns reduced reasoning loops and improved held-out
math and instruction-following scores. Its largest gain used 50 practice-set
forward-pass calls, potentially covering many sequences, and a 32,768-token
evaluation limit. Editing Gemma 3 270M, whose failures were mostly ordinary
reasoning mistakes, produced no established math gain. This supports studying a
concentrated defect, rather than assuming a general capability upgrade.

[Barry, *Can a Linear Program Do Weight-Space Programming?*](https://nathan.rs/posts/weight-space-lp/)
replaces the neuron/strength search with a sparse linear program using recorded
activations and a supplied loop direction. A heuristic direction from labelled
traces recovered much of the result, but the layer selection still came from the
earlier experiment. A constant residual steering vector recovered most of the
anti-loop benefit without editing weights. Its strength window was narrow:
weaker steering left loops; stronger steering harmed reasoning. The
[reference implementation](https://github.com/nathanrs/weight-space-lp) uses
MPS/CUDA for fitting and cloud GPUs for evaluation; a fast solve is not the cost
of collecting activations, choosing candidates and validating them end to end.

[Beaker, *Adventures in Weight Space Programming*](https://www.withbeaker.ai/blog/weight-space-programming)
demonstrates an automated propose, score and retain loop on the same small-model
problem. Its matched comparison is particularly useful: a simple ban on `Wait`
tokens scored 37.6 versus 36.6 for the single-neuron edit in that harness.
Shorter-answer edits also substantially harmed accuracy. These controls weaken
the case for starting with weight manipulation or rewarding brevity alone.
They do not establish that banning a token is a suitable Swift behavior policy.

All three are evidence about their tested models and settings. Swift has
different weights, dimensions and 64 layers. Directions, neuron selections,
layer IDs and strengths cannot be copied across. Runner's
[defaults](inference-defaults.md) also differ: thinking starts disabled, and the
active native release's completion/context limits are 4,096/8,192 tokens.
Publisher serving selects its context from available memory; neither path
reproduces the cited 32,768-token output allowance. Existing application evidence
does not diagnose the same defect.

## Practical investigation order

The first useful input is existing complete reasoning/answer captures and human
review, with exact model, runtime, template, effective thinking, sampler,
adapter and termination identities. Check ordinary prompt/template/sampling
controls against any concentrated failure before adding instrumentation. A
short prefix, a changed answer or a repetition count alone does not establish a
correctable fault. A token-blocking baseline, if relevant to a diagnosed fault,
belongs in a controlled comparison rather than a new default.

If that evidence justifies intervention, the simplest candidate is an optional,
disabled-by-default constant residual correction, `x_next = x + beta*d`, at an explicitly identified
layer and operation boundary. For Swift's width 5,120, one F32 vector is
`4 * 5120 = 20,480` bytes, or 20 KiB per site, before metadata. A separate
residual branch could preserve the standard Orca adapter at scale 1.3 without
consuming its existing rank-one slot or rewriting packed weights. This is a
proposed design, not an existing API or a proven Swift benefit. Constant steering
is a baseline; it cannot reproduce every activation-dependent correction.

Direction discovery would need bounded capture of selected residual/MLP sites
and token samples, with pooled statistics where sufficient. Capturing every
activation is unnecessary: two layers over 4,096 tokens, retaining both 5,120-wide
residuals and 17,408-wide FFN activations in F32, already requires 704 MiB per
sequence before overhead. Capture limits and evidence retention must be explicit.

Sparse LP fitting is a later offline authoring option when useful directions and
activation data exist. Composed rank-one corrections are justified only if they
offer a measured advantage over simpler controls or express a needed conditional
effect. No generic optimizer, Python inference service or hosted Beaker dependency
is needed in the desktop application. Production correction application would
remain native Rust; offline research tools stay outside the presentation and
serving paths.

## A shared patch without rewriting the packed models

For a correction sharing one residual direction, write `delta_W = -d * c^T`,
where `d` is the output direction and `c` contains coefficients for selected FFN
input channels. Its application is `y = W*x - d*(c^T*x)`. This is a rank-one
adapter: F32 A contains `c`, and F32 B contains `-d`, with alpha one and patch
scale one. Those settings are independent of the existing Orca scale of 1.3.

Swift's FFN-down shape is `[17408,5120]` in GGUF order. One pair has
`4 * (17408 + 5120) = 90,112` payload bytes, or 88 KiB, before container overhead.
Small factor/vector storage is not evidence of negligible runtime overhead,
lower peak RAM or improved throughput. The same factor bytes can target original
PQ2 and the exact [current-weight PTQ1 repack](swift-memory.md#current-weight-ptq1-option),
whose [export receipt](../config/swift.ptq1-export.json) records exact reconstructed
weight parity. The older publisher PTQ1 checkpoint is outside that claim;
activation arithmetic and behavior remain representation-specific.

Swift's base projections include folded Hadamard transforms. Factors must use
the original activation basis on an independent correction branch; editing
packed columns as if they were unfolded neurons is incorrect. FFN-only candidates
avoid the separate grouped-SSM authoring ambiguity but still require reproducible
Swift-specific directions and channel mappings. Constant steering must likewise
name its exact residual basis and insertion site.

## Existing implementation and delivery limits

The sibling Prism source inspected on 2026-11-11 is local main
[`a1f82b5cf0d54190290972908a61f333eb5262d9`](https://github.com/symbiogenesis/prism-inference/tree/a1f82b5cf0d54190290972908a61f333eb5262d9),
version 0.1.24. At that inspection, Runner pinned 0.1.23 at
`055c7430cb4771dfddb24ddaa30494af6ad182da`; the
[historical capture](evidence/readme-performance-0123-20261109/summary.json)
retains that baseline. The [active binary/source lock](../config/prism-inference.lock.json)
now selects 0.1.25 at `99bab71270b1e6a6fb6d47eec2e34b8097cdcd4c`.
The observations below describe the inspected source, not an acceptance verdict
for either binary release.

- [`RankOneAdapter::from_model`](https://github.com/symbiogenesis/prism-inference/blob/a1f82b5cf0d54190290972908a61f333eb5262d9/src/model/lora.rs#L129)
  accepts sparse supported FFN target pairs after metadata, shape and finite-value checks.
- [`Qwen35::with_rank_one_adapter`](https://github.com/symbiogenesis/prism-inference/blob/a1f82b5cf0d54190290972908a61f333eb5262d9/src/graph.rs#L1980)
  applies corrections in the original activation basis and seals adapter identity
  and scale bits into graph state. Scale changes clear state and invalidate checkpoints.
- [`SwiftSession` loading](https://github.com/symbiogenesis/prism-inference/blob/a1f82b5cf0d54190290972908a61f333eb5262d9/src/swift_session.rs#L146)
  admits only the exact approved Orca artifact. The graph has one adapter slot;
  an independent correction plus Orca generally needs rank two at overlapping
  sites or explicit multiple-adapter support. Generic constructor support does
  not make arbitrary candidates loadable through Runner's released service.
- [`visit_state`](https://github.com/symbiogenesis/prism-inference/blob/a1f82b5cf0d54190290972908a61f333eb5262d9/src/graph.rs#L1903)
  observes committed KV, recurrent and convolution state. It is not a residual
  stream or MLP-activation capture hook. No steering or corresponding capture API
  was found in the inspected source.

## Candidate identity and qualification boundary

Any candidate artifact needs exact base-model and representation identities,
tensor/layer/site and activation-basis mappings, factor/vector hashes, finite
shape-checked values, exact scale bits and deterministic composition order.
Unsupported sites or dimensions must reject before application.
Runtime source/executable identity and the existing adapter identity/scale also
belong in the retained profile and evidence. Changed correction settings must
clear live state and invalidate prefix/checkpoint reuse; loaded zero must bypass
correction arithmetic exactly. Rejected artifacts must leave the previous
profile intact, with explicit rollback to the original profile and Orca retained.

The operation must agree across prompt prefill, decode, continuation, reset,
cancellation/retry and every admitted CPU/accelerator placement, including fused
paths. Capture must observe the declared site consistently. A synthetic hook or
CPU-only numerical pass does not qualify tiled prefill, NPU execution or the
complete application. PQ2 and current-weight PTQ1 require independent arithmetic,
state, resource and behavioral acceptance while preserving the standard adapter.

The existing [LoRA capture and review path](swift-lora.md#standard-profile-qualification)
provides useful controls. Its fixed greedy/512-token settings do not establish
benefit at application sampling/output settings. Retain exact template and
effective thinking settings: the [completion renderer](inference-defaults.md#applied-defaults)
currently omits template kwargs, whereas server requests apply selected settings.

Require held-out complete-answer correctness, instruction following and creative
quality, independent human review, and matched latency/throughput/resource
observations. Measure useful-answer time separately from tokens/second. Lowering
4K/8K limits, removing Orca, accepting incomplete answers or rewarding shorter
output alone cannot count as benefit. Discovery and tuning data must stay
separate from held-out evaluation; freeze the candidate before held-out scoring.
No source establishes a Swift speed, memory or quality improvement.

## Laptop and future-model scope

Keep Swift/Bonsai 27B quantized: its pinned original PQ2 file is
[6.71 GiB](swift-memory.md#pinned-artifacts). Expanding roughly 27 billion weights
to FP16/BF16 would require about 54 GB before state, scratch and Windows, beyond
this laptop's 48 GB RAM. Sparse sidecar vectors/factors avoid that expansion;
activation capture and device copies still need accounting. Preserve current
admission, Job and physical-reserve policies; payload arithmetic does not measure
whole-process residency or establish NPU benefit.

Follow the [owner-device budget and power rules](../AGENTS.md#owner-device-execution-budgets):
five minutes total by default, with only a predeclared justified extension up to
fifteen minutes. Narrower harness limits and closed historical clocks still apply;
complete suites and benchmarks remain owner-operated CI work. This assessment
authorizes no new model workload or CI operation.

Future PrismML/Bonsai models, including a future Qwen4-derived release, and later
smaller checkpoints can reuse the investigation, artifact and correction
infrastructure. Their weights, architecture, tokenizer/template, dimensions and
supported sites need fresh compatibility review; directions, factors, strengths
and quality claims must be derived and qualified anew. Infrastructure reuse is
not transfer of an existing edit, and the present 27B priority does not depend on
adopting a smaller model first.
