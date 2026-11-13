# Runtime source candidates

The headless-startup patch, manifest and immutable receipt remain build/replay inputs.
They do not change Runner's release lock or qualify a production runtime.

## Headless UCRT argv candidate

The [patch](headless-ucrt-argv.patch) fixes reproduced CPU source-build completion
startup on Snapdragon X2 Elite Extreme: same-source control exits `0xC0000142`,
candidate `--version` exits zero under unchanged headless policy. The
[manifest](headless-ucrt-argv.json) and [model-free
evidence](headless-ucrt-argv.evidence.json) bind
source/compiler/binary/dependency/supervision identities. This upstream review artifact
leaves Runner's lock and defaults unchanged.

The source is the MIT-licensed Prism revision
[`73bfe6cc485554822f80c473a3c0c4f25e80f41b`](https://github.com/PrismML-Eng/llama.cpp/tree/73bfe6cc485554822f80c473a3c0c4f25e80f41b).
Its `common/arg.cpp` uses `CommandLineToArgvW` for Unicode argument repair. Read-only PE
inspection confirms that this imports `SHELL32.dll` through `llama-common.dll`. The
candidate replaces that MSVC/clang-cl path with UCRT wide argv initialization. The
rebuilt DLL imports `_configure_wide_argv`, `__p___wargv` and `__p___argc` from
`api-ms-win-crt-runtime-l1-1-0.dll`, and no longer imports `SHELL32.dll`. The paired
result fixes this source-build failure, without diagnosing every possible failure of the
independently built released binary.

UCRT configuration is guarded by a thread-safe function-local static, reusing a
preexisting wide startup table. Reconfiguring UCRT would overwrite its pointer without
freeing its old allocation; repeated parsing instead borrows one table. Wildcard
expansion is disabled when this helper initializes it. Existing UTF-8 conversion and the
argument-count repair guard remain in place. Configuration failure keeps the existing
empty-repair fallback. MinGW retains its original shell parsing and `LocalFree` path.

Compatibility covers Runner's `ProcessSpec` encoding, not arbitrary raw command lines,
externally configured wildcard-expanding CRTs, other CRT/toolchains or MinGW.
Integration must review UCRT entry points against its compiler/SDK.

### Validation scope

The manifest and immutable model-free receipt identify the original compiler,
source patch, binary and native mitigation checks. They are build inputs for the
candidate producer and the fixed startup fixture. They do not qualify model loading,
numerical equivalence, actual-model state, adapters or performance on current source.

The argument fixture exercises Unicode, quoting, backslashes, whitespace and literal
wildcards without loading a model. Every launch must require effective Win32k,
extension-point and unsafe-image denial, a one-process 512 MiB Job, ten-second
process deadline, 64 KiB output bounds and artifact handles held through teardown.
Version/argument success supplies no inference or power verdict.

### Reproduce

Verify the exact source archive, patch length and SHA-256 against the manifest. From
native ARM64 developer PowerShell with the recorded MSVC/SDK environment, use the
recorded LLVM compiler and separate absolute source/build directories. Build a control
before applying the patch if reproducing the paired result.

```powershell
git -c core.autocrlf=false -c core.eol=lf -C $source apply --check $patch
git -c core.autocrlf=false -c core.eol=lf -C $source apply $patch
cmake -S $source -B $build -G Ninja -DCMAKE_BUILD_TYPE=Release `
  "-DCMAKE_C_COMPILER=$llvm/bin/clang-cl.exe" `
  "-DCMAKE_CXX_COMPILER=$llvm/bin/clang-cl.exe" `
  "-DCMAKE_LINKER=$llvm/bin/lld-link.exe" `
  -DBUILD_SHARED_LIBS=ON -DGGML_CPU=ON -DGGML_NATIVE=OFF `
  -DGGML_BACKEND_DL=OFF -DGGML_CPU_ALL_VARIANTS=OFF -DGGML_OPENMP=OFF `
  -DGGML_CPU_KLEIDIAI=OFF -DGGML_LLAMAFILE=OFF `
  -DGGML_CUDA=OFF -DGGML_VULKAN=OFF -DGGML_OPENCL=OFF -DGGML_HEXAGON=OFF `
  -DLLAMA_BUILD_TESTS=ON -DGGML_BUILD_TESTS=OFF `
  -DLLAMA_BUILD_EXAMPLES=OFF -DLLAMA_BUILD_SERVER=OFF -DLLAMA_BUILD_APP=OFF `
  -DLLAMA_BUILD_UI=OFF -DLLAMA_OPENSSL=OFF -DLLAMA_SUBPROCESS=OFF `
  -DLLAMA_LLGUIDANCE=OFF -DLLAMA_BUILD_COMMIT=73bfe6c -DLLAMA_BUILD_NUMBER=10787
cmake --build $build --target llama-completion test-headless-argv --parallel 4
```

The fixture deliberately requires worker creation mitigations and is an explicit build
target, outside ordinary CTest. Use Runner's existing checked maintenance tool and
`bonsai-tools completion --request ABSOLUTE_JSON_PATH` from an external directory, with
`ciOnly=false`,
no runtime override, all generated EXE/DLL files listed as exact `artifacts`, the limits
above and no model artifacts. Probe `llama-completion.exe` with only `--version`. For
`test-headless-argv.exe`, pass `@('--model','模型/test.gguf','--seed','1','--prompt','日本語
🪴 café')` for the Unicode case; the retained evidence lists all seven exact argument
arrays. Each successful fixture prints its observed mitigation values. Other invocations
must retain absolute parent lifetime and owned teardown through the same tool. Do not
treat an inspection-policy launch as this check.

The owning runtime must review/integrate/build a CPU release and qualify model loading,
Unicode paths, continuation/state, adapters and cancellation under production
containment before a Runner lock update and settled-main X2 CI preflight/thread screen.
The retained checks are model-free and establish no speed, memory, energy or promotion
verdict. Benchmarks and full suites remain CI-only.

### Hosted headless comparator preparation

The native
[build-tools headless publisher operation](../../tools/bonsai/src/build_tools/headless.rs)
applies this exact patch to the verified source archive on hosted CI. It builds separate
`llama-completion`, `llama-server`, `llama-bench` and argument-fixture executables.
The existing installed publisher bundle, Runner runtime locks, app selection,
worker mitigations and filesystem grants are unchanged. Preparation downloads
no model artifacts and launches no owner-device job.

The compiler archive and its ARM64 executables reuse the retained LLVM 23.1.4
identities. The build records its hosted image, installed MSVC/SDK, compiler,
supervisor, CMake flags/cache and all packaged file digests. It uses four compile
jobs and restores the publisher toolchain's global `armv8.7-a` and floating-point
flags. Shared CPU libraries remain enabled; dynamic backend loading, OpenMP,
optional accelerators, UI, TLS fetching and subprocess support are disabled.
This differs from the released Clang 20 bundle and the earlier baseline-NEON
argument pilot; numerical and performance qualification remain pending.

A bounded static PE audit checks every candidate EXE/DLL, including normal and
delay imports, then follows mandatory system imports through the hosted image's
API-set mapping. GUI modules reject the candidate. Optional Windows delay imports
are retained in the report rather than described as absent: core Windows DLLs
themselves expose optional GUI APIs. Export forwarders and runtime `LoadLibrary`
calls are outside this static audit. The separate effective-mitigation probes
remain required; a named-import graph alone is not startup qualification.

On the hosted runner, completion/server receive only `--version` and the benchmark
tool receives only `--help` (its pinned CLI has no version switch); the seven
argument fixtures call parsing without loading their nonexistent model paths.
The unchanged supervisor retains all candidate artifact handles through a
one-process 512 MiB Job, ten-second deadline and 64 KiB output bounds. The fixture
requires effective Win32k, extension-point and unsafe-image denial. Actions
credentials are removed from the preparation child before these probes.

The resulting artifact is a source-bound candidate for review, not an installed
runtime or a model speed result. A passed hosted preparation still needs a
separately authorized owner-device trial within the existing five-minute total
budget and the independent numerical, adapter, resource and quality gates before
any release or lock update.

### Bounded X2 model-free startup

The Rust [`publisher-startup` command](../../tools/bonsai/README.md)
checks an already extracted copy of that exact hosted candidate using a prebuilt
maintenance tool. Its compiled frozen receipt pins the archived hosted package identities, rejects
extra/missing or redirected bundle entries, and supplies every pinned file to native
verification with handles retained through worker teardown. It runs the same ten fixed
version/help/argument probes through the ordinary required headless supervisor, without
model artifacts or runtime overrides. The owner X2 identity, supervisor digest, source,
hosted artifact, exact probes and bounded supervision receipts are retained separately.

```powershell
$request = 'C:/external/startup-request.json'
@{ candidateDirectory='C:/external/archived-candidate/candidate'
   outputDirectory='C:/external/fresh-startup-evidence'; workBudgetSeconds=60 } |
    ConvertTo-Json | Set-Content -LiteralPath $request -Encoding utf8NoBOM
& 'C:/external/prebuilt/bonsai-tools.exe' publisher-startup --request $request
```

The aggregate allowance includes setup, file verification, probes and owned teardown,
reserving ten seconds for reporting. Each worker has a ten-second deadline, a
one-process 512 MiB Job and 64 KiB per-stream output bounds. Budget exhaustion or a
failed probe stops the remaining probes and retains their pending status. No downloads,
local builds, production bundle replacement, model inference or benchmark runs occur.
Charge this capture together with all other owner-device work against the same
five-minute total. A pass establishes only these exact model-free startup invocations;
model loading, Unicode model paths, numerics, adapters, cancellation, release integration
and the settled-main thread screen remain separate gates.

### Explicit X2 candidate model smoke

The same command accepts `modelSmoke: true` with `dataDirectory`
to replace the ten model-free probes with one original-PQ2 completion. This
requires the exact reviewed hosted bundle and a prebuilt supervisor; it downloads
and builds nothing. It retains the compiled Swift CPU control, 8K context,
11.5 GiB Job ceiling, 15.5 GiB physical admission and 4 GiB available-memory floor.
Only the output cap (four tokens), a request to disable thinking and diagnostic counter
collection differ from that control. No adapter or alternative model is accepted.

```powershell
$request = 'C:/external/model-smoke-request.json'
@{ candidateDirectory='C:/external/archived-candidate/candidate'
   outputDirectory='C:/external/fresh-model-smoke'; workBudgetSeconds=90
   modelSmoke=$true; dataDirectory='C:/external/provisioned-data' } |
    ConvertTo-Json | Set-Content -LiteralPath $request -Encoding utf8NoBOM
& 'C:/external/prebuilt/bonsai-tools.exe' publisher-startup --request $request
```

The allowance includes verification of every candidate file and the 7.2 GB model,
loading, inference and owned teardown. Admission requires at least sixty seconds
of native allowance, preserving a worker deadline of twenty-four to thirty seconds,
twenty-four seconds for verification and eleven for teardown, plus the native-call
margin. A shorter remaining budget leaves the
gate pending. Charge this allowance together with every other owner-device trial
against the five-minute total.

A pass requires successful owned exit, hidden prompt echo, nonempty output and
unique positive prompt, sampler and decode-evaluation counters from the pinned
logger. The receipt distinguishes actual inference observations from executable
creation and records the cap separately from sampled tokens. The single response
does not qualify exact 8K occupancy, complete state, numerical parity, adapters,
cancellation, quality, speed or release/pin promotion. Diagnostic timing lines are
retained only as execution evidence; no performance comparison is evaluated.
Requested reasoning controls do not establish the embedded template's effective
thinking mode; that remains separately unqualified.
