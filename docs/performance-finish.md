# Completed performance source work

Three local Prism implementations close concrete source gaps from Runner's
attention/locality, runtime-overhead and Hexagon FFN backlog. They are integrated
in local `main`; their source history retains the implementation commits.
Outstanding qualification remains in [PLAN.md](../PLAN.md#cpu-kernels).
Runner's 0.1.25 development pin contains these changes. Actual-model and measured
benefit gates remain pending; development delivery does not qualify them.

| Source implementation | Production behavior | Work removed or enabled |
| --- | --- | --- |
| Complete six-head KV sharing | Pinned Swift decode groups all six query heads per KV head when the worker budget fits the four KV groups. Other geometries and larger worker budgets keep the previous partition. Scalar and baseline NEON retain the native FMA lanes, padding, F16 probabilities and gate arithmetic. | At 256 channels, value-cache iterator sweeps fall from 18 to 4 per KV group; the fixed accumulator is 24 KiB versus the old four-head helper's 32 KiB. These are source counts. |
| Reset-only terminal ingestion | Sampled `SwiftSession` requests with prompt reuse disabled ingest the last non-stop output-capped token without producing unused next-token logits. All computed state remains complete and transactional. Public Runtime APIs, latest-prefix reuse and speculation retain terminal logits. | Omits the terminal output-only graph work and 248,320-entry vocabulary projection. Its 993,280-byte logical result is omitted; no process-memory saving is established. |
| Native full resident FFN transport | Borrowed and owned sessions execute the existing gate/up, SwiGLU, optional supported Hadamard, down and optional residual chain in one packet. Exact operation/tensor pairs are 4/12, 5/14, 5/15 and 6/17. | The real backend replaces default refusal. Handles, dimensions, every operation, cache flushes, exact response correlation and terminal failure publication remain checked. |

Terminal ingestion clears logits before advancing state and admits no retained
prefix checkpoints. An accidental later ordinary Runtime call recomputes the
prefix. EOS consumes its original sampler draw and enters neither state nor
output. State-only ingestion deliberately does not execute or validate unused
output-only operations, just as in existing state-only prompt ingestion.
Whole-request state and sampler rollback, cancellation and conservative admission
remain in force. Timing appends `terminal-ingestion` after the original eleven
phases. Runner accepts the exact legacy and new receipt inventories, preserving
immutable identities and rejecting double-counted decode/terminal work or EOS
ingestion.

The native chain retains the existing explicit placement and transform subset.
Adapters disable it; arbitrary signs and grouped rotations are unsupported. The
optional down-projection Hadamard requires every authored sign to be positive
one. Correlated completion precedes finite output copying, and failures poison
the owner and clear output without replay. No new signed kernel, session cap,
resource discount or automatic backend selection is introduced. True `N > 1`
native-Q2 prefill needs a different matrix solver and scratch contract and remains
open. Model-free fake-DSP transport controls do not prove signed arithmetic.

Intermediate local controls passed before an inherited graph fixture failed.
The test-only correction uses actual Swift geometry and remains source-reviewed.
The original failed check is closed; final-source validation remains pending.

Require exact controls, strict Release analysis, production linking and
feature-disabled compilation. Complete-model state/logit/adapter and cancellation
acceptance, ARM64 codegen/spills, signed NPU lifecycle/resources and matched
whole-request benefit remain separate gates. Model-free source checks establish no measured
performance benefit.
