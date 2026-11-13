# Native Rust UI and local API contract

The first-party UI must remain **entirely native Rust**: application code, state,
events, rendering integration and network clients. Native OS APIs and audited
Rust bindings are permitted; PowerShell may provision, build and launch it.

Do not add a JavaScript/TypeScript frontend, HTML/CSS UI, browser shell, WebView,
Electron or web-rendered Tauri frontend, or the inference server's browser UI.
A replacement toolkit must preserve this boundary. Interoperability belongs at
the HTTP API boundary.

## Native application

`src/chat-ui/` provides the Windows `bonsai-chat` application using `egui`,
`eframe`, a native window and `wgpu`. State, widgets and HTTP clients are Rust.
The production manifest selects DX12, Vulkan and OpenGL/GLES through eframe's
`wgpu_no_default_features`, retaining upstream adapter selection and
`WGPU_BACKEND`. Source launches can explicitly override that variable for the chat
child with `--graphics-backend dx12|vulkan|gl`; omission preserves inheritance and
upstream selection. Browser/Metal backends are not requested; upstream test features
and installed drivers can affect backend availability. Toolkit portability does
not make Runner cross-platform: process ownership, credentials and speech remain
Windows-specific.

The native UI offers System, Light and Dark appearance and runs separately from
the broker/inference worker. Network, disk,
DPAPI and speech stay off the event loop, with bounded queues, cancellation and
joined shutdown. Windows high contrast, per-monitor scaling, labelled
AccessKit controls and keyboard navigation are supported. Full bidirectional
editing and native-control accessibility parity still require qualification.

### Launch and ownership

The native `bonsai-tools chat` command owns build reuse,
immutable tool snapshots and the per-user launch guard through teardown.
The [development guide](development-guide.md#source-launch-caches-and-runtime-selection)
owns source-launch receipts/cache rules; the [package contract](chat-packages.md#launch-without-a-checkout)
owns verified installed launches.

The single-file Windows distribution embeds the same five application executables
and their package manifest and notices in a native Rust launcher. Opening the
bundle verifies its embedded inventory, extracts it into the per-user package
cache and uses the existing verified package launch. Broker, UI and speech remain
separate owned processes with the same containment and teardown contracts. Model,
adapter and inference-runtime provisioning remain separate. Current releases
distribute this portable executable; their update path is verified manual
replacement, preserving the user data directory.

Swift launches default to native Rust Prism 0.1.25, original PQ2, four CPU row
workers and the standard adapter at scale 1.3. `--native-engine` remains accepted;
`--publisher-engine` selects the explicit publisher rollback. Engine options are
mutually exclusive, and retry preserves the selected engine and representation.
The UI identifies this as a development default with full-context/quality
qualification pending. Selection neither waives admission nor establishes full
release qualification. Supported X2 installations also select the experimental
resident `native-q2` path under the [Hexagon policy](hexagon.md); CPU fallback for
an absent signed package or CDSP driver does not change engine or representation.
PTQ1 remains an explicit opt-in.

The owner-selected cache default in new Swift runtime source is TurboQuant-inspired
K4/V4 (`rsq4`), with measured quality/memory qualification pending. Runner binds
that selection to the exact reviewed runtime source and verifies cache and startup
graph readiness. The 0.1.25 development pin activates K4/V4; the historical
0.1.23 control used FP16. This is an owner-selected development upgrade, with
performance and actual-model qualification still pending.
Trusted `--kv-cache f16` selects explicit recovery on a
new native Swift launch; retry preserves the selected cache. Requests cannot
override it or trigger an automatic downgrade. The [cache contract](quantized-kv.md)
keeps process admission, context limits and device qualification separate.

The launcher checks/builds presentation first. After the first UI tick, an owned
preparation worker verifies/provisions the selected pinned Rust runtime alongside
building/verifying the broker, then hands off text chat after both finish. Either
failure cancels and joins the other operation before reporting failure. Speech artifact
preparation, companion builds and fresh speech credentials continue separately
while Swift loads and serves text chat. A speech preparation failure leaves text
chat and its drafts/history intact; closing cancels both owned stages.
Speech artifact preparation uses at most two concurrent workers, preserves exact
verification and partial-download ownership, and cancels peers on failure.
The complete speech preparation Job, including compiler descendants, runs at
below-normal CPU priority so it yields to model loading and text chat. This does
not reserve disk bandwidth or change inference priority. Full-file artifact
verification uses bounded 1 MiB sequential reads and retains complete SHA-256,
length, cancellation and immutable-file checks before use.
Runtime preparation downloads only a missing locked runtime; model and adapter
provisioning remain explicit. Drafts and recovered conversations survive preparation;
model preparation failures leave the window available. Closing cancels preparation
and waits for owned descendants. Artifact verification and memory admission precede model use.
Owned inference and speech require [native containment](sandboxing.md), with no
uncontained fallback. Microphone consent is a separate UI capability check.

The client atomically places its broker in a separate kill-on-close Job before
the broker starts. Each attempt also has fresh ACL-protected private shutdown
storage. Stop and startup cancellation ask the broker to stop its independent
model Job and publish a fixed empty cleanup acknowledgment. The UI requires that
acknowledgment, broker exit and confirmed outer-Job cleanup before offering Retry;
a brokered PSEC model can lie outside the outer Job. All UI cleanup phases share
one fifteen-second absolute allowance, including the broker's ten-second model
stop and three seconds reserved for outer cleanup. Synchronous OS calls remain
nonpreemptible. Failed or timed-out teardown, or unknown model creation cleanup,
leaves the model unavailable and requires closing the app. This private file
control adds no HTTP endpoint and changes no model sandbox or resource admission.

The UI cannot load weights, choose arbitrary runtime arguments or bypass broker
artifact, model, memory, context or sampler policy. Explicit
[runtime/format and adapter choices](development-guide.md#runtime-selection) do not
establish promotion or full-model/device qualification.

### Application updates

Current portable releases update by replacing the application executable after
closing Swift and verifying the new download against its published SHA-256.
The native Settings view provides those instructions and an explicit link to
GitHub releases. Keep the same user data directory to preserve models, runtime
downloads, conversations and preferences. Portable, source and preview launches
start no automatic update discovery or installer work.

#### Legacy MSI updater

The Rust MSI updater remains as internal compatibility code, outside the current
portable release path. Its existing confirmation, publication validation and
process-ownership controls remain required if it is used or restored later.
Current Windows Release does not build or publish an MSI for that updater.

Only a legacy MSI-installed application discovers published GitHub releases.
A bounded background worker checks after startup and every six hours; the native Settings
view also accepts a manual check. Source launches and preview start no update
network requests. Update discovery sends no conversation or model data. Network,
metadata parsing and installer downloads stay off the event loop.

The maintenance executable consumes the repository's Windows ARM64 release
manifest, checksum inventory and GitHub asset identities. It accepts only a
strictly newer numeric version with matching source/release/architecture/file
identities. Published experimental prereleases remain eligible; draft releases
and non-Windows assets do not. No published release is an ordinary no-update
state. Failure retains the running app and never substitutes an unverified file.

Installation requires an explicit native confirmation and known owned teardown.
The UI queues a verified update request for its maintenance launcher and closes
through the ordinary persistence and process cleanup path. After the native
window exits, the launcher checks the published release again and requires the
staged receipt and installer to match that fresh identity. It retains its launch
guard, installer read handle and ancestor directory handles through the fixed
Windows Installer process creation. The queued request is consumed before
process creation; missing requests and final network/identity failures are errors.
It then exits, releasing
the guard so application files can be replaced. It must not install while model
ownership is uncertain or create a detached child that escapes the existing
containment contract.
The queued request alone never authorizes installation: the native window must
report the dedicated successful update exit after receiving its queued receipt.
An ordinary close, cancellation, crash or cleanup failure cannot activate a
leftover queue. Failed preparation or unknown model ownership disables in-app
installation while discovery and downloads remain available.
Conversation saving retains its existing opt-out; updating does not enable it.
Windows may request administrator approval. After the installer finishes,
reopen from the Start menu; automatic restart is not promised.
Model/runtime downloads and local user data
remain separate from MSI application files.

### Conversation and failure semantics

| Component | Contract |
| --- | --- |
| Workspace | No conversation-count cap on creation, branching or recovery. Each conversation retains its own draft, response settings, visible transcript and exact committed history. The encrypted workspace still has an 8 MiB byte budget and per-conversation message safeguards; this is not unlimited storage. Transcript count/byte eviction (at most 512 messages) never changes model history. |
| History search | A native search field above the conversation list filters titles and visible message bodies as you type, using Unicode case-insensitive substring matching. Reasoning and unsent drafts are excluded. Clear or Escape in the focused search restores the list before drawer dismissal or Stop. Filtering preserves the selected conversation and its draft; creating a conversation or branch clears the transient query so the new conversation appears. Rows are virtualized and unchanged-query matches are cached. The query is never saved to recovery or appearance preferences. |
| Presentation | Only user/assistant content enters the selectable transcript. Limited native text styles preserve every source character and byte offset; never interpret output as HTML, activate model-supplied links or execute code. Setup/retry/transport/speech status use separate controls and bounded collapsible diagnostics. Rename/delete/edit use centered native modals; deletion names the target and blocks background input. Cancel/Escape preserves the conversation/draft. |
| Message iteration | Edit and resend and Regenerate create a new conversation with the exact history prefix, preserving the original and its draft. Branch from here creates a conversation without sending; a selected user prompt becomes its draft. Continue response explicitly sends a followup to the last answer while retaining the existing draft. Actions verify saved message positions against history and respect readiness and request ownership without a conversation-count cap. An identified interrupted turn with visible answer text is included as a user/assistant pair and labelled Interrupted; the last available effort-stage answer is used, while other intermediate drafts stay outside history. |
| Recovery | Automatic by default with a persistent explicit opt-out; user-scoped DPAPI, 8 MiB bound, atomic replacement outside Git and one writer/data directory. A saved, locally identified in-flight turn with a usable partial answer becomes conversation context on restore. Older snapshots can recover an unbound trailing pair only when their saved completion state and last answer identify the interruption; ambiguous records remain visible separately. Restore never replays a request or activates microphone or playback. [Recovery operation](development-guide.md#optional-recovery-and-preview) defines saved fields, unreadable-file preservation, saving/deletion and same-user limits. |
| Window placement | Separate bounded geometry-only file in the data directory, independent of conversation saving. Preserve normal bounds and maximized state, never reopen minimized; native Windows restores offscreen placement onto current monitors. Preview leaves the real window settings untouched. Disk writes run on a coalescing worker joined at shutdown. |
| Appearance and layout | Device-wide System/Light/Dark theme, 14–24-point reading text and 640–1,040-point maximum reading width. Windows high contrast overrides the palette. The sidebar resizes from 216–340 points and can be hidden; below 900 points it becomes a left-anchored modal drawer with Escape/backdrop dismissal. A manual resize remains stable after release and layout changes; content cannot override it. Default window size is 1,080×780 points with a 480×480 minimum. A separate bounded `chat-appearance.json` stores only these preferences and sidebar geometry/visibility, independently of conversation saving; writes coalesce off the event loop and flush on close. Preview never opens this store. Text-size changes invalidate transcript, reasoning and Find geometry without changing source text or history. |
| Stop/retry | Standard and Extra effort generation have no elapsed-time cutoff. Stop freezes queued updates and wakes network and notification-pressure waits. Startup, connection and owned-teardown operations retain their deadlines. Retry waits for owned-backend teardown, creates fresh credentials/diagnostics and repeats artifact, identity and 15.5 GiB admission checks. Preserve completed turns and newer drafts; a usable partial answer and its original prompt become conversation context, otherwise restore the interrupted prompt only to an empty composer. Never replay automatically or reduce the profile. If completion wins the Stop race, the visible answer and committed history agree. |
| Rejections/failures | Recognized local input rejections restore the draft and retain readiness. Other disconnects require retry and identify no token/memory cause. [Broker policy](broker-policy.md#native-client-completion-and-recovery) defines accepted errors and completion. |
| Settings | A centered native modal blocks background pointer and keyboard actions. Response opens first and names the selected conversation; Voice, Appearance, Privacy and Application label their device-wide scope. Wide, sufficiently tall layouts use side navigation; narrow or short layouts use wrapping categories. Changes take effect immediately; Done, Close or Escape dismisses Settings and returns focus to the composer without sending or stopping generation. An open selector consumes Escape first; active input retains its explicit cancellation behavior. Voice setup and diagnostics stay inside the modal. |
| Response settings | Thinking, Extra effort, output cap and adapter scale belong to the selected conversation. They apply to the next response and every Extra effort pass; controls lock during generation. The composer's compact Thinking button opens its own native popup with only Thinking and Extra effort checkboxes and their explanations; these also appear in Settings. Reply length and adapter controls stay inside Settings behind the Advanced response controls disclosure. Popup actions retain ordinary button accessibility semantics; enabled thinking uses the native selection foreground/background pair. [Settings](development-guide.md#conversations-and-response-settings) defines defaults, bounds and execution; adapter and Extra effort quality gains remain unproven. |
| Context guidance | A native composer control estimates the selected conversation's next draft from committed content and retained reasoning, approximate framing and optional Extra effort planning overhead. Details separate estimated input from the exact reply reservation, 64-token reserve and 8,192-token limit. Estimates can be high or low and never gate Send, trim history or change settings automatically. Plain-language warnings offer Start fresh, preserving the original draft/history; details point to advanced Settings for reply limits. Exact broker admission is unchanged. |

Window geometry, appearance, sidebar geometry/visibility and voice are local
user-profile preferences, independent of the conversation-saving opt-out. The
bounded `chat-profile.dpapi` stores the voice description with Windows user
encryption, coalesces writes off the event loop and flushes on close. A missing
profile adopts the legacy recovered voice; an existing profile takes precedence.
Unreadable profile bytes stay intact until the user changes the voice preference.
Preview opens none of these stores, and restoring preferences never starts speech
or microphone capture. Response choices remain per conversation.

The model lifecycle has one explicit state: unavailable, preparing, ready,
generating, awaiting teardown, stopping, retryable or restarting. Send requires ready;
retry requires an owned-teardown notification and explicit user action before
restarting. The worker must acknowledge the new attempt before readiness can
open Send. Queued readiness/completion events cannot bypass those states,
and recognized rejections restore their prompt only to an empty composer.
Events come from one ordered worker channel, replaced when attaching a worker;
these phase guards do not add independent request or attempt identifiers.

Conversation browsing, creation, deletion and drafts remain available while the
model prepares or is unavailable. During generation, typing, browsing and creation
also remain available, while Send stays closed to a second request. Incoming text,
reasoning, stages and committed history belong to the conversation that submitted
the request, even when another conversation is visible. That conversation cannot
be deleted until generation settles. Background progress names its conversation
and offers Show response; Stop still controls the one active request.
History changes during preparation or generation are coalesced locally, and the
selected conversation's committed history is restored when the worker is ready,
before another Send and without replaying a prompt. Teardown and the brief
unacknowledged restart still block navigation until queued turn events settle.
A late rejected prompt returns only to its original conversation if it still exists.
Loading and failure status stays visible when switching conversations. Prepared
dictation can update a draft before Swift is ready, and switching cancels capture
and ignores late transcripts. Speech
controls follow their separate output capability; they cannot queue playback during
speech preparation. Once available, an explicit read action retains on-demand TTS
model loading and authenticated readiness checks. Switching stops playback; hidden
responses never restart streaming speech or finish another conversation's playback.
Stop speech remains available.

[Find controls](development-guide.md#conversations-and-response-settings) search
visible message bodies. The conversation has no permanent top bar or Find button.
The sidebar toggle sits beside Settings when docked, or in a compact left
rail when hidden or narrow. Settings stays in the sidebar footer. Find controls
appear above the transcript only while open, reached through Ctrl+F or a filtered
history result. Warnings, Retry and background-response notices remain conditional
above the transcript. Narrow composers put microphone and Send/Stop on their own
row below the audio controls. Ctrl+F is handled before the composer and sidebar text
editors, with dialogs and popups retaining priority. Selecting a filtered sidebar
result opens the same native Find controls with the trimmed query, matching-body
highlights and the first match selected; reselecting the current result does too.
Sidebar query changes also update the already-open conversation's Find and
highlights without taking sidebar focus, even when the filtered list is empty.
Clearing the sidebar query clears those highlights. Unchanged sidebar queries
preserve local Find edits, dismissal and match navigation.
Both searches use the same Unicode scalar lowercase convention. Find edits and
dismissal leave the sidebar filter intact; title-only results report no body
matches. [Streaming resources](streaming-resources.md#native-chat-and-transcript)
defines incremental matching, highlight/underline-cache invalidation, context-estimate
caching and work bounds. The compact Context: control uses a native painted circular
meter for estimated reserved occupancy, clamped to a full ring. Hover and accessible
description retain the approximate percentage; pending estimates show a hollow ring
with a dash, without a stale occupancy or idle animation. Its popup shares rounded
framing with Thinking. Both popups take keyboard priority: Escape closes them before
Find or Stop, and Enter cannot submit the background composer.
The generation owner's context remains updating until its turn settles; browsing
another conversation estimates that conversation's draft. Extra effort guidance
covers only its first pass, and message iteration can select a different prompt
or prefix. These estimates perform no network or disk work and establish neither
exact token occupancy nor request eligibility.
`bonsai-chat.exe --preview` uses the same presentation with marked sample content
and no broker, credentials, microphone or speech service; it checks layout and
interaction only.

Assistant headings, list/quote prefixes and inline or fenced code receive native
egui text styles. Markdown markers remain visible, code uses a monospace font,
and incomplete fences remain readable during streaming. Copy, selection and Find
continue to address the exact original text. This is a bounded presentation subset,
not a general Markdown or HTML renderer; additional style spans fall back to
ordinary text while retaining bounded search highlights.

Message cards use one compact header for the speaker, Copy, read-aloud and
iteration actions. Read-aloud remains keyboard accessible, including Stop during
playback, and reserves no separate footer while speech is unavailable. Cards
share 16-point horizontal and 12-point vertical insets with a 12-point gap;
thinking expands beneath the answer using its measured text height. Cached text
wrapping and Find geometry use the same content insets as presentation.

## Interoperability belongs at the API boundary

Use local LLM HTTP/JSON/SSE conventions: numeric loopback, bearer authorization,
`POST /v1/chat/completions`, text messages and `text/event-stream`. Preserve supplied
assistant reasoning separately as `reasoning_content`. [Broker policy](broker-policy.md#public-request-contract)
owns the supported subset, bounds, errors and native completion semantics.
Route names alone establish neither arbitrary-server nor OpenAI SDK compatibility;
clients emitting extra fields need an explicit adapter.

Protocol changes must preserve plain-text/streaming semantics and document
unsupported features. Keep backend policy outside presentation. Compatibility
cannot bypass ownership, admission, credentials or context limits.

## Speech input and output

Rust owns companion HTTP/authentication, schema validation, WAV processing and
verified model files. Reviewed native GGML C++ cores infer in separate executables
to avoid mixing GGML versions. The dependency-free `bonsai-audio` codec borrows
validated mono PCM16 WAV payloads and opens no devices; callers retain their own
sample-rate, duration and error policies.

Qwen3-ASR 1.7B uses CPU; TTS can register an optional local Hexagon `HTP0` backend with
CPU fallback. This is separate from Swift's resident `native-q2` acceleration.
The [TTS contract](../src/speech-tts/README.md) records backend selection and its
verification/qualification limits; containment and resource admission still apply.

Microphone access is requested on the UI thread only after explicit action.
Denied, cancelled or unavailable access fails closed; stale permission, capture
or transcription results cannot reactivate it. Audio stays on authenticated
numeric loopback. The composer microphone records one bounded utterance; finishing
capture transcribes with Qwen3-ASR 1.7B and submits the text through the ordinary
prompt path when Swift is ready. Otherwise the text remains a draft, with no
deferred automatic send. Cancel, conversation changes and teardown invalidate
late results. Speech output uses only visible assistant answers, excluding
reasoning/private drafts.

The composer's native plus button selects one audio file; Ctrl+V in the focused
composer accepts files copied from Explorer. A cancellable owned worker reads and
normalizes MP3/WAV and Windows-supported M4A/AAC/FLAC/WMA to in-memory mono 16 kHz
PCM16 WAV in sequential segments. File size and recording duration have no fixed
admission cap: available physical RAM and commit govern decoder buffers and
transcript growth. Each segment fits the recognizer's 30-second request contract;
backpressure keeps one decoded segment in flight. File input requires no
microphone permission. A dedicated composer notice shows progress, Cancel and
persistent actionable failures, independent of microphone cleanup notifications.
It enters the same authenticated Qwen3-ASR and
ordinary text-prompt path, preserving draft selection/cursor insertion, cancellation,
conversation
ownership and no deferred automatic send. Full and partial transcripts remain
available to view or copy until dismissed or replaced by another import; they are
session-only. A transcript too large for the chat composer stays complete in that
view, preserves the existing draft and sends no message. File decoding adds no public API route
or inference-model capability.

[Speech operation](development-guide.md#speech-setup-and-operation) defines
provisioning, capture lifecycle, fixed voice conditioning, mutually exclusive
talkers, budgets, service lifetime and external-service requirements (including
`consistent_voice: true` and complete-WAV fallback). The [ASR](../tools/canary-asr/README.md)
and [Qwen](../tools/qwen-tts/README.md) READMEs define companion protocols.
Service failure exposes setup guidance without substituting a recognizer or
Windows voice. Images, video and image generation remain unavailable in the
pinned Swift profile.

## Review and validation

Review native Rust presentation and protocol preservation through native builds,
focused UI/protocol checks and actual-model acceptance as appropriate. Full suites
and benchmarks stay CI-only under [validation](validation.md). Window launch and
synthetic work counts establish neither model quality, speech behavior, sustained
performance nor daily-service isolation. [PLAN.md](../PLAN.md) owns pending qualification.
