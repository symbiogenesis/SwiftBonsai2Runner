# Portable native chat packages

This contributor guide covers checked-directory assembly, installation and
rollback. To use the app, follow [Windows releases](releases.md): deploy only
the portable Windows ARM64 `.exe` and open it. Its setup runs inside the window.

Checked-directory packages use explicit install/rollback commands, without
automatic discovery or running-app replacement. Current-source single-executable
bundles instead use [portable updates](releases.md#update-and-remove), including
confirmed replacement/restart and deferred installation on close. Always preserve
the same user data directory.

`bonsai-tools chat-package` assembles reviewed Windows ARM64 files, installs
versioned directories, activates updates atomically and retains a rollback choice.
Install and launch with a trusted maintenance executable; no checkout, PowerShell
or compiler is needed. The native preparation worker downloads and verifies
models, adapters and the selected runtime after the window opens.

Packages provide no signed installer or publisher authentication. Verify the
manifest digest through a trusted channel and use an independently trusted
`bonsai-tools.exe` for installation and launch. SHA-256 detects changed bytes;
an unverified package cannot authenticate itself. Same-user replacement is outside
this boundary. Distribution and actual-model acceptance remain in
[PLAN.md](../PLAN.md#application-and-daily-service).

## Native command

Write one of the following JSON requests to an expanded absolute path outside
Git, then invoke the trusted executable directly:

```text
bonsai-tools.exe chat-package --request C:\BonsaiRequests\package.json
```

Success emits a JSON receipt on stdout; failure emits stderr and a nonzero exit
code. Unknown fields are rejected. Use expanded absolute paths without links,
device namespaces or alternate data streams. Keep outputs, artifacts and
installation storage outside Git.

## Assemble a package

Build and review the five executables from the intended settled `main` revision
using the pinned compiler and native speech build procedure. Creation does not
compile, sign, download or execute these inputs. Version and source revision are
supplied declarations, not proof of build provenance.

```json
{
  "operation": "create",
  "outputDirectory": "C:\\BonsaiPackages\\0.3.0",
  "version": "0.3.0",
  "sourceRevision": "0000000000000000000000000000000000000000",
  "chatPath": "C:\\ReviewedBuild\\bonsai-chat.exe",
  "brokerPath": "C:\\ReviewedBuild\\bonsai-broker.exe",
  "asrPath": "C:\\ReviewedBuild\\bonsai-asr.exe",
  "ttsPath": "C:\\ReviewedBuild\\bonsai-tts.exe",
  "toolPath": "C:\\ReviewedBuild\\bonsai-tools.exe",
  "thirdPartyNoticePath": "C:\\ReviewedBuild\\THIRD-PARTY-NOTICES.txt"
}
```

Replace the example revision with the reviewed build identity. Creation includes
the trusted tool's embedded `LICENSE`/`NOTICE` and supplied third-party notices.
Review them against exact shipped dependencies under the
[redistribution requirements](licensing.md#redistribution); the tool cannot certify
completeness. Models, adapters and runtimes remain separate artifacts with their own terms.

Schema-2 packages contain exactly five ARM64 executables under `bin/`, three
notice files and bounded `package.json` metadata with each file's size and
SHA-256. No scripts or install hooks are included. New staging is checked before
publication; an existing output is never replaced. Distribute the directory
intact and retain the returned `ManifestSha256` independently. `inspect` accepts
`packageDirectory` and `manifestSha256` for the same complete offline validation.

## Bundle a package into one executable

A verified schema-2 package can become one `.exe`, embedding all five executables,
metadata and notices while preserving separate contained workers. Models, adapters
and runtimes remain separate downloads; computation settings are unchanged.

ZIP deflate compresses new payloads. Inspection/extraction also accept retained
stored entries, with the same fixed inventory, expanded-byte limits and SHA-256
checks. Use GUI-subsystem `bonsai-launcher.exe` as `toolPath`, stored as
`bin/bonsai-tools.exe`, to launch without a terminal. The console build remains
available for source tooling.

The Windows launcher embeds the two-leaf application icon at build time. Bundling
preserves those launcher bytes, so the portable executable carries the same icon
without modifying its verified prefix after packaging.

```powershell
./scripts/release/Build-WindowsExecutable.ps1 -PackageDirectory C:\BonsaiPackages\0.3.0 -ManifestSha256 REPLACE_WITH_THE_REVIEWED_64_HEX_DIGEST -OutputPath C:\BonsaiPackages\SwiftBonsai2Runner.exe
```

The helper invokes the trusted package's maintenance binary with this request:

```json
{
  "operation": "create",
  "packageDirectory": "C:\\BonsaiPackages\\0.3.0",
  "manifestSha256": "REPLACE_WITH_THE_REVIEWED_64_HEX_DIGEST",
  "outputPath": "C:\\BonsaiPackages\\SwiftBonsai2Runner.exe"
}
```

Use `chat-bundle --request` or `chat-bundle --json` for these operations. Creation
rejects changed package bytes and existing output files. `inspect` accepts
`executablePath`; `extract` accepts that path and `installationDirectory` and
verifies the package before publication. Neither operation starts chat or loads
a model. Review the outer executable's checksum before executing a received
bundle; its internal checks do not independently authenticate its publisher.

Open the generated executable to extract and launch its verified package through
the native package launcher. The retained cache is under
`%LOCALAPPDATA%\SwiftBonsai2\application-bundles\<first-24-manifest-sha256-characters>\` and is
verified on reuse. `bundled-chat` accepts the installed launch options,
including `--data-directory` and `--no-speech-models`, while preserving its own
package location. Bundled launches automatically check and download verified
published releases. Immediate replacement restarts the app; a deferred verified
update installs on close without reopening it. Source and preview launches perform
no update discovery. The local
[single-executable build](development-guide.md#build-a-single-executable) builds
the five components and assembles this same distribution format.

## Install, update and roll back

```json
{
  "operation": "install",
  "installationDirectory": "C:\\BonsaiChat",
  "packageDirectory": "C:\\BonsaiPackages\\0.3.0",
  "manifestSha256": "REPLACE_WITH_THE_REVIEWED_64_HEX_DIGEST"
}
```

Activation validates inventory, architecture, sizes, hashes and external paths.
A single-writer lock serializes operations: copy to private staging, verify again,
publish a version directory and atomically replace `active.json`. Failure preserves
the current selection; existing versions are retained. Uppercase digests are
canonicalized before deriving directory identity.

Install the next reviewed package with the same operation and its new digest.
Rollback verifies the retained previous package before switching:

```json
{"operation":"rollback","installationDirectory":"C:\\BonsaiChat"}
```

Rollback retains the displaced version for an explicit switch back; unchanged
installation preserves the previous choice. Installation/rollback never restarts
a running app or changes model/recovery data.

## Launch without a checkout

Run the same trusted maintenance executable with this request. It resolves the
current active version on every launch; retain the trusted tool separately from
unverified incoming distributions.

```json
{
  "operation": "start",
  "installationDirectory": "C:\\BonsaiChat",
  "options": {"DataDirectory":"C:\\BonsaiData"}
}
```

`start` verifies the active manifest/inventory, derives fixed executable paths and
launches the app. Options cannot override paths or request builds. `launch` accepts
an explicitly bound `expectedDirectory`, `expectedId` and `expectedManifestSha256`.
Models/runtime/adapter/speech preparation runs inside the window.

The adapter is enabled by default; opt out explicitly with `NoAbliterateLora: true`.
`NoSpeechModels: true` skips automatic speech preparation; `NoTts: true` skips
output speech. `NoAbliterateLora`,
`UseRepackedPtq1`, `NativeEngine`, `NativeRowWorkers`, `PromptReuse`, `KvCache`,
`GraphicsBackend`, `TtsQuantization`, `TtsThreads`, `FrontendPort`, `BackendPort`,
`CanaryPort` and `TtsPort` remain explicit settings with the native launcher's
existing validation. Native Rust owns the per-user launch guard, artifact/speech
preparation, credentials, Jobs and retry policy. Progress, Pause setup and Retry
setup are available in the window; speech retry stays separate. No microphone
activates at installation or launch.

[Runtime selection](development-guide.md#runtime-selection) owns engine/worker,
cache and representation defaults. `NativeEngine: false` selects publisher;
`UseRepackedPtq1: false` explicitly selects original PQ2 through deferred startup.
Failed PTQ1 never falls back automatically.

## Validation

Foundation's `bonsai-tools` suite checks creation, installation, updates,
bidirectional rollback, legacy compatibility, write contention, tampering,
inventory, metadata and launch handoff.
Inert ARM64 headers and injected handoffs establish package behavior only;
actual-model, speech, signing and daily-service acceptance remain separate.
Complete suites are CI-only.
