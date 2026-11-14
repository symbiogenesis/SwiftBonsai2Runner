# Windows integration and worker sandboxing

The native UI requires containment for owned inference, ASR, TTS and file-decoder workers.
`bonsai-windows-process` combines pinned **MXC 1.0.1** Process Security Environment
(PSEC), suspended creation,
verified Job ownership before resume, exact worker PID, resource limits and
restricted handle inheritance. Missing capabilities or rejected policies stop
launch without an uncontained retry. Actual-model and broker-only isolation remain
unqualified; published Prism/speech engines need separate review. This guide owns
handle, storage, thread and audio contracts under the [UI architecture](ui-architecture.md).

## Windows consent and capabilities

After explicit action, the trusted UI requests `AppCapability` microphone access;
model workers have no microphone, playback or presentation role. PSEC separately
enforces filesystem, network and desktop grants in the kernel, without per-file
prompts. Windows Firewall is independent: distinguish its denials from PSEC
policy rejection during diagnosis.

The UI stops capture on observed revocation. Deadlines/revocation are cooperative,
not an OS-enforced recording quota while the UI/processes are suspended. Model
capability reports grant no Windows permissions. See [speech operation](development-guide.md#speech-setup-and-operation).

### Input capabilities

Explicit microphone, picker and clipboard actions belong to the trusted native
UI. Windows microphone consent remains authoritative and is checked at capture
transitions and during recording; no cached sandbox result grants device access.
Picker and pasted-file paths authorize only the selected input for that import.
They do not restrict the filesystem authority of the trusted UI itself.

Compressed audio is decoded by one offline MXC helper for the complete import.
The parent retains the selected read-only file and ancestor guards through helper
teardown. The helper receives exact file/runtime grants, private scratch and
restricted standard handles, without microphone, picker or host-loopback grants.
Bounded binary pipe frames carry normalized audio; ASR receives WAV bytes and
Swift receives transcript text. Failure to admit or initialize the helper stops
import without an in-process decoding fallback. Media Foundation compatibility
with the headless policy requires native qualification.

### Boundary work and overhead

Every new and existing authority boundary must keep work proportional to the
operation it protects. Apply process, Job and complete sandbox admission once
per worker creation, then retain their owners; a throwaway empty environment is
reserved for an explicit availability diagnostic. Retain immutable file identities
instead of reopening attacker-replaceable paths. Preserve required authenticity,
authentication, framing, resource, cancellation and revocation checks.

Streaming boundaries use bounded buffers and backpressure, borrowed payloads
where their lifetimes allow, and incremental scans. Do not create a process,
reverify a complete artifact, serialize audio as JSON/base64 or stage a complete
recording for each frame or segment. One sequential decoder serves an import;
the existing ASR lease can amortize model startup independently. Device permission
decisions must not be cached across explicit actions to improve timing.

Negligible overhead is a measured acceptance gate, covering existing boundaries
as well as additions. Source-level allocation/work controls do not establish it.
Follow the [boundary overhead acceptance](validation.md#boundary-overhead-acceptance)
contract, including separate cold startup and steady-state observations. Failed
performance acceptance requires optimization while keeping enforcement intact.

## Technology selection

[MXC ProcessContainer](https://github.com/microsoft/mxc/tree/v1.0.1) contains the
pinned Windows ARM64 workers. Separate-kernel VM isolation is an independent
future boundary requiring model sharing, transport, resource and teardown
qualification; see [PLAN.md](../PLAN.md#optional-agents).

## Worker authority

Treat model, tokenizer, adapter and audio parsing as compromise entry points,
even for verified pins. Provisioning/compiler tools are trusted maintenance with
their existing authority, outside inference policy. Independently managed speech
companions retain their own permissions.

The owned-worker policy grants:

- Read-only access to exact selected executable/runtime files, model, tokenizer
  or codec, adapter and only that worker's backend credential. Public policy
  inputs accept files, not broad data/profile directories.
- Internal read-only Windows runtime directories, never a drive-root grant
  (native negative evidence shows it exposes unrelated credentials). The trusted
  owner validates and retains full ancestry. Inside an actual AppContainer, read
  validation skips only the denied volume-root metadata check; local-drive and
  every non-root check remain in force.
- Writable application scratch below a fresh protected owner/LocalSystem
  directory. Keys and parent-opened stdout/stderr remain outside that writable
  grant; only their specific standard handles are inherited.
- Windows' own private PSEC temporary storage. Windows can replace the requested
  child TEMP/TMP with `Packages/sandbox.{id}/AC/Temp`; supplied scratch is not the
  sole writable location, and effective TEMP must not be claimed to equal cwd.
- Explicit bidirectional host-loopback access required by the current native
  HTTP servers, with external ingress and egress denied by policy and no
  Internet/private-LAN capability.

Host-loopback permission reaches services on the host's non-loopback addresses,
even with all-address egress-deny rules.
It **does not enforce broker-only communication** or deny all other local services.
The [versioned networking contract](https://github.com/microsoft/mxc/blob/v1.0.1/docs/backends/process-container/networking.md)
explains the coarse host-loopback grant. Retain numeric-loopback binding, bearer
authentication, exact Host/routes and listener PID checks. A stricter boundary
requires a separately proven brokered transport or accepted peer identity;
changing a JSON toggle is insufficient.

OS/profile environment entries are passed through; the launcher does not clear
the environment or relocate Windows/profile folders into a checkout. Child
TEMP/TMP select scratch before Windows applies PSEC's own temporary-storage policy.
Known credential-shaped variables, including common tokens, API keys, passwords
and connection strings, reject sandbox launch with a value-free error. This is a
conservative guard, not proof that every arbitrarily named inherited value is
non-secret. Run from an appropriately scoped parent environment.

Existing artifact verification and retained handles remain in force. Jobs still
enforce process count and committed memory; the physical-RAM watchdog remains
necessary for mapped weights. Scratch and log storage do not acquire a disk quota
merely by being private. Same-user processes outside the sandbox can still read
owner-protected credentials or interfere with endpoints. Administrator/trusted
owner compromise is outside this boundary. The development app enforces this
policy, while daily-service isolation remains unqualified under the
[broker boundary](broker-policy.md) and [PLAN acceptance](../PLAN.md).

## Native integration contract

Explicit availability probes require PSEC 1.1 and container-hosted loopback, then
create/dispose a real minimal environment; versions or exports alone are
insufficient. Ordinary launches admit the complete policy once, without that
throwaway preflight. HTTP workers require host-loopback support; offline decoders
require PSEC 1.1 without requesting or probing loopback authority. There are no audit/learning modes,
SDK backend dispatch, DACL augmentation, global loopback exemptions or weaker
fallbacks. Host Firewall restrictions remain independent.

The supported [Rust V1 spawn API](https://github.com/microsoft/mxc/blob/v1.0.1/docs/api-reference/rust/v1/api.md)
cannot accept Runner's existing Job and suspended-launch contract. The audited
native boundary therefore uses the exact release's doc-hidden PSEC compatibility
exports and generated FlatBuffer types to add the security-environment attribute
alongside `JOB_LIST`, mitigations and the standard-handle list. Windows' PSEC
broker can omit the requested Job assignment, so the launcher verifies membership,
assigns the still-suspended root when needed, verifies again and requires exactly
one suspension before resume. The one-process Job limit is tested by an actual
denied child creation. A redundant child-process creation attribute prevented
ordinary native workers from initializing on the tested host; child denial
therefore uses the verified Job limit, including for fixed-command inspection.
The [native terminate-on-close flag](https://learn.microsoft.com/en-us/windows/win32/api/processmodel/ne-processmodel-process_security_environment_flags)
also bounds the environment's lifetime. No executor wrapper is mistaken for the
model PID.

Microsoft marks those compatibility exports as unsupported SDK interfaces in the
[SDK boundary](https://github.com/microsoft/mxc/blob/v1.0.1/src/mxc-sdk/src/lib.rs).
This is explicit compatibility debt: review the encoder, ABI, dependencies and
actual enforcement before changing either exact pin. Current-main documentation
must not substitute for the shipped release's behavior. Prefer a supported
composition hook when one becomes available.

Ordinary headless `ProcessSpec::start` requests creation-time Win32k denial,
legacy extension-point and remote/low-integrity image denial, and heap termination.
The PSEC broker does not preserve every ordinary creation attribute; contained
launches require verified Win32k denial, Job ownership and native lifetime control,
and must not claim the other ordinary mitigations as proven. The ordinary policy uses
[`PROC_THREAD_ATTRIBUTE_MITIGATION_POLICY`](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-updateprocthreadattribute),
not Job UI limits that prevent [nested Jobs](https://learn.microsoft.com/en-us/windows/win32/procthread/nested-jobs).
Future GPU drivers or GUI helpers require separate compatibility review.

## Launch and qualification

The UI uses `--require-native-sandbox` for its broker and requires contained speech.
Direct development may explicitly choose the mutually exclusive
`--allow-unisolated-backend`, retaining caller authority. Failed containment never
selects that opt-out automatically.

Focused local checks and main-only Foundation suites follow the
[validation policy](validation.md). Record the exact SDK, policy, OS/architecture,
runtime and artifact identities. Distinguish portable parser/policy checks from
actual native enforcement and actual-model acceptance. Model-free native fixtures
cover file and hard-link denial, retained artifact/key guards, separate private
temp storage, child-process denial, exact listener identity, external TCP denial
and worker teardown after abrupt owner death. They do not qualify real
model/adapter/speech loading, cancellation under load, broker-only transport,
performance or sustained comfort; [PLAN.md](../PLAN.md) owns
that acceptance. Native admission errors and pending qualification are not passes.

The public API still rejects tools, arbitrary executables, dynamic runtime
selection and unsupported media. Model output remains inert text. Future plugins
or generated-code execution need separate per-task capabilities and, where the
threat model requires it, a separately qualified VM boundary. They must not
inherit the UI's or inference worker's authority.

## Resource ownership and process lifetime

Use Rust [owned/borrowed handle types](https://doc.rust-lang.org/std/os/windows/io/),
`File`, socket and process owners; transfer fresh Win32 handles into owners
immediately and borrow raw handles only for a call. Pseudo handles stay borrowed.
Match allocator-specific destructors: credentials use `LocalFree`, WinRT uses its
bindings, and audio retains waveOut headers/buffers until driver release.

`src/windows-process` creates hidden suspended processes with explicit executable,
working directory and quoted arguments. Creation attributes assign an unnamed
Job and inherit only the three standard streams. [Jobs](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects) kill
descendants on owner loss, reject breakaway and enforce
[process/commit bounds](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_basic_limit_information); they do not restrict
security identity. Parent monitoring retains query/synchronize handles and checks
creation time against PID reuse. Native PSEC composition and its distinct
effective mitigations are specified above.

Build tools preserve the Unicode Windows/profile environment with reviewed child
overlays. Never clear it, relocate system/profile folders or run native diagnostics
from a repository working directory; use [external validation lanes](validation.md).

## Files, locks and publication

Credentials use system CSPRNG bytes, protected ACLs, noninheritable retained
handles and scrubbed buffers; recovery uses user-scoped DPAPI.

| Purpose | Mechanism |
| --- | --- |
| One startup/build/download writer | Persistent sentinel, exclusive sharing and `File::try_lock`; handle release covers exit/crash. |
| One recovery writer | Persistent sentinel plus `File::try_lock`, read/write sharing without delete sharing, preventing replacement while locked. |
| Pinned artifacts/credentials | Retained file read handles deny write/delete sharing; artifact directory ancestors share reads/writes but deny deletion, preserving paths while permitting sibling atomic publication. |
| New evidence/private keys | Exclusive creation; protected owner/LocalSystem DACL precedes secret writes. |
| Artifact publication | Bounded staging, exact identity/hash verification and writer-guarded publication. |
| Recovery publication | Bounded user-scoped DPAPI, flushed temporary file and same-directory replacement under the recovery guard. |

Rust's [`File::try_lock`](https://doc.rust-lang.org/std/fs/struct.File.html#method.try_lock)
uses Windows byte-range locking (`LockFileEx`). Keep sentinels after releasing
their handles: unlink/recreate can give processes different locked file objects.
No record-oriented shared database needs independently locked ranges.
[Byte-range locks](https://learn.microsoft.com/en-us/windows/win32/fileio/locking-and-unlocking-byte-ranges-in-files)
do not constrain mapped I/O or replace deny-write/delete sharing for models.
The [file-sharing contract](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew)
governs those retained handles. Job [commit accounting](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_extended_limit_information)
does not cover mapped residency or device memory; reported peaks can include
denied allocations. Physical-memory monitoring remains necessary.
Flush and atomic namespace replacement have distinct purposes; neither promises
survival of every device/power failure.

Validate expanded absolute paths, device names (including superscript COM/LPT
digits), alternate streams, control characters and reparse points. Audited storage
boundaries use `FILE_FLAG_OPEN_REPARSE_POINT` and metadata from opened handles,
not only earlier pathname checks. [Credential storage](../src/windows-credentials/README.md)
requires owner-controlled ancestors; leaf checks cannot freeze all ancestry or
protect against hostile same-user processes.

## Threads, responsiveness and power

Named `std::thread::Builder` workers are OS threads. Network, disk/DPAPI, speech
orchestration and playback stay off the event loop, with bounded queues,
cancellation generations and joined shutdown. The broker uses async I/O,
bounded admission and absolute deadlines; a separate speech watchdog can kill
an owned companion when native inference cannot cancel cooperatively. Microphone
operations stay on their WinRT apartment thread with balanced initialization
and bounded asynchronous operations.

Retain normal Windows scheduling. Do not give inference realtime priority,
guess affinity from SKU or raise system timer resolution for polling.
[MMCSS](https://learn.microsoft.com/en-us/windows/win32/procthread/multimedia-class-scheduler-service)
requires a demonstrated audio deadline problem on that audio thread and balanced
registration; it does not justify boosting inference.
[EcoQoS](https://learn.microsoft.com/en-us/windows/win32/procthread/quality-of-service)
can suit explicit background work, but response generation/playback needs device
latency/power evidence. Current configurable thread budgets reserve CPU capacity;
they establish neither optimal topology nor performance.

WinMM waveOut playback retains stable headers/buffers, bounded buffering,
stop/reset and unprepare-before-free ordering. Microsoft
[recommends newer audio APIs](https://learn.microsoft.com/en-us/windows/win32/multimedia/waveform-functions);
WASAPI/AudioGraph migration still needs device-change, suspend/resume,
cancellation and latency qualification.
