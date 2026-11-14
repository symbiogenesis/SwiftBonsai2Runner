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
Philox, sampler, graph-lifetime and tokenizer replay use safe Rust directly;
all obsolete C ABI adapters and their unsafe allowance are removed. The tokenizer
shares serving's bounded transactional metadata loader, preserves all 1,810
immutable native requests and checks candidate cleanup, failed replacement and
owned result reuse. Its historical pointer/holder assertions have explicit Rust
ownership dispositions in the retained provenance. Historical graph-cache
fixtures exercise a test-only cache and do not qualify current serving graphs.

## Prism checked-in native files

Use Prism's [migration inventory](https://github.com/symbiogenesis/prism-inference/blob/main/tools/prism/upstream/migration.json)
for the file-level provider list. The native boundary under
`tools/prism/upstream/common/` includes `bridge.cpp`, its 39 C++ headers and coupled
`public_c_abi.h`/`public_c_abi.c`. Retire each route only after its numerical,
text/API, model/session or concurrency assertions have independent replacements;
remove the final dispatcher after every provider has migrated.

The [checked-in boundary verifier](../tools/ci/README.md#checked-in-native-retirement-boundary)
now binds Runner's two C files and the clean read-only Prism snapshot's 40 C++
files plus coupled C ABI source/header. Exact per-file families and declared
compiler/provider/contracts identities preserve the green-ID baseline and both
original ordered geometry schedules. Four focused controls, strict Release
analysis/docs and real snapshot verification passed. The guard rejects added,
deleted or changed files/declared edges and every retirement-qualified status;
inventory acceptance cannot authorize deletion. It verifies original bytes,
not Git revision provenance, external archives or compiled/link closure.

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
input provenance. Schema-2 captures recursively classify and verify generated
intermediates, including each original successful invocation and working directory;
schema-1 replay retains its direct-input scope. Schema 3 additionally binds the
original complete argument-array compilation database, rejecting omitted, extra,
duplicate or changed invocations and preserving distinct configurations of one
source file. Five focused controls and strict Release analysis passed. Owning
builds must still establish complete selection, effective configuration and
linked-library closure; a selected capture does not qualify native retirement.

The Rust [`native-archive-inventory` command](../tools/ci/README.md#immutable-external-source-archives)
also expands pinned external source ZIPs into complete sorted member identities,
including build inputs and notices, without extraction or source execution. It
binds original archive/pin hashes and rejects unsafe or aliased names, unsupported
members, expansion violations and file/directory collisions. Its four focused
controls, native strict Release analysis and tool link passed. Actual source
intake is not qualified: the pinned publisher URL returned HTTP 404, stopping
the fixed two-archive workload before Prism intake without retry. The
[retained no-go](evidence/native-source-archives/summary.json) records exact
identities, missing pre-setup power, intake/cleanup power, allowance and cleanup.
Submodule contents and selected compiler/link closure remain separate inventories.

Rust ASR exposes borrowed mel, encoder rows, prompt IDs, each full logits/greedy
decision and all-layer committed KV through EOS, before its state resets.
Observer refusal, cancellation and unwind clear every layer without publishing
a partial transcript. Six focused controls, strict Rust-only Release analysis,
docs and the qualification executable link passed. This supplies the Rust half
of a comparison: callers still bind exact source/model/PCM and independent
expected observations. It neither captures the native engine nor qualifies S2.

The [ASR replay comparator](../src/speech-asr/README.md#streaming-independent-replay)
now binds reviewed reference/candidate source/compiler/target/executable,
arithmetic, verified model, actual PCM digest and explicit tolerances. It reads
original expected values with bounded scratch and compares every stage, ordered
generation decision and all-layer KV, including exceptional bits when requested.
Engine success, the exact terminal transcript and EOF are mandatory; cancellation
during terminal/EOF intake cannot grant success. Seven focused controls and strict
Rust-only Release analysis/docs passed. Arbitrary readers still need caller-bound
I/O and the original absolute deadline. No independent native observations were
manufactured or captured, and production ASR remains on its existing native engine.

[Rust TTS observations](../src/speech-tts/README.md#independent-graph-observations)
likewise lend complete prompts/overlays, raw semantic and acoustic logits before
sampling, all committed Talker/predictor KV, seeded decisions through EOS, codes,
speaker/reference conditioning and validated Design/Base PCM. Callback refusal
and unwinding invalidate reuse after input admission; ordinary service installs
a no-op observer. Six focused controls, strict Release analysis/docs and service
linking passed. These observations execute Rust only. Independent pinned-model
codec/speaker controls, complete parity, listening and resource gates remain open.

The [native Q8 projection adapter](../tools/hexagon/native_rpc/README.md#q8-matrix-preparation-and-immutable-placement)
connects immutable host dispatch to signed-queue ABI calls, exact FD mapping,
correlated completions and mandatory independent output review. Failed or
uncertain work, including caught callback unwinds, remains terminal and retains
owners through supervised exit. Twelve focused model-free controls and native
warning-denied analysis/docs passed. Actual identity admission remains the
supervisor's obligation; simulated ABI calls authenticate no source/model/package/
catalog/driver/VTCM/loaded-DSP tuple. Contained HTP0 execution and TTS integration
remain pending. TTS's [signed-dispatch prerequisites](../src/speech-tts/README.md#optional-hexagon-runtime)
still require independent wire/Windows ABI controls; neither these simulations
nor fixed F32/guarded-Q2 controls qualify Q8 arithmetic, shared KV or speech state.

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
