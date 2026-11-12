# Paired values, bounded special parsing and resident tile packets

Three concrete performance source gaps from [PLAN.md](../PLAN.md) are closed in
the reviewed Prism source consumed by Runner's 0.1.25 development pin. This
delivery does not close actual-model, device or measured benefit gates.

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

Intermediate local checks were incomplete; their source-only validation diaries
remain in Git history. Final-source hosted validation is still required.

Source conversion, allocation and queue-write counts establish no measured
latency, throughput or process-memory gain. Owning hosted checks, ARM64 codegen
and stack review, actual-model state/logit/adapter and cancellation acceptance,
signed vendor lifecycle/resources and matched complete-request benefit remain
independent qualification gates.
