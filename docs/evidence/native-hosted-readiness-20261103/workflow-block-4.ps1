"BONSAI_NATIVE_JOB_STARTED_TICK=$([Diagnostics.Stopwatch]::GetTimestamp())" >> $env:GITHUB_ENV
"BONSAI_NATIVE_JOB_STARTED_UTC=$([DateTime]::UtcNow.ToString('o'))" >> $env:GITHUB_ENV
if ($env:BONSAI_NATIVE_BUDGET_MINUTES -cnotin @('5', '10', '15')) { throw 'Select a predeclared 5, 10 or 15 minute owner budget.' }
if ($env:BONSAI_NATIVE_BUDGET_MINUTES -cne '5' -and [string]::IsNullOrWhiteSpace($env:BONSAI_NATIVE_BUDGET_REASON)) { throw 'An extended budget requires a fixed workload estimate and reason before setup.' }
if (([string]$env:BONSAI_NATIVE_BUDGET_REASON).Length -gt 512 -or $env:BONSAI_NATIVE_BUDGET_REASON -match '[\x00-\x1f\x7f]') { throw 'The budget reason must be at most 512 plain-text characters.' }
