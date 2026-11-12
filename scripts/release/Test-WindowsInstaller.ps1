#Requires -Version 7.0
# This installs software and belongs on a disposable hosted ARM64 runner only.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$InstallerPath,
    [Parameter(Mandatory)][string]$ExpectedVersion,
    [Parameter(Mandatory)][string]$EvidenceDirectory
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
if (-not $IsWindows -or [Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne 'Arm64' -or
    $env:RUNNER_ENVIRONMENT -cne 'github-hosted') {
    throw 'Installer smoke tests require a disposable GitHub-hosted Windows ARM64 runner.'
}
foreach ($path in @($InstallerPath, $EvidenceDirectory)) {
    if (-not [IO.Path]::IsPathFullyQualified($path)) { throw 'Smoke-test paths must be expanded and absolute.' }
}
[void][IO.Directory]::CreateDirectory($EvidenceDirectory)
$installer = New-Object -ComObject WindowsInstaller.Installer
$database = $installer.OpenDatabase($InstallerPath, 0)
function Read-MsiProperty([string]$Name) {
    if ($Name -cnotmatch '^[A-Za-z]+$') { throw 'Invalid MSI property name.' }
    $query = 'SELECT `Value` FROM `Property` WHERE `Property` = ''' + $Name + ''''
    $view = $database.OpenView($query)
    try {
        $view.Execute()
        $record = $view.Fetch()
        if ($null -eq $record) { throw "MSI property $Name is absent." }
        [string]$record.StringData(1)
    } finally { $view.Close() }
}
$productCode = Read-MsiProperty 'ProductCode'
if ((Read-MsiProperty 'ProductVersion') -cne $ExpectedVersion -or
    (Read-MsiProperty 'ProductName') -cne 'Swift Bonsai 2 Runner') { throw 'MSI product identity mismatch.' }
$summary = $database.SummaryInformation(0)
if ([string]$summary.Property(7) -cne 'Arm64;1033') { throw 'MSI is not a native ARM64 package.' }
[void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($summary)
[void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($database)
[void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($installer)
$installRoot = Join-Path $EvidenceDirectory 'installed'
if (Test-Path -LiteralPath $installRoot) { throw 'Smoke installation path already exists.' }
$userData = Join-Path $env:LOCALAPPDATA 'SwiftBonsai2'
[void][IO.Directory]::CreateDirectory($userData)
$sentinel = Join-Path $userData ('installer-retention-' + [guid]::NewGuid().ToString('N') + '.txt')
'Preserve user model and conversation storage across uninstall.' | Set-Content -LiteralPath $sentinel -Encoding utf8NoBOM
$sentinelHash = (Get-FileHash -LiteralPath $sentinel -Algorithm SHA256).Hash
function Invoke-Msi([string[]]$MsiArguments, [int[]]$AllowedExitCodes) {
    $process = Start-Process -FilePath (Join-Path $env:WINDIR 'System32/msiexec.exe') `
        -ArgumentList $MsiArguments -WindowStyle Hidden -PassThru
    if (-not $process.WaitForExit(180000)) {
        Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        throw 'Windows Installer exceeded the three-minute smoke-step deadline.'
    }
    if ($process.ExitCode -notin $AllowedExitCodes) { throw "Windows Installer failed with exit code $($process.ExitCode)." }
}
$attempted = $false
$checked = $false
try {
    $attempted = $true
    Invoke-Msi @('/i', ('"' + $InstallerPath + '"'), '/qn', '/norestart', '/l*v',
        ('"' + (Join-Path $EvidenceDirectory 'install.log') + '"'), ('INSTALLFOLDER="' + $installRoot + '"')) @(0, 3010)
    $trustedTool = Join-Path $installRoot 'maintenance/bonsai-tools.exe'
    $capabilities = (& $trustedTool --version | Out-String) | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0 -or 'installed-chat' -cnotin $capabilities.Commands) { throw 'Installed native launcher is unavailable.' }
    $request = @{operation = 'installed'; installationDirectory = $installRoot} | ConvertTo-Json -Compress
    $package = (& $trustedTool chat-package --json $request | Out-String) | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0 -or $package.Manifest.version -cne $ExpectedVersion -or
        $package.Manifest.target -cne 'aarch64-pc-windows-msvc' -or $package.Manifest.files.Count -ne 8 -or
        $package.Manifest.sourceRevision -cne $env:GITHUB_SHA) {
        throw 'Installed package failed native inventory and digest validation.'
    }
    if ((Get-FileHash -LiteralPath $trustedTool -Algorithm SHA256).Hash -cne
        (Get-FileHash -LiteralPath (Join-Path $package.Directory 'bin/bonsai-tools.exe') -Algorithm SHA256).Hash) {
        throw 'Installed trusted maintenance executable differs from the verified package copy.'
    }
    $package | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $EvidenceDirectory 'installed-package.json') -Encoding utf8NoBOM
    $installedLock = Join-Path $installRoot 'config/swift-assessment.lock.json'
    $repositoryLock = Join-Path $PSScriptRoot '../../config/swift-assessment.lock.json'
    if ((Get-FileHash -LiteralPath $installedLock -Algorithm SHA256).Hash -cne
        (Get-FileHash -LiteralPath $repositoryLock -Algorithm SHA256).Hash) { throw 'Installed Swift model lock changed.' }
    $shortcutPath = Join-Path ([Environment]::GetFolderPath('CommonPrograms')) 'Swift Bonsai 2 Runner.lnk'
    if (-not (Test-Path -LiteralPath $shortcutPath -PathType Leaf)) { throw 'Start Menu shortcut is missing.' }
    $shell = New-Object -ComObject WScript.Shell
    $shortcut = $shell.CreateShortcut($shortcutPath)
    # Advertised shortcuts resolve their target through Windows Installer; the
    # argument is still readable without starting the application or a model.
    if ($shortcut.Arguments -cne 'installed-chat') { throw 'Start Menu shortcut bypasses the verified native launcher.' }
    $checked = $true
} finally {
    if ($attempted) {
        Invoke-Msi @('/x', $productCode, '/qn', '/norestart', '/l*v',
            ('"' + (Join-Path $EvidenceDirectory 'uninstall.log') + '"')) @(0, 1605, 3010)
    }
}
if (-not $checked -or (Test-Path -LiteralPath (Join-Path $installRoot 'maintenance/bonsai-tools.exe')) -or
    (Test-Path -LiteralPath (Join-Path ([Environment]::GetFolderPath('CommonPrograms')) 'Swift Bonsai 2 Runner.lnk'))) {
    throw 'MSI did not remove its application and shortcut.'
}
if ((Get-FileHash -LiteralPath $sentinel -Algorithm SHA256).Hash -cne $sentinelHash) { throw 'Uninstall changed retained user data.' }
[ordered]@{
    schemaVersion = 1; version = $ExpectedVersion; productCode = $productCode
    nativeArm64Msi = $true; installedInventoryVerified = $true; shortcutVerified = $true
    uninstallVerified = $true; userDataRetained = $true
    modelStarted = $false; actualModelAcceptance = 'pending'; upgradeAcceptance = 'pending'
} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $EvidenceDirectory 'smoke-receipt.json') -Encoding utf8NoBOM
