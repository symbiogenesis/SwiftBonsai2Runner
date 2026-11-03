# Rust CI tooling

`bonsai-ci` provides workspace impact planning, Cargo check orchestration,
regression tests and the aggregate gate. It reads Cargo manifests directly and uses the routing
policy in `.github/ci/impact.json`. Impact planning never builds inspected packages or runs Cargo metadata. Git is required for ancestor and diff queries; HTTPS baseline
lookup uses Rust with a 20-second timeout and the supplied `GITHUB_TOKEN`.

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

[Premerge portable checks](../../.github/workflows/premerge.yml) run on hosted
Linux ARM64 for pull requests to `main` and manual dispatch. They check workspace
formatting, compile portable CI/CPU-policy/broker targets and run six exact named
routing, gate and request-policy regressions. Missing named cases fail the screen.
This limited check uses read-only repository permissions and no models or physical
devices; main-only Foundation and device acceptance remain separate requirements.

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
