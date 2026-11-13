#Requires -Version 7.6
<# Check workflow syntax and expressions without a compiler or optional script runtimes. #>
[CmdletBinding()]
param(
    [string]$Root = (Split-Path (Split-Path $PSScriptRoot -Parent) -Parent),
    [string]$ActionlintPath,
    [switch]$CheckFixtures,
    [string]$FixtureCase
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$version = '1.7.12'
# Official v1.7.12 release assets. Update version and digests together after review:
# https://github.com/rhysd/actionlint/releases/tag/v1.7.12
$assets = @{
    'Linux-Arm64' = @('linux_arm64.tar.gz', '325e971b6ba9bfa504672e29be93c24981eeb1c07576d730e9f7c8805afff0c6')
    'Linux-X64' = @('linux_amd64.tar.gz', '8aca8db96f1b94770f1b0d72b6dddcb1ebb8123cb3712530b08cc387b349a3d8')
    'Windows-Arm64' = @('windows_arm64.zip', 'cadcf7ea4efe3a68728893813643cebe1185e5b1d4be5b96245f65c9a4d5ea41')
    'Windows-X64' = @('windows_amd64.zip', '6e7241b51e6817ea6a047693d8e6fed13b31819c9a0dd6c5a726e1592d22f6e9')
}
. (Join-Path $PSScriptRoot 'check_workflow_startup.ps1') -Root $Root
$scratch = $null
try {
    if (-not $ActionlintPath) {
        $platform = if ($IsWindows) { 'Windows' } elseif ($IsLinux) { 'Linux' } else { throw 'Supply -ActionlintPath on this platform.' }
        $key = "$platform-$([Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture)"
        if (-not $assets.ContainsKey($key)) { throw "Supply -ActionlintPath for unsupported architecture: $key" }
        $asset, $digest = $assets[$key]
        $scratch = Join-Path ([IO.Path]::GetTempPath()) ('bonsai-actionlint-' + [guid]::NewGuid().ToString('N'))
        [void][IO.Directory]::CreateDirectory($scratch)
        $archive = Join-Path $scratch $asset
        # Retry only the immutable download, never a failed check. Each attempt
        # has its own short network limit and the retry count is finite.
        Invoke-WebRequest -Uri "https://github.com/rhysd/actionlint/releases/download/v$version/actionlint_${version}_$asset" -OutFile $archive -ConnectionTimeoutSeconds 10 -OperationTimeoutSeconds 20 -MaximumRetryCount 2 -RetryIntervalSec 1
        if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() -cne $digest) { throw 'Pinned actionlint archive checksum differs; refusing to execute it.' }
        $executable = if ($IsWindows) { 'actionlint.exe' } else { 'actionlint' }
        if ($IsWindows) { Expand-Archive -LiteralPath $archive -DestinationPath $scratch }
        else { tar -xzf $archive -C $scratch $executable }
        $ActionlintPath = Join-Path $scratch $executable
    }
    # Explicit external paths also support an offline local check.
    $ActionlintPath = (Get-Command $ActionlintPath -CommandType Application -ErrorAction Stop).Source
    $identity = @(& $ActionlintPath -version)
    if ($identity[0] -cne $version) { throw "Expected actionlint $version; received $($identity[0])." }
    $workflows = @(Get-ChildItem -LiteralPath (Join-Path $Root '.github/workflows') -File | Where-Object { $_.Extension -cin @('.yml', '.yaml') } | Sort-Object Name | ForEach-Object FullName)
    if (-not $workflows.Count) { throw 'No workflow manifests found.' }
    Push-Location -LiteralPath $Root
    try {
        # Keep validation identical across hosts; neither optional linter is
        # installed or invoked. Our Rust/PowerShell source policy is separate.
        foreach ($workflow in $workflows) {
            # v1.7.12 predates GitHub's $/ same-repository action notation.
            # Normalize only uses scalars in memory to its equivalent ./ form;
            # the separate offline startup guard owns original paths and pins.
            # Preserve the real filename so local action inputs/config still resolve.
            $lines = [IO.File]::ReadAllLines($workflow)
            foreach ($reference in @(Read-StartupReferences $workflow)) {
                if ($reference.Value.StartsWith('$/', [StringComparison]::Ordinal)) {
                    $index = $reference.Line - 1
                    $lines[$index] = [regex]::Replace($lines[$index], '^(\s*(?:-\s*)?(?:uses|''uses''|"uses"):\s*[''"]?)\$/', '${1}./')
                }
            }
            $source = $lines -join "`n"
            $source | & $ActionlintPath -shellcheck= -pyflakes= -no-color -stdin-filename $workflow -
        }
    } finally { Pop-Location }
    "Workflow syntax policy passed: $($workflows.Count) manifests (actionlint $version)."
    if ($CheckFixtures) {
        & (Join-Path $PSScriptRoot 'test_workflow_syntax.ps1') -ActionlintPath $ActionlintPath -Case $FixtureCase
    }
} finally {
    if ($scratch) {
        $temporary = [IO.Path]::TrimEndingDirectorySeparator([IO.Path]::GetFullPath([IO.Path]::GetTempPath())) + [IO.Path]::DirectorySeparatorChar
        $absolute = [IO.Path]::GetFullPath($scratch)
        if (-not $absolute.StartsWith($temporary, [StringComparison]::OrdinalIgnoreCase) -or -not [IO.Path]::GetFileName($absolute).StartsWith('bonsai-actionlint-', [StringComparison]::Ordinal)) { throw 'Workflow linter cleanup escaped its temporary directory.' }
        Remove-Item -LiteralPath $absolute -Recurse -Force
    }
}
