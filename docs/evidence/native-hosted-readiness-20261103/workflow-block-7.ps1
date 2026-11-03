Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
[Environment]::CurrentDirectory = $env:BONSAI_NATIVE_WORK
$cpu = Get-CimInstance Win32_Processor -OperationTimeoutSec 10 | Select-Object -First 1
if ($cpu.Name -notmatch 'Snapdragon.*X.*Elite') { throw 'Actual Snapdragon X/X2 Elite identity is required; no generic ARM substitute.' }
$hostReport = [ordered]@{
  SchemaVersion=1; Revision=$env:GITHUB_SHA; RunId=$env:GITHUB_RUN_ID
  RunAttempt=$env:GITHUB_RUN_ATTEMPT; Cpu=$cpu.Name
  Os=[Environment]::OSVersion.VersionString
  Architecture=[Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture.ToString()
  Profile=$env:BONSAI_NATIVE_PROFILE
  BoundedHistoryRequested=($env:BONSAI_NATIVE_EXTENDED -eq 'true')
  StalledConsumerRequested=($env:BONSAI_NATIVE_STALLED_CONSUMER -eq 'true')
  AcceptanceProcessExitCode=$null; AggregateBudgetExpired=$false
  OwnerJobStartedTick=$env:BONSAI_NATIVE_JOB_STARTED_TICK
  OwnerJobStartedUtc=$env:BONSAI_NATIVE_JOB_STARTED_UTC
  WholeJobMaximumSeconds=[int]$env:BONSAI_NATIVE_WHOLE_JOB_SECONDS; PostProcessReserveSeconds=30
  BudgetReason=$env:BONSAI_NATIVE_BUDGET_REASON
  PreparationElapsedMilliseconds=$null; ModelCaseBudgetSeconds=$null
  PreparationWallElapsedMilliseconds=$null; PreparationMonotonicElapsedMilliseconds=$null
  TestProcessHardLimitSeconds=$null
  OwnerDeadlineApplied=$false; OwnerElapsedMilliseconds=$null
  OwnerWorkSeconds=$null; OwnerProcessExitReserveSeconds=$null
  StartupDiagnosticsReceipt='startup-diagnostics.json'
  StartupDiagnosticsReady=$false; StartupDiagnosticsClassification='not-collected'
  StartupFailureMarkers=@()
  Passed=$false; DefaultRuntimeQualified=$false
  AllRequestedWorkloadsPassed=$null
}
try {
  $case = switch -CaseSensitive ($env:BONSAI_NATIVE_PROFILE) {
    'pq2-no-adapter' { 'pinned_native_model_streams_recovers_and_restarts_in_required_sandbox' }
    'ptq1-adapter' {
      if ($env:BONSAI_NATIVE_EXTENDED -ceq 'true' -or $env:BONSAI_NATIVE_STALLED_CONSUMER -ceq 'true') {
        throw 'The bounded PTQ1 adapter profile does not admit extended_history or stalled_consumer.'
      }
      'pinned_native_ptq1_adapter_starts_and_streams_within_device_budget'
    }
    'ptq1-adapter-lifecycle' {
      if ($env:BONSAI_NATIVE_EXTENDED -ceq 'true' -or $env:BONSAI_NATIVE_STALLED_CONSUMER -ceq 'true') {
        throw 'The bounded PTQ1 adapter scale/history profile does not admit extended_history or stalled_consumer.'
      }
      'pinned_native_ptq1_adapter_checks_scale_change_rejection_and_history'
    }
    'ptq1-adapter-lifecycle-row4' {
      if ($env:BONSAI_NATIVE_EXTENDED -ceq 'true' -or $env:BONSAI_NATIVE_STALLED_CONSUMER -ceq 'true') {
        throw 'The bounded four-row PTQ1 adapter scale/history profile does not admit extended_history or stalled_consumer.'
      }
      'pinned_native_ptq1_adapter_row4_checks_scale_change_rejection_and_history'
    }
    'ptq1-adapter-boundaries-row4' {
      if ($env:BONSAI_NATIVE_EXTENDED -ceq 'true' -or $env:BONSAI_NATIVE_STALLED_CONSUMER -ceq 'true') {
        throw 'The bounded four-row PTQ1 scale-boundary/reset profile does not admit extended_history or stalled_consumer.'
      }
      'adapter_boundaries::pinned_native_ptq1_adapter_row4_checks_scale_boundaries_and_reset'
    }
    'ptq1-adapter-service-row4' {
      if ($env:BONSAI_NATIVE_EXTENDED -ceq 'true' -or $env:BONSAI_NATIVE_STALLED_CONSUMER -ceq 'true') {
        throw 'The bounded four-row PTQ1 service-integration profile does not admit extended_history or stalled_consumer.'
      }
      'service_integration::pinned_native_ptq1_adapter_row4_checks_released_service_integration'
    }
    'ptq1-adapter-service-rollback-row4' {
      if ($env:BONSAI_NATIVE_EXTENDED -ceq 'true' -or $env:BONSAI_NATIVE_STALLED_CONSUMER -ceq 'true') {
        throw 'The bounded PTQ1 service plus original-PQ2 rollback profile does not admit extended_history or stalled_consumer.'
      }
      'service_rollback::pinned_native_ptq1_service_and_original_pq2_rollback_row4'
    }
    'pq2-adapter-row4' {
      if ($env:BONSAI_NATIVE_EXTENDED -ceq 'true' -or $env:BONSAI_NATIVE_STALLED_CONSUMER -ceq 'true') {
        throw 'The bounded PQ2 four-row adapter profile does not admit extended_history or stalled_consumer.'
      }
      'pinned_native_pq2_adapter_row4_starts_and_streams_within_device_budget'
    }
    'pq2-adapter-lifecycle-row4' {
      if ($env:BONSAI_NATIVE_EXTENDED -ceq 'true' -or $env:BONSAI_NATIVE_STALLED_CONSUMER -ceq 'true') {
        throw 'The bounded PQ2 adapter scale/history profile does not admit extended_history or stalled_consumer.'
      }
      'pinned_native_pq2_adapter_row4_checks_scale_change_rejection_and_history'
    }
    'pq2-adapter-service-row4' {
      if ($env:BONSAI_NATIVE_EXTENDED -ceq 'true' -or $env:BONSAI_NATIVE_STALLED_CONSUMER -ceq 'true') {
        throw 'The bounded four-row PQ2 service-integration profile does not admit extended_history or stalled_consumer.'
      }
      'service_integration::pinned_native_pq2_adapter_row4_checks_released_service_integration'
    }
    'pq2-adapter-speed' {
      if ($env:BONSAI_NATIVE_EXTENDED -ceq 'true' -or $env:BONSAI_NATIVE_STALLED_CONSUMER -ceq 'true') {
        throw 'The paired request-throughput profile does not admit extended_history or stalled_consumer.'
      }
      'speed::pinned_native_pq2_adapter_compares_request_throughput'
    }
    'ptq1-pq2-complete-response' {
      if ($env:BONSAI_NATIVE_EXTENDED -ceq 'true' -or $env:BONSAI_NATIVE_STALLED_CONSUMER -ceq 'true') {
        throw 'The complete-response representation pair does not admit extended_history or stalled_consumer.'
      }
      'representation::pinned_native_ptq1_pq2_capture_complete_responses'
    }
    'ptq1-pq2-complete-response-row4' {
      if ($env:BONSAI_NATIVE_EXTENDED -ceq 'true' -or $env:BONSAI_NATIVE_STALLED_CONSUMER -ceq 'true') {
        throw 'The four-worker complete-response representation pair does not admit extended_history or stalled_consumer.'
      }
      'representation::pinned_native_ptq1_pq2_row4_capture_complete_responses'
    }
    default { throw 'Unknown actual-model acceptance profile.' }
  }
  $test = $env:BONSAI_NATIVE_TEST
  $listed = @(& $test --list --ignored)
  if ($LASTEXITCODE -ne 0 -or $listed -cnotcontains ($case + ': test')) { throw 'Required actual-model case was not found.' }
  # The model owner must not inherit Actions' artifact/OIDC/GitHub
  # credentials. Scope this omission to the test child; retain the
  # complete Windows/profile environment and the production guard.
  $start = [Diagnostics.ProcessStartInfo]::new($test)
  $start.WorkingDirectory = $env:BONSAI_NATIVE_WORK
  $start.UseShellExecute = $false
  $start.CreateNoWindow = $true
  $start.RedirectStandardOutput = $true
  $start.RedirectStandardError = $true
  foreach ($argument in @($case, '--exact', '--ignored', '--nocapture')) {
    $start.ArgumentList.Add($argument)
  }
  $jobStarted = [DateTime]::Parse($env:BONSAI_NATIVE_JOB_STARTED_UTC, [Globalization.CultureInfo]::InvariantCulture, [Globalization.DateTimeStyles]::RoundtripKind)
  # Charge checkout, hosted-binary intake, verification and inventory.
  # Reserve ten seconds between the Rust soft limit and process limit,
  # then thirty seconds for forced teardown, diagnostics and uploads.
  # An inherited owner clock starts before registration and may only
  # shorten these limits, preserving exit time before its watchdog.
  $ownerDeadline = @{
    RunId=$env:BONSAI_NATIVE_OWNER_RUN_ID; Revision=$env:BONSAI_NATIVE_OWNER_REVISION
    StartedTick=$env:BONSAI_NATIVE_OWNER_STARTED_TICK; StartedUtc=$env:BONSAI_NATIVE_OWNER_STARTED_UTC
    WorkSeconds=$env:BONSAI_NATIVE_OWNER_WORK_SECONDS
  }
  try {
    if (-not $env:BONSAI_TOOLS_PATH) { throw 'The source-bound hosted maintenance tool is required.' }
    if (-not $env:BONSAI_NATIVE_JOB_STARTED_TICK) { throw 'The original whole-job monotonic start is required.' }
    $budgetClock = [Diagnostics.Stopwatch]::StartNew()
    $limits = & $env:BONSAI_TOOLS_PATH native-acceptance-budget --json (@{
      jobStartedUtc=$jobStarted.ToString('o'); nowUtc=[DateTime]::UtcNow.ToString('o')
      jobStartedTick=[long]$env:BONSAI_NATIVE_JOB_STARTED_TICK
      nowTick=[Diagnostics.Stopwatch]::GetTimestamp(); tickFrequency=[Diagnostics.Stopwatch]::Frequency
      runId=$env:GITHUB_RUN_ID; revision=$env:GITHUB_SHA; ownerDeadline=$ownerDeadline
      wholeJobMaximumSeconds=[int]$env:BONSAI_NATIVE_WHOLE_JOB_SECONDS
    } | ConvertTo-Json -Depth 64 -Compress) | ConvertFrom-Json -AsHashtable
  } catch {
    $hostReport.AggregateBudgetExpired = $true
    throw
  }
  # Charge tool resolution, handshake and JSON execution after the
  # supplied clocks. IPC must never extend either original deadline.
  $budgetBridgeTick = [Diagnostics.Stopwatch]::GetTimestamp()
  $budgetBridgeElapsed = $budgetClock.Elapsed.TotalSeconds
  $budgetBridgeCharge = [int][Math]::Ceiling($budgetBridgeElapsed)
  $setupElapsed = $limits.PreparationElapsedSeconds + $budgetBridgeElapsed
  $hardLimitSeconds = $limits.HardLimitSeconds - $budgetBridgeCharge
  $caseBudgetSeconds = $limits.CaseBudgetSeconds - $budgetBridgeCharge
  # Receipt/environment preparation and process creation consume this
  # absolute limit rather than starting another relative wait.
  $processDeadlineTick = $budgetBridgeTick + ([long]$hardLimitSeconds * [Diagnostics.Stopwatch]::Frequency)
  if ($caseBudgetSeconds -lt 1) {
    $hostReport.AggregateBudgetExpired = $true
    throw 'Preparation exhausted the acceptance allowance; qualification remains pending.'
  }
  $hostReport.OwnerDeadlineApplied = $limits.OwnerDeadlineApplied
  if ($limits.OwnerDeadlineApplied) {
    $hostReport.OwnerElapsedMilliseconds = [Math]::Ceiling(($limits.OwnerElapsedSeconds + $budgetBridgeElapsed) * 1000)
    $hostReport.OwnerWorkSeconds = $limits.OwnerWorkSeconds
    $hostReport.OwnerProcessExitReserveSeconds = $limits.OwnerProcessExitReserveSeconds
  }
  $hostReport.PreparationElapsedMilliseconds = [Math]::Ceiling($setupElapsed * 1000)
  $hostReport.PreparationWallElapsedMilliseconds = [Math]::Ceiling(($limits.PreparationWallElapsedSeconds + $budgetBridgeElapsed) * 1000)
  $hostReport.PreparationMonotonicElapsedMilliseconds = [Math]::Ceiling(($limits.PreparationMonotonicElapsedSeconds + $budgetBridgeElapsed) * 1000)
  $hostReport.ModelCaseBudgetSeconds = $caseBudgetSeconds
  $hostReport.TestProcessHardLimitSeconds = $hardLimitSeconds
  $start.Environment['BONSAI_NATIVE_CASE_BUDGET_SECONDS'] = [string]$caseBudgetSeconds
  $start.Environment['BONSAI_NATIVE_WHOLE_JOB_SECONDS'] = [string]$limits.WholeJobMaximumSeconds
  foreach ($name in @('ACTIONS_RUNTIME_TOKEN', 'ACTIONS_ID_TOKEN_REQUEST_TOKEN', 'GITHUB_TOKEN', 'GH_TOKEN', 'ACTIONS_RUNNER_INPUT_TOKEN')) {
    [void]$start.Environment.Remove($name)
  }
  $child = [Diagnostics.Process]::Start($start)
  try {
    $stdout = $child.StandardOutput.ReadToEndAsync()
    $stderr = $child.StandardError.ReadToEndAsync()
    $remainingMilliseconds = [int][Math]::Floor(($processDeadlineTick - [Diagnostics.Stopwatch]::GetTimestamp()) * 1000.0 / [Diagnostics.Stopwatch]::Frequency)
    if ($remainingMilliseconds -lt 1 -or -not $child.WaitForExit($remainingMilliseconds)) {
      $hostReport.AggregateBudgetExpired = $true
      throw 'The acceptance process exceeded its remaining owner-job budget.'
    }
    [Console]::Out.Write($stdout.GetAwaiter().GetResult())
    $testStderr = $stderr.GetAwaiter().GetResult()
    [Console]::Error.Write($testStderr)
    # Retain only fixed broker classifications, never arbitrary
    # test stderr or native arguments in this public JSON receipt.
    $marker = '(?m)^Native backend startup failure: stage=(authenticated-readiness|native-identity), error=(Backend|Memory|Cancelled), process=(running|status-unavailable|exited-0x[0-9A-F]{8})\r?$'
    $hostReport.StartupFailureMarkers = @([regex]::Matches($testStderr, $marker) | ForEach-Object { $_.Value.TrimEnd("`r") })
    $code = $child.ExitCode
    $hostReport.AcceptanceProcessExitCode = $code
  } finally {
    try {
      if (-not $child.HasExited) {
        $child.Kill($true)
        if (-not $child.WaitForExit(10000)) { throw 'Native acceptance cleanup did not finish within ten seconds.' }
      }
    } finally { $child.Dispose() }
  }
  if ($code -ne 0) { throw 'Actual-model application acceptance failed.' }
  $receipt = Join-Path $env:BONSAI_NATIVE_EVIDENCE 'acceptance.json'
  if (-not (Test-Path -LiteralPath $receipt -PathType Leaf)) { throw 'Actual-model acceptance receipt is missing.' }
  $hostReport.Passed = $true
} finally {
  $env:BONSAI_NATIVE_DIAGNOSTICS_READY = 'false'
  $env:BONSAI_NATIVE_DIAGNOSTICS_STATUS = 'collection-failed'
  'BONSAI_NATIVE_DIAGNOSTICS_READY=false' >> $env:GITHUB_ENV
  try {
    if (-not $env:BONSAI_TOOLS_PATH) { throw 'The source-bound hosted maintenance tool is required.' }
    $diagnostics = & $env:BONSAI_TOOLS_PATH startup-diagnostics --json (@{
      evidenceDirectory=$env:BONSAI_NATIVE_EVIDENCE
      acceptanceExitCode=$hostReport.AcceptanceProcessExitCode; budgetExpired=$hostReport.AggregateBudgetExpired
    } | ConvertTo-Json -Depth 64 -Compress) | ConvertFrom-Json -AsHashtable
    # Recover the bounded typed workload result even when the test
    # exits 101 after retaining partial progress and owned cleanup.
    # Missing/malformed receipts remain unknown; this never changes
    # Passed or the original failing process disposition.
    if ($env:BONSAI_NATIVE_PROFILE -cin @('pq2-no-adapter', 'pq2-adapter-lifecycle-row4', 'pq2-adapter-service-row4', 'ptq1-adapter-lifecycle', 'ptq1-adapter-lifecycle-row4', 'ptq1-adapter-boundaries-row4', 'ptq1-adapter-service-row4', 'ptq1-adapter-service-rollback-row4') -and
        $diagnostics.AllRequestedWorkloadsPassed -is [bool]) {
      $hostReport.AllRequestedWorkloadsPassed = $diagnostics.AllRequestedWorkloadsPassed
    }
    $env:BONSAI_NATIVE_DIAGNOSTICS_READY = if ($diagnostics.Ready -ceq $true) { 'true' } else { 'false' }
    $env:BONSAI_NATIVE_DIAGNOSTICS_STATUS = $diagnostics.Classification
  } catch {
    # Keep collection errors value-free: paths and raw tool diagnostics
    # must never enter the public host receipt or diagnostic upload.
    $env:BONSAI_NATIVE_DIAGNOSTICS_READY = 'false'
    $env:BONSAI_NATIVE_DIAGNOSTICS_STATUS = 'collection-failed'
  } finally {
    $hostReport.StartupDiagnosticsReady = $env:BONSAI_NATIVE_DIAGNOSTICS_READY -ceq 'true'
    $hostReport.StartupDiagnosticsClassification = $env:BONSAI_NATIVE_DIAGNOSTICS_STATUS
    ('BONSAI_NATIVE_DIAGNOSTICS_READY=' + $env:BONSAI_NATIVE_DIAGNOSTICS_READY) >> $env:GITHUB_ENV
    $hostReport | ConvertTo-Json -Depth 4 |
      Set-Content -LiteralPath (Join-Path $env:BONSAI_NATIVE_EVIDENCE 'host.json') -Encoding utf8NoBOM
  }
}
$profileScope = if ($env:BONSAI_NATIVE_PROFILE -ceq 'ptq1-adapter') {
  'PTQ1 with the approved adapter requires one scale-1.3 four-token stream; scale-zero and invalid-scale/reuse checks run only within the same budget. Unrun multi-scale, history, context and restart gates remain pending.'
} elseif ($env:BONSAI_NATIVE_PROFILE -cin @('ptq1-adapter-lifecycle', 'ptq1-adapter-lifecycle-row4')) {
  $workers = if ($env:BONSAI_NATIVE_PROFILE -ceq 'ptq1-adapter-lifecycle-row4') { 'four' } else { 'one' }
  ('Current-weight PTQ1 with ' + $workers + ' explicitly configured row workers requires three one-token-cap streams: scale 1.3, scale zero, then scale 1.3 with the exact captured initial response supplied as assistant history after invalid-scale connection closure without a response, all in the same owner. Text, SHA-256, source messages, finish reason and usage bind that history; missing visible output keeps acceptance pending. Each further request requires more than 45 seconds remaining; a request timeout reserves 30 seconds for owned cleanup. Any missing required step remains pending and fails after cleanup. One-worker and four-worker evidence remain separate. This does not prove zero/no-adapter logits identity, generated-history state reuse or parity, numerical parity, quality or TPS.')
} elseif ($env:BONSAI_NATIVE_PROFILE -cin @('ptq1-adapter-service-row4', 'pq2-adapter-service-row4')) {
  $representation = if ($env:BONSAI_NATIVE_PROFILE -ceq 'pq2-adapter-service-row4') { 'Original PQ2' } else { 'Current-weight PTQ1' }
  ($representation + ' with four row workers combines scale-1.3, negative-zero and scale-two cap-one streams, invalid-scale/context rejection, explicit Stop after a nonempty complete SSE content record from the same scale-1.3 prompt, then fresh verified ownership and exact cap-one scale-1.3 replay. Same-owner prefix equality is informational because sampler draws advance. Counts, requests, output, credential replacement and both owned cleanups remain distinct. A terminal event already received cannot qualify cancellation; broker operation counters must retain exactly three completed and three failed operations, including the two rejections and interruption. Fresh startup requires more than 60 seconds remaining and all request/start work shares the original deadline minus 30 seconds for owned cleanup. Missing work stays pending. Matching reported prompt counts plus the source-bound identical count/generation body contract are not a private wire capture, server per-request scale acknowledgement, numerical adapter effect or complete state parity. Independent retained-data/source review must decide the service gate; full promotion, performance, provenance and quality remain pending.')
} elseif ($env:BONSAI_NATIVE_PROFILE -ceq 'ptq1-adapter-service-rollback-row4') {
  'One explicit Prism 0.1.14 workload retains the five-request PTQ1 scale-boundary/Stop/fresh-seeded-replay sequence, then only after both PTQ1 Jobs and listeners are reaped starts original PQ2 explicitly with the same released runtime, adapter 1.3 and four workers. The original-PQ2 arm must naturally complete the existing fixed tree-name request within its 32-token cap, release its Job/listener with one completed operation and no failures, then re-verify the entire retained original file length and CNG SHA-256. Rust power endpoints cover the model case and each owner; pre-setup aggregate conditions require separately retained owner observations and remain unknown otherwise. PTQ1 uses an earlier deadline withholding the 75-second rollback tail, and admission rejects before its first owner when at most 135 seconds remain. The third startup requires more than 75 seconds remaining; its requests reserve the existing 30 seconds for owned cleanup plus 15 seconds for post-cleanup full-file verification, all against the original absolute deadline. Only connection refusal confirms listener release; other probe failures stay unknown. Partial service and rollback receipts remain separate; neither a skipped rollback nor a retained hash qualifies the whole case. No performance/resource benefit, numerical/full-context parity, quality, comfort or default promotion is claimed.'
} elseif ($env:BONSAI_NATIVE_PROFILE -ceq 'pq2-adapter-row4') {
  'Original PQ2 with the approved adapter explicitly configures four native row workers and requires exactly one scale-1.3 four-token-cap stream, verified model/adapter readiness and owned cleanup. This does not independently observe thread count or establish TPS. Multi-scale, rejection/reuse, history, context, restart, performance and quality gates remain unrun.'
} elseif ($env:BONSAI_NATIVE_PROFILE -ceq 'pq2-adapter-lifecycle-row4') {
  'Original PQ2 with four row workers requires scale-1.3 and scale-zero streams, invalid-scale connection closure without a response, then scale-1.3 streaming with the exact captured initial response supplied as assistant history in the same owner. Text, SHA-256, source messages, finish reason and usage bind that history; missing visible output keeps acceptance pending. Each further request requires more than 45 seconds remaining; a request timeout reserves 30 seconds for owned cleanup. Any missing required step remains pending and fails after cleanup. This does not prove zero/no-adapter logits identity, generated-history state reuse or parity, numerical parity, quality or TPS.'
} elseif ($env:BONSAI_NATIVE_PROFILE -ceq 'pq2-adapter-speed') {
  'One fixed-order paired request measures original PQ2 with adapter 1.3 and one versus four workers, each in a fresh owner, using identical eight-token-cap counting prompts and seed 42. Request TPS includes prefill and complete HTTP response delivery, excludes startup, and is not device decode TPS. A ratio requires reported usage at the cap, identical visible output and both owned cleanups. No retry, repeated-trial benefit or default promotion is qualified.'
} elseif ($env:BONSAI_NATIVE_PROFILE -cin @('ptq1-pq2-complete-response', 'ptq1-pq2-complete-response-row4')) {
  $workers = if ($env:BONSAI_NATIVE_PROFILE -ceq 'ptq1-pq2-complete-response-row4') { 'four row workers' } else { 'one row worker' }
  ('One fixed-order original-PQ2/current-weight-PTQ1 pair captures complete responses to a short public tree-naming prompt, each with adapter 1.3, ' + $workers + ', production sampler and seed 42 in a fresh required-sandbox owner. Both must stop naturally within a 32-token cap, report usage and release their owned Job and listener. Exact UTF-8 responses and SHA-256 are retained, including losing or truncated completed captures. Latency and scoped resource ratios require identical output and usage; absent or inconsistent resource telemetry leaves the resource comparison pending. Job lifetime commitment, root lifetime residency and sampled serving bounds remain separate. Every startup requires more than 60 seconds remaining, requests reserve 30 seconds for cleanup, and an incomplete pair stays pending; no continuation jobs may split the matrix. One-worker and four-worker evidence remain separate. This is feasibility evidence only; repeated trials, full-context resources, paging, decode TPS, adapter baseline/zero controls, quality, numerical parity, comfort and default promotion remain pending.')
} else {
  'No adapter was enabled. Full-context numerical/resource, performance, quality and daily-service qualification remain separate.'
}
@('Bounded actual-model Rust application lifecycle passed under required native containment.',
  ('Explicit acceptance profile: ' + $env:BONSAI_NATIVE_PROFILE),
  "All setup, model loading, execution and cleanup share one predeclared $env:BONSAI_NATIVE_BUDGET_MINUTES-minute job (five preferred, fifteen maximum). Actual-model cases reserve sixty seconds from that total and are further reduced by checkout, intake and verification time. Inherited shorter owner clocks and separate teardown/evidence reserves still apply. Do not split a matrix into repeated jobs.",
  ('Bounded supplied history (128 repetitions, four-token cap) requested: ' + $env:BONSAI_NATIVE_EXTENDED),
  ('Explicit Stop with a nonreading connected client requested: ' + $env:BONSAI_NATIVE_STALLED_CONSUMER),
  'The stalled-client workload requires an interrupted broker operation, bounded owned shutdown and fresh-owner recovery; it does not prove socket-buffer saturation or that the backend was still generating at Stop.',
  'Bounded history records framing, termination and reported output usage. The stalled request reserves 4096 tokens but Stop follows its first visible prefix; neither workload proves 4096 generated tokens or exact 8K occupancy.',
  'Each optional workload requires more than 120 seconds remaining for cleanup and fresh-owner recovery; skipped requests remain pending. Read allRequestedWorkloadsPassed independently of lifecycle success in acceptance.json.',
  $profileScope,
  'This workflow cannot promote the default runtime.') | Add-Content -LiteralPath $env:GITHUB_STEP_SUMMARY
