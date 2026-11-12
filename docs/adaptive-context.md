# Memory-backed prompt context

The new native Swift source selects context capacity for each prompt from a fresh
memory observation. K4/V4 (`rsq4`) remains the default. Explicit FP16 recovery
uses its larger cache cost in the same calculation. Output remains capped at
4,096 tokens, including reasoning; larger context does not enlarge that output cap.

Runner's currently published 0.1.25 bundle still has its fixed 8,192-token limit.
The source implementation needs a new source-bound Prism bundle and a matching
Runner lock update before normal application launches activate it. Do not modify
old bundle digests, substitute a locally built executable under an old lock, or
interpret a source check as actual-model qualification.

The reviewed 0.1.26 Prism candidate is
`857dc4445d3d640c5e62ac85477b91871d2a8467` on local Prism `main`. Runner's
source eligibility recognizes that exact candidate; its binary lock deliberately
continues to identify the genuine published 0.1.25 bundle until the owner supplies
the new release receipt and archives.

## Admission and allocation

The native model owner observes available physical memory and its own private
and working-set bytes before exact counting, and checks again before generation.
The available budget preserves the existing 4 GiB physical reserve and 11.5 GiB
Swift process ceiling. Cache pages already retained by the process are not free
memory. Available OS commit is a separate ceiling. Failed or unavailable
observations reject admission; these snapshots do not promise continuous stability.

The calculation charges the selected packed records and scales, FP32 recurrent
and convolution state, rollback storage, page ownership, token buffers and scratch.
For Swift's 16 attention layers and four 256-channel KV heads, attention payload
alone costs 16,896 bytes per token for K4/V4 and 65,536 for FP16. These are storage
arithmetic, not measured process peaks or an available-context guarantee.

The initial conservative policy reserves another 512 MiB for cold workspace and
allocation uncertainty, plus 4,096 bytes per context token for score rows, token
buffers, page ownership and slack. The graph's 4 GiB live-state ceiling also applies;
explicit latest-prefix reuse retains its narrower 1 GiB complete-state rollback
ceiling. These allowances remain independent of the 4 GiB host reserve.

The result also respects graph, transactional-state and request resource bounds
and the verified model's unscaled 262,144-token training ceiling. That ceiling is
an upper bound, not the capacity promised by any memory observation. The broker
requires:

```text
exact rendered prompt + requested output + 64 reserve <= observed context capacity
```

Cache pages grow on demand. Context changes between requests retain immutable
weights, the selected precision, adapter and executor. They never discard messages,
lower the requested output cap, change cache precision or restart the model to
make a prompt fit. Memory pressure after counting can still reject generation;
the existing process and physical-memory watchdogs remain necessary.

Ordinary reset-only serving releases completed KV pages after terminal delivery
without reseeding the sampler. Explicit latest-prefix reuse retains its pages and
charges them to current process use. It preserves its prefix while the request fits
existing lazy bounds; actual bound growth invalidates old checkpoint geometry and
recomputes the supplied complete history. Neither path removes conversation messages.

## Private protocol and native UI

An eligible exact-source runtime advertises `context_policy: "available-memory-v1"`
with its maximum context bound in authenticated readiness. Its private counter
returns the memory-backed `context_tokens` with the exact `input_tokens` count.
Runner accepts that field only for an explicitly selected adaptive backend and
keeps the original canonical bytes for generation. Old runtimes and publisher
rollback retain fixed 8K admission.

Broker response headers identify the context policy, observed capacity and model
maximum. The native UI uses those observations for its approximate occupancy
meter. Capacity is unknown before an observation; an adaptive observation is
labelled as the last checked capacity and can change on the next prompt. The UI
does not derive eligibility from text length or a free-RAM estimate.

## Delivery and acceptance

Integrate reviewed Prism and Runner source into their local `main` branches.
The owner publishes the new runtime source and bundle. Review its exact release
receipt, archive and source with the existing
[runtime lock workflow](development-guide.md#prepare-a-runtime-lock-update), then
update the source-bound eligibility and compiled runtime/source pins together.

Focused model-free checks cover memory arithmetic and pressure, FP16 versus K4/V4,
resource ceilings, state resizing, strict counter negotiation and UI capacity
updates. They do not qualify full-model long-context answers, memory peaks,
Stop/Retry, accelerator operation or sustained use. The owner retains hosted and
actual-model acceptance under [validation](validation.md), within its original
execution and power-observation budgets. [PLAN.md](../PLAN.md) owns that work.
