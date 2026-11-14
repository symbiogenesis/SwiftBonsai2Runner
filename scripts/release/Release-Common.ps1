#Requires -Version 7.6
# Shared native release entry adapter. Importing performs no work.
Set-StrictMode -Version Latest
function Invoke-NativeReleasePolicy([hashtable]$Request) {
    $json = ConvertTo-Json -InputObject $Request -Depth 100 -Compress
    $nativePreference = $PSNativeCommandUseErrorActionPreference
    try {
        $PSNativeCommandUseErrorActionPreference = $false
        if ($env:BONSAI_TOOLS_PATH) {
            $output = @($json | & $env:BONSAI_TOOLS_PATH release-tool --request - 2>&1)
        } else {
            $manifest = Join-Path $PSScriptRoot '../../Cargo.toml'
            $output = @($json | & cargo run --quiet --locked --manifest-path $manifest -p bonsai-tools --bin bonsai-tools -- release-tool --request - 2>&1)
        }
        $code = $LASTEXITCODE
    } finally { $PSNativeCommandUseErrorActionPreference = $nativePreference }
    $raw = ($output | ForEach-Object { $_.ToString() }) -join "`n"
    if ($code -ne 0) { throw $raw }
    # Native stderr remains ErrorRecord data after 2>&1; compiler/rustup
    # diagnostics must stay visible without becoming part of the JSON reply.
    $stdout = @($output | ForEach-Object {
        if ($_ -is [Management.Automation.ErrorRecord]) {
            [Console]::Error.WriteLine($_.ToString())
        } else {
            $_.ToString()
        }
    }) -join "`n"
    ConvertFrom-Json -InputObject $stdout -NoEnumerate
}
function Resolve-ReleasePath {
    [CmdletBinding()]
    [OutputType([string])]
    param([Parameter(Mandatory)][string]$Path,
        [ValidateSet('File','Directory')][string]$Kind='Directory', [switch]$AllowMissing)
    Invoke-NativeReleasePolicy @{operation='path';path=$Path;directory=($Kind -ceq 'Directory');allowMissing=[bool]$AllowMissing}
}
function Remove-ReleaseDirectory {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$Path,[Parameter(Mandatory)][string]$ParentDirectory,
        [Parameter(Mandatory)][string]$LeafName,[int]$TimeoutMilliseconds=5000,
        [Management.Automation.ErrorRecord]$PrimaryFailure)
    try {
        $null = Invoke-NativeReleasePolicy @{operation='cleanup';path=$Path;parentDirectory=$ParentDirectory;leafName=$LeafName;timeoutMilliseconds=$TimeoutMilliseconds}
    } catch {
        if ($null -ne $PrimaryFailure) {
            $PrimaryFailure.Exception.Data['ReleaseCleanupFailure']=$_.Exception.Message
            [Console]::Error.WriteLine("Release cleanup also failed: $($_.Exception.Message)")
            throw $PrimaryFailure
        }
        throw
    }
}
function Invoke-ReleaseCommand {
    [CmdletBinding()]
    [OutputType([string])]
    param([Parameter(Mandatory)][string]$FilePath,[string[]]$ArgumentList=@(),
        [int]$TimeoutSeconds=180,[switch]$BuildCommand,[hashtable]$Environment=@{})
    $result = Invoke-NativeReleasePolicy @{operation='invoke';filePath=$FilePath;arguments=@($ArgumentList);timeoutSeconds=$TimeoutSeconds;buildCommand=[bool]$BuildCommand;environment=$Environment}
    if ($result.stderr) { [Console]::Error.Write($result.stderr) }
    $result.stdout
}
