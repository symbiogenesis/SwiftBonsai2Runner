# Inference defaults and quality evidence

Chat preserves the native template and publisher's evaluated sampler, with thinking
off by default. The development default is Rust Prism 0.1.27, original Swift PQ2, four CPU
row workers and standard adapter scale 1.3. `--publisher-engine` selects publisher rollback;
PTQ1 remains opt-in. The worker attempts [X2 NPU acceleration](hexagon.md), retaining
CPU execution if initialization is unavailable.

These defaults are not qualified as optimal for laptop use or creative writing.
Actual-model, performance, full-context/output, quality, isolation and installation
acceptance remain pending.

## Applied defaults

| Setting | Default | Reason |
| --- | --- | --- |
| Template | Verified embedded Jinja template | Preserve the model's trained role, turn and reasoning format |
| Thinking / effort | Disabled; `xhigh` when enabled | Chat defaults to off; Settings → Response can enable thinking for the next response |
| Separate thinking budget | `-1` (no extra cutoff) | Avoid forcing an early reasoning exit; total generation and context caps still apply |
| Sampler | Temperature **1**, top-p **0.95**, top-k **20**, min-p **0** | Matches Swift's published evaluation configuration and Qwen's thinking guidance |
| Penalties | Repetition **1**, presence **0**, frequency **0** | No additional repetition/logit correction by default |
| Added system prompt | None | Retain publisher instructions and user-supplied task/voice constraints |
| Generation harness | One completion, EOS honored | No automatic critique/rewrite, forced extra thinking, majority voting or best-of-N |
| Assistant history | Preserve supplied `reasoning_content` with `preserve_thinking: true` | Both verified templates support replaying prior assistant reasoning |
| Attention / recurrent state | K4/V4 (`rsq4`) in the 0.1.27 development pin; F32 recurrent/convolution state | Owner-selected approximate cache; measured quality/memory qualification remains pending, with explicit FP16 recovery |
| Context handling | Exact rendered token count; reject overflow | Preserve supplied history rather than silently shifting or truncating it |
| RoPE | Model metadata, no override | Context extension can change short-context behavior |
| Cache reuse / speculation | Disabled by default | Explicit [latest-prefix reuse](prefix-reuse.md) is available for native launches; speculation is outside Runner serving. Neither is an established quality improvement. |

CPU checks, server plans and the Rust development launcher pass sampler/reasoning arguments
explicitly. The pinned completion renderer nevertheless omits template kwargs: its single-turn
check uses the template's thinking-on/`xhigh` defaults. Server requests apply selected settings.

For exact counting and generation, the broker uses the identical canonical body with
`chat_template_kwargs`: selected `enable_thinking` (default false), `reasoning_effort: "xhigh"`,
`preserve_thinking: true`. Other client overrides reject. Original user/optional-system content
is preserved. Clients must retain/supply assistant-only text `reasoning_content` alongside answers;
both consume context. The broker cannot recover omitted history.

The [paired LoRA qualification](swift-lora.md#fixed-acceptance-contract) uses separate
greedy/template controls. Native Rust token-level reference also defaults to greedy
with explicit seeded sampling; its SplitMix64 contract proves no Prism RNG parity
and is not the chat backend.

## Experimental extra effort mode

The [native UI](ui-architecture.md) defaults to **Standard**. Opt-in **Extra effort (experimental)**
is a Rust client workflow with no new broker option, following
[GVS5H's scoped calls/working notes](https://github.com/slee-persis/GVS5H/blob/master/codebase/v2-current/escalation/multiagent.py):

1. Plan around user constraints and distinct approaches.
2. Draft from the original task, relevant sources and plan.
3. Review against original constraints and revise, retaining the draft for comparison.

The same model makes at most three sequential calls, each with a fresh context containing
the original task and relevant state. User instructions and approved facts stay authoritative;
generated notes/references are data. There are no autonomous tools or open-ended manager loop.

Each call retains sampler/template/exact admission at the model launch's reported
memory-selected context capacity. Reasoning-inclusive caps are 512/1,024/2,048/4,096
(default 4,096), with the broker's 64-token margin. Smaller caps leave more
prompt room but can stop reasoning before an answer. Count included plan/draft text; reject
overflow rather than dropping history or raising limits. Shared cancellation covers all
stages, with progress labels and no elapsed generation-time limit. Retain failed/incomplete
stages; truncated revisions neither replace complete drafts nor become finished answers.

Focused checks cover single-call defaults, stage order, bounds, cancellation and draft retention.
The experimental option has no qualified quality/speed gain; GVS5H coding results do not establish
Swift creative quality. Promotion requires evidence, with independent service/security gates and
CI-only benchmarks.

## What the benchmarks establish

Swift's published evaluation configuration motivates the sampler baseline; it
establishes neither an optimal sampler/effort/prompt nor creative-writing gains.

[Qwen guidance](https://huggingface.co/Qwen/Qwen3.8-27B#best-practices) supports the thinking sampler,
warns stronger presence penalties can hurt, and separately defines non-thinking settings.
Both exact GGUF templates (SHA-256 checked against committed metadata) accept `xhigh`/`medium`/`low`,
default to `xhigh`, and support `preserve_thinking`. Pinning effort is not evidence of a gain.
Clients must retain reasoning for replay; the two sampler modes stay separate.

No reviewed Swift PQ2 creative A/B supports a superior custom system prompt.
[Persona](https://aclanthology.org/2025.emnlp-main.1364/) and
[self-correction](https://arxiv.org/abs/2310.01798) experiments show irrelevant personas or revision
without external feedback can hurt; neither supports mandatory expert personas/rewrite loops.
Supply relevant references, audience, voice and constraints; request useful revisions.

[KIVI](https://arxiv.org/abs/2402.02750) validates a specific asymmetric cache method on tested
models, not arbitrary Q4/Q8 Swift caches. The owner selects the native
[TurboQuant-inspired K4/V4 default](quantized-kv.md) for development without
claiming completed quality or device qualification. FP16 (512 MiB at 8K) remains
the explicit comparison/recovery option and the historical 0.1.23 control's cache.
Prefix reuse saves computation, not added knowledge/reasoning.

## Capacity and execution limits

Native and publisher serving select context from available memory at launch under
the [memory-selected context policy](swift-memory.md#memory-selected-serving-context).
Both cap completions at **4,096 tokens**, including reasoning; the requested reply
and 64-token template reserve must fit alongside the canonical prompt. Byte limits
also apply. Fixed qualification packets retain their declared 8K context and output
bounds; those controls do not set the application's current capacity.

Truncation can precede an answer and cannot count as a complete or benchmark-equivalent
result. Larger allocated context does not establish usable capacity, accuracy or
long-range quality. Preserve native RoPE and qualify each cache/window combination,
full-state recovery and resources before making such claims. Offload, thread,
batch and prefix-reuse choices need their own backend/device evidence.

Optional abliteration retains [independent TPS/quality gates](swift-lora.md#standard-profile-qualification); fewer refusals
prove neither reasoning nor prose gains. Defaults do not promote adapters, representations,
accelerators or daily-service profiles.

## Output-budget experiments

Length prediction, guided sampling, trained budget controls and multiple-candidate
selection change work or answer distribution. They are separate from faster
kernels, remain unqualified for Swift and need compatible artifacts where applicable.
Charge predictor work, extra prefills, failed attempts and human revisions to
useful completed-answer time under the [measurement protocol](performance-expectations.md#measurement-and-comparison).
Preserve selected Thinking/Extra effort, exact reservations, cancellation and
truthful truncation.
