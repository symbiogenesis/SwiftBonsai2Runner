# Weight-space programming: Swift applicability

Weight corrections and adapter compression are conditional research, not active
Swift features. Behavioral corrections start with complete-answer diagnosis and
ordinary prompt/template/sampler controls. Adapter compression instead starts
with an explicit fidelity and storage objective. Neither a beneficial Swift
behavioral correction nor a matching reasoning-loop defect is established.
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

## Editing the existing adapter weights

The approved Abliteration GGUF contains F32 factors, not ternary factors. It can
be edited directly without expanding or rewriting the 27B packed model. For an
existing correction `s * B * (A^T h)`, changing `A` to `A + v` folds a correction
along the same `B` direction into that adapter at its existing rank. An unrelated
output direction generally needs another rank. The LP described above can
therefore edit an existing A factor when its direction matches B; an additional
LoRA is not required for that case.

Swift's published correction is already merged into the selected ternary model.
Its [pinned artifact repository](https://huggingface.co/ukisai/Swift-Bonsai-2-GGUF/tree/a3bdac086bbb6b04d87d0108d18943b5f63868f7)
does not supply separate original Swift factors. Do not call an edit to the
available Abliteration adapter an edit to that unavailable Swift checkpoint.
Original floating-point factors could be edited before merging and requantizing,
but that is a different artifact pipeline. Keep the current packed base fixed.

The approved adapter is already rank one, so rank truncation cannot reduce it
while retaining a nonzero correction. Editing dense values alone normally
changes neither storage nor executed operations. An executable sparse factor
representation is needed to turn zeroed weights into a storage/work reduction.

`bonsai-tools adapter-edit` inspects or creates a candidate from the exact
approved adapter. It retains the largest absolute A coefficients independently
at each of the 128 linear sites, with deterministic index ordering for ties.
This minimizes coefficient L2 error for the chosen number of retained entries;
it does not minimize answer loss. All B factors, the embedding A factor, alpha,
metadata, target inventory and surviving coefficient bits remain unchanged.
The candidate is a separate GGUF; its original dense disk layout and byte count
are unchanged. The approved file and application defaults remain untouched.

An explicit experimental Prism loader can store profitable zero-pruned A factors
as F32 values plus U16 indices, releasing their dense copies. At 50% retained
coefficients this reduces linear A payload by 25%; container overhead, embedding,
shared B, packed base and state remain separate. Sparse execution accumulates
the selected products in ascending index order with F64 arithmetic and rounds
the rank result to F32. It has a distinct graph identity and does not promise
bitwise parity with dense NEON reductions or a speed improvement.

Freeze the candidate before factual evaluation. Retain the edited bytes and
their digest, exact-source edit receipt, original/candidate storage accounting,
complete factual answers and process/power observations. A four-fact smoke is
a rejection screen, not held-out broad accuracy or preservation of the adapter's
intended behavior. Pruning/refitting on representative activations and independent
complete-answer evaluation remain necessary before promotion. The sparse loader
is source-only and is not exposed through the released application.

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
