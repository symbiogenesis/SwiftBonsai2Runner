# Development guide

To use Swift, [download the Windows ARM64 executable](releases.md#download-and-open)
and open it. Model, adapter, inference-engine and speech setup happen in the app,
with progress and retry controls. No checkout or PowerShell setup is needed.

Start with [Native chat app](#native-chat-app) for application controls.
The build, source-launch, [inventory/provisioning](#inventory-and-provisioning),
[CPU checks](#bounded-cpu-loaddecode-check) and the [development broker](#explicit-development-broker)
sections are separate contributor workflows. Run their command examples from the
repository root in native Windows ARM64 PowerShell 7.6+. Use
[validation](validation.md) for language standards and focused checks.

## Current state

The app defaults to native Rust [Prism Inference](https://github.com/symbiogenesis/prism-inference)
0.1.26, original Swift PQ2, four CPU row workers and the pinned pretrained adapter
at scale 1.3. On supported Snapdragon X2 installations, the runtime also enables
the experimental resident Hexagon `native-q2` path, with CPU fallback when the
signed runtime package or CDSP driver is absent. See [Hexagon policy](hexagon.md)
for placement, resource charges and qualification limits.
Publisher C++ rollback, Qwen `reference-cpu` and current-weight PTQ1
use explicit immutable selections. No training is required. [Runtime selection](#runtime-selection)
defines supported combinations; [memory](swift-memory.md) and
[LoRA qualification](swift-lora.md#standard-profile-qualification) distinguish
development evidence from pending quality, performance and service acceptance.

This is a development default with full-context/output, creative-quality,
sustained-performance, daily-isolation and clean-machine qualification pending.
It makes no speed claim. Missing or failed artifacts never select another engine
or representation automatically.

[Automatic profiles](hardware-capabilities.md#automatic-profile-selection),
[Adreno](adreno.md) and [Hexagon](hexagon.md) remain unqualified.
Inventory and nonlaunching policy plans load no model and cannot promote defaults; the
[X2 thread screen](cpu-kernels.md#x2-cpu-thread-screen) runs in device CI.
Owned UI inference/speech requires [native containment](sandboxing.md), whose
actual-model and broker-only/same-user isolation limits still apply.

## Implementation delivery

Use [PLAN's execution map](../PLAN.md#execution-map) for task state, prerequisites
and acceptance requirements. Before assigning work, identify its next action:
implementation, integration, qualification, owner/external dependency or
conditional work. Prefer existing candidates and qualification helpers.

### Candidate and worker ownership

Before qualification, integrate reviewed changes into local `main` and freeze
an exact candidate: Runner/Prism revisions, compiler/channel/target, executable
digests, runtime archive/file locks, model/representation, adapter/scale and
selected device/driver/kernel. A version number alone is insufficient. Continue
implementation in separate worktrees; review changed dependencies before reusing evidence.

The active 0.1.26 source retains efficiencies awaiting qualification.
The [historical 0.1.25 registry delivery](evidence/prism-0125-publish-20261112/summary.json)
retains its original helper identities. The 0.1.26 registry package instead selects
`bonsai-neon-dot 0.1.3`; the source-bound binary release retains its own exact
source and archive identities. Newer local CPU candidates need separate
source review, and binary repins require genuine source-bound published bundles.
Follow the [runtime lock update](#prepare-a-runtime-lock-update) and
[CPU verification contract](cpu-kernels.md).

Use one integrator and at most three workers with disjoint files and explicit
producer/consumer interfaces across application, runtime, speech or qualification
work. Each worker uses its own Git worktree and one commit per logical change.
Give shared pins, receipt schemas, workflow contracts and final serving integration
one owner. Agree interfaces before dependent work; review consumer effects before
integrating producers. Reassign finished workers to unblocked tasks, including
independent fixture replay or human-review preparation while source/provenance is pending.

The integrator reviews and integrates commits into each repository's local `main`, preserving
the owner's work and qualification candidate. Workers report commands, results,
limits and remaining gates. They never push, open routine delivery PRs or operate
CI. Local completion does not wait for an owner verdict; hosted/device acceptance
stays pending until supported by its own evidence.

### Speech integration interfaces

Use these interfaces for replacement work. The Rust CPU TTS service now consumes
the weight/Talker/codec path; independent model and lifecycle acceptance remain open.

| Work owner | Producer/consumer contract | Required ownership and acceptance |
| --- | --- | --- |
| Verified loading and Talker/CodePredictor | Immutable tensor views bind model/codec locks, shapes, dtype, transform metadata and approved adapter identity to the runtime consumer. | Retain file guards and model lifetimes; define cache/state, sampler progression and transactional cancellation/rollback. Qualify full serving integration and actual-model parity. |
| Codec and speaker streaming | Explicit sample rate, channel/frame geometry, bounded PCM chunks and reference/bootstrap inputs connect speaker extraction, codec state and synthesis. | One owner controls mutable stream/cache state and teardown. Preserve Design-before-Base release, bounds and independent audio/lifecycle/resource review. |
| Native Rust RPC | Typed operation/tensor/stride descriptors connect the verified model path to supported signed FastRPC execution. | Preserve the documented `libcdsprpc.dll` driver boundary, in-flight buffer lifetime, terminal completion and verified teardown. Header serialization alone is not dispatch or a completion fence. |
| Final serving integration | The existing speech service and native UI consume one qualified pipeline through their current bounded contracts. | One integrator owns service wiring, cancellation and cache invalidation; interface completion cannot waive full pinned-model, listening or resource gates. |

Prepare and qualify the ordinary Q4 baseline first while retaining the Q8
control. Ternary Talker artifacts, new 48 kHz model locks and signed offload keep
their separate prerequisites and qualification after baseline integration.
Qwen3-ASR remains a separate complete Rust-engine replacement, including its
artifact, transcript/state, cancellation, lifecycle, quality and resource gates.
Remove C++/native dependencies and compiler provisioning only under the
[file-family retirement contract](cpp-retirement.md). All presentation remains
native Rust under the [UI contract](ui-architecture.md).

### Qualification reuse and owner handoff

Reuse deadline, power, identity, typed-receipt and archive tooling. Extend shared
fixtures for complete committed state, logits, KV/GDN/convolution ownership,
adapter identity, sampler progression and recovery. Map assertions to each
prefix/prefill/speculation/accelerator path; shared tooling or immutable data
cannot transfer qualification between paths. Preserve independent-owner controls,
per-arm power and required replay inputs under [validation](validation.md) and
[native acceptance admission](../tools/ci/README.md#native-acceptance-admission).

Deliver one concrete owner handoff for a candidate, using the ordinary delivery
message unless a durable acceptance record is required. Include:

1. Reviewed local commits and exact Runner/Prism/compiler/artifact identities,
   with the candidate's PLAN IDs and any source still awaiting publication.
2. Exact owner commands, source refs, workflow names/inputs and their order;
   distinguish source publication, hosted preparation, device work and release
   publication. Use [release commands](releases.md) and the owning workflow
   guides, with concrete reviewed values instead of an unreviewed latest ref.
3. Expected asset/package names, receipt schemas and required digest checks,
   plus rollback identities. Acceptance consumes its own same-run package;
   caching compiler outputs never reuses another run's verdict or receipt.
4. The fixed device workload, conservative estimate and smallest supported
   allowance declared before setup; aggregate/per-arm power observations,
   independent owners and cleanup/evidence reserves retain their existing limits.
5. Local commands/results/limits, every pending hosted/device/human gate and
   the explicitly selected credential-free evidence with its durable destination
   and independently retained retrieval digest when archival is required.

Follow AGENTS.md for owner-only pushes and CI operations; preparation does not
authorize a device job. Preserve losing/no-go outcomes and closed clocks. Use
selected-receipt staging and `evidence-archive` under
[evidence retention](validation.md#evidence-retention).

## Build a single executable

On Windows ARM64 with PowerShell 7.6+, the pinned Rust compiler and the native
speech prerequisites, run this from the checkout:

```powershell
./scripts/Build-WindowsExecutable.ps1
```

The script builds or reuses all five native application executables, collects
the complete locked-dependency notices and writes one
`%LOCALAPPDATA%\SwiftBonsai2\build\distribution\SwiftBonsai2Runner.exe`.
Use `-OutputPath C:\Bonsai\SwiftBonsai2Runner.exe` for another absolute output.
Existing outputs are never overwritten; choose a new path or explicitly remove
an earlier local build first. `-RepositoryRoot` and `-DataDirectory` select an
alternate checkout or local data/cache root. Existing source-launch caches remain
the default. `-TargetDirectory` (or `CARGO_TARGET_DIR`) selects an absolute Cargo
cache root for local builds or CI cache restoration: maintenance and chat share
native-target artifacts there; speech keeps its required repository/compiler/host/ISA
partition beneath that root. Cargo checks content freshness, and native receipts
still verify source, host and executable identities. Notice collection reuses
verified speech sources without repeating compiler or Visual Studio preparation.
Cold native compiler/source setup
and compilation can take several minutes; this build performs no model trial,
application launch or CI operation.

The executable embeds the native window, broker, ASR, TTS, maintenance tool,
package manifest and notices. Double-click it to verify and extract the package into
the user cache and open chat without a terminal, checkout, PowerShell or compiler.
Workers retain their containment and teardown rules. The app downloads and verifies
the pinned model weights, adapter, inference runtime and speech models on first
launch; users do not provision them before opening the executable.
For a manual update, verify the new executable's published SHA-256, close Swift
and replace your copy. Keep the same data directory to preserve models and data.

To bundle an already reviewed schema-2 package without compiling or collecting
notices again:

```powershell
./scripts/Build-WindowsExecutable.ps1 -PackageDirectory C:\BonsaiPackages\0.2.0 -ManifestSha256 $reviewedDigest -OutputPath C:\Bonsai\SwiftBonsai2Runner.exe
```

The same package-to-executable helper runs in Windows Release. Local builds
record the current Git revision in their package; a dirty checkout's local
changes remain part of that build and do not become source-provenance evidence.
Review the final source and executable digest before redistribution. The
[Windows release guide](releases.md) describes published checksums, prerequisites
and the remaining hosted and clean-machine acceptance gates.

## Native chat app

Open the [portable executable](releases.md#download-and-open) on serviced Windows
11 ARM64. The native window opens first; its setup worker then downloads and
verifies the pinned Swift model, adapter, selected inference engine and speech
models. Text chat starts as soon as its files are ready while speech preparation
continues separately. Microphone capture and playback require explicit action.

Keep the window open to follow progress. **Pause setup** preserves partial
downloads, and **Retry setup** resumes after the reported problem is resolved.
Speech has its own **Retry speech setup** control in **Settings > Voice**;
retrying speech preserves the running text model, drafts and conversations.
Closing and reopening also resumes preparation. Use **Copy error** or
**Copy diagnostics** to report the failed step and selected data directory.
Keep the same data directory across updates to retain downloads and local data.

### Contributor source launch

The following commands build and launch from a checkout; they are not deployment
steps. Source launch requires native Windows ARM64,
the pinned Rust toolchain, a built maintenance executable, internet access for setup and
sufficient available RAM for the selected native context and launch budget.
Serviced Windows 11 must admit PSEC 1.1 and
container-hosted loopback; failed containment never falls back to caller authority.

Build the maintenance executable outside Git once, then launch. The native setup
worker prepares Swift and its pinned adapter. Rebuild after maintenance-source changes:

```powershell
$repo = $PWD.Path
$target = Join-Path $env:LOCALAPPDATA 'SwiftBonsai2/build/maintenance'
cargo --config 'target.aarch64-pc-windows-msvc.rustflags=["-C","target-feature=+crt-static"]' build --locked --release --target aarch64-pc-windows-msvc --manifest-path (Join-Path $repo 'Cargo.toml') --package bonsai-tools --bin bonsai-tools --target-dir $target
$env:BONSAI_TOOLS_PATH = Join-Path $target 'aarch64-pc-windows-msvc/release/bonsai-tools.exe'
$env:PATH = (Split-Path $env:BONSAI_TOOLS_PATH) + [IO.Path]::PathSeparator + $env:PATH
$data = Join-Path $env:LOCALAPPDATA 'SwiftBonsai2'
bonsai-tools chat --data-directory $data
```

The explicit target keeps application static-CRT flags out of host build scripts
and procedural macros, including when the host is also Windows ARM64.

The optional convenience launcher uses the same prebuilt executable:

```powershell
./scripts/Start-Chat.ps1
./scripts/Start-Chat.ps1 -PublisherEngine
```

It accepts `-DataDirectory` and `-ToolPath`, otherwise selecting
`BONSAI_TOOLS_PATH`, the maintenance build above, or `bonsai-tools.exe` on PATH.
Omitting engine options selects Rust. `-NativeEngine` remains an explicit Rust
selection; `-PublisherEngine` selects publisher rollback, prepared from its pinned
release. See [runtime selection](#runtime-selection). The script
only forwards to Rust and propagates its exit code; it builds or provisions nothing.

When native Rust inference is selected, the Rust preparation worker verifies the
pinned runtime after the first native frame and repairs it when needed. Source-built
brokers use the checkout's exact release lock; explicit prebuilt brokers use the
maintenance executable's compiled release lock. Replacement is staged and verified;
unrelated files, redirected paths and access failures stop repair. Swift weights
and the selected adapter are prepared from compiled pins before broker handoff.
No external model lockfile is required. Explicit PTQ1 is verified without fetching
or substituting PQ2.

Text chat starts loading once its runtime and broker are ready. Speech downloads
and builds continue in the owned background worker; controls activate after
preparation, and failures appear in speech status without stopping text chat.
At most two workers download/check speech artifacts; failure cancels and joins
the peer. ASR/TTS models load on demand for transcription or read-aloud. Closing
cancels preparation and waits for owned processes to stop.
Staged startup requires rebuilt maintenance tools and UI; older tools retain combined setup.

Direct developer launches of `bonsai-chat.exe` also verify the selected Rust
executable before hashing the model or creating credentials. Use the matching
launcher to prepare and repair its compiled runtime pin. Older or mismatched
tools must be rebuilt from the same checkout or supplied by the same verified app
package. Normal portable launches perform preparation in the window; model and
setup retries repeat verification without requiring a provisioning command.

During staged source startup, pinned runtime verification/provisioning and the
broker build run concurrently. Both must finish successfully before the UI starts
the broker; failure cancels and joins the peer. Full-file artifact verification
uses at most 1 MiB of read scratch with sequential access hints, retaining full
checksums and file guards. Speech preparation and its compiler descendants use
below-normal CPU priority to reduce competition with model loading and text chat,
including on Snapdragon X1. This CPU scheduling policy does not isolate disk I/O.

If the launcher reports a missing prebuilt executable, run the maintenance build
command it prints from the repository root, then retry. Correct or remove any
invalid `-ToolPath` or `BONSAI_TOOLS_PATH` override first. Older source launches used
`build/tools/<fingerprint>/`; those caches are not selected automatically. Rebuild
from the current checkout into `build/maintenance` to use the current compiler pin
and maintenance commands.

Rebuild the maintenance executable to receive the current direct CLI Rust default
and `--publisher-engine` option. The PowerShell facade forwards only options you
supply; omitted engine/worker options use the selected executable's defaults.
Older executables retain their own defaults. `-NativeEngine -NativeRowWorkers 4`
selects Rust/four workers explicitly; `-NativeRowWorkers 1` selects one Rust worker
with current tools. Publisher launches require one.

The native Rust `egui`/`eframe` app follows the [UI contract](ui-architecture.md).
The window opens before broker/speech preparation finishes, so drafting remains
available during setup. Progress and setup errors stay visible; Pause and Retry
setup preserve drafts and verified downloads. Retry requires confirmed process
cleanup; unknown cleanup requires closing the app. Speech setup has a separate
retry and never restarts a running text model. Closing stops preparation, compiler
descendants and owned model
workers. Artifact verification and memory admission still precede model use.

**Stop** tears down the owned broker/backend. **Retry model** waits for teardown,
then repeats identity, artifact and selected-memory admission checks with a fresh instance
and retained completed turns. Retry explicitly after uncertain transport/protocol
failures or after freeing RAM. Requests never replay automatically or fall back
to a smaller profile. Interrupted prompts return only to an empty composer,
preserving newer drafts; a closed connection alone identifies no token/memory cause.

### Conversations and response settings

Conversations have independent drafts and settings with no count cap, including on
branching and recovery. Storage still has an encrypted 8 MiB workspace budget
and per-conversation message limits.
Drag the sidebar's inner edge to resize it; its button beside Settings or
**Ctrl+Shift+B** toggles visibility. Width and visibility are saved. Narrow windows
use a left drawer that closes on selection, backdrop click or Escape without
stopping a response. A compact left rail keeps the toggle accessible when hidden.
Switch conversations while ready or generating. Use **New conversation** or a row's
**actions** to rename, copy or delete. Deletion names its target; **Keep conversation**
or Escape cancels. Deleting the last chat creates an empty one. Status and
collapsible diagnostics remain outside the transcript.

**Search conversations** filters titles and visible messages by Unicode
case-insensitive substring, excluding reasoning and drafts. It preserves the
selected conversation and draft even when unmatched. **Clear** or Escape in the
focused field restores the list before drawer dismissal or Stop. New conversations
and branches clear search; the query is not saved. Large lists render visible rows
and cache matches until the query or searchable content changes.
Sidebar edits immediately update Find and highlights in the open conversation without
moving focus; clearing removes highlights even with no matching conversations.
Selecting or reselecting a result opens Find with the trimmed query and first body
match selected. Title-only results show **No matches**. Editing or closing Find
leaves the sidebar filter intact.

While a response generates, you can draft the next message, browse another chat
or create a conversation. Only one response runs at a time; Enter never queues a
second request. **Stop** controls that response from any visible conversation;
**Show response** returns to its original chat. Incoming text and completed history
stay in that chat, and a newer draft survives completion, rejection or Stop.
Its Delete action stays disabled until generation settles. Switching stops speech;
background text never restarts playback. Navigation briefly locks during model
teardown/restart, while drafts remain editable.

The composer scrolls long drafts without hiding Microphone or Send/Stop.
Drafts can contain up to 1,048,576 Unicode characters; oversized edits reject
without inserting a partial paste. Complete requests still need to fit the
1 MiB serialized-byte bound, exact context count and reply reservation. Long
drafts remain editable after rejection. The separate 8 MiB recovery budget can
reject saving a large workspace; saving failures stay visible.
**Enter** sends when ready, **Shift+Enter** inserts a newline and **Ctrl+End**
moves to the draft's end. **Ctrl+F** searches visible message bodies
with Unicode case-insensitive matching, excluding reasoning. The search field and
previous/next controls appear above the transcript only while Find is open.
Settings is in the sidebar footer. The conversation has no permanent top bar or
Find button. **F3/Shift+F3**
(or **Enter/Shift+Enter** while Find is focused) moves between matches; **Latest**
or the bottom-center down arrow returns to the newest message. **Close find** or
**Escape** closes Find and clears highlights;
a second Escape stops active generation or speech.
**Ctrl+F** focuses Find from the composer or sidebar and closes an open conversation
drawer. It also works in empty conversations. Settings, dialogs and open popups
retain keyboard priority.

Each message's **actions** menu provides iteration tools for verified committed
turns. **Edit and resend** opens an editor; **Create branch and send** submits
the revised prompt in a new conversation using the preceding completed history.
**Regenerate** creates a branch and sends the original prompt again, excluding
its previous answer and later turns. Both retain the original conversation,
unsent draft and response settings. Cancel/Escape leaves the source unchanged.
**Branch from here** creates a conversation without sending: an assistant includes
its completed turn; a user message becomes a draft after the preceding turns.
Branches have no conversation-count cap and retain the usual recovery byte budget.

**Continue response** is available on the latest answer in conversation context when Swift is
ready. It sends an explicit followup asking Swift to continue without repeating,
and preserves the unsent composer draft. It uses the same context admission and
response settings as Send. An identified interrupted reply is included in history;
other intermediate Extra effort drafts are excluded. Older recovered messages without saved
positions remain copyable and searchable, with iteration disabled. Newly completed
turns gain verified positions through the normal committed-history event.

Assistant text uses native heading, list/quote and inline/fenced code styles.
Code is monospace; original Markdown markers remain visible. Copy, selection and
Find retain exact source text and offsets. HTML and link-like text remain literal;
this bounded styling subset does not execute code or activate generated links.

**Settings** opens a centered native modal with side navigation in wide windows
and wrapping categories in narrow or short windows. A compact page title and
close button sit above the controls; the scope stays visible while scrolling.
Its **Response** page names the selected conversation
under **This conversation**. The other pages state **All conversations · This
device**:

| Page | Scope and controls |
| --- | --- |
| **Response** | Selected conversation: Thinking, Extra effort and **Advanced response controls** for reply limits and adapter strength. |
| **Voice** | All conversations: the shared Voice profile and read-aloud setup. |
| **Appearance** | Device-wide System/Light/Dark theme, reading text size and maximum reading width, with a live sample and reset. Windows high contrast takes precedence. |
| **Privacy** | All conversations: automatic encrypted saving and its persistent opt-out. |
| **Application** | Device-wide runtime information, application updates and inline diagnostics. |

Changes apply immediately; **Done**, **Close** or Escape dismisses the modal
without reverting them. Closing restores composer focus and preserves its draft.
Escape closes an open selector before dismissing Settings. While Settings is
open, background conversation actions, Find, Send and generation Stop shortcuts
are blocked; active input retains its explicit cancellation behavior. A response
already in progress continues normally. Voice setup and diagnostics expand
inside Settings.

The composer's **Thinking** popup offers **Thinking** and **Extra effort**, also
available in **Settings > Response**. Both apply to the selected conversation and
lock during generation. Click outside or press Escape to dismiss; Enter cannot
send the background draft. Reply limits and adapter controls stay in advanced Settings.

| Setting | Behavior |
| --- | --- |
| **Thinking** | Off initially and in older saved chats; enables reasoning for the next response and every Extra effort pass. |
| **Mode** | Standard makes one call. Experimental Extra effort makes at most three sequential plan/draft/review calls, shows stage labels and retains complete drafts. Neither mode has an elapsed generation-time limit; Stop cancels the active response. A truncated revision does not replace a complete draft; quality gains remain unproven. |
| **Output cap** | Available only in **Settings > Response > Advanced response controls**, under **Reply length**. Defaults to 4,096; also offers 512/1,024/2,048 reasoning-inclusive tokens within the model session's selected context and 64-token template reserve. |
| **Abliteration scale** | The advanced adapter disclosure in **Settings > Response** explains this experimental correction. The pinned adapter defaults to 1.3; choose 0-2 in 0.1 steps for the next response/every Extra effort pass without restart. Zero disables the correction. Older saved chats use 1.3; effectiveness and quality remain experimental. |

The composer's **Context:** button pairs its label with a circular occupancy indicator
for the selected conversation's next Send. The ring includes the next reply reservation,
fills up to its full circumference and uses a warning color near capacity. Hover or
focus exposes an approximate percentage; an unfinished response instead shows a hollow
ring with a dash until the estimate updates. Open it for a segmented breakdown of committed
history including retained reasoning, the trimmed draft and approximate template framing,
Extra effort planning overhead when enabled, the selected reply reservation and
the fixed 64-token reserve. The input counts are heuristic and may
overestimate or underestimate the tokenizer. The selected reply cap, reserve and
session context limit are exact policy values; the estimate is no guarantee of fit.
The capacity comes from the owned broker and stays pending until startup reports
it. [Memory-selected context](swift-memory.md#memory-selected-serving-context)
defines launch-time sizing, reserves and pending larger-context qualification.
Extra effort estimates cover its planning pass only: later passes include generated
working material that is not yet known. Message iteration can use a different
prompt or committed prefix, so the composer estimate does not preflight those actions.

At approximately 85% occupancy or estimated overflow, a plain-language warning offers
**Start fresh**. The details explain when a smaller reply limit may help and point
to advanced Settings; they never change the limit. Starting fresh preserves the
previous conversation and its draft. No estimate disables Send, trims history or
changes a cap automatically.
Reply controls retain the normal generation lock. When the visible conversation
owns an active response, the space details explain that they update after the response
finishes; another selected conversation keeps its own next-draft estimate.
Escape closes either composer popup before Find or Stop, and Enter in those popups does
not submit the composer.

Response settings belong to each conversation and are retained while saving is
enabled; response controls lock during generation. The broker still counts the
exact prompt for every call and never discards history to make it fit. A framed
context rejection restores the prompt and keeps the model ready; lower the output
cap or start a conversation. Reaching an admitted response's cap is a separate
partial-answer outcome, after which you can explicitly ask Swift to continue.
Other recognized input/burst rejections follow the [broker error contract](broker-policy.md#native-client-completion-and-recovery).

### Optional recovery and preview

Chat automatically saves drafts,
response settings, voice description, transcript and exact completed history in
`DataDirectory/chat-recovery.dpapi`. User-scoped DPAPI encrypts the bounded 8 MiB
workspace; a file lock permits one writer per data folder.
An interrupted reply with a locally identified prompt and visible answer is marked
**Interrupted** and retained as conversation context. Later prompts can reference it;
its message menu supports Continue response and Branch from here. Stop, failure and
recovery never automatically resend the original request. If no usable answer exists,
the prompt remains available for review. Older snapshots can recover an unbound
trailing pair when their saved incomplete state and last answer identify it;
ambiguous partial replies remain visible outside history.
Restore activates neither microphone nor playback.
In **Settings > Privacy**, disable **Save conversations on this device** to delete
the saved copy and keep the workspace open without saving. This choice survives restarts;
the locked `chat-recovery.lock` sentinel retains only the saving preference.
Enable the setting again to resume saving. Corrupt or
unreadable recovery is preserved with a warning until explicit saving replaces it.
Save failures identify the failed step and available Windows error code without
including conversation text or paths. Unsaved changes stay in the open window;
the next automatic save retries the latest workspace.
Keep the data folder private and outside Git: DPAPI does not isolate other
same-user processes, and deletion is neither secure erasure nor backup removal.

The native window separately remembers its normal size, position and maximized
state in `DataDirectory/chat-window.json`. This small file contains geometry only
and remains active when conversation saving is off. Closing while minimized keeps
the previous visible placement; Windows moves a wholly offscreen restored window
back onto the current monitor configuration. Invalid settings use the ordinary
default window. Preview neither reads nor writes these settings.

**Settings > Appearance** adjusts prose, code, thinking and composer text from
14–24 points and the reading column from 640–1,040 points, constrained by the
available window width. Interface controls keep their ordinary size. The default
window is 1,080×780 points and can shrink to 480×480 for split-screen use.
Appearance, sidebar width and docked visibility persist separately in the bounded
`DataDirectory/chat-appearance.json`; this file contains no conversation content
and remains active when conversation saving is off. Writes run on a coalescing
worker and flush on close. Preview neither reads nor writes these preferences.

Window placement, appearance, sidebar geometry and the device-wide voice profile
form the local user profile. A manual sidebar resize remains at the chosen width
after releasing the handle, hiding/reopening it and switching window layouts;
wide content cannot expand it again. The voice profile saves separately with
Windows user encryption in `DataDirectory/chat-profile.dpapi`, including when
conversation saving is off. Existing saved conversations supply the initial voice
preference when no separate profile exists. Response options remain specific to
each conversation and recover with that conversation. Preview leaves all profile
files untouched.

The app supports Windows high contrast, display scaling, labelled AccessKit
controls and selectable/copyable text. Full bidirectional editing and native-control
accessibility parity remain unqualified. Check layout, streaming selection,
resizing and keyboard navigation with a marked model-free preview:

```powershell
$previewTarget = Join-Path ([IO.Path]::GetTempPath()) ('Bonsai-Preview-' + [guid]::NewGuid().ToString('N'))
cargo build --locked --release --package bonsai-chat --target-dir $previewTarget
Start-Process -FilePath (Join-Path $previewTarget 'release/bonsai-chat.exe') -ArgumentList '--preview' -WorkingDirectory ([IO.Path]::GetTempPath())
```

Preview uses the same UI and sample content, without broker, credentials,
microphone or speech services. It establishes neither real-model nor audio behavior.

### Native renderer failures

If chat exits with code 101 and `chat.log` records egui-wgpu's
`Failed to create staging buffer for index data` (or vertex data), the native
window's GPU upload failed. A sufficient destination-buffer capacity does not
establish a working staging allocation or device. The error alone identifies
neither the inference runtime nor the underlying driver failure. New launches
record the selected graphics adapter/backend and any device-lost reason in the
same retained stderr evidence.

The [retained Adreno X2-90 failure](evidence/native-renderer-20261103/summary.json)
selected Vulkan and recorded device loss with a driver-fault message before the
upload panic. In that case, select DX12 for the next explicit attempt. Repeating
Vulkan does not test an alternative backend. DX12 startup remains unqualified by
that failed launch; renderer selection does not change the inference engine.

The native UI supports DX12, Vulkan and OpenGL/GLES through wgpu. After rebuilding
the maintenance executable, select one explicitly with
`bonsai-tools chat --graphics-backend dx12` or
`./scripts/Start-Chat.ps1 -GraphicsBackend dx12`. Accepted values are `dx12`,
`vulkan` and `gl`. The option overrides `WGPU_BACKEND` only for the chat child;
omitting it preserves the inherited setting and upstream adapter selection.
The deferred request retains the requested option, while `chat.log` records
the actual selected adapter/backend. This selects only the window renderer;
it does not enable NPU inference or change model selection.

When using an older maintenance executable without this option, preserve your
current `WGPU_BACKEND`, set it for the attempt and restore it even when launch fails:

```powershell
$previousBackend = $env:WGPU_BACKEND
try {
    $env:WGPU_BACKEND = 'dx12'
    .\scripts\Start-Chat.ps1 -NativeEngine
} finally {
    $env:WGPU_BACKEND = $previousBackend
}
```

There is no automatic relaunch or request replay. A model-free `--preview` checks
the same renderer without loading a model; device diagnostics and their setup,
comparison and cleanup still share the five-minute owner-device budget.

### Speech setup and operation

The app prepares native ARM64 Qwen3-ASR 1.7B and Qwen3-TTS 1.7B
VoiceDesign/Base companions. Rust owns services, authentication, validation, WAV
processing and process ownership. TTS also performs Talker, acoustic prediction,
codec and speaker inference in Rust on the CPU; it requires no native speech
library or compiler at deployment. Qwen3-ASR retains its reviewed CPU C++ core.
The portable application's runtime dependencies are linked into its executables;
users do not install a separate Visual C++ speech runtime.
Swift's `native-q2` policy is separate. TTS NPU/GPU, ternary and new 48 kHz
artifacts retain independent gates. See the [TTS contract](../src/speech-tts/README.md).
The four pinned Q8_0 GGUF files total about 6.1 GiB under
`%LOCALAPPDATA%/SwiftBonsai2/speech/native/` by default; ASR uses `qwen3-asr/<revision>/`, and TTS uses
`qwen-tts/<revision>/`. Downloads resume with exact size/SHA-256
verification; launch checks cached sizes, then each service fully hashes and holds
files before first load. Original snapshots/private runtimes are preserved.

Speech setup starts automatically after text preparation. Follow its progress in
**Settings > Voice** and select **Retry speech setup** after fixing a reported
network, storage or permissions problem. Verified downloads are reused; a speech
failure leaves text chat available. No microphone opens during setup, and speech
models load only when transcription or read-aloud is requested.

The microphone button beside Send starts input only after explicit action and
Windows permission. Click it again to finish the utterance; capture also finishes
at 25 seconds. Mono 16 kHz WAV audio stays in memory and is transcribed locally by
Qwen3-ASR 1.7B with language autodetection. The recognized text enters the composer
and is sent once through the normal Swift prompt path if the model is ready.
If Swift is unavailable or still preparing, the text remains a draft for explicit
Send; readiness never sends it later. This is bounded utterance capture, not VAD
or streaming ASR. Cancel/Escape, revocation, Send, conversation changes, read-aloud
or closing cancel input and invalidate late results. Missing/not-ready Qwen3-ASR
or denied permission keeps capture off and exposes setup guidance.

The **plus button** at the composer's lower left selects one local audio file.
You can also copy an audio file in Explorer and press **Ctrl+V** while the composer
has focus. Supported extensions are MP3, WAV, M4A, AAC, FLAC and WMA, subject to
the installed Windows media decoders. Import and conversion run off the UI thread,
keep audio in memory and require no microphone permission. Select one file at a
time. File size and total duration have no fixed cap; available physical memory
and commit govern buffer and transcript allocation. One native asynchronous decoder
reads forward through the file, without repeated seeking or transcoder setup.
The worker transcribes one mono 16 kHz PCM16 WAV segment at a time, fitting the
recognizer's 30-second request contract. While recognition runs, the same decoder
prepares at most one following segment, with its extra buffer included in memory
admission. This overlaps decoding with recognition between segments. It prefers
a quiet pause in the final five seconds of a segment,
carrying all audio after the cut into the next request. Without a clear pause it
uses the full segment; the final segment keeps the complete remaining audio.
Actual decoder EOF determines completion, rather than the file's duration tags.
Sample waits are cancellable; native file opening and resource release remain
synchronous Windows operations. Decoder errors or unsupported timeline/format
changes stop import and preserve any completed transcript.
This energy heuristic does not guarantee a speech boundary or improved accuracy.
Import uses the same
Qwen3-ASR 1.7B transcription path as microphone input. Transcripts replace the
composer selection or insert at its cursor and count as ordinary text prompts:
the app sends once when Swift is ready, or leaves a draft for explicit Send otherwise.
If transcription finishes while its viewer is open, the result stays in the draft
for explicit Send. Later readiness or closing the viewer never sends that draft.
A transcript that exceeds the chat prompt size stays complete in the separate
transcript view; it never truncates the recording or modifies the existing draft.
The composer's audio card shows the current segment range, estimated total duration
and **Cancel transcription**. A bar shows completed audio only while the duration
estimate supports it; it does not predict remaining processing time. Rejections and
failures remain visible with a reason and next step, even after microphone cleanup.
**View transcript** opens a centered modal with read-only selectable scrolling text
and copy feedback; background shortcuts are blocked, and Escape closes the viewer.
**Copy transcript** also retains completed segments after failure or cancellation.
Transcripts stay in session memory until **Dismiss audio status** or a new import's
first segment is accepted. Cancelling the picker or failing a new import preserves
the previous transcript; copy it to keep it after closing the app.
Outside the viewer, Escape cancels import/transcription; conversation changes and
closing also cancel it and discard late results. Unsupported, empty or failed audio
leaves the existing draft intact.

Each completed response's lower-right **speaker button** reads it independently,
including while a newer response generates. While speech loads or waits for audio,
the active button shows a spinner and remains available to cancel. Once audio
plays, it becomes **Stop**. Normal preparation, playback and explicit cancellation
do not open the error panel; only an observed failure does. After a failure,
**Settings > Voice** offers in-app speech setup repair. The app retains service
paths and failure categories in **Settings > Application > Diagnostics**;
request text, voice descriptions and server error bodies are excluded.
Selecting another response switches playback. Empty/streaming responses cannot
start manual playback. Only visible answer text is spoken, excluding reasoning.
Describe the **Voice profile** in **Settings > Voice**, for example "A warm,
low-pitched narrator with a calm pace and clear consonants." No preset or
reference recording is required.
The native library's [fixed voice conditioning](../src/speech-tts/README.md#voice-conditioning)
reuses one reference across segments and messages with the same profile.
Changing the profile creates a new reference; restarts regenerate it from the
same fixed inputs. This design does not guarantee perceived speaker identity.

**Read aloud as responses arrive** is off by default per conversation. It sends
visible answer deltas in bounded sentence/word chunks while Swift generates;
loading/synthesis can lag text. **Stop speech** or manual replay suppresses
automatic reading for that response; the next prompt re-enables it. Disabling the
checkbox stops automatic reading while preserving manual replay. Extra effort
reads visible stages and cancels queued audio on replacement. Conversation
switch or generation Stop cancels speech. Model failure cancels the active chat's
speech; a hidden response never interrupts manual playback in the visible chat. Saving retains the
checkbox without replay on restore. Working drafts and reasoning are excluded.

Owned speech uses separate Windows Jobs and mutual exclusion: read-aloud releases
Qwen3-ASR before TTS loads; transcription releases TTS. Qwen3-ASR loads only after
a captured utterance or imported audio file. After a complete successful input,
its verified model and session stay warm for at most 30 idle seconds, avoiding
another full-file verification and model load for the next recording or file.
Reuse rechecks authenticated readiness and owned listener identity; each request
resets native recognition state. Cancellation, failure, conversation changes,
read-aloud, idle expiry or shutdown releases the owner. Resource monitoring
continues during the lease. Submitting completed text alone preserves the lease.
Qwen3-TTS loads on demand after Swift startup, synthesizes sequential 256-character
segments with bounded streaming playback/prefetch and stays warm at most 30 idle
seconds after success. Stop, failure, transcription or shutdown releases it immediately;
resource monitoring continues while warm. Each service requires 9 GiB available,
a 5 GiB physical reserve and a 16 GiB Job commit ceiling. Swift retains its
selected context and memory admission with a 4 GiB host reserve; speech failure
preserves text chat and answers.
[Streaming resources](streaming-resources.md) owns queue, segmentation and playback bounds.

TTS automatically reserves two logical CPUs, with at least one inference thread;
`bonsai-tools chat --tts-threads 1` through `256` selects an explicit budget and zero restores
automatic sizing. More threads do not imply better throughput. Q8_0 remains the
default. Evaluate smaller Q4_K_M talkers with
the unchanged Q8 codec explicitly:

```powershell
bonsai-tools chat --data-directory $data --tts-quantization Q4_K_M
& $env:BONSAI_TOOLS_PATH speech-build --json (@{
    operation = 'models'; dataDirectory = $data; workingDirectory = [IO.Path]::GetTempPath()
    model = 'QwenTts'; ttsQuantization = 'Q4_K_M'; verifyOnly = $false; deferCachedVerification = $false
} | ConvertTo-Json -Depth 64 -Compress) | ConvertFrom-Json -AsHashtable
& $env:BONSAI_TOOLS_PATH speech-evaluation --json (@{ operation = 'host-plan'; repetitions = 2 } | ConvertTo-Json -Depth 64 -Compress) | ConvertFrom-Json -AsHashtable
```

Each immutable profile is verified before loading. Actual evaluation belongs to
the main-only TTS workflow; [speech memory](performance-expectations.md#q4-speech-resource-comparison) defines paired
fresh/warm resource and listening review. Q4 is not qualified as equivalent to Q8;
streaming, prefetch and warm reuse alone establish no throughput or peak-memory gain.

For external services, pass `bonsai-tools chat --canary-key-file C:\Speech\canary.key`
and/or `--tts-key-file C:\Speech\qwen-tts.key`. Default ports 8766/8767 must differ from
each other and chat ports. Key-only services remain externally owned and trigger
no unused provisioning/builds. Authenticated capabilities are checked; TTS requires
`consistent_voice: true` and can use complete-WAV playback without streaming.
No substitute recognizer or Windows voice is used. Idle controls expose missing
setup guidance. See [Qwen3-ASR](../tools/canary-asr/README.md) and
[Qwen](../tools/qwen-tts/README.md) for protocols. Native smoke checks establish a
working path, not full audio quality, latency or daily-use qualification.

### Source launch, caches and runtime selection

This section covers contributor tooling. ASR builds require the pinned Rust
compiler, Visual Studio 18 ARM64 C++ tools and Windows SDK. TTS builds through
Cargo without GGML, CMake or external native source provisioning. These build tools
are not required on a machine running the portable executable. Pinned LLVM
ARM64/engine archives and all outputs stay outside Git; only child compiler settings
change. Downloads use Windows `System32/curl.exe` 8.4+ with HTTPS-only redirects
and resumable verified partials; LLVM extraction uses `System32/tar.exe`.

For model-only inspection, `bonsai-tools chat --speech-models-only` returns verified
artifact paths without building or launching companions. `speech-build` with
`operation: "models"` and `model: "QwenAsr"` or `"QwenTts"` selects one engine.
The legacy `"Canary"` selector, `CanaryModelPath` result and `--canary-*` connection
options name Qwen3-ASR. The explicit `provision` operation `speech-reference`
downloads original reference snapshots, which the app does not use.
`--no-tts` skips automatic TTS preparation; `--no-speech-models` skips all automatic
speech preparation. `--asr-path PATH` and `--tts-path PATH` select built executables;
services still verify their models.

`bonsai-tools chat` directly opens the native application; an owned `chat-prepare`
worker runs after the first UI tick. Rust owns options, speech setup, builds,
credentials, per-user admission and teardown. The maintenance executable can be
selected from a trusted hosted artifact or built once with the commands above;
the optional `Start-Chat.ps1` only forwards to this command. There is no
source-bootstrap script or PowerShell module dependency. Unchanged
launches call the executable directly without running Cargo. The JSON
`chat-launch` command remains available for automation.

Native commands observe and retain the caller's PID and creation identity before
supervised work; explicit caller identities remain supported. Caller loss stops
owned work, and native Jobs retain descendant cleanup. Source builds publish
checked immutable snapshots when requested through `source-build`.

Chat/broker outputs use `DataDirectory/build/chat/`; companions use
`DataDirectory/build/native-speech/`. Source/build fingerprints and SHA-256 executable
receipts skip Cargo, LLVM checks, source staging and VS setup when unchanged.
Speech receipts/output additionally bind CPU registry identity, Windows kernel
and OS processor-feature observations: moving X1 data to X2 rebuilds native
objects and CMake ISA probes. Missing mandatory host facts reject reuse/build.
Source, embedded/configuration pins, locks, local dependencies, build environment
or executable changes invalidate receipts; unknown embedded inputs disable reuse.
Documentation and CI changes alone do not rebuild Rust. Cargo's
content freshness handles preserved timestamps. Each variant has an atomic receipt,
short labels protect MSVC path space, and no startup cache cleanup is needed.
A version probe is reused only for the executable digest checked during that launch.

Root `.worktrees/` storage is excluded from the automatic source inventory, so
unrelated worker checkouts cannot delay launch or invalidate these receipts.
Explicit embedded files there still enter the fingerprint; Cargo path dependencies
there disable receipt reuse rather than treating untracked dependencies as stable.

Build cancellation, owner exit or crash closes its Windows Job and waits for
descendants before releasing the build lock. Writes complete before source/receipt
publication; corrupt completed partials are preserved before one fresh attempt.
Session credentials and diagnostics stay fresh. `--force-build` repeats Cargo/native
preparation, including after external compiler/cache changes. Absolute `--chat-path`
and `--broker-path` must select matching current builds; older apps lack deferred
startup flags. Unique system-temp lanes retain `source-build-*.log` and `chat.log`.

One user-wide exclusive `%LOCALAPPDATA%/SwiftBonsai2/chat.lock` covers preparation
through app exit, including launches from other checkouts/packages/data/ports.
Repeated launches report already starting/running. The empty sentinel persists
after release; `--speech-models-only` is independent. Direct `bonsai-tools chat`
and installed-package launches retain native admission. Installed launches use [package verification](chat-packages.md#launch-without-a-checkout).

### Runtime selection

| Runtime/model | Source and installed launch options | Boundary |
| --- | --- | --- |
| Native Rust Prism 0.1.26, original Swift PQ2 | Development default; optional explicit `--native-engine --use-original-pq2`; pinned adapter enabled | Four CPU row workers, adapter scale 1.3, K4/V4 cache and supported X2 resident `native-q2` acceleration. Exact artifact/readiness checks remain mandatory; full daily-use qualification is pending. |
| Native Rust Prism 0.1.26, current-weight PTQ1 export | `--use-repacked-ptq1`, optionally `--native-engine` | Experimental opt-in; separate [PTQ1 qualification](swift-memory.md#current-weight-ptq1-option) remains pending. Missing/changed bytes reject. |
| Publisher Prism CPU, original Swift PQ2 | `--publisher-engine`, optionally `--use-original-pq2` | Manual rollback with the same pinned adapter and response scale; no automatic fallback. |
| Publisher Prism CPU, current-weight PTQ1 export | `--publisher-engine --use-repacked-ptq1` | Same adapter opt-out/scale. No older publisher PTQ1 substitution. |

`--no-abliterate-lora` (direct app `--no-abliterate-lora`) omits the adapter and disables
its control. Launch prepares the selected pinned runtime automatically, including
an explicitly selected publisher rollback. No prior runtime-provisioning command
is required. Keep the same model, adapter and data-directory options when changing
an explicit engine selection.

Engine options are mutually exclusive. Installed launches retain
`NativeEngine: false` for publisher rollback and `NativeEngine: true` for explicit
Rust selection; omitting it selects the current development default.

The owner selects [TurboQuant-inspired K4/V4 (`rsq4`)](quantized-kv.md) as the
development default in new Swift runtime source. Runner activates that default
only for the exact reviewed runtime source after published-bundle verification
and repinning. The 0.1.26 development pin retains it; the historical 0.1.23
control retains FP16. This upgrade supplies no measured CPU or cache benefit. Trusted
source launches accept `--kv-cache f16|rsq4|rsq4k3v`
(`-KvCache F16|Rsq4|Rsq4K3V` through `Start-Chat.ps1`); installed-launch JSON uses
`KvCache: "f16"`, `"rsq4"` or `"rsq4k3v"`.
Omission follows the pinned runtime's reviewed default. Explicit `rsq4` rejects
an unsupported pin before starting inference. Publisher/reference launches reject
this native cache selection. Choose `f16` on a fresh launch for comparison or
recovery; retry preserves the cache, and client requests cannot change it.
Cache readiness includes the expected numerical graph identity. The development
choice accepts approximation and pending measured quality/memory qualification;
context and process admission follow the selected-cache memory planner.
Trusted native `--placement cpu` (installed JSON `Placement: "cpu"`,
PowerShell `-Placement cpu`) skips accelerator initialization with the reviewed
0.1.26 pin and survives setup and Retry. Readiness must report both CPU placement
and CPU projection execution. Omission or `auto` retains automatic placement;
this selection does not reduce the conservative memory allowance.

The prepared `rsq4k3v` capacity option rejects the current pin. Activation requires
a separate exact-source review of K4/V3 with the complete
[memory-selected context contract](swift-memory.md#memory-selected-serving-context).

A publisher startup diagnostic of `process=exited-0xC0000142` means Windows failed
runtime initialization. **Retry model** repeats the selected runtime; it does not
switch to Rust. Close the window and relaunch with `--native-engine` to select Rust
explicitly; the launcher prepares its pinned runtime. Retain the same data directory,
model and adapter options;
`--native-engine` alone keeps original PQ2 and the approved adapter selected.

Use `bonsai-tools chat --native-engine --use-original-pq2` to select the pinned
original Swift weights explicitly. `--use-original-pq2` and `--use-repacked-ptq1`
are mutually exclusive in the launcher, native app and broker. The thin
`Start-Chat.ps1` facade forwards `-UseOriginalPq2` or `-UseRepackedPtq1` to the
same Rust policy. Installed launches use `UseRepackedPtq1: false` for original
PQ2. Explicit selection survives deferred startup and retry without relying on
an omitted representation option; artifact failure never switches representation.
Original PQ2 and adapter scale 1.3 remain the defaults. The engine selection is
independent of the weight selection; current-release device rollback still
requires qualification.

Native Rust defaults to four row workers using the reviewed 0.1.26 implementation.
`--native-row-workers 1` explicitly selects one worker. Publisher/reference
launches default to one and reject four; no request can override this private launch setting.
The [fixed-input comparison](evidence/row-parallel-smoke.json) preserves all logits
and logical state bytes with a lower one-step evaluation duration. Application
startup/streaming acceptance, complete-answer latency and broader qualification
are separate. Keep the same model, adapter and resource limits when comparing.

The [full runtime-promotion policy](native-rust-engine.md#default-runtime-promotion)
remains unchanged and independent of this Rust/PQ2 development choice. Thinking
uses the same setting across engines; native quality, throughput and daily-service
qualification remain pending. Native HTTP remains numeric loopback
with the [broker's strict public subset](broker-policy.md).

`--frontend-port`/`--backend-port` select distinct local ports. Launch prepares the
compiled Swift model, selected adapter and runtime automatically before broker
handoff. Setup progress, safe Pause/Retry and separate speech retry need no shell
commands. Explicit PTQ1 still requires its reviewed export and cannot trigger a
PQ2 fallback. Keep credentials and complete
evidence directories private.

## X1/X2 configuration boundary

Retained inventories cover Snapdragon X2 Elite Extreme X2E94100 and a 32 GB
X Elite X1E-78-100; see [device evidence](cpu-kernels.md#x2-cpu-thread-screen).
The Rust/PQ2 development policy can use resident `native-q2` acceleration on X2;
X1 Hexagon/Adreno acceleration needs independent qualification. Shared CPU
work reductions may use [X2 benefit with an X1 applicability review](cpu-kernels.md#cross-generation-cpu-promotion)
for common deployment without an X1 benchmark. This fixed policy is separate
from qualified automatic profile selection.
[Hardware profiles](hardware-capabilities.md#automatic-profile-selection)
defines qualification for device/workload/objective selection, phase
placement, live-budget admission and CPU fallback/refusal. SKU names and TOPS
never authorize acceleration; [PLAN.md](../PLAN.md) owns pending work.

## Inventory and provisioning

These are contributor inspection and artifact-tool API examples. Ordinary app
launch performs its own setup; users do not run this section before opening Swift.

Use an absolute local data directory outside Git on a hard-link-capable filesystem
such as NTFS; junctions/device paths reject. The default retains `SwiftBonsai2`
for installation compatibility, separate from the `SwiftBonsai2Runner` checkout.

```powershell
bonsai-tools doctor --json '{}'
# Optional existing data directory and new report file:
bonsai-tools doctor --json '{"dataPath":"D:\\BonsaiData","outputPath":"D:\\Reports\\doctor.json"}'

$data = Join-Path $env:LOCALAPPDATA 'SwiftBonsai2'
bonsai-tools provision --json (@{ operation = 'engine'; dataDirectory = $data } | ConvertTo-Json -Compress)
bonsai-tools provision --json (@{ operation = 'model'; dataDirectory = $data } | ConvertTo-Json -Compress)
bonsai-tools provision --json (@{ operation = 'engine'; dataDirectory = $data; verifyOnly = $true } | ConvertTo-Json -Compress)
bonsai-tools provision --json (@{ operation = 'model'; dataDirectory = $data; verifyOnly = $true } | ConvertTo-Json -Compress)
```

These defaults provision the Qwen reference lock: about 13.15 GB of model data
and a 12.78 MB CPU runtime archive. To provision Swift instead, add
`lockPath = (Join-Path $PWD 'config/swift-assessment.lock.json')` to both requests.
Neither path downloads fallback, projector or full-precision weights.

Model bytes live under `models/<revision>/`, archives under `downloads/`, and
installed runtimes under `runtime/<commit>/`. Native downloads resume immutable URLs
through a bounded HTTPS-only curl process and per-artifact synchronization lock.
The verified partial remains open with writes/deletion denied through same-volume
hard-link publication, which cannot overwrite a destination. Invalid complete
partials are quarantined before one fresh retry; a second digest failure stops.
Cancellation and deadlines stop the complete owned transfer. Publication failures,
invalid bytes and failed extraction staging remain for inspection. Provisioning
never substitutes another model/runtime.

`bonsai-artifacts` shares ZIP, SHA-256, installed-file and PE checks between the
broker and maintenance. Extraction rejects unsafe or duplicate paths and excessive
expansion. Runtime verification compares installed files to the verified archive,
ARM64 PE architecture and reported build identity. Guards deny write/delete access
to the archive, verified files and ancestors through bounded version/help probes.
CPU and paired completion re-verify and hold fresh guards through execution;
GGUF inspection/export holds its source and validates output before publication.
Provisioning invokes a trusted maintenance executable directly and never builds it.
Receipts cannot transfer live file ownership, prove parser isolation or attest
source-to-binary provenance. See the API contracts for
[bounded native downloads](../tools/bonsai/src/startup_download.rs),
[PE images](https://learn.microsoft.com/en-us/windows/win32/debug/pe-format),
[exclusive creation](https://doc.rust-lang.org/std/fs/struct.OpenOptions.html#method.create_new)
and [hard links](https://learn.microsoft.com/en-us/windows/win32/fileio/hard-links-and-junctions).

Doctor marks unavailable facts when inventory queries are denied. Reports include
local paths; review them before sharing.

For bounded GGUF v3 metadata inspection after full size/SHA-256 verification:

```powershell
$inference = bonsai-tools provision --json (@{ operation = 'inference'; dataDirectory = $data } | ConvertTo-Json -Compress) | ConvertFrom-Json
$request = Join-Path $data 'inspect-request.json'
@{ operation = 'inspect'; modelPath = (Join-Path $data 'models/4ca720788d1e01f1bff70c033e0d0028fd02e502/Qwen3.8-27B-UD-Q3_K_XL.gguf'); ggufToolPath = $inference.Gguf } |
    ConvertTo-Json | Set-Content -LiteralPath $request -Encoding utf8NoBOM
bonsai-tools gguf --request $request
```

The [GGUF tool](https://github.com/symbiogenesis/prism-inference/blob/main/tools/gguf/README.md) also supports explicit Prism census and
streaming PQ2-to-PTQ1 export. The trusted prebuilt `bonsai-tools.exe` selected
during setup supplies the [native `gguf` command](../tools/bonsai/README.md), which
owns artifact defaults, policy and complete-process deadlines. `ggufToolPath`
selects an explicit trusted GGUF executable; otherwise Rust verifies the installed
pinned inference release without compiling or downloading.
Metadata inspection skips tensor payloads; census and export read them.

## Bounded CPU load/decode check

Run the trusted prebuilt maintenance executable directly:

```text
bonsai-tools cpu-check --request C:\Evidence\cpu-check.json
```

```json
{"dataDirectory":"C:\\BonsaiData","profile":"swift-pq2-assessment","checkOnly":true,"useAbliterateLora":false,"useRepackedPtq1":false}
```

Set `checkOnly` to `false` for one bounded load/decode check. The default profile
is `reference-cpu`; Swift options require `swift-pq2-assessment`. Provision the
selected runtime, model and optional adapter first. Select one workload within
the shared owner-device budget.

`checkOnly: true` reports admission without starting a model. Execution uses in-process
`llama-completion`, a 16-token cap, 8K context, FP16 K/V, F32 recurrent/convolution
state, no prompt-cache reuse and a 180-second model deadline. One 240-second
aggregate allowance includes setup, artifact verification and owned cleanup. Swift uses batch
128/microbatch 64. The embedded template's
thinking behavior is requested explicitly with `--reasoning on` and
`--reasoning-effort xhigh`, alongside the publisher sampler. No system-only
preamble is injected: Swift's verified template rejects it. This check preserves
that template and does not qualify another chat profile. It is not a
finished-answer quality check; see
[inference defaults](inference-defaults.md) for the pinned completion renderer's
template-argument limitation. Modern `llama-cli` starts a server and is not used
here.

| Profile | Service allowance | Required available RAM, including 4 GiB reserve |
| --- | --- | --- |
| `reference-cpu` (default Qwen) | 18 GiB | 22 GiB |
| `swift-pq2-assessment` | 11.5 GiB | 15.5 GiB |

Admission fails closed without closing other applications. The packaged
[maintenance tool](../tools/bonsai/README.md) owns the process in a kill-on-close
Windows Job from creation, with a job-wide commit cap, one-process limit and no
breakaway. It monitors process memory, available physical memory, parent liveness,
deadlines and output bounds. A Job commit cap does not cover every resident
file-backed mapping, so the physical-memory watchdog remains necessary. Reported
Job peaks can include denied allocations; these diagnostics are not performance
or peak-memory acceptance.

Select a trusted prebuilt Windows ARM64 `bonsai-tools` executable from the evaluated
main revision and invoke that executable directly (the examples select it through
`BONSAI_TOOLS_PATH`). Prepare the executable before the device allowance begins.
Model-free planning never starts inference.

Swift and LoRA load/decode success does not establish finished-answer quality,
throughput, numerical basis or refusal behavior. The approved standard LoRA
requires [paired same-backend TPS and quality gates](swift-lora.md#standard-profile-qualification).

## Prepare the server policy

```text
bonsai-tools server-plan --request C:\Evidence\server-plan.json
```

```json
{"dataDirectory":"C:\\BonsaiData","port":8080,"verification":"none"}
```

Native policy planning creates no model/key storage, inference process or listener.
Set `verification` to `runtime` for bounded installed-runtime version/help probes,
or `artifacts` to additionally verify the existing model. These operations never
download or load a model. Rust rejects unknown options, unsafe/network paths and
inherited runtime overrides, including `AIP_*`/`MTMD_*`, and checks every planned
switch against the verified server help. Environment values stay in the parent.

The planned arguments select numeric loopback, one 8K FP16 CPU slot, a private
key-file path, no host tools/MCP/UI, no reuse/speculation and the reference sampler.
Help checks establish switch availability, not option enforcement. Native server
Host/Origin checks, control routes, per-request overrides and total duration need
the broker policy; an offline flag is not OS egress denial. The Rust
preflight remains launch-disabled even though the separate development broker
implements authentication, request bounds and lifecycle controls.

## Explicit development broker

The [broker contract](broker-policy.md) defines authentication, exact counting,
HTTP bounds and cancellation: one active request, zero through eight waiting,
30-second queue wait and no total generation-time limit. Intake, connection and
write operations retain their individual deadlines. Keep the sending side open
until completion. Interrupted generation closes admission permanently; recovery
requires explicit backend teardown/relaunch.

Build from an external native working directory:

```powershell
$repo = (Get-Location).Path
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-Broker-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($scratch)
$previous = [Environment]::CurrentDirectory
Push-Location $scratch
try {
    [Environment]::CurrentDirectory = $scratch
    cargo +nightly-2026-11-12-aarch64-pc-windows-msvc build --locked --release --manifest-path (Join-Path $repo 'src/broker/Cargo.toml') --target-dir (Join-Path $scratch 'build')
    if ($LASTEXITCODE) { throw 'Broker build failed.' }
} finally {
    [Environment]::CurrentDirectory = $previous
    Pop-Location
}
$broker = Join-Path $scratch 'build/release/bonsai-broker.exe'
```

### Connect an existing development backend

Create fresh protected keys under an existing owner-controlled external parent:

```powershell
$keys = & $env:BONSAI_TOOLS_PATH credentials --json (@{ parentDirectory = $data } | ConvertTo-Json -Depth 64 -Compress) | ConvertFrom-Json -AsHashtable
```

Configure the separate backend to use `$keys.BackendKeyFile`, then run the broker
from the external native working directory:

```powershell
$previous = [Environment]::CurrentDirectory
Push-Location $scratch
try {
    [Environment]::CurrentDirectory = $scratch
    & $broker --development --allow-unverified-key-files `
        --frontend-port 5173 --backend-port 8081 `
        --frontend-key-file $keys.FrontendKeyFile --backend-key-file $keys.BackendKeyFile --max-waiting 2
} finally {
    [Environment]::CurrentDirectory = $previous
    Pop-Location
}
```

This mode binds only `127.0.0.1`; frontend port zero requests an OS-assigned port.
It requires both opt-ins because it does not verify key ACLs, ancestor ownership
or backend process identity. The helper returns paths only and creates distinct
64-byte lowercase-hex credentials with protected owner/LocalSystem ACLs. Keys and
partial private directories remain for inspection. Same-user processes and the
parent-directory owner remain within the trust boundary. No UI route is supplied.

### Owned Windows development backend

The launcher defaults to the compiled-in Qwen `reference-cpu` pins; Swift and
its pinned adapter require the explicit selections below. Selecting the Swift
profile defaults to Rust with four row workers; add `--publisher-engine` for
publisher comparison/rollback. It performs no download
and accepts no arbitrary arguments. Use existing external data/evidence directories:

```powershell
$previous = [Environment]::CurrentDirectory
Push-Location $scratch
try {
    [Environment]::CurrentDirectory = $scratch
    & $broker --launch-development --allow-unisolated-backend `
        --data-directory $data --evidence-parent ([IO.Path]::GetTempPath()) `
        --frontend-port 5173 --backend-port 8081 --max-waiting 2
} finally {
    [Environment]::CurrentDirectory = $previous
    Pop-Location
}
```

Append `--check-only` for read-only artifact/policy verification and memory
admission reporting, without credentials, a listener or a process.

Pass `--profile swift-pq2-assessment` for Swift's original PQ2 model, provisioned
with `config/swift-assessment.lock.json`. Provision the native runtime with
`provision` operation `inference`; publisher rollback instead uses operation
`engine` with the Swift lock, as shown in [runtime selection](#runtime-selection).
Publisher Swift uses batch/microbatch 128/64. Native Prism 0.1.26 and publisher
Swift select context and the Job/admission budget from available memory under the
[context policy](swift-memory.md#memory-selected-serving-context). The native planner
charges the selected cache and sequential scratch, retaining the 4 GiB host reserve
and startup margin. The historical 11.5 GiB Job/15.5 GiB fixed-8K controls retain
their original identities.
Add `--use-abliterate-lora` for the pinned adapter and per-request scale (default
1.3); the native UI requests it automatically, while direct broker launches omit it.
Publisher Prism and the pinned native Rust service both accept the approved adapter with PQ2 or explicit
`--use-repacked-ptq1`; native Rust verifies representation and adapter readiness.
The [runtime-selection table](#runtime-selection) lists app options. Profile names
are exact; arbitrary adapters, models, runtimes and inference overrides reject.

The [launcher contract](broker-policy.md#owned-windows-development-launcher) owns
retained artifact handles, fresh protected credentials/evidence, health/authentication
and listener-PID checks, memory guards and whole-Job teardown. Reference uses an
18 GiB Job cap/22 GiB admission with the same 4 GiB physical reserve. Startup,
guard or backend failure, Ctrl-C, closed generation admission and owner loss all
stop the Job; every backend connection rechecks listener identity.

The printed evidence directory contains `frontend.key`, `backend.key` and technical
logs. A client may read `frontend.key`; never share either key or the whole
directory. This example explicitly retains caller authority. The native UI instead requires
[sandboxing](sandboxing.md); broker-only/same-user bypass prevention and actual-model
daily acceptance remain unqualified.

## Inference runtimes and source builds

### Provision a native Rust Swift backend

Provision the independently released
[Prism Inference](https://github.com/symbiogenesis/prism-inference) Windows ARM64
executables, then select the Swift profile in the owned launcher. Its engine
default is Rust; the explicit `--native-engine` option remains accepted.
This is a prebuilt-runtime launch, not a source build. Use `$broker` and `$scratch`
from [development broker setup](#explicit-development-broker), and the provisioned
Swift weights from [app setup](#native-chat-app):

```powershell
$inference = bonsai-tools provision --json (@{ operation = 'inference'; dataDirectory = $data } | ConvertTo-Json -Compress) | ConvertFrom-Json
bonsai-tools provision --json (@{ operation = 'inference'; dataDirectory = $data; verifyOnly = $true } | ConvertTo-Json -Compress)
$previous = [Environment]::CurrentDirectory
Push-Location $scratch
try {
    [Environment]::CurrentDirectory = $scratch
    & $broker --launch-development --allow-unisolated-backend `
        --profile swift-pq2-assessment --native-engine `
        --data-directory $data --evidence-parent ([IO.Path]::GetTempPath()) `
        --frontend-port 5173 --backend-port 8081 --max-waiting 2
} finally {
    [Environment]::CurrentDirectory = $previous
    Pop-Location
}
```

The [release lock](../config/prism-inference.lock.json) pins the source revision,
archive and every installed file by length and SHA-256. Provisioning checks the
ARM64 executable architecture and installs under
`inference/<commit>/aarch64-pc-windows-msvc/` in the external data directory.
Rust `provision` and the launcher consume these binaries without invoking
Cargo. Updating the runtime means reviewing a new release lock and provisioning
that release. For GGUF commands using a custom data directory, set
`BONSAI_DATA_DIRECTORY` to the same absolute path, or supply the native `gguf`
request's `dataDirectory` or explicit `ggufToolPath`.
The crates.io packages provide the Rust source distribution;
ordinary `cargo install` compiles that source.

Add `includeCapabilities: true` to the inference provisioning request for a bounded model-free probe
of the verified release. It checks the reported version/native target and the
shared H0 schema, retains archive/file guards through exit and returns external
verification evidence. Invoke a trusted prebuilt `bonsai-tools` directly; provisioning
does not build the maintenance tool. See the
[capability contract](hardware-capabilities.md#runtime-inventory).

Run from an external native working directory. The selection accepts only the
original Swift model or explicit verified current-weight PTQ1 export and the verified
release. The exact approved adapter is optional; arbitrary models/adapters and backend
overrides are unsupported.
It retains the executable against replacement, fixes seed 42 and requires matching
native identity before frontend readiness. The selected context and Job/admission
budget, 4 GiB watchdog, protected keys and listener checks apply. Interrupted generation
closes admission and stops the backend; relaunch creates a fresh owned instance.

The [runtime contract](native-rust-engine.md) defines the pinned release's supported
loading, logits/state, tokenizer, sampler and recovery behavior;
[PLAN.md](../PLAN.md#runtime-and-correctness) owns pending qualification.
Private child protocol alone proves neither Job ownership nor same-user bypass
prevention.

### Prepare a runtime lock update

Runtime promotion uses checked prebuilt files from Prism's `main` Foundation
workflow. Review the exact source revision, all required run results and the
Windows ARM64 bundle receipt before generating a candidate. A receipt records
identities; it does not prove that the run passed or authorize publication.

Download the ARM64 bundle ZIP and receipt from that run, and the source ZIP from
`https://github.com/symbiogenesis/prism-inference/archive/<full-commit>.zip`,
into a new system-temp lane outside Git. Set `$reviewedVersion`, `$reviewedRevision`
and `$reviewedRunId` from the independently reviewed release and run, rather than
copying them from an unreviewed receipt:

```powershell
& $env:BONSAI_TOOLS_PATH inference-lock --json (@{
    receiptPath = Join-Path $lane "prism-inference-$reviewedVersion-aarch64-pc-windows-msvc.receipt.json"
    archivePath = Join-Path $lane "prism-inference-$reviewedVersion-aarch64-pc-windows-msvc.zip"
    sourceArchivePath = Join-Path $lane 'source.zip'
    sourceRevision = $reviewedRevision; version = $reviewedVersion
    runId = $reviewedRunId; runAttempt = 1
    outputPath = Join-Path $lane 'prism-inference.lock.candidate.json'
} | ConvertTo-Json -Depth 64 -Compress) | ConvertFrom-Json -AsHashtable
```

The command uses a trusted prebuilt `bonsai-tools` executable selected through
`BONSAI_TOOLS_PATH`; invoke it directly.
Production Rust provisioning still requires no Rust toolchain when using a prebuilt executable.
The review checks receipt identities, archive and individual file hashes,
the complete executable/license inventory, ARM64 PE headers and the source
archive's commit root, package versions and compiler pin. It writes a new
external candidate and retains `candidate.lock.json` and `verification.json` in
its verification directory. It verifies ZIP contents directly without extracting
the runtime, executing the reviewed binaries, building Prism source, publishing a
release or replacing the repository's lock.

Publication requires separate authorization under the session's repository
rules. After the exact checked bundle is available at the candidate's immutable
release URL, verify those downloaded bytes and review the candidate before
replacing `config/prism-inference.lock.json`. Rebuild the maintenance tool,
broker and native UI from that same checkout: all three embed the runtime pin.
Provision with the rebuilt tool's `provision` operation `inference`, then run the
required checks on the settled `main` revision. Preserve the prior release for
rollback. Packaging and hosted synthetic checks do not broaden the retained model/device evidence.

### Publisher Prism source build

Install Visual Studio 2026 native ARM64 C++ tools and CMake, or explicitly choose
the supported Visual Studio 2022 generator. The builder installs no toolchain.

```powershell
$doctor = bonsai-tools doctor --json '{}' | ConvertFrom-Json
$gitPath = ($doctor.Tools | Where-Object Name -eq Git).Path
$cmakePath = ($doctor.Tools | Where-Object Name -eq CMake).Path
& $env:BONSAI_TOOLS_PATH build-tools --json (@{ operation = 'engine'; repositoryRoot = $PWD.Path; dataDirectory = $data; gitPath = $gitPath; cmakePath = $cmakePath; fetchSource = $true } | ConvertTo-Json -Compress)
```

The builder fetches only the pinned commit, requires an exact clean checkout on
reuse, and builds Release `llama-server` under `builds/<commit>/cpu/`. External
receipts record configuration, compiler, hashes and version. Subprocess tools,
accelerators and automatic dependency/UI downloads are disabled. No test suite,
inference or benchmark runs; the native Rust UI is built separately from inference.

Default parallelism reserves two CPU threads and 2 GiB, budgets 1 GiB per compiler,
and caps project/shared compiler workers at eight. If memory is unavailable it
uses at most two. The `build-tools` request's `parallel` field accepts 1 through
12 to override that budget.
Incremental outputs are reused only after source/configuration checks.
