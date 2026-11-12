# Paired prefill, request preparation and tiled projection closeout

This delivery implements three remaining source costs from the
[performance backlog](../PLAN.md): repeated cached-key conversion during
six-head prefill, repeated BPE preparation within one request, and repeated
native-Q2 weight packing across multi-operation token tiles. The changes live
in the reviewed Prism source consumed by Runner's 0.1.25 development pin.
Actual-model, device and measured benefit qualification remains pending.

| Completed source work | Production behavior | Boundary |
| --- | --- | --- |
| Paired six-head prefill scores | Two prompt tokens share reads and F16 conversion of their common cached keys. | Each token retains its own causal prefix, original current-key F32 path, native reduction lanes, probability rounding and gate arithmetic. |
| Request-local BPE piece reuse | Repeated short pretokenized pieces reuse their exact completed token IDs. | Bounded inline storage belongs to one encoding request and one borrowed immutable rule owner; long pieces bypass it. No cross-piece merges or persistent tokenizer cache. |
| Multi-operation native-Q2 token tiles | Each weight tile is packed once across all token rows, with one reusable partial-output buffer. | Every token retains its projection accumulation order. Logical operations, resource admission, lifetime caps and terminal failure remain charged. This uses the existing N=1 solver. |

The BPE cache adds 2,824 inline bytes on a 64-bit target: sixteen entries
containing at most 32 input bytes and 32 token IDs, plus the borrowed rule owner.
Full byte comparison prevents collision reuse, and changing owners clears all
entries. Only successful complete pieces enter the cache. Token limits and
missing-byte error precedence apply equally on hits and misses.

Paired scoring uses 768 bytes of scalar/native-lane accumulators instead of
384 bytes for one token. Its score stage finishes before the existing 24 KiB
value stage; retained score buffers and conservative admission stay unchanged.
It computes the second token's scores before publishing the first token's
provisional values, so arithmetic failures can be detected earlier inside
private scratch. Whole-request rollback remains the publication boundary.

Multi-operation projections traverse weight tiles before tokens. Each token's
row/column additions retain their original order, while failures and provisional
completion counters follow the declared tile-major submission order. All token
inputs validate before submission; any failure faults the executor and clears
every output without replay. Existing logical operations and queue writes remain
unchanged. Transient 8/16/32-token tiles pack each weight tile once instead of
8/16/32 times, and caller output storage replaces repeated full pending outputs.

Intermediate numerical/tokenization controls passed before strict analysis
failed. Its branch-order correction was source-reviewed without a rerun.
Final-source strict analysis, production linking and feature-disabled compilation
remain pending; the source-only validation diary remains in Git history.

Owning hosted checks, ARM64 code generation and stack review, complete-model
state/logit/adapter and cancellation acceptance, signed vendor lifecycle and
matched whole-request benefit remain separate gates. Source work counts do
not establish measured latency, throughput, process-memory savings or reduced
admission. True N>1 native-Q2 matrix prefill remains open. No default, installed
runtime or automatic backend-selection policy changes in this delivery.
