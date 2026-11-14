# Weight-space programming: Swift applicability

Weight corrections are conditional research, not an active Swift feature. Start
with complete-answer diagnosis and ordinary prompt/template/sampler controls.
No beneficial Swift correction or matching reasoning-loop defect is established.
[PLAN.md](../PLAN.md) owns outstanding work; [inference defaults](inference-defaults.md)
define the selected model, adapter, Thinking and completion limits.

## Research boundary

[Sparks of Weight-Space Programming](https://rikiyat.github.io/blog/weight-space-programming/)
studies targeted small-model edits for concentrated failures.
[The linear-program follow-up](https://nathan.rs/posts/weight-space-lp/)
also finds a constant residual steering baseline useful, with a narrow strength
window. [Beaker's experiments](https://www.withbeaker.ai/blog/weight-space-programming)
show why ordinary token/output controls belong in the comparison and why shorter
answers alone cannot count as improved reasoning. These reports concern other
models and settings; directions, layers and strengths cannot transfer to Swift.

Swift defaults to Thinking off, a 4,096-token completion cap and memory-selected
context. It does not reproduce the cited long-reasoning workloads. A changed
answer or repeated token alone does not diagnose a correctable defect.

## Practical investigation order

Use existing complete reasoning/answer captures and independent human review,
with exact model/runtime/template, sampler, adapter, effective Thinking and
termination identities. Diagnose a reproducible concentrated failure before
capturing activations or fitting a correction. Keep tuning data separate from
held-out evaluation and freeze candidates before scoring.

If justified, a disabled-by-default constant correction `x_next = x + beta*d`
is a simple baseline at a named residual site. A width-5,120 F32 direction costs
20,480 bytes (20 KiB) per site before metadata. Its small payload does not prove
negligible latency or process-memory overhead. Activation capture must name
sites/tokens and bound scratch; capturing two full residual/FFN layers over
4,096 tokens already costs 704 MiB before overhead.

Sparse fitting and activation-dependent factors are later options only when
they outperform simpler controls. Production application stays native Rust;
no hosted fitting service or Python inference dependency belongs in the app.

## A shared patch without rewriting the packed models

For one output direction, `delta_W = -d * c^T` applies as
`y = W*x - d*(c^T*x)`. F32 A contains `c`, B contains `-d`, alpha and patch scale
are one, independently of the standard Orca scale 1.3. Swift FFN-down has GGUF
shape `[17408,5120]`; one factor pair costs `4*(17408+5120) = 90,112` bytes (88 KiB).

The same factors may target original PQ2 and the exact [current-weight PTQ1 repack](swift-memory.md#current-weight-ptq1-option),
whose reconstructed weights match. Activation arithmetic, state, resources and
behavior still qualify independently. Folded Hadamard base columns cannot be
edited as if they were unfolded neurons: corrections need their own original
activation basis. FFN-only sites avoid the separate grouped-SSM authoring ambiguity.

## Existing implementation and delivery limits

### Offline fitting and an explicit native research path

`bonsai-tools weight-patch` provides offline `direction` and `fit` operations.
It does not load a model, change the approved adapter, install a candidate or
enable a serving feature. The corresponding experimental Prism FFN probe is
source-only until its owning runtime release is reviewed and published; the
Runner runtime pins do not change for this investigation.

The intended composition at a selected FFN-down site is
`y = W*h + standard_adapter(h) + u*(v^T*h)`. Both adapter branches consume
the original, post-gating FFN activation `h`, before any Hadamard transform.
The new patch is an independent additive correction, not a network that consumes
the existing adapter's output. Swift's published correction is already merged
into the selected base artifact; Runner loads no separate Swift adapter.

Given an independently supplied unit direction `u`, the fitter chooses `v` for
the soft-constrained linear program:

```text
min lambda*sum(abs(v))
    + targetPenalty*sum(max(0, h_target*v + delta))
    + preservePenalty*sum(max(0, abs(h_preserve*v) - epsilon))
```

`direction` can instead estimate `u` from the difference between mean target
and preserve residual vectors in the fitting split. Held-out rows never
contribute to that estimate or the fit. Keep answer groups disjoint between
splits; distinct IDs cannot establish independent content or prevent manual
tuning against a holdout. Direction estimation is not a diagnosis of a defect.

The bounded CPU solver uses primal-dual hybrid gradient steps and reports a
feasible dual bound and primal-dual gap. Candidate certification applies to
the exported F32 factors. Exhausted work/time limits, nonconvergence and a
no-op remain explicit outcomes. A numerical certificate concerns this captured
linear objective only: it proves neither complete-answer quality nor how later
activations change after the patch is applied.

Requests bind exact model/runtime/adapter hashes, adapter scale bits, runtime
source and execution profile, graph identity, FFN site and activation basis.
Rows name their sample, answer group, split and target/preserve label. Factors
retain their own hashes and deterministic composition order. This command accepts
at most 48 MiB of JSON, allowing the probe's bounded 32 MiB capture plus metadata;
retain only deliberately selected rows rather than full-layer activation dumps.

```powershell
bonsai-tools weight-patch --request C:\Research\direction-request.json
bonsai-tools weight-patch --request C:\Research\fit-request.json
```

Start with a reproducible complete-answer failure and freeze its ordinary
controls before collecting activations. Compare a fitted patch with a constant
steering baseline at the same site. Do not transfer the cited Qwen model's
directions, layers or strengths to Swift. Capture on a dedicated experimental
CPU path does not qualify the application's fused, batched or accelerator paths.

### Released service

Runner loads only the approved adapter from its compiled locks. The generic
rank-one graph APIs do not authorize an arbitrary correction through the released
service. Composing an independent patch with Orca at overlapping sites requires
an explicit multi-adapter or higher-rank design, complete identity handling and
a reviewed source-bound release. Committed state observation is not an activation
capture contract; a proposed hook must name exactly what it observes.

## Candidate identity and qualification boundary

Bind base representation, runtime, tensor/layer/site, activation basis,
factor/vector hashes, finite shapes, exact scale bits and deterministic composition
order. Unsupported inputs reject before application and preserve the previous
profile. Changes clear live state and invalidate checkpoints/prefix reuse; loaded
zero bypasses correction arithmetic exactly. Retain explicit rollback with Orca.

Require matching prompt/decode/continuation/reset and cancellation behavior on
every selected CPU/accelerator path, including fused work. Synthetic or CPU-only
success cannot qualify NPU/tiled execution or the complete app. Reuse applicable
[LoRA controls](swift-lora.md#standard-profile-qualification), while retaining
their fixed-workload limits. Held-out complete-answer correctness, instruction
following, creative quality and independent human review remain mandatory.
Measure useful-answer latency, resources and comfort with matched power; shorter
or incomplete output cannot supply benefit.

## Laptop and future-model scope

### Performance and memory pilot

Adding an independent patch adds factor storage and arithmetic. The LP above
fits a behavioral correction; it does not directly minimize runtime or memory.
For an initial adapter-cost screen, Prism's source-only `swift_sanity_check`
example runs one fixed arm: original PQ2, four CPU workers, K4/V4 cache, 256-token
context, seed 42, Thinking off and no prompt reuse. The four answer-only questions
cover France's capital, `2+2`, water's formula and the largest Solar System planet,
each capped at eight output tokens. A model stop and exact normalized answer are
required for a factual pass; truncated output cannot pass.

Compare the approved adapter at scale 1.3 with no adapter using the same source
and fixed inputs. The absent-adapter arm measures an ablation ceiling for
simplification, not a recommended replacement or an edited adapter. Retain actual
answers, termination, token counts, request/load timing and externally supervised
process memory/cleanup with per-arm and aggregate power observations. Tiny output
counts and a single ordered pair do not establish stable throughput, broad
quality or daily-use memory. Basic factual passes cannot qualify creative,
refusal or instruction-following behavior. Existing lossless adapter storage
reductions are separate, already implemented local-source work, and must not be
credited again as a result of this experiment.

Keep Swift packed: approximately 27 billion FP16 weights alone need about 54 GB,
before state, scratch and Windows. Sparse vectors avoid whole-model expansion
but still charge capture, scratch and device copies. Follow the
[owner-device budget and power rules](../AGENTS.md#owner-device-execution-budgets)
and keep suites/benchmarks in owner-operated CI.

Future models require fresh artifact, architecture, template, basis, dimensions
and supported-site review. Infrastructure reuse does not transfer a fitted edit
or its quality verdict.
