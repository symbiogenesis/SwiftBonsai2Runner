# Windows releases

Swift Bonsai 2 Runner is a **Windows ARM64 application** for Snapdragon X and X2
laptops. Its installer uses the Windows Installer `.msi` format. The release
workflow builds and checks native ARM64 executables on GitHub-hosted Windows.

The first prepared release is the experimental **v0.1.0 prerelease**. Its
[release notes](releases/v0.1.0.md) describe the available application and remaining
acceptance limits. Publishing the prepared pipeline and its first installer is
pending the owner's source push and release workflow run; local source changes
alone do not create a downloadable release.

## Install and open

After publication, obtain the installer from the project's
[GitHub releases](https://github.com/symbiogenesis/SwiftBonsai2Runner/releases).
Choose `SwiftBonsai2Runner-0.1.0-windows-arm64.msi`, with `SHA256SUMS.txt` and
`release-manifest.json` from the same release. Check the installer's SHA-256
against the published checksum before opening it:

```powershell
Get-FileHash .\SwiftBonsai2Runner-0.1.0-windows-arm64.msi -Algorithm SHA256
```

The Microsoft installer format does not imply Microsoft publisher signing.
The first installer is unsigned; its checksum detects changed bytes but does
not establish independent publisher trust. Review the repository and release
source identity before installing.

Install the latest supported ARM64 Microsoft Visual C++ v14 runtime from
[Microsoft's redistributable downloads](https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist),
then run the MSI. Installation is per machine, requires administrator approval
and defaults to `%ProgramFiles%\Swift Bonsai 2 Runner`. After provisioning the
model below, open **Swift Bonsai 2 Runner** from the Start menu. The installed
application needs no source checkout or Rust/C++ compiler. The native Rust
launcher verifies the packaged executable inventory before starting chat.

Use serviced Windows 11 ARM64 with the containment capabilities described in
[Windows sandboxing](sandboxing.md), and leave **15.5 GiB RAM available** for the
model's admission check. Unsupported containment or unavailable resources stop
startup rather than weakening isolation or selecting another model.

## Models and speech

The MSI contains application executables and license notices. It does not
contain the 6.71 GiB Swift model, inference runtime, adapter or speech models,
and installation does not start a download or load a model. Provision these
artifacts explicitly before the first usable conversation. The native runtime
preparation can download the pinned inference runtime when it is missing.

Model, adapter and runtime identities remain locked independently of the MSI;
their own [license terms](licensing.md) apply. The Rust/PQ2 development default,
explicit publisher rollback and opt-in PTQ1 representation retain their existing
verification and admission rules. A failed preparation never silently changes
engine or representation.

To provision the installed application, open PowerShell as your ordinary user
and run the following. Adjust `$install` if you changed the MSI location; use
the same `$data` directory for provisioning and launch:

```powershell
$install = Join-Path $env:ProgramFiles 'Swift Bonsai 2 Runner'
$tool = Join-Path $install 'maintenance/bonsai-tools.exe'
$data = Join-Path $env:LOCALAPPDATA 'SwiftBonsai2'
& $tool provision --json (@{ operation = 'inference'; dataDirectory = $data } | ConvertTo-Json -Compress)
& $tool provision --json (@{ operation = 'model'; dataDirectory = $data; lockPath = (Join-Path $install 'config/swift-assessment.lock.json') } | ConvertTo-Json -Compress)
& $tool provision --json (@{ operation = 'adapter'; dataDirectory = $data } | ConvertTo-Json -Compress)
& $tool installed-chat --data-directory $data
```

The explicit `swift-assessment.lock.json` selects Swift weights; omitting that
model lock would select the separate reference model. The Start menu shortcut
uses `%LOCALAPPDATA%\SwiftBonsai2` by default. Running it before provisioning
leaves model setup incomplete and reports the missing setup.

For a text-only launch that skips speech model downloads, use:

```powershell
& $tool installed-chat --data-directory $data --no-speech-models
```

Native speech also needs the compatible ARM64 Microsoft Visual C++ runtime for
the shipped dynamic MSVC speech components. It is a separate prerequisite; the
MSI requires it even for text-only use and does not bundle or install that
redistributable. Speech model downloads
happen during launcher preparation, separately from MSI installation; microphone
access requires explicit consent. Text chat can use the package launch option
`NoSpeechModels: true`; see
[installed launch options](chat-packages.md#launch-without-a-checkout).

## Upgrade and remove

Close the application before installing a newer MSI. Windows Installer owns
the application files, Start menu shortcut and uninstall registration. Use
**Settings > Apps > Installed apps** to remove the application. Models, runtime
downloads, saved conversations and window settings live in the user's data
directory and are preserved on upgrade and uninstall.

MSI upgrades follow Windows Installer's version and downgrade rules. The
[portable package mechanism](chat-packages.md) separately supports retained
version directories and explicit rollback; those portable activation rules are
not the MSI update mechanism.

## Automatic update checks

MSI-installed chat checks the project's published GitHub releases in the
background at startup and every six hours while running. **Settings > Application
updates** also provides **Check for updates**. A failed check or unavailable
network leaves the installed application usable. Source launches and model-free
preview do not check for or install application updates.

The update feed is the existing GitHub release contract: a published Windows
ARM64 MSI, `release-manifest.json` and `SHA256SUMS.txt` from the same release.
The updater considers published prereleases and full releases within its
bounded release listing and compares numeric application versions. It never
installs an equal or older version. A repository with no published releases
simply has no update available; a first release is not required to add or run
this code.

Update discovery uses GitHub's release listing because its
[latest-release endpoint](https://docs.github.com/en/rest/releases/releases#get-the-latest-release)
excludes prereleases. Checks contact GitHub without sending conversation text,
model files or a personal GitHub token. Downloads must match the expected
repository, release version, Windows ARM64 target, MSI filename, size and SHA-256
identities before installation is offered. The application does not run an
executable supplied by a release to verify that release.

Choose **Download update**, then **Install update and close** when ready.
Installation requires confirmation; finish the current response first. The
normal shutdown path must confirm teardown of owned model and speech work and
finish local persistence before Windows Installer replaces application files.
The maintenance launcher waits for the native window to exit, checks the published
release again and compares its identity with the staged receipt and actual MSI
bytes. This final check also needs a working network connection. A changed or
withdrawn release, changed cache, or failed final check stops installation; reopen
the application to check and confirm again. The launcher retains read handles to
the verified MSI and its ancestor directories through Windows Installer process
creation, and consumes the queued request before starting that process.
Windows may request administrator approval
for the per-machine MSI. Conversation saving continues to honor its existing
setting; choosing an update does not enable saving. Model downloads and existing
user data remain outside the application installation directory.

The first release remains unsigned. HTTPS and release SHA-256 checks detect
changed or mismatched bytes within the GitHub release trust boundary; they are
not an independent publisher signature. If verification, shutdown or installer
handoff fails, the updater reports the failure rather than weakening checks.
The launcher exits after process creation so its application files can be replaced;
its file guards do not cover the full installer lifetime or isolate a compromised
process running as the same user.
After installation, reopen the application from the Start menu.

In-app installation stays unavailable during model preparation, an active
response or uncertain owned teardown. Checking and downloading remain available
if model setup fails. After closing Swift, the downloaded installer is under
`%LOCALAPPDATA%\SwiftBonsai2\updates\<version>\`; verify it against that release's
published checksum before opening it manually. Resolve uncertain model cleanup
before replacing application files.

## Publish a release

`AGENTS.md` reserves every Git push for the owner. Review the local commits on
`main`, then run the following yourself from the primary checkout to publish the
pipeline source and request the first release:

```powershell
git push origin main
gh workflow run release.yml --repo symbiogenesis/SwiftBonsai2Runner --ref main -f tag=v0.1.0 -f prerelease=true
```

The workflow accepts a `tag` matching the numeric workspace version in
`Cargo.toml`, with notes at `docs/releases/<tag>.md`. The default `prerelease`
input is `true`. It binds the build to the dispatched commit, stages the five
native executables and their notices, builds the ARM64 MSI, and checks install
and uninstall on the hosted Windows runner without loading a model. Checksums
and `release-manifest.json` identify the resulting files and source.
Future updates use that same release inventory; no separate update server or
feed must be published. Increase the workspace version and commit matching
release notes before requesting the next release.

Only the publication job receives repository-content write permission. It
uploads the checked assets to a draft release before publishing that release;
the GitHub release creates the requested tag at the checked commit. No workflow
step runs `git push`. An existing release is not silently replaced.

The owner can inspect the run and release with:

```powershell
gh run list --repo symbiogenesis/SwiftBonsai2Runner --workflow release.yml --limit 5
gh release view v0.1.0 --repo symbiogenesis/SwiftBonsai2Runner
```

Keep hosted installation checks distinct from clean-machine hardware
acceptance. Publishing an experimental installer does not close the
[remaining application, quality, performance and isolation work](../PLAN.md).
