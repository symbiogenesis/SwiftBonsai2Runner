# Windows releases

Swift Bonsai 2 Runner ships as a **single portable Windows ARM64 executable**
for Snapdragon X and X2 laptops. The prepared experimental release is **v0.2.0**;
its [release notes](releases/v0.2.0.md) describe the development default and
remaining acceptance limits. Publication requires the owner's source push and
Windows Release workflow run; local changes alone do not publish a release.

## Download and open

After publication, obtain the executable from the project's
[GitHub releases](https://github.com/symbiogenesis/SwiftBonsai2Runner/releases).
The complete release has three assets:

| Asset | Purpose |
| --- | --- |
| `SwiftBonsai2Runner-0.2.0-windows-arm64.exe` | The runnable application, containing all five application executables and their notices. |
| `SHA256SUMS.txt` | Checksums for the executable and release manifest. |
| `release-manifest.json` | Version, source revision, build identities and executable digest. |

Check the executable's SHA-256 against the checksum from the same release before
opening it:

```powershell
Get-FileHash .\SwiftBonsai2Runner-0.2.0-windows-arm64.exe -Algorithm SHA256
.\SwiftBonsai2Runner-0.2.0-windows-arm64.exe
```

**Deploy only the `.exe`.** The verification files and any local build-summary
JSON files are not needed beside it at runtime. Package metadata and project and
dependency notices are embedded. Windows Release publishes no MSI or separate
ZIP package and invokes no installer tool or installer-EULA acceptance.

The native Rust GUI launcher opens chat without a terminal, source checkout or
compiler. It verifies its compressed inventory and extracts a checked package
under `%LOCALAPPDATA%\SwiftBonsai2\application-bundles\<first-24-manifest-sha256-characters>\`.
UI, broker and speech still run as separate owned processes with the existing
containment and shutdown rules. Retained cache files are checked before use.

The experimental executable is unsigned. Published checksums detect changed
bytes within the GitHub release trust boundary; they do not establish an
independent publisher signature. Review the release source identity before use.

## Prerequisites and model setup

Use serviced Windows 11 ARM64 with the containment capabilities described in
[Windows sandboxing](sandboxing.md), and leave **15.5 GiB RAM available** before
model startup. Unsupported containment or unavailable resources stop startup.
Install the compatible ARM64 Microsoft Visual C++ v14 runtime from
[Microsoft's redistributable downloads](https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist).
It is a separate prerequisite for the native speech components; the bundle does
not install that redistributable.

The executable contains application code and notices. Swift weights (about
**6.71 GiB**), the adapter, inference runtime and speech models remain separate
locked downloads with their own [license terms](licensing.md). Opening the bundle
does not waive artifact checks or memory admission. Missing runtime preparation
can download the pinned inference runtime; model and adapter provisioning remain
explicit. Speech preparation can download the selected speech models, while
microphone capture requires explicit consent.

To provision Swift without a checkout, retain the verified `release-manifest.json`
for this setup command and download the model lock from its exact source revision.
Use the same data directory for setup and launch:

```powershell
$tool = (Resolve-Path .\SwiftBonsai2Runner-0.2.0-windows-arm64.exe).Path
$release = Get-Content .\release-manifest.json -Raw | ConvertFrom-Json
$data = Join-Path $env:LOCALAPPDATA 'SwiftBonsai2'
New-Item -ItemType Directory -Path $data -Force | Out-Null
$modelLock = Join-Path $data 'swift-assessment.lock.json'
Invoke-WebRequest "https://raw.githubusercontent.com/symbiogenesis/SwiftBonsai2Runner/$($release.sourceRevision)/config/swift-assessment.lock.json" -OutFile $modelLock
& $tool provision --json (@{ operation = 'inference'; dataDirectory = $data } | ConvertTo-Json -Compress)
& $tool provision --json (@{ operation = 'model'; dataDirectory = $data; lockPath = $modelLock } | ConvertTo-Json -Compress)
& $tool provision --json (@{ operation = 'adapter'; dataDirectory = $data } | ConvertTo-Json -Compress)
& $tool bundled-chat --data-directory $data
```

The explicit `swift-assessment.lock.json` selects Swift weights; omitting it
selects the separate reference model. Review the release manifest and source
identity with the published checksum inventory before using this command.

For text-only launch without speech-model preparation, use:

```powershell
& $tool bundled-chat --data-directory $data --no-speech-models
```

The Rust/PQ2 development default, explicit publisher rollback and opt-in PTQ1
representation retain their existing verification rules. A preparation failure
never silently changes the selected engine or representation. See the
[development guide](development-guide.md#native-chat-app) for model setup and the
[package contract](chat-packages.md#launch-without-a-checkout) for launch options.

## Update and remove

Portable releases check GitHub at startup and every six hours, including published
experimental prereleases, and automatically download a newer Windows ARM64
executable. Settings > Application also provides a manual check. Discovery chooses
the highest numeric version across the bounded release catalog; drafts and older
versions are excluded. An incomplete catalog or invalid newest release is an error,
never a reason to fall back to an older download.

Before replacement, the updater verifies the GitHub asset digest, release manifest,
checksum inventory and tag/source revision. Choose **Install update and close**,
then confirm **Close and install**. Swift saves according to your existing setting,
stops its owned processes, revalidates publication and starts the verified native
replacement helper. The helper replaces the same executable atomically and reopens
it with the original launch options and data directory. No MSI is used. Models,
runtime downloads, saved conversations and preferences remain separate. A failed
download or replacement retains the existing executable; a failed restart leaves
the updated executable available to open manually. A read-only executable location
requires manual replacement.

Manual updates remain available: download the newer executable from the project's
releases, verify its checksum, close Swift and replace the old `.exe`. Keep the same
data directory when reopening it. Source and preview launches perform no automatic
update work.

To remove the app, close it and delete its executable. Its cached application
package can also be removed to reclaim space; launching the bundle again recreates
that cache. User model and conversation data remain until explicitly removed.
The separate [checked-directory package mechanism](chat-packages.md) retains its
manual activation and rollback controls.

For local builds of the same format, use the
[single-executable build command](development-guide.md#build-a-single-executable).

## Publish a release

Release scripts require PowerShell 7.6 or newer. The workflow checks syntax,
argument preservation, UTF-8 output, native failures and unsafe paths before its
build jobs. It accepts serviced newer PowerShell versions.

`AGENTS.md` reserves every Git push and routine release operation for the owner.
After reviewing the local commits on `main`, the owner can publish the source and
request the release:

```powershell
git push origin main
gh workflow run release.yml --repo symbiogenesis/SwiftBonsai2Runner --ref main -f prerelease=true
```

Leave `tag` blank to derive it from `workspace.package.version` in `Cargo.toml`.
An explicit numeric version (`0.2.0`) or tag (`v0.2.0`) must match; both resolve
to canonical `v0.2.0`, with notes at
`docs/releases/v0.2.0.md`. A mismatch reports the requested tag, workspace version
and expected tag. Increase the workspace version and commit matching notes before
requesting the next release. `prerelease` defaults to `true`.

Windows Release builds and checks the five native application lanes on hosted
Windows ARM64, verifies their source-bound build receipts and complete notices,
then assembles the GUI-subsystem, deflated executable. Its model-free smoke check
verifies the GUI header, extraction and all executable identities. There is no
installer authoring, prerequisite installation or install/uninstall step.

The notice collector conservatively includes resolved Windows build/development
dependencies. Missing crate-archive notices are bound to their recorded source
commit; the reviewed egui 0.37.0 exception records both its unavailable archive
commit and the immutable release commit supplying the shared license material.
Missing or changed material stops publication. The complete notices remain
inside the executable.

Only the publication job receives repository-content write permission. It
verifies the exact three-asset inventory, uploads it to a draft release and
publishes only after remote asset and source checks pass. The release creates
its tag at the checked commit; no workflow step runs `git push`, and an existing
release is never silently replaced.

The owner can inspect the run and release with:

```powershell
gh run list --repo symbiogenesis/SwiftBonsai2Runner --workflow release.yml --limit 5
gh release view v0.2.0 --repo symbiogenesis/SwiftBonsai2Runner
```

Hosted packaging checks do not qualify clean-machine startup, actual-model
behavior, quality, sustained performance or isolation. Those remaining gates
stay in [PLAN.md](../PLAN.md).
