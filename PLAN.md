# Swift Bonsai 2 Runner TODO

This is the single backlog. Keep only outstanding work here; remove completed
items. [Technical docs](docs/README.md) own behavior, contracts, commands and
required evidence. Git history owns completed work and trial diaries.

Prioritize native daily use, then correctness, isolation, creative quality and
comfort, then measured optimization. Task IDs identify related work across docs.
Follow [delivery ownership](docs/development-guide.md#implementation-delivery),
[execution budgets](docs/validation.md) and
[promotion criteria](docs/validation.md#promotion-criteria). Local checks leave
hosted, device and human acceptance pending; the owner handles pushes and CI
under [AGENTS.md](AGENTS.md).

## Publication and qualification prerequisites

- [ ] **L1 — Qualify the Rust/PowerShell tooling migration.** Have the owner
  publish reviewed source and qualify Source checks/Foundation, migrated fixture
  inventories, exact native helper/cache handoffs and separate SDK/ABI controls.
  Public reconstruction and replay retain P1's schema-2 scalar-v2 identity.
- [ ] **C1 — Qualify current CI source.** Have the owner publish the reviewed
  startup, staged-index, ASR/UI fixture, workflow-consolidation and dependency
  corrections; require Source checks, Foundation and dependency-audit acceptance.
  Follow the [startup preflight](tools/ci/README.md#action-dependency-pins) before
  workflow changes and verify supported self-hosted runner versions.
- [ ] Qualify Runner's checked-tools producer, shared regression runner and
  native-acceptance caches: cold build, exact hit and relevant-input invalidation,
  fresh same-run packages, source/digest verification and every selected check.
- [ ] Qualify Prism's Foundation scheduling and source-package/Hexagon caches
  on matching hosted images, preserving complete correctness, mutation, packaging
  and independent ABI/SDK obligations.
- [ ] **R1 — Publish Runner v0.3.0 and qualify the pinned Prism 0.1.27
  development release.** Require all five Rust release lanes, source-bound
  compressed packaging, extraction/smoke checks, notices and independently
  verified three-asset/source identities under the [release guide](docs/releases.md).
- [ ] Qualify the single-executable builder from a checkout and on a clean
  Windows ARM64 installation: double-click launch, visible native startup errors,
  stored/deflated package compatibility, malformed compression and expansion limits.
  Verify no separately installed VC runtime or TTS inference DLL is required.
- [ ] **D1 — Qualify portable setup, repair and updates.** Exercise the published
  v0.2.2 baseline and the new release after publication: fresh/custom data folders,
  automatic model/adapter/runtime setup, interruption/resume, corruption repair,
  pause/retry, disk/network/permission failures and independent speech retry.
- [ ] Qualify automatic startup/15-minute release discovery, conditional metadata
  requests, compression, server-directed backoff and manual checks. Cover paginated
  numeric/prerelease selection, incomplete/no-release/network failures, changed
  publication and tampered staging/original bytes. Verify background download,
  one prompt per verified release, Later and the Settings badge, deferred replacement
  on close without reopening, explicit replacement/restart, close during download
  or generation, renamed executables, original options, read-only targets, sharing
  exhaustion, restart failure, data preservation and rollback; source and preview
  launches must stay silent. See [update behavior](docs/releases.md#update-and-remove).
- [ ] If installer distribution is selected again, qualify the retained MSI
  updater's discovery, verified handoff, post-exit publication checks, failure
  recovery, data preservation and installed upgrade/rollback paths.

## Application and daily service

### Actual-model acceptance

- [ ] **A1 — Qualify the Rust/PQ2 daily-use path.** Prepare the exact-source
  [acceptance package](docs/validation.md#actual-model-application-acceptance)
  after C1, with strict Release analyzers, production linking and fresh typed
  workflow receipts. Qualify explicit DX12 startup, authenticated four-worker
  readiness, Stop/Retry, recovery and explicit publisher rollback.
- [ ] Qualify acknowledged model/broker shutdown, descendant teardown, empty
  Jobs, released listeners and fresh-owner replay. Include startup timeout,
  failed-cleanup receipt retention and one shared absolute deadline.
- [ ] Diagnose native startup descendant-cancellation and bounded failure/output
  fixtures that produce empty child output; retain useful exit diagnostics.
- [ ] Diagnose publisher PQ2/PTQ1 exits before HTTP readiness, including missing
  backend exit codes and bounded diagnostics, before selecting more timing work.
- [ ] Qualify source-launch first frame/readiness, staged text/speech setup,
  bounded parallel artifact preparation, runtime/broker overlap, peer cancellation
  and descendant cleanup. Measure cold and unchanged launches separately, including
  X1 sequential hashing and speech-build contention.
- [ ] Qualify all native UI flows at normal/minimum window sizes, display scaling,
  high contrast, keyboard/IME input and with assistive technologies under the
  [application guide](docs/development-guide.md#native-chat-app).
- [ ] Qualify shared error selection, full-message Copy error, details/startup
  dialogs and Retry with simultaneous setup/model/speech/audio/update failures,
  preserving the composer and modal shortcut ownership.
- [ ] Qualify compact message headers, thinking expansion, speech/iteration
  controls, styled Unicode/code selection, copying and Find geometry.
- [ ] Qualify Settings scope, compact/wrapped navigation, persistence, generation
  locks, adapter disclosure, diagnostics, scrolling and Close/Escape focus return.
- [ ] Qualify responsive sidebar/drawer/hide/reopen behavior, stable manual width,
  branding/footer contrast, narrow composer controls, reading size/width and
  appearance persistence across restart and conversation-saving opt-out.
- [ ] Complete final-source native UI analysis and named ergonomics, appearance,
  profile, reader-size, Settings, deletion, context/Thinking and iteration controls.
  Optional capture recipes live in [validation](docs/validation.md#focused-local-checks).
- [ ] Qualify Find/sidebar query synchronization, Unicode/long/title-only results,
  Clear/Close, focus and preserved local Find edits/navigation across live changes.
- [ ] Fix and replay
  `conversation_history_escape_clears_before_drawer_dismissal_during_generation`:
  egui drops text-field focus before the search Escape guard, leaving the query
  uncleared.
- [ ] Qualify more than 16 conversations, branching/saving/reopening, virtualized
  live search and bounded encrypted storage while preserving drafts, settings,
  history, selection and keyboard focus.
- [ ] Qualify edited/regenerated/interrupted reply branches, Continue and exact
  committed-prefix submission through Stop, recovery, background ownership,
  completion races, eviction and capacity limits.
- [ ] Qualify composer context estimates/rings and pending states through draft,
  reasoning, branch and recovery changes; preserve Extra effort first-pass scope,
  reply settings, Start fresh and authoritative broker rejection.
- [ ] Qualify conversation switching/creation/deletion during startup, generation
  and failure: preserve drafts/cursors, route hidden responses/rejections to their
  owner and keep speech readiness independent of text readiness.
- [ ] Qualify encrypted conversation, window and voice-profile persistence,
  opt-out, unreadable-byte preservation, monitor/DPI changes and no automatic
  request replay, microphone capture or playback after restore.
- [ ] Qualify Qwen3-ASR microphone permission, explicit/25-second completion,
  autodetection, cancellation and exactly one ordinary Swift submission. Cover
  unavailable-model drafts, late results, ASR/TTS exclusion and recorder cleanup.
- [ ] Qualify the 30-second successful-input ASR lease: state reset, authenticated
  readiness/listener ownership, monitoring and cancellation/failure/expiry release.
  Compare repeated-input latency/resources with cold loading under matched power.
- [ ] Qualify source-built ASR DOTPROD/I8MM selection against ARMv8-A: independent
  feature discovery, unknown-feature fallback, cache separation, complete
  transcript parity and matched microphone/file latency.
- [ ] Qualify audio-file picker/paste, supported Windows decoders, rate/channel
  conversion, long files, unknown/changing-memory admission, full transcripts,
  draft insertion, exactly-once ordinary submission and preserved drafts without
  deferred Send under the
  [audio contract](docs/streaming-resources.md).
- [ ] Qualify persistent sequential Media Foundation decoding, one-segment
  lookahead, exact EOF/tail coverage, pause-aware cuts, failure/cancellation/flush
  cleanup and transcript-viewer behavior. Measure total import latency and
  boundary-word accuracy before making benefit claims.
- [ ] Run the named pause-boundary, segment-conservation, sequential-decoder and
  sequential-stream controls and final native analysis on the settled source.
- [ ] Qualify speech after X1/X2 data-directory moves, CPU-feature or Windows
  changes, retaining pinned-model loading/tokenization/sampling, workspace/lease,
  listening and lifecycle gates.
- [ ] Qualify full selected context/output, long histories, speech, offline
  restart, memory pressure and recovery. Independently exercise requested
  `bounded_history` and `stalled_consumer` workloads under the acceptance contract.
- [ ] Extend [transcript/Find](docs/streaming-resources.md#native-chat-and-transcript)
  device acceptance through appends, replacements, switches, eviction, Unicode
  wrapping, saturated results, selection and queued navigation.
- [ ] Complete signed-distribution acceptance: binary provenance, dependency and
  license notices, clean-machine prerequisites, installation, upgrades and rollback.

### Isolation, transport and recovery

- [ ] Establish [restricted inference authority](docs/sandboxing.md#worker-authority):
  approved read-only artifacts, bounded private cache/temp, denied unrelated
  files/credentials and egress, broker-only communication and no same-user bypass.
  Qualify MXC loading, atomic Job limits and exact identity without weaker fallback.
- [ ] Qualify trusted-host artifact ancestor sharing and nested rename/DELETE
  regressions through native/AppContainer integration; host guards must outlive
  contained readers and preserve their narrower rights.
- [ ] Bound scratch, PSEC-private temp and stdout/stderr storage; qualify
  exhaustion, diagnostic/credential retention and owner-loss cleanup.
- [ ] Qualify the [broker contract](docs/broker-policy.md): authentication, numeric
  loopback, Host/Origin/routes, rejected tools/loading/overrides, queue/byte/token
  bounds, rate limits, absolute operation deadlines and disconnect/stalled-consumer
  cancellation. Cover complete 429 responses, constant-space draining and explicit
  resend without replay; ordinary generation retains no elapsed-time cap.
- [ ] Qualify actual-model [streaming/audio resources](docs/streaming-resources.md):
  ready prefixes, chat/speech overlap, PCM/WinMM reuse, final-storage intake,
  fragmented framing, caps/tails/truncation, failed writes, driver refusal,
  Stop/recovery, task latency and peak memory.
- [ ] Qualify all 33 Swift control-token spellings in real template/tokenizer
  sessions, including escaped/split content, history/reasoning and lookalikes;
  require fixed-400 recovery and identical count/generation handling. Qualify the
  reference vocabulary separately under the [token contract](docs/oxibonsai-reuse.md).
- [ ] Qualify load/allocation/backend-loss recovery, bounded restore attempts and
  unchanged workspace on failure, with no restart loops, silent profile changes,
  corruption or automatic cloud fallback.

### Durable qualification evidence

- [ ] Deploy native `evidence-archive` selection/seal/restore to durable owner
  storage, finish CLI/analyzer/guard acceptance, capture selected required bytes
  before artifact expiry and rehearse digest-verified retrieval without the
  original runner. Follow [evidence retention](docs/validation.md#evidence-retention)
  and retain independent reviews and losing/no-go outcomes.

## Runtime and correctness

- [ ] **R2 — Qualify settled Prism source.** Complete pinned-compiler Foundation
  checks for all 12 crates: strict Release Clippy/rustdoc, tests, builds and
  public-reference intake/lifetime controls under [Prism conformance](docs/prism-upstream-tests.md).
- [ ] Qualify released private HTTP and internal-worker counting, adapter
  readiness/scales, active-graph cancellation, reset/rollback and stale-state
  rejection, with independently retained Prism-owned receipts.
- [ ] Run actual-PQ2 `swift_model_check rollback`: compare every token/logit and
  complete KV/GDN/convolution bits before reset, then continuation against fresh
  computation under the [runtime contract](docs/native-rust-engine.md).
- [ ] Publish the reviewed Windows read-consumer registry API, replace Prism
  server's Git dependency and validate the package before re-enabling publication.
- [ ] Qualify full-context/output scalar/NEON behavior, rank-one zero/nonzero
  adapter application and writer basis, sampling, termination and full-state
  lifecycle at the declared numerical tolerance.
- [ ] **R3 — Share complete-state/fault tooling.** Map each prefix, prefill,
  speculation and accelerator path to its own independent logits, KV/GDN/conv,
  sampler, restore, adapter and resource assertions under
  [qualification reuse](docs/validation.md#reusing-qualification-controls).
- [ ] Use batch-one attribution to select allocation/copy work. Qualify existing
  reset checkpoints, top-20/finite-logit checks, rank-one/attention changes,
  immutable logits and BPE/special parsing under the [engine contract](docs/native-rust-engine.md)
  and [performance contracts](docs/performance-deliver.md), including
  [paired values](docs/performance-close.md), [KV/FFN reuse](docs/performance-finish.md)
  and [paired prefill](docs/performance-resolve.md).
- [ ] Qualify 8/16/32-token CPU prefill, distinct `gdn-wy-f32-v1` arithmetic,
  selected-cache service consumers and independent packed-KV head workers:
  scratch/admission, complete state, adapters, cancellation and whole-request benefit.
- [ ] Qualify six-head/paired-token score/value reuse. Extend score/value or
  online-softmax work only if selected-context attention is a measured bottleneck.
- [ ] Qualify explicit [latest-prefix reuse](docs/prefix-reuse.md) across service,
  broker and UI: edited/switched histories, scale changes, cancellation/faults,
  fresh-state parity, bounded eviction/resources and repeated-history benefit.
- [ ] Qualify the batched target verifier and integrated Swift sampled suffix
  consumer: per-position logits, actual sampler/RNG progression, checkpoint
  admission, accepted-prefix restoration, EOS/Stop/failure and complete-cycle benefit.
- [ ] After a useful verifier, screen short suffix chains, then a compatible
  published Bonsai DFlash2 drafter, then bounded adaptive length. Charge target
  features, shared embeddings/head, adapters/tokenizers, both models and rejected
  work under [speculation contracts](docs/inference-algorithms.md#hybrid-cache-reuse-and-speculative-state).
- [ ] Apply reviewed [cache/batch/transfer planning](docs/inference-algorithms.md#cache-batch-and-transfer-planning)
  to selected consumers: complete page identities, one cache owner, bounded
  transfer queues, terminal completion and cancellation before publication/reuse.
  Qualify fault paths, resources and whole-request benefit.
- [ ] Consider FP32 factor buffers or SpecLA/TreeWY/RecoverSSM recovery only for
  measured state/checkpoint cost; preserve complete checkpoints and numerical scope.

## Complete C++ retirement

- [ ] **S1 — Qualify Rust TTS.** Compare pinned Q8/Q4 models against independent
  controls: tokenizer, Philox/MSVC tie behavior, graph/cache/compute leases,
  Design teardown/Base extraction, voice identity, audio/listening, cancellation,
  recovery and resources under [retirement rules](docs/cpp-retirement.md).
- [ ] Qualify TTS packed projections/worker teams, codec transformer/output tiles,
  convolution/ConvNeXt and exact Base-prefix reuse for cold/repeated/edited,
  voice-changed and failed utterances; diagnose the observed latency regression.
- [ ] Implement the signed Rust `hexagon-native-rpc` dispatcher, immutable hybrid
  placement/cache ownership and contained HTP0 execution. Qualify buffer lifetime,
  completion fences, cancellation and teardown; packet serialization alone does
  not establish dispatch.
- [ ] **S2 — Replace ASR's native engine in Rust.** Retain pinned Qwen3-ASR 1.7B
  controls and qualify transcript/state, microphone-to-Swift submission, quality,
  cancellation, lifecycle and resources before removing native dependencies.
- [ ] Retire remaining Runner/Prism native/reference/DLL/publisher families
  [file by file](docs/cpp-retirement.md): replace source assertions, geometry IDs,
  codec/ABI, state and fallback obligations with independent controls, then remove
  compiler/provisioning/build paths. Retain required replay data.

## Creative quality, adapters and memory

### Quality and speech

- [ ] **Q1 — Prepare independent creative/speech review packets.** Use complete
  answers/audio and declared writing/art-direction rubrics, including Thinking,
  output settings and Extra effort. Reuse applicable retained captures without
  transferring their verdicts to changed source.
- [ ] Qualify [standard LoRA](docs/swift-lora.md#standard-profile-qualification)
  writer basis, loaded-zero/no-adapter identity, nonzero effect and application
  at scale 1.3; retain scale-specific throughput, complete-answer quality and RAM.
  Run the full 28-sample LoRA/14-sample PTQ1 matrix only in its owning CI, or select
  one independently useful bounded capture.
- [ ] Qualify Q8/Q4 speech and thread choices, punctuation/live read-aloud,
  sampling/accent/reference changes and boundary smoothing. Distinguish retained
  PCM continuity from audible continuity under [speech measurement](docs/performance-expectations.md).
- [ ] Qualify chat, warm TTS and overlap: idle/active/ASR monitoring, 30-second
  leases, Stop/wake, shared threads, memory, heat and comfort.
- [ ] After ordinary Q4 acceptance, evaluate a verified 48 kHz decoder only with
  compatible locked weights, license, reference-audio/PCM/UI changes and listening
  acceptance.
- [ ] After Q4 acceptance and compatible runtime/Hadamard artifacts, evaluate
  ternary Talker separately; do not infer compatibility from repacked Q8/Q4 or
  extend it to codec/head/predictor without independent work.
- [ ] Complete [weight-space fault assessment](docs/weight-space-programming.md)
  and controls first. A diagnosed fault may activate disabled steering/capture/
  fitting, held-out quality, rollback and resource gates.
- [ ] Qualify source-excerpt relevance, citations and author-reviewed complete
  scene packets before adding embeddings or optional summaries. Summaries require
  independent source/version and author approval.

### Memory and profiles

- [ ] **P1 — Run public adapter reconstruction.** Execute the declared all-48
  A-factor [public-pack recipe](docs/adapter-public-reference.md), then independently
  replay retained comparisons with the schema-2 scalar auditor. Separately obtain
  authenticated original checkpoint/conversion/invocation/toolchain inputs and
  verify native writer mappings through [authoring intake](docs/adapter-authoring-intake.md).
- [ ] Qualify active-release PTQ1 scalar/explicit-NEON logits and complete live
  state for the selected worker policy, including loaded-zero identity, nonzero
  effect, scale/history changes and prefix reuse.
- [ ] Prepare a newly source-bound PTQ1 service/rollback packet for the selected
  runtime. Qualify scale/count/context rejection, active interruption, independent
  fresh-owner replay, original-PQ2 rollback and final file verification under
  [application acceptance](docs/validation.md#actual-model-application-acceptance).
- [ ] Close matched complete-answer/per-prompt adapter benefit, independent human
  quality/comfort, full context/output, resources, isolation, recovery and clean
  installation before reconsidering [PTQ1 default promotion](docs/swift-memory.md#release-decision).
- [ ] Qualify source request-timing attribution after owning hosted checks; use
  actual operation measurements before selecting hotspots.
- [ ] Investigate adapter reservation/rank-one allocator counters and lossless
  accumulation, preserving rounding/order, dynamic scales and load-without-apply
  under [allocation semantics](docs/swift-lora.md#allocation-semantics).
- [ ] Qualify the development Qwen reference service only when its 18 GiB runtime
  budget plus 4 GiB host reserve is admissible; retain explicit model selection.
- [ ] Qualify creative/daily/long-context/battery profiles with complete-request
  memory, energy, latency and comfort measurements under [profile policy](docs/hardware-capabilities.md).

## Native context expansion roadmap

- [ ] **M1 — Qualify memory-selected context.** Test paired context/memory child
  arguments, unknown/insufficient/changing RAM, prelaunch admission, Job pressure
  teardown, Retry and UI capacity reporting under the
  [memory contract](docs/swift-memory.md#memory-selected-serving-context).
- [ ] **M2 — Qualify cache-aware accounting and staged 64K/128K windows.** Cover
  selected-cache process/state/checkpoint bounds, native sequential scratch,
  two-cache rollback, full-state restore, adapters, cancellation, quality,
  resources and complete-request latency.
- [ ] **M3 — Qualify the scoped 262,144-token K4/V4 profile.** Test complete
  checkpoints, edited/divergent/reused histories, adapter changes, Stop/Retry,
  resource peaks, ingestion latency and held-out long-range quality. Keep generic
  allocation and FP16 bounds separate.
- [ ] Synchronize runtime/tokenizer, broker admission, capability/readiness and
  UI reporting through exact-source releases; count prompt, reply and template
  reserve without silent trimming. Preflight workload feasibility before selecting
  larger-window captures.
- [ ] Deliver K4/V3 only after a compatible reviewed release opens its separate
  exact-source gate; qualify held-out retrieval, instructions and creative
  continuation at the selected capacities, including adapter effects.
- [ ] Qualify capacity-first CPU placement with the selected cache/reuse path;
  measure distinct usable headroom from omitted NPU payloads and the latency
  tradeoff before reducing fixed memory charges.
- [ ] If allocation measurements identify avoidable retained workspace or overlap,
  remove it and qualify actual usable savings. Credit PTQ1 resident savings and
  unique-page sharing only when measured, without double-counting physical bytes.
- [ ] Qualify native long drafts, whole-edit rejection, undo/redo, recovery,
  source insertion and complete rejected/interrupted-draft restoration. Before
  raising serialized request limits, coordinate Prism server/message/tokenizer/
  worker limits with Runner request/recovery bounds and qualify Unicode/escaped
  byte limits as well as exact token limits.

## Bounded calibration and optional precision

- [ ] **K1 — Qualify K4/V4 independently on X1/X2.** Complete the
  [three-task paired packet](docs/quantized-kv.md#first-practical-promotion-packet)
  for held-out retrieval, instructions and creative continuation: at least 65%
  lower measured KV storage, 128 MiB lower near-full peak private bytes and no
  more than 10% latency regression where timing is qualified. Retain explicit
  fresh-session FP16 recovery; qualify larger windows separately under M1–M3.
- [ ] Close selected-profile packed-attention, scalar/NEON, exact quantized
  checkpoint, prefix/adapter/cancellation and discovery-inventory gates. Charge
  scales, rotations, scratch and retained copies; preserve FP32 GDN/conv state.
- [ ] Only for material recurrent-state traffic/storage, compare FP16 recurrence
  with FP32, then at most one DAMP-inspired split; qualify trajectory error,
  convolution, rounding/RNG, restore and total benefit before extra factors.
- [ ] Only for a demonstrated weight/codec bottleneck, evaluate exact entropy
  coding, random access and decode cost against PTQ1; new codebooks/rotations/
  scales need bounded native layerwise cost and held-out quality preflight under
  [compression constraints](docs/inference-algorithms.md#compression-and-approximation-constraints).

## Hardware selection

- [ ] Extend checked hardware inventory and model-free plans with truthful
  device/driver/kernel, capacity, backend and effective-placement identities
  under [hardware policy](docs/hardware-capabilities.md).
- [ ] Qualify the bounded X2 thread screen and exact-candidate model smoke.
- [ ] Diagnose legacy headless startup; integrate/release the UCRT candidate
  upstream, qualify exact binaries with Unicode model paths, thinking,
  continuation, adapters and cancellation, then repin under the
  [candidate contract](docs/runtime-candidates/README.md).
- [ ] Qualify scalar/NEON, optional ISA, thread topology, wait and power choices,
  with independent fallback and source-bound code generation.
- [ ] Measure event waits versus polling and valid energy telemetry before
  selecting idle/active power policy or claiming energy benefit.
- [ ] Implement the reviewed deterministic profile catalog, ranking and resource
  admission against independently verified common evidence.
- [ ] Connect truthful requested/effective diagnostics and native Auto controls
  only to qualified profiles and explicit supported subsets.
- [ ] Qualify request-boundary invalidation, complete state migration, driver/
  memory changes, device loss and strict-backend failure without silent replay.
- [ ] Qualify X1/X2 automatic operation, clean installation, pressure, upgrades
  and rollback independently; CPU fallback must retain its own acceptance.

## CPU kernels

- [ ] Qualify final-source exact/failure controls, strict native/x64 Release
  analysis/linking, disabled-feature fallback, ARM64 codegen/resources, complete
  model state/logits/adapters/cancellation and matched CPU-only request benefit
  under [CPU acceptance](docs/cpu-kernels.md#numerical-and-resource-acceptance).
  Publish helpers before dependent runtimes; apply [cross-generation promotion](docs/cpu-kernels.md#cross-generation-cpu-promotion)
  only within its reviewed shared-path scope.
- [ ] Publish and run the prepared [native CPU bundle comparison](docs/cpu-bundle-comparison.md):
  retain its fixed 0.1.23 baseline versus registry 0.1.25, FP16 cache, four workers
  and adapter 1.3. Keep thread screening and one-token attribution separate.
- [ ] Qualify CPU/prefill changes already consumed by the comparison baseline
  without requiring another repin; release newer candidates with exact source locks.
- [ ] Qualify [scan-owned values and joined prompt projections](docs/cpu-kernels.md#scan-owned-packed-values-and-joined-prompt-projections).
- [ ] Qualify [prepared packed-KV queries and CPU preparation reuse](docs/cpu-kernels.md#prepared-packed-kv-queries-and-cpu-preparation-reuse).
- [ ] Qualify [X1-oriented candidates](docs/cpu-kernels.md#x1-oriented-source-candidates),
  [next candidates](docs/cpu-kernels.md#next-cpu-candidates) and
  [further candidates](docs/cpu-kernels.md#further-x1-cpu-candidates), retaining each
  path's independent integration, numerical, fallback and benefit gates.
- [ ] Qualify [grouped dispatch, half widening and exact LUT execution](docs/cpu-kernels.md#grouped-dispatch-hardware-half-widening-and-exact-lut-execution).
- [ ] Qualify [complete-row SDOT, fixed attention lanes and bounded GDN phases](docs/cpu-kernels.md#complete-row-sdot-fixed-attention-lanes-and-bounded-gdn-phases).
- [ ] Qualify [scoped PQ2 validation, tiled Q/K/V and temporal GDN](docs/cpu-kernels.md#scoped-pq2-validation-tiled-qkv-preparation-and-temporal-gdn).
- [ ] Qualify direct PTQ1-by-Q8_K arithmetic separately from original-PQ2 and
  scalar controls; preserve packed scales, transforms, block boundaries and FMA order.
- [ ] Qualify explicit opt-in exact LUT integration. Extend cross-token LUT
  consumers only when attribution identifies useful work; include preparation,
  scratch, scalar/NEON identity and complete-request cost.
- [ ] Bound selected CPU screens to at most four NEON layouts on three shapes
  and at most two exact LUT candidates before choosing further integration.
- [ ] Integrate further selected cross-token and 8/16/32-token tiled-prefill
  consumers, preserving transformed bases, causal state, bounded scratch and
  rollback before timing the complete request.
- [ ] Use signed constant floor division only inside a measured parent hotspot
  with unchanged rounding/rejection and charged setup/dispatch costs.
- [ ] Capture a matched complete-request pair and record promotion or hold;
  update README measurements only after a freshly declared, source-bound capture
  with aggregate/per-arm power and cleanup observations.

## Adreno Vulkan

- [ ] Diagnose X2 `VK_ERROR_INCOMPATIBLE_DRIVER` and the separate GDI32 lookup
  error 126; establish their causes before further inference claims.
- [ ] Prepare one native ARM64 OpenCL Swift packed-projection pilot under
  unchanged mitigations, plus a separately locked Vulkan candidate under the
  [Adreno contract](docs/adreno.md).
- [ ] Bind operation support, effective placement, fallback and transfer ledgers
  to selected graph shapes. Qualify PQ2/PTQ1 and no-/approved-LoRA independently,
  including full-state lifecycle and resources.
- [ ] Compare matched CPU, GPU offload and qualified Hexagon paths with complete
  startup, preparation, transfers, synchronization and request attribution.
- [ ] Optimize measured bottlenecks only, including packed kernels, reusable
  weights, cache/lifetime accounting and bounded scratch.
- [ ] Qualify full-device benefit, pressure/device-loss/cancellation recovery,
  rollback, resources and safety before selecting a GPU profile.

## Hexagon NPU

- [ ] Close applicable [signed-runtime/SDK/Windows ABI prerequisites](docs/hexagon-source-audit.md)
  and independent raw-byte replay before expanding a selected device workload.
- [ ] Qualify the implemented six-arena/168-projection `native-q2` placement:
  exact selected tensors, adapter/full-state parity, arena admission, load costs,
  fallback, persistent lifecycle and complete-request benefit under [Hexagon policy](docs/hexagon.md).
- [ ] Qualify existing full-FFN chains, sibling packets, multi-tile N=1 and
  tile-major reuse independently. Implement true N>1 prefill only with a supported
  signed solver, causal state, bounded VTCM/DMA and measured total-cost benefit.
- [ ] Integrate remaining RMSNorm/RoPE/attention/conv/GDN VTCM chains only where
  signed operation support, transformed bases, full-state ownership and measured
  benefit justify placement.
- [ ] Extend guarded-Q2/HMX projection controls to broader live activations,
  shapes, tokens, adapters, logits and complete state. Preserve original Q8_K and
  per-128 FMA, every-limb/plane comparisons, selected DSP identity, cleanup and
  source-bound numerical acceptance under [packed arithmetic](docs/hexagon.md#packed-arithmetic-and-graph-state).
- [ ] Resolve persistent guarded-bridge session admission through explicit partial
  placement or a reviewed supported lifetime/fused design. Account for all prompt/
  output/attempted operations; resets must not replenish the terminal cap.

### Promotion sequence

- [ ] **NPU-1 — Prepare the qualification packet.** Freeze Runner/Prism/compiler/
  executable, original-PQ2/adapter, X2 SKU, driver/runtime/catalog and DSP identities;
  close applicable prerequisites and retain independent raw inputs, outputs,
  hashes, oracles, cleanup codes and driver-owner lifetime policy.
- [ ] **NPU-2 — Qualify arithmetic and adapters.** Compare every offloaded result,
  final logits and complete state with the same-artifact CPU oracle for varied
  prompts/shapes/generated history, selected scalar/NEON/four-worker policy,
  loaded-zero identity and scale-1.3/change-history cases. PTQ1/Q8_0 stay separate.
- [ ] **NPU-3 — Qualify placement, resources and lifetime admission.** Bind actual
  DSP/VTCM/DMA/mapping/cache behavior to tensors; charge host/device resources
  outside Job commit. Reserve whole-request work and qualify the guarded bridge's
  24,000-attempt lifetime or a separately reviewed serving design before admission.
- [ ] **NPU-4 — Qualify persistent serving and failures.** Cover fresh/reused/
  edited prefixes, full context/output reservations, repeated requests,
  cancellation before/after submission, Stop/Retry/reset, queue saturation,
  unsupported shapes, pressure, allocation/mapping failures, loss and stale caches.
  Require complete committed-state rollback and verified teardown; preserve
  uncertain buffers until terminal completion or supervised exit.
- [ ] **NPU-5 — Establish end-to-end benefit.** Select measured bottlenecks and
  compare matched CPU/default settings with repeated complete answers, prefill/
  decode, resources, quality and comfort. Charge startup, transfers, RPC and
  synchronization; require the [promotion benefit](docs/validation.md#promotion-criteria)
  and eligible matched power conditions.
- [ ] **NPU-6 — Qualify the explicit application profile.** Release/pin supported
  Rust helpers and signed kernels; verify broker/launcher placement/readiness,
  authentication/counting/one-slot/cancellation, child-only environment, override
  rejection and isolation beyond one Job. Cover clean install, absent/changed
  drivers/runtime, upgrades, pressure and original-PQ2 CPU rollback.
- [ ] **NPU-7 — Independently review and promote the scoped release.** Score all
  correctness, adapter, lifecycle, resource, quality, benefit, packaging/isolation
  and rollback gates against one settled source/release/device tuple. Retain
  numerical/human review, supported subsets and no-go cases; requalify after
  relevant identity changes.
- [ ] **NPU-8 — Qualify later expansion separately.** Add common-evidence
  request-boundary CPU/GPU/NPU selection only after backend/resource/fallback
  acceptance. Qualify HMX, PTQ1, X1 and Adreno independently of an explicit X2
  partial-HVX profile.
- [ ] Complete sustained qualification only in an eligible environment where
  setup, every arm, power, cleanup and retention fit the original allowance;
  leave it pending when the laptop budget cannot support it.

## Survey ideas outside the current laptop backlog

- [ ] **F1 — Conditional research.** After a useful verifier and compatible
  published auxiliary exist, screen trained parallel/phrase proposals with
  admitted native resources. After a useful
  fixed chain, consider scheduling, rejected-suffix conditioning or overlap only
  with required target features and independent hardware capacity under the
  [research scope](docs/research-register.md#research-catalog).
- [ ] Reopen exact full-precision weight codecs only for a suitable measured
  bottleneck; exact FP16 KV codecs only for material selected-context traffic;
  transfer/inactive-session codecs only for a selected capacity or transfer need.
- [ ] Reopen approximate checkpoint omission only if exact reuse and suffix
  recomputation cannot meet a demonstrated need. Wide trees, SVD caches and sparse
  indexes need a measured bottleneck and bounded native benefit.
- [ ] Consider [learned corpus retrieval](docs/research-register.md#conditional-learned-corpus-retrieval)
  only after embeddings are justified and vector search is a measured bottleneck;
  qualify passage relevance, complete-task cost, resource bounds and fallback.

## Future Qwen MoE migration

- [ ] After Qwen 4 and the corresponding Bonsai upgrade are available,
  authenticate actual architecture/artifacts and establish native Rust support.
- [ ] Evaluate justified expert capacity/tiering and download-space preflight
  under the [conditional MoE design](docs/model-selection.md#conditional-future-bonsai-moe-design),
  then qualify state/resources, quality, Stop/Retry, cleanup, native application
  behavior and practical benefit with explicit Swift rollback.

## Deferred Prism migration

- [ ] Reopen broader Prism work only for a Swift dependency or chosen milestone;
  close reference provenance/backend, geometry/performance and Windows-source
  inventories under [conformance](docs/prism-upstream-tests.md).
- [ ] Replace remaining numerical/codec operations, callbacks, backward graphs,
  importance encoders and owned `qb-*` APIs when selected.
- [ ] Replace selected chat/parser/template/tokenizer and capability/server
  conversions with complete independent source contracts.
- [ ] Replace selected acquisition/CLI/model/session/architecture/state/concurrency
  APIs with independently qualified native implementations.
- [ ] Close selected DSpark, LoRA conversion/merge/inference, grammar and safe
  public ABI work, preserving approved corrections and compile-only scope.

## Deferred dependency experiments

- [ ] Reopen MXC supervisor, wgpu backend removal or archive splitting only for
  demonstrated need and support/device/fallback/build-size acceptance under the
  [dependency audit](docs/dependency-audit.md).
- [ ] Integrate candidate speech SIMD projections, parallel projections,
  four-frame streaming and FastConformer transpose into serving before model
  qualification; qualify direct `memchr` transport scans independently.
- [ ] Qualify [SIMD-JSON tape reuse](docs/dependency-audit.md#current-candidate-and-pending-gate)
  in owning main-only CI: Unicode/duplicates/unknowns/shapes/bounds/canonical bytes,
  at least 5% lower equal-weight geometric-mean time and no case over 5% slower in
  both cold/reused screens, including scales, before changing the dependency choice.

## Optional agents

- [ ] After creative-chat qualification, evaluate OpenCode, then Aider, through
  a bounded authenticated inference bridge.
- [ ] Qualify MXC ProcessContainer isolation for agents/plugins/hooks/descendants
  with `allowDaclMutation:false`, file/reparse/handle/subprocess/network bypass
  checks and no unsandboxed fallback. Qualify a separate-kernel VM tier separately.
