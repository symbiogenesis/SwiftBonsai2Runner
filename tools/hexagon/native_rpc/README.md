# Native Rust FastRPC diagnostics

This std-only Rust boundary queries the installed Qualcomm Windows driver without
a ggml, GenieX, QNN or C/C++ inference library. It is separate from the
[Prism serving path](../../../docs/hexagon.md). Metadata, mapping, fixed compute
and full-model qualification have independent gates; reports keep
`profileEligible: false`.

## Driver ownership and trust

`lib.rs` exposes a safe API over audited `windows.rs`. It resolves query-only
`qcnspmcdm` SCM metadata, derives `libcdsprpc.dll` inside DriverStore using
`GetWindowsDirectoryW`, and rejects traversal/reparse ancestors. A read-only file
guard denies writes/deletion through cached no-UI Authenticode verification,
restricted loading, calls and unload. Dependencies search only the driver
directory and System32; raw exports never escape. Sessions are neither Send nor
Sync. A process-wide nonblocking mutex excludes concurrent owners; contention or
poisoning rejects. Terminal owners retain the lease through process exit.

Cached Authenticode does not supply a publisher pin, fresh revocation check or
kernel acceptance. The trusted supervisor independently binds path, SHA-256,
publisher, source and driver identities; library verification runs again on the
same handle before load. Native calls have no in-process cancellation guarantee:
use a source-bound prebuilt child, absolute aggregate deadline, parent-death
ownership and verified teardown under the
[device budget](../../../AGENTS.md#owner-device-execution-budgets).

The CLI rejects inherited `ADSP_`, `CDSP_`, `FASTRPC_`, `RPCMEM_`, `GGML_`,
`GENIEX_`, `QNN_`, `QAIRT_` and `HEXAGON_` names case-insensitively. Only explicit
session diagnostics permit their exact child-only `ADSP_LIBRARY_PATH`. No values
are logged or parent environment changed.

## CLI controls

`query.rs` accepts exactly one mode and emits one bounded JSON record. Package
arguments must be absolute. Capability/STATIC modes enable no unsigned process,
custom DSP module, kernel or power change.

| Mode | Fixed work |
| --- | --- |
| `--self-test` | Check 12-byte C layout and architecture decoding without native access. |
| `--inspect` | Return service-derived `driverDll` for independent signature/hash review without loading it. |
| `--query` | Verify/load the fixed driver; issue domain-3 `DSPRPC_GET_DSP_INFO` for eight read-only attributes. Keep per-call results; missing values stay null. |
| `--session-prepare` | Query architecture, effective default-CDSP domain and bundled module URI. No module open, mapping, unsigned request or synthesized fallback URI. |
| `--mapping-probe` | One 4096-byte heap-25 cached allocation (flags 1), descriptor/CPU check, domain-3 STATIC registration (flag 0, offset 0), matching unmap/free. |
| `--session-mapping-probe PACKAGE` | One signed bundled v73/v81 module in default-CDSP session zero, 4096-byte FD=2 map/unmap/free and u64 handle close. Use checked driver URI; no unsigned-process request. |
| `--reference-session-mapping-probe PACKAGE` | Same bounded session mapping with the explicit reference process initialization below. |
| `--reference-queue-compute-probe PACKAGE` | GenieX v0.10.0 X2/v81, reference initialization, one queue/16 KiB map, F32 K32/M32/N2. All 64 poisoned outputs must match independent exact signed results. |
| `--reference-queue-q2-probe PACKAGE` | Same ownership, Q2_0 K256/M32/N2, four 576-byte host tiles and guarded F32 activation limbs. Check all 64 expected dots, including fourth symbols/extremes; request 61,440-byte VTCM scratch. Supervisor case `GuardedQ2`. |
| `--reference-queue-noblock-probe PACKAGE`, `--reference-queue-q2-noblock-probe PACKAGE` | Select only the six-argument `dspqueue_write_noblock` export once for identical F32/Q2 work. Supervisor cases `F32NoBlock`/`GuardedQ2NoBlock`. No retry. A bounded drain after write refusal cannot permit normal cleanup or erase the original failure. |

The [compute supervisor](../../bonsai/src/hexagon_tools/device.rs) requires trusted
hosted ABI/build intake and rechecks package, catalog and driver identities.
Session modes retain nonreparse module/catalog/INF guards and require exactly one
child search directory. Package/URI selection does not independently prove the
DSP bytes loaded. Responses must match correlation, extents, counts, buffer/cache
identity and poisoned numeric output before explicit stop/queue-close/module-close/
unmap/free. Uncertain consumers pin allocations and owners until supervised exit.

STATIC registration may implicitly open a domain session. Every mapping attempt
retains its DLL/file owner until exit, including after successful scratch cleanup;
uncertain use forbids free/unload and further native admission. STATIC refusal
does not rule out FD=2, delayed-FD=3, DMA64 or compute. The pinned header's
[`AEE_ERPC`](https://github.com/qualcomm/fastrpc/blob/a56e9d4de3614a8d8f4b0e15ff561350c4b686e3/inc/AEEStdErr.h#L92)
is generic error 512, not a permission/signature/hardware diagnosis. Keep exact
codes and modes. Allocation alone proves neither registration nor DSP visibility.

## Bounded original-projection bridge

`Driver::reference_q2_projection` uses one guarded allocation, FD mapping, module
and queue for synchronous K256/N2 operations, one HVX thread and 61,440-byte VTCM
scratch. M is a multiple of 32 up to 17,408. A first full Swift projection needs
forty original-block operations. Quant-row scratch is independent of M at N2;
the host mapping is at most 1,396,736 bytes. Regenerate tensor offsets and NaN
poison after completion because DSP mutates descriptors; retain no borrowed host
view across execution.

`Q2ProjectionLimits` bounds rows, writes and one absolute session clock including
initialization/preparation: **24,000 attempted writes, 120 seconds, three-second
cleanup reserve**. Reset cannot replenish the cap. One all-eligible PQ2 evaluation
uses 23,256 operations; a hidden token plus evaluation needs 45,912 and must reject.
Outer process and aggregate owner deadlines also apply.

Each operation requires a fresh monotonic sequence, exact descriptor/cache reply,
finite/integer/bounded output and the caller's independent signed-dot oracle.
Only a complete projection and `Q2ProjectionReport::completed_cycle()` permit
publication. Refusal, stale reply, invalid output, expiry, callback error or unwind
pins uncertain consumers until exit, without queue retry, CPU replay or release.
After full acceptance, stop/close/unmap/free explicitly. Full-M solver, selected
DSP bytes, vendor teardown, model state and performance still require acceptance.

## Q8 matrix preparation and immutable placement

`Q8Matmul` constructs data-only Q8_0/F32/F32 packets from the exact newer
signed-package source in `sources.json`. Its explicit subset admits K multiples
of 128 through 8,192, M multiples of 32 through 17,408 and N=1/2/4. It preserves
original Q8_0 scale/code bytes in 1,088-byte tiles and selects one-HVX row-kernel
prefetch 16/8/4/2 against an explicit VTCM budget. The complete mapping is capped
at 64 MiB; raw activation conversion, quantized activations, weight prefetch and
single-row output scratch are charged separately. It rejects unsupported tails,
nonfinite scales/activations and invalid mapping identities before mutation.

`HybridPlan` freezes exact source/model/module/catalog/driver observations and
ordered, nonoverlapping tensor placements. Complete mutable KV/session state
stays CPU-owned; HTP0 cache ownership rejects until its own complete-state provider
exists. Construction authenticates no file and grants no native authority.

The existing guarded-Q2 native submitter consumes the same single-flight tracker:
attempted submissions receive monotonic sequences before the write; a complete
correlated buffer/cache/numeric result is required before reusing mapped bytes.
Refusal, cancellation, stale replies or changed mapping identities poison the
tracker permanently without replenishing its original operation budget.

The Q8 packet/solver controls are mathematical/layout checks, not native captures.
No Q8 native submission API, TTS integration or serving admission follows from
packet construction. Independent signed-package/device execution, exact loaded
DSP identity, contained HTP0 supervision, Q8 activation quantization/arithmetic,
correlated completions and teardown remain required. Q8 preparation does not
qualify TTS prefill, shared KV, quality, listening or performance.

## Established-session mapping prerequisite

`session.rs`/`session_native.rs` use `FASTRPC_GET_EFFECTIVE_DOMAIN_ID=14` and
`FASTRPC_GET_URI=15` for `cdsp`, session zero, with exact 24-/48-byte ARM64
structures. Decode architecture from the driver; reject invalid packed-decimal
nibbles and zero while retaining raw values. The checked URI must match v73/v81,
`htp_iface_skel_handle_invoke`, module version 1.0, domain and session. Verify
inputs, pointers, extents and terminated output after every call; never load or
print vendor-controlled strings.

Metadata stops before `remote_handle64_open` and supplies no path override.
A basename URI does not bind ELF bytes/catalog trust; an absolute URI is not an
established Windows deployment route. Once opened, the matching effective domain
owns every mapping tuple. Public delayed-FD=3 remains a constant: registration
until DSP `HAP_mmap` does not prove visibility or cache maintenance. The extended
SDK delayed flag is absent from the public header; no guessed value is exposed.
HVX/HMX and VTCM page/count observations prove neither throughput nor available
capacity. See [source/ABI boundaries](../../../docs/hexagon-source-audit.md).

## Terminal signed-session FD control

`session_mapping.rs` keeps metadata/open/handle/map/close/lifetime fields separate.
`session_mapping_native.rs` uses pinned public ABI, including **u64**
`remote_handle64_close`. STATIC and session probes share terminal admission.
After any remote-open attempt, driver/package guards and lease stay pinned until
exit, even after explicit close. Failed map/unmap retains scratch and module;
missing/refused handles cannot become mapped sessions. Process death does not
establish complete vendor teardown.

Flushed `hexagon-session:stage=...` stderr markers establish entry only; returned
codes establish completion. Mock controls check URI/map arguments, success,
refusal at each stage and uncertain ownership without loading a driver.

## Explicit reference process initialization

Reference modes request an **unsigned DSP process class** under exact signed
Windows package/catalog policy. They authorize no arbitrary DSP code or OS trust
change. There is no automatic retry between ordinary and reference modes.

The [public header](https://github.com/qualcomm/fastrpc/blob/a56e9d4de3614a8d8f4b0e15ff561350c4b686e3/inc/remote.h#L603-L622)
defines `DSPRPC_CONTROL_UNSIGNED_MODULE=2`: two ints, `domain` and `enable`.
Request enable1 for the independently checked default-CDSP domain after effective
domain discovery and before GET_URI/open, following the
[reference initializer](https://github.com/ggml-org/llama.cpp/blob/94256114c229674ef96e76eb2dea596e65b43818/ggml/src/ggml-hexagon/ggml-hexagon.cpp#L3875-L3912).
The [Linux implementation](https://github.com/qualcomm/fastrpc/blob/a56e9d4de3614a8d8f4b0e15ff561350c4b686e3/src/fastrpc_apps_user.c#L2434-L2454)
sets a per-domain pre-session attribute; it does not prove closed Windows behavior.
[GenieX package-signing requirements](https://github.com/qualcomm/GenieX/blob/a5c8e7fb4a364ddcccefbf187afd1bac52d88de5/notes/run.md#L303-L342)
remain independent.

Pin owners before initialization. Require its returned code before proceeding.
The receipt records mode, request2/size8/enable1 and raw returned
`processInitializationDomainAfter`/`processInitializationEnableAfter` (null when
unattempted). Returned writable bytes never configure URI/mapping or establish
effective process class; subsequent work uses the original validated metadata.
Refusal/incomplete URI ends the attempt without weaker retry. Entry markers and
`unsignedProcessRequested=true` record invocation, not effective class.

## Hosted build

Exact source hashes and Microsoft ABI references are in [`sources.json`](sources.json).
The binding vendors no vendor/ggml implementation. Build and full regressions
belong to hosted CI; owner diagnostics consume prebuilt source-bound helpers.

```powershell
rustup run nightly-2026-11-12-aarch64-pc-windows-msvc rustc --edition=2024 --crate-type=lib --crate-name=hexagon_native_rpc -D warnings -O tools/hexagon/native_rpc/lib.rs --out-dir ABSOLUTE_EXTERNAL_BUILD
rustup run nightly-2026-11-12-aarch64-pc-windows-msvc rustc --edition=2024 -D warnings -O tools/hexagon/native_rpc/query.rs --extern hexagon_native_rpc=ABSOLUTE_EXTERNAL_BUILD/libhexagon_native_rpc.rlib -o ABSOLUTE_EXTERNAL_BUILD/hexagon-native-query.exe
```
