# Windows releases

Swift Bonsai 2 Runner ships as a **portable Windows ARM64 executable** for
Snapdragon X and X2 laptops. [v0.3.0](releases/v0.3.0.md) is the experimental
release target; check [GitHub releases](https://github.com/symbiogenesis/SwiftBonsai2Runner/releases)
for completed publication. The published [v0.2.2 prerelease](https://github.com/symbiogenesis/SwiftBonsai2Runner/releases/tag/v0.2.2)
remains available.

## Download and open

Download from [GitHub releases](https://github.com/symbiogenesis/SwiftBonsai2Runner/releases).
The v0.3.0 release publishes three assets:

| Asset | Purpose |
| --- | --- |
| `SwiftBonsai2Runner-0.3.0-windows-arm64.exe` | All five application executables and their notices. |
| `SHA256SUMS.txt` | Checksums for the executable and release manifest. |
| `release-manifest.json` | Version, source revision, build identities and executable digest. |

Check the executable's SHA-256 against `SHA256SUMS.txt` from the same release
using your preferred checksum tool, then double-click the executable in Explorer.

**Deploy only the `.exe`.** Package metadata and project/dependency notices are
embedded; verification files and local build summaries need not accompany it.
Windows Release publishes no MSI or separate ZIP, uses no installer tool and
requires no installer-EULA acceptance.

The native Rust GUI launcher opens chat without a terminal, checkout or compiler.
It verifies and extracts its compressed package under
`%LOCALAPPDATA%\SwiftBonsai2\application-bundles\<first-24-manifest-sha256-characters>\`,
checking cached files before reuse. UI, broker and speech remain separate owned
processes under the existing containment and shutdown rules.

The experimental executable is unsigned. Checksums detect changed bytes within
GitHub's release trust boundary; they provide no independent publisher signature.
Review the release source identity before use.

## First launch and in-app setup

Use serviced Windows 11 ARM64 with the containment capabilities described in
[Windows sandboxing](sandboxing.md). Native Prism 0.1.26 selects context and its
launch memory budget from available RAM, retaining a 4 GiB Windows reserve and
startup margin. Unsupported containment, unknown memory or insufficient resources
stop startup; see [context sizing](swift-memory.md#memory-selected-serving-context).

Swift weights (about **6.71 GiB**), adapter, inference runtime and speech models
are separate locked downloads with their own [license terms](licensing.md).
Artifact checks and memory admission still apply. Opening the executable
prepares the compiled Swift model, adapter and selected runtime automatically.
No checkout, external lockfile or provisioning command is needed for the default
model. Speech preparation follows separately; text chat can work before it finishes.
Microphone capture requires explicit consent.

Keep the setup window open to see the current step and download progress. **Pause
setup** stops safely and retains partial downloads; **Retry setup** continues
without losing drafts or conversations. Closing and reopening also resumes setup.
Verified files are reused. Damaged model bytes are replaced only after the new
download verifies; unsafe paths, unreadable files and unrelated runtime files
require attention rather than being overwritten. Allow free storage for the
downloads and, during repair, both old and replacement files. Setup details and
**Copy diagnostics** provide the exact failed step and selected data folder.

If speech preparation fails, open **Settings > Voice** and use **Retry speech
setup** while continuing text chat. Setup status and diagnostics stay in the app.
To use an older deployment, update to the current executable and keep the same
data directory rather than following obsolete manual model-setup instructions.

Rust/PQ2 is the development default; publisher rollback is explicit and PTQ1 is
opt-in. Verification rules still apply, and preparation failures never silently
switch engine or representation. Contributor and explicit engine options belong
to the [development guide](development-guide.md#native-chat-app) and
[package contract](chat-packages.md#launch-without-a-checkout).

## Update and remove

The portable application checks GitHub at startup and every six
hours, then downloads newer Windows ARM64 releases, including experimental
prereleases. **Settings > Application** also offers a manual check. Discovery
selects the highest numeric version in the bounded catalog, excluding drafts and
older versions. An incomplete catalog or invalid newest release fails without
falling back. Published v0.2.0 requires manual replacement.

The updater verifies the GitHub asset digest, manifest, checksum inventory and
tag/source revision. Choose **Install update and close**, then **Close and install**.
Swift follows your save setting, stops owned processes, revalidates publication
and starts the verified native helper. It atomically replaces the executable and
reopens it with the original options and data directory. Models, runtimes, saved
conversations and preferences remain separate.

Failed downloads or replacement preserve the existing executable. If restart
fails, open the updated executable manually. Read-only locations require manual
replacement.

Manual updates remain available: download the newer executable from the project's
releases, verify its checksum, close Swift and replace the old `.exe`. Keep the same
data directory when reopening it. Source and preview launches perform no automatic
update work.

To remove Swift, close it and delete the executable and, optionally, its package
cache. Relaunching recreates the cache. Model and conversation data remain until
explicitly removed. [Checked-directory packages](chat-packages.md) have separate
manual activation and rollback controls.

For local builds of the same format, use the
[single-executable build command](development-guide.md#build-a-single-executable).

## Publish a release

Release scripts require PowerShell 7.6 or newer. Before building, the workflow
checks syntax, argument preservation, UTF-8 output, native failures and unsafe paths.

`AGENTS.md` reserves pushes and routine release operations for the owner.
After reviewing local `main`, the owner publishes source and requests the release:

```powershell
git push origin main
gh workflow run release.yml --repo symbiogenesis/SwiftBonsai2Runner --ref main -f prerelease=true
```

Leave `tag` blank to use `workspace.package.version` from `Cargo.toml`.
Explicit `0.3.0` or `v0.3.0` must match that version; both select `v0.3.0` and
`docs/releases/v0.3.0.md`. Mismatches report requested, workspace and expected
identities. For the next release, increment the workspace version and commit
matching notes first. `prerelease` defaults to `true`.

Windows Release builds and checks all five application lanes on hosted Windows
ARM64, verifies source-bound receipts and notices, and assembles the compressed
GUI executable. A model-free smoke check verifies its GUI header, extraction and
executable identities. It neither installs prerequisites nor runs an installer.

The notice collector conservatively includes resolved Windows build/development
dependencies. Missing crate-archive notices are bound to their recorded source
commit; the reviewed egui 0.37.0 exception records both its unavailable archive
commit and the immutable release commit supplying the shared license material.
Missing or changed material stops publication. The complete notices remain
inside the executable.

Only the publication job can write repository content. It verifies the three-asset
inventory, uploads a draft and publishes after remote asset/source checks pass.
The tag points to the checked commit. No step runs `git push` or silently replaces
an existing release.

The owner can inspect the run and release with:

```powershell
gh run list --repo symbiogenesis/SwiftBonsai2Runner --workflow release.yml --limit 5
gh release view v0.3.0 --repo symbiogenesis/SwiftBonsai2Runner
```

Hosted packaging checks do not qualify clean-machine startup, actual-model
behavior, quality, sustained performance or isolation. Those remaining gates
stay in [PLAN.md](../PLAN.md).
