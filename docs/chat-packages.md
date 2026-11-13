# Portable native chat packages

For the single-executable and Windows ARM64 MSI distributions and their GitHub
publication workflow, see
[Windows releases](releases.md). The checked-directory mechanism below remains
a separate installation and rollback option.

Automatic GitHub release checks and MSI installation handoff apply only to the
MSI-installed application. They do not replace this portable package's explicit
manifest review, activation or retained-version rollback operations.

The native `bonsai-tools chat-package` command assembles reviewed Windows ARM64
application files, installs versioned directories, activates updates atomically
and retains an explicit rollback choice. Installation and launch need a trusted
maintenance executable and the ordinary model/runtime installation. Neither a
source checkout, PowerShell nor a Rust/C++ compiler is required. Native speech
also requires the compatible ARM64 Microsoft Visual C++ runtime used by its
dynamic MSVC build; this package does not install runtime DLLs.

This checked-directory distribution path does not provide a signed installer,
publisher authentication or automatic update discovery. SHA-256 detects changed
bytes; review the manifest digest through a trusted channel before installation.
Use an independently trusted `bonsai-tools.exe` for installation and subsequent
launches. Executing an unverified received package's maintenance binary to verify
itself does not establish trust. Same-user replacement remains outside this
boundary. Remaining distribution and actual-model acceptance gates live in
[PLAN.md](../PLAN.md#application-and-daily-service).

## Native command

Write one of the following JSON requests to an expanded absolute path outside
Git, then invoke the trusted executable directly:

```text
bonsai-tools.exe chat-package --request C:\BonsaiRequests\package.json
```

Success returns a JSON receipt on stdout; errors return a nonzero exit code and
diagnostic on stderr. Requests reject unknown fields. All filesystem paths must
be expanded absolute paths without links, device namespaces or alternate data
streams. Package output, artifacts and installation storage remain outside Git.

## Assemble a package

Build and review the five executables from the intended settled `main` revision
using the pinned compiler and native speech build procedure. Creation does not
compile, sign, download or execute these inputs. Version and source revision are
supplied declarations, not proof of build provenance.

```json
{
  "operation": "create",
  "outputDirectory": "C:\\BonsaiPackages\\0.1.0",
  "version": "0.1.0",
  "sourceRevision": "0000000000000000000000000000000000000000",
  "chatPath": "C:\\ReviewedBuild\\bonsai-chat.exe",
  "brokerPath": "C:\\ReviewedBuild\\bonsai-broker.exe",
  "asrPath": "C:\\ReviewedBuild\\bonsai-asr.exe",
  "ttsPath": "C:\\ReviewedBuild\\bonsai-tts.exe",
  "toolPath": "C:\\ReviewedBuild\\bonsai-tools.exe",
  "thirdPartyNoticePath": "C:\\ReviewedBuild\\THIRD-PARTY-NOTICES.txt"
}
```

Replace the example revision with the reviewed build identity. The command
includes project `LICENSE` and `NOTICE` embedded in the trusted tool's source
build, plus the supplied third-party notice text. Review these against the exact
shipped dependencies under [redistribution requirements](licensing.md#redistribution).
The tool does not certify notice completeness. Models, adapters and inference
runtime files remain separately provisioned artifacts with their own terms.

Schema-2 packages contain exactly five ARM64 executables under `bin/`, three
notice files and bounded `package.json` metadata with each file's size and
SHA-256. No scripts or install hooks are included. New staging is checked before
publication; an existing output is never replaced. Distribute the directory
intact and retain the returned `ManifestSha256` independently. `inspect` accepts
`packageDirectory` and `manifestSha256` for the same complete offline validation.

## Bundle a package into one executable

The same verified schema-2 package can become one distributable `.exe`. This
embeds the five application executables, package metadata and notices in the
native maintenance launcher; it preserves the separate worker processes and
their containment. Models, adapters and inference runtime downloads stay separate.

```powershell
./scripts/release/Build-WindowsExecutable.ps1 -PackageDirectory C:\BonsaiPackages\0.1.0 -ManifestSha256 REPLACE_WITH_THE_REVIEWED_64_HEX_DIGEST -OutputPath C:\BonsaiPackages\SwiftBonsai2Runner.exe
```

The helper invokes the trusted package's maintenance binary with this request:

```json
{
  "operation": "create",
  "packageDirectory": "C:\\BonsaiPackages\\0.1.0",
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
package location. Bundled launches do not enable MSI update discovery. The local
[single-executable build](development-guide.md#build-a-single-executable) builds
the five components and assembles this same distribution format.

## Install, update and roll back

```json
{
  "operation": "install",
  "installationDirectory": "C:\\BonsaiChat",
  "packageDirectory": "C:\\BonsaiPackages\\0.1.0",
  "manifestSha256": "REPLACE_WITH_THE_REVIEWED_64_HEX_DIGEST"
}
```

The installer validates the fixed inventory, native architecture, sizes, hashes
and external paths before selecting a version. A single-writer lock serializes
operations. Rust copies into private staging, verifies again, publishes a new
version directory and atomically replaces `active.json`. Failed verification
preserves the current selection. Existing versions are retained. Digests supplied
in uppercase are canonicalized before deriving the version directory identity.

Install the next reviewed package with the same operation and its new digest.
Rollback verifies the retained previous package before switching:

```json
{"operation":"rollback","installationDirectory":"C:\\BonsaiChat"}
```

Rollback retains the displaced version, allowing an explicit switch back. An
unchanged install preserves the previous choice. Schema-1 packages retain their
original fixed script inventory and remain eligible for checked installation,
inspection and rollback; new packages use schema 2. Updating does not restart a
running app. Installation and rollback leave model and recovery data alone.

## Launch without a checkout

Run the same trusted maintenance executable with this request. It resolves the
current active version on every launch; retain the trusted tool separately from
unverified incoming distributions.

```json
{
  "operation": "start",
  "installationDirectory": "C:\\BonsaiChat",
  "options": {"DataDirectory":"C:\\BonsaiData","NoSpeechModels":true}
}
```

One native operation checks the selected manifest and complete fixed inventory,
then derives executable paths and starts the same native application. Options
cannot supply executable paths or request source builds. The legacy `launch`
operation also remains available for callers that bind `expectedDirectory`,
`expectedId` and `expectedManifestSha256` before handoff.

Provision Swift model/runtime and the optional adapter first using the
[setup commands](development-guide.md#native-chat-app). `NoSpeechModels: true`
keeps launch text-only; `NoTts: true` skips output speech. `NoAbliterateLora`,
`UseRepackedPtq1`, `NativeEngine`, `NativeRowWorkers`, `TtsQuantization`,
`TtsThreads`, `FrontendPort`, `BackendPort`, `CanaryPort` and `TtsPort` remain
explicit settings with the native launcher's existing validation. Native Rust
owns the per-user launch guard, model provisioning, credentials, Jobs and retry
policy. No microphone activates at installation or launch.

For current launcher builds, omitted `NativeEngine` selects the Rust development
default with four row workers. `NativeEngine: false` explicitly selects publisher
rollback and defaults to one worker; `NativeEngine: true` explicitly selects Rust.
Supply `NativeRowWorkers: 1` to select one Rust worker. Older retained packages
keep their original executable behavior; rebuild and install a matching package
to receive the new default. A package manifest alone does not change that behavior.

Set `UseRepackedPtq1: false` to select original PQ2 explicitly. The value is
preserved through deferred startup and forwarded as `--use-original-pq2` to the
native app and broker. No failed PTQ1 launch falls back automatically.

## Validation

Foundation's `bonsai-tools` Rust suite owns package creation, installation,
updates, bidirectional rollback, legacy-package compatibility, write contention,
tampering, fixed inventory, metadata and launch handoff checks. The former
PowerShell package tests and wrappers are deleted. Inert ARM64 fixture headers
and injected launch handoffs establish package behavior, not actual-model,
speech, signing or daily-service acceptance. Complete suites remain CI-only.
