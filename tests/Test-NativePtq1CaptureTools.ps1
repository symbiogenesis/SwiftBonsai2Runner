#requires -Version 7.6
<# .SYNOPSIS
Checks capture preparation and the Rust collector's source/ABI admission boundary.
#>
[CmdletBinding()]
[OutputType([string])]
param([Parameter(Mandatory)][ValidateSet('HostedGuard', 'CollectorControls', 'BuildEvidence', 'CapturePolicy')][string]$Case)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.TestSupport.psm1')
if ($Case -ceq 'CapturePolicy') {
    Import-Module (Join-Path $PSScriptRoot '../scripts/Bonsai.Ptq1.psm1')
    $repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
    $pins = Get-Ptq1CapturePin -RepositoryDirectory $repo
    foreach ($backend in @('auto', 'scalar', 'neon')) {
        $plan = & (Join-Path $repo 'scripts/Invoke-NativePtq1Numerics.ps1') -DataDirectory $repo -PlanOnly -RustBackend $backend
        Assert ($plan.RustBackend -ceq $backend -and $plan.NativeSamples -eq 1 -and $plan.RustSamples -eq 1 -and
            $plan.WorkBudgetSeconds -eq 240 -and $plan.WholeJobMinutes -eq 5 -and -not $plan.DefaultMayChange) 'Backend selection changed the bounded capture workload or promotion boundary.'
    }
    Assert-Failure { & (Join-Path $repo 'scripts/Invoke-NativePtq1Numerics.ps1') -DataDirectory $repo -PlanOnly -RustBackend 'invalid' }
    Assert-Failure { & (Join-Path $repo 'scripts/Invoke-NativePtq1Numerics.ps1') -DataDirectory $repo -PlanOnly -RustBackend 'AUTO' }
    $tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
    $fixture = [IO.Path]::GetFullPath((Join-Path $tempRoot ('Bonsai-Ptq1CapturePolicy-' + [guid]::NewGuid().ToString('N'))))
    [void][IO.Directory]::CreateDirectory((Join-Path $fixture 'tools/native-reference'))
    [void][IO.Directory]::CreateDirectory((Join-Path $fixture 'config'))
    try {
        $numericsPath = Join-Path $fixture 'numerics.json'
        $observations = [ordered]@{ Status = 'preparing'; ModelTrialsRequested = $false; Phase = 'capture-tool-intake' }
        Save-Ptq1NumericsReport -Report $observations -Path $numericsPath
        $observations.Status = 'failed'
        Save-Ptq1NumericsReport -Report $observations -Path $numericsPath
        $failed = Get-Content -LiteralPath $numericsPath -Raw | ConvertFrom-Json -DateKind String
        Assert ($failed.Status -ceq 'failed' -and -not $failed.ModelTrialsRequested -and
            $failed.Phase -ceq 'capture-tool-intake') 'Pre-model rejection lost its phase or model-execution boundary.'
        $retained = [IO.File]::ReadAllText($numericsPath)
        $observations['Failure'] = 'x' * 1048577
        Assert-Failure { Save-Ptq1NumericsReport -Report $observations -Path $numericsPath }
        Assert ([IO.File]::ReadAllText($numericsPath) -ceq $retained) 'Rejected receipt overwrote the last complete observations.'
        $observations.Remove('Failure')
        Assert-Failure { Save-Ptq1NumericsReport -Report $observations -Path (Join-Path $fixture 'config') }
        Assert (@(Get-ChildItem -LiteralPath $fixture -Filter '*.tmp').Count -eq 0) 'Failed receipt replacement left an unselected temporary file.'
        $lockPath = Join-Path $fixture 'config/prism-inference.lock.json'
        $pinsPath = Join-Path $fixture 'tools/native-reference/pins.json'
        Copy-Item -LiteralPath (Join-Path $repo 'config/prism-inference.lock.json') -Destination $lockPath
        $original = $pins | ConvertTo-Json -Depth 12
        foreach ($field in @('commit', 'url', 'bytes', 'sha256')) {
            $changed = $original | ConvertFrom-Json -DateKind String
            $changed.rustSource.$field = if ($field -ceq 'bytes') { $changed.rustSource.bytes + 1 } else { 'stale' }
            $changed | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $pinsPath -Encoding utf8NoBOM
            $failure = Assert-Failure -PassThru { Get-Ptq1CapturePin -RepositoryDirectory $fixture }
            Assert ($failure -like '*differs from the pinned Rust runtime*') 'Historical or changed numerical source was accepted.'
        }
        [IO.File]::WriteAllText($pinsPath, $original)
        $lock = Get-Content -LiteralPath $lockPath -Raw | ConvertFrom-Json -DateKind String
        $lock.commit = 'stale'
        $lock | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $lockPath -Encoding utf8NoBOM
        Assert-Failure { Get-Ptq1CapturePin -RepositoryDirectory $fixture }
        foreach ($allowance in @(0, -1, 241)) {
            $failure = Assert-Failure -PassThru {
                & (Join-Path $repo 'scripts/Invoke-NativePtq1Numerics.ps1') -DataDirectory $fixture -WorkBudgetSeconds $allowance
            }
            Assert ($failure -like '*remaining complete owner-job work allowance*') 'Invalid work allowance reached owner/model preparation.'
        }
        $failure = Assert-Failure -PassThru {
            & (Join-Path $repo 'scripts/Invoke-NativePtq1Numerics.ps1') -DataDirectory $fixture -WorkBudgetSeconds 240
        }
        Assert ($failure -like '*original monotonic owner-job work deadline*') 'A fresh child-relative deadline was accepted.'
        $tokens = $null; $parseErrors = $null
        $ast = [Management.Automation.Language.Parser]::ParseFile((Join-Path $repo 'scripts/Invoke-NativePtq1Numerics.ps1'), [ref]$tokens, [ref]$parseErrors)
        Assert ($parseErrors.Count -eq 0) 'Capture wrapper did not parse.'
        $function = $ast.Find({ param($node)
            $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -ceq 'Get-Ptq1WorkAllowance'
        }, $true)
        Assert ($null -ne $function) 'The original-deadline allowance check was not found.'
        . ([scriptblock]::Create($function.Extent.Text))
        # Supply the extracted production helper's dynamic-scope inputs explicitly.
        Set-Variable -Name WorkBudgetSeconds -Value 240 -Scope Local
        Set-Variable -Name WorkDeadlineTick -Value ([Diagnostics.Stopwatch]::GetTimestamp() - 1) -Scope Local
        $clock = [Diagnostics.Stopwatch]::StartNew()
        Assert-Failure { Get-Ptq1WorkAllowance $clock }
        Set-Variable -Name WorkDeadlineTick -Value ([Diagnostics.Stopwatch]::GetTimestamp() + (10 * [Diagnostics.Stopwatch]::Frequency)) -Scope Local
        $remaining = Get-Ptq1WorkAllowance $clock
        Assert ($remaining -ge 1 -and $remaining -le 10) 'Child startup reset the original work deadline.'
    } finally {
        $prefix = $tempRoot.TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
        if (-not $fixture.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase) -or $fixture -ceq $tempRoot) {
            throw 'Refusing cleanup outside the generated temporary test directory.'
        }
        Remove-Item -LiteralPath $fixture -Recurse -Force
    }
    return "Focused $Case capture preparation checks passed."
}
if ($Case -ceq 'BuildEvidence') {
    $tokens = $null; $parseErrors = $null
    $ast = [Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/Build-NativePtq1CaptureTools.ps1'), [ref]$tokens, [ref]$parseErrors)
    Assert ($parseErrors.Count -eq 0) 'Capture builder did not parse.'
    $function = $ast.Find({ param($node)
        $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -ceq 'Save-Ptq1BuildEvidence'
    }, $true)
    Assert ($null -ne $function) 'The bounded evidence writer was not found.'
    . ([scriptblock]::Create($function.Extent.Text))
    $tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
    $scratch = [IO.Path]::GetFullPath((Join-Path $tempRoot ('Bonsai-Ptq1BuildEvidence-' + [guid]::NewGuid().ToString('N'))))
    $evidence = Join-Path $scratch 'evidence'
    [void][IO.Directory]::CreateDirectory($evidence)
    try {
        $buildReceipt = [ordered]@{ RunnerRevision = ('a' * 40); Status = 'failed'; Phase = 'collector-build-and-tests'; ModelProcessStarted = $false }
        [IO.File]::WriteAllText((Join-Path $scratch 'collector-build.txt'), ('x' * 70000) + 'tail')
        [IO.File]::WriteAllText((Join-Path $scratch 'collector-tests.txt'), 'seven model-free controls passed')
        [IO.File]::WriteAllText((Join-Path $scratch 'unselected.txt'), 'must not be uploaded')
        Save-Ptq1BuildEvidence
        $tail = [IO.File]::ReadAllText((Join-Path $evidence 'collector-build.txt'))
        Assert ($tail.Length -eq 65536 -and $tail.EndsWith('tail')) 'Compiler evidence did not preserve its bounded tail.'
        Assert ([IO.File]::ReadAllText((Join-Path $evidence 'collector-tests.txt')) -ceq 'seven model-free controls passed') 'Small collector-test evidence changed.'
        Assert (-not [IO.File]::Exists((Join-Path $evidence 'unselected.txt'))) 'Unselected scratch data entered the retained evidence.'
        $actual = Get-Content -LiteralPath (Join-Path $evidence 'build-receipt.json') -Raw | ConvertFrom-Json -DateKind String
        Assert ($actual.RunnerRevision -ceq ('a' * 40) -and $actual.Status -ceq 'failed' -and
            $actual.Phase -ceq 'collector-build-and-tests' -and -not $actual.ModelProcessStarted) 'Failure receipt lost its identity or scope.'
        $buildReceipt.Failure = 'x' * 65537
        Assert-Failure { Save-Ptq1BuildEvidence }
    } finally {
        $prefix = $tempRoot.TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
        if (-not $scratch.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase) -or $scratch -ceq $tempRoot) {
            throw 'Refusing cleanup outside the generated temporary test directory.'
        }
        Remove-Item -LiteralPath $scratch -Recurse -Force
    }
    return "Focused $Case capture preparation checks passed."
}
if ($Case -ceq 'CollectorControls') {
    Import-Module (Join-Path $PSScriptRoot '../scripts/Bonsai.Tools.psm1')
    $tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
    $working = [IO.Path]::GetFullPath((Join-Path $tempRoot ('Bonsai-Ptq1CollectorControls-' + [guid]::NewGuid().ToString('N'))))
    $fixtureDirectory = Join-Path $working 'tools/native-reference/fixtures'
    [void][IO.Directory]::CreateDirectory($fixtureDirectory)
    try {
        $source = Join-Path $working 'tools/native-reference/swift_ptq1_reference.rs'
        $expected = Join-Path $fixtureDirectory 'collector-abi-v1.txt'
        $provenance = Join-Path $fixtureDirectory 'collector-abi-v1.provenance.json'
        $actual = Join-Path $working 'actual.txt'
        [IO.File]::WriteAllText($source, 'reviewed Rust collector source')
        [IO.File]::WriteAllText($expected, "model.size=80`nbatch.size=56`nsigned=18446744073709551615`n")
        [IO.File]::WriteAllText($provenance, '{"schema_version":1}')
        [IO.File]::WriteAllText($actual, "signed=18446744073709551615`nbatch.size=56`nmodel.size=80`n")
        $pins = Get-Content -LiteralPath (Join-Path $PSScriptRoot '../tools/native-reference/pins.json') -Raw | ConvertFrom-Json -DateKind String
        $receipt = [ordered]@{
            SchemaVersion = 2; RunnerRevision = ('a' * 40); CollectorLanguage = 'rust'
            CollectorSourcePath = 'tools/native-reference/swift_ptq1_reference.rs'
            RustToolchain = 'nightly-2026-10-22-aarch64-pc-windows-msvc'
            NativeSource = $pins.nativeSource; RustSource = $pins.rustSource
            CollectorSourceSha256 = (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash.ToLowerInvariant()
            AbiControlSha256 = (Get-FileHash -LiteralPath $expected -Algorithm SHA256).Hash.ToLowerInvariant()
            AbiProvenanceSha256 = (Get-FileHash -LiteralPath $provenance -Algorithm SHA256).Hash.ToLowerInvariant()
            Files = @(@{ Name = 'swift-ptq1-reference.exe' }, @{ Name = 'swift_model_check.exe' })
        }
        $receiptPath = Join-Path $working 'capture-tools.json'
        $reviewedJson = $receipt | ConvertTo-Json -Depth 12
        [IO.File]::WriteAllText($receiptPath, $reviewedJson)
        $accepted = Invoke-BonsaiTool -Command ptq1-capture -Request @{ operation='receipt'; bundleDirectory=$working; repositoryDirectory=$working; revision=('a' * 40); pins=$pins }
        Assert ($accepted.CollectorLanguage -ceq 'rust') 'The reviewed Rust collector receipt was rejected.'
        Invoke-BonsaiTool -Command ptq1-capture -Request @{ operation='compare-abi'; actualPath=$actual; expectedPath=$expected; expectedSha256=$receipt.AbiControlSha256 }
        foreach ($mutation in @(
            @{ SchemaVersion = 1 }, @{ CollectorLanguage = 'cpp' }, @{ RunnerRevision = ('b' * 40) },
            @{ CollectorSourceSha256 = ('0' * 64) }, @{ AbiControlSha256 = ('0' * 64) }, @{ AbiProvenanceSha256 = ('0' * 64) }
        )) {
            $changed = $reviewedJson | ConvertFrom-Json -DateKind String
            foreach ($key in $mutation.Keys) { $changed.$key = $mutation[$key] }
            [IO.File]::WriteAllText($receiptPath, ($changed | ConvertTo-Json -Depth 12))
            $failure = Assert-Failure -PassThru {
                Invoke-BonsaiTool -Command ptq1-capture -Request @{ operation='receipt'; bundleDirectory=$working; repositoryDirectory=$working; revision=('a' * 40); pins=$pins }
            }
            Assert ($failure -like '*reviewed source/revision pins*') 'An unreviewed collector receipt was accepted.'
        }
        foreach ($text in @(
            "model.size=80`nbatch.size=56`n", # omission
            "model.size=80`nbatch.size=56`nsigned=18446744073709551615`nextra=1`n", # extra
            "model.size=81`nbatch.size=56`nsigned=18446744073709551615`n", # changed value
            "model.size=80`nbatch.size=56`nsigned=18446744073709551615`nmodel.size=80`n", # duplicate
            "model.size=80`nbatch.size=56`nsigned=18446744073709551616`n", # overflow
            "model.size=80`nbatch.size=56`nsigned=18446744073709551615`ntail", # malformed tail
            ('x' * 65537)
        )) {
            [IO.File]::WriteAllText($actual, $text)
            Assert-Failure { Invoke-BonsaiTool -Command ptq1-capture -Request @{ operation='compare-abi'; actualPath=$actual; expectedPath=$expected; expectedSha256=$receipt.AbiControlSha256 } }
        }
        [IO.File]::WriteAllText($actual, "model.size=81`nbatch.size=56`nsigned=18446744073709551615`n")
        [IO.File]::Copy($actual, $expected, $true)
        $failure = Assert-Failure -PassThru { Invoke-BonsaiTool -Command ptq1-capture -Request @{ operation='compare-abi'; actualPath=$actual; expectedPath=$expected; expectedSha256=$receipt.AbiControlSha256 } }
        Assert ($failure -like '*control bytes differ*') 'Changed control bytes were accepted after receipt validation.'
    } finally {
        $prefix = $tempRoot.TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
        if (-not $working.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase) -or $working -ceq $tempRoot) {
            throw 'Refusing cleanup outside the generated temporary test directory.'
        }
        Remove-Item -LiteralPath $working -Recurse -Force
    }
    return "Focused $Case capture preparation checks passed."
}
$script = Join-Path $PSScriptRoot '../scripts/Build-NativePtq1CaptureTools.ps1'
$output = Join-Path ([IO.Path]::GetTempPath()) ('Bonsai-HostedCaptureGuard-' + [guid]::NewGuid().ToString('N'))
$savedActions = $env:GITHUB_ACTIONS
$savedEnvironment = $env:RUNNER_ENVIRONMENT
try {
    foreach ($environment in @('local', 'self-hosted')) {
        $env:GITHUB_ACTIONS = if ($environment -ceq 'local') { 'false' } else { 'true' }
        $env:RUNNER_ENVIRONMENT = if ($environment -ceq 'local') { 'github-hosted' } else { 'self-hosted' }
        $failure = Assert-Failure -PassThru { & $script -OutputDirectory $output }
        Assert ($failure -like '*hosted native Windows ARM64 CI*') 'Capture build did not reject the non-hosted environment.'
        Assert (-not (Test-Path -LiteralPath $output)) 'Rejected capture preparation created an output directory.'
    }
} finally {
    [Environment]::SetEnvironmentVariable('GITHUB_ACTIONS', $savedActions)
    [Environment]::SetEnvironmentVariable('RUNNER_ENVIRONMENT', $savedEnvironment)
}
"Focused $Case capture preparation checks passed."
