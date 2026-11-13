# Documentation

Start with [Windows releases](releases.md#download-and-open): download the portable
executable and open it; setup and retry happen inside the app. The
[application and development guide](development-guide.md) covers controls and
separate contributor commands. [PLAN.md](../PLAN.md) is the single backlog. These
guides describe current behavior and technical contracts; Git history holds
completed work, superseded release notes and trial diaries.

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
| [Dependencies](dependency-audit.md) | Dependency boundaries, security checks and parser/runtime selection |

## Models, memory and runtime

| Guide | Contents |
| --- | --- |
| [Swift memory and PTQ1](swift-memory.md) | Artifacts, memory-selected context, lossless packing, allocation and numerical requirements |
| [Native Snapdragon quantized KV](quantized-kv.md) | K4/V4 development default, optional K4/V3, X1/X2 boundaries, storage estimates and pending measured qualification |
| [Swift LoRA](swift-lora.md) | Artifact, basis, cost, storage overlay and paired device/human qualification |
| [Adapter authoring intake](adapter-authoring-intake.md) | Authenticated authoring inputs, schema-2 intake bounds and provenance limits |
| [Public adapter reference](adapter-public-reference.md) | One declared public-pack reconstruction and independent scalar comparison |
| [Weight-space programming](weight-space-programming.md) | Swift applicability, compact PQ2/PTQ1 correction mechanism and evidence needed before a weight edit |
| [Qwen reference](model-selection.md) | Selection rationale, compatibility/memory checks and stop/reopen conditions |
| [Native Rust engine](native-rust-engine.md) | Supported graph/API, numerical/state ownership, runtime overhead and promotion evaluator |
| [Experimental prefix reuse](prefix-reuse.md) | Explicit latest-state launch, exact runtime capability/readiness and lifecycle qualification |
| [C++ retirement](cpp-retirement.md) | Remaining native dependencies and independent replacement controls |
| [Prism conformance](prism-upstream-tests.md) | Source fidelity, broader upstream acceptance and pinned source-reuse review |
| [Runtime candidates](runtime-candidates/README.md) | Headless-startup patch, source manifest and reproduction |

## Hardware and inference

| Guide | Contents |
| --- | --- |
| [Hardware discovery and profiles](hardware-capabilities.md) | Implemented inventory, evidence identities, selection eligibility, phase placement and fallback |
| [CPU kernels and X2 screen](cpu-kernels.md) | Packed arithmetic, ISA dispatch, thread-screen controls and qualification |
| [Native CPU bundle comparison](cpu-bundle-comparison.md) | Fixed 0.1.23 versus registry 0.1.25, hosted preparation, owner workload and comparison limits |
| [Adreno and OpenCL](adreno.md) | Vulkan/compute access, placement, native OpenCL query and qualification limits |
| [Hexagon](hexagon.md) | Signed deployment, packed execution/state, memory and failure contracts |
| [Hexagon sources and evidence](hexagon-source-audit.md) | Pinned packages, prototype placement, X2 access, signed Q2 arithmetic and SDK/public wire provenance |
| [Inference algorithms](inference-algorithms.md) | LUT/prefill/GDN, attention, hybrid cache and speculation constraints |
| [Inference defaults](inference-defaults.md) | Sampler/thinking/output settings, prompt harness and quality limits |
| [Performance and speech](performance-expectations.md) | Latency, memory, energy and speech measurement requirements |
| [Research register](research-register.md) | Research sources, mathematical proof obligations and applicability |

Current engine optimizations live in [the engine contract](native-rust-engine.md)
and [Hexagon](hexagon.md), alongside their resource and failure requirements.
Keep each contract in its owning topic and link shared requirements.

[Retained evidence](evidence/README.md) has an active release, fixture or replay
consumer. Preserve required bytes, notices and integrity records under the
[evidence-retention policy](validation.md#evidence-retention); file presence alone
does not qualify current source. Routine check results belong in commits or
delivery messages. Candidate manifests and patches live in
[runtime-candidates/](runtime-candidates/README.md).
