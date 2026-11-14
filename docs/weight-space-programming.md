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

For a behavioral correction, use existing complete reasoning/answer captures
and independent human review, with exact model/runtime/template, sampler,
adapter, effective Thinking and termination identities. Diagnose a reproducible
concentrated failure before
capturing activations or fitting a correction. Keep tuning data separate from
held-out evaluation and freeze candidates before scoring. Compression/refitting
instead follows the direct-edit contract below; it need not invent a reasoning
defect to justify a fidelity and resource objective.

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
LoRA is not required for that case. The existing `weight-patch` command still
exports a standalone correction; direct A-factor refitting/export is separate
work tracked in PLAN.md, not an implemented mode of that command.

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

The [first direct-weight candidate](evidence/lora-weight-edit.md) retained half
of the linear A coefficients and saved 1.4375 MiB of native factor payload.
Both arms passed four facts, but a power-source change during the original arm
invalidated the speed comparison. It remains an uninstalled research artifact.

## Direct-edit evaluation contract

[PLAN.md](../PLAN.md#direct-lora-weight-editing) owns the outstanding work and its
priority. W8's primary research objective is to preserve quality with small adapter
edits around substantially cheaper model execution. Adapter-storage pruning,
direct A-factor refitting and their standalone qualification are secondary; the
half-retained candidate is not a prerequisite for W8. Applicable W1/W2/W5/W6
evaluation and release controls are shared. Further compression or kernel work
needs a measured reason; a small factor saving alone need not justify its cost.

W8 candidates couple explicit attention/KV approximations or selective computation
skipping with compensating adapters, while keeping base weights packed and
immutable. They may require new adapter targets/rank, native graph/loading/state
support and bounded calibration or distillation. Select one measured bottleneck
and execution profile first. Compare original execution, cheaper execution without
new compensation and the same cheaper execution with frozen adapter edits.
Existing adapters remain at surviving sites; skipped targets require explicit
scope. Bind skip/retention policy, resident weights and cache/state geometry into
distinct profile identities. Qualify state/reset/rollback per profile rather than
requiring intentionally different states to match original state bits.
Reduced operation counts, unloaded base weights and smaller KV state are separate
mechanisms and need separate latency/memory attribution.
The existing FFN-down adapter consumes post-gated hidden activations, so retaining
that input computation does not implement whole-FFN skipping. Swift's current
graph is dense; expert-routing experiments require a compatible MoE artifact,
not merely a changed router policy in the existing model. These are research
hypotheses, not established Swift benefits or implemented runtime modes.

Quality and intended adapter behavior have predeclared nonregression margins.
Speed, memory and accuracy are reported separately, with material-benefit targets
and accepted tradeoffs fixed before evaluation. A scalar score must not hide a
quality regression. Improved accuracy requires independent held-out improvement;
coefficient fidelity, shorter text and four factual passes cannot establish it.
Calibration, development and held-out splits keep answer/content groups disjoint.
Freeze candidate bytes, settings, rubric and acceptance criteria before opening
the held-out split; tuning after inspection needs a new untouched evaluation set.

For the secondary compression work, the following controls separate pruning from
sparse execution. Dense execution
of an edited candidate requires additional explicit research loader/helper
support; the current edited session loader always selects sparse execution.

| Control | Adapter weights | A reduction |
| --- | --- | --- |
| Original dense | Exact approved adapter | Existing dense profile |
| Edited dense | Frozen candidate bytes | Same dense profile |
| Edited sparse | The same frozen candidate bytes | Named sparse profile |

Use identical base, adapter scale, alpha, tokenizer/template, sampler, cache,
context, workers and prompts. Compare original/edited dense for weight effects,
and edited dense/sparse for execution effects. Distinct graph identities and
predeclared independent numerical tolerances are required; sparse F64 and dense
NEON reductions are not assumed bit-identical. These numerical comparisons use
the same edited weights; original versus edited answers measure the intended
weight change, not numerical parity. Verify final correction vectors,
logits and complete transactional state on the selected actual-model paths.
New profiles cannot inherit another profile's checkpoint or backend acceptance.

Refitting preserves `s * B * (A^T h)` on representative activations unless an
independently diagnosed behavior supplies a different target. Fixed-support
least-squares or aligned directional LP edits are candidate methods, not measured
results. The standalone patch's unit direction, sign and scale must be converted
explicitly before folding into A; alignment of directions alone does not make its
exported coefficients directly interchangeable. Charge support/indices and bound
the sparsity of final A, since a sparse delta can leave `A + delta` dense.
Recheck support, finite values and the objective after exporting F32 weights.
FFN probe captures identify their scalar/unfused basis and cannot qualify native
NEON execution. SSM refitting also requires the existing P1/native writer mapping.

The evaluator reuses native process supervision and owns typed admission,
derivation/source validation, original aggregate deadlines, comparison eligibility
and scoring in Rust. Plan fixed arm order or counterbalancing and cache handling
before execution. Record complete useful answers, termination/token accounting,
load/prefill/decode/request timings, factor allocation capacity, steady and peak
process memory, cleanup and per-arm/aggregate power. Factor storage, actual RAM
and download size are distinct claims. Unknown or changed power rejects a speed
comparison; equal endpoint observations do not establish uninterrupted stability.

Full matrices and repeated benchmarks remain owner-operated CI work. A laptop
capture must be independently useful and fit its single predeclared allowance;
prebuild source-bound native helpers, include setup/retention/cleanup and stop on
no-go. Closed trials cannot be resumed, retried or split into successive budgets.
Retain exact selected candidate bytes, minimal derivation/build identities and
required replay inputs under the existing evidence-retention/archive machinery.
No new archiver or duplicate source/build bundles are needed. Release and native
application acceptance follow the existing promotion and owner-push workflow.

## Selected W8 investigation: recover omitted FFN computation

The primary investigation retains most nonlinear FFN computation and fits a
small input-to-residual adapter for the contribution of omitted groups. This
offers a more plausible capacity tradeoff than replacing an entire nonlinear
branch with a low-rank linear map. The related
[literature review](research-register.md#small-edits-around-cheaper-execution)
supports the mechanism and its controls; it establishes neither a measured
Swift bottleneck nor improved Swift capability. Capacity screening precedes
selective loading and serving kernels.

At one selected site, capture the original, unfolded normalized input
`x = RMSNorm(residual_after_attention)` from the changed-prefix execution.
Evaluate the full teacher branch `t(x)` and retained branch `r(x)` on that same
input, then fit `t(x) - r(x) = B * (A^T * x)`. Future execution adds the small
correction to the retained branch. The full teacher includes its standard
FFN-down adapter at the frozen scale. The retained branch applies that adapter
only to retained original-basis hidden coordinates, treating omitted coordinates
as zero. Its missing contribution belongs in the fitting target; it must not be
applied a second time. Other adapter sites survive unchanged. New A/B factors
consume width-5,120 normalized input, not the existing width-17,408 down-adapter
input. These are distinct targets and cannot share factors by assumption.

Keep the original-prefix teacher branch separately. The difference between its
output and `t(x)` diagnoses prefix drift; fitting local omitted work does not
correct that drift automatically. Sequential composition and freely generated
continuations remain independent tests. Teacher-forced matching token IDs and
matching sample names alone cannot establish matching hidden inputs.

Prism's source-only `ffn_probe` collector now prepares these paired rows by
replaying the untouched probe, resetting the same loaded model and replaying
one fixed group-omitted site on the same token IDs. Both passes count against
its existing 256-token cap and original deadline. It propagates the retained
branch without recovery, retaining original teacher targets separately. It
still loads every weight and executes the full teacher FFN, so this capture
has no speed or RAM benefit. Compensated rollout, multi-site composition and
free generation require later support and acceptance. The released Runner
runtime does not expose this experiment; the
[tool contract](../tools/bonsai/README.md) describes the producer/consumer fields.

With only one edited site and fixed token IDs, earlier layers remain unchanged;
the selected site's inputs do not stress drift caused by earlier omissions.
The separate original-target fields prevent discarding that distinction when
later collectors support multiple sites. This first collector screens local
capacity only; it cannot qualify compensation on an already-compressed prefix.

Whole-FFN replacement remains a separate capacity experiment. Schema 1 fits
`t_hat = B * (A^T * x)` for the complete branch immediately before residual
addition, eventually substituting `residual_after_attention + t_hat`. A future
replacement omits that site's entire gate/up/down and standard adapter; the
teacher target already includes the adapter. Scale changes require a new
candidate or an explicitly qualified scale-dependent design in both experiments.

The existing post-SwiGLU capture is a different basis and cannot supply these
inputs. Capture the branch directly, because subtracting the residual after its
rounded addition does not recover the exact teacher delta. All observations
remain provisional until a complete token succeeds; discard them on failure.

### Source-derived opportunity

Swift has 64 FFNs, each with packed gate/up/down matrices of shape
5,120 by 17,408. The following original-PQ2 counts exclude norms, adapters,
scratch and state:

| Quantity | Calculated work or payload |
| --- | ---: |
| One original FFN | 267,386,880 weight MACs/token; 67.734375 MiB packed weights |
| All 64 original FFNs | 4.2333984375 GiB packed weights |
| One omitted 1,024-coordinate group at one site | 15,728,640 removed base-weight MACs/token; 3.984375 MiB packed payload |
| One rank-16 replacement | 163,840 factor MACs/token; 640 KiB F32 factors |
| Two omitted groups at all 64 sites, rank-16 recovery/site | 510 MiB gross; 470 MiB net packed/factor payload opportunity |
| Three omitted groups at all 64 sites, rank-16 recovery/site | 765 MiB gross; 725 MiB net packed/factor payload opportunity |
| Eight rank-16 replacements | 536.875 MiB net weight/factor payload opportunity |
| Sixteen rank-16 replacements | 1.048583984375 GiB net weight/factor payload opportunity |

These are geometry calculations, not elapsed-time, resident-memory or quality
results. Packed ternary and F32 operations have different costs. The current
loader allocates all model tensors and the graph retains all three FFN matrices;
a compute bypass alone releases no weights. A later memory-saving profile must
authenticate the complete original artifact and descriptors, omit the selected
payloads from allocation, and prove no model/graph owner retains them. The
original on-disk GGUF remains unchanged. FFN omission removes neither KV entries
nor recurrent state.

Two groups at every site cannot reach the 512 MiB private-commit target even
before adding recovery factors. Three groups provide a more useful eventual
resource envelope, with higher fidelity risk. These all-site calculations are
not a selected multi-site trial: each site's capacity and the composed model
must pass independently. Metadata, alignment, surviving adapters and temporary
copies reduce net benefit further.

Current-weight PTQ1 uses 28-byte blocks where PQ2 uses 34 bytes. Three omitted
groups at all 64 sites therefore offer 630 MiB gross and 590 MiB net payload
with rank-16 recovery per site; two groups offer 420 MiB gross and 380 MiB net.
Representation-specific geometry cannot be reused as process-memory evidence.

### Bounded native capacity screen

`bonsai-tools ffn-surrogate` fits a single site from at most 64 supplied rows,
rank 1 through 16, with a finite positive ridge penalty. Schema 1 accepts complete
branch targets; schema 2 accepts paired full/retained branches for structured
omission and records the original-prefix teacher separately.
It uses a deterministic pivoted output basis and a dual ridge solve rather than
gradient training or a whole-model weight expansion. Only fit rows construct
the basis and coefficients. Development groups assess reconstruction after
export to F32; final held-out rows are deliberately unsupported by this command.
Group IDs cannot prove semantic independence, so content grouping still needs
review. This is algebraic calibration, not a claim that direct behavioral
weight-space reasoning discovered a correction.

Freeze source/runtime/base/adapter/scale, graph and capture identities, selected
site, groups, rank, ridge and rejection limits before capture. Use exact
normalized-input and complete-branch observations from the separate Prism
research capture; its scalar/unfused teacher does not qualify production NEON.
Fitting's internal deadline does not renew the enclosing owner allowance.
Keep the output factors, digest, source/capture derivation and reconstruction
summary only if a candidate or an actionable no-go needs them.

A prospective initial screen should reject a candidate whose development
squared error exceeds 10% of teacher branch energy, whose development
relative squared error exceeds fit relative squared error by more than five
percentage points, or whose compensated development error exceeds the
uncompensated retained branch's error. All relative errors use complete teacher
branch energy, not the much smaller omitted-contribution energy. A schema-2
rejection retains diagnostics without publishing candidate factors.
Require nonzero teacher energy and independent development groups; do not hide
a failed site in an average. These conservative screening thresholds are design
choices, not answer-quality guarantees. No actual-model screen has been run.
Choose a single site and fixed capture workload only after source-bound helpers
and feasible setup/capture/cleanup estimates exist. Repeated ranks, sites or
ridge sweeps are a matrix and remain owner-operated CI work.

### Structured geometry and the long-prefill alternative

For structured FFN omission, Swift's folded down-projection Hadamard basis rules
out arbitrary original-neuron slicing. Complete contiguous 1,024-coordinate
groups respect the current independent transform blocks. A native compact
profile needs gate/up row omission, original-stride down columns and selective
loading. Keep canonical group ordering and each group's original signs; slicing
the first signs for a shorter compact width changes the function. The current
sign array places down-projection signs at offset 11,264, count 17,408. Bind its
canonical I32-le digest and the normalized, sign-before-transform recipe to the
model/source identity. Group omission changes accumulation order, so it has a
distinct numerical profile even when each group's algebra is preserved.

[TokenGate](https://arxiv.org/abs/2610.26754v1) is a separate long-prefill
candidate, including a Qwen3.5 hybrid experiment. It combines learned prompt
retention with attention Q/V LoRA. Do not transfer its GPU speedups to Swift.
Pre-gate recurrent summaries, retained positions, attention bias and post-gate
history need new native state/cache identities; deleting KV rows alone is
insufficient. Select it only when measured long-prefill cost justifies its
calibration and state work.

### Three-way success and capability improvement

The ambitious target is all three: at least 10% lower useful-answer latency,
at least 512 MiB lower actual serving private commit, and independently improved
held-out task quality. Also inspect peak Job commit, residency, startup and
prefill/decode separately; do not add accounting domains or substitute factor
counts for process RAM. Declare baseline, workload, aggregation, uncertainty,
per-category nonregression margins and comfort/resource limits before scoring.
The [existing promotion criteria](validation.md#promotion-criteria) still apply.
An intermediate two-way win may justify research, but does not meet this target.

Compensation that restores teacher behavior establishes recovery, not increased
cognitive ability. Keep reasoning, knowledge, instruction following, complete
creative work and intended adapter behavior separate, with independent human
review where required. A capability edit must start from a diagnosed failure
and survive untouched complete-answer evaluation against the original model,
the cheaper graph without compensation, and the frozen compensated graph.
If a separate behavioral edit is composed, also compare the compensated graph
before that edit to attribute the gain. For relative ability, add an
equal-resource comparison that reinvests saved work in useful reasoning, with
the same frozen task/rubric and complete answers. Keep this attribution separate
from the absolute latency/RAM gates: spending every saved millisecond does not
demonstrate faster answers. Prompt/sampler controls remain necessary;
shorter answers or repaired termination alone cannot count as better reasoning.
Nothing in the November literature establishes this three-way result for Swift.

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

### Closed pilots and future measurement scope

Adding an independent patch adds factor storage and arithmetic. The LP above
fits a behavioral correction; it does not directly minimize runtime or memory.
The [initial adapter-cost screen](evidence/weight-patch-sanity.md) used Prism's
source-only `swift_sanity_check` example: original PQ2, four CPU workers, K4/V4
cache, 256-token context, seed 42, Thinking off and no prompt reuse. The four
answer-only questions cover France's capital, `2+2`, water's formula and the largest Solar System planet,
each capped at eight output tokens. A model stop and exact normalized answer are
required for a factual pass; truncated output cannot pass.

It compared the approved adapter at scale 1.3 with no adapter. That absent-adapter
arm bounded simplification cost; it did not evaluate edited weights. The later
[direct-weight pilot](evidence/lora-weight-edit.md) evaluated the half-retained
candidate but lost speed eligibility after a power-source change. Both workloads
are closed. The direct-edit evaluation contract governs prospective work, rather
than another omission trial or repetition of these pilots. Their tiny output
counts and single ordered pairs establish neither stable throughput nor broad
quality/daily-use memory. Existing lossless allocation work is separate and must
not be credited again as a weight-edit result.

Keep Swift packed: approximately 27 billion FP16 weights alone need about 54 GB,
before state, scratch and Windows. Sparse vectors avoid whole-model expansion
but still charge capture, scratch and device copies. Follow the
[owner-device budget and power rules](../AGENTS.md#owner-device-execution-budgets)
and keep suites/benchmarks in owner-operated CI.

Future models require fresh artifact, architecture, template, basis, dimensions
and supported-site review. Infrastructure reuse does not transfer a fitted edit
or its quality verdict.
