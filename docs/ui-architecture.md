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

The dark UI runs separately from the broker/inference worker. Network, disk,
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

Swift launches default to native Rust Prism 0.1.23, original PQ2, four CPU row
workers and the standard adapter at scale 1.3. `--native-engine` remains accepted;
`--publisher-engine` selects the explicit publisher rollback. Engine options are
mutually exclusive, and retry preserves the selected engine and representation.
The UI identifies this as a development default with full-context/quality
qualification pending. Selection neither waives admission nor establishes full
release qualification. Supported X2 installations also select the experimental
resident `native-q2` path under the [Hexagon policy](hexagon.md); CPU fallback for
an absent signed package or CDSP driver does not change engine or representation.
PTQ1 remains an explicit opt-in.

The launcher checks/builds presentation first. After the first UI tick, an owned
preparation worker verifies/provisions the selected pinned Rust runtime,
builds/verifies the broker and speech companions and creates fresh credentials.
Runtime preparation downloads only a missing locked runtime; model and adapter
provisioning remain explicit. Drafts and recovered conversations survive preparation;
failures leave the window available. Closing cancels preparation and waits for
owned descendants. Artifact verification and memory admission precede model use.
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

Only an MSI-installed application discovers published GitHub releases. A bounded
background worker checks after startup and every six hours; the native Settings
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
| Workspace | At most 16 conversations, each with its own draft, response settings, visible transcript and exact committed history. Transcript count/byte eviction (at most 512 messages) never changes model history. |
| Presentation | Only user/assistant content enters the selectable plain-text transcript; never interpret output as HTML. Setup/retry/transport/speech status use separate controls and bounded collapsible diagnostics. Rename/delete use centered native modals; deletion names the target and blocks background input. Cancel/Escape preserves the conversation/draft. |
| Recovery | Automatic by default with a persistent explicit opt-out; user-scoped DPAPI, 8 MiB bound, atomic replacement outside Git and one writer/data directory. Restore completed history without request replay, microphone or playback; keep interrupted visible text separate. [Recovery operation](development-guide.md#optional-recovery-and-preview) defines saved fields, unreadable-file preservation, saving/deletion and same-user limits. |
| Window placement | Separate bounded geometry-only file in the data directory, independent of conversation saving. Preserve normal bounds and maximized state, never reopen minimized; native Windows restores offscreen placement onto current monitors. Preview leaves the real window settings untouched. Disk writes run on a coalescing worker joined at shutdown. |
| Stop/retry | Stop freezes queued updates and wakes network waits. Retry waits for owned-backend teardown, creates fresh credentials/diagnostics and repeats artifact, identity and 15.5 GiB admission checks. Preserve completed turns and newer drafts; restore an interrupted prompt only to an empty composer. Never replay automatically or reduce the profile. If completion wins the Stop race, the visible answer and committed history agree. |
| Rejections/failures | Recognized local input rejections restore the draft and retain readiness. Other disconnects require retry and identify no token/memory cause. [Broker policy](broker-policy.md#native-client-completion-and-recovery) defines accepted errors and completion. |
| Response settings | Thinking, output cap and adapter scale apply to the next response and every Extra effort pass; lock controls during generation. [Settings](development-guide.md#conversations-and-response-settings) defines defaults, bounds and execution; Extra effort quality gains remain unproven. |

The model lifecycle has one explicit state: unavailable, preparing, ready,
generating, awaiting teardown, stopping, retryable or restarting. Send requires ready;
retry requires an owned-teardown notification and explicit user action before
restarting. The worker must acknowledge the new attempt before readiness can
open Send. Queued readiness/completion events cannot bypass those states,
and recognized rejections restore their prompt only to an empty composer.
Events come from one ordered worker channel, replaced when attaching a worker;
these phase guards do not add independent request or attempt identifiers.

[Find controls](development-guide.md#conversations-and-response-settings) search
visible message bodies. [Streaming resources](streaming-resources.md#native-chat-and-transcript)
defines incremental matching, highlight/underline-cache invalidation and work bounds.
`bonsai-chat.exe --preview` uses the same presentation with marked sample content
and no broker, credentials, microphone or speech service; it checks layout and
interaction only.

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

Canary uses CPU; TTS can register an optional local Hexagon `HTP0` backend with
CPU fallback. This is separate from Swift's resident `native-q2` acceleration.
The [TTS contract](../src/speech-tts/README.md) records backend selection and its
verification/qualification limits; containment and resource admission still apply.

Microphone access is requested on the UI thread only after explicit action.
Denied, cancelled or unavailable access fails closed; stale permission, capture
or transcription results cannot reactivate it. Audio stays on authenticated
numeric loopback. Transcripts require review before send; speech output uses only
visible assistant answers, excluding reasoning/private drafts.

[Speech operation](development-guide.md#speech-setup-and-operation) defines
provisioning, capture lifecycle, fixed voice conditioning, mutually exclusive
talkers, budgets, service lifetime and external-service requirements (including
`consistent_voice: true` and complete-WAV fallback). The [Canary](../tools/canary-asr/README.md)
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
