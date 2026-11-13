# C++ retirement contract

The milestone removes C++ from Runner and Prism, including controls, generated
source, bundled/downloaded libraries and compiler/build dependencies.
[PLAN.md](../PLAN.md) owns remaining work. Preserve the [native UI](ui-architecture.md),
[local API](broker-policy.md), source identities, licenses/notices and independent
replay inputs. Rust bindings and process isolation do not retire a C++ dependency.
Vendor OS/driver ABIs require an explicit boundary decision.

## Replacement and deletion rules

Before retiring each file or inseparable family:

1. Bind behavior to immutable source/patch, license, target/toolchain, inputs,
   configuration, assertions and failure/lifecycle cases.
2. Preserve independent controls as immutable input/output fixtures, mathematical
   known answers or separately implemented Rust references. Identify captured
   observations as captures, without claiming fresh execution of the old engine.
3. Pass focused replacement checks and retain every approved assertion, tolerance,
   numerical order, exceptional bit, ISA boundary, green ID and exclusion. Leave
   owning hosted consumer acceptance pending when only local checks ran.
4. Remove operative includes/generators, compiler/link/download/cache paths,
   dependency/feature edges, workflow dispatch and obsolete documentation.
   Retain notices and required evidence as data.
5. Qualify actual-model/device behavior, quality, resources, isolation and
   distribution independently under [validation](validation.md). Synthetic replay
   and source deletion do not satisfy those gates.

The [conformance boundary](prism-upstream-tests.md) and
[native runtime](native-rust-engine.md) define acceptance. Provider replacement
must preserve the complete upstream catalog/schedule and all required green IDs;
missing captures/assertions remain pending or rejected.

## Runner checked-in native files

`tools/hexagon/public_qaic_wire.c` and `windows_queue_abi.c` are hosted C controls
for pinned public declarations and generated wire/ABI observations. They link no
inference library, SDK runtime or driver. Retire compilation only after independent
retained observations and Rust assertions satisfy their owning contracts.

Speech's licensed Philox, sampling, tokenizer and lifetime observations live in
`src/speech-tts/tests/fixtures/`. Keep their provenance, replay inputs and independent
assertions. [Speech documentation](../src/speech-tts/README.md) owns serving behavior.

## Prism checked-in native files

Use Prism's [migration inventory](https://github.com/symbiogenesis/prism-inference/blob/main/tools/prism/upstream/migration.json)
for the file-level provider list. The native boundary under
`tools/prism/upstream/common/` includes `bridge.cpp`, its 39 C++ headers and coupled
`public_c_abi.h`/`public_c_abi.c`. Retire each route only after its numerical,
text/API, model/session or concurrency assertions have independent replacements;
remove the final dispatcher after every provider has migrated.

## External engines, generators and build dependencies

| Boundary | Retirement requirement |
| --- | --- |
| TTS | Serving executes GGUF intake, Talker/CodePredictor, codec and speaker graphs in Rust on CPU. Preserve immutable controls and qualify Q8/Q4 model parity, listening, state/lifecycle and resources; native-linked results do not qualify Rust serving. |
| ASR | `transcribe-cpp` / `transcribe-cpp-sys` bundles Qwen3-ASR/GGML. A replacement must preserve verification, audio/token contracts, cancellation/state, output quality and device resources. |
| Publisher runtime and PTQ1 collector | The collector uses pinned native inference DLLs. A Rust driver does not replace that engine. Retire publisher locks, builders, capture routes and patches only after replacement/fallback and distribution gates pass. |
| Hexagon | Pure Rust model/codec execution and signed `hexagon-native-rpc` replace GGML host engines. Review vendor FastRPC separately: DriverStore `libcdsprpc.dll` (`qcnspmcdm`), `remote_handle64_*`, `rpcmem_*`, `dspqueue_*` and signed `libggml-htp-v73.so` / `libggml-htp-v81.so` DSP skels. |

Expand external families to exact pinned source files before porting. Prism's
remaining boundary includes CMake projects/adapters, native loader/LoRA-export
linkage, LLGuidance and codec downloads, packaging, caches and provider dispatch.
Preserve assertions when removing their build paths. TTS's Rust build and scalar,
tokenizer and graph-lifetime fixtures have no live source-overlay generator or
native Qwen/GGML registration dependency.

ASR's [registry-package inventory](../src/speech-asr/native-sources.json) binds
all 1,718 files in `transcribe-cpp-sys` 0.4.1 to byte lengths and SHA-256 identities,
including GGML, platform sources, generators, build inputs and notices. It retains
file identities rather than copied source. The Rust
[`native-source-verify` command](../tools/ci/README.md#native-source-inventory)
checks the complete extracted file set, original archive checksum, Cargo lock and
upstream revision without compiling or executing the package. Only Cargo's local
`.cargo-ok` extraction marker is excluded. This package inventory includes unused
backend families; it does not establish the selected preprocessed/compiled source
closure, inference parity or permission to delete native files.

The Rust [`native-closure-verify` command](../tools/ci/README.md#selected-compiler-source-closure)
separately checks a reviewed selection of compiled translation units against
original Clang dependency reports, preprocessed/object identities and generated
input provenance. Owning builds must still establish complete selection,
configuration and linked-library closure; passing a selected capture does not
qualify native retirement.

TTS has no signed RPC dispatcher. Its [signed-dispatch prerequisites](../src/speech-tts/README.md#optional-hexagon-runtime)
require independent wire/Windows ABI controls before hybrid placement. Fixed
F32/guarded-Q2 controls do not qualify Q8 prefill, shared KV or complete speech state.

Final acceptance requires source/generated-source and dependency inventories,
binary-linkage assessment, clean hosted consumer builds without a project C++
toolchain, preserved qualification contracts and no operative retired controls.
Keep historical fixture metadata and legally required notices as data.

### MSVC startup and panic runtime

[Static CRT linkage](https://doc.rust-lang.org/reference/linkage.html#static-and-dynamic-c-runtimes)
removes separately installed Visual C++ DLL requirements by embedding Microsoft
runtime support; it does not replace those bytes with Rust. Inspect every shipped
image's imports. Apply [target flags](https://doc.rust-lang.org/cargo/reference/config.html#buildrustflags)
to the explicit application target, excluding host build scripts/procedural macros.

Rust's [MSVC SEH implementation](https://github.com/rust-lang/rust/blob/main/library/panic_unwind/src/seh.rs)
uses `_CxxThrowException` and `__CxxFrameHandler3`. Both workspaces retain unwinding:
Runner cancels/joins preparation peers after a panic, Prism drains active CPU
workers, and scoped cancellation/completion guards depend on destructors. Changing
`panic = "abort"` would alter recovery; ordinary passing tests do not qualify
[abort-mode releases](https://doc.rust-lang.org/cargo/reference/profiles.html#panic).
Neither workspace has a qualified pure Rust ARM64 MSVC CRT/SEH provider.
