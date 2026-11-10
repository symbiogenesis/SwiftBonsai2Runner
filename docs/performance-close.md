# Paired values, bounded special parsing and resident tile packets

Three concrete performance source gaps from [PLAN.md](../PLAN.md) are closed in
local Prism source. The [delivery record](evidence/performance-close-20261109/summary.json)
binds the separate worker commits, integrated source and focused checks. Runner
continues to consume its immutable 0.1.23 release; application delivery requires
a qualified Prism release and verified repin.

| Completed implementation | Production behavior | Work or storage affected |
| --- | --- | --- |
| Paired six-head prefill values | Consecutive prompt tokens share conversion of their common cached F16 values, retaining each token's original F32 current value and causal prefix. | Common cached values convert once per pair. Twelve heads use 32-channel tiles within the existing 24 KiB accumulator high water. |
| Bounded special-token parsing | The existing reverse trie scans bounded windows with sufficient lookahead for the longest admitted spelling; output storage grows with actual segments. | Replaces up to 8 MiB of input-wide match indexes with 8 KiB of fixed match storage, and removes the eager reservation of 65,536 segment slots for ordinary text. |
| Resident multi-tile prefill packets | Independent token projections sharing one resident weight tile use bounded multi-operation packets. | Batches existing single-token operations; logical arithmetic and lifetime operations remain charged. True multi-token matrix arithmetic remains separate. |

The CPU path preserves each channel's native sixteen-lane FMA and reduction
tree, FP16 probabilities, original gates and complete-request rollback. Cached
values visible only to the second token remain FP16; each token's own value
remains binary32. Odd token tails retain the existing path. The new grouping
may discover a later token's failure earlier inside private scratch, and it
must never publish a successful request after an incomplete reduction.

Special parsing retains earliest-start, longest-at-that-start matching, borrowed
UTF-8 text, explicit literal/parse policy and existing input/segment limits.
Window boundaries do not create token boundaries. Allocation remains fallible,
and errors return no partial segment result. This complements the existing
request-local BPE cache rather than changing merges or token IDs.

Resident queue batching reuses the existing signed `N = 1` operation contract.
The audited solver selects `HVX_QUANT_ROW_N1`; changing the matrix token dimension
would require a separate solver, scratch and numerical contract. Packet batching
does not establish that missing support. Exact destination validation, terminal
failure, conservative resources and public owned-session forwarding remain
required.

One predeclared model-free local operation covers canonical formatting, selected
exact controls, strict Release analysis, production worker linking and
feature-disabled compilation. Its fixed inventory, original clock, power
observations and raw evidence belong to the delivery record. Full suites,
benchmarks, models, accelerator calls and CI operations are excluded.

The single operation applied canonical formatting and passed all six receipt
controls, then stopped during selected test compilation on one test-only
`unused_qualifications` error in `std::mem::size_of`. The compiler's exact
prelude-path correction is committed and independently source-reviewed without
a retry. The runtime and native production libraries and native test executable
compiled, but no selected Rust control ran. Strict Release analysis, production
worker linking and feature-disabled compilation were not reached. The failed
59.509-second clock remains closed. All 19 power snapshots report AC connected,
saver off and 71% charge; endpoint observations prove no uninterrupted stability
or performance comparison. Ten retained archive members were hash-verified.

Source conversion, allocation and queue-write counts establish no measured
latency, throughput or process-memory gain. Owning hosted checks, ARM64 codegen
and stack review, actual-model state/logit/adapter and cancellation acceptance,
signed vendor lifecycle/resources and matched complete-request benefit remain
independent qualification gates.
