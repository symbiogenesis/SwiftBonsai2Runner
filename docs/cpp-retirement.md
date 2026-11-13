# C++ retirement contract and inventory

The milestone removes C++ from SwiftBonsai2Runner and prism-inference, including
tests, controls, generated source, downloaded/bundled libraries, DLL-backed tools
and compiler/build dependencies. [PLAN.md](../PLAN.md) owns remaining work; this
guide defines inventory and retirement gates, not a completion claim.

Preserve native Rust UI and HTTP/JSON/SSE contracts. Assess vendor system/driver
ABIs explicitly; FFI or process isolation does not retire C++. Leave Git history
intact and retain source identities, licenses/notices, required attribution and
qualification evidence. These records need no operative C++ source or compiler calls.

## Replacement and deletion rules

For each file or inseparable family, record Rust replacement, immutable
observations or functionality removal under its existing acceptance contract.
Deleting a control cannot qualify its replacement. Before deletion:

1. Bind the replaced behavior to exact upstream revision, source/patch hashes,
   license, target, toolchain, inputs, configuration and expected observations.
   Capture all approved assertions and failure/lifecycle cases that it supplies.
2. Preserve independent controls through immutable fixtures, mathematical known
   answers or separately implemented Rust references. Fixtures retain complete
   input/output identity, schema, counts and digests. They are historical captures,
   not fresh execution of the retired engine. Reports must identify that provider
   honestly rather than relabeling a capture as a newly passing C++ oracle.
3. Pass focused replacement checks before source removal. Owning hosted consumer
   builds and assertions remain integration completion gates; record them pending
   when only local checks ran. Preserve numerical ordering/tolerances, signed and
   exceptional bits, ISA/target boundaries, subset errors, source IDs, approved green
   floors and exclusions. Source removal alone does not close those gates.
4. Remove operative source includes/generators, compiler/link/download/cache paths,
   feature/dependency edges, workflow/control dispatch and stale documentation for
   that file. Retain evidence and notices as data where needed.
5. Keep full-model, actual-device, quality, resource, isolation and distribution
   gates pending until independently satisfied. Synthetic replay does not close
   them. Owner-device work shares the [aggregate execution budget](validation.md):
   five minutes by default, with any longer dedicated allowance predeclared under
   that policy. Narrower controls keep their own limits. Complete suites and
   benchmarks remain CI-only.

Current [runtime conformance](prism-upstream-tests.md), [native Rust](native-rust-engine.md),
[speech](../src/speech-tts/README.md) and [validation](validation.md) contracts remain
the acceptance inputs. Conformance-provider changes must preserve every approved
green ID and the complete catalog/schedule, including 22,817 ordered backend
evaluation/support geometries and separately selected performance workloads.
Missing captures or unimplemented assertions remain visible pending/red outcomes.

## Runner checked-in native files

The hosted controls `tools/hexagon/public_qaic_wire.c` and `windows_queue_abi.c`
remain C references. They compile against pinned public declarations and immutable
generated observations without linking an inference library, SDK runtime or driver.
Retire compilation only after independent retained ABI/wire observations and Rust
assertions satisfy the owning contract. They are excluded from production inference.

Rust speech controls retain licensed immutable Philox, sampling, tokenizer and
lifetime observations under `src/speech-tts/tests/fixtures/`. Preserve those replay
inputs, provenance and independent assertions. Completed port inventories and their
check results belong in Git history. Current serving and model acceptance are defined
by the [speech source guide](../src/speech-tts/README.md) and [PLAN.md](../PLAN.md).

## Prism checked-in native files

All paths below are under `prism-inference/tools/prism/upstream/common/`.
There are **40 C++ files**: `bridge.cpp` and 39 `.hpp` files. The two public-C-ABI
files are coupled native cleanup, not additional C++ files. Each row must preserve
its existing source-level assertions and contracts through the process above.

| Family | File | Required retained behavior |
| --- | --- | --- |
| Text/API | `vocab_session.hpp` | Stateful vocabulary operations and token/piece buffer contracts. |
| Text/API | `vocab.hpp` | Vocabulary traits, IDs and byte spellings. |
| Text/API | `jinja.hpp` | Template evaluation, capabilities and failure behavior. |
| Text/API | `grammar_parser.hpp` | Grammar parsing and rejection cases. |
| Text/API | `grammar.hpp` | Grammar state, candidate filtering and lifecycle. |
| Text/API | `peg.hpp` | PEG construction, serialization and parsing. |
| Text/API | `json_schema.hpp` | Schema-to-grammar conversions and diagnostics. |
| Text/API | `chat_templates.hpp` | Exact template bytes and selection. |
| Text/API | `chat_parser.hpp` | Incremental content/reasoning parsing. |
| Text/API | `chat_auto.hpp` | Automatic chat-format/capability decisions. |
| Text/API | `chat_analysis.hpp` | Message analysis and source-tested conversions. |
| Text/API | `chat.hpp` | Chat message/request conversions. |
| Text/API | `arg_parser.hpp` | CLI defaults, precedence and rejected options. |
| Text/API | `model_resolution.hpp` | Acquisition/cache/sidecar plans and handler decisions. |
| Text/API | `reasoning_budget.hpp` | Budget lifecycle, forced tokens, clone/state and UTF-8 observations. |
| Text/API | `sampling.hpp` | Complete sampler ordering, draws, candidates and lifecycle. |
| Numerical/ownership | `tensor_graph.hpp` | Tensor construction and graph operation observations. |
| Numerical/ownership | `arena_graph.hpp` | Arena graph ownership, bounds and reuse. |
| Numerical/ownership | `batch_allocator.hpp` | Batch/sequence allocation and failure cases. |
| Numerical/ownership | `allocation.hpp` | Allocation topology, byte counts and cleanup. |
| Numerical/ownership | `threadpool.hpp` | Plans, persistent workers, barriers and failures. |
| Numerical/ownership | `backend_ops.hpp` | Full ordered operation geometry/support/evaluation schedule. |
| Numerical/ownership | `backend_session.hpp` | Backend session state, export and lifetime. |
| Numerical/ownership | `backend_sampler.hpp` | Backend sampler/candidate ownership and source observations. |
| Numerical/ownership | `context_graph.hpp` | Context graph state, construction and execution. |
| Numerical/ownership | `codec_buffers.hpp` | Owned aligned buffers, traces, ranges and primitive codec calls. |
| Numerical/ownership | `codec.hpp` | Codec traits, exact arithmetic and supported layouts. |
| Numerical/ownership | `gguf.hpp` | GGUF metadata/tensor read-write and bounds. |
| Numerical/ownership | `quantization.hpp` | Model quantization decisions and type/shape policies. |
| Numerical/ownership | `optimizer.hpp` | Optimization state, updates and source numerical assertions. |
| Model/multimodal | `model_runtime.hpp` | Actual model/session load, state, inference and recovery contracts. |
| Model/multimodal | `generated_model.hpp` | Generated model metadata and loading observations. |
| Model/multimodal | `dspark_runtime.hpp` | DSpark model/session execution and state. |
| Model/multimodal | `dspark.hpp` | DSpark serialization, adapter and approved correction controls. |
| Model/multimodal | `mtmd_chunks.hpp` | Multimodal chunk ownership and metadata. |
| Model/multimodal | `mtmd.hpp` | Multimodal input, model and API contracts. |
| Model/multimodal | `llguidance.hpp` | Guidance candidate/template behavior and exact external identity. |
| Transport/concurrency | `action_file.hpp` | Bounded file extent/EOF checks and Windows publication sharing. |
| Transport/concurrency | `concurrency.hpp` | Concurrent logger schedules, finalization and cleanup. |
| Dispatch | `bridge.cpp` | Remove each retired family route; retire final dispatcher after all provider migrations. |
| Coupled C ABI | `public_c_abi.h`, `public_c_abi.c` | Preserve target-specific layout/compile observations and negative-call contracts as fixtures/Rust assertions before removing native linkage. |

## External engines, generators and build dependencies

The default TTS service now executes its GGUF intake, Talker/CodePredictor,
codec and speaker graphs in Rust on CPU. `build.rs` performs no native
compilation/linking; source launch and hosted TTS builds no longer provision
Qwen/GGML or their static libraries. Upstream MIT notices and immutable captured
Philox/sampling/tokenizer/cache assertions remain. The former foreign-allocation,
C++ exception and native parameter-ABI checks describe the retired boundary;
Rust buffer ownership, bounded samples, voice bootstrap and service cancellation
need their corresponding Rust/hosted and actual-model acceptance. No historical
native-linked pass qualifies this replacement. Q8/Q4 model parity, listening,
state/lifecycle and memory acceptance remain open in S1.

The optional Hexagon source-overlay helper files and old extern declarations
are not compiled or dispatched by the Rust TTS service. Their presence supplies
no acceleration. Removing these historical developer paths and introducing
signed Rust RPC keep separate inventory and acceptance obligations.

The remaining ASR dependency is `transcribe-cpp` /`transcribe-cpp-sys`, including its bundled
Qwen3-ASR/GGML core. Native Rust replacements
must retain model verification, audio/token contracts, cancellation, state, output
quality and actual-device resource acceptance. Reusing a C++ library's Rust binding
does not satisfy this milestone.

Expand each remaining external family to exact files from its pinned source manifest before porting
it. Preserve immutable source identities, notices and independent model comparisons.
For Hexagon NPU speech and LLM offload, the C++ retirement boundary covers
`qwentts.cpp`, `ggml`, `ggml-base.dll`, `ggml-cpu.dll` and `ggml-hexagon.dll`
(replaced by pure-Rust model/codec execution and `hexagon-native-rpc` `dspqueue` +
`htp_ops` packet submission directly to the signed `libggml-htp-v73.so` /
`libggml-htp-v81.so` DSP skels), while retaining the vendor OS/DriverStore
`libcdsprpc.dll` (`qcnspmcdm`) FastRPC transport ABI (`remote_handle64_*`,
`rpcmem_*`, `dspqueue_*`).

Remove retired paths from `speech-tts/build*.rs`, native-speech provisioning/cache
actions and scripts, `native_startup.rs`/`speech_build.rs`, compiler/source locks,
Cargo edges and package notices where no longer required. Retire operative publisher
routes in `runtime.lock.json`, `swift-assessment.lock.json`, Rust `build-tools`
engine/headless-publisher operations, native capture tooling and candidate patches
only after their replacement/fallback and distribution gates pass.

The PTQ1 Rust collector still uses pinned native DLLs. Retiring its C++ driver
and compiler/header downloads replaces no inference engine and qualifies no new
numerical results. The one-token native/Rust comparison and broader promotion
gates remain required.

Prism's coupled cleanup includes its three `CMakeLists.txt` projects,
`RustBridge.cmake`, `Apply-ReferencePatch.cmake` and production patches; native loader
and LoRA-export linkage; LLGuidance/native codec control downloads; and foundation,
focused/conformance compiler caches, packaging and provider dispatch. Retain safe
Rust functionality and evidence rather than deleting the assertions with the build.

Final acceptance requires source/generated-source inventory, dependency and binary
linkage assessment, clean hosted consumer builds without a project C++ toolchain,
preserved qualification contracts, and no operative references to retired controls.
Historical capture metadata and legally required notices remain distinguishable
from live compiler, download, linkage or execution requirements.

### MSVC startup and panic runtime

TTS serving runs in Rust; Windows MSVC startup and panic support remain a
separate dependency boundary.
[Static C-runtime linkage](https://doc.rust-lang.org/reference/linkage.html#static-and-dynamic-c-runtimes)
can remove the separately installed Visual C++ DLL requirement, while embedding
Microsoft runtime support in the executable. It does not replace those bytes
with a Rust implementation. Inspect each shipped image's imports separately.
[Cargo's target flags](https://doc.rust-lang.org/cargo/reference/config.html#buildrustflags)
and an explicit application target keep linkage options out of host build scripts
and procedural macros.

Rust's
[MSVC SEH implementation](https://github.com/rust-lang/rust/blob/main/library/panic_unwind/src/seh.rs)
calls `_CxxThrowException` and uses the CRT's `__CxxFrameHandler3` personality
for stack cleanup. Both workspaces retain panic unwinding. Runner's
`ModelPreparation` cancels and joins preparation peers after a worker panic;
Prism's CPU worker pool catches panics, drains all active workers and returns a
failure. Scoped cancellation and tile-completion guards also rely on unwinding
to run their destructors. A blanket `panic = "abort"` change would alter these
recovery contracts.
[Cargo's panic-profile rules](https://doc.rust-lang.org/cargo/reference/profiles.html#panic)
also explain why ordinary passing tests do not establish abort-mode release behavior.

TTS's `libm` math implementation and the existing direct Windows API boundaries
replace individual operations. Neither workspace wires a qualified pure Rust
ARM64 MSVC CRT/SEH provider, and the local candidate inventory supplied no ready
replacement. This records the existing C++ retirement boundary; the 0.3 release
and performance-qualification gates retain their current scope.
