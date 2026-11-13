# Resident host weight accounting

This guide and its ledgers describe the historical exact HMX bitplane bridge.
The current [X2 `native-q2` development default](../../docs/hexagon.md) uses a
different direct-output layout and six larger arenas. Do not apply the 64 MiB
HMX ceiling or the traffic counts below to that serving path.

The experimental Prism HMX path can retain immutable, prepared weight tiles in
an explicitly bounded mapped host arena and reuse them through opaque
session-owned handles. Its default remains streaming. The optional arena is
limited to 64 MiB and does not change the terminal duration. The owner-authorized
terminal cap is 24,000 write attempts, still shared by the entire request.
This is host RAM shared with the signed kernel; it is not persistent DSP
VTCM storage, a fused Swift kernel or a daily inference service.

The runtime cache binds each complete projection to retained immutable tensor
storage and its geometry. A cache hit can avoid repacking its original two-bit
symbols and copying the packed tile into the mapped allocation for each token.
Original activation quantization, scales, ordered CPU reduction, transforms,
adapters and graph state retain their existing contracts. Native mappings,
cache metadata, scale storage and temporary packing need explicit admission in
addition to the original model and graph. The two cache limits account for
different resources and must not be substituted for one another.

Run `pwsh -NoProfile -File tools/hexagon/verify_resident_traffic.ps1` to reproduce the retained
[current static ledger](../../docs/evidence/hexagon-resident-host-24000-ledger.json).
The derivation uses the
[enforced operation budget](../../docs/evidence/hexagon-session-operation-budget.json) and derives
the packed payloads from the existing source-bound
[streaming ledger](../../docs/evidence/hexagon-swift-traffic-ledger.json). The
arena ceiling and native layout are explicit assumptions. This verifies
conditional arithmetic and retained source identities, without device access,
model loading or runtime acceptance.

| Original-PQ2 placement | One `blk.0.ffn_up.weight` | All eligible projections plus logits, idealized |
| --- | ---: | ---: |
| Packed resident weight bytes | 25,067,520 | 7,199,539,200 |
| Synchronous operations per token | 40 | 23,256 |
| Activation payload per token | 163,840 | 95,256,576 |
| Returned plane payload per token | 22,282,240 | 6,399,590,400 |
| Fits the 64 MiB arena | Yes, before other resources | No |
| Fits the terminal operation cap for one token | Yes | Yes, with 744 attempts left |

With eight repetitions of the first projection, retaining all of its packed
weights would avoid at most 175,472,640 bytes of repeated CPU packing. The
kernel's logical weight, activation and result payload remains 380,108,800 bytes
across those repetitions. These extents establish no bandwidth or latency gain.
The all-eligible case needs at least 13,999,104,000 bytes for the selected original
and resident packed weights alone, exceeding the 11.5 GiB Swift service allowance
before caches, scratch, state and other resources. It is a lower-bound
counterexample, not an admitted placement. Two all-eligible evaluations exceed
the session operation cap; a normal prompt and output reserve still need
explicit partial placement or a separately qualified serving design.

The native descriptor still covers the entire expanded allocation with the
unchanged `0x90` flags on every operation. The first-projection example expands
that allocation from 1,191,936 to 26,259,456 bytes. Its forty descriptors therefore
cover a cumulative 1,050,378,240-byte extent per token, versus 47,677,440 bytes for
the original scratch allocation. Descriptor coverage is not transferred bytes
or cache work; actual cache maintenance can grow and remains unmeasured.
Keeping already packed host bytes must not be reported as eliminating the
kernel's weight DMA, intermediate plane results or synchronization.

The current native-Q2 path has a separate layout and transport contract. Signed-kernel changes
require reviewed distribution, ownership, state, resource and performance gates under
[PLAN.md](../../PLAN.md#hexagon-npu). Host caching alone provides no signing authority.
