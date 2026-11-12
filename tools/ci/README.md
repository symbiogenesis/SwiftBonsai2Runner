# Rust CI tooling

`bonsai-ci` provides workspace impact planning, Cargo check orchestration, regression tests and the
aggregate gate. It reads Cargo manifests directly and uses the routing policy in
`.github/ci/impact.json`. Impact planning never builds inspected packages or runs Cargo metadata.
Git is required for ancestor and diff queries; HTTPS baseline lookup uses Rust with a 20-second
timeout and the supplied `GITHUB_TOKEN`.

```powershell
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('Bonsai-CI-' + [guid]::NewGuid().ToString('N'))
cargo build --locked --package bonsai-ci --target-dir $scratch
& (Join-Path $scratch 'debug/bonsai-ci.exe') impact --root $PWD --full
```

`impact` uses `GITHUB_WORKSPACE`, falling back to the current directory, unless
`--root` is supplied. `--config` overrides the policy path. Manual events and
`--full` select all groups. Push events compare `--after` (default `HEAD`) against
the newest successful main-push ancestor for `--workflow`. Missing credentials,
API/diff failure or an unusable baseline select full coverage, never the event's
unvalidated parent. Without `--workflow`, `--before` supplies the baseline.
Invalid UTF-8 diff paths also select full coverage.

The compact JSON plan is printed to stdout. If provided, `GITHUB_OUTPUT` receives
the plan, package list, boolean groups and filtered matrices;
`GITHUB_STEP_SUMMARY` receives a readable selection summary.

`gate --workflow foundation` reads `CI_NEEDS`, verifies the exact configured job
set and rejects planner failure, malformed results and unsuccessful selected
jobs. Only deliberately unselected skips are permitted.
The workflow transfers the planner's tested Linux executable to this gate rather
than installing Rust and compiling it a second time. Maintenance is an independent
producer; native integration and speech depend on its checked CLI artifact while the
broker and UI lanes run concurrently. The configured aggregate still requires
all selected jobs, including maintenance and speech.

The routing, baseline, output and gate regressions are Rust tests. Use named
focused selections locally with external targets/working directories; complete
suites remain CI-only under [the validation policy](../../docs/validation.md).

## Workflow scope

The retained workflows own current checks or open gates in [PLAN.md](../../PLAN.md):

| Purpose | Workflows |
| --- | --- |
| Routine source checks | `foundation`, `source-checks` |
| Windows ARM64 packaging and publication | `release` |
| Runtime application and numerical acceptance | `native-runtime-acceptance`, `native-ptq1-numerics` |
| Independent packing and pending public-pack reconstruction | `ptq1-packing-qualification`, `ptq1-public-authoring-reference` |
| Native accelerator and containment controls | `hexagon-qualification`, `sandbox-qualification` |
| Pending paired performance and dependency gates | `tts-evaluation`, `x2-cpu-evaluation`, `json-candidate` |

Standalone chat, speech and runtime-parity candidate pipelines duplicated the
production checks and builds now owned by Foundation, release and native-runtime
acceptance. The X2 compute pipeline duplicated Hexagon qualification and its
hosted Windows ABI control. These duplicate pipelines are retired.

Source checks combine portable pull-request checks, Markdown standards and the
weekly dependency audit, selecting jobs from changed paths without compiling a
planner. Hexagon qualification also owns its independent hosted Windows
ABI control. The completed row-worker experiment against frozen source is retired;
its CLI recipes and retained evidence remain available. These changes close no
current-runtime qualification gate.

Completed one-shot publisher startup, SDK inventory, public QAIC, adapter direction
and retained numerical audit pipelines are also retired. Their source recipes,
immutable inputs and historical evidence remain available. LoRA/PTQ1 plan-only
pipelines are replaced by the documented native CLI plan commands; they never
captured model qualification. Removing these pipelines closes no pending model,
device, provenance, performance or human acceptance gate.

## Native acceptance admission

`native-acceptance-guard --json REQUEST` is the shared typed validator used by
both publication and owner intake in the native-runtime acceptance workflow.
The `analyzer` operation requires the exact revision/compiler, pinned toolchain,
all three production packages and one successful zero-exit check for each of
Clippy and linking. String booleans, null exits, duplicate phases and incomplete
per-phase package coverage fail.

`test-output` requires a successful process and exactly one complete libtest
summary with positive passes, zero failures and zero ignored tests. Supply
`expectedPassed: 1` for an exact named control, or `null` for a nonempty grouped
selection. Duplicate summaries and malformed trailing content fail. `self-check`
runs 23 receipt and 13 output fixtures without compiling or loading a model.

The owning hosted workflow parses every actual PowerShell run block and the
launcher with the native PowerShell parser, executes both exact Rust guard
regressions, and retains a `workflow-contract.json` bound to the revision and
five normalized LF source identities. `workflow-receipt` validates that proof
before publishing or admitting an owner. These operations change no execution
policy and do not execute the parsed launcher or owner workload.

The hosted package schema is now version 2, with exactly five digest-bound files:
the acceptance executable, maintenance executable, this CI executable, strict
analyzer/link receipt and workflow-contract receipt. The package's own
`receipt.json` is a sixth member. Old three-file/schema-1 selections and helpers
remain stale and cannot qualify the new source. A successful contract check
closes no actual-model, performance, quality or promotion gate.

[Source checks](../../.github/workflows/source-checks.yml) run separate portable,
Markdown and dependency-audit jobs on hosted Linux ARM64. Pull requests to `main`
select portable checks for changes outside documentation: workspace formatting,
portable CI/CPU-policy/broker compilation and six exact named routing, gate and
request-policy regressions. Missing named cases fail the screen. Markdown or
linter changes select `rumdl` against [`.rumdl.toml`](../../.rumdl.toml), while
relevant main pushes and the weekly schedule select a fresh dependency audit.
Manual dispatch selects portable and Markdown checks on any branch, plus the
audit on `main`. Unknown source ranges select all checks eligible for that event.
Each check supersedes only its own pending work; manual checks keep independent
runs. A failed selector fails every eligible check before checkout or compilation.
Source-checks workflow edits select every eligible check. These jobs retain read-only repository
permissions and use no models or physical devices; Foundation and device acceptance
remain separate requirements.

## Native Rust checks

`rust-select` validates `AFFECTED_PACKAGES` and `LANE_PACKAGES` JSON arrays
against `config/rust-packages.json` and writes their intersection in registry
order to `BROKER_PACKAGES` in `GITHUB_ENV`. It validates `RUST_LANE` and writes
`has-packages` to `GITHUB_OUTPUT`. Empty intersections stay empty; unknown
packages fail before checking anything.

`rust-plan` prints the exact Cargo commands without executing them.
`rust-check` executes the same plan on Windows using owned Jobs. Both accept
`--root`, repeated `--package` or `--packages-json`, `--group All|Broker`,
`--profile Foundation|Compatibility|Native`, repeated `--phase`, `--toolchain`,
`--offline`, `--full-tests` and `--build`. The default phases are format, Clippy
and rustdoc; the default compiler comes from the repository's pinned channel
with its production Windows ARM64 host. Formatting shares one invocation;
compile phases batch all-feature packages while isolating restricted features.
Rustdoc emits JSON. Commands never widen an explicit package selection.

```powershell
bonsai-ci rust-plan --root $PWD --package bonsai-broker --phase clippy
bonsai-ci rust-check --root $PWD --package bonsai-broker --phase clippy --target C:\Scratch\target --evidence C:\Scratch\new-evidence --timeout-seconds 120
```

Execution requires expanded absolute external `--target` and new `--evidence`
directories that do not overlap. Logs and the schema-v2 `rust-checks.json`
receipt retain command arguments, selected packages, source/lock/registry
SHA-256 values, compiler identity, elapsed time and failures. Synthetic test
temporary files remain in a separate retained system-temp directory and are not
uploaded with evidence. Child environment overlays preserve inherited Windows
and profile variables. A command timeout or output overflow kills its complete
Job; stdout and stderr each have a 16 MiB bound. Nonzero checks are retained
while later planned checks continue. Process/timeout errors stop the plan.

Complete suites require `GITHUB_ACTIONS=true` and `GITHUB_REF=refs/heads/main`
before creating outputs. Hosted checks retain per-command timeouts (1-3600
seconds). Other executions share a 290-second maximum, reserving time within
the owner's five-minute aggregate allowance for teardown; setup before this
command still consumes that allowance. The caller must select a feasible
smaller workload and timeout. This command never runs benchmarks.

Foundation bootstraps this command on hosted Windows using the pinned compiler
and a separate target/image so it can subsequently check and rebuild itself.
A self-hosted cache miss does not compile the CI bootstrap. The deleted
PowerShell registry, check module and check script have no compatibility
wrappers; feature, batching, selection and suite-guard regressions are Rust
tests. The maintenance PowerShell/C# bootstrap and its tests have also been
retired; native command input, caller identity, build ownership and immutable
publication tests own those contracts.

## Action dependency pins

Keep every external action and reusable-workflow `uses:` reference at a full
40-character commit SHA, including references inside composite actions. Keep
release versions in comments for review. Same-repository actions use `$/` paths
so GitHub and the lockfile tool can inspect their dependency closure.

The generated [actions.lock](../../.github/workflows/actions.lock) records the
upstream reference, resolved commit and repository identities. Source checks run
`gh actions-lock --verify` to reject missing, stale or unverifiable lock entries.
The independent [startup preflight](check_workflow_startup.py) also checks the
literal references GitHub requires before creating any jobs, including the
dependency closure of same-repository composites, and enforces our inline-SHA
policy. A tag-keyed lock entry does not cover an inline `@SHA` reference even when
both resolve to the same commit. Local startup checks do not verify upstream
provenance or replace GitHub workflow validation.
A `sha-as-ref` warning is expected for deliberate full-SHA references; keep the
SHA and its exact lockfile key rather than changing the reference to a tag.

Use only the official `github/gh-actions-lock` tool at the version and binary
SHA-256 recorded in Source checks. The workflow verifies the downloaded bytes
before executing them. Keep additions to CI dependencies sparse: each action,
linter and downloaded tool adds code and maintainers to the trust boundary.
Update the tool's release version and reviewed checksum together; do not fetch a
fresh checksum at runtime or install an unpinned extension. No extra YAML parser
or security linter is required for this lockfile check.

When changing an action, use a disposable worktree and the same verified tool:

1. Resolve the intended upstream release to its commit and update every affected
   workflow and composite-action SHA, retaining its version comment.
2. Run `gh actions-lock --no-narrow` to regenerate the lockfile while preserving
   inline SHA references. The generated dependency keys must match the literal
   `uses:` references; a symbolic upstream `ref` retains provenance separately.
   Never restore inline SHAs after generating a tag-keyed lockfile without
   regenerating the matching entries. To repair an existing tag-keyed lockfile
   after that mistake, run `gh actions-lock --relock --no-narrow` once to rebuild
   the closure, and verify that reviewed commits and repository identities stay
   unchanged. A routine generation can otherwise retain the existing tag keys.
3. Before committing any workflow, composite action or lockfile change, run
   `python -B tools/ci/check_workflow_startup.py` and
   `python -B -m unittest discover -s tools/ci -p test_workflow_startup.py` from
   the repository root. These offline checks need no compiler or downloaded tool.
   A GitHub startup rejection also prevents its CI preflight from running, so
   this local check is required.
4. Run `gh actions-lock --verify` against the complete workflow directory. Review
   the workflow, composite-action and lockfile changes together, including any
   added transitive dependencies or changed repository identities.

Do not use `--accept-moved` in automation. Investigate moved references before
accepting them. Local verification does not establish a hosted CI verdict, and
the owner retains pushes and CI dispatch.
