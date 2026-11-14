# Outstanding work

Use [PLAN.md](PLAN.md) as the source of truth for all outstanding project work.

# Target hardware

Target native Windows ARM64 on Snapdragon X Elite and Snapdragon X2 Elite Extreme.
In project plans and contracts, X1 and X2 refer to these two families respectively.
Keep implementation, optimization and qualification work within this scope;
other hardware may supply prior art for a general principle, not another target.

# Owner-device execution budgets

Diagnostics, model qualification trials and benchmarks on the owner's laptop
default to five minutes total, preferably less. A dedicated, predeclared workload
may use the smallest justified longer allowance, up to fifteen minutes total.
Record its fixed work, conservative time estimate and reason before starting;
fifteen minutes is a ceiling, not a target. This includes self-hosted CI on that
laptop and setup, intake, loading, every comparison arm, cleanup and evidence
retention. Existing narrower tool/workflow limits remain effective unless
explicitly extended. Stop early once the selected work finishes or cannot fit.
Do not restart clocks, retry a failed trial or divide a long matrix into repeated
runs to work around the allowance. Complete suites and benchmarks remain CI-only;
hosted cloud CI has its own workflow limits. Historical trial deadlines stay closed.

For every trial on this laptop, record whether AC power is connected before and
after the workload, and separately for each arm of a comparison. Retain battery
saver and charge observations when available. Keep unavailable power status
explicitly unknown; never infer a past trial's power source from a later query.
Factor AC versus battery operation and observed changes into interpretation, and
do not qualify performance comparisons with missing or mismatched power
conditions. Equal endpoint snapshots alone do not prove uninterrupted stability.
Power observations share the same predeclared aggregate trial allowance.

# Evidence retention

Keep the smallest evidence set that supports a current claim, diagnoses an
actionable failure or supplies a required independent replay. Follow the
[evidence-retention policy](docs/validation.md#evidence-retention). Routine source
edits and focused checks normally need only their commands, results and limits
in the commit or delivery message; do not create a dated evidence bundle for
every change.

When a durable record is needed, prefer one concise summary containing the
required identities, results (including losing/no-go outcomes), limits, budget,
power and cleanup observations. Keep additional raw files only when a named
acceptance gate, replay consumer or diagnosis needs them. Preserve those required
bytes and integrity records; summary-only retention cannot replace replay inputs.

Do not retain blank or whitespace-only logs, placeholder receipts, successful
build chatter, duplicate summaries/manifests, copied repository source or
temporary validation drivers without a concrete consumer. Record empty output,
exit status, skipped work and unknown observations in the summary when relevant;
a missing file never establishes success. Review an explicit file selection
before copying from scratch or uploading; do not archive whole working trees.
Intentionally empty fixtures or schema-required files are allowed when their
purpose is explicit. Apply selection to retained artifacts without breaking
collector-required intermediate files or receipt formats.

# Parallel work and commits

Each worker in a parallel fan-out must use its own individual Git worktree.
Within each worktree, place each logical change in its own individual commit.

# GitHub tooling

Prefer the `gh` CLI for GitHub-related work when it is available, including
repository, issue, release and Actions inspection or management. Use it within
the owner's authorized scope and the push, pull-request and CI rules below.

Before committing workflow, composite-action or `actions.lock` changes, run
`pwsh -NoLogo -NoProfile -File tools/ci/check_workflow_startup.ps1` and its focused regression command
in [the action-pin maintenance guide](tools/ci/README.md#action-dependency-pins).
Generate lockfiles with `gh actions-lock --no-narrow` so literal inline SHA
references retain matching dependency keys. A startup rejection prevents CI's
own verifier from running, so do not rely on that job as the first check.

# Tooling languages

Use Rust or PowerShell 7.6 or later for repository tooling and CI. Do not add
owned Python files, Python runtime setup, inline Python or Python subprocesses.
Use `tools/ci/check_owned_languages.ps1` to check the boundary. Historical
provenance records and external source citations remain evidence, not executable
dependencies. Keep binary parsing, numerical work, typed validation and policy,
and process lifetime enforcement in Rust. Keep PowerShell small: environment
setup, command composition, entry-point adapters and checks that must run before
Rust bootstrap. Reuse native commands instead of duplicating their policy in
scripts or routing Rust through PowerShell back into Rust. Native application
provisioning and supervision stay in Rust so installed use needs no shell.
Owner-device qualification must consume source-bound native helpers without
compiling them inside its allowance.

# Runner repository workflow

Never push anything, including `main`, branches or tags, directly or through
scripts. The owner handles every push. Keep the primary checkout on local `main`,
review and integrate completed Runner changes there, and leave them local.
Do not create Runner pull requests as the routine delivery workflow. Parallel
workers must still use individual Git worktrees and one commit per logical change.

# Prism repository workflow

The same no-push rule applies to the sibling `..\prism-inference` clone. The owner
handles every Prism push. Do not create Prism pull requests as the routine delivery
workflow. Keep parallel changes in individual worktrees and commits; review them
before integrating into local Prism `main` and leave them local.

# CI ownership and completion

The owner handles Host CI and Guest CI, including dispatch, monitoring and verdicts,
except for the Foundation release-bundle operation below. Other CI needs
the owner's explicit direction. Fan-out scripts complete locally without pushing,
dispatching, querying, polling or waiting for CI. Record local checks and pending
CI acceptance accurately; pending CI does not block local completion.

Agents may dispatch, monitor, download and verify artifacts from, and cancel a
Foundation run needed to produce a release bundle. Hosted release builds use only
GitHub's own execution cap: do not impose job, compiler, build-command or aggregate
operator timeouts, or cancel a valid build because a local wall-clock deadline
elapsed. Prefer a dedicated bundle-only lane; its result does not qualify the
complete Foundation suite. Bind each operation to reviewed source, retain failed
and cancelled outcomes, and verify the resulting artifacts before publication.
Owner-device diagnostics, model trials and benchmarks retain their separate
power and execution-budget rules. This exception does not authorize Git pushes
or other CI workflows.

# UI architecture

The first-party UI must remain native and written entirely in Rust. Use native
Rust UI libraries or Rust bindings to operating-system controls. Do not add a
JavaScript/TypeScript frontend, HTML/CSS UI, browser/WebView shell, Electron, or
a web-rendered Tauri frontend. PowerShell may provision, build and launch the
Rust application.

Keep interoperability separate from presentation. Preserve documented local
LLM HTTP/JSON/SSE conventions and explicit supported-subset boundaries rather
than requiring a web UI or claiming complete OpenAI API compatibility.

Read [the UI architecture contract](docs/ui-architecture.md) before changing
the UI, its dependencies, transport or launcher.
