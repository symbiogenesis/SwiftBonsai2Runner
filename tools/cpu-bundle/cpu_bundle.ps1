#requires -Version 7.6
<# Native hosted preparation and offline admission for one fixed CPU ABBA comparison. #>
[CmdletBinding()]
param([ValidateSet('prepare','evaluate')][string]$Operation,
    [string]$BaselineRoot, [string]$ToolingRoot, [ValidateSet('0.1.25')][string]$CandidateRegistryVersion,
    [string]$RunnerRevision, [string]$Output, [string]$Preparation, [string]$Owner)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$script:Baseline = '055c7430cb4771dfddb24ddaa30494af6ad182da'
$script:Channel = 'nightly-2026-11-09'; $script:Target = 'aarch64-pc-windows-msvc'
$script:Features = @('cli','experimental-neon','parallel-rows')
$script:Phases = @('source_analysis','portable_library','state_fixture','collector_analysis','collector_controls','collector_link')
$script:StateControl = 'graph_parallel_rows_preserve_logits_checkpoints_and_recovery'
$script:CollectorControls = @(
    'tests::completion_requires_matching_terminal_runtime_counts_and_nonempty_output',
    'tests::deadlines_require_original_bounded_absolute_time',
    'tests::failure_receipt_retains_provisional_output_without_committing_or_overwriting',
    'tests::first_text_event_timing_ignores_empty_failed_and_subsequent_deliveries',
    'tests::output_rejects_overflow_and_any_delivery_after_terminal',
    'tests::records_hash_exact_utf8_and_keep_unentered_counts_unknown',
    'tests::selection_rejects_relative_paths_bad_commit_and_output_overflow')
$script:Inventories = @{state_fixture=@($script:StateControl); collector_controls=$script:CollectorControls}
$script:Schedule = @('baseline','candidate','candidate','baseline')
$script:Messages = @(@{role='user'; content='Count from 1 to 10. Answer with only the numbers.'})
$script:Profile = @{adapter_scale_bits='3fa66666'; row_workers=4; backend_requested='ExperimentalNeon'; execution_device='cpu'
    execution_basis='direct-session-without-projection-executor'; backend_statistics=$null; kv_cache='f16'; prompt_reuse='disabled'
    seed=42; thinking_enabled=$false; maximum_prompt_tokens=128}
$script:Model = '5912bb739217cf25b4283a098c7b893d9baff36bdf2e72d3dbf4cb999d5633d5'
$script:Adapter = 'f1669534803d340a496015f5c45125f3437b4d13ec764f40e34488ce83967f42'
$script:RegistryRevision = '99bab71270b1e6a6fb6d47eec2e34b8097cdcd4c'
$script:RegistryPackages = @{
    'prism-inference'=@('0.1.25','0a117db7589a821b27c224e41aaababa5ddd2ac9852344c0a8121a0849046363')
    'bonsai-neon-dot'=@('0.1.2','f932c3309ef3645b055b91bdd9ee6986b7ef98c24bc1831701138e3ab9d3a31b')
    'bonsai-native-hash'=@('0.1.3','315284b1f3fc7e98f2cb0f749e63690853f1c441d2b3a3eec2ad68abe45180a2')
    'bonsai-ternary'=@('0.1.3','f6ea8b95000a131bb48e27453b776e9e42096194648a3a8ebde782d5cfe9f604')
    'bonsai-gguf'=@('0.1.5','fdd15a1ed0911fca0d987c349c3ffa02078b729b05148f46e78a285f89189b4b')
    'bonsai-prism'=@('0.1.5','9ecabe1b5c626d4ceaef666b27f11c5a906dea56234252d3cbb8f46125d976ea')}
function Require($Condition,[string]$Message) { if (-not $Condition) { throw $Message } }
function Get-Field($Value,[string]$Name) { if ($Value -is [Collections.IDictionary] -and $Value.Contains($Name)) { return ,$Value[$Name] }; $null }
function Equal($First,$Second) {
    if ($null -eq $First -or $null -eq $Second) { return $null -eq $First -and $null -eq $Second }
    if ($First -is [Collections.IDictionary] -and $Second -is [Collections.IDictionary]) {
        if ($First.Count -ne $Second.Count) { return $false }
        foreach ($key in $First.Keys) { if (-not $Second.Contains($key) -or -not (Equal $First[$key] $Second[$key])) { return $false } }; return $true
    }
    if ($First -is [array] -and $Second -is [array]) {
        if ($First.Count -ne $Second.Count) { return $false }; for ($i=0;$i -lt $First.Count;$i++) { if (-not (Equal $First[$i] $Second[$i])) { return $false } }; return $true
    }
    if ($First -is [bool] -or $Second -is [bool] -or $First -is [string] -or $Second -is [string]) { return $First.GetType() -eq $Second.GetType() -and $First -ceq $Second }
    $firstInteger=$First -is [int] -or $First -is [long] -or $First -is [System.Numerics.BigInteger]
    $secondInteger=$Second -is [int] -or $Second -is [long] -or $Second -is [System.Numerics.BigInteger]
    if ($firstInteger -or $secondInteger) { return $firstInteger -and $secondInteger -and $First -eq $Second }
    return $First.GetType() -eq $Second.GetType() -and $First -eq $Second
}
function Integer($Value,[decimal]$Minimum,[decimal]$Maximum,[string]$Name) {
    Require (($Value -is [int] -or $Value -is [long] -or $Value -is [System.Numerics.BigInteger]) -and $Value -ge $Minimum -and $Value -le $Maximum) "Invalid $Name."
    $Value
}
function Positive($Value,[string]$Name) {
    Require (($Value -is [int] -or $Value -is [long] -or $Value -is [double]) -and [double]::IsFinite($Value) -and $Value -gt 0) "Missing positive finite $Name."
    [double]$Value
}
function Hex($Value,[int]$Size,[string]$Name) { Require ($Value -is [string] -and $Value -cmatch "^[0-9a-f]{$Size}$") "Invalid $Name."; $Value }
function Digest([byte[]]$Bytes) { [Convert]::ToHexStringLower([Security.Cryptography.SHA256]::HashData($Bytes)) }
function FileDigest([string]$Path) { (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant() }
function Check-JsonElement([System.Text.Json.JsonElement]$Element) {
    if ($Element.ValueKind -eq [System.Text.Json.JsonValueKind]::Object) {
        $keys = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
        foreach ($property in $Element.EnumerateObject()) { Require ($keys.Add($property.Name)) "Duplicate JSON field: $($property.Name)"; Check-JsonElement $property.Value }
    } elseif ($Element.ValueKind -eq [System.Text.Json.JsonValueKind]::Array) { foreach ($child in $Element.EnumerateArray()) { Check-JsonElement $child } }
    elseif ($Element.ValueKind -eq [System.Text.Json.JsonValueKind]::Number) { Require ([double]::IsFinite($Element.GetDouble())) 'Nonfinite JSON number.' }
}
function ReadJson([string]$Path) {
    $item = Get-Item -LiteralPath $Path -Force
    Require (-not $item.PSIsContainer -and -not ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -and $item.Length -le 4194304) 'Missing or oversized JSON proof.'
    $stream = [IO.File]::OpenRead($Path)
    try { Require ($stream.Length -le 4194304) 'Oversized JSON proof.'; $bytes=[byte[]]::new($stream.Length); $stream.ReadExactly($bytes); Require ($stream.ReadByte() -eq -1) 'JSON proof grew.' } finally { $stream.Dispose() }
    $text = [Text.UTF8Encoding]::new($false,$true).GetString($bytes).TrimStart([char]0xfeff)
    $document = [System.Text.Json.JsonDocument]::Parse($text)
    try { Require ($document.RootElement.ValueKind -eq [System.Text.Json.JsonValueKind]::Object) 'Expected a JSON object.'; Check-JsonElement $document.RootElement } finally { $document.Dispose() }
    ConvertFrom-Json -InputObject $text -AsHashtable
}
function WriteNew([string]$Path,$Value) {
    $bytes=[Text.UTF8Encoding]::new($false).GetBytes((ConvertTo-Json -InputObject $Value -Depth 70) + "`n")
    $file=[IO.File]::Open($Path,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None)
    try { $file.Write($bytes) } finally { $file.Dispose() }
}
function RejectRedirects([string]$Path) {
    $current=[IO.Path]::GetFullPath($Path); $depth=0
    while ($current) {
        $depth++; Require ($depth -le 65) 'Path ancestry exceeds bound.'
        if (Test-Path -LiteralPath $current) { Require (-not ([IO.File]::GetAttributes($current) -band [IO.FileAttributes]::ReparsePoint)) 'Redirected evidence/source path.' }
        $current=[IO.Path]::GetDirectoryName($current)
    }
}
function Member([string]$Root,$Relative) {
    Require ($Relative -is [string] -and $Relative -and -not [IO.Path]::IsPathRooted($Relative) -and $Relative -notmatch '[:\\]') 'Evidence member must be relative.'
    $prefix=[IO.Path]::GetFullPath($Root).TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
    $path=[IO.Path]::GetFullPath((Join-Path $Root $Relative))
    Require ($path.StartsWith($prefix,[StringComparison]::Ordinal)) 'Evidence member escapes its root.'; RejectRedirects $path; $path
}
function CheckedJson([string]$Root,$Reference) {
    Require ($Reference -is [Collections.IDictionary] -and (Equal @($Reference.Keys | Sort-Object) @('path','sha256'))) 'Expected a digest-bound JSON reference.'
    $path=Member $Root $Reference.path
    Require ((FileDigest $path) -ceq (Hex $Reference.sha256 64 'proof digest')) 'Evidence member digest differs.'; ReadJson $path
}
function Command([string[]]$Arguments,[string]$Directory,[hashtable]$Environment=@{}) {
    $info=[Diagnostics.ProcessStartInfo]::new(); $info.FileName=$Arguments[0]; $info.WorkingDirectory=$Directory; $info.UseShellExecute=$false; $info.RedirectStandardOutput=$true; $info.RedirectStandardError=$true
    foreach ($argument in $Arguments[1..($Arguments.Count-1)]) { $info.ArgumentList.Add($argument) }
    foreach ($key in $Environment.Keys) { if ($null -eq $Environment[$key]) { [void]$info.Environment.Remove($key) } else { $info.Environment[$key]=$Environment[$key] } }
    $process=[Diagnostics.Process]::new(); $process.StartInfo=$info
    try { [void]$process.Start(); $stdout=$process.StandardOutput.ReadToEndAsync(); $stderr=$process.StandardError.ReadToEndAsync(); $process.WaitForExit(); $out=$stdout.GetAwaiter().GetResult(); $err=$stderr.GetAwaiter().GetResult(); Require ($process.ExitCode -eq 0) "Command failed ($($process.ExitCode)): $($Arguments -join ' ')`n$($out.Substring([Math]::Max(0,$out.Length-12000)))`n$($err.Substring([Math]::Max(0,$err.Length-12000)))"; $out.Trim() } finally { $process.Dispose() }
}
function Git([string]$Root,[string[]]$Arguments) { Command (@('git','-c',('safe.directory='+$Root.Replace('\','/')),'-C',$Root)+$Arguments) $Root }
function SourceIdentity([string]$Root,[string]$Revision) {
    Require ([IO.Path]::IsPathFullyQualified($Root) -and [IO.Directory]::Exists($Root)) 'Source root must be absolute and present.'
    RejectRedirects $Root
    Require ((Git $Root @('rev-parse','HEAD')) -ceq $Revision) 'Source revision differs.'
    Require (-not (Git $Root @('status','--porcelain','--untracked-files=normal'))) 'Source checkout is not clean.'
    @{revision=$Revision; tree=(Git $Root @('rev-parse','HEAD^{tree}')); clean=$true; manifest_sha256=(FileDigest (Join-Path $Root 'Cargo.toml')); lock_sha256=(FileDigest (Join-Path $Root 'Cargo.lock')); toolchain_sha256=(FileDigest (Join-Path $Root 'rust-toolchain.toml'))}
}
function TestSummary([string]$Text,[string[]]$Names) {
    $summaries=[regex]::Matches($Text,'(?m)^test result:.*$'); Require ($summaries.Count -eq 1) 'Require exactly one complete libtest result summary.'
    $summary=$summaries[0].Value.TrimEnd("`r"); $match=[regex]::Match($summary,'^test result: ok\. ([0-9]+) passed; 0 failed; 0 ignored; 0 measured; [0-9]+ filtered out; finished in [0-9]+(?:\.[0-9]+)?s$')
    Require ($match.Success -and [long]$match.Groups[1].Value -eq $Names.Count -and $Names.Count -gt 0) 'Libtest summary did not execute the exact positive control inventory.'
    $executed=@([regex]::Matches($Text,'(?m)^test ([^\s]+) \.\.\. ok\r?$') | ForEach-Object { $_.Groups[1].Value })
    Require (Equal @($executed | Sort-Object) @($Names | Sort-Object)) 'Libtest named controls differ from the required inventory.'
    @{passed=$Names.Count; failed=0; ignored=0; measured=0; names=@($executed | Sort-Object); summary=$summary}
}
# Cargo.lock's package entries are deliberately parsed as a small, fail-closed
# subset. Cargo itself owns full TOML validation and dependency resolution.
function LockPackages([string]$Path) {
    $entries=[Collections.Generic.List[object]]::new(); $entry=$null
    foreach ($line in [IO.File]::ReadLines($Path)) {
        if ($line -ceq '[[package]]') { $entry=@{}; $entries.Add($entry) }
        elseif ($null -ne $entry -and $line -cmatch '^(name|version|source|checksum) = ("(?:[^"\\]|\\.)*")$') { $entry[$Matches[1]]=ConvertFrom-Json $Matches[2] }
        elseif ($null -ne $entry -and $line -cmatch '^(name|version|source|checksum)\s*=') { throw 'Unsupported lock identity syntax.' }
    }
    foreach ($package in $entries) { Require ($package.ContainsKey('name') -and $package.ContainsKey('version')) 'Incomplete lock package.' }
    return ,$entries.ToArray()
}
function LockSubset([string]$DriverLock,[string]$SourceLock,[bool]$Registry) {
    $inventory=@{}; foreach ($item in (LockPackages $SourceLock)) { $inventory[(@($item.name,$item.version,(Get-Field $item 'source')) -join '|')]=Get-Field $item 'checksum' }
    foreach ($item in (LockPackages $DriverLock)) {
        if ($item.name -ceq 'prism-cpu-bundle-driver') { continue }
        $source=Get-Field $item 'source'; $checksum=Get-Field $item 'checksum'
        if ($Registry -and $script:RegistryPackages.ContainsKey($item.name)) {
            $pin=$script:RegistryPackages[$item.name]; Require ($item.version -ceq $pin[0] -and $checksum -ceq $pin[1] -and $source -ceq 'registry+https://github.com/rust-lang/crates.io-index') 'Registry dependency differs from the approved package bytes.'
            if ($inventory.ContainsKey((@($item.name,$item.version,'') -join '|'))) { continue }
        }
        $key=@($item.name,$item.version,$source) -join '|'; Require ($inventory.ContainsKey($key) -and (Equal $inventory[$key] $checksum)) 'External collector resolved a dependency outside the source lock.'
    }
}
function VerifyRegistryTree([string]$Root,[string]$ArchiveSha256) {
    $directory=[IO.DirectoryInfo]::new($Root); $archive=Join-Path $directory.Parent.Parent.Parent.FullName ('cache/'+$directory.Parent.Name+'/'+$directory.Name+'.crate')
    Require ((FileDigest $archive) -ceq (Hex $ArchiveSha256 64 'registry archive digest')) 'Cargo cache archive differs from the approved publication.'
    $file=[IO.File]::OpenRead($archive); $gzip=[IO.Compression.GZipStream]::new($file,[IO.Compression.CompressionMode]::Decompress); $tar=[System.Formats.Tar.TarReader]::new($gzip)
    $seen=[Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal); $total=0L
    try {
        while ($null -ne ($entry=$tar.GetNextEntry($false))) {
            if ($entry.EntryType -eq [System.Formats.Tar.TarEntryType]::Directory) { continue }
            Require ($entry.EntryType -in [System.Formats.Tar.TarEntryType]::RegularFile,[System.Formats.Tar.TarEntryType]::V7RegularFile) 'Registry source archive contains a non-regular member.'
            $parts=$entry.Name.Split('/'); Require ($parts.Count -ge 2 -and $parts[0] -ceq $directory.Name -and '..' -cnotin $parts -and $entry.Name -notmatch '[:\\]') 'Registry archive member has an invalid source path.'
            $relative=($parts[1..($parts.Count-1)] -join '/'); Require ($seen.Add($relative) -and $seen.Count -le 20000) 'Duplicate or excessive registry source members.'
            $total+=$entry.Length; Require ($entry.Length -ge 0 -and $entry.Length -le 67108864 -and $total -le 268435456) 'Registry source archive exceeds the preparation bound.'
            Require ($null -ne $entry.DataStream) 'Missing regular registry source bytes.'
            $expected=[Convert]::ToHexStringLower([Security.Cryptography.SHA256]::HashData($entry.DataStream)); $path=Member $Root $relative
            $item=Get-Item -LiteralPath $path -Force; Require (-not ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -and $item.Length -eq $entry.Length -and (FileDigest $path) -ceq $expected) 'Cargo-extracted registry source bytes differ from the approved archive.'
        }
    } finally { $tar.Dispose(); $gzip.Dispose(); $file.Dispose() }
    Require ($seen.Count -gt 0) 'Empty registry source archive.'
    $actual=[Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    foreach ($item in Get-ChildItem -LiteralPath $Root -Recurse -Force) { Require (-not ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) 'Unexpected link in Cargo-extracted registry source.'; $relative=[IO.Path]::GetRelativePath($Root,$item.FullName).Replace('\','/'); if (-not $item.PSIsContainer -and $relative -cne '.cargo-ok') { [void]$actual.Add($relative) } }
    Require ($actual.SetEquals($seen)) 'Cargo-extracted registry tree contains missing or unexpected source files.'
}
function RegistrySource([string]$OutputDirectory) {
    $lane=Join-Path ([IO.Path]::GetTempPath()) ('cpu-registry-'+[guid]::NewGuid().ToString('N')); [void][IO.Directory]::CreateDirectory($lane)
    try {
        [IO.File]::WriteAllText((Join-Path $lane 'lib.rs'),"// Registry identity resolution only.`n")
        [IO.File]::WriteAllText((Join-Path $lane 'Cargo.toml'),@'
[workspace]
[package]
name="prism-cpu-registry-resolver"
version="0.0.0"
edition="2024"
[lib]
path="lib.rs"
[dependencies]
prism-inference={version="=0.1.25",default-features=false,features=["cli","experimental-neon","parallel-rows"]}
bonsai-neon-dot="=0.1.2"
bonsai-native-hash="=0.1.3"
'@)
        $metadata=ConvertFrom-Json -AsHashtable (Command @('cargo',("+$script:Channel"),'metadata','--manifest-path',(Join-Path $lane 'Cargo.toml'),'--format-version','1') $OutputDirectory)
        $locked=LockPackages (Join-Path $lane 'Cargo.lock'); $packages=@{}
        foreach ($name in $script:RegistryPackages.Keys) {
            $pin=$script:RegistryPackages[$name]; $matches=@($locked | Where-Object { $_.name -ceq $name -and $_.version -ceq $pin[0] })
            Require ($matches.Count -eq 1 -and $matches[0].checksum -ceq $pin[1] -and $matches[0].source -ceq 'registry+https://github.com/rust-lang/crates.io-index') 'Cargo registry resolution differs from approved bytes.'
            $selected=@($metadata.packages | Where-Object { $_.name -ceq $name -and $_.version -ceq $pin[0] -and $_.source -ceq $matches[0].source }); Require ($selected.Count -eq 1) 'Missing unambiguous Cargo-verified registry package.'
            $root=[IO.Path]::GetDirectoryName($selected[0].manifest_path); VerifyRegistryTree $root $pin[1]
            $vcs=(ReadJson (Join-Path $root '.cargo_vcs_info.json')).git.sha1; Require ((Hex $vcs 40 'registry package VCS identity') -ceq $script:RegistryRevision) 'Registry package VCS source differs.'
            $packages[$name]=@{name=$name; version=$pin[0]; sha256=$pin[1]; revision=$vcs; source=$matches[0].source; manifest_sha256=(FileDigest (Join-Path $root 'Cargo.toml')); root=$root}
        }
        $runtime=$packages['prism-inference']; $identity=@{kind='registry'; revision=$runtime.revision; clean=$true; clean_scope='Cargo-verified immutable registry archive; not a Git checkout'; version=$runtime.version; archive_sha256=$runtime.sha256; manifest_sha256=$runtime.manifest_sha256; lock_sha256=(FileDigest (Join-Path $runtime.root 'Cargo.lock'))}
        @{root=$runtime.root; identity=$identity; packages=$packages}
    } finally { [IO.Directory]::Delete($lane,$true) }
}
function ManifestInfo([string]$Root,[string]$Directory,[hashtable]$Environment) {
    $manifest=Join-Path $Root 'Cargo.toml'
    $metadata=ConvertFrom-Json -AsHashtable (Command @('cargo',("+$script:Channel"),'metadata','--manifest-path',$manifest,'--format-version','1','--no-deps','--offline','--locked') $Directory $Environment)
    $selected=@($metadata.packages | Where-Object { [IO.Path]::GetFullPath($_.manifest_path) -ceq [IO.Path]::GetFullPath($manifest) }); Require ($selected.Count -eq 1) 'Missing unambiguous source manifest.'; $selected[0]
}
function Prepare([string]$BaselineDirectory,[string]$ToolingDirectory,[string]$RegistryVersion,[string]$RunnerCommit,[string]$OutputDirectory) {
    foreach ($entry in @{GITHUB_ACTIONS='true'; GITHUB_REF='refs/heads/main'; RUNNER_ENVIRONMENT='github-hosted'; RUNNER_ARCH='ARM64'}.GetEnumerator()) { Require ([Environment]::GetEnvironmentVariable($entry.Key) -ceq $entry.Value) 'Preparation requires owning hosted ARM64 main CI.' }
    Require ($IsWindows -and [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture -eq 'Arm64' -and $env:GITHUB_REPOSITORY -cin @('symbiogenesis/prism-inference','symbiogenesis/SwiftBonsai2Runner')) 'Preparation requires native owning Windows ARM64 CI.'
    $revision=Hex $env:GITHUB_SHA 40 'orchestrator revision'; Require ($revision -ceq $RunnerCommit -and $env:GITHUB_RUN_ID -cmatch '^[1-9][0-9]*$') 'Missing or changed hosted source/run identity.'
    $orchestrator=@{repository=$env:GITHUB_REPOSITORY; revision=$revision; run_id=$env:GITHUB_RUN_ID; run_attempt=$env:GITHUB_RUN_ATTEMPT}
    $runnerRoot=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
    $reviewedEvaluator=Join-Path $runnerRoot 'tools/cpu-bundle/cpu_bundle.ps1'
    Require ([IO.Path]::GetFullPath($PSCommandPath) -ceq [IO.Path]::GetFullPath($reviewedEvaluator)) 'Preparation must execute the tracked Runner evaluator.'
    $runnerIdentity=SourceIdentity $runnerRoot $RunnerCommit
    $evaluatorDigest=FileDigest $reviewedEvaluator
    $orchestrator.source=$runnerIdentity
    Require ($RegistryVersion -ceq '0.1.25') 'Only the reviewed exact registry candidate is admitted.'
    Require ([IO.Path]::IsPathFullyQualified($BaselineDirectory) -and [IO.Path]::IsPathFullyQualified($ToolingDirectory) -and $BaselineDirectory -cne $ToolingDirectory) 'Use independent absolute source checkouts.'
    RejectRedirects $BaselineDirectory; RejectRedirects $ToolingDirectory; RejectRedirects $OutputDirectory
    $pathComparison=if ($IsWindows) { [StringComparison]::OrdinalIgnoreCase } else { [StringComparison]::Ordinal }
    Require (-not [string]::Equals([IO.Path]::GetFullPath($BaselineDirectory).TrimEnd('\','/'),[IO.Path]::GetFullPath($ToolingDirectory).TrimEnd('\','/'),$pathComparison)) 'Use independent source checkouts.'
    Require ([IO.Path]::IsPathFullyQualified($OutputDirectory) -and -not (Test-Path -LiteralPath $OutputDirectory)) 'Use a new external artifact directory.'
    foreach ($source in @($BaselineDirectory,$ToolingDirectory)) { $prefix=[IO.Path]::GetFullPath($source).TrimEnd('\','/')+[IO.Path]::DirectorySeparatorChar; Require (-not ([IO.Path]::GetFullPath($OutputDirectory).StartsWith($prefix,[StringComparison]::OrdinalIgnoreCase))) 'Artifact directory must be outside both source trees.' }
    $toolingIdentity=SourceIdentity $ToolingDirectory (Git $ToolingDirectory @('rev-parse','HEAD'))
    $baselineIdentity=SourceIdentity $BaselineDirectory $script:Baseline
    $driverFiles=@('examples/cpu_bundle_check.rs') + @([IO.Directory]::EnumerateFiles((Join-Path $ToolingDirectory 'examples/cpu_bundle_check'),'*.rs',[IO.SearchOption]::AllDirectories) | ForEach-Object { [IO.Path]::GetRelativePath($ToolingDirectory,$_).Replace('\','/') } | Sort-Object)
    $hashes=@{}; foreach ($name in $driverFiles) { $hashes[$name]=FileDigest (Join-Path $ToolingDirectory $name) }; Require ($hashes.Count -ge 2) 'Missing collector source or focused controls.'
    [void][IO.Directory]::CreateDirectory($OutputDirectory); $registry=RegistrySource $OutputDirectory
    $roots=@{baseline=$BaselineDirectory; candidate=$registry.root}; $identities=@{baseline=$baselineIdentity; candidate=$registry.identity}; $packages=$registry.packages
    $messageBytes=[Text.UTF8Encoding]::new($false).GetBytes('[{"role":"user","content":"Count from 1 to 10. Answer with only the numbers."}]')
    [IO.File]::WriteAllBytes((Join-Path $OutputDirectory 'messages.json'),$messageBytes)
    $compiler=Command @('rustc',("+$script:Channel"),'-vV') $OutputDirectory; Require ($compiler.Contains("host: $script:Target")) 'Compiler is not native Windows ARM64.'
    $arms=@{}
    foreach ($role in @('baseline','candidate')) {
        $root=$roots[$role]; $legacy=$role -ceq 'baseline'; $isRegistry=-not $legacy
        $flags='--check-cfg=cfg(cpu_bundle_legacy_f16)'+$(if ($legacy) {' --cfg cpu_bundle_legacy_f16'} else {''})
        $environment=@{RUSTFLAGS=$flags; CARGO_ENCODED_RUSTFLAGS=$null}
        $toolchain=Join-Path $root 'rust-toolchain.toml'
        if (Test-Path -LiteralPath $toolchain) { Require ([IO.File]::ReadAllText($toolchain) -cmatch ('(?m)^channel\s*=\s*"'+[regex]::Escape($script:Channel)+'"\s*$')) 'Unexpected pinned compiler.' }
        $hashRoot=if ($isRegistry) { $packages['bonsai-native-hash'].root } else { Join-Path $root 'tools/native-hash' }
        $checks=[Collections.Generic.List[object]]::new(); $lane=Join-Path ([IO.Path]::GetTempPath()) ('cpu-bundle-'+$role+'-'+[guid]::NewGuid().ToString('N')); [void][IO.Directory]::CreateDirectory($lane)
        try {
            $environment.CARGO_TARGET_DIR=Join-Path $lane 'build'; $driverManifest=Join-Path $lane 'Cargo.toml'
            $cargo=@('cargo',("+$script:Channel")); $rootManifest=Join-Path $root 'Cargo.toml'
            [void](Command ($cargo+@('fetch','--locked','--manifest-path',$rootManifest)) $lane $environment)
            $manifest=ManifestInfo $root $lane $environment; $hashManifest=ManifestInfo $hashRoot $lane $environment
            $serde=@($manifest.dependencies | Where-Object { $_.name -ceq 'serde_json' }); Require ($serde.Count -eq 1) 'Missing exact JSON dependency.'
            $serdeVersion='='+$serde[0].req.TrimStart('=','^','~')
            $runtimeDependency=if ($isRegistry) {'version="=0.1.25",'} else {'path='+(ConvertTo-Json -InputObject $root.Replace('\','/') -Compress)+',version="='+$manifest.version+'",'}
            $hashDependency=if ($isRegistry) {'version="=0.1.3"'} else {'path='+(ConvertTo-Json -InputObject $hashRoot.Replace('\','/') -Compress)+',version="='+$hashManifest.version+'"'}
            $driverPath=ConvertTo-Json -InputObject ((Join-Path $ToolingDirectory $driverFiles[0]).Replace('\','/')) -Compress
            $source="[workspace]`nresolver=`"3`"`n[package]`nname=`"prism-cpu-bundle-driver`"`nversion=`"0.0.0`"`nedition=`"2024`"`n[[bin]]`nname=`"cpu-bundle-check`"`npath=$driverPath`n[dependencies]`nbonsai-prism-runtime={package=`"prism-inference`",$runtimeDependency default-features=false,features=[`"cli`",`"experimental-neon`",`"parallel-rows`"]}`nbonsai-native-hash={$hashDependency}`nserde_json=`"$serdeVersion`"`n"
            [IO.File]::WriteAllText($driverManifest,$source,[Text.UTF8Encoding]::new($false))
            [IO.File]::Copy((Join-Path $root 'Cargo.lock'),(Join-Path $lane 'Cargo.lock'))
            [void](Command ($cargo+@('metadata','--offline','--format-version','1','--manifest-path',$driverManifest)) $lane $environment)
            LockSubset (Join-Path $lane 'Cargo.lock') (Join-Path $root 'Cargo.lock') $isRegistry
            $commands=@(
                ($cargo+@('clippy','--manifest-path',$rootManifest,'--locked','--offline','--release','-p','prism-inference','--all-targets','--features','prism-inference/parallel-rows','--','-D','warnings')),
                ($cargo+@('check','--manifest-path',$rootManifest,'--locked','--offline','--release','-p','prism-inference','--lib','--no-default-features')),
                ($cargo+@('test','--manifest-path',$rootManifest,'--locked','--offline','--release','-p','prism-inference','--features','parallel-rows','--test','graph',$script:StateControl,'--','--exact','--color','never')),
                ($cargo+@('clippy','--manifest-path',$driverManifest,'--locked','--offline','--release','--all-targets','--','-D','warnings')),
                ($cargo+@('test','--manifest-path',$driverManifest,'--locked','--offline','--release','--bin','cpu-bundle-check','--','--color','never')),
                ($cargo+@('build','--manifest-path',$driverManifest,'--locked','--offline','--release','--bin','cpu-bundle-check')))
            for ($i=0;$i -lt $script:Phases.Count;$i++) {
                $phase=$script:Phases[$i]; $text=Command $commands[$i] $lane $environment; $check=@{phase=$phase; command=$commands[$i]; exit_code=0}
                if ($script:Inventories.ContainsKey($phase)) { $check.test_summary=TestSummary $text $script:Inventories[$phase] }
                if ($phase -ceq 'source_analysis') {
                    $check.helper_checks=@()
                    foreach ($package in @('bonsai-neon-dot','bonsai-native-hash')) {
                        $helperRoot=if ($isRegistry) { $packages[$package].root } else { Join-Path $root $(if ($package -ceq 'bonsai-neon-dot') {'tools/neon-dot'} else {'tools/native-hash'}) }
                        $helperManifest=Join-Path $helperRoot 'Cargo.toml'; $helperLock=if ($isRegistry) { Join-Path $helperRoot 'Cargo.lock' } else { Join-Path $root 'Cargo.lock' }
                        [void](Command ($cargo+@('fetch','--locked','--manifest-path',$helperManifest)) $lane $environment)
                        $arguments=$cargo+@('clippy','--manifest-path',$helperManifest,'--locked','--offline','--release','--all-targets','--','-D','warnings'); [void](Command $arguments $lane $environment)
                        $check.helper_checks+=@{package=$package; command=$arguments; exit_code=0; manifest_sha256=(FileDigest $helperManifest); lock_sha256=(FileDigest $helperLock)}
                    }
                }
                $checks.Add($check)
            }
            $executable=Join-Path $OutputDirectory ($role+'.exe'); [IO.File]::Copy((Join-Path $lane 'build/release/cpu-bundle-check.exe'),$executable)
            $arms[$role]=@{source=$identities[$role]; legacy_f16_build=$legacy; executable=@{path=($role+'.exe'); bytes=([IO.FileInfo]::new($executable).Length); sha256=(FileDigest $executable)}; driver_manifest_sha256=(FileDigest $driverManifest); driver_lock_sha256=(FileDigest (Join-Path $lane 'Cargo.lock')); rustflags=$flags; checks=$checks.ToArray()}
        } finally { [IO.Directory]::Delete($lane,$true) }
        if ($legacy) { Require (Equal (SourceIdentity $root $script:Baseline) $baselineIdentity) 'Baseline source changed during preparation.' }
        else { foreach ($package in $packages.Values) { VerifyRegistryTree $package.root $package.sha256 } }
    }
    Require (Equal (SourceIdentity $ToolingDirectory $toolingIdentity.revision) $toolingIdentity) 'Collector/tooling checkout changed during preparation.'
    foreach ($name in $hashes.Keys) { Require ((FileDigest (Join-Path $ToolingDirectory $name)) -ceq $hashes[$name]) 'Collector sources changed during preparation.' }
    Require ((Equal (SourceIdentity $runnerRoot $RunnerCommit) $runnerIdentity) -and (FileDigest $reviewedEvaluator) -ceq $evaluatorDigest) 'Runner evaluator source changed during preparation.'
    # Bind this new native evaluator to Runner, independently of immutable Prism collector sources.
    $evaluator=Join-Path $OutputDirectory 'evaluator.ps1'; [IO.File]::Copy($reviewedEvaluator,$evaluator)
    Require ((FileDigest $evaluator) -ceq $evaluatorDigest) 'Retained evaluator differs from reviewed Runner source.'
    $registryIdentities=@{}; foreach ($name in $packages.Keys) { $identity=$packages[$name].Clone(); $identity.Remove('root'); $registryIdentities[$name]=$identity }
    $receipt=@{schema_version=2; kind='prism-cpu-bundle-preparation'; compiler=$compiler; orchestrator=$orchestrator; evaluator_sha256=(FileDigest $evaluator); evaluator_language='PowerShell 7.6'; tooling_source=$toolingIdentity; registry_packages=$registryIdentities
        retained_files=@('preparation.json','messages.json','evaluator.ps1'); target=$script:Target; features=$script:Features; collector_sources=$hashes
        messages=@{path='messages.json';sha256=(Digest $messageBytes)}; output_tokens=32; model_sha256=$script:Model; model_bytes=7206168928L; adapter_sha256=$script:Adapter; adapter_bytes=9682464
        per_arm_timeout_seconds=60; schedule=$script:Schedule; profile=$script:Profile; arms=$arms; minimum_relative_request_improvement=0.10; maximum_owned_arm_regression=0.10; model_executed=$false; promotion_qualified=$false}
    WriteNew (Join-Path $OutputDirectory 'preparation.json') $receipt; $receipt
}
function Power($Value) {
    Require ($Value -is [Collections.IDictionary] -and (Get-Field $Value 'status') -ceq 'observed' -and (Get-Field $Value 'source') -ceq 'GetSystemPowerStatus') 'Unknown power source.'
    $ac=Get-Field $Value 'ac_online'; $saver=Get-Field $Value 'battery_saver'; Require ($ac -is [bool] -and $saver -is [bool]) 'Missing known AC/saver observations.'
    $charge=Get-Field $Value 'battery_percent'; Require ($null -eq $charge -or (($charge -is [int] -or $charge -is [long] -or $charge -is [double]) -and [double]::IsFinite($charge) -and $charge -ge 0 -and $charge -le 100)) 'Invalid battery charge observation.'
    return ,@($ac,$saver)
}
function NativeConditions($Report,$ExpectedHost) {
    Require ($Report -is [Collections.IDictionary] -and (Get-Field $Report 'schema_version') -eq 1 -and (Get-Field $Report 'kind') -ceq 'runner-native-cpu-observations' -and (Get-Field $Report 'status') -ceq 'observed' -and (Get-Field $Report 'architecture') -ceq 'aarch64' -and (Get-Field $Report 'scope') -ceq 'current-process' -and (Get-Field $Report 'source') -ceq 'GetSystemCpuSetInformation') 'Unknown native CPU conditions.'
    [void](Integer (Get-Field $Report 'schema_version') 1 1 'native observation schema')
    $hostValue=Get-Field $Report 'host_identity'
    Require ((Equal $hostValue $ExpectedHost) -and $hostValue.status -ceq 'observed' -and $hostValue.source -ceq 'HKLM/HARDWARE/DESCRIPTION/System/CentralProcessor/0;RtlGetVersion;IsProcessorFeaturePresent') 'Unknown or changed native host identity.'
    foreach ($field in @('processor_name','processor_identifier','vendor_identifier')) { $value=Get-Field $hostValue $field; Require ($value -is [string] -and $value.Trim() -and $value.Trim() -ine 'unknown') 'Incomplete native host identity.' }
    Require ($hostValue.processor_name.Contains('Snapdragon') -and $hostValue.processor_name.Contains('X2')) 'This fixed comparison requires the selected X2 family.'
    $version=Get-Field $hostValue 'windows_version'; Require ($version -is [array] -and $version.Count -eq 3) 'Missing native kernel version.'
    foreach ($value in $version) { [void](Integer $value 0 4294967295 'native kernel version') }; Require ($version[0] -gt 0 -and $version[2] -gt 0) 'Missing native kernel version.'
    $features=Get-Field $hostValue 'processor_features'; Require ($features -is [array] -and $features.Count -le 256) 'Invalid native feature observations.'
    foreach ($value in $features) { [void](Integer $value 0 255 'native feature') }; Require (Equal $features @($features | Sort-Object -Unique)) 'Invalid native feature observations.'
    $topology=Get-Field $Report 'topology'; Require ($topology -is [Collections.IDictionary] -and (Get-Field $topology 'cpu_sets')) 'Missing CPU topology.'; [void](Integer (Get-Field $topology 'logical_processors') 4 4096 'native logical processor count')
    $policy=Get-Field $Report 'power_policy_snapshot'
    Require ($policy -is [Collections.IDictionary] -and (Get-Field $policy 'status') -ceq 'observed' -and (Get-Field $policy 'source') -ceq 'PowerGetActiveScheme/PowerRegisterForEffectivePowerModeNotifications' -and (Get-Field $policy 'notification_version') -eq 2) 'Unknown effective power policy.'
    [void](Integer (Get-Field $policy 'notification_version') 2 2 'power notification version')
    $scheme=Get-Field $policy 'active_scheme'; Require ($scheme -is [string] -and $scheme -cmatch '^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$' -and $scheme -cne '00000000-0000-0000-0000-000000000000') 'Invalid power scheme.'
    Require ($policy.effective_mode -cin @('battery-saver','better-battery','balanced','high-performance','max-performance','game-mode','mixed-reality')) 'Unknown effective power mode.'
    @{host=$hostValue; topology=$topology; power=(Power (Get-Field $Report 'power_snapshot')); scheme=$scheme; mode=$policy.effective_mode}
}
function ValidateArm($Receipt,$OwnerProof,$Packet,[string]$Role,[long]$SourceDeadline) {
    $expected=$script:Profile.Clone()
    $extra=@{schema_version=1; kind='swift-cpu-bundle-arm'; source_commit_declaration=$Packet.arms[$Role].source.revision; executable_sha256=$Packet.arms[$Role].executable.sha256; input_sha256=$Packet.messages.sha256
        legacy_f16_build=($Role -ceq 'baseline'); output_tokens_reserved=$Packet.output_tokens; original_deadline_unix_milliseconds=$SourceDeadline; capture_status='complete'
        arm_succeeded=$true; session_succeeded=$true; terminal_delivered=$true; stage='complete'; terminal_accounting_matches=$true; reused_tokens=0; model_sha256=$OwnerProof.model_sha256; adapter_sha256=$OwnerProof.adapter_sha256
        model_bytes=7206168928L; weight_representation='PQ2_0'; effective_row_workers=4; observed_adapter_sha256=$OwnerProof.adapter_sha256; model_loaded=$true; output_committed=$true; receipt_reserve_milliseconds=2000; sampler='production SwiftSampler and model-declared stops'}
    foreach ($key in $extra.Keys) { $expected[$key]=$extra[$key] }
    foreach ($key in $expected.Keys) {
        Require ($Receipt.Contains($key) -and (Equal $Receipt[$key] $expected[$key])) "Collector identity/settings/completion differ: $key."
        if ($expected[$key] -is [int] -or $expected[$key] -is [long]) { [void](Integer $Receipt[$key] ([decimal]::MinValue) ([decimal]::MaxValue) $key) }
    }
    [void](Integer (Get-Field $Receipt 'prompt_tokens') 1 128 'prompt count'); [void](Integer (Get-Field $Receipt 'generated_tokens') 1 $Packet.output_tokens 'output count')
    Require ((Get-Field $Receipt 'finish') -cin @('stop','length')) 'Missing terminal reason.'
    $text=Get-Field $Receipt 'output_text'; Require ($text -is [string] -and $text.Trim()) 'Missing complete returned text.'; $encoded=[Text.Encoding]::UTF8.GetBytes($text)
    [void](Integer (Get-Field $Receipt 'output_bytes') 1 65536 'returned output bytes'); Require ($Receipt.output_bytes -eq $encoded.Length -and $Receipt.output_sha256 -ceq (Digest $encoded)) 'Returned text digest or byte count differs.'
    $timings=@(foreach ($key in @('load_milliseconds','preparation_milliseconds','request_milliseconds','elapsed_milliseconds')) { Positive (Get-Field $Receipt $key) $key }); Require ($timings[0]+$timings[1]+$timings[2] -le $timings[3]) 'Operation durations exceed their enclosing clock.'
}
function ValidatePreparation($Packet,[string]$Path) {
    [void](Integer $Packet.schema_version 2 2 'packet schema'); [void](Integer $Packet.per_arm_timeout_seconds 60 60 'per-arm timeout')
    Require ($Packet.schema_version -eq 2 -and $Packet.kind -ceq 'prism-cpu-bundle-preparation' -and $Packet.target -ceq $script:Target -and (Equal $Packet.features $script:Features) -and (Equal $Packet.schedule $script:Schedule) -and (Equal $Packet.profile $script:Profile) -and
        $Packet.per_arm_timeout_seconds -eq 60 -and $Packet.model_sha256 -ceq $script:Model -and $Packet.adapter_sha256 -ceq $script:Adapter -and $Packet.model_executed -is [bool] -and $Packet.model_executed -eq $false -and $Packet.promotion_qualified -is [bool] -and $Packet.promotion_qualified -eq $false) 'Preparation identity or fixed policy differs.'
    Require (Equal $Packet.retained_files @('preparation.json','messages.json','evaluator.ps1')) 'Evidence selection differs from declared replay inputs.'
    $directory=[IO.Path]::GetDirectoryName($Path); $messagesPath=Member $directory $Packet.messages.path
    Require ((FileDigest $messagesPath) -ceq $Packet.messages.sha256 -and (Equal (ConvertFrom-Json -InputObject ([IO.File]::ReadAllText($messagesPath)) -AsHashtable -NoEnumerate) $script:Messages)) 'Prepared public workload changed.'
    Require ((FileDigest (Join-Path $directory 'evaluator.ps1')) -ceq $Packet.evaluator_sha256 -and $Packet.evaluator_language -ceq 'PowerShell 7.6') 'Prepared evaluator bytes changed.'
    Require ($Packet.collector_sources -is [Collections.IDictionary] -and $Packet.collector_sources.Count -ge 2) 'Missing collector source identities.'
    foreach ($name in $Packet.collector_sources.Keys) { Require ($name -cmatch '^examples/cpu_bundle_check(?:\.rs|/[^:]+\.rs)$') 'Unreviewed collector source member.'; [void](Hex $Packet.collector_sources[$name] 64 'collector source digest') }
    Require (Equal @($Packet.arms.Keys | Sort-Object) @('baseline','candidate')) 'Missing prepared arms.'; Require ($Packet.arms.baseline.source.revision -ceq $script:Baseline) 'Unreviewed baseline source.'
    foreach ($role in @('baseline','candidate')) {
        $prepared=$Packet.arms[$role]; [void](Hex $prepared.source.revision 40 'source revision'); [void](Hex $prepared.executable.sha256 64 'executable digest')
        Require ((Equal $prepared.source.clean $true) -and (Equal $prepared.legacy_f16_build ($role -ceq 'baseline')) -and $prepared.checks -is [array] -and (Equal @($prepared.checks | ForEach-Object { $_.phase }) $script:Phases)) 'Missing successful source-bound preparation checks.'
        foreach ($check in $prepared.checks) {
            [void](Integer $check.exit_code 0 0 'check exit'); Require ($check.command -is [array] -and $check.command.Count -gt 0) 'Missing check command.'
            if ($script:Inventories.ContainsKey($check.phase)) {
                $names=$script:Inventories[$check.phase]; $summary=Get-Field $check 'test_summary'; Require ($null -ne $summary) 'Missing executed controls.'
                foreach ($field in @('failed','ignored','measured')) { [void](Integer (Get-Field $summary $field) 0 0 $field) }; [void](Integer $summary.passed $names.Count $names.Count 'executed count'); Require (Equal $summary.names @($names | Sort-Object)) 'Missing exact executed control inventory.'
            }
            if ($check.phase -ceq 'source_analysis') {
                $helpers=Get-Field $check 'helper_checks'; Require ($helpers -is [array] -and (Equal @($helpers | ForEach-Object {$_.package}) @('bonsai-neon-dot','bonsai-native-hash'))) 'Missing explicit helper-manifest strict analysis.'
                foreach ($helper in $helpers) { [void](Integer $helper.exit_code 0 0 'helper exit'); Require ($helper.command) 'Missing helper command.'; [void](Hex $helper.manifest_sha256 64 'helper manifest digest'); [void](Hex $helper.lock_sha256 64 'helper lock digest') }
            }
        }
    }
    $candidate=$Packet.arms.candidate.source; Require ($candidate.revision -cne $script:Baseline) 'Candidate is the baseline source.'
    # Runner's native lane only admits the reviewed registry candidate.
    Require ($candidate.kind -ceq 'registry' -and $candidate.revision -ceq $script:RegistryRevision -and $candidate.version -ceq '0.1.25' -and $candidate.archive_sha256 -ceq $script:RegistryPackages['prism-inference'][1]) 'Registry candidate differs from reviewed publication.'
    Require ($Packet.registry_packages -is [Collections.IDictionary] -and (Equal @($Packet.registry_packages.Keys | Sort-Object) @($script:RegistryPackages.Keys | Sort-Object))) 'Missing exact registry dependency identities.'
    foreach ($name in $script:RegistryPackages.Keys) { $package=$Packet.registry_packages[$name]; $pin=$script:RegistryPackages[$name]; Require ($package.name -ceq $name -and $package.version -ceq $pin[0] -and $package.sha256 -ceq $pin[1] -and $package.revision -ceq $script:RegistryRevision) 'Changed registry dependency bytes/source.' }
    [void](Integer $Packet.output_tokens 1 32 'prepared output cap'); [void](Hex $Packet.messages.sha256 64 'messages digest')
}
function Evaluate([string]$PreparationPath,[string]$OwnerPath) {
    $packet=ReadJson $PreparationPath; $proof=ReadJson $OwnerPath; ValidatePreparation $packet $PreparationPath
    [void](Integer $proof.schema_version 1 1 'owner schema')
    Require ($proof.schema_version -eq 1 -and $proof.kind -ceq 'prism-cpu-bundle-owner' -and $proof.preparation_sha256 -ceq (FileDigest $PreparationPath) -and (Equal $proof.workload_predeclared $true) -and (Equal $proof.completed $true)) 'Owner proof is missing, changed or incomplete.'
    $budget=Integer $proof.budget_seconds 1 900 'aggregate allowance'; $frequency=Integer $proof.clock_frequency 1 1000000000000 'clock frequency'; $started=Integer $proof.started_tick 0 ([long]::MaxValue) 'original start'; $deadline=Integer $proof.deadline_tick 1 ([long]::MaxValue) 'original deadline'; $retained=Integer $proof.retention_completed_tick 1 ([long]::MaxValue) 'retention completion'
    Require ([decimal]$deadline -eq [decimal]$started+[decimal]$budget*$frequency -and $started -lt $retained -and $retained -le $deadline) 'Work or retention escaped the original aggregate allowance.'
    $sourceDeadline=Integer $proof.original_deadline_unix_milliseconds 1 ([long]::MaxValue) 'collector original deadline'
    Require ($proof.model_sha256 -ceq $script:Model -and $proof.adapter_sha256 -ceq $script:Adapter -and $proof.host_identity -is [Collections.IDictionary] -and $proof.host_identity.Count) 'Owner artifact/native host identities differ.'
    $conditions=[Collections.Generic.List[object]]::new(); $conditions.Add((Power $proof.power_before)); $conditions.Add((Power $proof.power_after))
    Require ($proof.arms -is [array] -and $proof.arms.Count -eq 4) 'The complete fixed ABBA workload is required.'
    $receipts=[Collections.Generic.List[object]]::new(); $wallMs=[Collections.Generic.List[double]]::new(); $ownedMs=[Collections.Generic.List[double]]::new(); $snapshots=[Collections.Generic.List[object]]::new(); $observations=[Collections.Generic.List[object]]::new(); $previousEnd=$started; $seen=[Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    for ($i=0;$i -lt 4;$i++) {
        $arm=$proof.arms[$i]; $role=$script:Schedule[$i]; Require ($arm.id -ceq "arm-$($i+1)" -and $arm.role -ceq $role) 'Arm order differs from fixed ABBA schedule.'
        $begin=Integer $arm.started_tick 0 ([long]::MaxValue) 'arm start'; $end=Integer $arm.cleanup_completed_tick 1 ([long]::MaxValue) 'arm cleanup'; Require ($previousEnd -le $begin -and $begin -lt $end -and $end -le $retained) 'Arms overlap or cleanup is incomplete.'; $previousEnd=$end
        $executionDeadline=Integer $arm.execution_deadline_tick 1 ([long]::MaxValue) 'arm deadline'; $armDeadline=Integer $arm.collector_deadline_unix_milliseconds 1 ([long]::MaxValue) 'collector arm deadline'
        $derived=[decimal]$sourceDeadline-[decimal]::Floor(([decimal]$deadline-$executionDeadline)*1000/$frequency)
        Require ($begin -lt $executionDeadline -and $executionDeadline -le $deadline -and [decimal]$executionDeadline-$begin -le 60*[decimal]$frequency -and [Math]::Abs($derived-$armDeadline) -le 2) 'Arm deadline is not derived from original aggregate clock.'
        Require (Equal $arm.host_identity $proof.host_identity) 'Native host identity changed.'; $conditions.Add((Power $arm.power_before)); $conditions.Add((Power $arm.power_after))
        Require ((Equal $arm.owned_job_empty $true) -and (Equal $arm.cleanup_succeeded $true)) 'Owned model cleanup was not confirmed.'
        foreach ($name in @('receipt','started_receipt','supervision')) { $reference=Get-Field $arm $name; Require ($reference -is [Collections.IDictionary] -and $seen.Add($reference.path)) 'Missing or reused arm evidence path.' }
        $directory=[IO.Path]::GetDirectoryName($OwnerPath); $receipt=CheckedJson $directory $arm.receipt; $startReceipt=CheckedJson $directory $arm.started_receipt; $supervision=CheckedJson $directory $arm.supervision
        [void](Integer $startReceipt.original_deadline_unix_milliseconds $armDeadline $armDeadline 'started deadline')
        Require ($startReceipt.source_commit_declaration -ceq $receipt.source_commit_declaration -and $startReceipt.original_deadline_unix_milliseconds -eq $armDeadline) 'Started/final source deadline identity differs.'
        Require ($supervision.Status -ceq 'completed' -and (Equal $supervision.ModelProcessStarted $true) -and -not (Get-Field $supervision 'CancellationFailure')) 'Native supervision did not complete successfully.'; [void](Integer $supervision.ExitCode 0 0 'supervisor exit')
        $snapshots.Add((NativeConditions $supervision.CpuConditions.Before $proof.host_identity)); $snapshots.Add((NativeConditions $supervision.CpuConditions.After $proof.host_identity))
        $wall=Positive $supervision.WallMilliseconds 'supervised wall time'; Require ($wall -le ($executionDeadline-$begin)*1000.0/$frequency+1.0) 'Observed process wall exceeded declared arm deadline.'
        $minimum=Integer $supervision.MinimumAvailablePhysicalBytes 4294967296 ([long]::MaxValue) 'physical reserve'; $peak=Integer $supervision.PeakJobMemoryBytes 1 12348030976 'Job memory peak'
        ValidateArm $receipt $proof $packet $role $armDeadline
        $elapsed=($end-$begin)*1000.0/$frequency; Require ($receipt.elapsed_milliseconds -le $wall+1.0 -and $wall -le $elapsed+1.0) 'Collector, supervisor and cleanup durations are inconsistent.'
        $firstDelta=Get-Field $receipt 'first_text_delta_milliseconds'; if ($null -ne $firstDelta) { Require ((Positive $firstDelta 'first nonempty text callback') -le $receipt.request_milliseconds) 'First text callback lies outside request.' }
        $phases=Get-Field $receipt 'generation_phases'
        if ($null -ne $phases) {
            Require ($phases -is [array] -and $phases.Count -gt 0) 'Invalid phase telemetry.'; $names=[Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
            foreach ($phase in $phases) {
                Require ($phase -is [Collections.IDictionary] -and $phase.phase -is [string] -and $phase.phase -and $names.Add($phase.phase)) 'Invalid or repeated phase name.'
                $ms=$phase.milliseconds; Require (($ms -is [int] -or $ms -is [long] -or $ms -is [double]) -and [double]::IsFinite($ms) -and $ms -ge 0) 'Invalid phase time.'
                foreach ($field in @('attempted_calls','completed_calls','attempted_tokens','completed_tokens_lower_bound')) { [void](Integer $phase[$field] 0 ([long]::MaxValue) $field) }
                Require ($phase.completed_calls -le $phase.attempted_calls -and $phase.completed_tokens_lower_bound -le $phase.attempted_tokens -and $phase.partial_progress_unknown -is [bool]) 'Invalid phase accounting.'
            }
        }
        $observation=@{id=$arm.id; role=$role; source_revision=$receipt.source_commit_declaration; prompt_tokens=$receipt.prompt_tokens; generated_tokens=$receipt.generated_tokens; finish=$receipt.finish
            load_milliseconds=$receipt.load_milliseconds; preparation_milliseconds=$receipt.preparation_milliseconds; request_milliseconds=$receipt.request_milliseconds; process_wall_milliseconds=$wall; owned_arm_milliseconds=$elapsed
            source_request_tokens_per_second=$receipt.generated_tokens*1000.0/$receipt.request_milliseconds; source_request_tps_scope='committed output tokens divided by SwiftSession completion time; includes prefill/sampling, excludes loading/preparation; not post-first-token decode TPS'
            first_text_delta_milliseconds=$firstDelta; first_text_delta_scope='first nonempty text callback after completion entry; includes prefill/sampling; not a model token timestamp or broker HTTP TTFT'; generation_phases=$phases
            os_lifetime_peak_job_commit_bytes=$peak; minimum_available_physical_bytes=$minimum; process_memory=(Get-Field $supervision 'ProcessMemory'); memory_scope='Job commit, process counters and available RAM overlap; never sum them or claim total RAM; sampled counters may miss short peaks'}
        $observations.Add($observation); $receipts.Add($receipt); $wallMs.Add($wall); $ownedMs.Add($elapsed)
    }
    foreach ($condition in $conditions) { Require (Equal $condition $conditions[0]) 'Unknown or mismatched power conditions.' }
    foreach ($snapshot in $snapshots) { Require ((Equal $snapshot $snapshots[0]) -and (Equal $snapshot.power $conditions[0])) 'Native CPU or effective power conditions changed across arms.' }
    foreach ($receipt in $receipts) { foreach ($field in @('output_text','output_sha256','output_bytes','prompt_tokens','generated_tokens','finish')) { Require (Equal $receipt[$field] $receipts[0][$field]) 'Output, token counts or terminal reason differ across arms.' } }
    Require ($packet.minimum_relative_request_improvement -eq 0.10 -and $packet.maximum_owned_arm_regression -eq 0.10) 'Benefit thresholds differ.'
    $requestMs=@($receipts | ForEach-Object { $_.preparation_milliseconds+$_.request_milliseconds }); $ratios=@(($requestMs[1]/$requestMs[0]),($requestMs[2]/$requestMs[3])); $wallRatios=@(($wallMs[1]/$wallMs[0]),($wallMs[2]/$wallMs[3])); $ownedRatios=@(($ownedMs[1]/$ownedMs[0]),($ownedMs[2]/$ownedMs[3]))
    $benefit=$wallRatios[0] -le 0.90 -and $wallRatios[1] -le 0.90 -and $ownedRatios[0] -le 1.10 -and $ownedRatios[1] -le 1.10
    @{schema_version=2; kind='prism-cpu-bundle-comparison'; status='admitted'; short_request_benefit_observed=$benefit; primary_metric='supervised_load_inclusive_process_wall_milliseconds'
        candidate_over_baseline_process_wall_ratios=$wallRatios; candidate_over_baseline_request_ratios=$ratios; candidate_over_baseline_owned_arm_ratios=$ownedRatios; observations=$observations.ToArray()
        ratio_range=@([Math]::Min($wallRatios[0],$wallRatios[1]),[Math]::Max($wallRatios[0],$wallRatios[1])); process_wall_ratio_range=@([Math]::Min($wallRatios[0],$wallRatios[1]),[Math]::Max($wallRatios[0],$wallRatios[1])); request_ratio_range=@([Math]::Min($ratios[0],$ratios[1]),[Math]::Max($ratios[0],$ratios[1])); pairs=2
        complete_answer_observed=(@($receipts | Where-Object { $_.finish -cne 'stop' }).Count -eq 0); budget_seconds=$budget; aggregate_elapsed_seconds=($retained-$started)/[double]$frequency; power_mode=$(if ($conditions[0][0]) {'ac'} else {'battery'})
        continuous_power_stability_proven=$false; statistical_significance_qualified=$false; benefit_gate_qualified=$false; promotion_qualified=$false; DefaultMayChange=$false; individual_optimization_verdicts=$false; state_quality_sustained_acceptance_qualified=$false
        scope='Two matched reversed short-request pairs for the complete source bundle only; source/state/resource and uncertainty acceptance remain independently required.'}
}
if ($MyInvocation.InvocationName -ne '.') {
    try {
        $result=switch ($Operation) {
            prepare { Prepare $BaselineRoot $ToolingRoot $CandidateRegistryVersion $RunnerRevision $Output }
            evaluate { Evaluate $Preparation $Owner }
            default { throw 'Supply -Operation prepare or evaluate and its required inputs.' }
        }
        ConvertTo-Json -InputObject $result -Depth 70
    } catch { [Console]::Error.WriteLine((ConvertTo-Json -InputObject @{status='rejected';reason=$_.Exception.Message;promotion_qualified=$false} -Compress)); exit 2 }
}
