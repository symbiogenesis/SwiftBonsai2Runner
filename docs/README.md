# Documentation

[Application controls and development](development-guide.md) covers use and local
builds. [Windows distribution](releases.md) covers the portable format, setup,
updates and publication. [PLAN.md](../PLAN.md) is the single backlog.

## Application contracts

| Document | Owns |
| --- | --- |
| [UI architecture](ui-architecture.md) | Native Rust boundary, launch, conversation state and speech |
| [Broker](broker-policy.md) | HTTP/JSON/SSE subset, authentication, admission and recovery |
| [Streaming resources](streaming-resources.md) | Text, Find, HTTP, recovery and audio bounds |
| [Windows sandboxing](sandboxing.md) | Worker authority, Jobs, handles, files and containment limits |
| [Portable packages](chat-packages.md) | Developer assembly, checked-directory activation and rollback |
| [Validation](validation.md) | Checks, acceptance, promotion and evidence retention |
| [Dependencies](dependency-audit.md) | Dependency selection and security checks |
| [Licensing](licensing.md) | Code, model, font and redistribution terms |

## Models, memory and runtime

| Document | Owns |
| --- | --- |
| [Swift memory and PTQ1](swift-memory.md) | Artifact formats, context selection, packing and allocation |
| [Quantized KV](quantized-kv.md) | K4/V4, optional K4/V3 and qualification |
| [Swift LoRA](swift-lora.md) | Adapter basis, cost, overlay and qualification |
| [Adapter authoring intake](adapter-authoring-intake.md) | Authenticated inputs, bounds and provenance |
| [Public adapter reference](adapter-public-reference.md) | Public-pack reconstruction and scalar comparison |
| [Weight-space programming](weight-space-programming.md) | Compact corrections and weight-edit evidence |
| [Model selection](model-selection.md) | Qwen compatibility, memory and future MoE scope |
| [Native Rust engine](native-rust-engine.md) | Graph/API, numerical state, overhead and evaluation |
| [C++ retirement](cpp-retirement.md) | Native dependency boundaries and replacement controls |
| [Prism conformance](prism-upstream-tests.md) | Independent upstream acceptance and source reuse |
| [Runtime candidates](runtime-candidates/README.md) | Headless-startup candidate and reproduction |

## Hardware and inference

| Document | Owns |
| --- | --- |
| [Hardware profiles](hardware-capabilities.md) | Discovery, selection, placement and fallback |
| [CPU kernels](cpu-kernels.md) | Packed arithmetic, ISA dispatch and promotion |
| [CPU bundle comparison](../tools/cpu-bundle/README.md) | Fixed workload, preparation and comparison limits |
| [Adreno](adreno.md) | Vulkan/OpenCL access and placement |
| [Hexagon](hexagon.md) | Signed deployment, packed execution, memory and failures |
| [Hexagon source audit](hexagon-source-audit.md) | Package, SDK and wire provenance |
| [Inference algorithms](inference-algorithms.md) | Prefill, attention, hybrid state and speculation |
| [Inference defaults](inference-defaults.md) | Sampling, Thinking, output and prompt harness |
| [Performance](performance-expectations.md) | Latency, memory, energy and speech measurement |
| [Research](research-register.md) | Sources, proof obligations and applicability |

[Retained evidence](evidence/README.md) lists fixture, policy and replay consumers.
