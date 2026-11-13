# Windows releases

Swift Bonsai 2 Runner ships as a **portable Windows ARM64 executable** for
Snapdragon X and X2 laptops. The experimental [v0.3.0 prerelease](https://github.com/symbiogenesis/SwiftBonsai2Runner/releases/tag/v0.3.0)
is published from `80b085d7d320493e57a4ad45d21fa9874f1d8c58`.
[Release CI](https://github.com/symbiogenesis/SwiftBonsai2Runner/actions/runs/43499917247)
passed all five Rust lanes, packaging, extraction smoke checks and publication.
Independent downloads verified all three asset digests, source/tag identities and
model-free extraction; the [verification record](evidence/windows-release-030-20261113/summary.json)
retains results and limits. See the [release notes](releases/v0.3.0.md).

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
[Windows sandboxing](sandboxing.md). Native Prism 0.1.27 selects context and its
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

The application executables include their Microsoft runtime dependencies. TTS
inference runs in Rust on the CPU, without GGML DLLs or a separate runtime
installer. These build guarantees still require clean-machine speech acceptance;
historical native-engine captures do not qualify the Rust replacement.

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

The portable application checks GitHub at startup and every 15 minutes,
then downloads newer Windows ARM64 releases, including experimental
prereleases. **Settings > Application** also offers a manual check. Discovery
selects the highest numeric version in the bounded catalog, excluding drafts and
older versions. An incomplete catalog or invalid newest release fails without
falling back. Published v0.2.0 requires manual replacement.

Checks reuse unchanged metadata through conditional HTTP requests and request gzip
compression. Network and server failures back off, including GitHub's retry and
rate-limit instructions; cached metadata never turns a failed request into success.

The updater verifies the GitHub asset digest, manifest, checksum inventory and
tag/source revision before offering the downloaded update. Choose to install now
and Swift closes, replaces the executable and reopens with the original options
and data directory. Choose **Later** to keep working: the Settings button shows a
pending-update badge and closing Swift installs the verified update without
reopening. The same verified release does not repeatedly prompt during the session.
Downloads continue independently of that choice; closing before verification
finishes exits normally without waiting for the download or replacing the app.

Both installation paths follow your save setting, stop owned processes,
revalidate publication and start the verified native helper. Unknown teardown or
changed publication cannot authorize replacement. Models, runtimes, saved
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
checks workflow lock coverage and tooling policy, then syntax, argument preservation,
UTF-8 output, native failures and unsafe paths. These inexpensive checks finish
before the five build lanes start. Failed Rust commands print their phase, log
location and bounded output directly in the run; complete logs remain in artifacts.

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

If publication fails after creating a draft, rerun only the failed job from the
same run: `gh run rerun RUN_ID --failed --repo symbiogenesis/SwiftBonsai2Runner`.
The successful packaging job's retained assets are reused. Publication resumes
only a draft with the exact source, release notes, title, prerelease setting and
asset identities. It uploads missing assets and verifies existing ones without
replacing them. Lost mutation responses trigger authoritative readback rather
than a repeated upload or publication request. If the release already published
before a final readback failed, the same retry succeeds only after verifying all
metadata, all three asset digests and the exact source tag without any remote
write. This completed-release verification can succeed after `main` advances;
draft creation, upload and publication still require the original current-main
checks. Unknown assets, changed bytes or metadata, an absent or changed source
tag, or advanced `main` for a draft stop recovery for review.
A fresh dispatch still refuses an existing release; do not rebuild to recover
an interrupted upload or delete a draft merely to bypass these checks.

The owner can inspect the run and release with:

```powershell
gh run list --repo symbiogenesis/SwiftBonsai2Runner --workflow release.yml --limit 5
gh release view v0.3.0 --repo symbiogenesis/SwiftBonsai2Runner
```

Hosted packaging checks do not qualify clean-machine startup, actual-model
behavior, quality, sustained performance or isolation. Those remaining gates
stay in [PLAN.md](../PLAN.md).
