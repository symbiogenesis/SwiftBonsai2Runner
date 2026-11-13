#Requires -Version 7.6
# Exercise build orchestration without compiling, provisioning or contacting CI.
[CmdletBinding()]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'Release-Common.ps1')
if (-not $IsWindows -or [Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne 'Arm64') { return }
$fixture = Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-Build-Controls-' + [guid]::NewGuid().ToString('N'))
$savedTarget = $env:CARGO_TARGET_DIR
try {
    foreach ($relative in @('scripts/release', '.github/release')) {
        [void][IO.Directory]::CreateDirectory((Join-Path $fixture $relative))
    }
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot '../Build-WindowsExecutable.ps1') -Destination (Join-Path $fixture 'scripts')
    '[workspace.package]
version = "0.2.2"' | Set-Content -LiteralPath (Join-Path $fixture 'Cargo.toml')
    'channel = "nightly-2026-11-12"' | Set-Content -LiteralPath (Join-Path $fixture 'rust-toolchain.toml')
    # Import real path checks, then replace only process execution in the copy.
    $common = (Join-Path $PSScriptRoot 'Release-Common.ps1').Replace("'", "''")
    ". '$common'" + @'

function Invoke-ReleaseCommand($FilePath, $ArgumentList, $TimeoutSeconds, $Environment) {
    if ($FilePath -ceq 'git') { return ('a' * 40) }
    $record = @{ command = $FilePath; arguments = $ArgumentList; environment = $Environment }
    if ($FilePath -ceq 'cargo') {
        $target = $ArgumentList[[array]::IndexOf($ArgumentList, '--target-dir') + 1]
        $release = if ($ArgumentList -ccontains '--target') { 'aarch64-pc-windows-msvc/release' } else { 'release' }
        $directory = Join-Path $target $release
        [void][IO.Directory]::CreateDirectory($directory)
        foreach ($name in @('bonsai-tools.exe', 'bonsai-launcher.exe')) {
            [IO.File]::WriteAllText((Join-Path $directory $name), 'fixture')
        }
    } else {
        $request = $ArgumentList[2] | ConvertFrom-Json
        $record.request = $request
        switch -Exact ($ArgumentList[0]) {
            'source-build' { $result = @{ ChatPath = 'chat'; BrokerPath = 'broker' } }
            'speech-build' {
                if ($request.operation -cne 'build') { throw 'Unexpected speech build operation.' }
                $result = @{ AsrPath = 'asr'; TtsPath = 'tts' }
            }
            'chat-package' { $result = @{ ManifestSha256 = ('b' * 64) } }
            default { throw 'Unexpected build command.' }
        }
    }
    $record | ConvertTo-Json -Depth 8 -Compress | Add-Content -LiteralPath (Join-Path $PSScriptRoot '../calls.jsonl')
    if ($FilePath -cne 'cargo') { $result | ConvertTo-Json -Compress }
}
'@ | Set-Content -LiteralPath (Join-Path $fixture 'scripts/release/Release-Common.ps1')
    @'
param($RepositoryRoot, $OutputPath)
if ($args.Count) { throw 'Notice collection must not require native sources.' }
[IO.File]::WriteAllText($OutputPath, 'verified notice fixture')
'@ | Set-Content -LiteralPath (Join-Path $fixture '.github/release/Collect-Notices.ps1')
    @'
param($PackageDirectory, $ManifestSha256, $OutputPath)
if ($ManifestSha256 -cne ('b' * 64)) { throw 'Package digest changed.' }
[IO.File]::WriteAllText($OutputPath, 'bundle fixture')
'@ | Set-Content -LiteralPath (Join-Path $fixture 'scripts/release/Build-WindowsExecutable.ps1')
    foreach ($mode in @('default', 'environment', 'explicit')) {
        $env:CARGO_TARGET_DIR = if ($mode -ceq 'default') { $null } else { Join-Path $fixture 'CI cache' }
        $parameters = @{ RepositoryRoot = $fixture; DataDirectory = (Join-Path $fixture $mode) }
        if ($mode -ceq 'explicit') { $parameters.TargetDirectory = Join-Path $fixture 'explicit cache' }
        $callsPath = Join-Path $fixture 'scripts/calls.jsonl'
        if (Test-Path -LiteralPath $callsPath) { Remove-Item -LiteralPath $callsPath }
        & (Join-Path $fixture 'scripts/Build-WindowsExecutable.ps1') @parameters
        $calls = @(Get-Content -LiteralPath $callsPath | ForEach-Object { $_ | ConvertFrom-Json })
        if ($calls.Count -ne 4) { throw 'Build duplicated preparation or skipped a required stage.' }
        $expected = switch ($mode) {
            'default' { Join-Path $parameters.DataDirectory 'build/maintenance' }
            'environment' { $env:CARGO_TARGET_DIR }
            'explicit' { $parameters.TargetDirectory }
        }
        $cargo = $calls[0]
        $targetIndex = [array]::IndexOf($cargo.arguments, '--target')
        if ($targetIndex -lt 0 -or $cargo.arguments[$targetIndex + 1] -cne 'aarch64-pc-windows-msvc' -or
            @($cargo.arguments | Where-Object { $_ -ceq '--target' }).Count -ne 1) {
            throw 'Every local build must isolate application CRT flags with one explicit ARM64 target.'
        }
        $actual = $cargo.arguments[[array]::IndexOf($cargo.arguments, '--target-dir') + 1]
        if ($actual -cne $expected -or $cargo.environment.CARGO_BUILD_FINGERPRINT -cne 'content' -or
            $cargo.environment.CARGO_UNSTABLE_CHECKSUM_FRESHNESS -cne 'true' -or
            $cargo.environment.CARGO_ENCODED_RUSTFLAGS -cne ('-C' + [char]0x1f + 'target-feature=+crt-static')) { throw 'Cargo cache or content freshness changed.' }
        foreach ($call in $calls[1..2]) {
            $selected = if ($mode -ceq 'default') { $null } else { $expected }
            if ($call.request.targetDirectory -cne $selected) { throw 'Build cache selection was not forwarded.' }
        }
        if (-not (Test-Path -LiteralPath (Join-Path $parameters.DataDirectory 'build/distribution/SwiftBonsai2Runner.exe'))) {
            throw 'No executable was packaged.'
        }
    }
    Write-Output 'Executable build controls passed: local defaults, CI cache, explicit override and one native preparation.'
} finally {
    $env:CARGO_TARGET_DIR = $savedTarget
    Remove-ReleaseDirectory -Path $fixture -ParentDirectory ([IO.Path]::GetTempPath()) -LeafName ([IO.Path]::GetFileName($fixture))
}
