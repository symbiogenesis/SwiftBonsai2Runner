# Outstanding work

Use [PLAN.md](PLAN.md) as the source of truth for all outstanding project work.

# Owner-device execution budgets

Diagnostics, model qualification trials and benchmarks on the owner's laptop
must fit within five minutes total, preferably less. This includes self-hosted
CI on that laptop and setup, loading, comparison and cleanup. Shrink the trial
or leave the gate pending when it cannot fit; do not divide a long matrix into
repeated runs to work around the limit. Complete suites and benchmarks remain
CI-only; hosted cloud CI has its own workflow limits.

For every trial on this laptop, record whether AC power is connected before and
after the workload, and separately for each arm of a comparison. Retain battery
saver and charge observations when available. Keep unavailable power status
explicitly unknown; never infer a past trial's power source from a later query.
Factor AC versus battery operation and observed changes into interpretation, and
do not qualify performance comparisons with missing or mismatched power
conditions. Equal endpoint snapshots alone do not prove uninterrupted stability.
Power observations share the same five-minute aggregate trial allowance.

# Parallel work and commits

Each worker in a parallel fan-out must use its own individual Git worktree.
Within each worktree, place each logical change in its own individual commit.

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

The owner handles Host CI and Guest CI, including dispatch, monitoring and verdicts.
Do not dispatch or monitor CI unless the owner explicitly directs that action.
Never wait for CI as part of task or fan-out completion. Fan-out scripts must
complete locally without pushing, dispatching, querying, polling or waiting for CI.
Record local checks and any pending CI acceptance without claiming pending checks
have passed; pending CI does not block local completion.

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
