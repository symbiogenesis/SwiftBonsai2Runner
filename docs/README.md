# Documentation

Start with the [development guide](development-guide.md) for setup, launch and
developer commands, or [portable packages](chat-packages.md) to run without a
checkout. [PLAN.md](../PLAN.md) is the single backlog; these guides own current
behavior, technical contracts and retained evidence.

The current application uses a native Rust UI, pinned Prism 0.1.23 inference on
Windows ARM64, a CPU baseline with experimental X2 resident Hexagon acceleration,
and a strict local HTTP/JSON/SSE subset. Experimental runtimes, formats and
accelerators require separate qualification. Evidence scopes below distinguish
implemented behavior from actual-model, device, quality and isolation acceptance.

## Use and develop

| Guide | Contents |
| --- | --- |
| [Development](development-guide.md) | Requirements, provisioning, app/settings/speech, developer commands, build caches and runtime lock updates |
| [Portable packages](chat-packages.md) | Assembly, installation, update, rollback and installed launch |
| [Windows releases](releases.md) | Windows ARM64 MSI setup, release assets, upgrades and owner publication |
| [Validation](validation.md) | Local-check policy, CI ownership, standards, regression coverage, acceptance and promotion |
| [Licensing](licensing.md) | Project, dependency, model and font terms; notices and redistribution |

## Application contracts

| Contract | Contents |
| --- | --- |
| [UI architecture](ui-architecture.md) | Rust/native boundary, launch ownership, conversation state and speech responsibilities |
| [Broker](broker-policy.md) | Public/private protocol, authentication, framing, admission, completion and recovery |
| [Streaming resources](streaming-resources.md) | Text/Find, HTTP, recovery and speech/PCM storage, work bounds and ownership |
| [Windows sandboxing](sandboxing.md) | Worker authority, Jobs/handles, files/locks, threads, power and containment limits |
| [Dependencies](dependency-audit.md) | Dated security/footprint evidence and parser/runtime selection decisions |

## Models, memory and runtime

| Guide | Contents |
| --- | --- |
| [Swift memory and PTQ1](swift-memory.md) | Artifacts, census, budgets, lossless packing/allocation, numerical evidence, decision screen and approximation boundaries |
| [Swift LoRA](swift-lora.md) | Artifact, basis, cost, storage overlay and paired device/human qualification |
| [Adapter authoring intake](adapter-authoring-intake.md) | Authenticated authoring inputs, schema-2 intake bounds and provenance limits |
| [Public adapter reference](adapter-public-reference.md) | One declared public-pack reconstruction and independent scalar comparison |
| [Weight-space programming](weight-space-programming.md) | Swift applicability, compact PQ2/PTQ1 correction mechanism and evidence needed before a weight edit |
| [Qwen reference](model-selection.md) | Selection rationale, compatibility/memory checks and stop/reopen conditions |
| [Native Rust engine](native-rust-engine.md) | Supported graph/API, numerical/state ownership, runtime overhead and promotion evaluator |
| [Experimental prefix reuse](prefix-reuse.md) | Explicit latest-state launch, exact runtime capability/readiness and lifecycle qualification |
| [C++ retirement](cpp-retirement.md) | File-by-file inventories, independent replacement controls, dependency/build cleanup and remaining integration gates |
| [Prism conformance](prism-upstream-tests.md) | Source fidelity, broader upstream acceptance and pinned source-reuse review |
| [OxiBonsai review](oxibonsai-reuse.md) | Pinned source/license, reusable regressions, arithmetic/security transfer limits |
| [Runtime candidates](runtime-candidates/README.md) | Independent row-parallel and headless-startup patches, manifests, evidence and reproduction |

## Hardware and inference

| Guide | Contents |
| --- | --- |
| [Hardware discovery and profiles](hardware-capabilities.md) | Implemented inventory, evidence identities, selection eligibility, phase placement and fallback |
| [CPU kernels and X2 screen](cpu-kernels.md) | Packed arithmetic, ISA dispatch, thread-screen controls and device evidence |
| [Prefill, snapshots and FFN delivery](performance-deliver.md) | Source contracts, conservative resource charges and qualification limits |
| [Paired values, bounded parsing and resident packets](performance-close.md) | Source behavior and remaining qualification boundaries |
| [KV sharing, terminal ingestion and resident FFN](performance-finish.md) | Source changes and qualification limits before release and repin |
| [Paired prefill, BPE reuse and tiled projections](performance-resolve.md) | Request-local reuse, native-Q2 token tiles and pending release/qualification |
| [Adreno and OpenCL](adreno.md) | Vulkan/compute access, placement, native OpenCL query, X2 observations and source audit |
| [Hexagon](hexagon.md) | Signed deployment, packed execution/state, memory and failure contracts |
| [Hexagon sources and evidence](hexagon-source-audit.md) | Pinned packages, prototype placement, X2 access, signed Q2 arithmetic and SDK/public wire provenance |
| [Inference algorithms](inference-algorithms.md) | LUT/prefill/GDN, attention, hybrid cache and speculation constraints |
| [Inference defaults](inference-defaults.md) | Sampler/thinking/output settings, prompt harness and quality limits |
| [Performance and speech](performance-expectations.md) | Publisher benchmark context, paired speech results and latency/memory/energy measurement |
| [Research register](research-register.md) | Dated sources, mathematical proof obligations, applicability, negative results and experiment review |

Retained JSON receipts live in [evidence/](evidence/), candidate manifests and
patches in [runtime-candidates/](runtime-candidates/README.md), and charts in
[assets/](assets/). Owning guides explain their scope and reproduction; file
presence alone is not qualification. Add prose to its owning topic, link shared
requirements and put outstanding work in PLAN.

Keep evidence that supports current claims, records useful device failures or
provides replay inputs consumed by tools. Completed source-review diaries,
temporary validation drivers and duplicate build/release payloads belong in Git
history. Trimmed historical summaries identify the original revision and state
when raw replay requires that history; they do not qualify current source.
The evidence tree before this cleanup is available at commit `3c03fbfe`.
