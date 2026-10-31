# Prepared temporary native acceptance owner

`Invoke-TemporaryNativeAcceptanceOwner.ps1` is a reviewed-source preparation for
one explicitly authorized owner-device attempt. Preparing or committing it does
not register a runner, execute a test, authorize an attempt or qualify a default.
It is derived from the retained 2026-10-30 temporary owner wrapper; historical
timestamps, source IDs and the fixed 906-byte receipt assumption are removed.

The only admitted profile is `pq2-adapter-lifecycle-row4`. The existing official
Windows ARM64 runner must already be installed outside Git, idle and unregistered.
The helper downloads no runner, package, tool or model and performs no build.
Its fixed outer allowance is **240 seconds total**, with work ending at **210
seconds** and 30 seconds reserved for cleanup. The surrounding task must charge
all earlier verification/intake and later archive/confirmation work against the
remaining 60 seconds of the shared five-minute budget. There is no retry loop.

Before invocation, independently review the exact helper bytes, settled Runner
main revision embedding Prism 0.1.10, released bytes and owning Prism/Runner main
results. Publication and the single device attempt need existing authorization.
The helper separately requires a successful exact-revision Runner Foundation,
completed successful hosted acceptance preparation and one queued acceptance job
whose only label is `native-acceptance-<RunId>`. It rechecks main immediately before
starting the listener. No in-progress preparation or producer-point substitute is
accepted.

Explicit inputs bind the run/attempt/revision, Foundation run, listener SHA-256,
artifact ID/byte count/SHA-256, retained ZIP and extracted package, receipt byte
count/SHA-256 and expected maintenance `ToolsSourceId`. The receipt must name the
pinned native ARM64 nightly compiler and exactly the two expected binaries.
Every ZIP member and extracted binary is checked against those reviewed identities;
the binaries must be native ARM64 PE files. Do not derive the expected inputs from
unreviewed local bytes merely to make verification pass.

The owning workflow must honor these inherited variables before model execution:

- `BONSAI_NATIVE_OWNER_RUN_ID` and `BONSAI_NATIVE_OWNER_REVISION` must match the job.
- `BONSAI_NATIVE_OWNER_STARTED_TICK` is the original machine-wide monotonic tick;
  `BONSAI_NATIVE_OWNER_STARTED_UTC` supplies reporting attribution.
- `BONSAI_NATIVE_OWNER_WORK_SECONDS` is exactly 210. Clamp the test/process budget
  to the remaining outer allowance, preserving cooperative Stop and process cleanup
  before the outer watchdog cutoff. Reject malformed or incomplete bindings.

**Do not execute this helper with a workflow that ignores that earlier deadline.**
The workflow's independent five-minute timeout does not provide that clamp. A run
that reaches the outer watchdog cutoff remains incomplete even if an earlier
stream succeeded.

Invoke only as a dedicated `pwsh -NoProfile -NonInteractive -File` process. An
exclusive runner-directory lease prevents concurrent helpers. The independent
watchdog binds the dedicated owner PID/creation time and exact run/profile/budget.
Only recorded runner/API process identities, their recorded children and the
unique named registration are eligible for cleanup. Credential deletion is limited
to the three known files in the exact runner directory and refuses mismatched
runner ownership. API/runner output and registration-token responses are not
written to helper reports; fixed credential-free proof/lifetime/cleanup receipts
are retained in a fresh external evidence directory. Official runner internal
diagnostics remain private and are not an artifact selection.

This source has not been executed or locally tested as part of preparation.
Syntax/analyzer and failure-path qualification remain required before use.
Network/API failures can prevent remote deregistration; incomplete cleanup is
reported rather than treated as success. Filesystem or OS stalls cannot be
converted into a successful bounded receipt. A missing lifecycle step, exhausted
allowance or absent archive keeps its qualification gate pending.
