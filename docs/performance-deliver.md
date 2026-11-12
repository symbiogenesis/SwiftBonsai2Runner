# Prefill, snapshot and resident FFN implementation closeout

Three concrete source items from [PLAN.md](../PLAN.md) are implemented in Prism:
complete six-head tiled-prefill locality, immutable runtime logits snapshots,
and bounded multi-token resident full-FFN packets.
Runner's installed release pin remains 0.1.23; using these changes in the
application requires a qualified release and immutable repin.

| Implementation | Production behavior | Source cost and boundary |
| --- | --- | --- |
| Six-head tiled prefill | The pinned Swift prefill path shares each cached key/value conversion across all six query heads belonging to a KV head. Explicit prefix extents exclude future tile tokens; current-token keys/values retain their original unrounded binary32 path. | Removes the old two single-head tails. Extra score storage is charged as `2 * tile_tokens * context_limit * sizeof(f32)`, up to 2 MiB at 32 tokens and context 8192. Iterator counts are not uniformly lower because the old four-head path also shares work across tokens. |
| Immutable logits snapshots | Rollback and saved-prefix snapshots share complete immutable logits. Restore shares the same values; the next evaluation detaches without copying stale logits and then reuses its mutable output. | A Swift vocabulary result contains 248,320 binary32 values, or 993,280 logical bytes. Snapshot capture and restore omit those copies. Snapshot budgets keep their conservative logical charges; shared storage does not reduce admission. |
| Batched resident FFN chains | The native-Q2 executor sends two complete independent resident FFN chains in one bounded queue packet. Each token retains its ordered gate/up, SwiGLU, optional supported Hadamard, down and optional residual operations. Odd tails retain scalar-chain submission. | Queue writes become `ceil(tokens / 2)` instead of one per token. Logical operations, original single-token solver arithmetic, lifetime caps and conservative memory admission remain charged. This does not implement a new matrix solver. |

Snapshots preserve exact decoder identity, complete state, token history, finite
output proof and rollback behavior. Invalid/empty logits do not force a copy of
an idle reusable output buffer. Live output mutation cannot alter a retained
checkpoint. Capture, restoration, cancellation and failed delivery retain their
existing transactional boundary, and original floating-point bits and sampler
progression remain unchanged.

FFN batching uses the existing signed-operation subset and explicit resident
placement. Correlated completion and validation of every output precede
publication. Unsupported shapes reject before submission; uncertain completion
poisons the session and never triggers replay. The existing adapter and Hadamard
sign restrictions remain in force. True multi-token native-Q2 matrix prefill,
new signed arithmetic and broader placement remain separate work.
The admitted native working allocation reserves an 8 KiB command area and two
sets of eight maximum-width binary32 regions for simultaneous chain lifetimes.
The existing larger packed-weight workspace still dominates ordinary Swift
geometry; small shapes receive the larger required reservation before allocation.

Intermediate local controls passed before strict analysis failed. Later narrow
source corrections were reviewed without a rerun; final-source validation,
production linking and feature-disabled compilation remain pending. Source copy
and queue-write counts establish no measured latency, throughput or memory gain.

Owning hosted acceptance, complete-model state/logit/adapter and cancellation
parity, ARM64 code generation and resources, signed vendor lifetime, and matched
complete-request benefit remain required before a qualified release and repin.
