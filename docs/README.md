# Documentation

Start with [Windows releases](releases.md#download-and-open): download the portable
executable and open it; setup and retry happen inside the app. The
[application and development guide](development-guide.md) covers controls and
separate contributor commands. [PLAN.md](../PLAN.md) is the single backlog; these guides own current
behavior, technical contracts and retained evidence.

The app uses a native Rust UI, pinned Prism 0.1.27 on Windows ARM64, CPU inference
with experimental X2 resident Hexagon acceleration, and a strict local HTTP/JSON/SSE
subset. Implementation does not establish model, device, quality or isolation
acceptance; experimental runtimes, formats and accelerators need separate qualification.

## Use and develop

| Guide | Contents |
| --- | --- |
| [Windows releases](releases.md) | Download, in-app setup and retry, confirmed updates and owner publication |
| [Application and development](development-guide.md) | App/settings/speech controls, separate contributor builds, build caches and runtime lock updates |
| [Portable packages](chat-packages.md) | Developer assembly, checked-directory installation, update, rollback and installed launch |
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
| [Native Snapdragon quantized KV](quantized-kv.md) | K4/V4 development default, optional K4/V3, X1/X2 boundaries, storage estimates and pending measured qualification |
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
| [Native CPU bundle comparison](cpu-bundle-comparison.md) | Fixed 0.1.23 versus registry 0.1.25, hosted preparation, owner workload and comparison limits |
| [Prefill, snapshots and FFN delivery](performance-deliver.md) | Source contracts, conservative resource charges and qualification limits |
| [Paired values, bounded parsing and resident packets](performance-close.md) | Source behavior and remaining qualification boundaries |
| [KV sharing, terminal ingestion and resident FFN](performance-finish.md) | Source changes consumed by the development pin and remaining qualification limits |
| [Paired prefill, BPE reuse and tiled projections](performance-resolve.md) | Request-local reuse, native-Q2 token tiles and remaining qualification limits |
| [Adreno and OpenCL](adreno.md) | Vulkan/compute access, placement, native OpenCL query, X2 observations and source audit |
| [Hexagon](hexagon.md) | Signed deployment, packed execution/state, memory and failure contracts |
| [Hexagon sources and evidence](hexagon-source-audit.md) | Pinned packages, prototype placement, X2 access, signed Q2 arithmetic and SDK/public wire provenance |
| [Inference algorithms](inference-algorithms.md) | LUT/prefill/GDN, attention, hybrid cache and speculation constraints |
| [Inference defaults](inference-defaults.md) | Sampler/thinking/output settings, prompt harness and quality limits |
| [Performance and speech](performance-expectations.md) | Publisher benchmark context, paired speech results and latency/memory/energy measurement |
| [Research register](research-register.md) | Dated sources, mathematical proof obligations, applicability, negative results and experiment review |

Receipts live in [evidence/](evidence/), candidate manifests/patches in
[runtime-candidates/](runtime-candidates/README.md), and charts in [assets/](assets/).
Owning guides explain scope and reproduction; file presence alone is not
qualification. Keep prose in its owning topic, link shared requirements and put
outstanding work in PLAN.

Follow the [evidence-retention policy](validation.md#evidence-retention): retain
only current claim evidence, actionable failures and required replay inputs.
Routine check results belong in commits or delivery messages. Preserve required
raw bytes and integrity records; leave completed review diaries, temporary drivers
and duplicate payloads in Git history. Historical summaries identify their original
revision and replay limits; they cannot qualify current source.
